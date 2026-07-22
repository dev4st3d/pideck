use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use pi_gui::services::pi_process::discover_and_probe;
use pi_gui::services::sdk_bridge::{
    BridgeCommand, BridgeErrorKind, SdkBridgeClient, SdkBridgeConfig,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "pi-gui-sdk-bridge-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).expect("create temp directory");
    path
}

fn bridge_config(workspace: &Path) -> Option<SdkBridgeConfig> {
    let installation = discover_and_probe(None, Duration::from_secs(5)).ok()?;
    SdkBridgeConfig::from_installation(&installation, workspace.to_path_buf())
}

fn write_branched_session(path: &Path, cwd: &Path) {
    let cwd = serde_json::to_string(&cwd.to_string_lossy()).unwrap();
    fs::write(
        path,
        format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"bridge-test\",\"timestamp\":\"2026-07-22T00:00:00Z\",\"cwd\":{cwd}}}\n\
             {{\"type\":\"message\",\"id\":\"u1\",\"parentId\":null,\"timestamp\":\"2026-07-22T00:00:01Z\",\"message\":{{\"role\":\"user\",\"content\":\"first\",\"timestamp\":1}}}}\n\
             {{\"type\":\"message\",\"id\":\"u2\",\"parentId\":\"u1\",\"timestamp\":\"2026-07-22T00:00:02Z\",\"message\":{{\"role\":\"user\",\"content\":\"second\",\"timestamp\":2}}}}\n\
             {{\"type\":\"message\",\"id\":\"orphan\",\"parentId\":\"missing\",\"timestamp\":\"2026-07-22T00:00:03Z\",\"message\":{{\"role\":\"user\",\"content\":\"orphan\",\"timestamp\":3}}}}\n"
        ),
    )
    .expect("write session");
}

fn fake_bridge_config(root: &Path) -> Option<SdkBridgeConfig> {
    let real = bridge_config(root)?;
    let sdk_root = root.join("fake-sdk");
    fs::create_dir_all(sdk_root.join("dist")).expect("create fake SDK");
    fs::write(sdk_root.join("package.json"), "{\"type\":\"module\"}\n")
        .expect("write fake SDK package");
    fs::write(
        sdk_root.join("dist/index.js"),
        r#"
class FakeManager {
  static open() { return new FakeManager(); }
  appendLabelChange() { return "checkpoint"; }
  getLabel() { return undefined; }
  getLeafId() { return "target"; }
}

export const SessionManager = FakeManager;

export async function createAgentSession({ sessionManager }) {
  let finishNavigation;
  const session = {
    sessionManager,
    async navigateTree(targetId, options) {
      if (targetId === "cancel-target") {
        return await new Promise((resolve) => { finishNavigation = resolve; });
      }
      if (options.summarize) {
        if (targetId !== "summary-target" ||
            options.customInstructions !== "Preserve decisions" ||
            options.replaceInstructions !== true ||
            options.label !== "alternate") {
          throw new Error("Branch summary options were not forwarded");
        }
        return { cancelled: false, editorText: "", summaryEntry: { id: "summary-1" } };
      }
      throw new Error("Unexpected fake navigation");
    },
    abortBranchSummary() {
      finishNavigation?.({ cancelled: true, aborted: true });
    },
    dispose() {},
  };
  return { session };
}
"#,
    )
    .expect("write fake SDK module");
    Some(SdkBridgeConfig {
        node: real.node,
        sdk_root,
        script: real.script,
        working_directory: root.to_path_buf(),
    })
}

