//! Project ownership and terminal navigation; shells and persistence run off the UI thread.

use gpui_component::{
    button::{Button, ButtonVariants},
    input::{InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::path::PathBuf;

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, FontWeight, IntoElement, KeyBinding, KeyDownEvent,
    PathPromptOptions, Pixels, PromptLevel, Render, ScrollHandle, SharedString, Subscription,
    Window, WindowControlArea, actions, anchored, deferred, div, prelude::*, px, svg,
};

use super::checklist::ChecklistView;
use super::project_panels::{FilesPanel, GitPanel, ProjectPanelEvent};
use super::terminal::{TerminalPanelEvent, TerminalView};
use crate::services::{
    app_update::{self, CheckOutcome, PrepareOutcome, ScheduleOutcome},
    terminal_workspace::{MAX_TERMINAL_TABS, TerminalWorkspace},
};
use crate::theme;
use crate::theme::terminal_manager as chrome;

const APPEARANCE_SAVE_ERROR: &str =
    "The theme changed, but its preference could not be saved. Choose the theme again to retry.";
const APPEARANCE_LOAD_ERROR: &str =
    "The saved theme could not be read. Choose a theme to save a new preference.";

actions!(
    terminal_manager,
    [
        AddProject,
        NewTerminal,
        CloseTerminal,
        NextTerminal,
        PreviousTerminal,
        NextProject,
        PreviousProject,
        ToggleProjects,
        ToggleChecklist,
        FocusProjects,
    ]
);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SidebarTab {
    Projects,
    #[default]
    Files,
    Git,
}

#[derive(Clone)]
enum PendingClose {
    Window,
    Project(PathBuf),
    UpdateRestart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateState {
    Checking,
    Current,
    Available(String),
    Downloading,
    Prepared(String),
    Scheduling,
    Unavailable,
    Error(String),
}

struct ProjectTerminals {
    terminal: Entity<TerminalView>,
    _subscription: Subscription,
    files: Entity<FilesPanel>,
    checklist: Entity<ChecklistView>,
    git: Entity<GitPanel>,
    sidebar_tab: SidebarTab,
    project_kind: Option<&'static str>,
    _panel_subscriptions: Vec<Subscription>,
}

pub(crate) struct TerminalManager {
    workspace: Option<TerminalWorkspace>,
    projects: Vec<ProjectTerminals>,
    storage_path: PathBuf,
    focus_handle: FocusHandle,
    sidebar_focus: FocusHandle,
    sidebar_width: f32,
    resizing_sidebar: bool,
    project_filter: Option<Entity<InputState>>,
    project_query: String,
    _project_filter_subscription: Option<Subscription>,
    notice: Option<String>,
    restore_warning: Option<String>,
    save_failed: bool,
    picker_pending: bool,
    prompt_pending: bool,
    saving: bool,
    revision: u64,
    saved_revision: u64,
    close_after_save: bool,
    selector_open: bool,
    selector_focus: FocusHandle,
    selector_trigger_focus: FocusHandle,
    selector_bounds: Option<Bounds<Pixels>>,
    selector_scroll: ScrollHandle,
    selector_selected: usize,
    appearance_menu_open: bool,
    appearance_focus: FocusHandle,
    appearance_trigger_focus: FocusHandle,
    appearance_bounds: Option<Bounds<Pixels>>,
    appearance_selected: usize,
    appearance_revision: u64,
    appearance_saving: bool,
    discard_on_close: bool,
    pending_close: Option<PendingClose>,
    update_state: UpdateState,
    update_generation: u64,
    update_restart_pending: bool,
    update_scheduling: bool,
    _bounds_subscription: Subscription,
}

impl TerminalManager {
    pub(crate) fn bind_keys(cx: &mut App) {
        ChecklistView::bind_keys(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-shift-o", AddProject, Some("TerminalManager")),
            KeyBinding::new("ctrl-shift-t", NewTerminal, Some("TerminalManager")),
            KeyBinding::new("ctrl-`", NewTerminal, Some("TerminalManager")),
            KeyBinding::new("ctrl-shift-w", CloseTerminal, Some("TerminalManager")),
            KeyBinding::new("ctrl-tab", NextTerminal, Some("TerminalManager")),
            KeyBinding::new("ctrl-shift-tab", PreviousTerminal, Some("TerminalManager")),
            KeyBinding::new("ctrl-alt-down", NextProject, Some("TerminalManager")),
            KeyBinding::new("ctrl-alt-up", PreviousProject, Some("TerminalManager")),
            KeyBinding::new("ctrl-shift-b", ToggleProjects, Some("TerminalManager")),
            KeyBinding::new("ctrl-shift-l", ToggleChecklist, Some("TerminalManager")),
            KeyBinding::new("f6", FocusProjects, Some("TerminalManager")),
        ]);
    }

    pub(crate) fn new(
        initial: PathBuf,
        storage_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let load_path = storage_path.clone();
        let appearance_path = storage_path.with_file_name("appearance.json");
        let load = cx.background_executor().spawn(async move {
            (
                TerminalWorkspace::load(&load_path, &initial),
                crate::services::appearance::load(&appearance_path),
            )
        });
        cx.spawn_in(window, async move |view, cx| {
            let ((workspace, warning), appearance) = load.await;
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    if view.appearance_revision == 0 {
                        match appearance {
                            Ok(appearance) => {
                                theme::set_appearance(appearance);
                                super::file_editor::FileEditor::apply_appearance(cx);
                            }
                            Err(error) => {
                                eprintln!("Appearance preference could not be read: {error}");
                                view.notice = Some(APPEARANCE_LOAD_ERROR.into());
                            }
                        }
                    }
                    view.restore_warning = warning;
                    view.workspace = Some(workspace);
                    view.restore_terminals(window, cx);
                    view.start_save(window, cx);
                    cx.notify();
                });
            });
        })
        .detach();
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        let mut manager = Self {
            workspace: None,
            projects: Vec::new(),
            storage_path,
            focus_handle,
            sidebar_focus: cx.focus_handle(),
            sidebar_width: chrome::SIDEBAR_WIDTH,
            resizing_sidebar: false,
            project_filter: None,
            project_query: String::new(),
            _project_filter_subscription: None,
            notice: None,
            restore_warning: None,
            save_failed: false,
            picker_pending: false,
            prompt_pending: false,
            saving: false,
            revision: 1,
            saved_revision: 0,
            close_after_save: false,
            selector_open: false,
            selector_focus: cx.focus_handle(),
            selector_trigger_focus: cx.focus_handle(),
            selector_bounds: None,
            selector_scroll: ScrollHandle::new(),
            selector_selected: 0,
            appearance_menu_open: false,
            appearance_focus: cx.focus_handle(),
            appearance_trigger_focus: cx.focus_handle(),
            appearance_bounds: None,
            appearance_selected: 0,
            appearance_revision: 0,
            appearance_saving: false,
            discard_on_close: false,
            pending_close: None,
            update_state: UpdateState::Unavailable,
            update_generation: 0,
            update_restart_pending: false,
            update_scheduling: false,
            _bounds_subscription: cx
                .observe_window_bounds(window, |view, window, cx| view.resize(window, cx)),
        };
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Find project…"));
        manager._project_filter_subscription = Some(cx.subscribe(
            &filter,
            |view, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    view.project_query = input.read(cx).value().to_lowercase();
                    cx.notify();
                }
            },
        ));
        manager.project_filter = Some(filter);
        manager.check_for_updates(window, cx);
        manager
    }

    fn check_for_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.update_state,
            UpdateState::Checking | UpdateState::Downloading | UpdateState::Scheduling
        ) {
            return;
        }
        self.update_generation = self.update_generation.wrapping_add(1);
        let generation = self.update_generation;
        self.update_state = UpdateState::Checking;
        let check = cx
            .background_executor()
            .spawn(async { app_update::check_for_update() });
        cx.spawn_in(window, async move |view, cx| {
            let result = check.await;
            let _ = cx.update(|_, cx| {
                let _ = view.update(cx, |view, cx| {
                    if view.update_generation != generation {
                        return;
                    }
                    view.update_state = match result {
                        Ok(CheckOutcome::Current) => UpdateState::Current,
                        Ok(CheckOutcome::Available { version }) => UpdateState::Available(version),
                        Ok(CheckOutcome::Prepared { version }) => UpdateState::Prepared(version),
                        Ok(CheckOutcome::Unavailable) => UpdateState::Unavailable,
                        Err(error) => UpdateState::Error(error.message().into()),
                    };
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn prepare_and_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_scheduling || self.prompt_pending || self.picker_pending {
            return;
        }
        if matches!(self.update_state, UpdateState::Prepared(_)) {
            self.request_update_restart(window, cx);
            return;
        }
        if !matches!(
            self.update_state,
            UpdateState::Available(_) | UpdateState::Error(_)
        ) {
            return;
        }
        self.update_generation = self.update_generation.wrapping_add(1);
        let generation = self.update_generation;
        self.update_state = UpdateState::Downloading;
        let prepare = cx
            .background_executor()
            .spawn(async { app_update::prepare_update() });
        cx.spawn_in(window, async move |view, cx| {
            let result = prepare.await;
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    if view.update_generation != generation {
                        return;
                    }
                    match result {
                        Ok(PrepareOutcome::Prepared { version }) => {
                            view.update_state = UpdateState::Prepared(version);
                            view.request_update_restart(window, cx);
                        }
                        Ok(PrepareOutcome::Current) => view.update_state = UpdateState::Current,
                        Ok(PrepareOutcome::Unavailable) => {
                            view.update_state = UpdateState::Unavailable
                        }
                        Err(error) => {
                            view.update_state = UpdateState::Error(error.message().into())
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn request_update_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_none()
            || self.update_scheduling
            || self.prompt_pending
            || self.picker_pending
            || self.close_after_save
            || self.pending_close.is_some()
            || !matches!(self.update_state, UpdateState::Prepared(_))
        {
            return;
        }
        self.discard_on_close = false;
        if self.projects.iter().any(|project| {
            project.terminal.read(cx).has_dirty_files(cx)
                || project.terminal.read(cx).has_saving_files(cx)
        }) {
            self.confirm_dirty_close(PendingClose::UpdateRestart, window, cx);
        } else {
            self.finish_close_intent(PendingClose::UpdateRestart, window, cx);
        }
    }

    fn begin_update_persistence(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.restore_warning.is_some() || self.save_failed {
            self.notice = Some(
                "Pideck needs to save the latest layout before restarting. Resolve the layout warning and try again."
                    .into(),
            );
            return;
        }
        self.update_restart_pending = true;
        self.close_after_save = true;
        self.persist(window, cx);
    }

    fn schedule_prepared_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let UpdateState::Prepared(version) = &self.update_state else {
            self.update_restart_pending = false;
            self.close_after_save = false;
            return;
        };
        let version = version.clone();
        self.update_scheduling = true;
        self.update_state = UpdateState::Scheduling;
        self.selector_open = false;
        self.appearance_menu_open = false;
        window.focus(&self.focus_handle);
        cx.notify();
        let schedule_version = version.clone();
        let schedule = cx
            .background_executor()
            .spawn(async move { app_update::schedule_prepared_update(&schedule_version) });
        cx.spawn_in(window, async move |view, cx| {
            let result = schedule.await;
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    match result {
                        Ok(ScheduleOutcome::Scheduled) => {
                            window.remove_window();
                            return;
                        }
                        Ok(ScheduleOutcome::Unavailable) => {
                            view.release_update_schedule(UpdateState::Unavailable, None);
                        }
                        Ok(ScheduleOutcome::NotPrepared) => {
                            view.release_update_schedule(
                                UpdateState::Error(
                                    "The prepared update is no longer available. Check for updates and try again."
                                        .into(),
                                ),
                                None,
                            );
                        }
                        Err(error) => {
                            // The cached package stays ready for the next explicit restart.
                            view.release_update_schedule(
                                UpdateState::Prepared(version.clone()),
                                Some(error.message().into()),
                            );
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn release_update_schedule(&mut self, state: UpdateState, notice: Option<String>) {
        self.update_state = state;
        self.notice = notice;
        self.update_scheduling = false;
        self.update_restart_pending = false;
        self.close_after_save = false;
        self.discard_on_close = false;
        self.pending_close = None;
    }

    fn interaction_locked(&self) -> bool {
        self.update_scheduling
    }

    fn restore_terminals(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = &self.workspace else {
            return;
        };
        let layouts = workspace.projects.clone();
        for project in layouts {
            self.push_terminal(
                project.path,
                project.tab_count,
                project.active_tab,
                window,
                cx,
            );
        }
        self.resize(window, cx);
        self.focus_active(window, cx);
    }

    fn choose_appearance(
        &mut self,
        appearance: theme::Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.interaction_locked() {
            return;
        }
        theme::set_appearance(appearance);
        super::file_editor::FileEditor::apply_appearance(cx);
        for project in &self.projects {
            project
                .terminal
                .update(cx, |pane, cx| pane.apply_appearance(cx));
            project.files.update(cx, |_, cx| cx.notify());
            project.git.update(cx, |_, cx| cx.notify());
        }
        self.appearance_menu_open = false;
        self.appearance_revision = self.appearance_revision.wrapping_add(1);
        self.save_appearance(window, cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    fn save_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.appearance_saving {
            return;
        }
        self.appearance_saving = true;
        let revision = self.appearance_revision;
        let appearance = theme::appearance();
        let path = self.storage_path.with_file_name("appearance.json");
        let save = cx
            .background_executor()
            .spawn(async move { crate::services::appearance::save(&path, appearance) });
        cx.spawn_in(window, async move |view, cx| {
            let result = save.await;
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.appearance_saving = false;
                    if let Err(error) = result {
                        eprintln!("Appearance preference could not be saved: {error}");
                        view.notice = Some(APPEARANCE_SAVE_ERROR.into());
                        view.close_after_save = false;
                    } else if matches!(
                        view.notice.as_deref(),
                        Some(APPEARANCE_SAVE_ERROR | APPEARANCE_LOAD_ERROR)
                    ) {
                        view.notice = None;
                    }
                    // Serialize writes so a slow previous choice cannot overwrite the latest one.
                    if view.appearance_revision != revision {
                        view.save_appearance(window, cx);
                    } else if view.close_after_save
                        && !view.saving
                        && view.revision == view.saved_revision
                    {
                        view.complete_window_close(window, cx);
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn appearance_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("appearance-menu")
            .track_focus(&self.appearance_focus)
            .debug_selector(|| "appearance-menu".into())
            .occlude()
            .w(px(chrome::MENU_WIDTH))
            .max_h(px(chrome::MENU_MAX_HEIGHT))
            .overflow_y_scroll()
            .p(px(chrome::SMALL_GAP))
            .font_family(chrome::CHROME_FONT)
            .font_weight(FontWeight::NORMAL)
            .text_size(px(chrome::CONTROL_TEXT_SIZE))
            .line_height(px(chrome::CONTROL_LINE_HEIGHT))
            .text_color(theme::bone())
            .bg(theme::panel_lift())
            .border_1()
            .border_color(theme::edge_hard())
            .rounded(px(chrome::CONTROL_RADIUS))
            .on_mouse_down_out(
                cx.listener(|view, event: &gpui::MouseDownEvent, window, cx| {
                    if view
                        .appearance_bounds
                        .is_some_and(|bounds| bounds.contains(&event.position))
                    {
                        return;
                    }
                    view.appearance_menu_open = false;
                    if view.appearance_focus.is_focused(window) {
                        view.focus_active(window, cx);
                    }
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                let count = theme::Appearance::ALL.len();
                match event.keystroke.key.as_str() {
                    "up" => {
                        view.appearance_selected = (view.appearance_selected + count - 1) % count
                    }
                    "down" => view.appearance_selected = (view.appearance_selected + 1) % count,
                    "tab" => {
                        view.appearance_selected = (view.appearance_selected
                            + if event.keystroke.modifiers.shift {
                                count - 1
                            } else {
                                1
                            })
                            % count
                    }
                    "enter" | "space" => view.choose_appearance(
                        theme::Appearance::ALL[view.appearance_selected],
                        window,
                        cx,
                    ),
                    "escape" => {
                        view.appearance_menu_open = false;
                        window.focus(&view.appearance_trigger_focus);
                    }
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .children(
                theme::Appearance::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, appearance)| {
                        div()
                            .id(("appearance-choice", index))
                            .h(px(chrome::MENU_ROW_HEIGHT))
                            .px(px(chrome::COMPACT_GAP))
                            .flex()
                            .items_center()
                            .gap(px(chrome::SMALL_GAP))
                            .justify_between()
                            .rounded(px(chrome::CONTROL_RADIUS))
                            .border_1()
                            .border_color(if index == self.appearance_selected {
                                theme::focus()
                            } else {
                                gpui::rgba(0)
                            })
                            .when(index == self.appearance_selected, |row| {
                                row.bg(theme::selection())
                            })
                            .cursor_pointer()
                            .hover(|row| row.bg(theme::panel_hover()))
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.choose_appearance(appearance, window, cx)
                            }))
                            .child(
                                div()
                                    .size(px(10.0))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .bg(appearance.swatch())
                                    .border_1()
                                    .border_color(theme::edge()),
                            )
                            .child(div().flex_1().min_w_0().child(appearance.label()))
                            .when(appearance == theme::appearance(), |row| row.child("✓"))
                    }),
            )
    }

    fn push_terminal(
        &mut self,
        path: PathBuf,
        count: usize,
        active: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let terminal = cx.new(|cx| {
            let mut terminal = TerminalView::new(path.clone(), cx);
            terminal.restore_layout(count, active, cx);
            terminal
        });
        let project_path = path.clone();
        let subscription = cx.subscribe_in(
            &terminal,
            window,
            move |view, terminal, event, window, cx| {
                if let TerminalPanelEvent::Review {
                    path,
                    kind,
                    direction,
                } = event
                {
                    if let Some(project) = view
                        .projects
                        .iter()
                        .find(|project| project.terminal.entity_id() == terminal.entity_id())
                    {
                        project.git.update(cx, |git, cx| {
                            git.navigate_review(path, *kind, *direction, cx)
                        });
                    }
                    return;
                }
                if matches!(event, TerminalPanelEvent::FilesSaved) {
                    if let Some(project) = view
                        .projects
                        .iter()
                        .find(|project| project.terminal.entity_id() == terminal.entity_id())
                    {
                        project.git.clone().update(cx, |git, cx| git.refresh(cx));
                    }
                    return;
                }
                if matches!(event, TerminalPanelEvent::ContentChanged) {
                    view.on_content_changed(window, cx);
                    cx.notify();
                    return;
                }
                if matches!(event, TerminalPanelEvent::CloseRequested) {
                    terminal.update(cx, |terminal, cx| terminal.activate(cx));
                }
                let (count, active) = terminal.read(cx).layout_snapshot();
                if let Some(workspace) = &mut view.workspace
                    && let Some(project) = workspace
                        .projects
                        .iter_mut()
                        .find(|project| project.path == path)
                {
                    project.update_layout(count, active);
                }
                view.persist(window, cx);
                cx.notify();
            },
        );
        let reminders = self
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.projects.iter().find(|p| p.path == project_path))
            .map(|project| project.checklist.clone())
            .unwrap_or_default();
        let checklist = cx.new(|cx| ChecklistView::new(reminders, window, cx));
        let checklist_path = project_path.clone();
        let checklist_subscription =
            cx.subscribe_in(&checklist, window, move |view, checklist, _, window, cx| {
                if let Some(workspace) = &mut view.workspace
                    && let Some(project) = workspace
                        .projects
                        .iter_mut()
                        .find(|p| p.path == checklist_path)
                {
                    project.checklist = checklist.read(cx).snapshot();
                }
                view.on_content_changed(window, cx);
                view.persist(window, cx);
                cx.notify();
            });
        let files =
            cx.new(|cx| FilesPanel::new(project_path.clone(), terminal.downgrade(), window, cx));
        let kind_path = project_path.clone();
        let git = cx.new(|cx| GitPanel::new(project_path, terminal.downgrade(), window, cx));
        let file_pane = terminal.clone();
        let file_subscription =
            cx.subscribe_in(&files, window, move |view, _, event, window, cx| {
                view.open_panel_item(&file_pane, event, window, cx);
            });
        let git_pane = terminal.clone();
        let git_subscription = cx.subscribe_in(&git, window, move |view, _, event, window, cx| {
            view.open_panel_item(&git_pane, event, window, cx);
        });
        let observed_files = files.clone();
        let git_observation = cx.observe(&git, move |_, git, cx| {
            if let Some(status) = git.read(cx).status().cloned() {
                observed_files.update(cx, |files, cx| files.set_git_status(&status, cx));
            }
            cx.notify();
        });
        git.update(cx, |git, cx| git.activate(cx));
        let terminal_id = terminal.entity_id();
        let kind_task = cx
            .background_executor()
            .spawn(async move { crate::services::project_files::project_kind(&kind_path) });
        cx.spawn(async move |view, cx| {
            let kind = kind_task.await;
            let _ = view.update(cx, |view, cx| {
                if let Some(project) = view
                    .projects
                    .iter_mut()
                    .find(|project| project.terminal.entity_id() == terminal_id)
                {
                    project.project_kind = kind;
                    cx.notify();
                }
            });
        })
        .detach();
        self.projects.push(ProjectTerminals {
            terminal,
            _subscription: subscription,
            files,
            checklist,
            git,
            sidebar_tab: SidebarTab::Files,
            project_kind: None,
            _panel_subscriptions: vec![
                file_subscription,
                git_subscription,
                git_observation,
                checklist_subscription,
            ],
        });
    }

    fn active_terminal(&self) -> Option<Entity<TerminalView>> {
        self.workspace
            .as_ref()
            .and_then(|workspace| self.projects.get(workspace.active))
            .map(|project| project.terminal.clone())
    }

    fn focus_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(terminal) = self.active_terminal() {
            terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
        }
        self.activate_sidebar(cx);
        if let Some(project) = self.active_project() {
            project.git.clone().update(cx, |git, cx| git.refresh(cx));
        }
        if let Some(workspace) = &self.workspace {
            let name = project_name(&workspace.projects[workspace.active].path);
            window.set_window_title(&format!("{name} — Pideck"));
        }
    }

    fn resize(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // Terminal elements measure their own content bounds before resizing the PTY.
        cx.notify();
    }

    fn select_project(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.interaction_locked() {
            return;
        }
        let Some(workspace) = &mut self.workspace else {
            return;
        };
        if !workspace.select_project(index) {
            return;
        }
        self.selector_open = false;
        self.focus_active(window, cx);
        self.persist(window, cx);
        cx.notify();
    }

    fn cycle_project(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = &self.workspace else {
            return;
        };
        let count = workspace.projects.len();
        let next = if reverse {
            (workspace.active + count - 1) % count
        } else {
            (workspace.active + 1) % count
        };
        self.select_project(next, window, cx);
    }

    fn choose_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.interaction_locked()
            || self.picker_pending
            || self.prompt_pending
            || self.close_after_save
            || self.workspace.is_none()
        {
            return;
        }
        self.picker_pending = true;
        self.selector_open = false;
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add project".into()),
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = match picker.await {
                Ok(Ok(Some(paths))) => {
                    cx.background_executor()
                        .spawn(async move {
                            let Some(path) = paths.into_iter().next() else {
                                return Ok(None);
                            };
                            let path = std::fs::canonicalize(path).map_err(|_| ())?;
                            if !path.is_dir() {
                                return Err(());
                            }
                            Ok(Some(
                                crate::services::paths::without_windows_verbatim_prefix(&path),
                            ))
                        })
                        .await
                }
                Ok(Ok(None)) => Ok(None),
                _ => Err(()),
            };
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.picker_pending = false;
                    match result {
                        Ok(Some(path)) => {
                            if let Some(workspace) = &mut view.workspace {
                                let before = workspace.projects.len();
                                let index = workspace.insert_project(path);
                                let path = workspace.projects[index].path.clone();
                                if workspace.projects.len() > before {
                                    view.push_terminal(path, 1, 0, window, cx);
                                }
                                view.resize(window, cx);
                                view.focus_active(window, cx);
                                view.persist(window, cx);
                            }
                        }
                        Ok(None) => view.focus_active(window, cx),
                        Err(()) => view.notice = Some(
                            "The folder could not be opened. Choose an accessible project folder."
                                .into(),
                        ),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
    }

    fn remove_project(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.interaction_locked() {
            return;
        }
        let Some(workspace) = &self.workspace else {
            return;
        };
        if self.prompt_pending || workspace.projects.len() <= 1 {
            return;
        }
        let path = workspace.projects[index].path.clone();
        if self.projects[index].terminal.read(cx).has_dirty_files(cx)
            || self.projects[index].terminal.read(cx).has_saving_files(cx)
        {
            self.confirm_dirty_close(PendingClose::Project(path), window, cx);
            return;
        }
        self.prompt_pending = true;
        let title = format!("Remove {}?", project_name(&path));
        let count = self.projects[index].terminal.read(cx).terminal_count();
        let detail = format!(
            "Its {count} terminal{} will close and its saved checklist will be removed. Files on disk stay in place.",
            if count == 1 { "" } else { "s" }
        );
        let prompt = window.prompt(
            PromptLevel::Warning,
            &title,
            Some(&detail),
            &["Keep project", "Remove project"],
            cx,
        );
        cx.spawn_in(window, async move |view, cx| {
            let accepted = prompt.await == Ok(1);
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.prompt_pending = false;
                    if accepted
                        && let Some(workspace) = &mut view.workspace
                        && let Some(index) = workspace
                            .projects
                            .iter()
                            .position(|project| project.path == path)
                        && workspace.remove_project(index)
                    {
                        view.projects.remove(index);
                        view.focus_active(window, cx);
                        view.persist(window, cx);
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn persist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.revision += 1;
        self.start_save(window, cx);
    }

    fn start_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.restore_warning.is_some() {
            return;
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        self.saving = true;
        self.save_failed = false;
        let revision = self.revision;
        let path = self.storage_path.clone();
        let save = cx
            .background_executor()
            .spawn(async move { workspace.save(&path) });
        cx.spawn_in(window, async move |view, cx| {
            let result = save.await;
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.saving = false;
                    match result {
                        Ok(()) => {
                            view.save_failed = false;
                            view.saved_revision = revision;
                            if view.revision != revision {
                                view.start_save(window, cx);
                            } else if view.close_after_save {
                                view.complete_window_close(window, cx);
                            }
                        }
                        Err(error) => {
                            eprintln!("Terminal layout save failed: {error}");
                            view.save_failed = true;
                            view.close_after_save = false;
                            view.update_restart_pending = false;
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.update_scheduling {
            return false;
        }
        if self.workspace.is_none() {
            return true;
        }
        if self.prompt_pending
            || self.picker_pending
            || self.close_after_save
            || self.pending_close.is_some()
        {
            return false;
        }
        self.discard_on_close = false;
        if self.projects.iter().any(|project| {
            project.terminal.read(cx).has_dirty_files(cx)
                || project.terminal.read(cx).has_saving_files(cx)
        }) {
            self.confirm_dirty_close(PendingClose::Window, window, cx);
            return false;
        }
        if self.restore_warning.is_some() || self.save_failed {
            self.prompt_pending = true;
            let prompt = window.prompt(PromptLevel::Warning, "Close without saving this layout?",
                Some("Running shells and commands will stop. The previous saved layout will be kept."), &["Cancel", "Close without saving"], cx);
            cx.spawn_in(window, async move |view, cx| {
                let accepted = prompt.await == Ok(1);
                let _ = cx.update(|window, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.prompt_pending = false;
                        if accepted {
                            window.remove_window();
                        }
                        cx.notify();
                    });
                });
            })
            .detach();
            return false;
        }
        let running = self
            .projects
            .iter()
            .any(|project| project.terminal.read(cx).has_running_sessions(cx));
        if !running {
            self.close_after_save = true;
            self.persist(window, cx);
            return false;
        }
        self.prompt_pending = true;
        let prompt = window.prompt(PromptLevel::Warning, "Close all terminals?",
            Some("Running shells and commands will stop. Your project folders and terminal layout will be saved."), &["Cancel", "Close terminals"], cx);
        cx.spawn_in(window, async move |view, cx| {
            let accepted = prompt.await == Ok(1);
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.prompt_pending = false;
                    if accepted {
                        view.close_after_save = true;
                        view.persist(window, cx);
                    }
                    cx.notify();
                });
            });
        })
        .detach();
        false
    }

    fn replace_saved_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt_pending || self.picker_pending || self.restore_warning.is_none() {
            return;
        }
        self.prompt_pending = true;
        let prompt = window.prompt(PromptLevel::Warning, "Replace the saved layout?",
            Some("The saved layout could not be restored. Replace it with the projects and terminal tabs currently open?"), &["Cancel", "Replace saved layout"], cx);
        cx.spawn_in(window, async move |view, cx| {
            let accepted = prompt.await == Ok(1);
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.prompt_pending = false;
                    if accepted {
                        view.restore_warning = None;
                        view.persist(window, cx);
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn toggle_checklist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.interaction_locked() {
            return;
        }
        let Some(workspace) = &mut self.workspace else {
            return;
        };
        workspace.inspector_visible = !workspace.inspector_visible;
        if workspace.inspector_visible {
            if f32::from(window.viewport_size().width) < 720.0 {
                self.notice = Some("Widen the window to show the checklist.".into());
            } else if let Some(project) = self.active_project() {
                project
                    .checklist
                    .update(cx, |list, cx| list.focus(window, cx));
            }
        } else {
            self.focus_active(window, cx);
        }
        self.persist(window, cx);
        cx.notify();
    }

    fn checklist_toggle(&self, enabled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.workspace.as_ref().is_some_and(|w| w.inspector_visible);
        div()
            .id("toggle-checklist")
            .debug_selector(|| "toggle-checklist".into())
            .size(px(chrome::MAIN_CONTROL_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(chrome::CONTROL_RADIUS))
            .border_1()
            .border_color(gpui::transparent_black())
            .bg(theme::panel())
            .when(selected, |b| b.bg(theme::panel_hover()))
            .tooltip(text_tooltip(if selected {
                "Hide project checklist · Ctrl+Shift+L"
            } else {
                "Show project checklist · Ctrl+Shift+L"
            }))
            .when(enabled, |b| {
                b.tab_index(0)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::panel_hover()))
                    .focus(|s| s.border_color(theme::focus()))
                    .on_click(cx.listener(|view, _, window, cx| view.toggle_checklist(window, cx)))
            })
            .when(!enabled, |b| b.opacity(0.5))
            .child(
                svg()
                    .path("icons/inspector.svg")
                    .size(px(16.0))
                    .text_color(if selected {
                        theme::bone()
                    } else {
                        theme::ash()
                    }),
            )
    }

    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.interaction_locked() {
            return;
        }
        if let Some(workspace) = &mut self.workspace {
            workspace.sidebar_visible = !workspace.sidebar_visible;
            self.resize(window, cx);
            self.focus_active(window, cx);
            self.persist(window, cx);
        }
    }

    fn open_panel_item(
        &mut self,
        pane: &Entity<TerminalView>,
        event: &ProjectPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.interaction_locked() {
            return;
        }
        if matches!(event, ProjectPanelEvent::ToggleSidebar) {
            self.toggle_sidebar(window, cx);
            return;
        }
        if matches!(
            event,
            ProjectPanelEvent::FilesChanged | ProjectPanelEvent::BranchChanged
        ) {
            if let Some(project) = self
                .projects
                .iter()
                .find(|project| project.terminal.entity_id() == pane.entity_id())
            {
                project.git.update(cx, |git, cx| git.refresh(cx));
                if matches!(event, ProjectPanelEvent::BranchChanged) {
                    project.files.update(cx, |files, cx| files.refresh(cx));
                    pane.update(cx, |pane, cx| pane.reload_clean_files(window, cx));
                }
            }
            return;
        }
        let active = self
            .active_terminal()
            .is_some_and(|active| active.entity_id() == pane.entity_id());
        let focus = window.focused(cx);
        pane.update(cx, |pane, cx| match event {
            ProjectPanelEvent::OpenFile(path) => pane.open_file(path.clone(), window, cx),
            ProjectPanelEvent::OpenDiff { file, content } => {
                pane.open_diff(file.clone(), content.clone(), window, cx)
            }
            ProjectPanelEvent::ToggleSidebar
            | ProjectPanelEvent::FilesChanged
            | ProjectPanelEvent::BranchChanged => {}
        });
        if !active && let Some(focus) = focus {
            window.focus(&focus);
        }
    }

    fn active_project(&self) -> Option<&ProjectTerminals> {
        self.workspace
            .as_ref()
            .and_then(|workspace| self.projects.get(workspace.active))
    }

    fn activate_sidebar(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.active_project() else {
            return;
        };
        match project.sidebar_tab {
            SidebarTab::Files => project
                .files
                .clone()
                .update(cx, |panel, cx| panel.activate(cx)),
            SidebarTab::Git => project
                .git
                .clone()
                .update(cx, |panel, cx| panel.activate(cx)),
            SidebarTab::Projects => {}
        }
    }

    fn focus_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.active_project() else {
            return;
        };
        match project.sidebar_tab {
            SidebarTab::Files => project
                .files
                .clone()
                .update(cx, |panel, cx| panel.focus(window, cx)),
            SidebarTab::Git => project
                .git
                .clone()
                .update(cx, |panel, cx| panel.focus(window, cx)),
            SidebarTab::Projects => window.focus(&self.sidebar_focus),
        }
    }

    fn select_sidebar(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace
            && let Some(project) = self.projects.get_mut(workspace.active)
        {
            project.sidebar_tab = tab;
        }
        self.focus_sidebar(window, cx);
        if let Some(project) = self.active_project() {
            project.git.clone().update(cx, |git, cx| git.refresh(cx));
        }
        cx.notify();
    }

    fn titlebar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project = self
            .workspace
            .as_ref()
            .map(|workspace| project_name(&workspace.projects[workspace.active].path));
        let title = project.unwrap_or_default();
        div()
            .h(px(chrome::TITLEBAR_HEIGHT))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(theme::chrome())
            .border_b_1()
            .border_color(theme::edge())
            .pr(px(chrome::TITLEBAR_INSET))
            .child(
                div()
                    .flex_shrink_0()
                    .h_full()
                    .px(px(chrome::TITLEBAR_INSET))
                    .flex()
                    .items_center()
                    .whitespace_nowrap()
                    .line_height(px(31.0))
                    .font_family(chrome::HEADING_FONT)
                    .font_weight(FontWeight::NORMAL)
                    .text_size(px(chrome::WORDMARK_SIZE))
                    .text_color(theme::focus())
                    .child("Pideck."),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .px(px(chrome::TITLEBAR_INSET))
                    .flex()
                    .items_center()
                    .justify_center()
                    .window_control_area(WindowControlArea::Drag)
                    .text_size(px(chrome::TITLEBAR_TEXT_SIZE))
                    .text_color(theme::ash())
                    .child(div().truncate().child(title)),
            )
            .child(
                window_control(
                    "window-minimize",
                    "Minimize window",
                    "icons/window-min.svg",
                    false,
                )
                .on_click(|_, window, _| window.minimize_window()),
            )
            .child(
                window_control(
                    "window-maximize",
                    if window.is_maximized() {
                        "Restore window"
                    } else {
                        "Maximize window"
                    },
                    "icons/window-max.svg",
                    false,
                )
                .on_click(|_, window, _| window.zoom_window()),
            )
            .child(
                window_control(
                    "window-close",
                    "Close window",
                    "icons/window-close.svg",
                    true,
                )
                .on_click(cx.listener(|view, _, window, cx| {
                    if view.request_close(window, cx) {
                        window.remove_window();
                    }
                })),
            )
    }

    fn project_picker(
        &self,
        title: String,
        path: String,
        available: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .relative()
            .min_w(px(100.0))
            .w_full()
            .h(px(chrome::MAIN_CONTROL_HEIGHT))
            .on_children_prepainted(cx.processor(|view, bounds: Vec<Bounds<Pixels>>, _, _| {
                view.selector_bounds = bounds.first().copied();
            }))
            .child(
                picker_trigger("project-selector", self.selector_open)
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .w_full()
                    .debug_selector(|| "project-selector".into())
                    .track_focus(&self.selector_trigger_focus)
                    .tooltip(text_tooltip(path))
                    .when(!available, |button| button.opacity(0.55))
                    .when(available, |button| {
                        button.tab_index(0).cursor_pointer().on_click(cx.listener(
                            |view, _, window, cx| {
                                view.appearance_menu_open = false;
                                view.selector_open = !view.selector_open;
                                view.selector_selected = view
                                    .workspace
                                    .as_ref()
                                    .map_or(0, |workspace| workspace.active);
                                if view.selector_open {
                                    view.selector_scroll.scroll_to_item(view.selector_selected);
                                    window.focus(&view.selector_focus);
                                } else {
                                    window.focus(&view.selector_trigger_focus);
                                }
                                cx.notify();
                            },
                        ))
                    })
                    .child(
                        svg()
                            .path("icons/folder.svg")
                            .size(px(chrome::ICON_SIZE))
                            .flex_shrink_0()
                            .text_color(theme::ash()),
                    )
                    .child(
                        div()
                            .id("project-selector-label")
                            .debug_selector(|| "project-selector-label".into())
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(title)),
                    )
                    .child(picker_chevron(self.selector_open)),
            )
            .when(self.selector_open, |wrapper| {
                wrapper.child(picker_popup(self.project_selector(cx)))
            })
    }

    fn appearance_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .w(px(chrome::APPEARANCE_CONTROL_WIDTH))
            .h(px(chrome::MAIN_CONTROL_HEIGHT))
            .flex_shrink_0()
            .on_children_prepainted(cx.processor(|view, bounds: Vec<Bounds<Pixels>>, _, _| {
                view.appearance_bounds = bounds.first().copied();
            }))
            .child(
                picker_trigger("appearance-picker", self.appearance_menu_open)
                    .text_size(px(12.0))
                    .font_weight(FontWeight::NORMAL)
                    .debug_selector(|| "appearance-picker".into())
                    .track_focus(&self.appearance_trigger_focus)
                    .w_full()
                    .tab_index(0)
                    .cursor_pointer()
                    .tooltip(text_tooltip("Choose appearance"))
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.selector_open = false;
                        view.appearance_menu_open = !view.appearance_menu_open;
                        if view.appearance_menu_open {
                            view.appearance_selected = theme::Appearance::ALL
                                .iter()
                                .position(|appearance| *appearance == theme::appearance())
                                .unwrap_or(0);
                            window.focus(&view.appearance_focus);
                        } else {
                            window.focus(&view.appearance_trigger_focus);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(10.0))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(theme::canvas())
                            .border_1()
                            .border_color(theme::bone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(theme::appearance().label()),
                    )
                    .child(picker_chevron(self.appearance_menu_open)),
            )
            .when(self.appearance_menu_open, |wrapper| {
                wrapper.child(picker_popup(self.appearance_menu(cx)))
            })
    }

    fn workbench_toolbar(
        &self,
        title: String,
        path: String,
        sidebar_visible: bool,
        available: bool,
        can_add_terminal: bool,
        wide: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .h(px(chrome::TOOLBAR_HEIGHT))
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .bg(theme::chrome())
            .border_b_1()
            .border_color(theme::edge())
            .child(
                div()
                    .w(px(if sidebar_visible {
                        self.sidebar_width
                    } else {
                        200.0
                    }))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .pl(px(chrome::SIDEBAR_INSET))
                    .border_r_1()
                    .border_color(theme::edge())
                    .pr(px(8.0))
                    .child(self.project_picker(title.clone(), path.clone(), available, cx)),
            )
            .child(
                div()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .px(px(chrome::TOOLBAR_INSET))
                    .flex()
                    .items_center()
                    .gap(px(chrome::GAP))
                    .when(!sidebar_visible, |toolbar| {
                        toolbar.child(sidebar_toggle(available, cx))
                    })
                    .child(
                        div()
                            .id("workspace-path")
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(theme::ash())
                            .tooltip(text_tooltip(path.clone()))
                            .child(if wide {
                                let parent = std::path::Path::new(&path)
                                    .parent()
                                    .and_then(|p| p.file_name())
                                    .map(|p| p.to_string_lossy().into_owned());
                                parent.map_or_else(
                                    || format!("Projects  /  {title}"),
                                    |parent| format!("Projects  /  {parent}  /  {title}"),
                                )
                            } else {
                                title
                            }),
                    )
                    .child(self.appearance_picker(cx))
                    .child(
                        div()
                            .id("new-terminal")
                            .debug_selector(|| "new-terminal".into())
                            .h(px(chrome::MAIN_CONTROL_HEIGHT))
                            .px(px(12.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .rounded(px(chrome::CONTROL_RADIUS))
                            .border_1()
                            .border_color(theme::focus())
                            .bg(theme::focus())
                            .text_color(theme::on_accent())
                            .text_size(px(chrome::CONTROL_TEXT_SIZE))
                            .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                            .font_weight(FontWeight::SEMIBOLD)
                            .tooltip(text_tooltip(if can_add_terminal {
                                "Open a terminal in this project · Ctrl+` or Ctrl+Shift+T"
                            } else if available {
                                "This project has eight terminal tabs"
                            } else {
                                "Wait for the workspace to be ready"
                            }))
                            .when(!can_add_terminal, |button| button.opacity(0.55))
                            .when(can_add_terminal, |button| {
                                button
                                    .tab_index(0)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(theme::accent_hover()))
                                    .active(|style| style.bg(theme::accent_pressed()))
                                    .focus(|style| style.border_color(theme::bone()))
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        if let Some(terminal) = view.active_terminal() {
                                            terminal.update(cx, |terminal, cx| {
                                                terminal.add_tab(window, cx)
                                            });
                                        }
                                    }))
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(chrome::SMALL_GAP))
                                    .child(
                                        svg()
                                            .path("icons/plus.svg")
                                            .size(px(12.0))
                                            .text_color(theme::on_accent()),
                                    )
                                    .child("New terminal"),
                            )
                            .when(wide, |button| {
                                button.child(
                                    div()
                                        .font_family(theme::mono())
                                        .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                        .child("Ctrl ⇧ T"),
                                )
                            }),
                    )
                    .child(self.checklist_toggle(available, cx)),
            )
    }

    fn update_footer(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let status = match &self.update_state {
            UpdateState::Unavailable => "Development build".to_owned(),
            UpdateState::Checking => "Checking for updates…".to_owned(),
            UpdateState::Current => "Up to date".to_owned(),
            UpdateState::Available(version) => format!("Update {version} available"),
            UpdateState::Downloading => "Preparing update…".to_owned(),
            UpdateState::Prepared(version) => format!("Update {version} ready"),
            UpdateState::Scheduling => "Starting update…".to_owned(),
            UpdateState::Error(_) => "Update check failed".to_owned(),
        };
        let owner = cx.weak_entity();
        let state = self.update_state.clone();
        Some(
            div()
                .id("app-update-status")
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .id("update-message")
                        .truncate()
                        .tooltip(text_tooltip(status.clone()))
                        .child(status),
                )
                .child(
                    Button::new("status-menu")
                        .label("⋯")
                        .ghost()
                        .w(px(24.0))
                        .h(px(24.0))
                        .tooltip("Version and updates")
                        .dropdown_menu(move |menu, _, _| {
                            let owner = owner.clone();
                            let menu = menu.item(
                                PopupMenuItem::new(format!(
                                    "Pideck {}",
                                    app_update::CURRENT_VERSION
                                ))
                                .disabled(true),
                            );
                            let (label, enabled) = match state {
                                UpdateState::Available(_) => ("Update and restart", true),
                                UpdateState::Prepared(_) => ("Restart to update", true),
                                UpdateState::Current | UpdateState::Error(_) => {
                                    ("Check for updates", true)
                                }
                                UpdateState::Unavailable => {
                                    ("Updates are available in installed builds", false)
                                }
                                _ => ("Update in progress…", false),
                            };
                            menu.item(PopupMenuItem::new(label).disabled(!enabled).on_click(
                                move |_, window, cx| {
                                    let owner = owner.clone();
                                    window.defer(cx, move |window, cx| {
                                        let _ =
                                            owner.update(cx, |view, cx| match view.update_state {
                                                UpdateState::Available(_) => {
                                                    view.prepare_and_restart(window, cx)
                                                }
                                                UpdateState::Prepared(_) => {
                                                    view.request_update_restart(window, cx)
                                                }
                                                UpdateState::Current | UpdateState::Error(_) => {
                                                    view.check_for_updates(window, cx)
                                                }
                                                _ => {}
                                            });
                                    });
                                },
                            ))
                        }),
                )
                .into_any_element(),
        )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self
            .active_project()
            .map_or(SidebarTab::Projects, |project| project.sidebar_tab);
        let changed = self
            .active_project()
            .and_then(|project| project.git.read(cx).change_count());
        let content = match (tab, self.active_project()) {
            (SidebarTab::Files, Some(project)) => project.files.clone().into_any_element(),
            (SidebarTab::Git, Some(project)) => project.git.clone().into_any_element(),
            _ => self.projects_panel(cx).into_any_element(),
        };
        div()
            .w(px(self.sidebar_width))
            .h_full()
            .min_w_0()
            .overflow_hidden()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(theme::floor())
            .border_r_1()
            .border_color(theme::edge())
            .child(
                div()
                    .h(px(chrome::SIDEBAR_NAV_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .px(px(12.0))
                    .bg(theme::floor())
                    .border_b_1()
                    .border_color(theme::edge())
                    .children(
                        [
                            (SidebarTab::Files, "Files"),
                            (SidebarTab::Projects, "Projects"),
                            (SidebarTab::Git, "Git"),
                        ]
                        .into_iter()
                        .map(|(item, label)| {
                            div()
                                .id(SharedString::from(format!("sidebar-{label}")))
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(chrome::SMALL_GAP))
                                .border_b_2()
                                .border_color(if tab == item {
                                    theme::focus()
                                } else {
                                    gpui::rgba(0x00000000)
                                })
                                .text_size(px(12.0))
                                .text_color(if tab == item {
                                    theme::bone()
                                } else {
                                    theme::ash()
                                })
                                .font_weight(if tab == item {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .tab_index(0)
                                .cursor_pointer()
                                .hover(|style| {
                                    style.bg(theme::panel_hover()).text_color(theme::bone())
                                })
                                .focus(|style| {
                                    style.bg(theme::selection()).border_color(theme::focus())
                                })
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.select_sidebar(item, window, cx)
                                }))
                                .child(
                                    svg()
                                        .path(match item {
                                            SidebarTab::Files => "icons/files.svg",
                                            SidebarTab::Projects => "icons/folder.svg",
                                            SidebarTab::Git => "icons/branch.svg",
                                        })
                                        .size(px(14.0))
                                        .text_color(if tab == item {
                                            theme::bone()
                                        } else {
                                            theme::ash()
                                        }),
                                )
                                .child(label)
                                .when(item == SidebarTab::Git, |item| {
                                    item.when_some(changed, |item, count| {
                                        item.child(
                                            div()
                                                .font_family(theme::mono())
                                                .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                                .text_color(theme::focus())
                                                .child(count.to_string()),
                                        )
                                    })
                                })
                        }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(content),
            )
    }

    fn project_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.projects.clone())
            .unwrap_or_default();
        div()
            .id("project-selector-menu")
            .track_focus(&self.selector_focus)
            .occlude()
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                let count = view
                    .workspace
                    .as_ref()
                    .map_or(0, |workspace| workspace.projects.len());
                match event.keystroke.key.as_str() {
                    "up" => view.selector_selected = (view.selector_selected + count) % (count + 1),
                    "down" => view.selector_selected = (view.selector_selected + 1) % (count + 1),
                    "tab" => {
                        view.selector_selected = (view.selector_selected
                            + if event.keystroke.modifiers.shift {
                                count
                            } else {
                                1
                            })
                            % (count + 1);
                    }
                    "enter" | "space" => {
                        if view.selector_selected < count {
                            view.select_project(view.selector_selected, window, cx);
                        } else {
                            view.choose_project(window, cx);
                        }
                    }
                    "escape" => {
                        view.selector_open = false;
                        window.focus(&view.selector_trigger_focus);
                    }
                    _ => return,
                }
                view.selector_scroll.scroll_to_item(view.selector_selected);
                cx.stop_propagation();
                cx.notify();
            }))
            .debug_selector(|| "project-selector-menu".into())
            .w(px(300.0))
            .max_h(px(chrome::MENU_MAX_HEIGHT))
            .overflow_y_scroll()
            .track_scroll(&self.selector_scroll)
            .p(px(chrome::SMALL_GAP))
            .rounded(px(chrome::CONTROL_RADIUS))
            .font_family(chrome::CHROME_FONT)
            .font_weight(FontWeight::NORMAL)
            .text_size(px(chrome::CONTROL_TEXT_SIZE))
            .line_height(px(chrome::CONTROL_LINE_HEIGHT))
            .text_color(theme::bone())
            .bg(theme::panel_lift())
            .border_1()
            .border_color(theme::edge_hard())
            .on_mouse_down_out(
                cx.listener(|view, event: &gpui::MouseDownEvent, window, cx| {
                    if view
                        .selector_bounds
                        .is_some_and(|bounds| bounds.contains(&event.position))
                    {
                        return;
                    }
                    view.selector_open = false;
                    if view.selector_focus.is_focused(window) {
                        view.focus_active(window, cx);
                    }
                    cx.notify();
                }),
            )
            .children(entries.into_iter().enumerate().map(|(index, project)| {
                let active = self
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.active == index);
                div()
                    .id(("choose-project", index))
                    .h(px(chrome::MENU_ROW_HEIGHT))
                    .px(px(chrome::COMPACT_GAP))
                    .flex()
                    .items_center()
                    .gap(px(chrome::COMPACT_GAP))
                    .rounded(px(chrome::CONTROL_RADIUS))
                    .border_1()
                    .border_color(if self.selector_selected == index {
                        theme::focus()
                    } else {
                        gpui::rgba(0)
                    })
                    .cursor_pointer()
                    .when(self.selector_selected == index, |row| {
                        row.bg(theme::selection())
                    })
                    .hover(|style| style.bg(theme::panel_hover()))
                    .tooltip(text_tooltip(project.path.to_string_lossy().into_owned()))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.select_project(index, window, cx)
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(project_name(&project.path)),
                    )
                    .child(
                        div()
                            .w(px(chrome::ICON_SIZE))
                            .flex_shrink_0()
                            .when(active, |mark| mark.child("✓")),
                    )
            }))
            .child(
                div()
                    .id("selector-add-project")
                    .h(px(chrome::MENU_ROW_HEIGHT))
                    .px(px(chrome::COMPACT_GAP))
                    .flex()
                    .items_center()
                    .rounded(px(chrome::CONTROL_RADIUS))
                    .border_1()
                    .border_color(gpui::rgba(0))
                    .when(
                        self.selector_selected
                            == self
                                .workspace
                                .as_ref()
                                .map_or(0, |workspace| workspace.projects.len()),
                        |row| row.bg(theme::selection()).border_color(theme::focus()),
                    )
                    .when(!self.picker_pending, |row| {
                        row.cursor_pointer()
                            .hover(|style| style.bg(theme::panel_hover()))
                            .on_click(
                                cx.listener(|view, _, window, cx| view.choose_project(window, cx)),
                            )
                    })
                    .when(self.picker_pending, |row| row.opacity(0.55))
                    .child("Add project folder…"),
            )
    }

    fn confirm_dirty_close(
        &mut self,
        intent: PendingClose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prompt_pending || self.pending_close.is_some() {
            return;
        }
        self.prompt_pending = true;
        let detail = if matches!(intent, PendingClose::Project(_)) {
            "Removing this project also removes its saved checklist and stops its terminals. Save changed files, discard their edits, or cancel."
        } else {
            "Closing also stops the affected terminals. Save all changed files, discard their edits, or cancel."
        };
        let prompt = window.prompt(
            PromptLevel::Warning,
            "Save changed files before closing?",
            Some(detail),
            &["Cancel", "Save all and close", "Discard changes and close"],
            cx,
        );
        cx.spawn_in(window, async move |view, cx| {
            let answer = prompt.await.unwrap_or(0);
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.prompt_pending = false;
                    match answer {
                        1 => {
                            view.pending_close = Some(intent.clone());
                            for pane in view.close_panes(&intent) {
                                pane.update(cx, |pane, cx| pane.save_all(window, cx));
                            }
                            view.continue_pending_close(window, cx);
                        }
                        2 => {
                            // A discard decision still waits for any already-running saves so
                            // restart never races an editor write.
                            view.discard_on_close = true;
                            view.pending_close = Some(intent);
                            view.continue_pending_close(window, cx);
                        }
                        _ => {}
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn close_panes(&self, intent: &PendingClose) -> Vec<Entity<TerminalView>> {
        match intent {
            PendingClose::Window | PendingClose::UpdateRestart => self
                .projects
                .iter()
                .map(|project| project.terminal.clone())
                .collect(),
            PendingClose::Project(path) => self
                .workspace
                .as_ref()
                .and_then(|workspace| {
                    workspace
                        .projects
                        .iter()
                        .position(|project| &project.path == path)
                })
                .and_then(|index| self.projects.get(index))
                .map(|project| vec![project.terminal.clone()])
                .unwrap_or_default(),
        }
    }

    fn on_content_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_after_save {
            self.close_after_save = false;
            self.update_restart_pending = false;
            self.discard_on_close = false;
            self.notice =
                Some("Files changed while closing. Review the changes and close again.".into());
        }
        self.continue_pending_close(window, cx);
    }

    fn complete_window_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_scheduling || self.saving || self.revision != self.saved_revision {
            return;
        }
        if self.appearance_saving {
            return;
        }
        if !self.discard_on_close
            && self.projects.iter().any(|project| {
                project.terminal.read(cx).has_dirty_files(cx)
                    || project.terminal.read(cx).has_saving_files(cx)
            })
        {
            self.close_after_save = false;
            self.update_restart_pending = false;
            self.notice = Some(
                "Files changed while closing. Save them or close again to review the changes."
                    .into(),
            );
            cx.notify();
            return;
        }
        if self.update_restart_pending {
            self.schedule_prepared_update(window, cx);
        } else {
            window.remove_window();
        }
    }

    fn continue_pending_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(intent) = self.pending_close.clone() else {
            return;
        };
        let panes = self.close_panes(&intent);
        if panes.iter().any(|pane| pane.read(cx).has_saving_files(cx)) {
            return;
        }
        self.pending_close = None;
        if !self.discard_on_close
            && let Some(pane) = panes
                .iter()
                .find(|pane| pane.read(cx).has_dirty_files(cx))
                .cloned()
        {
            if let Some(index) = self
                .projects
                .iter()
                .position(|project| project.terminal.entity_id() == pane.entity_id())
            {
                self.select_project(index, window, cx);
            }
            self.notice =
                Some("Some files could not be saved. Resolve their errors before closing.".into());
            pane.update(cx, |pane, cx| pane.focus_dirty_file(window, cx));
            return;
        }
        self.finish_close_intent(intent, window, cx);
    }

    fn finish_close_intent(
        &mut self,
        intent: PendingClose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match intent {
            PendingClose::Window => {
                if self.restore_warning.is_some() || self.save_failed {
                    window.remove_window();
                } else {
                    self.close_after_save = true;
                    self.persist(window, cx);
                }
            }
            PendingClose::Project(path) => {
                self.discard_on_close = false;
                if let Some(workspace) = &mut self.workspace
                    && let Some(index) = workspace
                        .projects
                        .iter()
                        .position(|project| project.path == path)
                    && workspace.remove_project(index)
                {
                    self.projects.remove(index);
                    self.focus_active(window, cx);
                    self.persist(window, cx);
                }
            }
            PendingClose::UpdateRestart => {
                if self.restore_warning.is_some() || self.save_failed {
                    self.notice = Some(
                        "Pideck needs to save the latest layout before restarting. Resolve the layout warning and try again."
                            .into(),
                    );
                    self.discard_on_close = false;
                    return;
                }
                let running = self
                    .projects
                    .iter()
                    .any(|project| project.terminal.read(cx).has_running_sessions(cx));
                if !running {
                    self.begin_update_persistence(window, cx);
                    return;
                }
                self.prompt_pending = true;
                let prompt = window.prompt(
                    PromptLevel::Warning,
                    "Restart Pideck to install the update?",
                    Some("Running shells and commands will stop after the latest workspace layout is saved."),
                    &["Cancel", "Restart to update"],
                    cx,
                );
                cx.spawn_in(window, async move |view, cx| {
                    let accepted = prompt.await == Ok(1);
                    let _ = cx.update(|window, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.prompt_pending = false;
                            if accepted {
                                view.begin_update_persistence(window, cx);
                            } else {
                                view.discard_on_close = false;
                            }
                            cx.notify();
                        });
                    });
                })
                .detach();
            }
        }
    }

    fn projects_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = Vec::new();
        let available = self.workspace.is_some()
            && !self.prompt_pending
            && !self.close_after_save
            && !self.interaction_locked();
        let can_add = available && !self.picker_pending;
        let project_count = self
            .workspace
            .as_ref()
            .map_or(0, |workspace| workspace.projects.len());
        let can_remove = available && !self.picker_pending && project_count > 1;
        if let Some(workspace) = &self.workspace {
            for (index, project) in workspace.projects.iter().enumerate() {
                if !self.project_query.is_empty()
                    && !project
                        .path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&self.project_query)
                {
                    continue;
                }
                let selected = workspace.active == index;
                let count = self
                    .projects
                    .get(index)
                    .map_or(0, |project| project.terminal.read(cx).terminal_count());
                rows.push(
                    div()
                        .id(("project", index))
                        .tab_index(0)
                        .h(px(chrome::ROW_HEIGHT))
                        .pl(px(chrome::INSET))
                        .pr(px(chrome::INSET))
                        .flex()
                        .items_center()
                        .gap(px(chrome::GAP))
                        .border_1()
                        .border_color(gpui::rgba(0x00000000))
                        .rounded(px(chrome::CONTROL_RADIUS))
                        .bg(if selected {
                            theme::selection()
                        } else {
                            theme::floor()
                        })
                        .cursor_pointer()
                        .hover(move |style| {
                            style.bg(if selected {
                                theme::selection()
                            } else {
                                theme::panel_hover()
                            })
                        })
                        .active(|style| style.bg(theme::selection()))
                        .focus(|style| style.border_color(theme::focus()))
                        .tooltip(text_tooltip(project.path.to_string_lossy().into_owned()))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.select_project(index, window, cx)
                        }))
                        .child(
                            svg()
                                .path("icons/folder.svg")
                                .size(px(chrome::ICON_SIZE))
                                .flex_shrink_0()
                                .text_color(if selected {
                                    theme::bone()
                                } else {
                                    theme::ash()
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(chrome::ROW_DETAIL_GAP))
                                .child(
                                    div()
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .font_weight(if selected {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .child(project_name(&project.path)),
                                )
                                .child(
                                    div()
                                        .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                        .font_family(chrome::CHROME_FONT)
                                        .font_weight(FontWeight::NORMAL)
                                        .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                                        .text_color(theme::ash())
                                        .child(format!(
                                            "{} terminal{}{}",
                                            count,
                                            if count == 1 { "" } else { "s" },
                                            self.projects
                                                .get(index)
                                                .and_then(|project| project.project_kind)
                                                .map_or_else(String::new, |kind| format!(
                                                    " · {kind}"
                                                ))
                                        )),
                                ),
                        )
                        .child({
                            let owner = cx.weak_entity();
                            Button::new(("project-actions", index))
                                .label("⋯")
                                .ghost()
                                .w(px(28.0))
                                .h(px(32.0))
                                .tooltip("Project actions")
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .dropdown_menu(move |menu, _, _| {
                                    let open_owner = owner.clone();
                                    let remove_owner = owner.clone();
                                    menu.item(PopupMenuItem::new("Open project").on_click(
                                        move |_, window, cx| {
                                            let owner = open_owner.clone();
                                            window.defer(cx, move |window, cx| {
                                                let _ = owner.update(cx, |view, cx| {
                                                    view.select_project(index, window, cx)
                                                });
                                            });
                                        },
                                    ))
                                    .item(
                                        PopupMenuItem::new("Remove project")
                                            .disabled(!can_remove)
                                            .on_click(move |_, window, cx| {
                                                let owner = remove_owner.clone();
                                                window.defer(cx, move |window, cx| {
                                                    let _ = owner.update(cx, |view, cx| {
                                                        view.remove_project(index, window, cx)
                                                    });
                                                });
                                            }),
                                    )
                                })
                        }),
                );
            }
        }
        div()
            .id("project-sidebar")
            .track_focus(&self.sidebar_focus)
            .tab_index(0)
            .w_full()
            .h_full()
            .min_h_0()
            .bg(theme::floor())
            .flex()
            .flex_col()
            .focus(|style| style.border_color(theme::focus()))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if !view.sidebar_focus.is_focused(window) {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "down" => {
                        view.cycle_project(false, window, cx);
                        view.focus_sidebar(window, cx);
                    }
                    "up" => {
                        view.cycle_project(true, window, cx);
                        view.focus_sidebar(window, cx);
                    }
                    "enter" if view.sidebar_focus.is_focused(window) => {
                        view.focus_active(window, cx)
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .h(px(48.0))
                    .flex_shrink_0()
                    .px(px(chrome::SIDEBAR_INSET))
                    .flex()
                    .items_center()
                    .justify_between()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::bone_dim())
                    .child("Projects")
                    .child(
                        div()
                            .text_size(px(chrome::DETAIL_TEXT_SIZE))
                            .text_color(theme::ash())
                            .child(project_count.to_string()),
                    ),
            )
            .when_some(self.project_filter.as_ref(), |panel, input| {
                panel.child(super::project_panels::filter_field(input, 0.0, 12.0))
            })
            .child(
                div()
                    .id("project-list")
                    .flex_1()
                    .min_h_0()
                    .py(px(chrome::SMALL_GAP))
                    .px(px(8.0))
                    .gap(px(4.0))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .when(rows.is_empty(), |list| {
                        list.child(
                            div()
                                .p(px(8.0))
                                .text_color(theme::ash())
                                .child("No projects match. Clear the filter to see all projects."),
                        )
                    })
                    .children(rows),
            )
            .child(
                div()
                    .px(px(12.0))
                    .py(px(12.0))
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(theme::edge())
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        button(
                            "add-project",
                            if self.picker_pending {
                                "Opening…"
                            } else {
                                "Add folder"
                            },
                            can_add,
                        )
                        .w_full()
                        .justify_center()
                        .bg(theme::panel_hover())
                        .when(can_add, |button| {
                            button.on_click(cx.listener(Self::choose_project_click))
                        }),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme::ash())
                            .child("Project actions are in the row menu."),
                    ),
            )
    }

    fn choose_project_click(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose_project(window, cx);
    }
}

