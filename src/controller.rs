//! GPUI-owned runtime controller and its pure generation gate.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gpui::{Context, Task};

use crate::services::rpc::{ConnectionGeneration, RequestId, SessionEpoch};
use crate::services::runtime_worker::{
    AttemptGeneration, RuntimeService, RuntimeWorkerHandle, WorkerResult,
};
use crate::services::session_catalog::{
    CatalogWorkerResult, CorruptSession, SessionCatalogConfig, SessionCatalogWorker, SessionRoot,
    SessionSummary,
};
use crate::state::reducer::reduce;
use crate::state::runtime::{
    BashExecution, BashStatus, CompactionState, FacetStatus, PromptDelivery, QueueContents,
    QueueDeliveryMode, RetryState, RuntimeInput, RuntimeIntent, RuntimeLifecycle, RuntimeMessage,
    RuntimeOperation, RuntimeState, SafeError, StampedInput, SubmissionKind, ToolExecution,
};
use crate::state::{ControllerStatus, ShellProjection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionPreference {
    Default,
    FollowUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerRuntime {
    Unavailable,
    Idle,
    Running,
    Cancelling,
    BashRunning,
    BashCancelling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerProjection {
    pub runtime: ComposerRuntime,
    pub delivery: PromptDelivery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedUserInput {
    pub request: RequestId,
    pub text: String,
    pub kind: SubmissionKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationProjection {
    pub epoch: SessionEpoch,
    pub revision: u64,
    pub lifecycle: RuntimeLifecycle,
    pub status: FacetStatus,
    pub messages: Vec<RuntimeMessage>,
    pub accepted_user_inputs: Vec<AcceptedUserInput>,
    pub tools: HashMap<crate::services::rpc::ToolCallId, ToolExecution>,
    pub bash_executions: Vec<BashExecution>,
    pub queue: QueueContents,
    pub steering_mode: Option<QueueDeliveryMode>,
    pub follow_up_mode: Option<QueueDeliveryMode>,
    pub auto_compaction_enabled: Option<bool>,
    pub auto_retry_enabled: Option<bool>,
    pub pending_operation: Option<RuntimeOperation>,
    pub context_awaiting_fresh_usage: bool,
    pub retry: RetryState,
    pub compaction: CompactionState,
    pub error: Option<SafeError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogStatus {
    Loading,
    Ready,
    Empty,
    Inaccessible,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogProjection {
    pub status: CatalogStatus,
    pub sessions: Vec<SessionSummary>,
    pub corrupt: Vec<CorruptSession>,
    pub root: Option<SessionRoot>,
    pub error: Option<String>,
    pub current_session_id: Option<String>,
    pub current_session_name: Option<String>,
    pub current_session_file: Option<PathBuf>,
    pub switching: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedSubmission {
    pub request: RequestId,
    pub kind: AcceptedSubmissionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedSubmissionKind {
    Prompt(SubmissionKind),
    Bash { exclude_from_context: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionRejection {
    Empty,
    EmptyBash,
    Pending,
    BashRunning,
    Unavailable,
    NotRunning,
}

impl SubmissionRejection {
    pub fn message(self) -> &'static str {
        match self {
            Self::Empty => "Write a prompt first.",
            Self::EmptyBash => "Write a Bash command after ! or !!.",
            Self::Pending => "The previous acceptance is still pending.",
            Self::BashRunning => "A Bash command is already running.",
            Self::Unavailable => "Pi is not ready. The draft was kept.",
            Self::NotRunning => "Follow-ups can be queued while Pi is running.",
        }
    }
}

pub struct ControllerCore {
    status: ControllerStatus,
    attempt: AttemptGeneration,
    generation: ConnectionGeneration,
    runtime: RuntimeState,
    workspace: String,
    connection_error: Option<String>,
    stale_attempts_ignored: u64,
    next_submission: u64,
}

impl ControllerCore {
    pub fn new(workspace: impl Into<String>) -> Self {
        Self {
            status: ControllerStatus::Idle,
            attempt: AttemptGeneration::default(),
            generation: ConnectionGeneration::default(),
            runtime: RuntimeState::default(),
            workspace: workspace.into(),
            connection_error: None,
            stale_attempts_ignored: 0,
            next_submission: 1,
        }
    }

    pub fn status(&self) -> ControllerStatus {
        self.status
    }

    pub fn attempt(&self) -> AttemptGeneration {
        self.attempt
    }

    pub fn generation(&self) -> ConnectionGeneration {
        self.generation
    }

    pub fn runtime(&self) -> &RuntimeState {
        &self.runtime
    }

    pub fn stale_attempts_ignored(&self) -> u64 {
        self.stale_attempts_ignored
    }

    pub fn begin_connect(&mut self) -> (AttemptGeneration, ConnectionGeneration) {
        self.attempt = self.attempt.next();
        self.generation = self.generation.next();
        self.status = ControllerStatus::Connecting;
        self.connection_error = None;
        self.runtime.lifecycle = crate::state::runtime::RuntimeLifecycle::Loading;
        self.runtime.mark_hydration_loading();
        (self.attempt, self.generation)
    }

    pub fn begin_stop(&mut self) -> bool {
        if matches!(
            self.status,
            ControllerStatus::Connecting | ControllerStatus::Active
        ) {
            self.status = ControllerStatus::Stopping;
            true
        } else {
            false
        }
    }

    pub fn apply_worker_result(
        &mut self,
        result: WorkerResult,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        let result_attempt = match &result {
            WorkerResult::Connecting { attempt, .. }
            | WorkerResult::Connected { attempt, .. }
            | WorkerResult::Input { attempt, .. }
            | WorkerResult::ConnectionFailed { attempt, .. }
            | WorkerResult::Stopped { attempt } => *attempt,
        };
        if result_attempt != self.attempt {
            self.stale_attempts_ignored = self.stale_attempts_ignored.saturating_add(1);
            return Vec::new();
        }

        match result {
            WorkerResult::Connecting { generation, .. } => {
                if generation == self.generation && self.status != ControllerStatus::Stopping {
                    self.status = ControllerStatus::Connecting;
                }
                Vec::new()
            }
            WorkerResult::Connected { generation, .. } => {
                if generation != self.generation || self.status == ControllerStatus::Stopping {
                    self.stale_attempts_ignored = self.stale_attempts_ignored.saturating_add(1);
                    return Vec::new();
                }
                self.status = ControllerStatus::Active;
                let epoch = self.runtime.epoch;
                reduce(
                    &mut self.runtime,
                    StampedInput {
                        generation,
                        epoch,
                        observed_at: Instant::now(),
                        input: RuntimeInput::Connected {
                            recovery: generation.value() > 1,
                        },
                    },
                )
            }
            WorkerResult::Input { input, .. } => {
                if self.status != ControllerStatus::Active {
                    self.stale_attempts_ignored = self.stale_attempts_ignored.saturating_add(1);
                    return Vec::new();
                }
                reduce(&mut self.runtime, *input)
            }
            WorkerResult::ConnectionFailed {
                generation,
                failure,
                ..
            } => {
                if generation == self.generation && self.status != ControllerStatus::Stopping {
                    self.status = ControllerStatus::Failed;
                    self.connection_error = Some(failure.summary);
                }
                Vec::new()
            }
            WorkerResult::Stopped { .. } => {
                self.status = ControllerStatus::Stopped;
                Vec::new()
            }
        }
    }

    pub fn projection(&self) -> ShellProjection {
        ShellProjection::from_runtime(
            self.status,
            self.workspace.clone(),
            &self.runtime,
            self.connection_error.as_deref(),
        )
    }

    pub fn composer_projection(&self) -> ComposerProjection {
        let has_model = self
            .runtime
            .session
            .data
            .as_ref()
            .and_then(|session| session.model.as_ref())
            .is_some();
        let bash_status = self
            .runtime
            .bash_executions
            .iter()
            .rev()
            .find(|execution| execution.status.is_active())
            .map(|execution| execution.status);
        let runtime = if self.status != ControllerStatus::Active {
            ComposerRuntime::Unavailable
        } else if bash_status == Some(BashStatus::Cancelling) {
            ComposerRuntime::BashCancelling
        } else if bash_status == Some(BashStatus::Running) {
            ComposerRuntime::BashRunning
        } else if !has_model {
            ComposerRuntime::Unavailable
        } else {
            match self.runtime.lifecycle {
                RuntimeLifecycle::Ready | RuntimeLifecycle::Settled => ComposerRuntime::Idle,
                RuntimeLifecycle::Running => ComposerRuntime::Running,
                RuntimeLifecycle::Cancelling => ComposerRuntime::Cancelling,
                RuntimeLifecycle::Loading
                | RuntimeLifecycle::Disconnected
                | RuntimeLifecycle::Failed => ComposerRuntime::Unavailable,
            }
        };
        ComposerProjection {
            runtime,
            delivery: self.runtime.prompt_delivery.clone(),
        }
    }

    pub fn conversation_projection(&self) -> ConversationProjection {
        let error = match &self.runtime.messages.status {
            FacetStatus::Failed(error) => Some(error.clone()),
            FacetStatus::Loading | FacetStatus::Ready => self
                .runtime
                .errors
                .back()
                .filter(|_| {
                    matches!(
                        self.runtime.lifecycle,
                        RuntimeLifecycle::Disconnected | RuntimeLifecycle::Failed
                    )
                })
                .cloned(),
        };
        ConversationProjection {
            epoch: self.runtime.display_epoch,
            revision: self.runtime.revision,
            lifecycle: self.runtime.lifecycle,
            status: self.runtime.messages.status.clone(),
            messages: self
                .runtime
                .messages
                .data
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter(|message| message.visible)
                .cloned()
                .collect(),
            accepted_user_inputs: self
                .runtime
                .optimistic_user_inputs
                .iter()
                .filter(|_| !self.runtime.replacement_awaiting_state)
                .filter(|_| {
                    !matches!(
                        self.runtime.lifecycle,
                        RuntimeLifecycle::Disconnected | RuntimeLifecycle::Failed
                    )
                })
                .filter(|input| input.accepted && !input.authoritative_seen)
                .map(|input| AcceptedUserInput {
                    request: input.request.clone(),
                    text: input.text.clone(),
                    kind: input.kind,
                })
                .collect(),
            tools: self.runtime.tools.clone(),
            bash_executions: self.runtime.bash_executions.clone(),
            queue: self.runtime.queue.clone(),
            steering_mode: self
                .runtime
                .session
                .data
                .as_ref()
                .map(|session| session.steering_mode),
            follow_up_mode: self
                .runtime
                .session
                .data
                .as_ref()
                .map(|session| session.follow_up_mode),
            auto_compaction_enabled: self
                .runtime
                .session
                .data
                .as_ref()
                .map(|session| session.auto_compaction_enabled),
            auto_retry_enabled: self.runtime.auto_retry_enabled,
            pending_operation: self.runtime.pending_operation.clone(),
            context_awaiting_fresh_usage: self.runtime.context_awaiting_fresh_usage,
            retry: self.runtime.retry.clone(),
            compaction: self.runtime.compaction.clone(),
            error,
        }
    }

    pub fn submit(
        &mut self,
        text: String,
        preference: SubmissionPreference,
    ) -> Result<
        (
            AcceptedSubmission,
            Vec<crate::state::runtime::RuntimeEffect>,
        ),
        SubmissionRejection,
    > {
        if text.trim().is_empty() {
            return Err(SubmissionRejection::Empty);
        }
        if let Some(parsed) = parse_bash_submission(&text) {
            let (command, exclude_from_context) = parsed?;
            if self.status != ControllerStatus::Active {
                return Err(SubmissionRejection::Unavailable);
            }
            if self
                .runtime
                .bash_executions
                .iter()
                .any(|execution| execution.status.is_active())
            {
                return Err(SubmissionRejection::BashRunning);
            }
            let request = self.next_composer_request("bash");
            let epoch = self.runtime.epoch;
            let effects = reduce(
                &mut self.runtime,
                StampedInput {
                    generation: self.generation,
                    epoch,
                    observed_at: Instant::now(),
                    input: RuntimeInput::Intent(RuntimeIntent::ExecuteBash {
                        request: request.clone(),
                        command,
                        exclude_from_context,
                    }),
                },
            );
            if effects.is_empty() {
                return Err(SubmissionRejection::Unavailable);
            }
            return Ok((
                AcceptedSubmission {
                    request,
                    kind: AcceptedSubmissionKind::Bash {
                        exclude_from_context,
                    },
                },
                effects,
            ));
        }
        if matches!(self.runtime.prompt_delivery, PromptDelivery::Pending { .. }) {
            return Err(SubmissionRejection::Pending);
        }

        let kind = match (self.composer_projection().runtime, preference) {
            (ComposerRuntime::Idle, SubmissionPreference::Default) => SubmissionKind::Prompt,
            (ComposerRuntime::Running, SubmissionPreference::Default) => SubmissionKind::Steer,
            (ComposerRuntime::Running, SubmissionPreference::FollowUp) => SubmissionKind::FollowUp,
            (_, SubmissionPreference::FollowUp) => return Err(SubmissionRejection::NotRunning),
            _ => return Err(SubmissionRejection::Unavailable),
        };
        let request = self.next_composer_request("composer");
        let epoch = self.runtime.epoch;
        let effects = reduce(
            &mut self.runtime,
            StampedInput {
                generation: self.generation,
                epoch,
                observed_at: Instant::now(),
                input: RuntimeInput::Intent(RuntimeIntent::Submit {
                    request: request.clone(),
                    text,
                    kind,
                }),
            },
        );
        if effects.is_empty() {
            return Err(SubmissionRejection::Unavailable);
        }
        Ok((
            AcceptedSubmission {
                request,
                kind: AcceptedSubmissionKind::Prompt(kind),
            },
            effects,
        ))
    }

    fn next_composer_request(&mut self, prefix: &str) -> RequestId {
        let request = RequestId::new(format!(
            "{prefix}-{}-{}-{}",
            self.generation.value(),
            self.runtime.epoch.value(),
            self.next_submission
        ));
        self.next_submission = self.next_submission.saturating_add(1);
        request
    }

    pub fn abort(&mut self) -> Vec<crate::state::runtime::RuntimeEffect> {
        if self.composer_projection().runtime != ComposerRuntime::Running {
            return Vec::new();
        }
        let epoch = self.runtime.epoch;
        reduce(
            &mut self.runtime,
            StampedInput {
                generation: self.generation,
                epoch,
                observed_at: Instant::now(),
                input: RuntimeInput::Intent(RuntimeIntent::Abort),
            },
        )
    }

    pub fn abort_bash(&mut self) -> Vec<crate::state::runtime::RuntimeEffect> {
        if self.composer_projection().runtime != ComposerRuntime::BashRunning {
            return Vec::new();
        }
        self.intent(RuntimeIntent::AbortBash)
    }

    pub fn abort_retry(&mut self) -> Vec<crate::state::runtime::RuntimeEffect> {
        if !matches!(self.runtime.retry, RetryState::Waiting { .. }) {
            return Vec::new();
        }
        self.intent(RuntimeIntent::AbortRetry)
    }

    pub fn set_steering_mode(
        &mut self,
        mode: QueueDeliveryMode,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::SetSteeringMode(mode))
    }

    pub fn set_follow_up_mode(
        &mut self,
        mode: QueueDeliveryMode,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::SetFollowUpMode(mode))
    }

    pub fn compact(
        &mut self,
        custom_instructions: Option<String>,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::Compact {
            custom_instructions,
        })
    }

    pub fn set_auto_compaction(
        &mut self,
        enabled: bool,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::SetAutoCompaction { enabled })
    }

    pub fn set_auto_retry(&mut self, enabled: bool) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::SetAutoRetry { enabled })
    }

    pub fn new_session(&mut self) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::ReplaceSession(
            crate::state::runtime::SessionMutation::New {
                parent_session: None,
            },
        ))
    }

    pub fn switch_session(
        &mut self,
        session_path: String,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        if self
            .runtime
            .session
            .data
            .as_ref()
            .and_then(|session| session.file.as_deref())
            == Some(session_path.as_str())
            || self.runtime.replacement_awaiting_state
        {
            return Vec::new();
        }
        self.intent(RuntimeIntent::ReplaceSession(
            crate::state::runtime::SessionMutation::Switch { session_path },
        ))
    }

    pub fn set_session_name(&mut self, name: String) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::SetSessionName { name })
    }

    pub fn export_html(
        &mut self,
        output_path: Option<String>,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::ExportHtml { output_path })
    }

    fn intent(&mut self, intent: RuntimeIntent) -> Vec<crate::state::runtime::RuntimeEffect> {
        let epoch = self.runtime.epoch;
        reduce(
            &mut self.runtime,
            StampedInput {
                generation: self.generation,
                epoch,
                observed_at: Instant::now(),
                input: RuntimeInput::Intent(intent),
            },
        )
    }
}

