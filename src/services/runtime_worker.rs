//! Blocking Pi runtime work isolated behind a message-driven standard-thread boundary.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use async_channel::{Receiver, Sender};

use super::pi_process::{
    DiscoveryError, PiLaunchConfig, ProjectTrust, ResourcePolicy, SessionLaunch, StartError,
};
use super::rpc::{
    ConnectionGeneration, ConnectionStatus, RpcClient, RpcClientError, RpcClientErrorKind,
    RpcClientStartError, RpcDispatch, SessionEpoch, disconnected_input, dispatch_for_effect,
    normalize_call_result, normalize_tagged_record,
};
use crate::state::runtime::{NormalizedEvent, RuntimeEffect, RuntimeInput, StampedInput};

const COORDINATOR_TICK: Duration = Duration::from_millis(10);
const EVENT_POLL: Duration = Duration::from_millis(50);
const MAX_PENDING_RUNTIME_EVENTS: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttemptGeneration(u64);

impl AttemptGeneration {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStartFailureKind {
    MissingPi,
    IncompatiblePi,
    Readiness,
    Launch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStartFailure {
    pub kind: RuntimeStartFailureKind,
    pub summary: String,
}

impl RuntimeStartFailure {
    pub fn new(kind: RuntimeStartFailureKind, summary: impl Into<String>) -> Self {
        Self {
            kind,
            summary: summary.into(),
        }
    }
}

impl fmt::Display for RuntimeStartFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.summary.fmt(formatter)
    }
}

impl std::error::Error for RuntimeStartFailure {}

pub enum RuntimePoll {
    Input(Box<StampedInput>),
    Timeout,
    Closed,
}

/// Injectable, GPUI-independent startup boundary.
pub trait RuntimeService: Send + Sync + 'static {
    fn connect(
        &self,
        generation: ConnectionGeneration,
    ) -> Result<Arc<dyn RuntimeConnection>, RuntimeStartFailure>;

    fn set_resume_session(&self, _session_file: Option<std::path::PathBuf>) {}
}

/// One connected runtime. Every method may block and is called only on worker threads.
pub trait RuntimeConnection: Send + Sync + 'static {
    fn prepare(&self, _effect: &RuntimeEffect) {}
    fn execute(&self, effect: RuntimeEffect) -> Option<StampedInput>;
    fn poll(&self, epoch: SessionEpoch, timeout: Duration) -> RuntimePoll;
    fn stop(&self);
}

pub struct RpcRuntimeService {
    config: PiLaunchConfig,
    resume_session: Mutex<Option<std::path::PathBuf>>,
}

impl RpcRuntimeService {
    pub fn new(config: PiLaunchConfig) -> Self {
        Self {
            config,
            resume_session: Mutex::new(None),
        }
    }

    pub fn persisted_profile_with_endpoint(
        working_directory: impl Into<std::path::PathBuf>,
        session_directory: impl Into<std::path::PathBuf>,
        orchestration_endpoint: String,
    ) -> Self {
        let working_directory = working_directory.into();
        let mut config = PiLaunchConfig::new(
            working_directory.clone(),
            ProjectTrust::Reject,
            SessionLaunch::NewInDirectory(session_directory.into()),
            ResourcePolicy::command_sources(),
        );
        config.disable_tools = false;
        config.offline = false;
        attach_orchestration_adapter(&mut config, orchestration_endpoint);
        apply_native_shell_extension_policy(&mut config, &working_directory);
        Self::new(config)
    }
}

fn attach_orchestration_adapter(config: &mut PiLaunchConfig, orchestration_endpoint: String) {
    let adapter = crate::services::sdk_bridge::orchestration_adapter_path();
    if adapter.is_file() {
        config.resources.extensions.push(adapter);
    }
    config.environment_overrides.push((
        crate::services::sdk_bridge::ORCHESTRATION_PIPE_ENV.into(),
        orchestration_endpoint.into(),
    ));
}

