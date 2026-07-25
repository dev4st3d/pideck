use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{
    Command, ConnectionGeneration, ExtensionUiResponse, IncomingRecord, JsonlCodec, OutboundRecord,
    RequestId, ResponseResult, RpcCommand, RpcEvent, RpcResponse, SessionState, encode_record,
};
use crate::services::pi_process::{
    PiLaunchConfig, PiSupervisor, ProcessFailureKind, ShutdownReport, StartError, SupervisorState,
};

const MAX_PENDING_NOTIFICATIONS: usize = 256;

trait RecoverPoison<T> {
    fn recover_poison(self) -> T;
}

impl<T> RecoverPoison<T> for std::sync::LockResult<T> {
    fn recover_poison(self) -> T {
        self.unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Debug, Clone)]
pub struct RpcDeadlines {
    pub readiness: Duration,
    pub read: Duration,
    pub mutation: Duration,
    pub long_mutation: Duration,
    pub prompt: Duration,
    pub bash: Duration,
    pub urgent: Duration,
}

impl Default for RpcDeadlines {
    fn default() -> Self {
        Self {
            readiness: Duration::from_secs(10),
            read: Duration::from_secs(15),
            mutation: Duration::from_secs(30),
            // Compaction and session replacement can legitimately wait on a
            // provider response or summary prompt. Treating them like small
            // settings mutations turns ordinary latency into a poisoned RPC
            // connection and kills the Pi process.
            long_mutation: Duration::from_secs(30 * 60),
            prompt: Duration::from_secs(30),
            bash: Duration::from_secs(60 * 60),
            urgent: Duration::from_secs(5),
        }
    }
}

impl RpcDeadlines {
    fn validate(&self) -> Result<(), RpcClientStartError> {
        if [
            self.readiness,
            self.read,
            self.mutation,
            self.long_mutation,
            self.prompt,
            self.bash,
            self.urgent,
        ]
        .into_iter()
        .any(|deadline| deadline.is_zero())
        {
            return Err(RpcClientStartError::InvalidDeadlines);
        }
        Ok(())
    }

    fn for_command(&self, command: &Command) -> Duration {
        match command {
            Command::Prompt { .. } | Command::Steer { .. } | Command::FollowUp { .. } => {
                self.prompt
            }
            Command::Compact { .. }
            | Command::NewSession { .. }
            | Command::SwitchSession { .. }
            | Command::Fork { .. }
            | Command::Clone => self.long_mutation,
            Command::Bash { .. } => self.bash,
            Command::Abort | Command::AbortRetry | Command::AbortBash => self.urgent,
            command if command_class(command) == CommandClass::Read => self.read,
            _ => self.mutation,
        }
    }
}

#[derive(Debug)]
pub enum RpcClientStartError {
    InvalidDeadlines,
    Process(StartError),
    Readiness(RpcClientError),
    ReadinessRejected,
}

impl fmt::Display for RpcClientStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDeadlines => {
                formatter.write_str("RPC operation deadlines must be greater than zero")
            }
            Self::Process(error) => error.fmt(formatter),
            Self::Readiness(error) => write!(formatter, "Pi RPC readiness failed: {error}"),
            Self::ReadinessRejected => {
                formatter.write_str("Pi rejected the correlated get_state readiness request")
            }
        }
    }
}

