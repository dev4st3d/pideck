//! GPUI-owned runtime controller and its pure generation gate.

use std::sync::Arc;

use gpui::{Context, Task};

use crate::services::rpc::{ConnectionGeneration, RequestId};
use crate::services::runtime_worker::{
    AttemptGeneration, RuntimeService, RuntimeWorkerHandle, WorkerResult,
};
use crate::state::reducer::reduce;
use crate::state::runtime::{
    PromptDelivery, RuntimeInput, RuntimeIntent, RuntimeLifecycle, RuntimeState, StampedInput,
    SubmissionKind,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerProjection {
    pub runtime: ComposerRuntime,
    pub delivery: PromptDelivery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedSubmission {
    pub request: RequestId,
    pub kind: SubmissionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionRejection {
    Empty,
    Pending,
    Unavailable,
    NotRunning,
}

impl SubmissionRejection {
    pub fn message(self) -> &'static str {
        match self {
            Self::Empty => "Write a prompt first.",
            Self::Pending => "The previous acceptance is still pending.",
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
        let runtime = if self.status != ControllerStatus::Active || !has_model {
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
        let request = RequestId::new(format!(
            "composer-{}-{}-{}",
            self.generation.value(),
            self.runtime.epoch.value(),
            self.next_submission
        ));
        self.next_submission = self.next_submission.saturating_add(1);
        let epoch = self.runtime.epoch;
        let effects = reduce(
            &mut self.runtime,
            StampedInput {
                generation: self.generation,
                epoch,
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
        Ok((AcceptedSubmission { request, kind }, effects))
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
                input: RuntimeInput::Intent(RuntimeIntent::Abort),
            },
        )
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
        assert_eq!(prompt.kind, SubmissionKind::Prompt);
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
        assert_eq!(steer.kind, SubmissionKind::Steer);
        let EffectKind::Request(steer_request) = effects[0].effect.clone() else {
            panic!("expected request");
        };
        complete_submission(&mut core, steer_request);

        let (follow_up, effects) = core
            .submit("Then verify".to_owned(), SubmissionPreference::FollowUp)
            .expect("follow-up");
        assert_eq!(follow_up.kind, SubmissionKind::FollowUp);
        assert!(matches!(
            effects[0].effect,
            EffectKind::Request(RuntimeRequest::Submit {
                kind: SubmissionKind::FollowUp,
                ..
            })
        ));
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
