//! Workspace navigation rail.

use gpui::{AnimationExt, AnyElement};

use super::*;
use crate::views::motion;

/// The workspace rail owns only navigation. Session tooling lives in an
/// independent right-side drawer so opening the inspector never displaces or
/// destroys the user's place in the project tree.
pub(super) fn workspace_rail(
    open: bool,
    motion_key: u64,
    body: impl IntoElement,
) -> AnyElement {
    let expanded_width = theme::SIDE_W;
    let target_width = if open { expanded_width } else { 0.0 };
    let shell = div()
        .id("workspace-rail")
        .h_full()
        .flex_shrink_0()
        .overflow_hidden()
        .bg(theme::floor())
        .child(
            div()
                .id("workspace-rail-body")
                .w(px(expanded_width))
                .h_full()
                .min_h_0()
                .overflow_hidden()
                .bg(theme::floor())
                .border_r_1()
                .border_color(theme::edge_soft())
                .child(body),
        );

    if motion_key == 0 {
        shell.w(px(target_width)).into_any_element()
    } else {
        shell
            .with_animation(
                ("workspace-rail", motion_key),
                motion::settle(motion::SIDEBAR_MS),
                move |panel, delta| {
                    let (from, to) = if open {
                        (0.0, expanded_width)
                    } else {
                        (expanded_width, 0.0)
                    };
                    panel
                        .w(px(from + (to - from) * delta))
                        .opacity(if open { 0.88 + 0.12 * delta } else { 1.0 - 0.12 * delta })
                },
            )
            .into_any_element()
    }
}

impl RootView {
    pub(super) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let opening = !self.sidebar_open;
        self.sidebar_unmount_task.take();
        if opening {
            self.sidebar_mounted = true;
        }
        self.sidebar_open = opening;
        self.sidebar_motion_key = self.sidebar_motion_key.wrapping_add(1);
        if !self.sidebar_open {
            // History sits beside Places; hide it when the rail closes.
            self.history_open = false;
            self.history_confirmation = None;
            self.hovered_thread_key = None;
            // Never leave focus parked on chrome that just became invisible.
            if self.sidebar_tree_focus.is_focused(window) {
                window.focus(&self.focus_handle);
            }
            self.schedule_sidebar_unmount(cx);
        }
        cx.notify();
    }

    pub(super) fn on_toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_sidebar(window, cx);
    }

    pub(super) fn ensure_sidebar_open(&mut self) {
        if self.sidebar_open {
            return;
        }
        self.sidebar_unmount_task.take();
        self.sidebar_mounted = true;
        self.sidebar_open = true;
        self.sidebar_motion_key = self.sidebar_motion_key.wrapping_add(1);
    }

    fn schedule_sidebar_unmount(&mut self, cx: &mut Context<Self>) {
        let motion_key = self.sidebar_motion_key;
        self.sidebar_unmount_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(motion::SIDEBAR_MS))
                .await;
            let _ = view.update(cx, |view, cx| {
                if !view.sidebar_open && view.sidebar_motion_key == motion_key {
                    view.sidebar_mounted = false;
                    view.sidebar_unmount_task = None;
                    cx.notify();
                }
            });
        }));
    }

    /// Painted workspace rows in scroll order; shared with the sidebar render
    /// pass so the keyboard cursor can never address a row that is not visible.
    pub(super) fn sidebar_rows(&self) -> Vec<shell::SidebarRow> {
        let thread_statuses = self.thread_statuses();
        let slices = shell::sidebar_project_slices(
            &self.projects,
            &self.render_projections.catalog,
            &self.project_catalogs,
            &thread_statuses,
        );
        shell::sidebar_rows(&slices)
    }

    /// The tree's single tab stop routes all of its keys here.
    pub(super) fn on_workspace_tree_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Any key dismisses pointer modality; the ring returns to the
        // keyboard-driven focus color.
        self.sidebar_tree_pointer_focus = false;
        match event.keystroke.key.as_str() {
            "down" => self.move_sidebar_cursor(shell::SidebarCursorMove::Next, cx),
            "up" => self.move_sidebar_cursor(shell::SidebarCursorMove::Previous, cx),
            "home" => self.move_sidebar_cursor(shell::SidebarCursorMove::First, cx),
            "end" => self.move_sidebar_cursor(shell::SidebarCursorMove::Last, cx),
            "left" => self.collapse_sidebar_cursor(cx),
            "right" => self.expand_sidebar_cursor(cx),
            "enter" | "space" => self.activate_sidebar_cursor(window, cx),
            "delete" | "backspace" => self.trash_sidebar_cursor(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(super) fn move_sidebar_cursor(&mut self, movement: shell::SidebarCursorMove, cx: &mut Context<Self>) {
        let rows = self.sidebar_rows();
        let Some((node, slot)) =
            shell::sidebar_moved_cursor(&rows, self.sidebar_cursor.as_ref(), movement)
        else {
            return;
        };
        if self.sidebar_cursor.as_ref() != Some(&node) {
            self.sidebar_cursor = Some(node);
            cx.notify();
        }
        self.sessions_scroll.scroll_to_item(slot);
    }

    pub(super) fn expand_sidebar_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(shell::SidebarNode::Project(path)) = self.sidebar_cursor.clone() else {
            return;
        };
        let expanded = self
            .projects
            .projects()
            .iter()
            .find(|project| project_key(&project.path) == project_key(&path))
            .is_some_and(|project| project.expanded);
        if expanded {
            // Already open: step down into the first child (or the next node).
            self.move_sidebar_cursor(shell::SidebarCursorMove::Next, cx);
        } else {
            self.set_project_expanded(path, true, cx);
        }
    }

    pub(super) fn collapse_sidebar_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self.sidebar_cursor.clone() else {
            return;
        };
        match node {
            shell::SidebarNode::Project(path) => {
                let expanded = self
                    .projects
                    .projects()
                    .iter()
                    .find(|project| project_key(&project.path) == project_key(&path))
                    .is_some_and(|project| project.expanded);
                if expanded {
                    self.set_project_expanded(path, false, cx);
                }
            }
            shell::SidebarNode::Thread { project, .. } => {
                self.sidebar_cursor = Some(shell::SidebarNode::Project(project));
                cx.notify();
            }
        }
    }

    pub(super) fn activate_sidebar_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.sidebar_cursor.clone() else {
            return;
        };
        match node {
            shell::SidebarNode::Project(path) => self.activate_project(path, None, window, cx),
            shell::SidebarNode::Thread { project, session } => {
                if self.projects.is_active(&project) {
                    self.switch_session(session, window, cx);
                } else {
                    self.activate_project(project, Some(session), window, cx);
                }
            }
        }
    }

    /// Delete in the tree uses the same guard rails as hover-only trash chrome.
    pub(super) fn trash_sidebar_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(shell::SidebarNode::Thread { project, session }) = self.sidebar_cursor.clone()
        else {
            return;
        };
        if !self.sidebar_thread_deletable(&project, &session) {
            return;
        }
        self.trash_thread(project, session, cx);
    }

    pub(super) fn sidebar_thread_deletable(
        &self,
        project: &std::path::Path,
        session: &std::path::Path,
    ) -> bool {
        if !crate::services::session_catalog::reversible_trash_available() {
            return false;
        }
        let catalog = &self.render_projections.catalog;
        let selected = self.projects.is_active(project)
            && catalog
                .pending_session_file
                .as_ref()
                .or(catalog.current_session_file.as_ref())
                .is_some_and(|path| project_key(path) == project_key(session));
        if selected {
            return false;
        }
        match self.thread_statuses().get(&project_key(session)) {
            Some(status) if status.active => false,
            Some(status) => !matches!(
                status.activity,
                ThreadActivity::Opening
                    | ThreadActivity::Working
                    | ThreadActivity::Cancelling
                    | ThreadActivity::Attention
            ),
            None => true,
        }
    }

}