impl std::error::Error for RpcClientStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Process(error) => Some(error),
            Self::Readiness(error) => Some(error),
            Self::InvalidDeadlines | Self::ReadinessRejected => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcClientErrorKind {
    UnknownOutcome,
    Encoding,
    ProtocolFault,
    StdoutFault,
    ProcessExit,
    WriterFailure,
    ConnectionPoisoned,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcClientError {
    pub kind: RpcClientErrorKind,
    pub generation: ConnectionGeneration,
    pub operation: Option<String>,
}

impl RpcClientError {
    fn new(
        kind: RpcClientErrorKind,
        generation: ConnectionGeneration,
        operation: Option<&str>,
    ) -> Self {
        Self {
            kind,
            generation,
            operation: operation.map(ToOwned::to_owned),
        }
    }
}

impl fmt::Display for RpcClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation = self.operation.as_deref().unwrap_or("RPC operation");
        match self.kind {
            RpcClientErrorKind::UnknownOutcome => write!(
                formatter,
                "{operation} exceeded its deadline; its outcome is unknown and it was not cancelled"
            ),
            RpcClientErrorKind::Encoding => write!(formatter, "could not encode {operation}"),
            RpcClientErrorKind::ProtocolFault => {
                write!(formatter, "Pi returned an invalid response for {operation}")
            }
            RpcClientErrorKind::StdoutFault => {
                write!(
                    formatter,
                    "Pi output closed or could not be read during {operation}"
                )
            }
            RpcClientErrorKind::ProcessExit => {
                write!(formatter, "Pi exited before {operation} completed")
            }
            RpcClientErrorKind::WriterFailure => {
                write!(
                    formatter,
                    "Pi input closed before {operation} could be confirmed"
                )
            }
            RpcClientErrorKind::ConnectionPoisoned => write!(
                formatter,
                "the Pi connection was closed after an uncertain mutation outcome"
            ),
            RpcClientErrorKind::Stopped => {
                write!(formatter, "the Pi connection was explicitly stopped")
            }
        }
    }
}

impl std::error::Error for RpcClientError {}

pub struct RpcCall {
    id: RequestId,
    generation: ConnectionGeneration,
    receiver: mpsc::Receiver<Result<RpcResponse, RpcClientError>>,
}

impl RpcCall {
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    pub fn generation(&self) -> ConnectionGeneration {
        self.generation
    }

