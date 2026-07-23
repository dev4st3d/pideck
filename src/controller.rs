//! GPUI-owned runtime controller and its pure generation gate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gpui::{Context, Task};

use crate::model_runtime::{
    AuthFlow, AuthMethod, AuthPrompt, CatalogPhase, ModelCatalogSnapshot, ModelChangePolicy,
    ModelIdentity, ModelRuntimeState, ThinkingLevel, model_change_policy,
};
use crate::orchestration::{
    OrchestrationAction, OrchestrationActionRequest, OrchestrationPhase, OrchestrationSnapshot,
    OrchestrationState,
};
use crate::resource_center::{ResourceCenterState, ResourceInventorySnapshot, ResourcePhase};
use crate::services::rpc::{ConnectionGeneration, RequestId, SessionEpoch};
use crate::services::runtime_worker::{
    AttemptGeneration, RuntimeService, RuntimeWorkerHandle, WorkerResult,
};
use crate::services::sdk_bridge::{
    BridgeCapabilities, BridgeCommand, BridgeErrorKind, BridgeEvent, BridgeWorkerResult,
    OrchestrationEvent, ResourceEvent, SdkBridgeWorker, SensitiveValue, decode_resource_snapshot,
};
use crate::services::session_catalog::{
    CatalogWorkerResult, CorruptSession, SessionCatalogConfig, SessionCatalogWorker, SessionRoot,
    SessionSummary,
};
use crate::state::reducer::reduce;
use crate::state::runtime::{
    BashExecution, BashStatus, CommandSource, CompactionState, DialogAnswer, ExtensionDialog,
    ExtensionFailure, ExtensionStatus, ExtensionWidget, FacetStatus, ModelSummary, PromptDelivery,
    QueueContents, QueueDeliveryMode, RetryState, RuntimeCommand, RuntimeForkMessage, RuntimeInput,
    RuntimeIntent, RuntimeLifecycle, RuntimeMessage, RuntimeNotification, RuntimeOperation,
    RuntimeState, RuntimeThinkingLevel, RuntimeTreeNode, SafeError, StampedInput, SubmissionKind,
    ToolExecution,
};
use crate::state::{ControllerStatus, ShellProjection};

