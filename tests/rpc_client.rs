use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use pi_gui::services::pi_process::{
    DiscoveryError, PiLaunchConfig, ProjectTrust, ResourcePolicy, SessionLaunch, StartError,
};
use pi_gui::services::rpc::{
    Command, ConnectionStatus, ExtensionUiResponse, ExtensionUiResponseBody, IncomingRecord,
    RequestId, ResponseResult, RpcClient, RpcClientErrorKind, RpcClientStartError, RpcDeadlines,
    RpcEvent, ThinkingLevel,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
static COMPILED_FAKE: OnceLock<PathBuf> = OnceLock::new();

struct TestEnvironment {
    root: PathBuf,
    executable: PathBuf,
}

impl TestEnvironment {
    fn new(mode: &str) -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "pi gui rpc client 日本語 spaces-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create test directory");
        fs::write(root.join("fake-mode.txt"), mode).expect("write fake mode");
        let bin = root.join("bin with spaces 日本語");
        fs::create_dir_all(&bin).expect("create fake bin directory");
        let executable = bin.join(if cfg!(windows) {
            "fake pi 日本語.exe"
        } else {
            "fake pi 日本語"
        });
        fs::copy(compiled_fake(), &executable).expect("copy fake executable");
        Self { root, executable }
    }

    fn config(&self) -> PiLaunchConfig {
        let mut config = PiLaunchConfig::new(
            &self.root,
            ProjectTrust::Reject,
            SessionLaunch::Ephemeral,
            ResourcePolicy::disabled(),
        );
        config.executable_override = Some(self.executable.clone());
        config.probe_timeout = Duration::from_secs(10);
        config.shutdown_timeout = Duration::from_millis(250);
        config.stderr_capacity_bytes = 4096;
        config.environment_overrides.push((
            OsString::from("FAKE_PI_ENVIRONMENT"),
            OsString::from("isolated-child-value"),
        ));
        config
    }

    fn deadlines(&self) -> RpcDeadlines {
        RpcDeadlines {
            readiness: Duration::from_secs(2),
            read: Duration::from_millis(250),
            mutation: Duration::from_millis(250),
            prompt: Duration::from_millis(250),
            bash: Duration::from_millis(250),
            urgent: Duration::from_millis(250),
        }
    }

    fn start(&self) -> RpcClient {
        RpcClient::start_with_deadlines(self.config(), self.deadlines()).expect("start RPC client")
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn compiled_fake() -> &'static Path {
    COMPILED_FAKE
        .get_or_init(|| {
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let output_directory = std::env::temp_dir()
                .join(format!("pi-gui-rpc-compiled-fake-{}", std::process::id()));
            fs::create_dir_all(&output_directory).expect("create fake output directory");
            let output = output_directory.join(if cfg!(windows) {
                "fake-pi.exe"
            } else {
                "fake-pi"
            });
            let status = ProcessCommand::new("rustc")
                .arg("--edition=2024")
                .arg(manifest.join("tests/fixtures/fake_pi.rs"))
                .arg("-o")
                .arg(&output)
                .status()
                .expect("run rustc for fake Pi");
            assert!(status.success(), "fake Pi must compile");
            output
        })
        .as_path()
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        thread::yield_now();
    }
    panic!("timed out waiting for {}", path.display());
}

fn assert_response_command(result: ResponseResult, expected: &str) {
    assert_eq!(result.command(), expected);
    assert!(!matches!(result, ResponseResult::Failure { .. }));
}

#[test]
fn concurrent_reads_correlate_out_of_order_and_events_route_independently() {
    let environment = TestEnvironment::new("rpc-out-of-order");
    let client = environment.start();

    let messages = client.request(Command::GetMessages);
    let commands = client.request(Command::GetCommands);
    assert_ne!(messages.id(), commands.id());
    assert_response_command(
        commands.wait().expect("commands response").result,
        "get_commands",
    );
    assert_response_command(
        messages.wait().expect("messages response").result,
        "get_messages",
    );

    let notification = client
        .recv_notification_timeout(Duration::from_secs(1))
        .expect("interleaved event");
    assert_eq!(notification.generation, client.generation());
    assert!(matches!(
        notification.record,
        IncomingRecord::Event(event) if matches!(event.as_ref(), RpcEvent::AgentStart)
    ));
    client.stop();
}