    pub fn wait(self) -> Result<RpcResponse, RpcClientError> {
        self.receiver.recv().unwrap_or_else(|_| {
            Err(RpcClientError::new(
                RpcClientErrorKind::Stopped,
                self.generation,
                None,
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaggedIncomingRecord {
    pub generation: ConnectionGeneration,
    pub record: IncomingRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Starting,
    Ready,
    Faulted(RpcClientErrorKind),
    Stopped,
}

#[derive(Debug, Clone)]
pub struct RpcDiagnostics {
    pub generation: ConnectionGeneration,
    pub status: ConnectionStatus,
    pub pending_requests: usize,
    pub timed_out_requests: u64,
    pub stale_records_ignored: u64,
    pub supervisor_state: Option<SupervisorState>,
    pub stderr: Vec<String>,
}

#[derive(Clone)]
pub struct RpcClient {
    inner: Arc<RpcClientInner>,
}

impl RpcClient {
    pub fn start(config: PiLaunchConfig) -> Result<Self, RpcClientStartError> {
        Self::start_with_deadlines(config, RpcDeadlines::default())
    }

    pub fn start_with_deadlines(
        config: PiLaunchConfig,
        deadlines: RpcDeadlines,
    ) -> Result<Self, RpcClientStartError> {
        deadlines.validate()?;
        let (notification_sender, notifications) = mpsc::sync_channel(MAX_PENDING_NOTIFICATIONS);
        let inner = Arc::new(RpcClientInner {
            config,
            deadlines,
            lifecycle: Mutex::new(()),
            active: Mutex::new(None),
            generation: AtomicU64::new(0),
            notification_sender,
            notifications: Mutex::new(notifications),
            stale_records_ignored: AtomicU64::new(0),
            ready_state: Mutex::new(None),
        });
        inner.start_fresh_generation()?;
        Ok(Self { inner })
    }

    pub fn generation(&self) -> ConnectionGeneration {
        ConnectionGeneration::new(self.inner.generation.load(Ordering::Acquire))
    }

    pub fn initial_state(&self) -> Option<SessionState> {
        self.inner.ready_state.lock().recover_poison().clone()
    }

    pub fn request(&self, command: Command) -> RpcCall {
        let generation = self.generation();
        let Some(connection) = self.inner.active_connection() else {
            return failed_call(
                RequestId::new(format!("pi-gui-{generation}-unavailable")),
                generation,
                RpcClientError::new(
                    RpcClientErrorKind::Stopped,
                    generation,
                    command_name(&command),
                ),
            );
        };
        connection.submit(command)
    }

    pub(crate) fn request_with_id(&self, id: RequestId, command: Command) -> RpcCall {
        let generation = self.generation();
        let Some(connection) = self.inner.active_connection() else {
            return failed_call(
                id,
                generation,
                RpcClientError::new(
                    RpcClientErrorKind::Stopped,
                    generation,
                    command_name(&command),
                ),
            );
        };
        connection.submit_with_id(id, command)
    }

    pub fn send_extension_ui_response(
        &self,
        response: ExtensionUiResponse,
    ) -> Result<(), RpcClientError> {
        let generation = self.generation();
        let Some(connection) = self.inner.active_connection() else {
            return Err(RpcClientError::new(
                RpcClientErrorKind::Stopped,
                generation,
                Some("extension_ui_response"),
            ));
        };
        connection.send_extension_ui_response(response)
    }

    pub fn recv_notification_timeout(
        &self,
        timeout: Duration,
    ) -> Result<TaggedIncomingRecord, mpsc::RecvTimeoutError> {
        let started = Instant::now();
        loop {
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                return Err(mpsc::RecvTimeoutError::Timeout);
            };
            let record = self
                .inner
                .notifications
                .lock()
                .recover_poison()
                .recv_timeout(remaining)?;
            if record.generation == self.generation() {
                return Ok(record);
            }
            self.inner
                .stale_records_ignored
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn retry_fresh(&self) -> Result<ConnectionGeneration, RpcClientStartError> {
        let _lifecycle = self.inner.lifecycle.lock().recover_poison();
        if let Some(connection) = self.inner.active.lock().recover_poison().take() {
            connection.stop();
        }
        self.inner.ready_state.lock().recover_poison().take();
        self.inner.start_fresh_generation()
    }

    pub fn stop(&self) -> Option<ShutdownReport> {
        let _lifecycle = self.inner.lifecycle.lock().recover_poison();
        let connection = self.inner.active.lock().recover_poison().take()?;
        Some(connection.stop())
    }

    pub(crate) fn status(&self) -> ConnectionStatus {
        let Some(connection) = self.inner.active_connection() else {
            return ConnectionStatus::Stopped;
        };
        *connection.status.lock().recover_poison()
    }

    pub fn diagnostics(&self) -> RpcDiagnostics {
        let generation = self.generation();
        let stale_records_ignored = self.inner.stale_records_ignored.load(Ordering::Relaxed);
        let Some(connection) = self.inner.active_connection() else {
            return RpcDiagnostics {
                generation,
                status: ConnectionStatus::Stopped,
                pending_requests: 0,
                timed_out_requests: 0,
                stale_records_ignored,
                supervisor_state: None,
                stderr: Vec::new(),
            };
        };
        connection.diagnostics(stale_records_ignored)
    }
}

struct RpcClientInner {
    config: PiLaunchConfig,
    deadlines: RpcDeadlines,
    lifecycle: Mutex<()>,
    active: Mutex<Option<Arc<Connection>>>,
    generation: AtomicU64,
    notification_sender: mpsc::SyncSender<TaggedIncomingRecord>,
    notifications: Mutex<mpsc::Receiver<TaggedIncomingRecord>>,
    stale_records_ignored: AtomicU64,
    ready_state: Mutex<Option<SessionState>>,
}

impl RpcClientInner {
    fn active_connection(&self) -> Option<Arc<Connection>> {
        self.active.lock().recover_poison().clone()
    }

    fn start_fresh_generation(&self) -> Result<ConnectionGeneration, RpcClientStartError> {
        let generation = ConnectionGeneration::new(
            self.generation
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1),
        );
        let mut supervisor =
            PiSupervisor::start(self.config.clone()).map_err(RpcClientStartError::Process)?;
        let stdout = supervisor.take_stdout().ok_or_else(|| {
            RpcClientStartError::Readiness(RpcClientError::new(
                RpcClientErrorKind::StdoutFault,
                generation,
                Some("get_state"),
            ))
        })?;
        let connection = Connection::new(
            generation,
            supervisor,
            stdout,
            self.deadlines.clone(),
            self.notification_sender.clone(),
        );
        *self.active.lock().recover_poison() = Some(Arc::clone(&connection));

        let readiness = connection
            .dispatch_direct(Command::GetState, self.deadlines.readiness, false)
            .wait();
        match readiness {
            Ok(response) => match response.result {
                ResponseResult::GetState(state) => {
                    *self.ready_state.lock().recover_poison() = Some(state);
                    connection.mark_ready();
                    Ok(generation)
                }
                ResponseResult::Failure { .. } => {
                    self.clear_failed_connection(&connection);
                    Err(RpcClientStartError::ReadinessRejected)
                }
                _ => {
                    connection.protocol_fault();
                    self.clear_failed_connection(&connection);
                    Err(RpcClientStartError::Readiness(RpcClientError::new(
                        RpcClientErrorKind::ProtocolFault,
                        generation,
                        Some("get_state"),
                    )))
                }
            },
            Err(error) => {
                self.clear_failed_connection(&connection);
                Err(RpcClientStartError::Readiness(error))
            }
        }
    }

    fn clear_failed_connection(&self, connection: &Arc<Connection>) {
        let current = self.active.lock().recover_poison().take();
        if let Some(current) = current {
            if Arc::ptr_eq(&current, connection) {
                current.stop();
            } else {
                *self.active.lock().recover_poison() = Some(current);
            }
        }
    }
}

impl Drop for RpcClientInner {
    fn drop(&mut self) {
        if let Some(connection) = self.active.get_mut().recover_poison().take() {
            connection.stop();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandClass {
    Read,
    Mutation,
    Bypass,
}

fn command_class(command: &Command) -> CommandClass {
    match command {
        Command::GetState
        | Command::GetAvailableModels
        | Command::GetSessionStats
        | Command::GetForkMessages
        | Command::GetEntries { .. }
        | Command::GetTree
        | Command::GetLastAssistantText
        | Command::GetMessages
        | Command::GetCommands => CommandClass::Read,
        Command::Abort | Command::AbortRetry | Command::AbortBash => CommandClass::Bypass,
        _ => CommandClass::Mutation,
    }
}

fn command_name(command: &Command) -> Option<&'static str> {
    Some(match command {
        Command::Prompt { .. } => "prompt",
        Command::Steer { .. } => "steer",
        Command::FollowUp { .. } => "follow_up",
        Command::Abort => "abort",
        Command::NewSession { .. } => "new_session",
        Command::GetState => "get_state",
        Command::SetModel { .. } => "set_model",
        Command::CycleModel => "cycle_model",
        Command::GetAvailableModels => "get_available_models",
        Command::SetThinkingLevel { .. } => "set_thinking_level",
        Command::CycleThinkingLevel => "cycle_thinking_level",
        Command::SetSteeringMode { .. } => "set_steering_mode",
        Command::SetFollowUpMode { .. } => "set_follow_up_mode",
        Command::Compact { .. } => "compact",
        Command::SetAutoCompaction { .. } => "set_auto_compaction",
        Command::SetAutoRetry { .. } => "set_auto_retry",
        Command::AbortRetry => "abort_retry",
        Command::Bash { .. } => "bash",
        Command::AbortBash => "abort_bash",
        Command::GetSessionStats => "get_session_stats",
        Command::ExportHtml { .. } => "export_html",
        Command::SwitchSession { .. } => "switch_session",
        Command::Fork { .. } => "fork",
        Command::Clone => "clone",
        Command::GetForkMessages => "get_fork_messages",
        Command::GetEntries { .. } => "get_entries",
        Command::GetTree => "get_tree",
        Command::GetLastAssistantText => "get_last_assistant_text",
        Command::SetSessionName { .. } => "set_session_name",
        Command::GetMessages => "get_messages",
        Command::GetCommands => "get_commands",
    })
}

struct MutationJob {
    id: RequestId,
    command: Command,
    deadline: Duration,
    result: mpsc::Sender<Result<RpcResponse, RpcClientError>>,
}

struct PendingRequest {
    operation: &'static str,
    deadline: Instant,
    mutation: bool,
    result: mpsc::Sender<Result<RpcResponse, RpcClientError>>,
}

struct PendingRegistry {
    entries: Mutex<HashMap<RequestId, PendingRequest>>,
    changed: Condvar,
}

struct Connection {
    generation: ConnectionGeneration,
    supervisor: Mutex<Option<PiSupervisor>>,
    deadlines: RpcDeadlines,
    pending: PendingRegistry,
    abandoned_reads: Mutex<HashSet<RequestId>>,
    next_request: AtomicU64,
    closed: AtomicBool,
    status: Mutex<ConnectionStatus>,
    timed_out_requests: AtomicU64,
    mutation_sender: mpsc::Sender<MutationJob>,
    notification_sender: mpsc::SyncSender<TaggedIncomingRecord>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl Connection {
    fn new(
        generation: ConnectionGeneration,
        supervisor: PiSupervisor,
        stdout: mpsc::Receiver<crate::services::pi_process::StdoutEvent>,
        deadlines: RpcDeadlines,
        notification_sender: mpsc::SyncSender<TaggedIncomingRecord>,
    ) -> Arc<Self> {
        let (mutation_sender, mutation_receiver) = mpsc::channel();
        let connection = Arc::new(Self {
            generation,
            supervisor: Mutex::new(Some(supervisor)),
            deadlines,
            pending: PendingRegistry {
                entries: Mutex::new(HashMap::new()),
                changed: Condvar::new(),
            },
            abandoned_reads: Mutex::new(HashSet::new()),
            next_request: AtomicU64::new(1),
            closed: AtomicBool::new(false),
            status: Mutex::new(ConnectionStatus::Starting),
            timed_out_requests: AtomicU64::new(0),
            mutation_sender,
            notification_sender,
            workers: Mutex::new(Vec::new()),
        });

        let mut workers = connection.workers.lock().recover_poison();
        let weak = Arc::downgrade(&connection);
        workers.push(thread::spawn(move || reader_loop(weak, stdout)));
        let weak = Arc::downgrade(&connection);
        workers.push(thread::spawn(move || deadline_loop(weak)));
        let weak = Arc::downgrade(&connection);
        workers.push(thread::spawn(move || {
            mutation_loop(weak, mutation_receiver)
        }));
        drop(workers);
        connection
    }

    fn mark_ready(&self) {
        let mut status = self.status.lock().recover_poison();
        if *status == ConnectionStatus::Starting {
            *status = ConnectionStatus::Ready;
        }
    }

    fn next_id(&self) -> RequestId {
        let sequence = self.next_request.fetch_add(1, Ordering::Relaxed);
        RequestId::new(format!("pi-gui-{}-{sequence}", self.generation.value()))
    }

    fn submit(&self, command: Command) -> RpcCall {
        self.submit_with_id(self.next_id(), command)
    }

    fn submit_with_id(&self, id: RequestId, command: Command) -> RpcCall {
        let deadline = self.deadlines.for_command(&command);
        match command_class(&command) {
            CommandClass::Mutation => {
                let (result, receiver) = mpsc::channel();
                let call = RpcCall {
                    id: id.clone(),
                    generation: self.generation,
                    receiver,
                };
                if self.closed.load(Ordering::Acquire)
                    || self
                        .mutation_sender
                        .send(MutationJob {
                            id,
                            command,
                            deadline,
                            result: result.clone(),
                        })
                        .is_err()
                {
                    let _ = result.send(Err(RpcClientError::new(
                        RpcClientErrorKind::Stopped,
                        self.generation,
                        None,
                    )));
                }
                call
            }
            CommandClass::Read => self.dispatch_direct_with_id(id, command, deadline, false),
            CommandClass::Bypass => self.dispatch_direct_with_id(id, command, deadline, true),
        }
    }

    fn dispatch_direct(&self, command: Command, deadline: Duration, mutation: bool) -> RpcCall {
        let id = self.next_id();
        self.dispatch_direct_with_id(id, command, deadline, mutation)
    }

    fn dispatch_direct_with_id(
        &self,
        id: RequestId,
        command: Command,
        deadline: Duration,
        mutation: bool,
    ) -> RpcCall {
        let (result, receiver) = mpsc::channel();
        let call = RpcCall {
            id: id.clone(),
            generation: self.generation,
            receiver,
        };
        self.register_and_write(id, command, deadline, mutation, result);
        call
    }

    fn register_and_write(
        &self,
        id: RequestId,
        command: Command,
        deadline: Duration,
        mutation: bool,
        result: mpsc::Sender<Result<RpcResponse, RpcClientError>>,
    ) {
        let operation = command_name(&command).unwrap_or("unknown");
        if self.closed.load(Ordering::Acquire) {
            let _ = result.send(Err(RpcClientError::new(
                RpcClientErrorKind::Stopped,
                self.generation,
                Some(operation),
            )));
            return;
        }
        let record = OutboundRecord::Command(RpcCommand::new(id.clone(), command));
        let bytes = match encode_record(&record) {
            Ok(bytes) => bytes,
            Err(_) => {
                let _ = result.send(Err(RpcClientError::new(
                    RpcClientErrorKind::Encoding,
                    self.generation,
                    Some(operation),
                )));
                return;
            }
        };
        self.pending.entries.lock().recover_poison().insert(
            id,
            PendingRequest {
                operation,
                deadline: Instant::now() + deadline,
                mutation,
                result,
            },
        );
        self.pending.changed.notify_all();

        let write_result = self
            .supervisor
            .lock()
            .recover_poison()
            .as_ref()
            .map_or(Err(()), |supervisor| {
                supervisor.write_record(bytes).map_err(|_| ())
            });
        if write_result.is_err() {
            self.close(
                RpcClientErrorKind::WriterFailure,
                ConnectionStatus::Faulted(RpcClientErrorKind::WriterFailure),
                true,
            );
        }
    }

    fn send_extension_ui_response(
        &self,
        response: ExtensionUiResponse,
    ) -> Result<(), RpcClientError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(RpcClientError::new(
                RpcClientErrorKind::Stopped,
                self.generation,
                Some("extension_ui_response"),
            ));
        }
        let bytes =
            encode_record(&OutboundRecord::ExtensionUiResponse(response)).map_err(|_| {
                RpcClientError::new(
                    RpcClientErrorKind::Encoding,
                    self.generation,
                    Some("extension_ui_response"),
                )
            })?;
        let result = self
            .supervisor
            .lock()
            .recover_poison()
            .as_ref()
            .ok_or_else(|| {
                RpcClientError::new(
                    RpcClientErrorKind::Stopped,
                    self.generation,
                    Some("extension_ui_response"),
                )
            })?
            .write_record(bytes);
        if result.is_err() {
            self.close(
                RpcClientErrorKind::WriterFailure,
                ConnectionStatus::Faulted(RpcClientErrorKind::WriterFailure),
                true,
            );
            return Err(RpcClientError::new(
                RpcClientErrorKind::WriterFailure,
                self.generation,
                Some("extension_ui_response"),
            ));
        }
        Ok(())
    }

    fn route(&self, record: IncomingRecord) {
        let replaceable_stream_update = matches!(
            &record,
            IncomingRecord::Event(event)
                if matches!(
                    event.as_ref(),
                    RpcEvent::MessageUpdate { .. } | RpcEvent::ToolExecutionUpdate { .. }
                )
        );
        let IncomingRecord::Response(response) = record else {
            let mut notification = TaggedIncomingRecord {
                generation: self.generation,
                record,
            };
            loop {
                if self.closed.load(Ordering::Acquire) {
                    return;
                }
                match self.notification_sender.try_send(notification) {
                    Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => return,
                    Err(mpsc::TrySendError::Full(_)) if replaceable_stream_update => return,
                    Err(mpsc::TrySendError::Full(pending)) => {
                        notification = pending;
                        thread::sleep(Duration::from_millis(1));
                    }
                }
            }
        };
        let Some(id) = response.id.clone() else {
            self.protocol_fault();
            return;
        };
        let pending = self.pending.entries.lock().recover_poison().remove(&id);
        self.pending.changed.notify_all();
        let Some(pending) = pending else {
            let was_abandoned = self.abandoned_reads.lock().recover_poison().remove(&id);
            if !was_abandoned {
                self.protocol_fault();
            }
            return;
        };
        if response.result.command() != pending.operation {
            let _ = pending.result.send(Err(RpcClientError::new(
                RpcClientErrorKind::ProtocolFault,
                self.generation,
                Some(pending.operation),
            )));
            self.protocol_fault();
            return;
        }
        let _ = pending.result.send(Ok(*response));
    }

    fn protocol_fault(&self) {
        self.close(
            RpcClientErrorKind::ProtocolFault,
            ConnectionStatus::Faulted(RpcClientErrorKind::ProtocolFault),
            true,
        );
    }

    fn close(&self, error_kind: RpcClientErrorKind, status: ConnectionStatus, terminate: bool) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        *self.status.lock().recover_poison() = status;
        let pending = {
            let mut entries = self.pending.entries.lock().recover_poison();
            entries
                .drain()
                .map(|(_, pending)| pending)
                .collect::<Vec<_>>()
        };
        self.pending.changed.notify_all();
        for pending in pending {
            let _ = pending.result.send(Err(RpcClientError::new(
                error_kind,
                self.generation,
                Some(pending.operation),
            )));
        }
        if terminate && let Some(supervisor) = self.supervisor.lock().recover_poison().as_ref() {
            supervisor.terminate_due_to_rpc_fault();
        }
    }

    fn stop(&self) -> ShutdownReport {
        self.close(
            RpcClientErrorKind::Stopped,
            ConnectionStatus::Stopped,
            false,
        );
        let report = self
            .supervisor
            .lock()
            .recover_poison()
            .as_mut()
            .map(PiSupervisor::shutdown)
            .unwrap_or(ShutdownReport {
                forced: false,
                abort_sent: false,
            });
        self.join_workers();
        report
    }

    fn join_workers(&self) {
        let current = thread::current().id();
        let mut workers = self.workers.lock().recover_poison();
        for worker in workers.drain(..) {
            if worker.thread().id() != current {
                let _ = worker.join();
            }
        }
    }

    fn diagnostics(&self, stale_records_ignored: u64) -> RpcDiagnostics {
        let supervisor = self.supervisor.lock().recover_poison();
        RpcDiagnostics {
            generation: self.generation,
            status: *self.status.lock().recover_poison(),
            pending_requests: self.pending.entries.lock().recover_poison().len(),
            timed_out_requests: self.timed_out_requests.load(Ordering::Relaxed),
            stale_records_ignored,
            supervisor_state: supervisor.as_ref().map(PiSupervisor::state),
            stderr: supervisor
                .as_ref()
                .map(PiSupervisor::stderr_snapshot)
                .unwrap_or_default(),
        }
    }
}

fn failed_call(id: RequestId, generation: ConnectionGeneration, error: RpcClientError) -> RpcCall {
    let (sender, receiver) = mpsc::channel();
    let _ = sender.send(Err(error));
    RpcCall {
        id,
        generation,
        receiver,
    }
}

fn reader_loop(
    connection: Weak<Connection>,
    stdout: mpsc::Receiver<crate::services::pi_process::StdoutEvent>,
) {
    let mut codec = JsonlCodec::default();
    while let Ok(event) = stdout.recv() {
        let Some(connection) = connection.upgrade() else {
            return;
        };
        if connection.closed.load(Ordering::Acquire) {
            return;
        }
        match event {
            crate::services::pi_process::StdoutEvent::Data(bytes) => match codec.feed(&bytes) {
                Ok(records) => {
                    for record in records {
                        connection.route(record);
                        if connection.closed.load(Ordering::Acquire) {
                            return;
                        }
                    }
                }
                Err(_) => {
                    connection.protocol_fault();
                    return;
                }
            },
            crate::services::pi_process::StdoutEvent::Eof => {
                match codec.finish() {
                    Ok(Some(record)) => connection.route(record),
                    Ok(None) => {}
                    Err(_) => {
                        connection.protocol_fault();
                        return;
                    }
                }
                if connection.closed.load(Ordering::Acquire) {
                    return;
                }
                let terminal_state = connection
                    .supervisor
                    .lock()
                    .recover_poison()
                    .as_ref()
                    .map(|supervisor| supervisor.wait_for_terminal(Duration::from_millis(250)));
                let kind = match terminal_state {
                    Some(SupervisorState::Stopped { .. })
                    | Some(SupervisorState::Failed(
                        crate::services::pi_process::ProcessFailure {
                            kind: ProcessFailureKind::UnexpectedExit,
                            ..
                        },
                    )) => RpcClientErrorKind::ProcessExit,
                    _ => RpcClientErrorKind::StdoutFault,
                };
                connection.close(kind, ConnectionStatus::Faulted(kind), true);
                return;
            }
            crate::services::pi_process::StdoutEvent::ReadError(_) => {
                connection.close(
                    RpcClientErrorKind::StdoutFault,
                    ConnectionStatus::Faulted(RpcClientErrorKind::StdoutFault),
                    true,
                );
                return;
            }
        }
    }
    if let Some(connection) = connection.upgrade()
        && !connection.closed.load(Ordering::Acquire)
    {
        connection.close(
            RpcClientErrorKind::StdoutFault,
            ConnectionStatus::Faulted(RpcClientErrorKind::StdoutFault),
            true,
        );
    }
}

fn deadline_loop(connection: Weak<Connection>) {
    loop {
        let Some(connection) = connection.upgrade() else {
            return;
        };
        let mut entries = connection.pending.entries.lock().recover_poison();
        while entries.is_empty() && !connection.closed.load(Ordering::Acquire) {
            entries = connection.pending.changed.wait(entries).recover_poison();
        }
        if connection.closed.load(Ordering::Acquire) {
            return;
        }
        let now = Instant::now();
        let next_deadline = entries
            .values()
            .map(|pending| pending.deadline)
            .min()
            .unwrap_or(now);
        if next_deadline > now {
            let (next_entries, wait) = connection
                .pending
                .changed
                .wait_timeout(entries, next_deadline - now)
                .recover_poison();
            entries = next_entries;
            if !wait.timed_out() {
                drop(entries);
                continue;
            }
        }
        let now = Instant::now();
        let expired_ids = entries
            .iter()
            .filter_map(|(id, pending)| (pending.deadline <= now).then_some(id.clone()))
            .collect::<Vec<_>>();
        let expired = expired_ids
            .into_iter()
            .filter_map(|id| entries.remove(&id).map(|pending| (id, pending)))
            .collect::<Vec<_>>();
        let mutation_timed_out = expired.iter().any(|(_, pending)| pending.mutation);
        if !mutation_timed_out {
            let mut abandoned = connection.abandoned_reads.lock().recover_poison();
            abandoned.extend(expired.iter().map(|(id, _)| id.clone()));
        }
        drop(entries);
        if expired.is_empty() {
            continue;
        }
        connection
            .timed_out_requests
            .fetch_add(expired.len() as u64, Ordering::Relaxed);
        for (_, pending) in expired {
            let _ = pending.result.send(Err(RpcClientError::new(
                RpcClientErrorKind::UnknownOutcome,
                connection.generation,
                Some(pending.operation),
            )));
        }
        if mutation_timed_out {
            connection.close(
                RpcClientErrorKind::ConnectionPoisoned,
                ConnectionStatus::Faulted(RpcClientErrorKind::ConnectionPoisoned),
                true,
            );
            return;
        }
    }
}

fn mutation_loop(connection: Weak<Connection>, receiver: mpsc::Receiver<MutationJob>) {
    loop {
        let job = match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(job) => job,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if connection
                    .upgrade()
                    .is_none_or(|connection| connection.closed.load(Ordering::Acquire))
                {
                    return;
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let Some(connection) = connection.upgrade() else {
            let _ = job.result.send(Err(RpcClientError::new(
                RpcClientErrorKind::Stopped,
                ConnectionGeneration::default(),
                command_name(&job.command),
            )));
            return;
        };
        if connection.closed.load(Ordering::Acquire) {
            let _ = job.result.send(Err(RpcClientError::new(
                RpcClientErrorKind::Stopped,
                connection.generation,
                command_name(&job.command),
            )));
            continue;
        }
        let (completion, completed) = mpsc::channel();
        connection.register_and_write(job.id, job.command, job.deadline, true, completion);
        let result = completed.recv().unwrap_or_else(|_| {
            Err(RpcClientError::new(
                RpcClientErrorKind::Stopped,
                connection.generation,
                None,
            ))
        });
        let _ = job.result.send(result);
    }
}
