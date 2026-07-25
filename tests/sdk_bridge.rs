use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use pi_gui::services::pi_process::discover_and_probe;
use pi_gui::services::sdk_bridge::{
    BridgeCommand, BridgeErrorKind, ORCHESTRATION_PIPE_ENV, SdkBridgeClient, SdkBridgeConfig,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[test]
fn bridge_schema_covers_resource_and_orchestration_requests_events_and_cancellation() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../bridge/protocol.schema.json"))
            .expect("bridge protocol schema must be valid JSON");
    let encoded = schema.to_string();
    for required in [
        "get_resource_inventory",
        "reload_resources",
        "resource_progress",
        "resources_changed",
        "get_orchestration_snapshot",
        "orchestration_action",
        "orchestration_snapshot",
        "orchestration_disconnected",
        "targetId",
        "unsupported_capability",
    ] {
        assert!(encoded.contains(required), "schema is missing {required}");
    }
}

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

fn fake_bridge_config(root: &Path) -> SdkBridgeConfig {
    let sdk_root = root.join("fake-sdk");
    fs::create_dir_all(sdk_root.join("dist")).expect("create fake SDK");
    fs::write(
        sdk_root.join("package.json"),
        "{\"type\":\"module\",\"version\":\"0.82.0\"}\n",
    )
    .expect("write fake SDK package");
    fs::write(
        sdk_root.join("dist/index.js"),
        r#"
class FakeManager {
  static open() { return new FakeManager(); }
  static inMemory() { return new FakeManager(); }
  appendLabelChange() { return "checkpoint"; }
  getLabel() { return undefined; }
  getLeafId() { return "target"; }
}

export const SessionManager = FakeManager;

export async function createAgentSession({ sessionManager, resourceLoader }) {
  if (resourceLoader) {
    const sourceInfo = {
      path: `${process.cwd()}/global-extension.js`,
      source: "global-extension",
      scope: "user",
      origin: "top-level",
    };
    return {
      session: {
        getActiveToolNames() { return ["read", "synthetic_tool"]; },
        getAllTools() {
          return [
            { name: "read", description: "Read files", sourceInfo: { ...sourceInfo, path: "<builtin:read>", source: "builtin" } },
            { name: "synthetic_tool", description: "Synthetic dynamic tool", sourceInfo },
          ];
        },
        dispose() {},
      },
      extensionsResult: resourceLoader.getExtensions(),
    };
  }
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

let storedAuth = false;
const fakeModel = {
  provider: "synthetic-provider",
  id: "synthetic-model",
  name: "Synthetic Model",
  api: "synthetic-api",
  reasoning: true,
  thinkingLevelMap: { minimal: null, xhigh: "xhigh", max: null },
  input: ["text"],
  contextWindow: 32000,
  maxTokens: 4096,
  cost: {
    input: 1,
    output: 2,
    cacheRead: 0.1,
    cacheWrite: 1.25,
    tiers: [{ inputTokensAbove: 200000, input: 2, output: 3, cacheRead: 0.2, cacheWrite: 2.5 }],
  },
};
const fakeProvider = {
  id: "synthetic-provider",
  name: "Synthetic Provider",
  auth: {
    apiKey: {
      async login(interaction) {
        interaction.notify({ type: "progress", message: "Waiting for synthetic input" });
        const key = await interaction.prompt({ type: "secret", message: "Synthetic credential" });
        if (key !== "synthetic-value") throw new Error("Unexpected synthetic input");
        return { type: "api_key", key };
      },
    },
  },
};

export class ModelRuntime {
  static async create() { return new ModelRuntime(); }
  getModels() { return [fakeModel]; }
  getAvailableSnapshot() { return [fakeModel]; }
  getProviders() { return [fakeProvider]; }
  getProviderAuthStatus() {
    return storedAuth
      ? { configured: true, source: "stored" }
      : { configured: true, source: "environment", label: "SYNTHETIC_SECRET" };
  }
  getModel(provider, id) {
    return provider === fakeModel.provider && id === fakeModel.id ? fakeModel : undefined;
  }
  getError() { return undefined; }
  async refresh() { return { aborted: false, errors: new Map() }; }
  async login(_provider, _type, interaction) {
    await fakeProvider.auth.apiKey.login(interaction);
    storedAuth = true;
  }
  async logout() { storedAuth = false; }
}

export function getAgentDir() { return `${process.cwd()}/agent-home`; }

export class SettingsManager {
  constructor(projectTrusted = false) { this.projectTrusted = projectTrusted; }
  static create(_cwd, _agentDir, options = {}) { return new SettingsManager(options.projectTrusted === true); }
  static inMemory() { return new SettingsManager(false); }
  getGlobalSettings() { return {}; }
  getProjectSettings() {
    return this.projectTrusted
      ? { packages: ["missing-project-package@1.0.0"], extensions: ["project-extension.js"] }
      : {};
  }
  isProjectTrusted() { return this.projectTrusted; }
  setProjectTrusted(value) { this.projectTrusted = value; }
  async reload() {}
  getDefaultProvider() { return "synthetic-provider"; }
  getDefaultModel() { return "synthetic-model"; }
  getDefaultThinkingLevel() { return "high"; }
  getEnabledModels() { return ["synthetic-provider/synthetic-model"]; }
  getSteeringMode() { return "one-at-a-time"; }
  getFollowUpMode() { return "one-at-a-time"; }
  getTransport() { return "auto"; }
  getCompactionEnabled() { return true; }
  getRetryEnabled() { return true; }
  getHideThinkingBlock() { return false; }
  getShowCacheMissNotices() { return false; }
  getQuietStartup() { return false; }
  getCollapseChangelog() { return false; }
  getEnableInstallTelemetry() { return true; }
  getEnableAnalytics() { return false; }
  getShowImages() { return true; }
  getClearOnShrink() { return false; }
  getImageAutoResize() { return true; }
  getBlockImages() { return false; }
  getDoubleEscapeAction() { return "tree"; }
  getTreeFilterMode() { return "default"; }
  getShowHardwareCursor() { return false; }
  getEditorPaddingX() { return 0; }
  getOutputPad() { return 1; }
  getAutocompleteMaxVisible() { return 5; }
  setDefaultModelAndProvider() {}
  setDefaultThinkingLevel() {}
  setEnabledModels() {}
  getEnableSkillCommands() { return true; }
  setEnableSkillCommands() {}
  getThemeSetting() { return "synthetic-theme"; }
  setTheme() {}
  getDefaultProjectTrust() { return "never"; }
  async flush() {}
  drainErrors() { return []; }
}

export class DefaultPackageManager {
  constructor({ settingsManager }) { this.settingsManager = settingsManager; }
  async resolve(onMissing) {
    if (typeof onMissing !== "function") throw new Error("inventory must use the non-installing resolver");
    const root = process.cwd();
    const globalMetadata = { source: "global-extension", scope: "user", origin: "top-level" };
    const projectMetadata = { source: "project-extension", scope: "project", origin: "top-level" };
    return {
      extensions: [
        { path: `${root}/global-extension.js`, enabled: true, metadata: globalMetadata },
        ...(this.settingsManager.projectTrusted
          ? [{ path: `${root}/project-extension.js`, enabled: true, metadata: projectMetadata }]
          : []),
      ],
      skills: [{ path: `${root}/SKILL.md`, enabled: true, metadata: globalMetadata }],
      prompts: [{ path: `${root}/prompt.md`, enabled: true, metadata: globalMetadata }],
      themes: [{ path: `${root}/theme.json`, enabled: true, metadata: globalMetadata }],
    };
  }
  listConfiguredPackages() {
    return this.settingsManager.projectTrusted
      ? [
          {
            source: "synthetic-package@1.2.3",
            scope: "user",
            filtered: true,
            installedPath: `${process.cwd()}/installed-package`,
          },
          {
            source: "missing-project-package@1.0.0",
            scope: "project",
            filtered: false,
          },
        ]
      : [];
  }
}

export class DefaultResourceLoader {
  constructor() {
    const root = process.cwd();
    this.sourceInfo = {
      path: `${root}/global-extension.js`,
      source: "global-extension",
      scope: "user",
      origin: "top-level",
    };
  }
  async reload() {}
  getExtensions() {
    return {
      extensions: [
        {
          path: `${process.cwd()}/global-extension.js`,
          resolvedPath: `${process.cwd()}/global-extension.js`,
          sourceInfo: this.sourceInfo,
        },
      ],
      errors: [
        {
          path: `${process.cwd()}/broken-extension.js`,
          error: "TOP_SECRET extension failure",
        },
      ],
      runtime: {
        pendingProviderRegistrations: [
          {
            name: "dynamic-provider",
            extensionPath: `${process.cwd()}/global-extension.js`,
          },
        ],
      },
    };
  }
  getSkills() {
    return {
      skills: [
        {
          name: "synthetic-skill",
          description: "Synthetic skill",
          filePath: `${process.cwd()}/SKILL.md`,
          sourceInfo: this.sourceInfo,
        },
      ],
      diagnostics: [
        {
          type: "error",
          message: "TOP_SECRET skill failure",
          path: `${process.cwd()}/broken-skill.md`,
        },
      ],
    };
  }
  getPrompts() {
    return {
      prompts: [
        {
          name: "synthetic-prompt",
          description: "Synthetic prompt",
          filePath: `${process.cwd()}/prompt.md`,
          sourceInfo: this.sourceInfo,
        },
      ],
      diagnostics: [],
    };
  }
  getThemes() {
    return {
      themes: [
        {
          name: "synthetic-theme",
          sourcePath: `${process.cwd()}/theme.json`,
          sourceInfo: this.sourceInfo,
        },
      ],
      diagnostics: [],
    };
  }
}

export function loadProjectContextFiles() {
  return [
    { path: `${process.cwd()}/AGENTS.md`, content: "TOP_SECRET context" },
  ];
}
"#,
    )
    .expect("write fake SDK module");
    SdkBridgeConfig {
        node: PathBuf::from("node"),
        sdk_root,
        script: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bridge/pi-bridge.mjs"),
        working_directory: root.to_path_buf(),
    }
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
    assert_eq!(client.hello().sdk_version, "0.82.0");
    assert!(client.hello().capabilities.navigate_tree);
    assert!(client.hello().capabilities.labels);
    assert!(client.hello().capabilities.resource_inventory);
    let inventory = client
        .call_default(BridgeCommand::GetResourceInventory)
        .expect("read real SDK resource inventory");
    let inventory: pi_gui::resource_center::ResourceInventorySnapshot =
        serde_json::from_value(inventory).expect("decode real SDK resource inventory");
    assert!(!inventory.project_trusted);
    assert!(!inventory.package_mutations.install);

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
    let config = fake_bridge_config(&root);
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
    let config = fake_bridge_config(&root);
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

#[test]
fn model_runtime_auth_flow_is_secret_free_and_logout_reports_environment_fallback() {
    let root = temp_dir();
    let config = fake_bridge_config(&root);
    let client = SdkBridgeClient::start(config).expect("start fake bridge");
    assert!(client.hello().capabilities.model_runtime);
    assert!(client.hello().capabilities.provider_auth);

    let snapshot = client
        .call_default(BridgeCommand::GetModelRuntime)
        .expect("read cached model snapshot");
    let encoded = snapshot.to_string();
    assert!(encoded.contains("synthetic-provider"));
    assert!(encoded.contains("inputTokensAbove"));
    assert!(!encoded.contains("SYNTHETIC_SECRET"));
    assert!(!encoded.contains("headers"));
    assert!(!encoded.contains("baseUrl"));

    let caller = client.clone();
    let login = thread::spawn(move || {
        caller.call_with_id(
            BridgeCommand::LoginProvider {
                operation_id: 42,
                provider: "synthetic-provider".to_owned(),
                auth_type: pi_gui::model_runtime::AuthMethod::ApiKey,
            },
            "operation-42".to_owned(),
            Duration::from_secs(5),
        )
    });
    let events = client.events();
    let prompt = loop {
        let event = events.recv_blocking().expect("authentication event");
        if let pi_gui::services::sdk_bridge::BridgeEvent::Auth(
            pi_gui::model_runtime::AuthEvent::AuthPrompt { operation, prompt },
        ) = event
        {
            assert_eq!(operation, 42);
            break prompt;
        }
    };
    client
        .call_default(BridgeCommand::AuthRespond {
            operation_id: 42,
            prompt_id: prompt.prompt_id,
            value: pi_gui::services::sdk_bridge::SensitiveValue::new("synthetic-value".to_owned()),
        })
        .expect("answer synthetic prompt");
    let logged_in = login.join().expect("join login").expect("complete login");
    assert!(!logged_in.to_string().contains("synthetic-value"));

    let logged_out = client
        .call_default(BridgeCommand::LogoutProvider {
            provider: "synthetic-provider".to_owned(),
        })
        .expect("logout");
    assert_eq!(logged_out["environmentFallback"], true);
    client.stop();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resource_inventory_rejects_project_code_tracks_dynamic_state_and_redacts_failures() {
    let root = temp_dir();
    let config = fake_bridge_config(&root);
    let client = SdkBridgeClient::start(config).expect("start fake bridge");
    assert!(client.hello().capabilities.resource_inventory);
    assert!(client.hello().capabilities.resource_reload);
    assert!(client.hello().capabilities.active_tool_state);
    assert!(client.hello().capabilities.resource_settings);
    assert!(!client.hello().capabilities.package_mutations);
    assert_eq!(client.hello().transport, "stdio-jsonl");
    assert_eq!(client.hello().ownership, "pi-sdk-sidecar");

    let first = client
        .call_default(BridgeCommand::GetResourceInventory)
        .expect("read resource inventory");
    let snapshot: pi_gui::resource_center::ResourceInventorySnapshot =
        serde_json::from_value(first.clone()).expect("decode resource inventory");
    assert!(!snapshot.project_trusted);
    assert!(!snapshot.package_mutations.install);
    assert!(!snapshot.package_mutations.configure);
    assert!(snapshot.items.iter().any(|item| {
        item.kind == pi_gui::resource_center::ResourceKind::Tool
            && item.name == "synthetic_tool"
            && item.active == Some(true)
    }));
    assert!(snapshot.items.iter().any(|item| {
        item.kind == pi_gui::resource_center::ResourceKind::Provider
            && item.name == "dynamic-provider"
    }));
    assert!(snapshot.items.iter().any(|item| {
        item.scope == pi_gui::resource_center::ResourceScope::Project
            && item.state == pi_gui::resource_center::ResourceLoadState::Disabled
            && item.trust == pi_gui::resource_center::ResourceTrust::Rejected
    }));
    assert!(snapshot.items.iter().any(|item| {
        item.kind == pi_gui::resource_center::ResourceKind::Package
            && item.name == "missing-project-package@1.0.0"
            && item.state == pi_gui::resource_center::ResourceLoadState::Error
    }));
    let encoded = first.to_string();
    assert!(!encoded.contains("TOP_SECRET"));

    client
        .call_default(BridgeCommand::SetSkillCommandsEnabled { enabled: false })
        .expect("set targeted skill-command preference");
    client
        .call_default(BridgeCommand::SetResourceTheme {
            theme: "synthetic-theme".to_owned(),
        })
        .expect("set targeted resource theme");
    let reloaded = client
        .call_default(BridgeCommand::ReloadResources)
        .expect("reload resources");
    assert!(reloaded["generation"].as_u64().expect("reload generation") > snapshot.generation);
    client.stop();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn bridge_rejects_mismatched_sdk_versions() {
    let root = temp_dir();
    let config = fake_bridge_config(&root);
    fs::write(
        config.sdk_root.join("package.json"),
        "{\"type\":\"module\",\"version\":\"0.81.1\"}\n",
    )
    .expect("write incompatible fake SDK package");

    let error = match SdkBridgeClient::start(config) {
        Ok(client) => {
            client.stop();
            panic!("mismatched SDK version should be rejected");
        }
        Err(error) => error,
    };
    assert_eq!(error.kind, BridgeErrorKind::Protocol);
    assert_eq!(error.summary, "The Pi SDK bridge version is incompatible.");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn bridge_rejects_unknown_protocol_versions_and_exits_on_stdin_close() {
    let root = temp_dir();
    let config = fake_bridge_config(&root);
    let mut child = Command::new(&config.node)
        .arg(&config.script)
        .arg(&config.sdk_root)
        .env_remove(ORCHESTRATION_PIPE_ENV)
        .current_dir(&config.working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start raw bridge");
    let mut stdin = child.stdin.take().expect("bridge stdin");
    stdin
        .write_all(
            b"{\"version\":999,\"type\":\"request\",\"id\":\"future\",\"command\":\"hello\",\"params\":{}}\n",
        )
        .expect("write future request");
    stdin.flush().expect("flush request");
    let mut reader = BufReader::new(child.stdout.take().expect("bridge stdout"));
    let mut output = String::new();
    reader.read_line(&mut output).expect("read rejection");
    let response: serde_json::Value =
        serde_json::from_str(&output).expect("decode rejection response");
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "incompatible_protocol");

    stdin
        .write_all(
            b"{\"version\":1,\"type\":\"request\",\"id\":\"install\",\"command\":\"package_install\",\"params\":{\"source\":\"dangerous-package\"}}\n",
        )
        .expect("write disabled package mutation");
    stdin.flush().expect("flush package request");
    output.clear();
    reader
        .read_line(&mut output)
        .expect("read capability rejection");
    let response: serde_json::Value =
        serde_json::from_str(&output).expect("decode capability rejection");
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "unsupported_capability");
    drop(stdin);
    assert!(child.wait().expect("wait for clean bridge exit").success());
    let _ = fs::remove_dir_all(root);
}
