//! Discovery, compatibility probing, and lifecycle containment for an external Pi process.
//!
//! `Ready` means that the compatible child is contained and all three standard streams are being
//! drained. Protocol-level readiness still requires the correlated `get_state` probe added by the
//! RPC client; this module deliberately exposes no general-purpose write/request API.

mod diagnostics;
mod discovery;
mod platform;

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub use discovery::{
    DiscoveryError, ExecutableSource, PiCapabilities, PiInstallation, SUPPORTED_PI_VERSION,
    discover_and_probe,
};
pub use platform::ExitStatus;

use diagnostics::{StderrRing, drain_stderr};
use platform::{ProcessHandle, spawn_contained};

const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_STDERR_CAPACITY: usize = 64 * 1024;
const DEFAULT_STDOUT_QUEUE_CAPACITY: usize = 256;
const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const ABORT_RECORD: &[u8] = b"{\"type\":\"abort\",\"id\":\"pi-gui-shutdown\"}\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTrust {
    Approve,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLaunch {
    Ephemeral,
    Existing(PathBuf),
    Id(String),
    NewInDirectory(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePolicy {
    pub discover_extensions: bool,
    pub extensions: Vec<PathBuf>,
    pub discover_skills: bool,
    pub skills: Vec<PathBuf>,
    pub discover_prompt_templates: bool,
    pub prompt_templates: Vec<PathBuf>,
    pub discover_themes: bool,
    pub themes: Vec<PathBuf>,
    pub discover_context_files: bool,
}

impl ResourcePolicy {
    pub fn discover_all() -> Self {
        Self {
            discover_extensions: true,
            extensions: Vec::new(),
            discover_skills: true,
            skills: Vec::new(),
            discover_prompt_templates: true,
            prompt_templates: Vec::new(),
            discover_themes: true,
            themes: Vec::new(),
            discover_context_files: true,
        }
    }

    pub fn disabled() -> Self {
        Self {
            discover_extensions: false,
            extensions: Vec::new(),
            discover_skills: false,
            skills: Vec::new(),
            discover_prompt_templates: false,
            prompt_templates: Vec::new(),
            discover_themes: false,
            themes: Vec::new(),
            discover_context_files: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PiLaunchConfig {
    pub executable_override: Option<PathBuf>,
    pub working_directory: PathBuf,
    pub trust: ProjectTrust,
    pub session: SessionLaunch,
    pub resources: ResourcePolicy,
    pub offline: bool,
    pub disable_tools: bool,
    pub environment_overrides: Vec<(OsString, OsString)>,
    pub probe_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub stderr_capacity_bytes: usize,
    pub stdout_queue_capacity: usize,
}

impl PiLaunchConfig {
    pub fn new(
        working_directory: impl Into<PathBuf>,
        trust: ProjectTrust,
        session: SessionLaunch,
        resources: ResourcePolicy,
    ) -> Self {
        Self {
            executable_override: None,
            working_directory: working_directory.into(),
            trust,
            session,
            resources,
            offline: false,
            disable_tools: false,
            environment_overrides: Vec::new(),
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
            stderr_capacity_bytes: DEFAULT_STDERR_CAPACITY,
            stdout_queue_capacity: DEFAULT_STDOUT_QUEUE_CAPACITY,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessFailureKind {
    UnexpectedExit,
    EarlyStdoutEof,
    StdoutRead,
    StdoutBackpressure,
    StdinWrite,
    RpcFault,
    Wait,
    TerminationTimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessFailure {
    pub kind: ProcessFailureKind,
    pub message: String,
    pub exit_status: Option<ExitStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorState {
    Starting,
    Ready,
    Stopping,
    Stopped {
        exit_status: ExitStatus,
        forced: bool,
    },
    Failed(ProcessFailure),
}

impl SupervisorState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped { .. } | Self::Failed(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdoutEvent {
    Data(Vec<u8>),
    Eof,
    ReadError(String),
}

#[derive(Debug)]
pub enum StartError {
    Discovery(DiscoveryError),
    InvalidWorkingDirectory(PathBuf),
    CanonicalizeWorkingDirectory(io::Error),
    InvalidConfiguration(String),
    Spawn(io::Error),
    MissingPipe(&'static str),
}

impl fmt::Display for StartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => error.fmt(formatter),
            Self::InvalidWorkingDirectory(path) => write!(
                formatter,
                "the Pi working directory does not exist or is not a directory: {}",
                path.display()
            ),
            Self::CanonicalizeWorkingDirectory(error) => {
                write!(
                    formatter,
                    "could not canonicalize the Pi working directory: {error}"
                )
            }
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid Pi launch configuration: {message}")
            }
            Self::Spawn(error) => write!(formatter, "could not start Pi: {error}"),
            Self::MissingPipe(name) => write!(
                formatter,
                "Pi started without a supervised {name} pipe; the child was terminated"
            ),
        }
    }
}

impl std::error::Error for StartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Discovery(error) => Some(error),
            Self::CanonicalizeWorkingDirectory(error) | Self::Spawn(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DiscoveryError> for StartError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownReport {
    pub forced: bool,
    pub abort_sent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdinWriteError {
    Closed,
    TimedOut,
    Failed(String),
}

impl fmt::Display for StdinWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("Pi stdin is closed"),
            Self::TimedOut => formatter.write_str("timed out waiting for the Pi stdin writer"),
            Self::Failed(message) => write!(formatter, "could not write Pi stdin: {message}"),
        }
    }
}

impl std::error::Error for StdinWriteError {}

struct SharedState {
    state: Mutex<SupervisorState>,
    changed: Condvar,
    forced: AtomicBool,
}

impl SharedState {
    fn new() -> Self {
        Self {
            state: Mutex::new(SupervisorState::Starting),
            changed: Condvar::new(),
            forced: AtomicBool::new(false),
        }
    }

    fn snapshot(&self) -> SupervisorState {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn mark_ready(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(*state, SupervisorState::Starting) {
            *state = SupervisorState::Ready;
            self.changed.notify_all();
        }
    }

    fn begin_stopping(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(*state, SupervisorState::Starting | SupervisorState::Ready) {
            *state = SupervisorState::Stopping;
            self.changed.notify_all();
            true
        } else {
            false
        }
    }

    fn record_exit(&self, status: ExitStatus) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*state {
            SupervisorState::Stopping => {
                *state = SupervisorState::Stopped {
                    exit_status: status,
                    forced: self.forced.load(Ordering::Acquire),
                };
            }
            SupervisorState::Starting | SupervisorState::Ready if status.success() => {
                *state = SupervisorState::Stopped {
                    exit_status: status,
                    forced: false,
                };
            }
            SupervisorState::Starting | SupervisorState::Ready => {
                *state = SupervisorState::Failed(ProcessFailure {
                    kind: ProcessFailureKind::UnexpectedExit,
                    message: format!("Pi exited unexpectedly with {status}"),
                    exit_status: Some(status),
                });
            }
            SupervisorState::Stopped { .. } | SupervisorState::Failed(_) => return,
        }
        self.changed.notify_all();
    }

    fn fail(&self, failure: ProcessFailure) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(*state, SupervisorState::Starting | SupervisorState::Ready) {
            *state = SupervisorState::Failed(failure);
            self.changed.notify_all();
            true
        } else {
            false
        }
    }

    fn wait_terminal(&self, timeout: Duration) -> SupervisorState {
        let started = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while !state.is_terminal() {
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                break;
            };
            let result = self.changed.wait_timeout(state, remaining);
            let (next, timed_out) = match result {
                Ok((next, wait)) => (next, wait.timed_out()),
                Err(poisoned) => {
                    let (next, wait) = poisoned.into_inner();
                    (next, wait.timed_out())
                }
            };
            state = next;
            if timed_out {
                break;
            }
        }
        state.clone()
    }

    fn fail_termination_timeout(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.is_terminal() {
            *state = SupervisorState::Failed(ProcessFailure {
                kind: ProcessFailureKind::TerminationTimedOut,
                message: "Pi did not exit after its process tree was terminated".to_owned(),
                exit_status: None,
            });
            self.changed.notify_all();
        }
    }
}

enum StdinControl {
    Write {
        record: Vec<u8>,
        acknowledge: mpsc::Sender<Result<(), String>>,
    },
    AbortAndClose(mpsc::Sender<bool>),
}

pub struct PiSupervisor {
    installation: PiInstallation,
    canonical_working_directory: PathBuf,
    shared: Arc<SharedState>,
    process: Arc<ProcessHandle>,
    stderr: Arc<Mutex<StderrRing>>,
    stdout: Option<mpsc::Receiver<StdoutEvent>>,
    stdin_control: Option<mpsc::Sender<StdinControl>>,
    threads: Vec<JoinHandle<()>>,
    shutdown_timeout: Duration,
}

impl PiSupervisor {
    pub fn start(config: PiLaunchConfig) -> Result<Self, StartError> {
        validate_config(&config)?;
        if !config.working_directory.is_dir() {
            return Err(StartError::InvalidWorkingDirectory(
                config.working_directory.clone(),
            ));
        }
        let canonical_working_directory = fs::canonicalize(&config.working_directory)
            .map_err(StartError::CanonicalizeWorkingDirectory)?;
        let installation =
            discover_and_probe(config.executable_override.as_deref(), config.probe_timeout)?;
        let arguments = launch_arguments(&installation, &config);
        if arguments.iter().any(|argument| contains_nul(argument)) {
            return Err(StartError::InvalidConfiguration(
                "launch arguments must not contain NUL characters".to_owned(),
            ));
        }
        let mut child = spawn_contained(
            &installation.executable,
            &arguments,
            &canonical_working_directory,
            &config.environment_overrides,
        )
        .map_err(StartError::Spawn)?;
        let process = Arc::new(child.handle);

        let stdin = take_pipe(&process, child.stdin.take(), "stdin")?;
        let stdout_pipe = take_pipe(&process, child.stdout.take(), "stdout")?;
        let stderr_pipe = take_pipe(&process, child.stderr.take(), "stderr")?;
        let shared = Arc::new(SharedState::new());
        let stderr = Arc::new(Mutex::new(StderrRing::new(config.stderr_capacity_bytes)));
        let (stdout_sender, stdout) = mpsc::sync_channel(config.stdout_queue_capacity);
        let (stdin_sender, stdin_receiver) = mpsc::channel();

        let mut threads = Vec::with_capacity(4);
        threads.push(spawn_stdin_worker(
            stdin,
            stdin_receiver,
            Arc::clone(&process),
            Arc::clone(&shared),
        ));
        threads.push(spawn_stdout_worker(
            stdout_pipe,
            stdout_sender,
            Arc::clone(&process),
            Arc::clone(&shared),
        ));
        let stderr_ring = Arc::clone(&stderr);
        threads.push(thread::spawn(move || {
            drain_stderr(stderr_pipe, stderr_ring)
        }));
        threads.push(spawn_wait_worker(Arc::clone(&process), Arc::clone(&shared)));
        shared.mark_ready();

        Ok(Self {
            installation,
            canonical_working_directory,
            shared,
            process,
            stderr,
            stdout: Some(stdout),
            stdin_control: Some(stdin_sender),
            threads,
            shutdown_timeout: config.shutdown_timeout,
        })
    }

    pub fn installation(&self) -> &PiInstallation {
        &self.installation
    }

    pub fn working_directory(&self) -> &Path {
        &self.canonical_working_directory
    }

    pub fn state(&self) -> SupervisorState {
        self.shared.snapshot()
    }

    pub fn stdout(&self) -> &mpsc::Receiver<StdoutEvent> {
        self.stdout
            .as_ref()
            .expect("stdout was transferred to the RPC client")
    }

    pub fn take_stdout(&mut self) -> Option<mpsc::Receiver<StdoutEvent>> {
        self.stdout.take()
    }

    pub fn write_record(&self, record: Vec<u8>) -> Result<(), StdinWriteError> {
        self.write_record_with_timeout(record, DEFAULT_WRITE_TIMEOUT)
    }

    pub fn write_record_with_timeout(
        &self,
        record: Vec<u8>,
        timeout: Duration,
    ) -> Result<(), StdinWriteError> {
        let Some(sender) = &self.stdin_control else {
            return Err(StdinWriteError::Closed);
        };
        let (acknowledge, result) = mpsc::channel();
        sender
            .send(StdinControl::Write {
                record,
                acknowledge,
            })
            .map_err(|_| StdinWriteError::Closed)?;
        result
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => StdinWriteError::TimedOut,
                mpsc::RecvTimeoutError::Disconnected => StdinWriteError::Closed,
            })?
            .map_err(StdinWriteError::Failed)
    }

    pub(crate) fn terminate_due_to_rpc_fault(&self) {
        if self.shared.fail(ProcessFailure {
            kind: ProcessFailureKind::RpcFault,
            message: "Pi RPC connection failed and was closed".to_owned(),
            exit_status: None,
        }) {
            let _ = self.process.terminate();
        }
    }

    pub fn stderr_snapshot(&self) -> Vec<String> {
        self.stderr
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot()
    }

    pub fn wait_for_terminal(&self, timeout: Duration) -> SupervisorState {
        self.shared.wait_terminal(timeout)
    }

    pub fn shutdown(&mut self) -> ShutdownReport {
        if self.shared.snapshot().is_terminal() {
            self.stdin_control.take();
            self.join_threads();
            return ShutdownReport {
                forced: false,
                abort_sent: false,
            };
        }

        self.shared.begin_stopping();
        let abort_sent = self.close_stdin_with_abort();
        let mut state = self.shared.wait_terminal(self.shutdown_timeout);
        let forced = !state.is_terminal();
        if forced {
            self.shared.forced.store(true, Ordering::Release);
            let _ = self.process.terminate();
            state = self.shared.wait_terminal(Duration::from_secs(2));
            if !state.is_terminal() {
                self.shared.fail_termination_timeout();
            }
        }
        self.join_threads();
        ShutdownReport { forced, abort_sent }
    }

    fn close_stdin_with_abort(&mut self) -> bool {
        let Some(sender) = self.stdin_control.take() else {
            return false;
        };
        let (acknowledge, result) = mpsc::channel();
        if sender
            .send(StdinControl::AbortAndClose(acknowledge))
            .is_err()
        {
            return false;
        }
        result
            .recv_timeout(Duration::from_millis(250))
            .unwrap_or(false)
    }

    fn join_threads(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.threads.iter().any(|thread| !thread.is_finished()) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        for worker in self.threads.drain(..) {
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

impl Drop for PiSupervisor {
    fn drop(&mut self) {
        if self.shared.snapshot().is_terminal() {
            return;
        }
        self.shared.begin_stopping();
        if let Some(sender) = self.stdin_control.take() {
            let (acknowledge, _result) = mpsc::channel();
            let _ = sender.send(StdinControl::AbortAndClose(acknowledge));
        }
        self.shared.forced.store(true, Ordering::Release);
        let _ = self.process.terminate();
        let _ = self.process.wait_for(Duration::from_secs(2));
    }
}

fn validate_config(config: &PiLaunchConfig) -> Result<(), StartError> {
    if config.probe_timeout.is_zero() {
        return Err(StartError::InvalidConfiguration(
            "the compatibility probe timeout must be greater than zero".to_owned(),
        ));
    }
    if config.shutdown_timeout.is_zero() {
        return Err(StartError::InvalidConfiguration(
            "the shutdown timeout must be greater than zero".to_owned(),
        ));
    }
    if config.stdout_queue_capacity == 0 {
        return Err(StartError::InvalidConfiguration(
            "the stdout queue capacity must be greater than zero".to_owned(),
        ));
    }
    if matches!(&config.session, SessionLaunch::Id(id) if id.trim().is_empty()) {
        return Err(StartError::InvalidConfiguration(
            "the session ID must not be empty".to_owned(),
        ));
    }
    for (name, value) in &config.environment_overrides {
        if name.is_empty() || contains_nul(name) || contains_equals(name) || contains_nul(value) {
            return Err(StartError::InvalidConfiguration(
                "environment overrides require a non-empty key without `=` or NUL and a value without NUL"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn contains_nul(value: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;

    value.encode_wide().any(|character| character == 0)
}

#[cfg(windows)]
fn contains_equals(value: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;

    value
        .encode_wide()
        .any(|character| character == b'=' as u16)
}

#[cfg(unix)]
fn contains_nul(value: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;

    value.as_bytes().contains(&0)
}

#[cfg(unix)]
fn contains_equals(value: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;

    value.as_bytes().contains(&b'=')
}

#[cfg(not(any(windows, unix)))]
fn contains_nul(value: &OsStr) -> bool {
    value.to_string_lossy().contains('\0')
}

#[cfg(not(any(windows, unix)))]
fn contains_equals(value: &OsStr) -> bool {
    value.to_string_lossy().contains('=')
}

fn launch_arguments(installation: &PiInstallation, config: &PiLaunchConfig) -> Vec<OsString> {
    let mut arguments = installation.launcher_arguments.clone();
    arguments.extend([OsString::from("--mode"), OsString::from("rpc")]);
    arguments.push(match config.trust {
        ProjectTrust::Approve => OsString::from("--approve"),
        ProjectTrust::Reject => OsString::from("--no-approve"),
    });
    if config.offline {
        arguments.push(OsString::from("--offline"));
    }
    if config.disable_tools {
        arguments.push(OsString::from("--no-tools"));
    }
    match &config.session {
        SessionLaunch::Ephemeral => arguments.push(OsString::from("--no-session")),
        SessionLaunch::Existing(path) => {
            arguments.push(OsString::from("--session"));
            arguments.push(path.as_os_str().to_owned());
        }
        SessionLaunch::Id(id) => {
            arguments.push(OsString::from("--session-id"));
            arguments.push(OsString::from(id));
        }
        SessionLaunch::NewInDirectory(path) => {
            arguments.push(OsString::from("--session-dir"));
            arguments.push(path.as_os_str().to_owned());
        }
    }
    append_resource_arguments(&mut arguments, &config.resources);
    arguments
}

fn append_resource_arguments(arguments: &mut Vec<OsString>, resources: &ResourcePolicy) {
    append_resource_kind(
        arguments,
        resources.discover_extensions,
        "--no-extensions",
        "--extension",
        &resources.extensions,
    );
    append_resource_kind(
        arguments,
        resources.discover_skills,
        "--no-skills",
        "--skill",
        &resources.skills,
    );
    append_resource_kind(
        arguments,
        resources.discover_prompt_templates,
        "--no-prompt-templates",
        "--prompt-template",
        &resources.prompt_templates,
    );
    append_resource_kind(
        arguments,
        resources.discover_themes,
        "--no-themes",
        "--theme",
        &resources.themes,
    );
    if !resources.discover_context_files {
        arguments.push(OsString::from("--no-context-files"));
    }
}

fn append_resource_kind(
    arguments: &mut Vec<OsString>,
    discover: bool,
    disable_flag: &str,
    explicit_flag: &str,
    paths: &[PathBuf],
) {
    if !discover {
        arguments.push(OsString::from(disable_flag));
    }
    for path in paths {
        arguments.push(OsString::from(explicit_flag));
        arguments.push(path.as_os_str().to_owned());
    }
}

fn take_pipe<T>(
    process: &ProcessHandle,
    pipe: Option<T>,
    name: &'static str,
) -> Result<T, StartError> {
    pipe.ok_or_else(|| {
        let _ = process.terminate();
        StartError::MissingPipe(name)
    })
}

fn spawn_stdin_worker(
    mut stdin: Box<dyn Write + Send>,
    receiver: mpsc::Receiver<StdinControl>,
    process: Arc<ProcessHandle>,
    shared: Arc<SharedState>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(control) = receiver.recv() {
            match control {
                StdinControl::Write {
                    record,
                    acknowledge,
                } => {
                    let result = stdin
                        .write_all(&record)
                        .and_then(|()| stdin.flush())
                        .map_err(|error| error.to_string());
                    let failed = result.is_err();
                    let _ = acknowledge.send(result);
                    if failed {
                        if shared.fail(ProcessFailure {
                            kind: ProcessFailureKind::StdinWrite,
                            message: "could not write to Pi stdin".to_owned(),
                            exit_status: None,
                        }) {
                            let _ = process.terminate();
                        }
                        return;
                    }
                }
                StdinControl::AbortAndClose(acknowledge) => {
                    let sent = stdin
                        .write_all(ABORT_RECORD)
                        .and_then(|()| stdin.flush())
                        .is_ok();
                    let _ = acknowledge.send(sent);
                    return;
                }
            }
        }
    })
}

fn spawn_stdout_worker(
    mut stdout: Box<dyn Read + Send>,
    sender: mpsc::SyncSender<StdoutEvent>,
    process: Arc<ProcessHandle>,
    shared: Arc<SharedState>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) => {
                    let _ = sender.try_send(StdoutEvent::Eof);
                    handle_stdout_eof(&process, &shared);
                    return;
                }
                Ok(count) => {
                    if matches!(
                        sender.try_send(StdoutEvent::Data(buffer[..count].to_vec())),
                        Err(mpsc::TrySendError::Full(_))
                    ) && shared.fail(ProcessFailure {
                        kind: ProcessFailureKind::StdoutBackpressure,
                        message: "Pi stdout exceeded the bounded transport queue".to_owned(),
                        exit_status: None,
                    }) {
                        let _ = process.terminate();
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    let _ = sender.try_send(StdoutEvent::ReadError(message.clone()));
                    if shared.fail(ProcessFailure {
                        kind: ProcessFailureKind::StdoutRead,
                        message: format!("could not read Pi stdout: {message}"),
                        exit_status: None,
                    }) {
                        let _ = process.terminate();
                    }
                    return;
                }
            }
        }
    })
}

fn handle_stdout_eof(process: &ProcessHandle, shared: &SharedState) {
    for _ in 0..10 {
        match process.try_wait() {
            Ok(Some(status)) => {
                let _ = process.terminate();
                shared.record_exit(status);
                return;
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                if shared.fail(ProcessFailure {
                    kind: ProcessFailureKind::Wait,
                    message: format!("could not inspect Pi after stdout closed: {error}"),
                    exit_status: None,
                }) {
                    let _ = process.terminate();
                }
                return;
            }
        }
    }

    if shared.fail(ProcessFailure {
        kind: ProcessFailureKind::EarlyStdoutEof,
        message: "Pi closed stdout while its process was still running".to_owned(),
        exit_status: None,
    }) {
        let _ = process.terminate();
    }
}

fn spawn_wait_worker(process: Arc<ProcessHandle>, shared: Arc<SharedState>) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match process.try_wait() {
                Ok(Some(status)) => {
                    let _ = process.terminate();
                    shared.record_exit(status);
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    if shared.fail(ProcessFailure {
                        kind: ProcessFailureKind::Wait,
                        message: format!("could not wait for Pi: {error}"),
                        exit_status: None,
                    }) {
                        let _ = process.terminate();
                    }
                    return;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_arguments_make_security_and_resource_choices_explicit() {
        let installation = PiInstallation {
            executable: PathBuf::from("pi"),
            launcher_arguments: Vec::new(),
            source: ExecutableSource::Explicit,
            version: SUPPORTED_PI_VERSION.to_owned(),
            capabilities: PiCapabilities {
                rpc_mode: true,
                explicit_trust: true,
                explicit_session: true,
                resource_controls: true,
            },
        };
        let config = PiLaunchConfig::new(
            ".",
            ProjectTrust::Reject,
            SessionLaunch::Ephemeral,
            ResourcePolicy::disabled(),
        );
        let arguments = launch_arguments(&installation, &config);
        let strings = arguments
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            &strings[..4],
            ["--mode", "rpc", "--no-approve", "--no-session"]
        );
        assert!(strings.contains(&"--no-extensions".to_owned()));
        assert!(strings.contains(&"--no-skills".to_owned()));
        assert!(strings.contains(&"--no-prompt-templates".to_owned()));
        assert!(strings.contains(&"--no-themes".to_owned()));
        assert!(strings.contains(&"--no-context-files".to_owned()));
        assert!(!strings.contains(&"--approve".to_owned()));
    }

    #[test]
    fn state_does_not_overwrite_a_failure_with_exit() {
        let shared = SharedState::new();
        shared.mark_ready();
        assert!(shared.fail(ProcessFailure {
            kind: ProcessFailureKind::EarlyStdoutEof,
            message: "early EOF".to_owned(),
            exit_status: None,
        }));
        shared.record_exit(ExitStatus::from_code(Some(1)));
        assert!(matches!(shared.snapshot(), SupervisorState::Failed(_)));
    }
}