fn project_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn picker_trigger(id: &'static str, open: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(chrome::MAIN_CONTROL_HEIGHT))
        .min_w_0()
        .px(px(chrome::CONTROL_INSET))
        .flex()
        .items_center()
        .gap(px(chrome::COMPACT_GAP))
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(if open {
            theme::edge_hard()
        } else {
            gpui::rgba(0)
        })
        .bg(if open {
            theme::selection()
        } else {
            gpui::rgba(0)
        })
        .font_family(chrome::CHROME_FONT)
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(chrome::CONTROL_TEXT_SIZE))
        .line_height(px(chrome::CONTROL_LINE_HEIGHT))
        .text_color(theme::bone())
        .hover(move |style| {
            style.bg(if open {
                theme::selection()
            } else {
                theme::panel_hover()
            })
        })
        .active(|style| style.bg(theme::selection()))
        .focus(|style| style.border_color(theme::focus()))
}

fn picker_chevron(open: bool) -> impl IntoElement {
    svg()
        .path(if open {
            "icons/chevron-up.svg"
        } else {
            "icons/chevron-down.svg"
        })
        .size(px(chrome::CONTROL_INSET))
        .flex_shrink_0()
        .text_color(theme::ash())
}

fn picker_popup(menu: impl IntoElement) -> impl IntoElement {
    // A separate absolute sibling keeps the popup out of trigger sizing and click bubbling.
    div()
        .absolute()
        .top(px(chrome::MAIN_CONTROL_HEIGHT + chrome::MENU_GAP))
        .left_0()
        .child(deferred(anchored().snap_to_window().child(menu)).with_priority(1))
}