impl RootView {
    pub(super) fn refresh_sessions(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.refresh_sessions(cx);
        });
        self.refresh_project_catalogs(cx);
    }

    pub(super) fn on_sessions_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.delta.precise() {
            // Precise (touchpad) deltas are applied here instead of falling
            // through to the stock handler: it accumulates fractional
            // offsets, and rows resting between pixel grids re-rasterize
            // with shifted metrics — once the list scrolls, row text and
            // fills visibly change size. Keep every settled offset whole.
            self.sessions_scroll_motion.cancel();
            let distance = event.delta.pixel_delta(px(20.0)).y;
            if distance == px(0.0) {
                return false;
            }
            let before = self.sessions_scroll.offset();
            let max_offset = self.sessions_scroll.max_offset().height;
            let next_y = (before.y + distance)
                .clamp(-max_offset, Pixels::ZERO)
                .round();
            if next_y != before.y {
                self.sessions_scroll.set_offset(point(before.x, next_y));
                cx.notify();
            }
            return true;
        }

        let distance = event.delta.pixel_delta(px(20.0)).y;
        if distance == px(0.0) {
            return false;
        }

        let now = Instant::now();
        if self.sessions_scroll_motion.push(distance, now) {
            self.advance_sessions_scroll(now, cx);
            self.schedule_sessions_scroll_frame(window, cx);
        }
        true
    }

    pub(super) fn advance_sessions_scroll(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(step) = self.sessions_scroll_motion.advance(now) else {
            return;
        };

        let before = self.sessions_scroll.offset();
        let max_offset = self.sessions_scroll.max_offset().height;
        // Whole pixels only: a fractional settle leaves rows between pixel
        // grids, which reads as the list changing size after it scrolls.
        let next_y = (before.y + step).clamp(-max_offset, Pixels::ZERO).round();
        self.sessions_scroll.set_offset(point(before.x, next_y));
        if (f32::from(next_y) - f32::from(before.y)).abs() < 0.01 {
            self.sessions_scroll_motion.cancel();
        }
        cx.notify();
    }

    pub(super) fn schedule_sessions_scroll_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sessions_scroll_motion.schedule_frame() {
            return;
        }
        cx.on_next_frame(window, |view, window, cx| {
            view.sessions_scroll_motion.begin_frame();
            view.advance_sessions_scroll(Instant::now(), cx);
            view.schedule_sessions_scroll_frame(window, cx);
        });
    }

}