#[test]
fn mutation_lane_serializes_but_abort_and_extension_responses_bypass() {
    let environment = TestEnvironment::new("rpc-bypass");
    let client = environment.start();

    let first = client.request(Command::SetAutoCompaction { enabled: false });
    wait_for_file(&environment.root.join("seen-set_auto_compaction.txt"));
    let second = client.request(Command::SetThinkingLevel {
        level: ThinkingLevel::High,
    });
    let abort = client.request(Command::Abort);
    wait_for_file(&environment.root.join("seen-abort.txt"));
    client
        .send_extension_ui_response(ExtensionUiResponse {
            id: RequestId::from("dialog-1"),
            response: ExtensionUiResponseBody::Cancelled,
        })
        .expect("extension response bypasses mutation lane");
    wait_for_file(&environment.root.join("seen-extension_ui_response.txt"));
    assert!(
        !environment
            .root
            .join("seen-set_thinking_level.txt")
            .exists(),
        "second mutation reached Pi before the first response"
    );

    fs::write(
        environment.root.join("release-first-mutation.txt"),
        "release",
    )
    .expect("release first mutation");
    assert_response_command(
        first.wait().expect("first mutation response").result,
        "set_auto_compaction",
    );
    assert_response_command(abort.wait().expect("abort response").result, "abort");
    assert_response_command(
        second.wait().expect("second mutation response").result,
        "set_thinking_level",
    );
    client.stop();
}

#[test]
fn fake_rpc_accepts_prompt_steer_follow_up_and_abort() {
    let environment = TestEnvironment::new("rpc-normal");
    let client = environment.start();
    let commands = [
        (
            "prompt",
            Command::Prompt {
                message: "first line\nsecond line".to_owned(),
                images: None,
                streaming_behavior: None,
            },
        ),
        (
            "steer",
            Command::Steer {
                message: "change course".to_owned(),
                images: None,
            },
        ),
        (
            "follow_up",
            Command::FollowUp {
                message: "then summarize".to_owned(),
                images: None,
            },
        ),
        ("abort", Command::Abort),
    ];

    for (name, command) in commands {
        assert_response_command(
            client
                .request(command)
                .wait()
                .expect("accepted response")
                .result,
            name,
        );
        wait_for_file(&environment.root.join(format!("seen-{name}.txt")));
    }
    let transcript = fs::read_to_string(environment.root.join("rpc-input.jsonl"))
        .expect("recorded RPC commands");
    assert!(transcript.contains("first line\\nsecond line"));
    assert_eq!(transcript.matches("\"type\":\"prompt\"").count(), 1);
    assert_eq!(transcript.matches("\"type\":\"steer\"").count(), 1);
    assert_eq!(transcript.matches("\"type\":\"follow_up\"").count(), 1);
    assert_eq!(transcript.matches("\"type\":\"abort\"").count(), 1);
    client.stop();
}

#[test]
fn prompt_disconnect_is_uncertain_and_never_replayed_by_the_client() {
    let environment = TestEnvironment::new("rpc-prompt-disconnect");
    let client = environment.start();
    let error = client
        .request(Command::Prompt {
            message: "keep this draft".to_owned(),
            images: None,
            streaming_behavior: None,
        })
        .wait()
        .expect_err("prompt must lose acceptance confirmation");
    assert!(matches!(
        error.kind,
        RpcClientErrorKind::ProcessExit | RpcClientErrorKind::StdoutFault
    ));
    wait_for_file(&environment.root.join("seen-prompt.txt"));
    let transcript =
        fs::read_to_string(environment.root.join("rpc-input.jsonl")).expect("recorded prompt");
    assert_eq!(transcript.matches("\"type\":\"prompt\"").count(), 1);
    client.stop();
}

#[test]
fn timed_out_read_is_unknown_outcome_without_claiming_cancellation() {
    let environment = TestEnvironment::new("rpc-read-timeout");
    let client = environment.start();

    let error = client
        .request(Command::GetMessages)
        .wait()
        .expect_err("read must time out");
    assert_eq!(error.kind, RpcClientErrorKind::UnknownOutcome);
    assert!(error.to_string().contains("was not cancelled"));
    assert_response_command(
        client
            .request(Command::GetCommands)
            .wait()
            .expect("connection remains usable after missing read response")
            .result,
        "get_commands",
    );
    client.stop();
}

#[test]
fn late_read_response_is_ignored_after_unknown_outcome() {
    let environment = TestEnvironment::new("rpc-late-read");
    let client = environment.start();

    let error = client
        .request(Command::GetMessages)
        .wait()
        .expect_err("read must time out before its delayed response");
    assert_eq!(error.kind, RpcClientErrorKind::UnknownOutcome);
    wait_for_file(&environment.root.join("late-response-sent.txt"));
    assert_response_command(
        client
            .request(Command::GetCommands)
            .wait()
            .expect("late response must not fault the connection")
            .result,
        "get_commands",
    );
    client.stop();
}