/// Omit TUI-chrome extensions that native GPUI replaces, without changing installed Pi files.
fn apply_native_shell_extension_policy(
    config: &mut PiLaunchConfig,
    working_directory: &std::path::Path,
) {
    let agent_dir = super::pi_process::resolve_agent_dir(working_directory);
    super::pi_process::apply_gui_extension_policy(&mut config.resources, &agent_dir);
}

impl RuntimeService for RpcRuntimeService {
    fn connect(
        &self,
        generation: ConnectionGeneration,
    ) -> Result<Arc<dyn RuntimeConnection>, RuntimeStartFailure> {
        let mut config = self.config.clone();
        if let Some(session_file) = self
            .resume_session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            config.session = SessionLaunch::Existing(session_file);
        }
        let client = RpcClient::start(config).map_err(start_failure)?;
        Ok(Arc::new(RpcRuntimeConnection {
            client,
            generation,
            disconnect_reported: AtomicBool::new(false),
        }))
    }

    fn set_resume_session(&self, session_file: Option<std::path::PathBuf>) {
        *self
            .resume_session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = session_file;
    }
}

struct RpcRuntimeConnection {
    client: RpcClient,
    generation: ConnectionGeneration,
    disconnect_reported: AtomicBool,
}

impl RuntimeConnection for RpcRuntimeConnection {
    fn prepare(&self, effect: &RuntimeEffect) {
        let stop_before = matches!(&effect.effect,
            crate::state::runtime::EffectKind::Request(crate::state::runtime::RuntimeRequest::ClearQueue { .. } | crate::state::runtime::RuntimeRequest::ClearQueuedInputs { .. }))
            .then_some(effect.sequence);
        self.client.prepare_effect(effect.epoch, stop_before);
    }

    fn execute(&self, effect: RuntimeEffect) -> Option<StampedInput> {
        match dispatch_for_effect(&effect) {
            RpcDispatch::Command(command) => {
                let id = match &effect.effect {
                    crate::state::runtime::EffectKind::Request(
                        crate::state::runtime::RuntimeRequest::ExecuteBash { request, .. },
                    ) => Some(request.clone()),
                    _ => None,
                };
                let call = self.client.request_effect(id, command, effect.epoch, effect.sequence);
                normalize_call_result(&effect, call.wait())
            }
            RpcDispatch::ExtensionUiResponse(response) => {
                let _ = self.client.send_extension_ui_response(response);
                None
            }
        }
    }

    fn poll(&self, epoch: SessionEpoch, timeout: Duration) -> RuntimePoll {
        match self.client.recv_notification_timeout(timeout) {
            Ok(record) => {
                let mut input = match normalize_tagged_record(record, epoch) {
                    Some(input) => input,
                    None => return RuntimePoll::Timeout,
                };
                input.generation = self.generation;
                RuntimePoll::Input(Box::new(input))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => RuntimePoll::Closed,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let kind = match self.client.status() {
                    ConnectionStatus::Faulted(kind) => Some(kind),
                    ConnectionStatus::Stopped => Some(RpcClientErrorKind::Stopped),
                    ConnectionStatus::Starting | ConnectionStatus::Ready => None,
                };
                let Some(kind) = kind else {
                    return RuntimePoll::Timeout;
                };
                if self.disconnect_reported.swap(true, Ordering::AcqRel) {
                    return RuntimePoll::Closed;
                }
                let mut input = disconnected_input(
                    RpcClientError {
                        kind,
                        generation: self.generation,
                        operation: None,
                    },
                    epoch,
                );
                input.generation = self.generation;
                RuntimePoll::Input(Box::new(input))
            }
        }
    }

    fn stop(&self) {
        self.client.stop();
    }
}