fn button(id: &'static str, label: &'static str, enabled: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(chrome::CONTROL_HEIGHT))
        .px(px(chrome::CONTROL_INSET))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(chrome::SMALL_GAP))
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(theme::edge())
        .font_family(chrome::CHROME_FONT)
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(chrome::CONTROL_TEXT_SIZE))
        .line_height(px(chrome::CONTROL_LINE_HEIGHT))
        .whitespace_nowrap()
        .text_color(if enabled {
            theme::bone_dim()
        } else {
            theme::smoke()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|style| style.bg(theme::panel_hover()).text_color(theme::bone()))
                .active(|style| style.bg(theme::panel_lift()))
                .focus(|style| style.border_color(theme::focus()).bg(theme::panel_lift()))
        })
        .child(label)
}

fn sidebar_toggle(enabled: bool, cx: &mut Context<TerminalManager>) -> impl IntoElement {
    div()
        .id("toggle-projects")
        .size(px(chrome::CONTROL_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(gpui::rgba(0x00000000))
        .tooltip(text_tooltip("Toggle project sidebar · Ctrl+Shift+B"))
        .when(enabled, |control| {
            control
                .tab_index(0)
                .cursor_pointer()
                .hover(|style| style.bg(theme::panel_hover()))
                .active(|style| style.bg(theme::selection()))
                .focus(|style| style.border_color(theme::focus()))
                .on_click(cx.listener(|view, _, window, cx| view.toggle_sidebar(window, cx)))
        })
        .child(
            svg()
                .path("icons/sidebar.svg")
                .size(px(chrome::ICON_SIZE))
                .text_color(theme::ash()),
        )
}

fn window_control(
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    close: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .group(id)
        .w(px(chrome::WINDOW_CONTROL_WIDTH))
        .h(px(chrome::TITLEBAR_HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(gpui::rgba(0x00000000))
        .text_color(theme::ash())
        .tab_index(0)
        .cursor_pointer()
        .tooltip(text_tooltip(label))
        .hover(move |style| {
            if close {
                style.bg(theme::error()).text_color(theme::on_accent())
            } else {
                style.bg(theme::panel_lift()).text_color(theme::bone())
            }
        })
        .active(move |style| {
            if close {
                style.bg(theme::error()).opacity(0.82)
            } else {
                style.bg(theme::selection())
            }
        })
        .focus(|style| style.border_color(theme::focus()).text_color(theme::bone()))
        // GPUI 0.2.2 SVG painting requires an explicit color on the SVG itself.
        .child(
            svg()
                .path(icon)
                .size(px(chrome::ICON_SIZE))
                .text_color(theme::ash())
                .group_hover(id, move |style| {
                    style.text_color(if close {
                        theme::on_accent()
                    } else {
                        theme::bone()
                    })
                }),
        )
}

struct WorkbenchTooltip(SharedString);

impl Render for WorkbenchTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(chrome::TOOLTIP_MAX_WIDTH))
            .px(px(chrome::INSET))
            .py(px(chrome::COMPACT_GAP))
            .rounded(px(chrome::CONTROL_RADIUS))
            .font_weight(FontWeight::NORMAL)
            .bg(theme::panel_lift())
            .border_1()
            .border_color(theme::edge_hard())
            .font_family(chrome::CHROME_FONT)
            .text_size(px(chrome::CHROME_TEXT_SIZE))
            .line_height(px(chrome::CHROME_LINE_HEIGHT))
            .text_color(theme::bone())
            .whitespace_normal()
            .child(self.0.clone())
    }
}

pub(super) fn text_tooltip(
    label: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let label = label.into();
    move |_, cx| cx.new(|_| WorkbenchTooltip(label.clone())).into()
}

impl Render for TerminalManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = f32::from(window.viewport_size().width);
        // Files, Projects, and Git share one sidebar width so switching tabs
        // does not jump the workbench layout.
        let default_width = if viewport <= 1100.0 {
            chrome::SIDEBAR_MIN
        } else {
            chrome::SIDEBAR_WIDTH
        };
        self.sidebar_width = self
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.sidebar_width)
            .map_or(default_width, f32::from)
            .clamp(
                chrome::SIDEBAR_MIN,
                chrome::SIDEBAR_MAX.min((viewport - 380.0).max(chrome::SIDEBAR_MIN)),
            );
        let inspector_visible = self.workspace.as_ref().is_some_and(|w| w.inspector_visible)
            && viewport >= 720.0
            && !self.update_scheduling;
        let inspector_width = chrome::CHECKLIST_WIDTH.min((viewport - 480.0).max(240.0));
        let sidebar_visible = self.workspace.as_ref().is_some_and(|w| w.sidebar_visible)
            && (!inspector_visible || viewport >= self.sidebar_width + inspector_width + 480.0);
        let path = self
            .workspace
            .as_ref()
            .map(|workspace| {
                workspace.projects[workspace.active]
                    .path
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_default();
        let title = self
            .workspace
            .as_ref()
            .map(|workspace| project_name(&workspace.projects[workspace.active].path))
            .unwrap_or_else(|| "Terminal workspace".into());
        let terminal = self.active_terminal();
        let available = self.workspace.is_some()
            && !self.prompt_pending
            && !self.close_after_save
            && !self.interaction_locked();
        let can_add_terminal = available
            && terminal
                .as_ref()
                .is_some_and(|terminal| terminal.read(cx).layout_snapshot().0 < MAX_TERMINAL_TABS);
        let wide_toolbar =
            f32::from(window.viewport_size().width) >= chrome::WIDE_TOOLBAR_MIN_WIDTH;
        let branch = self
            .active_project()
            .and_then(|project| project.git.read(cx).branch())
            .unwrap_or("—")
            .to_owned();
        let changed = self
            .active_project()
            .and_then(|project| project.git.read(cx).change_count());
        let terminal_count = terminal
            .as_ref()
            .map_or(0, |terminal| terminal.read(cx).terminal_count());
        let warning = self
            .restore_warning
            .clone()
            .or_else(|| {
                self.save_failed.then(|| {
                    "Layout and checklist changes could not be saved. Check storage access and choose Retry save.".into()
                })
            })
            .or_else(|| self.notice.clone());
        let update_footer = self.update_footer(cx);
        let save_status = if self.workspace.is_none() {
            "Loading layout…"
        } else if self.restore_warning.is_some() {
            "Saved layout preserved"
        } else if self.saving {
            "Saving layout…"
        } else if !self.save_failed && self.revision == self.saved_revision {
            "Layout saved"
        } else {
            "Unsaved layout"
        };
        div()
            .id("terminal-manager")
            .key_context("TerminalManager")
            .track_focus(&self.focus_handle)
            .tab_group()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .text_color(theme::bone())
            .font_family(chrome::CHROME_FONT)
            .font_weight(FontWeight::NORMAL)
            .text_size(px(chrome::CHROME_TEXT_SIZE))
            .line_height(px(chrome::CHROME_LINE_HEIGHT))
            .on_mouse_move(cx.listener(|view, event: &gpui::MouseMoveEvent, _, cx| {
                if view.resizing_sidebar {
                    if let Some(workspace) = &mut view.workspace {
                        workspace.sidebar_width = Some(
                            f32::from(event.position.x)
                                .clamp(chrome::SIDEBAR_MIN, chrome::SIDEBAR_MAX)
                                as u16,
                        );
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|view, _, window, cx| {
                    if view.resizing_sidebar {
                        view.resizing_sidebar = false;
                        view.persist(window, cx);
                        cx.notify();
                    }
                }),
            )
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if view.interaction_locked() {
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape" && view.selector_open {
                    view.selector_open = false;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                let modifiers = event.keystroke.modifiers;
                // A focused terminal consumes Tab before it bubbles to workspace chrome.
                if event.keystroke.key == "tab"
                    && !modifiers.control
                    && !modifiers.alt
                    && !modifiers.platform
                {
                    if modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                    cx.stop_propagation();
                }
            }))
            .on_action(
                cx.listener(|view, _: &AddProject, window, cx| view.choose_project(window, cx)),
            )
            .on_action(cx.listener(|view, _: &NewTerminal, window, cx| {
                if !view.interaction_locked()
                    && let Some(terminal) = view.active_terminal()
                {
                    terminal.update(cx, |terminal, cx| terminal.add_tab(window, cx));
                }
            }))
            .on_action(cx.listener(|view, _: &CloseTerminal, window, cx| {
                if !view.interaction_locked()
                    && let Some(terminal) = view.active_terminal()
                {
                    terminal.update(cx, |terminal, cx| terminal.close_active_tab(window, cx));
                }
            }))
            .on_action(cx.listener(|view, _: &NextTerminal, window, cx| {
                if !view.interaction_locked()
                    && let Some(terminal) = view.active_terminal()
                {
                    terminal.update(cx, |terminal, cx| terminal.cycle_tab(false, window, cx));
                }
            }))
            .on_action(cx.listener(|view, _: &PreviousTerminal, window, cx| {
                if !view.interaction_locked()
                    && let Some(terminal) = view.active_terminal()
                {
                    terminal.update(cx, |terminal, cx| terminal.cycle_tab(true, window, cx));
                }
            }))
            .on_action(cx.listener(|view, _: &NextProject, window, cx| {
                view.cycle_project(false, window, cx)
            }))
            .on_action(cx.listener(|view, _: &PreviousProject, window, cx| {
                view.cycle_project(true, window, cx)
            }))
            .on_action(
                cx.listener(|view, _: &ToggleProjects, window, cx| view.toggle_sidebar(window, cx)),
            )
            .on_action(cx.listener(|view, _: &ToggleChecklist, window, cx| {
                view.toggle_checklist(window, cx)
            }))
            .on_action(cx.listener(|view, _: &FocusProjects, window, cx| {
                if !view.interaction_locked()
                    && let Some(workspace) = &mut view.workspace
                {
                    workspace.sidebar_visible = true;
                    view.resize(window, cx);
                    view.persist(window, cx);
                    view.focus_sidebar(window, cx);
                }
            }))
            .child(self.titlebar(window, cx))
            .when(!self.update_scheduling, |workbench| {
                workbench.child(self.workbench_toolbar(
                    title.clone(),
                    path,
                    sidebar_visible,
                    available,
                    can_add_terminal,
                    wide_toolbar,
                    cx,
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .when(sidebar_visible && !self.update_scheduling, |body| {
                        body.child(
                            div()
                                .relative()
                                .h_full()
                                .flex_shrink_0()
                                .child(self.sidebar(cx))
                                .child(
                                    div()
                                        .id("sidebar-resize")
                                        .absolute()
                                        .right(px(-3.0))
                                        .top_0()
                                        .bottom_0()
                                        .w(px(6.0))
                                        .cursor(gpui::CursorStyle::ResizeLeftRight)
                                        .tab_index(0)
                                        .tooltip(text_tooltip(
                                            "Resize sidebar · Left/Right arrows · Home to reset",
                                        ))
                                        .focus(|style| style.bg(theme::edge_hard()))
                                        .on_mouse_down(
                                            gpui::MouseButton::Left,
                                            cx.listener(|view, _, _, cx| {
                                                view.resizing_sidebar = true;
                                                cx.stop_propagation();
                                            }),
                                        )
                                        .on_key_down(cx.listener(
                                            |view, event: &KeyDownEvent, window, cx| {
                                                let next = match event.keystroke.key.as_str() {
                                                    "left" => Some(
                                                        (view.sidebar_width - 16.0)
                                                            .max(chrome::SIDEBAR_MIN)
                                                            as u16,
                                                    ),
                                                    "right" => Some(
                                                        (view.sidebar_width + 16.0)
                                                            .min(chrome::SIDEBAR_MAX)
                                                            as u16,
                                                    ),
                                                    "home" => None,
                                                    _ => return,
                                                };
                                                if let Some(workspace) = &mut view.workspace {
                                                    workspace.sidebar_width = next;
                                                }
                                                view.persist(window, cx);
                                                cx.stop_propagation();
                                                cx.notify();
                                            },
                                        )),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id("terminal-content-slot")
                            .debug_selector(|| "terminal-content-slot".into())
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .h_full()
                            .when(!self.update_scheduling, |body| {
                                body.when_some(terminal, |body, terminal| body.child(terminal))
                            })
                            .when(self.update_scheduling, |body| {
                                body.flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(theme::ash())
                                    .child("Starting the update…")
                            })
                            .when(
                                self.workspace.is_none() && !self.update_scheduling,
                                |body| {
                                    body.flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(chrome::GAP))
                                        .child(
                                            svg()
                                                .path("icons/terminal.svg")
                                                .size(px(chrome::ICON_SIZE))
                                                .text_color(theme::ash()),
                                        )
                                        .child(
                                            div()
                                                .text_color(theme::ash())
                                                .child("Opening projects…"),
                                        )
                                },
                            ),
                    )
                    .when(inspector_visible, |body| {
                        body.when_some(
                            self.active_project().map(|p| p.checklist.clone()),
                            |body, checklist| {
                                body.child(
                                    div()
                                        .w(px(inspector_width))
                                        .flex_shrink_0()
                                        .h_full()
                                        .min_h_0()
                                        .child(checklist),
                                )
                            },
                        )
                    }),
            )
            .when_some(warning, |workbench, warning| {
                workbench.child(
                    div()
                        .flex_shrink_0()
                        .px(px(chrome::INSET))
                        .py(px(chrome::CONTROL_INSET))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(chrome::GAP))
                        .bg(theme::error_wash())
                        .border_t_1()
                        .border_color(theme::edge_hard())
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(chrome::SIDEBAR_WIDTH))
                                .whitespace_normal()
                                .child(warning),
                        )
                        .when(self.restore_warning.is_some(), |banner| {
                            banner.child(
                                button(
                                    "replace-layout",
                                    "Replace saved layout",
                                    available && !self.picker_pending,
                                )
                                .when(
                                    available && !self.picker_pending,
                                    |button| {
                                        button.on_click(cx.listener(|view, _, window, cx| {
                                            view.replace_saved_layout(window, cx)
                                        }))
                                    },
                                ),
                            )
                        })
                        .when(
                            self.save_failed && self.restore_warning.is_none(),
                            |banner| {
                                banner.child(
                                    button("retry-save", "Retry save", available && !self.saving)
                                        .when(available && !self.saving, |button| {
                                            button.on_click(cx.listener(|view, _, window, cx| {
                                                view.persist(window, cx)
                                            }))
                                        }),
                                )
                            },
                        )
                        .when(
                            self.restore_warning.is_none() && !self.save_failed,
                            |banner| {
                                banner.child(button("dismiss-notice", "Dismiss", true).on_click(
                                    cx.listener(|view, _, _, cx| {
                                        view.notice = None;
                                        cx.notify();
                                    }),
                                ))
                            },
                        ),
                )
            })
            .child(
                div()
                    .id("workspace-status")
                    .h(px(chrome::FOOTER_HEIGHT))
                    .min_w_0()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .bg(theme::chrome())
                    .border_t_1()
                    .border_color(theme::edge())
                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                    .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                    .text_color(theme::ash())
                    .tooltip(text_tooltip(
                        self.active_project()
                            .and_then(|project| project.git.read(cx).status())
                            .and_then(crate::services::project_git::GitStatus::line_stats)
                            .map_or_else(
                                || save_status.to_owned(),
                                |stats| {
                                    format!(
                                        "{save_status} · +{} additions · −{} deletions",
                                        stats.additions, stats.deletions
                                    )
                                },
                            ),
                    ))
                    .child(
                        div()
                            .w(px(if sidebar_visible {
                                self.sidebar_width
                            } else {
                                chrome::COLLAPSED_BRAND_WIDTH
                            }))
                            .flex_shrink_0()
                            .h_full()
                            .border_r_1()
                            .border_color(theme::edge())
                            .px(px(chrome::SIDEBAR_INSET))
                            .flex()
                            .items_center()
                            .gap(px(chrome::COMPACT_GAP))
                            .child(
                                svg()
                                    .path("icons/branch.svg")
                                    .w(px(12.0))
                                    .h(px(14.0))
                                    .flex_shrink_0()
                                    .text_color(theme::ash()),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(theme::mono())
                                    .child(branch),
                            )
                            .child(div().flex_1())
                            .when_some(changed.filter(|_| sidebar_visible), |footer, count| {
                                footer.child(div().flex_shrink_0().child(format!(
                                    "{count} change{}",
                                    if count == 1 { "" } else { "s" }
                                )))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(chrome::TOOLBAR_INSET))
                            .flex()
                            .items_center()
                            .gap(px(chrome::GAP))
                            .child(
                                svg()
                                    .path("icons/terminal.svg")
                                    .size(px(12.0))
                                    .text_color(theme::ash()),
                            )
                            .child(div().flex_shrink_0().child(format!(
                                "{terminal_count} terminal{}",
                                if terminal_count == 1 { "" } else { "s" }
                            )))
                            .child(div().flex_1())
                            .when_some(update_footer, |footer, update| footer.child(update)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(cx: &mut Context<TerminalManager>) -> TerminalManager {
        super::super::file_editor::FileEditor::initialize(cx);
        TerminalManager {
            workspace: Some(TerminalWorkspace::new("synthetic-project-one".into())),
            projects: Vec::new(),
            storage_path: PathBuf::new(),
            focus_handle: cx.focus_handle(),
            sidebar_focus: cx.focus_handle(),
            sidebar_width: chrome::SIDEBAR_WIDTH,
            resizing_sidebar: false,
            project_filter: None,
            project_query: String::new(),
            _project_filter_subscription: None,
            notice: None,
            restore_warning: None,
            save_failed: false,
            picker_pending: false,
            prompt_pending: false,
            saving: false,
            revision: 1,
            saved_revision: 0,
            close_after_save: false,
            selector_open: false,
            selector_focus: cx.focus_handle(),
            selector_trigger_focus: cx.focus_handle(),
            selector_bounds: None,
            selector_scroll: ScrollHandle::new(),
            selector_selected: 0,
            appearance_menu_open: false,
            appearance_focus: cx.focus_handle(),
            appearance_trigger_focus: cx.focus_handle(),
            appearance_bounds: None,
            appearance_selected: 0,
            appearance_revision: 0,
            appearance_saving: false,
            discard_on_close: false,
            pending_close: None,
            update_state: UpdateState::Unavailable,
            update_generation: 0,
            update_restart_pending: false,
            update_scheduling: false,
            _bounds_subscription: Subscription::new(|| {}),
        }
    }

    #[gpui::test]
    fn new_terminal_shortcuts_create_one_tab_each_and_respect_update_lock(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(super::super::file_editor::FileEditor::initialize);
        let (root, cx) = cx.add_window_view(|window, cx| {
            let manager = cx.new(|cx| {
                let mut view = fixture(cx);
                TerminalManager::bind_keys(cx);
                view.saving = true;
                view.push_terminal("synthetic-project-one".into(), 1, 0, window, cx);
                window.focus(&view.focus_handle);
                view
            });
            gpui_component::Root::new(manager, window, cx)
        });
        let manager = root.read_with(cx, |root, _| {
            root.view()
                .clone()
                .downcast::<TerminalManager>()
                .ok()
                .unwrap()
        });
        let terminal = manager.read_with(cx, |view, _| view.active_terminal().unwrap());
        cx.simulate_keystrokes("ctrl-`");
        assert_eq!(
            terminal.read_with(cx, |view, _| view.layout_snapshot()),
            (2, 1)
        );
        // The new tab has focus: both shortcuts must also bubble from its PTY input handler.
        cx.simulate_keystrokes("ctrl-`");
        assert_eq!(
            terminal.read_with(cx, |view, _| view.layout_snapshot()),
            (3, 2)
        );
        cx.simulate_keystrokes("ctrl-shift-t");
        assert_eq!(
            terminal.read_with(cx, |view, _| view.layout_snapshot()),
            (4, 3)
        );
        manager.update(cx, |view, cx| {
            view.update_scheduling = true;
            cx.notify();
        });
        cx.simulate_keystrokes("ctrl-`");
        assert_eq!(
            terminal.read_with(cx, |view, _| view.layout_snapshot()),
            (4, 3)
        );
    }

    #[gpui::test]
    fn checklist_toggle_preserves_project_state_and_sidebar_preference(
        cx: &mut gpui::TestAppContext,
    ) {
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, window, cx| {
                view.saving = true;
                let workspace = view.workspace.as_mut().unwrap();
                workspace.projects[0].checklist.sections[0]
                    .add("First project reminder".into(), None);
                workspace.insert_project("synthetic-project-two".into());
                workspace.projects[1].checklist.sections[0]
                    .add("Second project reminder".into(), None);
                view.push_terminal("synthetic-project-one".into(), 1, 0, window, cx);
                view.push_terminal("synthetic-project-two".into(), 1, 0, window, cx);
                view.toggle_checklist(window, cx);
                assert!(view.workspace.as_ref().unwrap().inspector_visible);
                let first = view.projects[0].checklist.clone();
                let second = view.projects[1].checklist.clone();
                assert_ne!(first.entity_id(), second.entity_id());
                assert_eq!(
                    first.read(cx).snapshot().sections[0].tasks[0].label,
                    "First project reminder"
                );
                assert_eq!(
                    second.read(cx).snapshot().sections[0].tasks[0].label,
                    "Second project reminder"
                );
                view.toggle_checklist(window, cx);
                assert!(!view.workspace.as_ref().unwrap().inspector_visible);
                assert!(view.workspace.as_ref().unwrap().sidebar_visible);
                assert_eq!(first.read(cx).snapshot().sections[0].tasks.len(), 1);
                view.update_scheduling = true;
                view.toggle_checklist(window, cx);
                assert!(!view.workspace.as_ref().unwrap().inspector_visible);
            })
            .unwrap();
    }

    #[gpui::test]
    fn checklist_renders_at_the_right_and_toggles_from_toolbar_and_keyboard(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(super::super::file_editor::FileEditor::initialize);
        let (root, cx) = cx.add_window_view(|window, cx| {
            let manager = cx.new(|cx| {
                let mut view = fixture(cx);
                // Match app startup: component initialization precedes app shortcuts.
                TerminalManager::bind_keys(cx);
                view.saving = true;
                view.push_terminal("synthetic-project-one".into(), 1, 0, window, cx);
                view
            });
            gpui_component::Root::new(manager, window, cx)
        });
        let manager = root.read_with(cx, |root, _| {
            root.view()
                .clone()
                .downcast::<TerminalManager>()
                .ok()
                .unwrap()
        });
        cx.simulate_resize(gpui::size(px(1440.0), px(900.0)));
        assert!(cx.debug_bounds("project-checklist").is_none());
        let toggle = cx.debug_bounds("toggle-checklist").unwrap();
        cx.simulate_click(toggle.center(), gpui::Modifiers::none());
        let inspector = cx.debug_bounds("project-checklist").unwrap();
        assert_eq!(inspector.size.width, px(chrome::CHECKLIST_WIDTH));
        assert!(inspector.left() >= px(1000.0));
        cx.simulate_resize(gpui::size(px(800.0), px(600.0)));
        let compact = cx.debug_bounds("project-checklist").unwrap();
        assert_eq!(compact.size.width, px(320.0));
        let terminal_width = cx.debug_bounds("terminal-content-slot").unwrap().size.width;
        assert!(manager.read_with(cx, |v, _| v.workspace.as_ref().unwrap().sidebar_visible));
        cx.simulate_keystrokes("ctrl-shift-l");
        assert!(
            !manager.read_with(cx, |v, _| v.workspace.as_ref().unwrap().inspector_visible),
            "shortcut must change the saved preference"
        );
        // GPUI 0.2.2 retains removed selectors in debug_bounds. Measure the
        // still-rendered terminal to verify that closing releases the space.
        assert!(cx.debug_bounds("terminal-content-slot").unwrap().size.width > terminal_width);
        cx.simulate_keystrokes("ctrl-shift-l");
        assert_eq!(
            cx.debug_bounds("terminal-content-slot").unwrap().size.width,
            terminal_width
        );
        cx.simulate_resize(gpui::size(px(680.0), px(600.0)));
        assert!(cx.debug_bounds("terminal-content-slot").unwrap().right() > px(675.0));
        assert!(manager.read_with(cx, |v, _| v.workspace.as_ref().unwrap().inspector_visible));
        cx.simulate_resize(gpui::size(px(1440.0), px(900.0)));
        assert_eq!(
            cx.debug_bounds("project-checklist").unwrap().size.width,
            px(chrome::CHECKLIST_WIDTH)
        );
    }

    #[gpui::test]
    fn selectors_keep_their_rendered_geometry_across_themes_and_open_states(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            crate::fonts::initialize(cx);
            super::super::file_editor::FileEditor::initialize(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            view.appearance_saving = true;
            view.workspace.as_mut().unwrap().projects[0].path =
                "a-project-with-a-name-that-must-truncate-in-the-toolbar".into();
            view
        });
        let original = theme::appearance();
        for width in [800.0, 960.0, 1280.0] {
            cx.simulate_resize(gpui::size(px(width), px(540.0)));
            for sidebar_visible in [true, false] {
                view.update(cx, |view, cx| {
                    view.workspace.as_mut().unwrap().sidebar_visible = sidebar_visible;
                    cx.notify();
                });
                cx.refresh().unwrap();
                cx.run_until_parked();
                let picker = cx.debug_bounds("appearance-picker").unwrap();
                let project = cx.debug_bounds("project-selector").unwrap();
                let action = cx.debug_bounds("new-terminal").unwrap();
                let label = cx.debug_bounds("project-selector-label").unwrap();
                assert!(label.left() > project.left() + px(chrome::CONTROL_INSET));
                assert!(label.right() < project.right() - px(chrome::CONTROL_INSET));
                assert!(project.right() <= picker.left());
                assert!(picker.right() <= action.left());
                assert!(action.right() <= px(width));
                for appearance in theme::Appearance::ALL {
                    cx.update(|window, cx| {
                        view.update(cx, |view, cx| {
                            view.choose_appearance(appearance, window, cx)
                        });
                    });
                    cx.run_until_parked();
                    assert_eq!(cx.debug_bounds("appearance-picker").unwrap(), picker);
                    assert_eq!(cx.debug_bounds("project-selector").unwrap(), project);
                    assert_eq!(cx.debug_bounds("new-terminal").unwrap(), action);
                    cx.simulate_click(picker.center(), gpui::Modifiers::none());
                    assert!(view.read_with(cx, |view, _| view.appearance_menu_open));
                    assert_eq!(cx.debug_bounds("appearance-picker").unwrap(), picker);
                    assert_eq!(cx.debug_bounds("new-terminal").unwrap(), action);
                    let menu = cx.debug_bounds("appearance-menu").unwrap();
                    assert!(menu.top() >= picker.bottom() + px(chrome::MENU_GAP - 1.0));
                    assert!(menu.left() >= px(0.0) && menu.right() <= px(width));
                    assert!(menu.bottom() <= px(540.0));
                    cx.simulate_click(picker.center(), gpui::Modifiers::none());
                    assert!(!view.read_with(cx, |view, _| view.appearance_menu_open));
                }
            }
        }
        cx.update(|_, cx| {
            theme::set_appearance(original);
            super::super::file_editor::FileEditor::apply_appearance(cx);
        });
    }

    #[gpui::test]
    fn git_tab_keeps_the_shared_sidebar_width(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::fonts::initialize(cx);
            super::super::file_editor::FileEditor::initialize(cx);
        });
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut view = fixture(cx);
            view.push_terminal("synthetic-project-one".into(), 1, 0, window, cx);
            view
        });
        cx.simulate_resize(gpui::size(px(1280.0), px(720.0)));
        cx.refresh().unwrap();
        cx.run_until_parked();
        let files_width = view.read_with(cx, |view, _| view.sidebar_width);
        view.update(cx, |view, cx| {
            view.projects[0].sidebar_tab = SidebarTab::Git;
            cx.notify();
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        let git_width = view.read_with(cx, |view, _| view.sidebar_width);
        assert_eq!(files_width, chrome::SIDEBAR_WIDTH);
        assert_eq!(git_width, files_width);
    }

    #[gpui::test]
    fn selector_keyboard_navigation_dismissal_and_mutual_exclusion(cx: &mut gpui::TestAppContext) {
        cx.update(super::super::file_editor::FileEditor::initialize);
        let (view, cx) = cx.add_window_view(|_, cx| fixture(cx));
        let project = cx.debug_bounds("project-selector").unwrap();
        let picker = cx.debug_bounds("appearance-picker").unwrap();
        cx.simulate_click(project.center(), gpui::Modifiers::none());
        assert!(view.read_with(cx, |view, _| view.selector_open));
        let menu = cx.debug_bounds("project-selector-menu").unwrap();
        assert_eq!(menu.left(), project.left());
        assert!(menu.top() >= project.bottom());
        cx.simulate_keystrokes("shift-tab");
        assert_eq!(view.read_with(cx, |view, _| view.selector_selected), 1);
        cx.simulate_keystrokes("tab");
        assert_eq!(view.read_with(cx, |view, _| view.selector_selected), 0);
        cx.simulate_click(picker.center(), gpui::Modifiers::none());
        assert!(view.read_with(cx, |view, _| view.appearance_menu_open
            && !view.selector_open));
        cx.simulate_keystrokes("escape");
        cx.update(|window, cx| {
            view.read_with(cx, |view, _| {
                assert!(!view.appearance_menu_open);
                assert!(view.appearance_trigger_focus.is_focused(window));
            });
        });
        // GPUI's keystroke helper sends only key-down; buttons activate on key-up.
        cx.simulate_keystrokes("enter");
        assert!(!view.read_with(cx, |view, _| view.appearance_menu_open));
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse("enter").unwrap(),
        });
        assert!(view.read_with(cx, |view, _| view.appearance_menu_open));
        cx.simulate_click(gpui::point(px(400.0), px(450.0)), gpui::Modifiers::none());
        assert!(!view.read_with(cx, |view, _| view.appearance_menu_open));
        cx.simulate_click(project.center(), gpui::Modifiers::none());
        cx.simulate_click(project.center(), gpui::Modifiers::none());
        assert!(!view.read_with(cx, |view, _| view.selector_open));
    }

    #[gpui::test]
    fn repeated_appearance_choices_coalesce_while_a_preference_save_is_pending(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(super::super::file_editor::FileEditor::initialize);
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, window, cx| {
                let previous = theme::appearance();
                view.appearance_saving = true;
                view.appearance_menu_open = true;
                view.choose_appearance(theme::Appearance::Graphite, window, cx);
                view.choose_appearance(theme::Appearance::Black, window, cx);
                assert_eq!(theme::appearance(), theme::Appearance::Black);
                assert_eq!(view.appearance_revision, 2);
                assert!(view.appearance_saving);
                assert!(!view.appearance_menu_open);
                assert_eq!(view.workspace.as_ref().unwrap().projects.len(), 1);
                theme::set_appearance(previous);
                super::super::file_editor::FileEditor::apply_appearance(cx);
            })
            .unwrap();
    }

    #[gpui::test]
    fn project_selection_retains_terminal_entities_and_live_layout(cx: &mut gpui::TestAppContext) {
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, window, cx| {
                view.workspace
                    .as_mut()
                    .unwrap()
                    .insert_project("synthetic-project-two".into());
                view.push_terminal("synthetic-project-one".into(), 3, 1, window, cx);
                view.push_terminal("synthetic-project-two".into(), 2, 0, window, cx);
                let first = view.projects[0].terminal.clone();
                let second = view.projects[1].terminal.clone();
                assert_eq!(
                    view.active_terminal().unwrap().entity_id(),
                    second.entity_id()
                );
                assert!(view.workspace.as_mut().unwrap().select_project(0));
                assert_eq!(
                    view.active_terminal().unwrap().entity_id(),
                    first.entity_id()
                );
                assert_eq!(first.read(cx).layout_snapshot(), (3, 1));
                assert!(view.workspace.as_mut().unwrap().select_project(1));
                assert_eq!(
                    view.active_terminal().unwrap().entity_id(),
                    second.entity_id()
                );
                assert_eq!(second.read(cx).layout_snapshot(), (2, 0));
                assert!(!first.read(cx).has_running_sessions(cx));
                assert!(!second.read(cx).has_running_sessions(cx));
            })
            .unwrap();
    }

    #[gpui::test]
    fn update_footer_keeps_routine_actions_in_the_status_menu(cx: &mut gpui::TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| fixture(cx));
        cx.simulate_resize(gpui::size(px(800.0), px(540.0)));
        for state in [
            UpdateState::Current,
            UpdateState::Available("1.2.3".into()),
            UpdateState::Prepared("1.2.3".into()),
            UpdateState::Error("Try again.".into()),
            UpdateState::Unavailable,
        ] {
            view.update(cx, |view, cx| {
                view.update_state = state;
                cx.notify();
            });
            cx.refresh().unwrap();
            assert!(cx.debug_bounds("check-for-updates").is_none());
            assert!(cx.debug_bounds("update-and-restart").is_none());
            assert!(!view.read_with(cx, |view, _| view.update_scheduling));
        }
    }

    #[gpui::test]
    fn scheduling_failure_releases_close_state_and_keeps_prepared_retry(
        cx: &mut gpui::TestAppContext,
    ) {
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, _, _| {
                view.update_scheduling = true;
                view.update_restart_pending = true;
                view.close_after_save = true;
                view.discard_on_close = true;
                view.pending_close = Some(PendingClose::UpdateRestart);
                view.release_update_schedule(
                    UpdateState::Prepared("1.2.3".into()),
                    Some("Scheduler did not start.".into()),
                );
                assert_eq!(view.update_state, UpdateState::Prepared("1.2.3".into()));
                assert!(!view.update_scheduling);
                assert!(!view.update_restart_pending);
                assert!(!view.close_after_save);
                assert!(!view.discard_on_close);
                assert!(view.pending_close.is_none());
                assert!(view.notice.is_some());
            })
            .unwrap();
    }

    #[gpui::test]
    fn update_restart_close_panes_matches_window_scope(cx: &mut gpui::TestAppContext) {
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, _, _cx| {
                assert_eq!(
                    view.close_panes(&PendingClose::UpdateRestart).len(),
                    view.close_panes(&PendingClose::Window).len()
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn update_restart_requires_workspace_and_prepared_package(cx: &mut gpui::TestAppContext) {
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, window, cx| {
                view.workspace = None;
                view.update_state = UpdateState::Prepared("1.2.3".into());
                view.request_update_restart(window, cx);
                assert!(!view.update_restart_pending);
                assert!(!view.prompt_pending);
            })
            .unwrap();
    }

    #[gpui::test]
    fn final_update_scheduling_blocks_native_close_and_workspace_mutation(
        cx: &mut gpui::TestAppContext,
    ) {
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, window, cx| {
                view.update_scheduling = true;
                let before = view.workspace.as_ref().unwrap().sidebar_visible;
                view.toggle_sidebar(window, cx);
                assert_eq!(view.workspace.as_ref().unwrap().sidebar_visible, before);
                assert!(!view.request_close(window, cx));
            })
            .unwrap();
    }

    #[gpui::test]
    fn repeated_close_keeps_an_accepted_discard(cx: &mut gpui::TestAppContext) {
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, window, cx| {
                view.close_after_save = true;
                view.discard_on_close = true;
                assert!(!view.request_close(window, cx));
                assert!(view.discard_on_close);
            })
            .unwrap();
    }

    #[gpui::test]
    fn new_file_changes_cancel_the_final_close(cx: &mut gpui::TestAppContext) {
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, window, cx| {
                view.close_after_save = true;
                view.discard_on_close = true;
                view.on_content_changed(window, cx);
                assert!(!view.close_after_save);
                assert!(!view.discard_on_close);
                assert!(view.notice.is_some());
            })
            .unwrap();
    }

    #[gpui::test]
    fn restore_failure_blocks_save_even_after_layout_changes(cx: &mut gpui::TestAppContext) {
        let window = cx.add_window(|_, cx| fixture(cx));
        window
            .update(cx, |view, window, cx| {
                view.restore_warning = Some("Saved layout could not be restored.".into());
                view.notice = Some("A different folder could not be opened.".into());
                view.workspace
                    .as_mut()
                    .unwrap()
                    .insert_project("synthetic-project-two".into());
                view.persist(window, cx);
                assert!(!view.saving);
                assert!(!view.save_failed);
                assert_ne!(view.revision, view.saved_revision);
                assert!(view.restore_warning.is_some());
                view.start_save(window, cx);
                assert!(!view.saving);
            })
            .unwrap();
    }
}
