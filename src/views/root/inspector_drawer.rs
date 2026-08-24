//! Independent session-details sheet with focus restoration and delayed unmount.

use gpui::{AnimationExt, AnyElement};

use super::inspector::{SessionRailParams, session_rail};
use super::*;
use crate::views::motion;

pub(super) struct SessionInspectorDrawerParams<'a> {
    pub(super) open: bool,
    pub(super) motion_key: u64,
    pub(super) projection: &'a ShellProjection,
    pub(super) conversation: &'a ConversationProjection,
    pub(super) orchestration: &'a OrchestrationProjection,
    pub(super) selected_task_id: Option<&'a str>,
    pub(super) goal_edit_composer: &'a Entity<Composer>,
    pub(super) delivery_focus: DeliveryFocus,
    pub(super) usage_tooltip_hovered: bool,
    pub(super) usage_tooltip_visible: bool,
    pub(super) usage_tooltip_epoch: u64,
    pub(super) inspector_focus: &'a FocusHandle,
}

pub(super) fn session_inspector_drawer(
    params: SessionInspectorDrawerParams<'_>,
    cx: &mut Context<RootView>,
) -> AnyElement {
    let SessionInspectorDrawerParams {
        open,
        motion_key,
        projection,
        conversation,
        orchestration,
        selected_task_id,
        goal_edit_composer,
        delivery_focus,
        usage_tooltip_hovered,
        usage_tooltip_visible,
        usage_tooltip_epoch,
        inspector_focus,
    } = params;
    let (status_label, status_color) = inspector_status(conversation);

    let drawer = div()
        .id("session-inspector-drawer-motion")
        .absolute()
        .top(px(6.0))
        .right(px(6.0))
        .bottom(px(6.0))
        .flex()
        .justify_end()
        .overflow_hidden()
        .child(
            div()
                .id("session-inspector-drawer")
                .track_focus(inspector_focus)
                .when(open, |drawer| drawer.tab_index(0))
                // Escape resolves to AbortRun; capture it so composers hosted
                // in the drawer (e.g. the goal editor) cannot swallow it.
                .capture_action(cx.listener(|view, _: &AbortRun, window, cx| {
                    view.close_inspector(window, cx)
                }))
                .w(px(theme::INSPECT_W))
                .h_full()
                .min_h_0()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(px(theme::RADIUS_LG))
                .border_1()
                .border_color(theme::edge())
                .bg(theme::panel())
                .shadow(theme::dock_shadow())
                .child(
                    div()
                        .h(px(40.0))
                        .px(px(10.0))
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .bg(theme::panel())
                        .border_b_1()
                        .border_color(theme::edge_soft())
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(7.0))
                                .child(
                                    div()
                                        .size(px(6.0))
                                        .rounded_full()
                                        .bg(status_color),
                                )
                                .child(
                                    div()
                                        .font_family(theme::main())
                                        .text_size(theme::text_size(theme::T_UI))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::bone())
                                        .child("Session details"),
                                )
                                .child(
                                    div()
                                        .font_family(theme::mono())
                                        .text_size(theme::text_size(theme::T_TINY))
                                        .text_color(theme::smoke())
                                        .child(status_label),
                                ),
                        )
                        .child(
                            div()
                                .id("close-session-inspector")
                                .when(open, |button| button.tab_index(0))
                                .size(px(theme::CHROME))
                                .rounded(px(theme::RADIUS_SM))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .cursor_pointer()
                                .text_color(theme::smoke())
                                .hover(|button| {
                                    button.bg(theme::panel_hover()).text_color(theme::bone())
                                })
                                .active(|button| button.bg(theme::canvas()))
                                .tooltip(controls::text_tooltip(
                                    "Close session details",
                                    Some("Ctrl+I"),
                                ))
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.close_inspector(window, cx)
                                }))
                                .child(
                                    svg()
                                        .path("icons/close.svg")
                                        .size(px(12.0))
                                        .text_color(theme::smoke()),
                                ),
                        ),
                )
                .child(session_rail(
                    SessionRailParams {
                        projection,
                        conversation,
                        orchestration,
                        selected_task_id,
                        goal_edit_composer,
                        delivery_focus,
                        usage_tooltip_hovered,
                        usage_tooltip_visible,
                        usage_tooltip_epoch,
                    },
                    cx,
                )),
        );

    let drawer = if motion_key == 0 {
        drawer
            .w(px(if open { theme::INSPECT_W } else { 0.0 }))
            .opacity(if open { 1.0 } else { 0.0 })
            .into_any_element()
    } else {
        drawer
            .with_animation(
                ("session-inspector-drawer", motion_key),
                motion::settle(motion::DRAWER_MS),
                move |drawer, delta| {
                    let (from, to) = if open {
                        (0.0, theme::INSPECT_W)
                    } else {
                        (theme::INSPECT_W, 0.0)
                    };
                    let width = from + (to - from) * delta;
                    let opacity = if open { delta } else { 1.0 - delta };
                    let offset = if open {
                        10.0 * (1.0 - delta)
                    } else {
                        10.0 * delta
                    };
                    drawer.w(px(width)).mr(px(-offset)).opacity(opacity)
                },
            )
            .into_any_element()
    };

    div()
        .id("session-inspector-layer")
        .absolute()
        .left_0()
        .right_0()
        .top(px(theme::TITLE_H))
        .bottom_0()
        .when(open, |layer| {
            layer.child(
                div()
                    .id("session-inspector-scrim")
                    .absolute()
                    .size_full()
                    .bg(gpui::rgba(0x0000_0012))
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.close_inspector(window, cx)
                    })),
            )
        })
        .child(drawer)
        .into_any_element()
}