fn parse_bash_submission(text: &str) -> Option<Result<(String, bool), SubmissionRejection>> {
    let (remainder, exclude_from_context) = if let Some(command) = text.strip_prefix("!!") {
        (command, true)
    } else {
        let command = text.strip_prefix('!')?;
        (command, false)
    };
    let command = remainder.strip_prefix(' ').unwrap_or(remainder);
    if command.trim().is_empty() {
        Some(Err(SubmissionRejection::EmptyBash))
    } else {
        Some(Ok((command.to_owned(), exclude_from_context)))
    }
}

pub struct RuntimeController {
    core: ControllerCore,
    service: Arc<dyn RuntimeService>,
    worker: RuntimeWorkerHandle,
    catalog_worker: SessionCatalogWorker,
    catalog_generation: u64,
    catalog_status: CatalogStatus,
    catalog_sessions: Vec<SessionSummary>,
    catalog_corrupt: Vec<CorruptSession>,
    catalog_root: Option<SessionRoot>,
    catalog_error: Option<String>,
    _event_task: Task<()>,
    _catalog_task: Task<()>,
}

impl RuntimeController {
    pub fn new(
        workspace: impl Into<String>,
        service: Arc<dyn RuntimeService>,
        catalog_config: SessionCatalogConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let workspace = workspace.into();
        let worker = RuntimeWorkerHandle::spawn(Arc::clone(&service));
        let results = worker.results();
        let event_task = cx.spawn(async move |controller, cx| {
            while let Ok(result) = results.recv().await {
                let updated = controller.update(cx, |controller, cx| {
                    controller.receive(result);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        });
        let catalog_worker = SessionCatalogWorker::spawn(catalog_config);
        let catalog_results = catalog_worker.results();
        let catalog_task = cx.spawn(async move |controller, cx| {
            while let Ok(result) = catalog_results.recv().await {
                let updated = controller.update(cx, |controller, cx| {
                    controller.receive_catalog(result);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        });
        let catalog_generation = 1;
        let _ = catalog_worker.refresh(catalog_generation);
        Self {
            core: ControllerCore::new(workspace),
            service,
            worker,
            catalog_worker,
            catalog_generation,
            catalog_status: CatalogStatus::Loading,
            catalog_sessions: Vec::new(),
            catalog_corrupt: Vec::new(),
            catalog_root: None,
            catalog_error: None,
            _event_task: event_task,
            _catalog_task: catalog_task,
        }
    }

    pub fn projection(&self) -> ShellProjection {
        self.core.projection()
    }

    pub fn composer_projection(&self) -> ComposerProjection {
        self.core.composer_projection()
    }

    pub fn conversation_projection(&self) -> ConversationProjection {
        self.core.conversation_projection()
    }

    pub fn catalog_projection(&self) -> CatalogProjection {
        let session = self.core.runtime.session.data.as_ref();
        CatalogProjection {
            status: self.catalog_status,
            sessions: self.catalog_sessions.clone(),
            corrupt: self.catalog_corrupt.clone(),
            root: self.catalog_root.clone(),
            error: self.catalog_error.clone(),
            current_session_id: session.map(|session| session.id.to_string()),
            current_session_name: session.and_then(|session| session.name.clone()),
            current_session_file: session
                .and_then(|session| session.file.as_deref())
                .map(PathBuf::from),
            switching: self.core.runtime.replacement_awaiting_state,
        }
    }

    pub fn submit(
        &mut self,
        text: String,
        preference: SubmissionPreference,
        cx: &mut Context<Self>,
    ) -> Result<AcceptedSubmission, SubmissionRejection> {
        let (submission, effects) = self.core.submit(text, preference)?;
        self.send_effects(effects);
        cx.notify();
        Ok(submission)
    }

    pub fn abort(&mut self, cx: &mut Context<Self>) -> bool {
        let effects = self.core.abort();
        if effects.is_empty() {
            return false;
        }
        self.send_effects(effects);
        cx.notify();
        true
    }

    pub fn abort_bash(&mut self, cx: &mut Context<Self>) -> bool {
        let effects = self.core.abort_bash();
        if effects.is_empty() {
            return false;
        }
        self.send_effects(effects);
        cx.notify();
        true
    }

    pub fn abort_retry(&mut self, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(|core| core.abort_retry(), cx)
    }

    pub fn set_steering_mode(&mut self, mode: QueueDeliveryMode, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(|core| core.set_steering_mode(mode), cx)
    }

    pub fn set_follow_up_mode(&mut self, mode: QueueDeliveryMode, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(|core| core.set_follow_up_mode(mode), cx)
    }

    pub fn compact(&mut self, custom_instructions: Option<String>, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(|core| core.compact(custom_instructions), cx)
    }

    pub fn set_auto_compaction(&mut self, enabled: bool, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(|core| core.set_auto_compaction(enabled), cx)
    }

    pub fn set_auto_retry(&mut self, enabled: bool, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(|core| core.set_auto_retry(enabled), cx)
    }

    pub fn new_session(&mut self, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(ControllerCore::new_session, cx)
    }

    pub fn switch_session(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        let path = path.to_string_lossy().into_owned();
        self.send_core_effects(|core| core.switch_session(path), cx)
    }

    pub fn set_session_name(&mut self, name: String, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(|core| core.set_session_name(name), cx)
    }

    pub fn export_html(&mut self, output_path: Option<String>, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(|core| core.export_html(output_path), cx)
    }

    pub fn refresh_sessions(&mut self, cx: &mut Context<Self>) {
        self.start_catalog_refresh();
        cx.notify();
    }

    pub fn connect(&mut self, cx: &mut Context<Self>) {
        if !matches!(
            self.core.projection().action,
            Some(crate::state::RecoveryAction::Connect | crate::state::RecoveryAction::Retry)
        ) {
            return;
        }
        let (attempt, generation) = self.core.begin_connect();
        let resume_session = self
            .core
            .runtime
            .session
            .data
            .as_ref()
            .and_then(|session| session.file.as_deref())
            .map(PathBuf::from);
        self.service.set_resume_session(resume_session);
        if !self.worker.connect(attempt, generation) {
            self.core
                .apply_worker_result(WorkerResult::ConnectionFailed {
                    attempt,
                    generation,
                    failure: crate::services::runtime_worker::RuntimeStartFailure::new(
                        crate::services::runtime_worker::RuntimeStartFailureKind::Launch,
                        "The runtime worker is unavailable.",
                    ),
                });
        }
        cx.notify();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if self.core.begin_stop() {
            let _ = self.worker.stop();
            cx.notify();
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.worker.request_shutdown();
    }

    fn receive(&mut self, result: WorkerResult) {
        let before = self.catalog_refresh_identity();
        let effects = self.core.apply_worker_result(result);
        self.send_effects(effects);
        if before != self.catalog_refresh_identity() {
            self.start_catalog_refresh();
        }
    }

    fn receive_catalog(&mut self, result: CatalogWorkerResult) {
        if !catalog_result_is_current(self.catalog_generation, &result) {
            return;
        }
        match result.result {
            Ok(scan) => {
                self.catalog_status = if scan.sessions.is_empty() {
                    CatalogStatus::Empty
                } else {
                    CatalogStatus::Ready
                };
                self.catalog_sessions = scan.sessions;
                self.catalog_corrupt = scan.corrupt;
                self.catalog_root = Some(scan.root);
                self.catalog_error = None;
            }
            Err(error) => {
                self.catalog_status = if self.catalog_sessions.is_empty() {
                    CatalogStatus::Inaccessible
                } else {
                    CatalogStatus::Stale
                };
                self.catalog_error = Some(error.summary);
            }
        }
    }

    fn start_catalog_refresh(&mut self) {
        self.catalog_generation = self.catalog_generation.saturating_add(1);
        self.catalog_status = CatalogStatus::Loading;
        self.catalog_error = None;
        let _ = self.catalog_worker.refresh(self.catalog_generation);
    }

    fn catalog_refresh_identity(
        &self,
    ) -> (
        crate::services::rpc::SessionEpoch,
        Option<String>,
        Option<String>,
    ) {
        let session = self.core.runtime.session.data.as_ref();
        (
            self.core.runtime.display_epoch,
            session.and_then(|session| session.file.clone()),
            session.and_then(|session| session.name.clone()),
        )
    }

    fn send_core_effects(
        &mut self,
        operation: impl FnOnce(&mut ControllerCore) -> Vec<crate::state::runtime::RuntimeEffect>,
        cx: &mut Context<Self>,
    ) -> bool {
        let effects = operation(&mut self.core);
        if effects.is_empty() {
            return false;
        }
        self.send_effects(effects);
        cx.notify();
        true
    }

    fn send_effects(&self, effects: Vec<crate::state::runtime::RuntimeEffect>) {
        let attempt = self.core.attempt();
        for effect in effects {
            let _ = self.worker.execute(attempt, effect);
        }
    }
}

fn catalog_result_is_current(generation: u64, result: &CatalogWorkerResult) -> bool {
    result.generation == generation
}

impl Drop for RuntimeController {
    fn drop(&mut self) {
        self.worker.request_shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::runtime_worker::{RuntimeStartFailure, RuntimeStartFailureKind};

    #[test]
    fn newer_attempt_rejects_late_prior_attempt_results() {
        let mut core = ControllerCore::new("workspace");
        let (first_attempt, first_generation) = core.begin_connect();
        let (second_attempt, second_generation) = core.begin_connect();
        assert!(second_attempt > first_attempt);
        assert!(second_generation > first_generation);

        core.apply_worker_result(WorkerResult::Connected {
            attempt: first_attempt,
            generation: first_generation,
        });
        assert_eq!(core.status(), ControllerStatus::Connecting);
        assert_eq!(core.runtime().generation, ConnectionGeneration::default());
        assert_eq!(core.stale_attempts_ignored(), 1);

        core.apply_worker_result(WorkerResult::Connected {
            attempt: second_attempt,
            generation: second_generation,
        });
        assert_eq!(core.status(), ControllerStatus::Active);
        assert_eq!(core.runtime().generation, second_generation);
    }

    #[test]
    fn retry_progresses_error_to_loading_to_ready_on_new_generation() {
        let mut core = ControllerCore::new("workspace");
        let (first_attempt, first_generation) = core.begin_connect();
        core.apply_worker_result(WorkerResult::ConnectionFailed {
            attempt: first_attempt,
            generation: first_generation,
            failure: RuntimeStartFailure::new(
                RuntimeStartFailureKind::Readiness,
                "Pi did not become ready.",
            ),
        });
        assert_eq!(core.status(), ControllerStatus::Failed);

        let (second_attempt, second_generation) = core.begin_connect();
        assert_eq!(core.status(), ControllerStatus::Connecting);
        core.apply_worker_result(WorkerResult::Connected {
            attempt: second_attempt,
            generation: second_generation,
        });
        assert_eq!(core.status(), ControllerStatus::Active);
        assert_eq!(core.runtime().generation, second_generation);
    }

    #[test]
    fn mismatched_connection_generation_is_rejected_within_current_attempt() {
        let mut core = ControllerCore::new("workspace");
        let (attempt, generation) = core.begin_connect();
        core.apply_worker_result(WorkerResult::Connected {
            attempt,
            generation: generation.next(),
        });
        assert_eq!(core.status(), ControllerStatus::Connecting);
        assert_eq!(core.runtime().generation, ConnectionGeneration::default());
    }

    #[test]
    fn stale_input_generation_cannot_overwrite_connected_state() {
        let mut core = ControllerCore::new("workspace");
        let (attempt, generation) = core.begin_connect();
        core.apply_worker_result(WorkerResult::Connected {
            attempt,
            generation,
        });
        let ignored_before = core.runtime().stale_inputs_ignored;
        core.apply_worker_result(WorkerResult::Input {
            attempt,
            input: Box::new(StampedInput {
                generation: ConnectionGeneration::default(),
                epoch: crate::services::rpc::SessionEpoch::default(),
                observed_at: Instant::now(),
                input: RuntimeInput::Disconnected {
                    error: crate::state::runtime::SafeError::new(
                        crate::state::runtime::ErrorKind::Disconnected,
                        "stale",
                    ),
                },
            }),
        });
        assert_eq!(
            core.runtime().stale_inputs_ignored,
            ignored_before.saturating_add(1)
        );
        assert_eq!(
            core.runtime().lifecycle,
            crate::state::runtime::RuntimeLifecycle::Loading
        );
    }

    fn ready_core() -> ControllerCore {
        use crate::services::rpc::SessionId;
        use crate::state::runtime::{
            ModelSummary, QueueDeliveryMode, RuntimeThinkingLevel, SessionSnapshot,
        };

        let mut core = ControllerCore::new("workspace");
        core.status = ControllerStatus::Active;
        core.generation = ConnectionGeneration::new(1);
        core.runtime = RuntimeState::new(core.generation);
        core.runtime.lifecycle = RuntimeLifecycle::Ready;
        core.runtime.session.ready(SessionSnapshot {
            id: SessionId::from("session"),
            file: None,
            name: None,
            model: Some(ModelSummary {
                provider: "test".to_owned(),
                id: "model".to_owned(),
                name: "Model".to_owned(),
                reasoning: false,
                context_window: 100_000,
                max_tokens: 4_096,
                supports_images: false,
            }),
            thinking_level: RuntimeThinkingLevel::Off,
            steering_mode: QueueDeliveryMode::All,
            follow_up_mode: QueueDeliveryMode::All,
            auto_compaction_enabled: true,
            message_count: 0,
        });
        core
    }

    fn complete_submission(
        core: &mut ControllerCore,
        request: crate::state::runtime::RuntimeRequest,
    ) {
        let epoch = core.runtime.epoch;
        reduce(
            &mut core.runtime,
            StampedInput {
                generation: core.generation,
                epoch,
                observed_at: Instant::now(),
                input: RuntimeInput::Response {
                    request,
                    result: Ok(crate::state::runtime::NormalizedResponse::Accepted),
                },
            },
        );
    }

    #[test]
    fn controller_routes_prompt_steer_follow_up_and_suppresses_rapid_submit() {
        use crate::state::runtime::{EffectKind, RuntimeRequest};

        let mut core = ready_core();
        let (prompt, effects) = core
            .submit("First".to_owned(), SubmissionPreference::Default)
            .expect("prompt");
        assert_eq!(
            prompt.kind,
            AcceptedSubmissionKind::Prompt(SubmissionKind::Prompt)
        );
        let EffectKind::Request(prompt_request) = effects[0].effect.clone() else {
            panic!("expected request");
        };
        assert!(matches!(
            prompt_request,
            RuntimeRequest::Submit {
                kind: SubmissionKind::Prompt,
                ..
            }
        ));
        assert_eq!(
            core.submit("Duplicate".to_owned(), SubmissionPreference::Default),
            Err(SubmissionRejection::Pending)
        );

        complete_submission(&mut core, prompt_request);
        assert_eq!(core.runtime.lifecycle, RuntimeLifecycle::Running);
        let (steer, effects) = core
            .submit("Adjust".to_owned(), SubmissionPreference::Default)
            .expect("steer");
        assert_eq!(
            steer.kind,
            AcceptedSubmissionKind::Prompt(SubmissionKind::Steer)
        );
        let EffectKind::Request(steer_request) = effects[0].effect.clone() else {
            panic!("expected request");
        };
        complete_submission(&mut core, steer_request);

        let (follow_up, effects) = core
            .submit("Then verify".to_owned(), SubmissionPreference::FollowUp)
            .expect("follow-up");
        assert_eq!(
            follow_up.kind,
            AcceptedSubmissionKind::Prompt(SubmissionKind::FollowUp)
        );
        assert!(matches!(
            effects[0].effect,
            EffectKind::Request(RuntimeRequest::Submit {
                kind: SubmissionKind::FollowUp,
                ..
            })
        ));
    }

    #[test]
    fn controller_routes_bang_commands_to_direct_bash_with_exclusion_semantics() {
        use crate::state::runtime::{EffectKind, RuntimeRequest};

        let mut core = ready_core();
        let (included, effects) = core
            .submit("!printf 'a b'\n".to_owned(), SubmissionPreference::Default)
            .expect("included bash");
        assert_eq!(
            included.kind,
            AcceptedSubmissionKind::Bash {
                exclude_from_context: false
            }
        );
        assert!(matches!(
            &effects[0].effect,
            EffectKind::Request(RuntimeRequest::ExecuteBash {
                command,
                exclude_from_context: false,
                ..
            }) if command == "printf 'a b'\n"
        ));
        assert!(matches!(
            crate::services::rpc::dispatch_for_effect(&effects[0]),
            crate::services::rpc::RpcDispatch::Command(crate::services::rpc::Command::Bash {
                command,
                exclude_from_context: Some(false),
            }) if command == "printf 'a b'\n"
        ));
        assert_eq!(
            core.submit("!!echo later".to_owned(), SubmissionPreference::Default),
            Err(SubmissionRejection::BashRunning)
        );

        let mut core = ready_core();
        let (excluded, effects) = core
            .submit("!! cargo test".to_owned(), SubmissionPreference::Default)
            .expect("excluded bash");
        assert_eq!(
            excluded.kind,
            AcceptedSubmissionKind::Bash {
                exclude_from_context: true
            }
        );
        assert!(matches!(
            &effects[0].effect,
            EffectKind::Request(RuntimeRequest::ExecuteBash {
                command,
                exclude_from_context: true,
                ..
            }) if command == "cargo test"
        ));
        assert_eq!(
            parse_bash_submission("!!   "),
            Some(Err(SubmissionRejection::EmptyBash))
        );
        assert!(parse_bash_submission("ordinary prompt").is_none());
    }

    #[test]
    fn conversation_projection_hides_non_display_custom_messages() {
        use crate::state::runtime::{
            BlockKey, MessageBlock, MessageKey, MessageRole, RuntimeMessage,
        };

        fn custom(key: &str, visible: bool) -> RuntimeMessage {
            RuntimeMessage {
                key: MessageKey(key.to_owned()),
                role: MessageRole::Custom,
                timestamp: 1,
                content: vec![MessageBlock::Custom {
                    key: BlockKey("custom:0".to_owned()),
                    kind: "synthetic".to_owned(),
                    text: "private context".to_owned(),
                }],
                visible,
                terminal: true,
                stop_reason: None,
                error: None,
                assistant: None,
            }
        }

        let mut core = ready_core();
        core.runtime
            .messages
            .ready(vec![custom("visible", true), custom("hidden", false)]);
        let projection = core.conversation_projection();
        assert_eq!(projection.messages.len(), 1);
        assert_eq!(projection.messages[0].key, MessageKey("visible".to_owned()));
    }

    #[test]
    fn disconnected_projection_hides_accepted_optimistic_input() {
        let mut core = ready_core();
        let (submission, effects) = core
            .submit(
                "May not be durable".to_owned(),
                SubmissionPreference::Default,
            )
            .expect("prompt");
        let crate::state::runtime::EffectKind::Request(request) = effects[0].effect.clone() else {
            panic!("expected request");
        };
        complete_submission(&mut core, request);
        assert_eq!(core.conversation_projection().accepted_user_inputs.len(), 1);

        let epoch = core.runtime.epoch;
        reduce(
            &mut core.runtime,
            StampedInput {
                generation: core.generation,
                epoch,
                observed_at: Instant::now(),
                input: RuntimeInput::Disconnected {
                    error: crate::state::runtime::SafeError::new(
                        crate::state::runtime::ErrorKind::Disconnected,
                        "Connection closed",
                    ),
                },
            },
        );
        assert_eq!(submission.request.as_str(), "composer-1-0-1");
        assert!(
            core.conversation_projection()
                .accepted_user_inputs
                .is_empty()
        );
    }

    #[test]
    fn stale_catalog_scan_generation_is_rejected() {
        let result = CatalogWorkerResult {
            generation: 3,
            result: Err(crate::services::session_catalog::SessionCatalogError {
                summary: "stale".to_owned(),
            }),
        };
        assert!(!catalog_result_is_current(4, &result));
        assert!(catalog_result_is_current(3, &result));
    }

    #[test]
    fn controller_rejects_empty_and_preserves_uncertain_submission_identity() {
        let mut core = ready_core();
        assert_eq!(
            core.submit("  \n".to_owned(), SubmissionPreference::Default),
            Err(SubmissionRejection::Empty)
        );
        let (submission, _) = core
            .submit("Keep this draft".to_owned(), SubmissionPreference::Default)
            .expect("prompt");
        let epoch = core.runtime.epoch;
        reduce(
            &mut core.runtime,
            StampedInput {
                generation: core.generation,
                epoch,
                observed_at: Instant::now(),
                input: RuntimeInput::Disconnected {
                    error: crate::state::runtime::SafeError::new(
                        crate::state::runtime::ErrorKind::Disconnected,
                        "Connection closed",
                    ),
                },
            },
        );
        assert_eq!(
            core.runtime.prompt_delivery,
            PromptDelivery::Uncertain {
                request: submission.request,
                kind: SubmissionKind::Prompt,
            }
        );
    }
}