#[test]
fn timed_out_mutation_is_unknown_outcome_and_poisons_connection() {
    let environment = TestEnvironment::new("rpc-timeout");
    let client = environment.start();

    let error = client
        .request(Command::SetAutoCompaction { enabled: false })
        .wait()
        .expect_err("mutation must time out");
    assert_eq!(error.kind, RpcClientErrorKind::UnknownOutcome);
    assert!(error.to_string().contains("was not cancelled"));
    assert_eq!(
        client.diagnostics().status,
        ConnectionStatus::Faulted(RpcClientErrorKind::ConnectionPoisoned)
    );

    let later = client
        .request(Command::SetAutoRetry { enabled: false })
        .wait()
        .expect_err("poisoned connection rejects later mutation");
    assert!(matches!(
        later.kind,
        RpcClientErrorKind::Stopped | RpcClientErrorKind::ConnectionPoisoned
    ));
    client.stop();
}

fn assert_two_pending_rejected(mode: &str, expected: RpcClientErrorKind) {
    let environment = TestEnvironment::new(mode);
    let client = environment.start();
    let first = client.request(Command::GetMessages);
    let second = client.request(Command::GetCommands);
    let first_error = first.wait().expect_err("first pending request must fail");
    let second_error = second.wait().expect_err("second pending request must fail");
    assert_eq!(first_error.kind, expected);
    assert_eq!(second_error.kind, expected);
    client.stop();
}

#[test]
fn codec_fault_rejects_every_pending_request() {
    assert_two_pending_rejected("rpc-parse-error", RpcClientErrorKind::ProtocolFault);
}

#[test]
fn process_exit_rejects_every_pending_request() {
    assert_two_pending_rejected("rpc-exit", RpcClientErrorKind::ProcessExit);
}

#[test]
fn stdout_eof_rejects_every_pending_request() {
    assert_two_pending_rejected("rpc-early-eof", RpcClientErrorKind::StdoutFault);
}

#[test]
fn writer_failure_rejects_already_pending_requests() {
    let environment = TestEnvironment::new("rpc-writer-failure");
    let client = environment.start();
    let first = client.request(Command::GetMessages);
    let second = client.request(Command::GetCommands);
    let notification = client
        .recv_notification_timeout(Duration::from_secs(1))
        .expect("writer-closed marker");
    assert!(matches!(
        notification.record,
        IncomingRecord::UnknownEvent(event) if event.event_type == "writer_closed"
    ));
    let trigger = client.request(Command::GetTree);

    assert_eq!(
        first.wait().expect_err("first pending request").kind,
        RpcClientErrorKind::WriterFailure
    );
    assert_eq!(
        second.wait().expect_err("second pending request").kind,
        RpcClientErrorKind::WriterFailure
    );
    assert!(matches!(
        trigger.wait().expect_err("trigger request").kind,
        RpcClientErrorKind::WriterFailure | RpcClientErrorKind::Stopped
    ));
    client.stop();
}

#[test]
fn explicit_stop_rejects_every_pending_request() {
    let environment = TestEnvironment::new("rpc-read-timeout");
    let mut deadlines = environment.deadlines();
    deadlines.read = Duration::from_secs(10);
    let client = RpcClient::start_with_deadlines(environment.config(), deadlines).expect("start");
    let first = client.request(Command::GetMessages);
    wait_for_file(&environment.root.join("seen-get_messages.txt"));
    let second = client.request(Command::GetMessages);
    client.stop();
    assert_eq!(
        first.wait().expect_err("first stopped request").kind,
        RpcClientErrorKind::Stopped
    );
    assert_eq!(
        second.wait().expect_err("second stopped request").kind,
        RpcClientErrorKind::Stopped
    );
}

#[test]
fn missing_and_unknown_response_ids_are_protocol_faults() {
    for mode in ["rpc-missing-id", "rpc-unknown-id"] {
        let environment = TestEnvironment::new(mode);
        let client = environment.start();
        let error = client
            .request(Command::GetMessages)
            .wait()
            .expect_err("invalid response id must fault");
        assert_eq!(error.kind, RpcClientErrorKind::ProtocolFault);
        client.stop();
    }
}

#[test]
fn fresh_retry_advances_generation_and_ignores_buffered_old_records() {
    let environment = TestEnvironment::new("rpc-generation");
    fs::write(environment.root.join("generation-label.txt"), "old").expect("old label");
    let client = environment.start();
    let old_generation = client.generation();
    wait_for_file(&environment.root.join("generation-marker-sent.txt"));

    fs::write(environment.root.join("generation-label.txt"), "new").expect("new label");
    let new_generation = client.retry_fresh().expect("fresh retry");
    assert_eq!(new_generation, old_generation.next());
    let notification = client
        .recv_notification_timeout(Duration::from_secs(1))
        .expect("new generation marker");
    assert_eq!(notification.generation, new_generation);
    let IncomingRecord::UnknownEvent(event) = notification.record else {
        panic!("expected generation marker");
    };
    assert_eq!(event.raw["label"], "new");
    assert!(client.diagnostics().stale_records_ignored >= 1);
    client.stop();
}