fn inspector_status(conversation: &ConversationProjection) -> (&'static str, gpui::Rgba) {
    match conversation.lifecycle {
        RuntimeLifecycle::Loading => ("Loading", theme::data()),
        RuntimeLifecycle::Ready => ("Ready", theme::live()),
        RuntimeLifecycle::Running => ("Working", theme::working()),
        RuntimeLifecycle::Cancelling => ("Cancelling", theme::data()),
        RuntimeLifecycle::Settled => ("Settled", theme::live()),
        RuntimeLifecycle::Disconnected => ("Disconnected", theme::error()),
        RuntimeLifecycle::Failed => ("Needs attention", theme::error()),
    }
}

impl RootView {
    pub(super) fn session_rail_visible(&self) -> bool {
        self.inspector_open
    }

    pub(super) fn open_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inspector_open {
            return;
        }
        self.inspector_unmount_task.take();
        self.inspector_mounted = true;
        self.inspector_restore_focus = window.focused(cx);
        self.inspector_open = true;
        self.inspector_motion_key = self.inspector_motion_key.wrapping_add(1);
        window.focus(&self.inspector_focus);
        cx.notify();
    }

    pub(super) fn close_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.inspector_open {
            return;
        }
        self.inspector_open = false;
        self.inspector_motion_key = self.inspector_motion_key.wrapping_add(1);
        if let Some(focus) = self.inspector_restore_focus.take() {
            window.focus(&focus);
        } else {
            window.focus(&self.composer.read(cx).focus_handle(cx));
        }
        self.schedule_inspector_unmount(cx);
        cx.notify();
    }

    fn schedule_inspector_unmount(&mut self, cx: &mut Context<Self>) {
        let motion_key = self.inspector_motion_key;
        self.inspector_unmount_task.take();
        self.inspector_unmount_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(motion::DRAWER_MS))
                .await;
            let _ = view.update(cx, |view, cx| {
                if !view.inspector_open && view.inspector_motion_key == motion_key {
                    view.inspector_mounted = false;
                    view.inspector_unmount_task = None;
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn toggle_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inspector_open {
            self.close_inspector(window, cx);
        } else {
            self.open_inspector(window, cx);
        }
    }

    pub(super) fn on_toggle_inspector(
        &mut self,
        _: &ToggleInspector,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inspector(window, cx);
    }

}
