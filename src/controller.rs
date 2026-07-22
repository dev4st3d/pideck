//! GPUI-owned runtime controller and its pure generation gate.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use gpui::{Context, Task};

use crate::services::rpc::{ConnectionGeneration, RequestId, SessionEpoch};
use crate::services::runtime_worker::{
    AttemptGeneration, RuntimeService, RuntimeWorkerHandle, WorkerResult,
};
use crate::state::reducer::reduce;
use crate::state::runtime::{
    BashExecution, BashStatus, CompactionState, FacetStatus, PromptDelivery, RetryState,
    RuntimeInput, RuntimeIntent, RuntimeLifecycle, RuntimeMessage, RuntimeState, SafeError,
    StampedInput, SubmissionKind, ToolExecution,
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
    pub retry: RetryState,
    pub compaction: CompactionState,
    pub error: Option<SafeError>,
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
            epoch: self.runtime.epoch,
            revision: self.runtime.revision,
            lifecycle: self.runtime.lifecycle,
            status: self.runtime.messages.status.clone(),
            messages: if self.runtime.replacement_awaiting_state {
                Vec::new()
            } else {
                self.runtime
                    .messages
                    .data
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .filter(|message| message.visible)
                    .cloned()
                    .collect()
            },
            accepted_user_inputs: self
                .runtime
                .optimistic_user_inputs
                .iter()
                .filter(|_| !self.runtime.replacement_awaiting_state)
                .filter(|input| input.accepted && !input.authoritative_seen)
                .map(|input| AcceptedUserInput {
                    request: input.request.clone(),
                    text: input.text.clone(),
                    kind: input.kind,
                })
                .collect(),
            tools: if self.runtime.replacement_awaiting_state {
                HashMap::new()
            } else {
                self.runtime.tools.clone()
            },
            bash_executions: if self.runtime.replacement_awaiting_state {
                Vec::new()
            } else {
                self.runtime.bash_executions.clone()
            },
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
        let epoch = self.runtime.epoch;
        reduce(
            &mut self.runtime,
            StampedInput {
                generation: self.generation,
                epoch,
                observed_at: Instant::now(),
                input: RuntimeInput::Intent(RuntimeIntent::AbortBash),
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
    worker: RuntimeWorkerHandle,
    _event_task: Task<()>,
}

impl RuntimeController {
    pub fn new(
        workspace: impl Into<String>,
        service: Arc<dyn RuntimeService>,
        cx: &mut Context<Self>,
    ) -> Self {
        let worker = RuntimeWorkerHandle::spawn(service);
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
        Self {
            core: ControllerCore::new(workspace),
            worker,
            _event_task: event_task,
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

    pub fn connect(&mut self, cx: &mut Context<Self>) {
        if !matches!(
            self.core.projection().action,
            Some(crate::state::RecoveryAction::Connect | crate::state::RecoveryAction::Retry)
        ) {
            return;
        }
        let (attempt, generation) = self.core.begin_connect();
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
        let effects = self.core.apply_worker_result(result);
        self.send_effects(effects);
    }

    fn send_effects(&self, effects: Vec<crate::state::runtime::RuntimeEffect>) {
        let attempt = self.core.attempt();
        for effect in effects {
            let _ = self.worker.execute(attempt, effect);
        }
    }
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