fn start_failure(error: RpcClientStartError) -> RuntimeStartFailure {
    match error {
        RpcClientStartError::Process(StartError::Discovery(
            DiscoveryError::MissingExplicit(_)
            | DiscoveryError::NotAFile(_)
            | DiscoveryError::NotExecutable(_)
            | DiscoveryError::MissingFromPath,
        )) => RuntimeStartFailure::new(
            RuntimeStartFailureKind::MissingPi,
            "Pi was not found. Install the supported Pi CLI, then retry.",
        ),
        RpcClientStartError::Process(StartError::Discovery(
            DiscoveryError::IncompatibleNode(_),
        )) => RuntimeStartFailure::new(
            RuntimeStartFailureKind::IncompatiblePi,
            "Pi requires Node 22.19.0 or newer. Update Node, then reconnect.",
        ),
        RpcClientStartError::Process(StartError::Discovery(
            DiscoveryError::IncompatibleVersion { .. }
            | DiscoveryError::InvalidNpmInstall { .. }
            | DiscoveryError::MissingCapabilities(_),
        )) => RuntimeStartFailure::new(
            RuntimeStartFailureKind::IncompatiblePi,
            format!(
                "Install @earendil-works/pi-coding-agent@{}, then reconnect. The current installation does not meet this build's contract.",
                super::pi_process::SUPPORTED_PI_VERSION,
            ),
        ),
        RpcClientStartError::Readiness(_) | RpcClientStartError::ReadinessRejected => {
            RuntimeStartFailure::new(
                RuntimeStartFailureKind::Readiness,
                "Pi started but did not become ready.",
            )
        }
        RpcClientStartError::InvalidDeadlines | RpcClientStartError::Process(_) => {
            RuntimeStartFailure::new(RuntimeStartFailureKind::Launch, "Pi could not be started.")
        }
    }
}

#[derive(Debug, Clone)]
pub enum WorkerResult {
    Connecting {
        attempt: AttemptGeneration,
        generation: ConnectionGeneration,
    },
    Connected {
        attempt: AttemptGeneration,
        generation: ConnectionGeneration,
    },
    Input {
        attempt: AttemptGeneration,
        input: Box<StampedInput>,
    },
    ConnectionFailed {
        attempt: AttemptGeneration,
        generation: ConnectionGeneration,
        failure: RuntimeStartFailure,
    },
    Stopped {
        attempt: AttemptGeneration,
    },
}

// Keep the coordinator responsive when the UI is paused. Non-replaceable
// records retain order in this bounded outbox; the internal producers provide
// backpressure once it fills. Stop and shutdown never wait for a UI receive.
struct ResultOutbox {
    sender: Sender<WorkerResult>,
    pending: VecDeque<WorkerResult>,
}

impl ResultOutbox {
    fn flush(&mut self) {
        while let Some(result) = self.pending.pop_front() {
            match self.sender.try_send(result) {
                Ok(()) => {}
                Err(async_channel::TrySendError::Full(result)) => {
                    self.pending.push_front(result);
                    break;
                }
                Err(async_channel::TrySendError::Closed(_)) => {
                    self.pending.clear();
                    break;
                }
            }
        }
    }

    fn send(&mut self, result: WorkerResult) {
        self.flush();
        let result = if self.pending.is_empty() {
            match self.sender.try_send(result) {
                Ok(()) | Err(async_channel::TrySendError::Closed(_)) => return,
                Err(async_channel::TrySendError::Full(result)) => result,
            }
        } else {
            result
        };
        if matches!(&result, WorkerResult::Input { input, .. }
            if matches!(&input.input, RuntimeInput::Event(
                NormalizedEvent::MessageUpdate(_) | NormalizedEvent::ToolUpdate { .. })))
        {
            return;
        }
        self.pending.push_back(result);
    }
}

enum WorkerCommand {
    Connect {
        attempt: AttemptGeneration,
        generation: ConnectionGeneration,
    },
    Effect {
        attempt: AttemptGeneration,
        effect: RuntimeEffect,
    },
    Stop,
    Shutdown,
}

enum InternalResult {
    Connected {
        attempt: AttemptGeneration,
        generation: ConnectionGeneration,
        connection: Arc<dyn RuntimeConnection>,
    },
    ConnectionFailed {
        attempt: AttemptGeneration,
        generation: ConnectionGeneration,
        failure: RuntimeStartFailure,
    },
    Input {
        attempt: AttemptGeneration,
        input: Box<StampedInput>,
    },
    PollClosed {
        attempt: AttemptGeneration,
    },
    StopCompleted {
        attempt: AttemptGeneration,
    },
}