fn session_paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .replace('\\', "/")
            .eq_ignore_ascii_case(&right.to_string_lossy().replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

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
pub struct CommandCatalogProjection {
    pub status: FacetStatus,
    pub commands: Vec<RuntimeCommand>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryProjection {
    pub status: FacetStatus,
    pub tree: Vec<RuntimeTreeNode>,
    pub leaf_id: Option<crate::services::rpc::EntryId>,
    pub fork_messages: Vec<RuntimeForkMessage>,
    pub lifecycle: RuntimeLifecycle,
    pub switching: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionUiProjection {
    pub active_dialog: Option<ExtensionDialog>,
    pub queued_dialogs: usize,
    pub statuses: Vec<(String, ExtensionStatus)>,
    pub widgets: Vec<(String, ExtensionWidget)>,
    pub title: Option<String>,
    pub errors: Vec<ExtensionFailure>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeOperationKind {
    Navigate,
    SetLabel,
    ExportJsonl,
    ImportJsonl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeProjection {
    pub capabilities: Option<BridgeCapabilities>,
    pub unavailable: Option<String>,
    pub pending: Option<(u64, BridgeOperationKind)>,
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageProjection {
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub context_percent: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub estimated_cost: Option<f64>,
    pub pricing_known: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRuntimeProjection {
    pub phase: CatalogPhase,
    pub catalog: Option<ModelCatalogSnapshot>,
    pub stock_models: Vec<ModelSummary>,
    pub auth: Option<AuthFlow>,
    pub feedback: Option<String>,
    pub active_model: Option<ModelIdentity>,
    pub active_thinking: Option<ThinkingLevel>,
    pub requested_thinking: Option<ThinkingLevel>,
    pub effective_thinking: Option<ThinkingLevel>,
    pub clamp_notice: Option<String>,
    pub model_change_policy: ModelChangePolicy,
    pub usage: UsageProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceCenterProjection {
    pub phase: ResourcePhase,
    pub snapshot: Option<ResourceInventorySnapshot>,
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrchestrationProjection {
    pub phase: OrchestrationPhase,
    pub snapshot: Option<OrchestrationSnapshot>,
    pub feedback: Option<String>,
    pub pending_actions: usize,
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

    fn take_runtime_notifications(&mut self) -> Vec<RuntimeNotification> {
        self.runtime.notifications.drain(..).collect()
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

    pub fn history_projection(&self) -> HistoryProjection {
        HistoryProjection {
            status: self.runtime.tree.status.clone(),
            tree: self.runtime.tree.data.clone().unwrap_or_default(),
            leaf_id: self.runtime.tree_leaf_id.clone(),
            fork_messages: self.runtime.fork_messages.data.clone().unwrap_or_default(),
            lifecycle: self.runtime.lifecycle,
            switching: self.runtime.replacement_awaiting_state,
        }
    }

    pub fn extension_ui_projection(&self) -> ExtensionUiProjection {
        ExtensionUiProjection {
            active_dialog: self.runtime.dialogs.front().cloned(),
            queued_dialogs: self.runtime.dialogs.len().saturating_sub(1),
            statuses: self
                .runtime
                .statuses
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            widgets: self
                .runtime
                .widgets
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            title: self.runtime.title.clone(),
            errors: self.runtime.extension_errors.iter().cloned().collect(),
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

    pub fn invoke_dynamic_command(
        &mut self,
        text: String,
        source: CommandSource,
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
        let request = self.next_composer_request("command");
        let effects = self.intent(RuntimeIntent::InvokeCommand {
            request: request.clone(),
            text,
            kind,
            source,
        });
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

    pub fn refresh_commands(&mut self) -> Vec<crate::state::runtime::RuntimeEffect> {
        if self.status != ControllerStatus::Active {
            return Vec::new();
        }
        self.intent(RuntimeIntent::RefreshCommands)
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

    pub fn answer_dialog(
        &mut self,
        request: RequestId,
        answer: DialogAnswer,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::AnswerDialog { request, answer })
    }

    pub fn expire_dialog(&mut self, request: RequestId) {
        let _ = self.intent(RuntimeIntent::ExpireDialog { request });
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
        if self.composer_projection().runtime != ComposerRuntime::Idle {
            return Vec::new();
        }
        self.intent(RuntimeIntent::ReplaceSession(
            crate::state::runtime::SessionMutation::New {
                parent_session: None,
            },
        ))
    }

    pub fn fork_before(
        &mut self,
        entry_id: crate::services::rpc::EntryId,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        if self.composer_projection().runtime != ComposerRuntime::Idle {
            return Vec::new();
        }
        self.intent(RuntimeIntent::ReplaceSession(
            crate::state::runtime::SessionMutation::Fork { entry_id },
        ))
    }

    pub fn clone_current_path(&mut self) -> Vec<crate::state::runtime::RuntimeEffect> {
        if self.composer_projection().runtime != ComposerRuntime::Idle {
            return Vec::new();
        }
        self.intent(RuntimeIntent::ReplaceSession(
            crate::state::runtime::SessionMutation::Clone,
        ))
    }

    pub fn reload_current_session(
        &mut self,
        editor_text: Option<String>,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        let Some(session_path) = self
            .runtime
            .session
            .data
            .as_ref()
            .and_then(|session| session.file.clone())
        else {
            return Vec::new();
        };
        let effects = self.intent(RuntimeIntent::ReplaceSession(
            crate::state::runtime::SessionMutation::Switch { session_path },
        ));
        self.runtime.requested_editor_text = editor_text;
        effects
    }

    pub fn switch_session(
        &mut self,
        session_path: String,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        if self.composer_projection().runtime != ComposerRuntime::Idle {
            return Vec::new();
        }
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

    pub fn set_model(
        &mut self,
        provider: String,
        id: String,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::SetModel { provider, id })
    }

    pub fn set_thinking_level(
        &mut self,
        level: RuntimeThinkingLevel,
    ) -> Vec<crate::state::runtime::RuntimeEffect> {
        self.intent(RuntimeIntent::SetThinkingLevel(level))
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

fn model_thinking(level: RuntimeThinkingLevel) -> ThinkingLevel {
    match level {
        RuntimeThinkingLevel::Off => ThinkingLevel::Off,
        RuntimeThinkingLevel::Minimal => ThinkingLevel::Minimal,
        RuntimeThinkingLevel::Low => ThinkingLevel::Low,
        RuntimeThinkingLevel::Medium => ThinkingLevel::Medium,
        RuntimeThinkingLevel::High => ThinkingLevel::High,
        RuntimeThinkingLevel::Xhigh => ThinkingLevel::Xhigh,
        RuntimeThinkingLevel::Max => ThinkingLevel::Max,
    }
}

fn runtime_thinking(level: ThinkingLevel) -> RuntimeThinkingLevel {
    match level {
        ThinkingLevel::Off => RuntimeThinkingLevel::Off,
        ThinkingLevel::Minimal => RuntimeThinkingLevel::Minimal,
        ThinkingLevel::Low => RuntimeThinkingLevel::Low,
        ThinkingLevel::Medium => RuntimeThinkingLevel::Medium,
        ThinkingLevel::High => RuntimeThinkingLevel::High,
        ThinkingLevel::Xhigh => RuntimeThinkingLevel::Xhigh,
        ThinkingLevel::Max => RuntimeThinkingLevel::Max,
    }
}

pub struct RuntimeController {
    core: ControllerCore,
    service: Arc<dyn RuntimeService>,
    worker: RuntimeWorkerHandle,
    preferred_session_file: Option<PathBuf>,
    catalog_worker: SessionCatalogWorker,
    catalog_generation: u64,
    catalog_status: CatalogStatus,
    catalog_sessions: Vec<SessionSummary>,
    catalog_corrupt: Vec<CorruptSession>,
    catalog_root: Option<SessionRoot>,
    catalog_error: Option<String>,
    bridge_worker: SdkBridgeWorker,
    bridge_capabilities: Option<BridgeCapabilities>,
    bridge_unavailable: Option<String>,
    bridge_pending: Option<(u64, BridgeOperationKind)>,
    bridge_feedback: Option<String>,
    next_bridge_operation: u64,
    model_runtime: ModelRuntimeState,
    model_snapshot_pending: Option<u64>,
    resource_center: ResourceCenterState,
    resource_snapshot_pending: Option<u64>,
    orchestration: OrchestrationState,
    orchestration_snapshot_pending: Option<u64>,
    orchestration_actions_pending: HashMap<u64, OrchestrationAction>,
    requested_thinking: Option<ThinkingLevel>,
    clamp_notice: Option<String>,
    _event_task: Task<()>,
    _catalog_task: Task<()>,
    _bridge_task: Task<()>,
}

impl RuntimeController {
    pub fn new(
        workspace: impl Into<String>,
        service: Arc<dyn RuntimeService>,
        catalog_config: SessionCatalogConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let workspace = workspace.into();
        let bridge_worker = SdkBridgeWorker::spawn(PathBuf::from(&workspace));
        let bridge_results = bridge_worker.results();
        let bridge_task = cx.spawn(async move |controller, cx| {
            while let Ok(result) = bridge_results.recv().await {
                let updated = controller.update(cx, |controller, cx| {
                    controller.receive_bridge(result);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        });
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
            preferred_session_file: None,
            catalog_worker,
            catalog_generation,
            catalog_status: CatalogStatus::Loading,
            catalog_sessions: Vec::new(),
            catalog_corrupt: Vec::new(),
            catalog_root: None,
            catalog_error: None,
            bridge_worker,
            bridge_capabilities: None,
            bridge_unavailable: None,
            bridge_pending: None,
            bridge_feedback: None,
            next_bridge_operation: 1,
            model_runtime: ModelRuntimeState::default(),
            model_snapshot_pending: None,
            resource_center: ResourceCenterState::default(),
            resource_snapshot_pending: None,
            orchestration: OrchestrationState::default(),
            orchestration_snapshot_pending: None,
            orchestration_actions_pending: HashMap::new(),
            requested_thinking: None,
            clamp_notice: None,
            _event_task: event_task,
            _catalog_task: catalog_task,
            _bridge_task: bridge_task,
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

    pub fn history_projection(&self) -> HistoryProjection {
        self.core.history_projection()
    }

    pub fn extension_ui_projection(&self) -> ExtensionUiProjection {
        self.core.extension_ui_projection()
    }

    pub fn command_catalog_projection(&self) -> CommandCatalogProjection {
        CommandCatalogProjection {
            status: self.core.runtime.commands.status.clone(),
            commands: self.core.runtime.commands.data.clone().unwrap_or_default(),
        }
    }

    pub fn take_runtime_notifications(&mut self) -> Vec<RuntimeNotification> {
        self.core.take_runtime_notifications()
    }

    pub fn take_requested_editor_text(&mut self) -> Option<String> {
        self.core.runtime.requested_editor_text.take()
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

    pub fn bridge_projection(&self) -> BridgeProjection {
        BridgeProjection {
            capabilities: self.bridge_capabilities.clone(),
            unavailable: self.bridge_unavailable.clone(),
            pending: self.bridge_pending,
            feedback: self.bridge_feedback.clone(),
        }
    }

    pub fn model_runtime_projection(&self) -> ModelRuntimeProjection {
        let session = self.core.runtime.session.data.as_ref();
        let active_model = session.and_then(|session| {
            session.model.as_ref().map(|model| ModelIdentity {
                provider: model.provider.clone(),
                id: model.id.clone(),
            })
        });
        let active_thinking = session.map(|session| model_thinking(session.thinking_level));
        let pricing_known = active_model
            .as_ref()
            .and_then(|identity| self.model_runtime.catalog.as_ref()?.model(identity))
            .is_some_and(|model| {
                !model.pricing.rates.is_zero()
                    || model.pricing.tiers.iter().any(|tier| !tier.rates.is_zero())
            });
        let stats = self.core.runtime.stats.data.as_ref();
        let reasoning = self
            .core
            .runtime
            .messages
            .data
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|message| message.assistant.as_ref()?.usage.reasoning)
            .reduce(u64::saturating_add);
        ModelRuntimeProjection {
            phase: self.model_runtime.phase.clone(),
            catalog: self.model_runtime.catalog.clone(),
            stock_models: self.core.runtime.models.data.clone().unwrap_or_default(),
            auth: self.model_runtime.auth.clone(),
            feedback: self.model_runtime.feedback.clone(),
            active_model,
            active_thinking,
            requested_thinking: self.requested_thinking,
            effective_thinking: active_thinking,
            clamp_notice: self.clamp_notice.clone(),
            model_change_policy: model_change_policy(
                self.core.status() == ControllerStatus::Active,
                !matches!(
                    self.core.runtime.lifecycle,
                    RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
                ),
            ),
            usage: UsageProjection {
                context_tokens: stats.and_then(|stats| stats.context_tokens),
                context_window: stats.and_then(|stats| stats.context_window),
                context_percent: stats.and_then(|stats| stats.context_percent),
                input_tokens: stats.map(|stats| stats.input_tokens),
                output_tokens: stats.map(|stats| stats.output_tokens),
                cache_read_tokens: stats.map(|stats| stats.cache_read_tokens),
                cache_write_tokens: stats.map(|stats| stats.cache_write_tokens),
                reasoning_tokens: reasoning,
                total_tokens: stats.map(|stats| stats.total_tokens),
                estimated_cost: stats.map(|stats| stats.cost),
                pricing_known,
            },
        }
    }

    pub fn resource_center_projection(&self) -> ResourceCenterProjection {
        ResourceCenterProjection {
            phase: self.resource_center.phase.clone(),
            snapshot: self.resource_center.snapshot.clone(),
            feedback: self.resource_center.feedback.clone(),
        }
    }

    pub fn orchestration_projection(&self) -> OrchestrationProjection {
        OrchestrationProjection {
            phase: self.orchestration.phase,
            snapshot: self.orchestration.snapshot.clone(),
            feedback: self.orchestration.feedback.clone(),
            pending_actions: self.orchestration_actions_pending.len(),
        }
    }

    pub fn orchestration_action(
        &mut self,
        action: OrchestrationAction,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self
            .bridge_capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.orchestration)
        {
            self.orchestration
                .fail("The installed Pi bridge does not expose orchestration actions.");
            cx.notify();
            return false;
        }
        let Some(session_id) = self
            .core
            .runtime
            .session
            .data
            .as_ref()
            .map(|session| session.id.to_string())
        else {
            self.orchestration
                .fail("No active Pi session is available for this action.");
            cx.notify();
            return false;
        };
        let operation = self.take_bridge_operation();
        let request = OrchestrationActionRequest {
            session_id,
            action: action.clone(),
        };
        let accepted = self.bridge_worker.execute(
            operation,
            BridgeCommand::OrchestrationAction { action: request },
        );
        if accepted {
            self.orchestration_actions_pending.insert(operation, action);
            self.orchestration.feedback = Some("Sending action to Pi…".to_owned());
        } else {
            self.orchestration
                .fail("The orchestration bridge is unavailable.");
        }
        cx.notify();
        accepted
    }

    pub fn reload_resources(&mut self, cx: &mut Context<Self>) -> bool {
        if !self
            .bridge_capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.resource_reload)
        {
            return false;
        }
        let Some(operation) = self.resource_center.begin_refresh() else {
            return false;
        };
        let accepted = self
            .bridge_worker
            .execute(operation, BridgeCommand::ReloadResources);
        if accepted {
            self.resource_snapshot_pending = Some(operation);
        } else {
            self.resource_center
                .apply_failure("The resource bridge is unavailable.".to_owned());
        }
        cx.notify();
        accepted
    }

    pub fn set_active_model(&mut self, identity: ModelIdentity, cx: &mut Context<Self>) -> bool {
        if self.model_runtime_projection().model_change_policy != ModelChangePolicy::Allowed {
            self.model_runtime.feedback =
                Some("Model changes apply only after the current stream settles.".to_owned());
            cx.notify();
            return false;
        }
        self.requested_thinking = None;
        self.clamp_notice = None;
        self.send_core_effects(|core| core.set_model(identity.provider, identity.id), cx)
    }

    pub fn set_active_thinking(
        &mut self,
        requested: ThinkingLevel,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.model_runtime_projection().model_change_policy != ModelChangePolicy::Allowed {
            self.model_runtime.feedback =
                Some("Thinking changes apply only after the current stream settles.".to_owned());
            cx.notify();
            return false;
        }
        let active = self.model_runtime_projection().active_model;
        let effective = active
            .as_ref()
            .and_then(|identity| self.model_runtime.catalog.as_ref()?.model(identity))
            .map(|model| model.clamp_thinking(requested))
            .unwrap_or(requested);
        self.requested_thinking = Some(requested);
        self.clamp_notice = (effective != requested).then(|| {
            format!(
                "Requested {}. This model uses {} instead.",
                requested.label(),
                effective.label()
            )
        });
        self.send_core_effects(
            |core| core.set_thinking_level(runtime_thinking(effective)),
            cx,
        )
    }

    pub fn refresh_model_catalog(&mut self, cx: &mut Context<Self>) -> bool {
        if !self
            .bridge_capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.model_runtime)
        {
            return false;
        }
        let (operation, start) = self.model_runtime.begin_refresh();
        if !start {
            return false;
        }
        let accepted = self
            .bridge_worker
            .execute(operation, BridgeCommand::RefreshModels);
        if !accepted {
            self.model_runtime.apply_refresh(
                operation,
                Err("The model bridge is unavailable.".to_owned()),
            );
        }
        cx.notify();
        accepted
    }

    pub fn login_provider(
        &mut self,
        provider: String,
        method: AuthMethod,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.model_runtime.auth.is_some()
            || !self
                .bridge_capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.provider_auth)
        {
            return false;
        }
        let operation = self.model_runtime.start_auth(provider.clone(), method);
        let accepted = self.bridge_worker.execute(
            operation,
            BridgeCommand::LoginProvider {
                operation_id: operation,
                provider,
                auth_type: method,
            },
        );
        if !accepted {
            self.model_runtime.finish_auth(
                operation,
                Err("The model bridge is unavailable.".to_owned()),
            );
        }
        cx.notify();
        accepted
    }

    pub fn answer_auth_prompt(
        &mut self,
        prompt: &AuthPrompt,
        value: String,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(flow) = self.model_runtime.auth.as_ref() else {
            return false;
        };
        let operation_id = flow.operation;
        let command_id = self.model_runtime.take_operation();
        let accepted = self.bridge_worker.execute(
            command_id,
            BridgeCommand::AuthRespond {
                operation_id,
                prompt_id: prompt.prompt_id.clone(),
                value: SensitiveValue::new(value),
            },
        );
        cx.notify();
        accepted
    }

    pub fn cancel_provider_auth(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(operation) = self.model_runtime.auth.as_ref().map(|flow| flow.operation) else {
            return false;
        };
        self.model_runtime.cancel_auth(operation);
        let accepted = self.bridge_worker.cancel(operation);
        cx.notify();
        accepted
    }

    pub fn logout_provider(&mut self, provider: String, cx: &mut Context<Self>) -> bool {
        let operation = self.model_runtime.take_operation();
        let accepted = self
            .bridge_worker
            .execute(operation, BridgeCommand::LogoutProvider { provider });
        cx.notify();
        accepted
    }

    pub fn set_model_defaults(
        &mut self,
        model: Option<ModelIdentity>,
        thinking: Option<ThinkingLevel>,
        cx: &mut Context<Self>,
    ) -> bool {
        let operation = self.model_runtime.take_operation();
        let accepted = self.bridge_worker.execute(
            operation,
            BridgeCommand::SetModelDefaults { model, thinking },
        );
        cx.notify();
        accepted
    }

    pub fn set_model_scope(&mut self, models: Vec<ModelIdentity>, cx: &mut Context<Self>) -> bool {
        let operation = self.model_runtime.take_operation();
        let accepted = self
            .bridge_worker
            .execute(operation, BridgeCommand::SetModelScope { models });
        cx.notify();
        accepted
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

    pub fn invoke_dynamic_command(
        &mut self,
        text: String,
        source: CommandSource,
        preference: SubmissionPreference,
        cx: &mut Context<Self>,
    ) -> Result<AcceptedSubmission, SubmissionRejection> {
        let (submission, effects) = self.core.invoke_dynamic_command(text, source, preference)?;
        self.send_effects(effects);
        cx.notify();
        Ok(submission)
    }

    pub fn refresh_commands(&mut self, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(ControllerCore::refresh_commands, cx)
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

    pub fn answer_dialog(
        &mut self,
        request: RequestId,
        answer: DialogAnswer,
        cx: &mut Context<Self>,
    ) -> bool {
        self.send_core_effects(|core| core.answer_dialog(request, answer), cx)
    }

    pub fn expire_dialog(&mut self, request: RequestId, cx: &mut Context<Self>) {
        self.core.expire_dialog(request);
        cx.notify();
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

    pub fn fork_before(
        &mut self,
        entry_id: crate::services::rpc::EntryId,
        cx: &mut Context<Self>,
    ) -> bool {
        self.send_core_effects(|core| core.fork_before(entry_id), cx)
    }

    pub fn clone_current_path(&mut self, cx: &mut Context<Self>) -> bool {
        self.send_core_effects(ControllerCore::clone_current_path, cx)
    }

    pub fn navigate_tree(
        &mut self,
        target_id: crate::services::rpc::EntryId,
        summarize: bool,
        custom_instructions: Option<String>,
        label: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((session_path, cwd)) = self.bridge_session_context() else {
            return false;
        };
        if !self
            .bridge_capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.navigate_tree)
            || self.bridge_pending.is_some()
        {
            return false;
        }
        self.start_bridge_operation(
            BridgeOperationKind::Navigate,
            BridgeCommand::NavigateTree {
                session_path,
                cwd,
                target_id: target_id.to_string(),
                summarize,
                custom_instructions,
                replace_instructions: false,
                label,
            },
            cx,
        )
    }

    pub fn set_tree_label(
        &mut self,
        target_id: crate::services::rpc::EntryId,
        label: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((session_path, cwd)) = self.bridge_session_context() else {
            return false;
        };
        if !self
            .bridge_capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.labels)
            || self.bridge_pending.is_some()
        {
            return false;
        }
        self.start_bridge_operation(
            BridgeOperationKind::SetLabel,
            BridgeCommand::SetLabel {
                session_path,
                cwd,
                target_id: target_id.to_string(),
                label,
            },
            cx,
        )
    }

    pub fn export_jsonl(&mut self, output_path: Option<String>, cx: &mut Context<Self>) -> bool {
        let Some((session_path, cwd)) = self.bridge_session_context() else {
            return false;
        };
        if !self
            .bridge_capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.jsonl_export)
            || self.bridge_pending.is_some()
        {
            return false;
        }
        self.start_bridge_operation(
            BridgeOperationKind::ExportJsonl,
            BridgeCommand::ExportJsonl {
                session_path,
                cwd,
                output_path,
            },
            cx,
        )
    }

    pub fn import_jsonl(&mut self, input_path: String, cx: &mut Context<Self>) -> bool {
        let Some(root) = self.catalog_root.as_ref() else {
            return false;
        };
        if !self
            .bridge_capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.jsonl_import)
            || self.bridge_pending.is_some()
        {
            return false;
        }
        self.start_bridge_operation(
            BridgeOperationKind::ImportJsonl,
            BridgeCommand::ImportJsonl {
                input_path,
                cwd: self.core.workspace.clone(),
                session_dir: root.path.to_string_lossy().into_owned(),
            },
            cx,
        )
    }

    pub fn cancel_bridge_operation(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((id, _)) = self.bridge_pending else {
            return false;
        };
        let accepted = self.bridge_worker.cancel(id);
        if accepted {
            self.bridge_feedback = Some("Cancelling session operation…".to_owned());
            cx.notify();
        }
        accepted
    }

    pub fn restart_bridge(&mut self, cx: &mut Context<Self>) -> bool {
        self.bridge_capabilities = None;
        self.bridge_unavailable = None;
        self.bridge_feedback = Some("Restarting session bridge…".to_owned());
        self.resource_snapshot_pending = None;
        self.resource_center.phase = ResourcePhase::Loading;
        self.resource_center.feedback = Some("Restarting resource bridge…".to_owned());
        self.orchestration_snapshot_pending = None;
        self.orchestration_actions_pending.clear();
        self.orchestration.phase = OrchestrationPhase::Loading;
        self.orchestration.feedback =
            Some("Reconnecting to Pi's orchestration extensions…".to_owned());
        let accepted = self.bridge_worker.restart();
        cx.notify();
        accepted
    }

    pub fn switch_session(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        if self.core.status() != ControllerStatus::Active
            || self.core.runtime.replacement_awaiting_state
            || self.core.runtime.pending_operation.is_some()
        {
            return false;
        }
        let path = crate::services::session_catalog::without_windows_verbatim_prefix(&path);
        let already_active = self
            .core
            .runtime
            .session
            .data
            .as_ref()
            .and_then(|session| session.file.as_deref())
            .map(Path::new)
            .map(crate::services::session_catalog::without_windows_verbatim_prefix)
            .is_some_and(|current| session_paths_equal(&current, &path));
        if already_active {
            return false;
        }

        self.preferred_session_file = Some(path.clone());
        self.start_connection(Some(path), cx)
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
        let resume_session = self.preferred_session_file.clone().or_else(|| {
            self.core
                .runtime
                .session
                .data
                .as_ref()
                .and_then(|session| session.file.as_deref())
                .map(PathBuf::from)
        });
        self.start_connection(resume_session, cx);
    }

    fn start_connection(
        &mut self,
        resume_session: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.service.set_resume_session(resume_session);
        let (attempt, generation) = self.core.begin_connect();
        let accepted = self.worker.connect(attempt, generation);
        if !accepted {
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
        accepted
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
        let previous_session = self.orchestration.expected_session_id.clone();
        let effects = self.core.apply_worker_result(result);
        self.send_effects(effects);
        let active_session = self
            .core
            .runtime
            .session
            .data
            .as_ref()
            .map(|session| session.id.to_string());
        self.orchestration.begin_session(active_session);
        if previous_session != self.orchestration.expected_session_id {
            self.orchestration_actions_pending.clear();
            self.request_orchestration_snapshot();
        }
        if before != self.catalog_refresh_identity() {
            self.preferred_session_file = self
                .core
                .runtime
                .session
                .data
                .as_ref()
                .and_then(|session| session.file.as_deref())
                .map(PathBuf::from);
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

    fn receive_bridge(&mut self, result: BridgeWorkerResult) {
        match result {
            BridgeWorkerResult::Capabilities(Ok(hello)) => {
                self.bridge_capabilities = Some(hello.capabilities);
                self.bridge_unavailable = None;
                if self
                    .bridge_capabilities
                    .as_ref()
                    .is_some_and(|capabilities| capabilities.model_runtime)
                {
                    let operation = self.model_runtime.take_operation();
                    if self
                        .bridge_worker
                        .execute(operation, BridgeCommand::GetModelRuntime)
                    {
                        self.model_snapshot_pending = Some(operation);
                    }
                }
                if self
                    .bridge_capabilities
                    .as_ref()
                    .is_some_and(|capabilities| capabilities.resource_inventory)
                {
                    let operation = self.resource_center.take_operation();
                    if self
                        .bridge_worker
                        .execute(operation, BridgeCommand::GetResourceInventory)
                    {
                        self.resource_snapshot_pending = Some(operation);
                    }
                }
                self.request_orchestration_snapshot();
                self.bridge_feedback =
                    Some(format!("Session bridge ready · SDK {}", hello.sdk_version));
            }
            BridgeWorkerResult::Capabilities(Err(error)) => {
                self.bridge_capabilities = None;
                self.bridge_unavailable = Some(error.summary);
                self.orchestration.disconnect();
            }
            BridgeWorkerResult::Event(BridgeEvent::Auth(event)) => {
                self.model_runtime.apply_auth_event(event);
            }
            BridgeWorkerResult::Event(BridgeEvent::Resource(event)) => match event {
                ResourceEvent::ResourceProgress { message, .. } => {
                    self.resource_center.feedback = Some(message);
                }
                ResourceEvent::ResourcesChanged { .. } => {}
            },
            BridgeWorkerResult::Event(BridgeEvent::Orchestration(event)) => match event {
                OrchestrationEvent::OrchestrationSnapshot { snapshot } => {
                    self.orchestration.apply_snapshot(*snapshot);
                }
                OrchestrationEvent::OrchestrationDisconnected => {
                    self.orchestration.disconnect();
                }
            },
            BridgeWorkerResult::Completed {
                id,
                command,
                result,
            } => {
                if self.receive_model_bridge_result(id, &command, &result) {
                    return;
                }
                if self.receive_resource_bridge_result(id, &command, &result) {
                    return;
                }
                if self.receive_orchestration_bridge_result(id, &command, &result) {
                    return;
                }
                let Some((pending_id, kind)) = self.bridge_pending else {
                    return;
                };
                if pending_id != id {
                    return;
                }
                self.bridge_pending = None;
                match result {
                    Ok(value)
                        if value.get("cancelled").and_then(serde_json::Value::as_bool)
                            == Some(true) =>
                    {
                        self.bridge_feedback =
                            Some("Session operation cancelled. History is unchanged.".to_owned());
                    }
                    Ok(value) => match kind {
                        BridgeOperationKind::Navigate => {
                            let editor_text = value
                                .get("editorText")
                                .and_then(serde_json::Value::as_str)
                                .map(ToOwned::to_owned);
                            let effects = self.core.reload_current_session(editor_text);
                            self.send_effects(effects);
                            self.bridge_feedback = Some(
                                "Navigated in the same file. Existing branches remain intact."
                                    .to_owned(),
                            );
                            self.start_catalog_refresh();
                        }
                        BridgeOperationKind::SetLabel => {
                            let effects = self.core.reload_current_session(None);
                            self.send_effects(effects);
                            self.bridge_feedback = Some("Entry label updated.".to_owned());
                            self.start_catalog_refresh();
                        }
                        BridgeOperationKind::ExportJsonl => {
                            let path = value
                                .get("path")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("the selected path");
                            self.bridge_feedback = Some(format!(
                                "Exported the active path to {path}. Other branches were not copied."
                            ));
                        }
                        BridgeOperationKind::ImportJsonl => {
                            let Some(path) = value
                                .get("path")
                                .and_then(serde_json::Value::as_str)
                                .map(ToOwned::to_owned)
                            else {
                                self.bridge_feedback =
                                    Some("Import completed without a session path.".to_owned());
                                return;
                            };
                            let effects = self.core.switch_session(path);
                            self.send_effects(effects);
                            self.bridge_feedback = Some(
                                "Imported into a new session file and switched to it.".to_owned(),
                            );
                            self.start_catalog_refresh();
                        }
                    },
                    Err(error) => {
                        if matches!(error.kind, BridgeErrorKind::Disconnected) {
                            self.bridge_capabilities = None;
                            self.bridge_unavailable = Some(
                                "The session bridge disconnected. Restart it to use SDK actions."
                                    .to_owned(),
                            );
                        }
                        self.bridge_feedback = Some(format!(
                            "{} failed. The current session remains unchanged. {}",
                            match command {
                                BridgeCommand::Hello => "Bridge negotiation",
                                BridgeCommand::NavigateTree { .. } => "Navigation",
                                BridgeCommand::SetLabel { .. } => "Label update",
                                BridgeCommand::ExportJsonl { .. } => "JSONL export",
                                BridgeCommand::ImportJsonl { .. } => "JSONL import",
                                BridgeCommand::GetModelRuntime
                                | BridgeCommand::RefreshModels
                                | BridgeCommand::LoginProvider { .. }
                                | BridgeCommand::AuthRespond { .. }
                                | BridgeCommand::LogoutProvider { .. }
                                | BridgeCommand::SetModelDefaults { .. }
                                | BridgeCommand::SetModelScope { .. } => "Model operation",
                                BridgeCommand::GetResourceInventory
                                | BridgeCommand::ReloadResources
                                | BridgeCommand::SetSkillCommandsEnabled { .. }
                                | BridgeCommand::SetResourceTheme { .. } => "Resource operation",
                                BridgeCommand::GetOrchestrationSnapshot { .. }
                                | BridgeCommand::OrchestrationAction { .. } => {
                                    "Orchestration operation"
                                }
                            },
                            error.summary
                        ));
                    }
                }
            }
        }
    }

    fn receive_model_bridge_result(
        &mut self,
        id: u64,
        command: &BridgeCommand,
        result: &Result<serde_json::Value, crate::services::sdk_bridge::BridgeError>,
    ) -> bool {
        match command {
            BridgeCommand::GetModelRuntime => {
                if self.model_snapshot_pending != Some(id) {
                    return true;
                }
                self.model_snapshot_pending = None;
                match result {
                    Ok(value) => {
                        match serde_json::from_value::<ModelCatalogSnapshot>(value.clone()) {
                            Ok(snapshot) => self.model_runtime.apply_snapshot(snapshot),
                            Err(_) => {
                                self.model_runtime.phase = CatalogPhase::Failed(
                                    "The model bridge returned an invalid catalog.".to_owned(),
                                )
                            }
                        }
                    }
                    Err(error) => {
                        self.model_runtime.phase = CatalogPhase::Failed(error.summary.clone())
                    }
                }
                true
            }
            BridgeCommand::RefreshModels => {
                let parsed = result
                    .as_ref()
                    .map_err(|error| error.summary.clone())
                    .and_then(|value| {
                        serde_json::from_value::<ModelCatalogSnapshot>(value.clone())
                            .map_err(|_| "The model bridge returned an invalid catalog.".to_owned())
                    });
                self.model_runtime.apply_refresh(id, parsed);
                true
            }
            BridgeCommand::LoginProvider { operation_id, .. } => {
                let parsed = result
                    .as_ref()
                    .map_err(|error| error.summary.clone())
                    .and_then(|value| {
                        serde_json::from_value::<ModelCatalogSnapshot>(value.clone())
                            .map_err(|_| "The provider returned an invalid catalog.".to_owned())
                    });
                match parsed {
                    Ok(snapshot) => {
                        self.model_runtime.apply_snapshot(snapshot);
                        self.model_runtime.finish_auth(*operation_id, Ok(()));
                    }
                    Err(summary) => {
                        self.model_runtime.finish_auth(*operation_id, Err(summary));
                    }
                }
                true
            }
            BridgeCommand::AuthRespond { .. } => true,
            BridgeCommand::LogoutProvider { provider } => {
                match result {
                    Ok(value) => {
                        if let Some(snapshot) = value.get("snapshot").cloned().and_then(|value| {
                            serde_json::from_value::<ModelCatalogSnapshot>(value).ok()
                        }) {
                            self.model_runtime.apply_snapshot(snapshot);
                        }
                        self.model_runtime.feedback = Some(
                            if value
                                .get("environmentFallback")
                                .and_then(serde_json::Value::as_bool)
                                == Some(true)
                            {
                                format!(
                                    "Removed Pi's stored {provider} credential. Environment authentication is still active."
                                )
                            } else {
                                format!("Logged out of {provider}.")
                            },
                        );
                    }
                    Err(error) => {
                        self.model_runtime.feedback = Some(format!(
                            "Could not log out of {provider}. {}",
                            error.summary
                        ));
                    }
                }
                true
            }
            BridgeCommand::SetModelDefaults { .. } | BridgeCommand::SetModelScope { .. } => {
                match result {
                    Ok(value) => {
                        match serde_json::from_value::<ModelCatalogSnapshot>(value.clone()) {
                            Ok(snapshot) => {
                                self.model_runtime.apply_snapshot(snapshot);
                                self.model_runtime.feedback = Some(
                                "Saved Pi defaults for future sessions. The active session is unchanged."
                                    .to_owned(),
                            );
                            }
                            Err(_) => {
                                self.model_runtime.feedback = Some(
                                    "Pi saved settings but returned an invalid catalog.".to_owned(),
                                );
                            }
                        }
                    }
                    Err(error) => {
                        self.model_runtime.feedback =
                            Some(format!("Pi settings were not changed. {}", error.summary));
                    }
                }
                true
            }
            BridgeCommand::Hello
            | BridgeCommand::NavigateTree { .. }
            | BridgeCommand::SetLabel { .. }
            | BridgeCommand::ExportJsonl { .. }
            | BridgeCommand::ImportJsonl { .. }
            | BridgeCommand::GetResourceInventory
            | BridgeCommand::ReloadResources
            | BridgeCommand::SetSkillCommandsEnabled { .. }
            | BridgeCommand::SetResourceTheme { .. }
            | BridgeCommand::GetOrchestrationSnapshot { .. }
            | BridgeCommand::OrchestrationAction { .. } => false,
        }
    }

    fn receive_resource_bridge_result(
        &mut self,
        id: u64,
        command: &BridgeCommand,
        result: &Result<serde_json::Value, crate::services::sdk_bridge::BridgeError>,
    ) -> bool {
        match command {
            BridgeCommand::GetResourceInventory
            | BridgeCommand::ReloadResources
            | BridgeCommand::SetSkillCommandsEnabled { .. }
            | BridgeCommand::SetResourceTheme { .. } => {
                if self.resource_snapshot_pending != Some(id)
                    && matches!(
                        command,
                        BridgeCommand::GetResourceInventory | BridgeCommand::ReloadResources
                    )
                {
                    return true;
                }
                self.resource_snapshot_pending = None;
                match result {
                    Ok(value)
                        if value.get("cancelled").and_then(serde_json::Value::as_bool)
                            == Some(true) =>
                    {
                        self.resource_center.feedback = Some(
                            "Resource reload was cancelled; the prior inventory was kept."
                                .to_owned(),
                        );
                        self.resource_center.phase = if self.resource_center.snapshot.is_some() {
                            ResourcePhase::Ready
                        } else {
                            ResourcePhase::Failed("Resource reload was cancelled.".to_owned())
                        };
                    }
                    Ok(value) => match decode_resource_snapshot(value.clone()) {
                        Ok(snapshot) => self.resource_center.apply_snapshot(snapshot),
                        Err(error) => self.resource_center.apply_failure(error.summary),
                    },
                    Err(error) => self.resource_center.apply_failure(error.summary.clone()),
                }
                true
            }
            BridgeCommand::Hello
            | BridgeCommand::NavigateTree { .. }
            | BridgeCommand::SetLabel { .. }
            | BridgeCommand::ExportJsonl { .. }
            | BridgeCommand::ImportJsonl { .. }
            | BridgeCommand::GetModelRuntime
            | BridgeCommand::RefreshModels
            | BridgeCommand::LoginProvider { .. }
            | BridgeCommand::AuthRespond { .. }
            | BridgeCommand::LogoutProvider { .. }
            | BridgeCommand::SetModelDefaults { .. }
            | BridgeCommand::SetModelScope { .. }
            | BridgeCommand::GetOrchestrationSnapshot { .. }
            | BridgeCommand::OrchestrationAction { .. } => false,
        }
    }

    fn request_orchestration_snapshot(&mut self) {
        if self.orchestration_snapshot_pending.is_some()
            || !self
                .bridge_capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.orchestration)
        {
            return;
        }
        let Some(session_id) = self.orchestration.expected_session_id.clone() else {
            return;
        };
        let operation = self.take_bridge_operation();
        if self.bridge_worker.execute(
            operation,
            BridgeCommand::GetOrchestrationSnapshot {
                session_id: Some(session_id),
            },
        ) {
            self.orchestration_snapshot_pending = Some(operation);
            if self.orchestration.snapshot.is_none() {
                self.orchestration.phase = OrchestrationPhase::Loading;
            }
        }
    }

    fn receive_orchestration_bridge_result(
        &mut self,
        id: u64,
        command: &BridgeCommand,
        result: &Result<serde_json::Value, crate::services::sdk_bridge::BridgeError>,
    ) -> bool {
        match command {
            BridgeCommand::GetOrchestrationSnapshot { .. } => {
                if self.orchestration_snapshot_pending != Some(id) {
                    return true;
                }
                self.orchestration_snapshot_pending = None;
                match result {
                    Ok(value) => {
                        match serde_json::from_value::<OrchestrationSnapshot>(value.clone()) {
                            Ok(snapshot) => {
                                self.orchestration.apply_snapshot(snapshot);
                            }
                            Err(_) => self
                                .orchestration
                                .fail("Pi returned an invalid orchestration snapshot."),
                        }
                    }
                    Err(error) if matches!(error.kind, BridgeErrorKind::Disconnected) => {
                        self.orchestration.disconnect();
                    }
                    Err(error) => self.orchestration.fail(error.summary.clone()),
                }
                true
            }
            BridgeCommand::OrchestrationAction { .. } => {
                let Some(_action) = self.orchestration_actions_pending.remove(&id) else {
                    return true;
                };
                match result {
                    Ok(value) => {
                        if let Some(command) = value
                            .get("invokeCommand")
                            .and_then(serde_json::Value::as_str)
                        {
                            match self.core.invoke_dynamic_command(
                                command.to_owned(),
                                CommandSource::Extension,
                                SubmissionPreference::Default,
                            ) {
                                Ok((_submission, effects)) => {
                                    self.send_effects(effects);
                                    self.orchestration.feedback =
                                        Some("Pi accepted the goal action.".to_owned());
                                }
                                Err(error) => self.orchestration.fail(format!(
                                    "Pi could not run the goal action: {}",
                                    error.message()
                                )),
                            }
                        } else {
                            self.orchestration.feedback =
                                Some("Pi accepted the orchestration action.".to_owned());
                            self.request_orchestration_snapshot();
                        }
                    }
                    Err(error) if matches!(error.kind, BridgeErrorKind::Disconnected) => {
                        self.orchestration.disconnect();
                    }
                    Err(error) => self.orchestration.fail(error.summary.clone()),
                }
                true
            }
            BridgeCommand::Hello
            | BridgeCommand::NavigateTree { .. }
            | BridgeCommand::SetLabel { .. }
            | BridgeCommand::ExportJsonl { .. }
            | BridgeCommand::ImportJsonl { .. }
            | BridgeCommand::GetModelRuntime
            | BridgeCommand::RefreshModels
            | BridgeCommand::LoginProvider { .. }
            | BridgeCommand::AuthRespond { .. }
            | BridgeCommand::LogoutProvider { .. }
            | BridgeCommand::SetModelDefaults { .. }
            | BridgeCommand::SetModelScope { .. }
            | BridgeCommand::GetResourceInventory
            | BridgeCommand::ReloadResources
            | BridgeCommand::SetSkillCommandsEnabled { .. }
            | BridgeCommand::SetResourceTheme { .. } => false,
        }
    }

    fn bridge_session_context(&self) -> Option<(String, String)> {
        if !matches!(
            self.core.runtime.lifecycle,
            RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
        ) || self.core.runtime.replacement_awaiting_state
        {
            return None;
        }
        let path = self.core.runtime.session.data.as_ref()?.file.clone()?;
        Some((path, self.core.workspace.clone()))
    }

    fn start_bridge_operation(
        &mut self,
        kind: BridgeOperationKind,
        command: BridgeCommand,
        cx: &mut Context<Self>,
    ) -> bool {
        let id = self.take_bridge_operation();
        if !self.bridge_worker.execute(id, command) {
            return false;
        }
        self.bridge_pending = Some((id, kind));
        self.bridge_feedback = Some(
            match kind {
                BridgeOperationKind::Navigate => "Navigating within the current session file…",
                BridgeOperationKind::SetLabel => "Updating entry label…",
                BridgeOperationKind::ExportJsonl => "Exporting the active path…",
                BridgeOperationKind::ImportJsonl => "Importing into a new session file…",
            }
            .to_owned(),
        );
        cx.notify();
        true
    }

    fn take_bridge_operation(&mut self) -> u64 {
        let id = self.next_bridge_operation;
        self.next_bridge_operation = self.next_bridge_operation.saturating_add(1);
        id
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
    fn identical_session_paths_are_not_reopened() {
        assert!(session_paths_equal(
            Path::new("sessions/thread.jsonl"),
            Path::new("sessions/thread.jsonl")
        ));
    }

    #[test]
    fn runtime_notifications_are_delivered_once_to_the_native_ui() {
        let mut core = ControllerCore::new("workspace");
        core.runtime.notifications.push_back(RuntimeNotification {
            message: "Binance tools ON".to_owned(),
            kind: crate::state::runtime::NotificationKind::Info,
        });

        let delivered = core.take_runtime_notifications();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].message, "Binance tools ON");
        assert!(core.take_runtime_notifications().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn windows_session_path_comparison_ignores_case_and_separator_style() {
        assert!(session_paths_equal(
            Path::new(r"C:\Sessions\Thread.jsonl"),
            Path::new("c:/sessions/thread.jsonl")
        ));
    }

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
                supported_thinking: vec![RuntimeThinkingLevel::Off],
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
    fn same_file_navigation_reload_advances_epoch_and_preserves_editor_text() {
        use crate::state::runtime::{EffectKind, RuntimeRequest, SessionMutation};

        let mut core = ready_core();
        core.runtime.session.data.as_mut().unwrap().file = Some("session.jsonl".to_owned());
        let prior_epoch = core.runtime.epoch;
        let effects = core.reload_current_session(Some("restored prompt".to_owned()));

        assert_eq!(core.runtime.epoch, prior_epoch.next());
        assert!(core.runtime.replacement_awaiting_state);
        assert_eq!(
            core.runtime.requested_editor_text.as_deref(),
            Some("restored prompt")
        );
        assert!(matches!(
            effects[0].effect,
            EffectKind::Request(RuntimeRequest::SessionMutation(SessionMutation::Switch {
                ref session_path
            })) if session_path == "session.jsonl"
        ));
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
