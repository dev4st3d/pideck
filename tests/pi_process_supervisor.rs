use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use pi_gui::services::pi_process::{
    DiscoveryError, PiLaunchConfig, PiSupervisor, ProcessFailureKind, ProjectTrust, ResourcePolicy,
    SessionLaunch, StartError, StdoutEvent, SupervisorState,
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
            "pi gui supervisor 日本語 spaces-{}-{sequence}",
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
        config.probe_timeout = Duration::from_secs(3);
        config.shutdown_timeout = Duration::from_millis(150);
        config.stderr_capacity_bytes = 4096;
        config
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
            let output_directory =
                std::env::temp_dir().join(format!("pi-gui-compiled-fake-{}", std::process::id()));
            fs::create_dir_all(&output_directory).expect("create fake output directory");
            let output = output_directory.join(if cfg!(windows) {
                "fake-pi.exe"
            } else {
                "fake-pi"
            });
            let status = Command::new("rustc")
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

fn wait_for_output(supervisor: &PiSupervisor, needle: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match supervisor.stdout().recv_timeout(Duration::from_millis(100)) {
            Ok(StdoutEvent::Data(bytes))
                if bytes.windows(needle.len()).any(|part| part == needle) =>
            {
                return;
            }
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("stdout channel closed before output: {error}"),
        }
    }
    panic!("timed out waiting for fake output");
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {}", path.display());
}

fn heartbeat_value(path: &Path) -> String {
    fs::read_to_string(path).expect("read descendant heartbeat")
}

fn assert_heartbeat_stops(path: &Path) {
    thread::sleep(Duration::from_millis(100));
    let first = heartbeat_value(path);
    thread::sleep(Duration::from_millis(250));
    let second = heartbeat_value(path);
    assert_eq!(
        first, second,
        "descendant continued after process-tree cleanup"
    );
}

#[test]
fn starts_in_unicode_space_path_and_closes_normally() {
    let environment = TestEnvironment::new("normal");
    let mut supervisor = PiSupervisor::start(environment.config()).expect("start fake Pi");

    assert_eq!(supervisor.state(), SupervisorState::Ready);
    assert_eq!(
        supervisor.working_directory(),
        fs::canonicalize(&environment.root).expect("canonical test directory")
    );
    wait_for_output(&supervisor, b"fake_ready");
    let report = supervisor.shutdown();
    assert!(!report.forced);
    assert!(report.abort_sent);
    assert!(matches!(
        supervisor.state(),
        SupervisorState::Stopped { forced: false, .. }
    ));

    let arguments = fs::read_to_string(environment.root.join("launch-args.txt"))
        .expect("read launch arguments");
    for required in [
        "--mode",
        "rpc",
        "--no-approve",
        "--no-session",
        "--no-extensions",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
    ] {
        assert!(arguments.lines().any(|argument| argument == required));
    }
    let stdin = fs::read_to_string(environment.root.join("stdin.txt")).expect("read fake stdin");
    assert!(stdin.contains("\"type\":\"abort\""));
}

#[test]
fn records_failure_and_bounded_redacted_stderr() {
    let environment = TestEnvironment::new("stderr-flood");
    let mut supervisor = PiSupervisor::start(environment.config()).expect("start fake Pi");
    wait_for_output(&supervisor, b"fake_ready");

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline
        && !supervisor
            .stderr_snapshot()
            .iter()
            .any(|line| line.contains("redacted"))
    {
        thread::sleep(Duration::from_millis(20));
    }
    let diagnostics = supervisor.stderr_snapshot();
    assert!(diagnostics.iter().map(String::len).sum::<usize>() <= 4096);
    assert!(diagnostics.iter().any(|line| line.contains("redacted")));
    assert!(!diagnostics.join("\n").contains("private-test-token"));
    supervisor.shutdown();
}

#[test]
fn nonzero_exit_is_recoverable_failure() {
    let environment = TestEnvironment::new("fail");
    let mut supervisor = PiSupervisor::start(environment.config()).expect("start fake Pi");
    let state = supervisor.wait_for_terminal(Duration::from_secs(3));
    assert!(matches!(
        state,
        SupervisorState::Failed(ref failure)
            if failure.kind == ProcessFailureKind::UnexpectedExit
                && failure.exit_status.is_some_and(|status| status.code() == Some(23))
    ));
    supervisor.shutdown();
    let diagnostics = supervisor.stderr_snapshot();
    assert!(!diagnostics.is_empty());
    assert!(diagnostics.iter().all(|line| line.contains("redacted")));
    assert!(!diagnostics.join("\n").contains("synthetic child failure"));
}

#[test]
fn early_stdout_eof_fails_and_reaps_child() {
    let environment = TestEnvironment::new("early-eof");
    let mut supervisor = PiSupervisor::start(environment.config()).expect("start fake Pi");
    let state = supervisor.wait_for_terminal(Duration::from_secs(3));
    assert!(matches!(
        state,
        SupervisorState::Failed(ref failure)
            if failure.kind == ProcessFailureKind::EarlyStdoutEof
    ));
    supervisor.shutdown();
}

