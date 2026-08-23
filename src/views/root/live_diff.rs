//! Cancellable, throttled workspace-diff refresh coordination.
//!
//! The coordinator is intentionally pure Rust. View code owns task spawning;
//! this module only decides when a scan is needed and rejects stale results.

use std::time::Duration;

use super::*;

const LIVE_POLL_DELAY: Duration = Duration::from_millis(850);
const SETTLED_DEBOUNCE: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffScope {
    epoch: u64,
    workspace: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffScanRequest {
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) workspace: String,
    pub(super) revision: u64,
    pub(super) delay: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffObservation {
    pub(super) reset_snapshot: bool,
    pub(super) request: Option<DiffScanRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffCompletion {
    pub(super) accepted: bool,
    pub(super) request: Option<DiffScanRequest>,
}

#[derive(Debug, Default)]
pub(super) struct LiveDiffCoordinator {
    scope: Option<DiffScope>,
    requested_revision: u64,
    scanned_revision: Option<u64>,
    generation: u64,
    in_flight: bool,
    live: bool,
}

impl LiveDiffCoordinator {
    pub(super) fn observe(
        &mut self,
        epoch: u64,
        revision: u64,
        workspace: &str,
        live: bool,
    ) -> DiffObservation {
        let scope = DiffScope {
            epoch,
            workspace: workspace.to_owned(),
        };
        let reset_snapshot = self.scope.as_ref() != Some(&scope);
        if reset_snapshot {
            self.generation = self.generation.wrapping_add(1);
            self.scope = Some(scope);
            self.scanned_revision = None;
            self.in_flight = false;
        }

        self.requested_revision = revision;
        self.live = live;
        let needs_scan = self.scanned_revision != Some(revision) || live;
        let request = if !self.in_flight && needs_scan {
            let delay = if reset_snapshot {
                Duration::ZERO
            } else if live {
                LIVE_POLL_DELAY
            } else {
                SETTLED_DEBOUNCE
            };
            self.begin_request(delay)
        } else {
            None
        };

        DiffObservation {
            reset_snapshot,
            request,
        }
    }

    pub(super) fn complete(
        &mut self,
        request: &DiffScanRequest,
        continue_live_polling: bool,
    ) -> DiffCompletion {
        let accepted = self.scope.as_ref().is_some_and(|scope| {
            request.generation == self.generation
                && request.epoch == scope.epoch
                && request.workspace == scope.workspace
        });
        if !accepted {
            return DiffCompletion {
                accepted: false,
                request: None,
            };
        }

        self.in_flight = false;
        self.scanned_revision = Some(request.revision);
        let poll_live = self.live && continue_live_polling;
        let needs_follow_up = poll_live || self.scanned_revision != Some(self.requested_revision);
        let follow_up = if needs_follow_up {
            self.begin_request(if poll_live {
                LIVE_POLL_DELAY
            } else {
                SETTLED_DEBOUNCE
            })
        } else {
            None
        };

        DiffCompletion {
            accepted: true,
            request: follow_up,
        }
    }

    pub(super) fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.scope = None;
        self.requested_revision = 0;
        self.scanned_revision = None;
        self.in_flight = false;
        self.live = false;
    }

    fn begin_request(&mut self, delay: Duration) -> Option<DiffScanRequest> {
        let scope = self.scope.as_ref()?;
        self.in_flight = true;
        Some(DiffScanRequest {
            generation: self.generation,
            epoch: scope.epoch,
            workspace: scope.workspace.clone(),
            revision: self.requested_revision,
            delay,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_immediately_for_a_new_workspace_scope() {
        let mut coordinator = LiveDiffCoordinator::default();
        let observation = coordinator.observe(1, 7, "/workspace", false);

        assert!(observation.reset_snapshot);
        assert_eq!(observation.request.unwrap().delay, Duration::ZERO);
    }

    #[test]
    fn coalesces_revisions_while_a_scan_is_in_flight() {
        let mut coordinator = LiveDiffCoordinator::default();
        let first = coordinator
            .observe(1, 7, "/workspace", true)
            .request
            .unwrap();
        assert!(coordinator.observe(1, 8, "/workspace", true).request.is_none());
        assert!(coordinator.observe(1, 12, "/workspace", true).request.is_none());

        let completion = coordinator.complete(&first, true);
        let next = completion.request.unwrap();
        assert!(completion.accepted);
        assert_eq!(next.revision, 12);
        assert_eq!(next.delay, LIVE_POLL_DELAY);
    }

    #[test]
    fn rejects_a_result_from_the_previous_workspace() {
        let mut coordinator = LiveDiffCoordinator::default();
        let stale = coordinator
            .observe(1, 2, "/one", false)
            .request
            .unwrap();
        let next = coordinator.observe(2, 1, "/two", false);

        assert!(next.reset_snapshot);
        assert!(!coordinator.complete(&stale, true).accepted);
    }

    #[test]
    fn an_error_stops_timer_polling_until_a_new_observation() {
        let mut coordinator = LiveDiffCoordinator::default();
        let first = coordinator
            .observe(1, 3, "/workspace", true)
            .request
            .unwrap();

        let completion = coordinator.complete(&first, false);
        assert!(completion.accepted);
        assert!(completion.request.is_none());

        let retry = coordinator.observe(1, 3, "/workspace", true).request;
        assert!(retry.is_some());
    }

    #[test]
    fn stops_polling_after_the_final_settled_revision() {
        let mut coordinator = LiveDiffCoordinator::default();
        let first = coordinator
            .observe(1, 3, "/workspace", true)
            .request
            .unwrap();
        coordinator.observe(1, 4, "/workspace", false);
        let completion = coordinator.complete(&first, true);
        let final_scan = completion.request.unwrap();
        assert_eq!(final_scan.revision, 4);
        assert_eq!(final_scan.delay, SETTLED_DEBOUNCE);

        let final_completion = coordinator.complete(&final_scan, true);
        assert!(final_completion.accepted);
        assert!(final_completion.request.is_none());
    }
}

fn workspace_diff_error_message(error: crate::services::git_diff::GitDiffError) -> String {
    use crate::services::git_diff::GitDiffError;

    match error {
        GitDiffError::GitUnavailable => {
            "Git is unavailable; live changes cannot be inspected.".to_owned()
        }
        GitDiffError::NotRepository => "This workspace is not a Git repository.".to_owned(),
        GitDiffError::InspectionFailed => {
            "Git could not inspect the current workspace changes.".to_owned()
        }
    }
}

impl RootView {
    pub(super) fn sync_workspace_diff(
        &mut self,
        conversation: &ConversationProjection,
        workspace: &str,
        cx: &mut Context<Self>,
    ) {
        let live = matches!(
            conversation.lifecycle,
            RuntimeLifecycle::Running | RuntimeLifecycle::Cancelling
        ) || conversation.pending_operation.is_some();
        let observation = self.workspace_diff_refresh.observe(
            conversation.epoch.value(),
            conversation.revision,
            workspace,
            live,
        );

        if observation.reset_snapshot {
            self.workspace_diff_task.take();
            self.workspace_diff = None;
            self.workspace_diff_error = None;
            self.workspace_diff_files_expanded = false;
            self.workspace_diff_open = false;
            self.workspace_diff_selected = 0;
            self.workspace_diff_collapsed_folders.clear();
            self.workspace_diff_files_scroll = ScrollHandle::new();
            self.workspace_diff_scroll = ScrollHandle::new();
            self.conversation_list
                .refresh_trailing(&self.conversation_list_state);
        }

        if let Some(request) = observation.request {
            self.start_workspace_diff_scan(request, cx);
        } else if observation.reset_snapshot {
            cx.notify();
        }
    }

    pub(super) fn start_workspace_diff_scan(
        &mut self,
        request: DiffScanRequest,
        cx: &mut Context<Self>,
    ) {
        self.workspace_diff_loading = true;
        self.workspace_diff_error = None;
        let task_request = request.clone();
        self.workspace_diff_task = Some(cx.spawn(async move |view, cx| {
            if !task_request.delay.is_zero() {
                cx.background_executor().timer(task_request.delay).await;
            }
            let workspace = PathBuf::from(&task_request.workspace);
            let scan = cx
                .background_executor()
                .spawn(async move { crate::services::git_diff::load_workspace_diff(&workspace) });
            let result = scan.await;
            let _ = view.update(cx, |view, cx| {
                let completion = view
                    .workspace_diff_refresh
                    .complete(&task_request, result.is_ok());
                if !completion.accepted {
                    return;
                }

                view.workspace_diff_task = None;
                view.workspace_diff_loading = completion.request.is_some();
                match result {
                    Ok(snapshot) => {
                        view.workspace_diff_error = None;
                        view.workspace_diff = (!snapshot.is_empty()).then(|| Arc::new(snapshot));
                        let last_file = view
                            .workspace_diff
                            .as_ref()
                            .map_or(0, |diff| diff.files.len().saturating_sub(1));
                        view.workspace_diff_selected = view.workspace_diff_selected.min(last_file);
                        if view.workspace_diff.is_none() {
                            view.workspace_diff_open = false;
                            view.workspace_diff_files_expanded = false;
                            view.workspace_diff_selected = 0;
                            view.workspace_diff_collapsed_folders.clear();
                        }
                    }
                    Err(error) => {
                        view.workspace_diff_error = Some(workspace_diff_error_message(error));
                    }
                }
                view.conversation_list
                    .refresh_trailing(&view.conversation_list_state);
                if let Some(next) = completion.request {
                    view.start_workspace_diff_scan(next, cx);
                } else {
                    view.workspace_diff_loading = false;
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

}