#[test]
fn bridge_negotiates_mutates_through_sdk_exports_imports_and_restarts() {
    let root = temp_dir();
    let workspace = root.join("workspace");
    let sessions = root.join("sessions");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&sessions).unwrap();
    let Some(config) = bridge_config(&workspace) else {
        let _ = fs::remove_dir_all(root);
        return;
    };
    let session = sessions.join("source.jsonl");
    write_branched_session(&session, &workspace);
    let client = SdkBridgeClient::start(config.clone()).expect("start compatible bridge");
    assert!(client.hello().capabilities.navigate_tree);
    assert!(client.hello().capabilities.labels);

    client
        .call_default(BridgeCommand::SetLabel {
            session_path: session.to_string_lossy().into_owned(),
            cwd: workspace.to_string_lossy().into_owned(),
            target_id: "u2".to_owned(),
            label: Some("checkpoint".to_owned()),
        })
        .expect("set label");
    assert!(fs::read_to_string(&session).unwrap().contains("checkpoint"));
    client
        .call_default(BridgeCommand::SetLabel {
            session_path: session.to_string_lossy().into_owned(),
            cwd: workspace.to_string_lossy().into_owned(),
            target_id: "u2".to_owned(),
            label: None,
        })
        .expect("clear label");
    let cleared = fs::read_to_string(&session).unwrap();
    let last_u2_label = cleared
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .rfind(|entry| {
            entry.get("type").and_then(serde_json::Value::as_str) == Some("label")
                && entry.get("targetId").and_then(serde_json::Value::as_str) == Some("u2")
        })
        .expect("label clear entry");
    assert!(
        last_u2_label
            .get("label")
            .is_none_or(serde_json::Value::is_null)
    );
    let navigation = client
        .call_default(BridgeCommand::NavigateTree {
            session_path: session.to_string_lossy().into_owned(),
            cwd: workspace.to_string_lossy().into_owned(),
            target_id: "u1".to_owned(),
            summarize: false,
            custom_instructions: None,
            replace_instructions: false,
            label: None,
        })
        .expect("navigate");
    assert_eq!(navigation["cancelled"], false);
    assert_eq!(navigation["editorText"], "first");
    let persisted = fs::read_to_string(&session).unwrap();
    assert!(persisted.contains("checkpoint"));
    assert!(persisted.contains("\"checkpointId\"") || persisted.contains("\"type\":\"label\""));
    assert!(persisted.contains("\"id\":\"orphan\""));
    assert!(!persisted.contains("\"type\":\"model_change\""));
    assert!(!persisted.contains("\"type\":\"thinking_level_change\""));

    let export = root.join("active-path.jsonl");
    client
        .call_default(BridgeCommand::ExportJsonl {
            session_path: session.to_string_lossy().into_owned(),
            cwd: workspace.to_string_lossy().into_owned(),
            output_path: Some(export.to_string_lossy().into_owned()),
        })
        .expect("export active path");
    assert!(export.is_file());
    let exported = fs::read_to_string(&export).unwrap();
    assert!(exported.contains("\"targetId\":\"u1\""));
    assert!(!exported.contains("\"id\":\"u1\""));
    assert!(!exported.contains("\"id\":\"u2\""));
    assert!(!exported.contains("\"id\":\"orphan\""));

    let imported = client
        .call_default(BridgeCommand::ImportJsonl {
            input_path: export.to_string_lossy().into_owned(),
            cwd: workspace.to_string_lossy().into_owned(),
            session_dir: sessions.to_string_lossy().into_owned(),
        })
        .expect("import into new file");
    let imported_path = PathBuf::from(imported["path"].as_str().expect("import path"));
    assert!(imported_path.is_file());
    assert_ne!(imported_path, export);

    let missing = client
        .call_default(BridgeCommand::ImportJsonl {
            input_path: root.join("missing.jsonl").to_string_lossy().into_owned(),
            cwd: workspace.to_string_lossy().into_owned(),
            session_dir: sessions.to_string_lossy().into_owned(),
        })
        .expect_err("missing import must fail safely");
    assert_eq!(missing.kind, BridgeErrorKind::Rejected);
    client.stop();

    let restarted = SdkBridgeClient::start(config).expect("restart bridge");
    assert_eq!(restarted.hello().protocol_version, 1);
    restarted.stop();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn timed_out_bridge_request_issues_cancellation_without_claiming_success() {
    let root = temp_dir();
    let Some(config) = bridge_config(&root) else {
        let _ = fs::remove_dir_all(root);
        return;
    };
    let client = SdkBridgeClient::start(config).expect("start bridge");
    let error = client
        .call_with_id(
            BridgeCommand::Hello,
            "timeout-test".to_owned(),
            Duration::ZERO,
        )
        .expect_err("zero deadline must time out");
    assert_eq!(error.kind, BridgeErrorKind::Timeout);
    client.stop();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn branch_summary_options_are_forwarded_to_the_sdk() {
    let root = temp_dir();
    let Some(config) = fake_bridge_config(&root) else {
        let _ = fs::remove_dir_all(root);
        return;
    };
    let client = SdkBridgeClient::start(config).expect("start fake bridge");
    let result = client
        .call_default(BridgeCommand::NavigateTree {
            session_path: root.join("source.jsonl").to_string_lossy().into_owned(),
            cwd: root.to_string_lossy().into_owned(),
            target_id: "summary-target".to_owned(),
            summarize: true,
            custom_instructions: Some("Preserve decisions".to_owned()),
            replace_instructions: true,
            label: Some("alternate".to_owned()),
        })
        .expect("navigate with a branch summary");
    assert_eq!(result["cancelled"], false);
    assert_eq!(result["summaryEntryId"], "summary-1");
    client.stop();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn navigation_cancellation_aborts_the_sdk_operation() {
    let root = temp_dir();
    let Some(config) = fake_bridge_config(&root) else {
        let _ = fs::remove_dir_all(root);
        return;
    };
    let client = SdkBridgeClient::start(config).expect("start fake bridge");
    let caller = client.clone();
    let request = thread::spawn(move || {
        caller.call_with_id(
            BridgeCommand::NavigateTree {
                session_path: "source.jsonl".to_owned(),
                cwd: ".".to_owned(),
                target_id: "cancel-target".to_owned(),
                summarize: true,
                custom_instructions: None,
                replace_instructions: false,
                label: None,
            },
            "cancel-navigation".to_owned(),
            Duration::from_secs(5),
        )
    });
    thread::sleep(Duration::from_millis(50));
    client
        .cancel("cancel-navigation")
        .expect("send navigation cancellation");
    let result = request
        .join()
        .expect("join navigation request")
        .expect("cancelled navigation response");
    assert_eq!(result["cancelled"], true);
    assert_eq!(result["aborted"], true);
    client.stop();
    let _ = fs::remove_dir_all(root);
}