struct ActiveConnection {
    attempt: AttemptGeneration,
    connection: Arc<dyn RuntimeConnection>,
    epoch: Arc<AtomicU64>,
    cancelled: Arc<AtomicBool>,
}

pub struct RuntimeWorkerHandle {
    commands: mpsc::Sender<WorkerCommand>,
    results: Receiver<WorkerResult>,
    shutdown_requested: Arc<AtomicBool>,
}

impl RuntimeWorkerHandle {
    pub fn spawn(service: Arc<dyn RuntimeService>) -> Self {
        let (commands, command_receiver) = mpsc::channel();
        let (results, result_receiver) = async_channel::bounded(MAX_PENDING_RUNTIME_EVENTS);
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        thread::spawn(move || coordinator(service, command_receiver, results));
        Self {
            commands,
            results: result_receiver,
            shutdown_requested,
        }
    }

    pub fn results(&self) -> Receiver<WorkerResult> {
        self.results.clone()
    }

    pub fn connect(&self, attempt: AttemptGeneration, generation: ConnectionGeneration) -> bool {
        self.commands
            .send(WorkerCommand::Connect {
                attempt,
                generation,
            })
            .is_ok()
    }

    pub fn execute(&self, attempt: AttemptGeneration, effect: RuntimeEffect) -> bool {
        self.commands
            .send(WorkerCommand::Effect { attempt, effect })
            .is_ok()
    }

    pub fn stop(&self) -> bool {
        self.commands.send(WorkerCommand::Stop).is_ok()
    }

    pub fn request_shutdown(&self) -> bool {
        if self.shutdown_requested.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.results.close();
        self.commands.send(WorkerCommand::Shutdown).is_ok()
    }
}

impl Drop for RuntimeWorkerHandle {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

fn coordinator(
    service: Arc<dyn RuntimeService>,
    commands: mpsc::Receiver<WorkerCommand>,
    results: Sender<WorkerResult>,
) {
    let (internal_sender, internal_receiver) = mpsc::sync_channel(MAX_PENDING_RUNTIME_EVENTS);
    let mut results = ResultOutbox {
        sender: results,
        pending: VecDeque::new(),
    };
    let mut desired_attempt = None;
    let mut active: Option<ActiveConnection> = None;

    loop {
        results.flush();
        if results.sender.is_closed() {
            stop_active(&mut active);
            return;
        }
        for _ in 0..32 {
            if results.pending.len() >= MAX_PENDING_RUNTIME_EVENTS {
                break;
            }
            let Ok(result) = internal_receiver.try_recv() else { break };
            handle_internal(
                result,
                desired_attempt,
                &mut active,
                &mut results,
                &internal_sender,
            );
        }

        let command = match commands.recv_timeout(COORDINATOR_TICK) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => WorkerCommand::Shutdown,
        };
        match command {
            WorkerCommand::Connect {
                attempt,
                generation,
            } => {
                desired_attempt = Some(attempt);
                stop_active(&mut active);
                results.pending.clear();
                results.send(WorkerResult::Connecting { attempt, generation });
                let service = Arc::clone(&service);
                let internal = internal_sender.clone();
                thread::spawn(move || match service.connect(generation) {
                    Ok(connection) => {
                        if let Err(error) = internal.send(InternalResult::Connected {
                            attempt,
                            generation,
                            connection,
                        }) && let InternalResult::Connected { connection, .. } = error.0
                        {
                            connection.stop();
                        }
                    }
                    Err(failure) => {
                        let _ = internal.send(InternalResult::ConnectionFailed {
                            attempt,
                            generation,
                            failure,
                        });
                    }
                });
            }
            WorkerCommand::Effect { attempt, effect } => {
                let Some(current) = active.as_ref().filter(|current| current.attempt == attempt)
                else {
                    continue;
                };
                current.epoch.fetch_max(effect.epoch.value(), Ordering::AcqRel);
                current.connection.prepare(&effect);
                let connection = Arc::clone(&current.connection);
                let internal = internal_sender.clone();
                thread::spawn(move || {
                    if let Some(input) = connection.execute(effect) {
                        let _ = internal.send(InternalResult::Input {
                            attempt,
                            input: Box::new(input),
                        });
                    }
                });
            }
            WorkerCommand::Stop => {
                results.pending.clear();
                let stopped_attempt = desired_attempt.take();
                if let Some(active_connection) = active.take() {
                    active_connection.cancelled.store(true, Ordering::Release);
                    let connection = active_connection.connection;
                    let internal = internal_sender.clone();
                    thread::spawn(move || {
                        connection.stop();
                        let _ = internal.send(InternalResult::StopCompleted {
                            attempt: active_connection.attempt,
                        });
                    });
                } else if let Some(attempt) = stopped_attempt {
                    results.send(WorkerResult::Stopped { attempt });
                }
            }
            WorkerCommand::Shutdown => {
                stop_active(&mut active);
                return;
            }
        }
    }
}