#[test]
fn broken_stdin_falls_back_to_forced_kill() {
    let environment = TestEnvironment::new("broken-stdin");
    let mut supervisor = PiSupervisor::start(environment.config()).expect("start fake Pi");
    wait_for_output(&supervisor, b"fake_ready");

    let report = supervisor.shutdown();
    assert!(report.forced);
    assert!(!report.abort_sent);
    assert!(matches!(
        supervisor.state(),
        SupervisorState::Stopped { forced: true, .. }
    ));
}

#[test]
fn stdout_queue_overflow_fails_closed_without_blocking_drain() {
    let environment = TestEnvironment::new("stdout-flood");
    let mut config = environment.config();
    config.stdout_queue_capacity = 1;
    let mut supervisor = PiSupervisor::start(config).expect("start fake Pi");

    let state = supervisor.wait_for_terminal(Duration::from_secs(3));
    assert!(matches!(
        state,
        SupervisorState::Failed(ref failure)
            if failure.kind == ProcessFailureKind::StdoutBackpressure
    ));
    supervisor.shutdown();
}

#[test]
fn root_exit_also_reaps_live_descendants() {
    let environment = TestEnvironment::new("root-exit-descendant");
    let heartbeat = environment.root.join("descendant-heartbeat.txt");
    let mut supervisor = PiSupervisor::start(environment.config()).expect("start fake Pi");
    let state = supervisor.wait_for_terminal(Duration::from_secs(3));
    assert!(matches!(state, SupervisorState::Stopped { .. }));
    wait_for_file(&heartbeat);
    supervisor.shutdown();
    assert_heartbeat_stops(&heartbeat);
}

#[test]
fn forced_shutdown_reaps_descendants() {
    let environment = TestEnvironment::new("ignore");
    let heartbeat = environment.root.join("descendant-heartbeat.txt");
    let mut supervisor = PiSupervisor::start(environment.config()).expect("start fake Pi");
    wait_for_output(&supervisor, b"fake_ready");
    wait_for_file(&heartbeat);

    let report = supervisor.shutdown();
    assert!(report.forced);
    assert!(matches!(
        supervisor.state(),
        SupervisorState::Stopped { forced: true, .. }
    ));
    assert_heartbeat_stops(&heartbeat);
}

#[test]
fn drop_raii_reaps_descendants() {
    let environment = TestEnvironment::new("ignore");
    let heartbeat = environment.root.join("descendant-heartbeat.txt");
    let supervisor = PiSupervisor::start(environment.config()).expect("start fake Pi");
    wait_for_output(&supervisor, b"fake_ready");
    wait_for_file(&heartbeat);

    drop(supervisor);
    assert_heartbeat_stops(&heartbeat);
}

#[test]
fn normal_child_close_is_stopped_not_failed() {
    let environment = TestEnvironment::new("exit-normal");
    let mut supervisor = PiSupervisor::start(environment.config()).expect("start fake Pi");
    let state = supervisor.wait_for_terminal(Duration::from_secs(3));
    assert!(matches!(
        state,
        SupervisorState::Stopped { forced: false, .. }
    ));
    supervisor.shutdown();
}

#[test]
fn missing_and_incompatible_pi_are_recoverable_start_errors() {
    let environment = TestEnvironment::new("normal");
    let mut missing = environment.config();
    missing.executable_override = Some(environment.root.join("missing pi.exe"));
    assert!(matches!(
        PiSupervisor::start(missing),
        Err(StartError::Discovery(DiscoveryError::MissingExplicit(_)))
    ));

    fs::write(
        environment
            .executable
            .parent()
            .expect("fake bin directory")
            .join("fake-version.txt"),
        "99.0.0",
    )
    .expect("write incompatible version");
    assert!(matches!(
        PiSupervisor::start(environment.config()),
        Err(StartError::Discovery(
            DiscoveryError::IncompatibleVersion { .. }
        ))
    ));
}

#[test]
fn embedded_nul_cannot_truncate_trailing_security_flags() {
    let environment = TestEnvironment::new("normal");
    let mut config = environment.config();
    config.session = SessionLaunch::Id("unsafe\0session".to_owned());

    assert!(matches!(
        PiSupervisor::start(config),
        Err(StartError::InvalidConfiguration(message))
            if message.contains("NUL")
    ));
    assert!(!environment.root.join("launch-args.txt").exists());
}

#[test]
fn timed_out_probe_reaps_its_descendants() {
    let environment = TestEnvironment::new("normal");
    let executable_directory = environment.executable.parent().expect("fake bin directory");
    fs::write(executable_directory.join("probe-hang"), "").expect("write probe marker");
    let heartbeat = executable_directory.join("descendant-heartbeat.txt");
    let mut config = environment.config();
    config.probe_timeout = Duration::from_secs(1);

    assert!(matches!(
        PiSupervisor::start(config),
        Err(StartError::Discovery(DiscoveryError::ProbeTimedOut { .. }))
    ));
    wait_for_file(&heartbeat);
    assert_heartbeat_stops(&heartbeat);
}

#[test]
fn missing_capability_is_recoverable_start_error() {
    let environment = TestEnvironment::new("normal");
    fs::write(
        environment
            .executable
            .parent()
            .expect("fake bin directory")
            .join("missing-capability"),
        "",
    )
    .expect("write capability marker");
    assert!(matches!(
        PiSupervisor::start(environment.config()),
        Err(StartError::Discovery(DiscoveryError::MissingCapabilities(
            _
        )))
    ));
}
