//! GPUI-owned runtime controller and its pure generation gate.

use std::sync::Arc;

use gpui::{Context, Task};

use crate::services::rpc::ConnectionGeneration;
use crate::services::runtime_worker::{
    AttemptGeneration, RuntimeService, RuntimeWorkerHandle, WorkerResult,
};
use crate::state::reducer::reduce;
use crate::state::runtime::{RuntimeInput, RuntimeState, StampedInput};
use crate::state::{ControllerStatus, ShellProjection};

pub struct ControllerCore {
    status: ControllerStatus,
    attempt: AttemptGeneration,
    generation: ConnectionGeneration,
    runtime: RuntimeState,
    workspace: String,
    connection_error: Option<String>,
    stale_attempts_ignored: u64,
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
        self.runtime.session.loading();
        self.runtime.messages.loading();
        self.runtime.entries.loading();
        self.runtime.stats.loading();
        self.runtime.commands.loading();
        self.runtime.models.loading();
        self.runtime.tree.loading();
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
        let attempt = self.core.attempt();
        let effects = self.core.apply_worker_result(result);
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
}