fn handle_internal(
    result: InternalResult,
    desired_attempt: Option<AttemptGeneration>,
    active: &mut Option<ActiveConnection>,
    results: &mut ResultOutbox,
    internal_sender: &mpsc::SyncSender<InternalResult>,
) {
    match result {
        InternalResult::Connected {
            attempt,
            generation,
            connection,
        } => {
            if desired_attempt != Some(attempt) {
                stop_connection(connection);
                return;
            }
            stop_active(active);
            let epoch = Arc::new(AtomicU64::new(SessionEpoch::default().value()));
            let cancelled = Arc::new(AtomicBool::new(false));
            spawn_event_pump(
                attempt,
                Arc::clone(&connection),
                Arc::clone(&epoch),
                Arc::clone(&cancelled),
                internal_sender.clone(),
            );
            *active = Some(ActiveConnection {
                attempt,
                connection,
                epoch,
                cancelled,
            });
            results.send(WorkerResult::Connected { attempt, generation });
        }
        InternalResult::ConnectionFailed {
            attempt,
            generation,
            failure,
        } => {
            if desired_attempt == Some(attempt) {
                results.send(WorkerResult::ConnectionFailed { attempt, generation, failure });
            }
        }
        InternalResult::Input { attempt, input } => {
            if desired_attempt == Some(attempt)
                && active
                    .as_ref()
                    .is_some_and(|active| active.attempt == attempt)
            {
                results.send(WorkerResult::Input { attempt, input });
            }
        }
        InternalResult::PollClosed { attempt } => {
            if desired_attempt == Some(attempt)
                && active
                    .as_ref()
                    .is_some_and(|active| active.attempt == attempt)
            {
                stop_active(active);
            }
        }
        InternalResult::StopCompleted { attempt } => {
            results.send(WorkerResult::Stopped { attempt });
        }
    }
}

fn spawn_event_pump(
    attempt: AttemptGeneration,
    connection: Arc<dyn RuntimeConnection>,
    epoch: Arc<AtomicU64>,
    cancelled: Arc<AtomicBool>,
    internal: mpsc::SyncSender<InternalResult>,
) {
    thread::spawn(move || {
        while !cancelled.load(Ordering::Acquire) {
            let epoch = SessionEpoch::new(epoch.load(Ordering::Acquire));
            match connection.poll(epoch, EVENT_POLL) {
                RuntimePoll::Input(input) => {
                    if internal
                        .send(InternalResult::Input { attempt, input })
                        .is_err()
                    {
                        connection.stop();
                        return;
                    }
                }
                RuntimePoll::Timeout => {}
                RuntimePoll::Closed => {
                    let _ = internal.send(InternalResult::PollClosed { attempt });
                    return;
                }
            }
        }
    });
}

fn stop_active(active: &mut Option<ActiveConnection>) {
    if let Some(active) = active.take() {
        active.cancelled.store(true, Ordering::Release);
        stop_connection(active.connection);
    }
}

fn stop_connection(connection: Arc<dyn RuntimeConnection>) {
    thread::spawn(move || connection.stop());
}