#[test]
fn readiness_requires_correlated_get_state_not_process_start() {
    let environment = TestEnvironment::new("rpc-readiness-missing");
    let error = match RpcClient::start_with_deadlines(environment.config(), environment.deadlines())
    {
        Ok(client) => {
            client.stop();
            panic!("process-level startup must not imply RPC readiness")
        }
        Err(error) => error,
    };
    assert!(matches!(
        error,
        RpcClientStartError::Readiness(ref error)
            if error.kind == RpcClientErrorKind::UnknownOutcome
    ));
    wait_for_file(&environment.root.join("seen-get_state.txt"));
}

#[test]
fn fragmented_frames_stderr_noise_and_environment_overrides_are_supported() {
    let environment = TestEnvironment::new("rpc-fragmented");
    let mut config = environment.config();
    config.offline = true;
    config.disable_tools = true;
    let client = RpcClient::start_with_deadlines(config, environment.deadlines())
        .expect("start fragmented RPC client");
    assert_response_command(
        client
            .request(Command::GetMessages)
            .wait()
            .expect("fragmented response")
            .result,
        "get_messages",
    );
    let notification = client
        .recv_notification_timeout(Duration::from_secs(1))
        .expect("fragmented script event");
    assert!(matches!(
        notification.record,
        IncomingRecord::Event(event) if matches!(event.as_ref(), RpcEvent::AgentStart)
    ));
    assert_eq!(
        fs::read_to_string(environment.root.join("launch-environment.txt"))
            .expect("child environment"),
        "isolated-child-value"
    );
    let launch_arguments =
        fs::read_to_string(environment.root.join("launch-args.txt")).expect("launch arguments");
    assert!(
        launch_arguments
            .lines()
            .any(|argument| argument == "--offline")
    );
    assert!(
        launch_arguments
            .lines()
            .any(|argument| argument == "--no-tools")
    );
    client.stop();

    let stderr_environment = TestEnvironment::new("rpc-stderr");
    let stderr_client = stderr_environment.start();
    stderr_client
        .request(Command::GetMessages)
        .wait()
        .expect("response with stderr noise");
    let deadline = Instant::now() + Duration::from_secs(1);
    while stderr_client.diagnostics().stderr.is_empty() && Instant::now() < deadline {
        thread::yield_now();
    }
    let diagnostics = stderr_client.diagnostics().stderr;
    assert!(diagnostics.iter().all(|entry| entry.contains("redacted")));
    assert!(!diagnostics.join("\n").contains("private synthetic"));
    stderr_client.stop();
}

#[test]
fn ignored_shutdown_is_forced_after_the_configured_grace_period() {
    let environment = TestEnvironment::new("rpc-ignore-shutdown");
    let client = environment.start();
    let report = client.stop().expect("stop ignored-shutdown script");
    assert!(report.forced);
}

#[test]
fn installed_pi_smoke_is_isolated_offline_and_closes_cleanly() {
    let root = std::env::temp_dir().join(format!(
        "pi-gui-installed-smoke-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let agent_directory = root.join("isolated agent directory");
    fs::create_dir_all(&agent_directory).expect("create isolated Pi directory");
    let mut config = PiLaunchConfig::new(
        &root,
        ProjectTrust::Reject,
        SessionLaunch::Ephemeral,
        ResourcePolicy::disabled(),
    );
    config.offline = true;
    config.disable_tools = true;
    config.environment_overrides.push((
        OsString::from("PI_CODING_AGENT_DIR"),
        agent_directory.as_os_str().to_owned(),
    ));
    config.probe_timeout = Duration::from_secs(15);
    config.shutdown_timeout = Duration::from_secs(3);

    let client = match RpcClient::start_with_deadlines(config, RpcDeadlines::default()) {
        Ok(client) => client,
        Err(RpcClientStartError::Process(StartError::Discovery(
            DiscoveryError::MissingFromPath
            | DiscoveryError::IncompatibleVersion { .. }
            | DiscoveryError::MissingCapabilities(_),
        ))) => {
            let _ = fs::remove_dir_all(root);
            return;
        }
        Err(error) => panic!("installed Pi smoke failed: {error}"),
    };
    assert!(client.initial_state().is_some());
    assert_eq!(client.generation().value(), 1);
    let report = client.stop().expect("stop installed Pi");
    assert!(
        !report.forced,
        "installed Pi should close after stdin closes"
    );
    let _ = fs::remove_dir_all(root);
}
