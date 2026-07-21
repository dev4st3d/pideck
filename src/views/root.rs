//! Interactive harness desk: sidebar, conversation, inspector, and composer.

use gpui::{
    Context, FontWeight, IntoElement, Render, ScrollHandle, SharedString, Task, Window, div,
    prelude::*, px, relative,
};

use crate::state::{DashboardState, SideSection};
use crate::theme;
use crate::views::{controls, motion, pier};

struct UiChrome {
    side_scroll: ScrollHandle,
    conversation_scroll: ScrollHandle,
    inspector_scroll: ScrollHandle,
    side_scroll_generation: u64,
    conversation_scroll_generation: u64,
    _side_scroll_task: Option<Task<()>>,
    _conversation_scroll_task: Option<Task<()>>,
}

impl UiChrome {
    fn new() -> Self {
        Self {
            side_scroll: ScrollHandle::new(),
            conversation_scroll: ScrollHandle::new(),
            inspector_scroll: ScrollHandle::new(),
            side_scroll_generation: 0,
            conversation_scroll_generation: 0,
            _side_scroll_task: None,
            _conversation_scroll_task: None,
        }
    }
}

pub struct RootView {
    state: DashboardState,
    ui: UiChrome,
}

impl RootView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            state: DashboardState::placeholder(),
            ui: UiChrome::new(),
        }
    }

    fn select_side(&mut self, section: SideSection, cx: &mut Context<Self>) {
        if self.state.side_section == section {
            return;
        }

        self.state.select_side(section);
        self.ui.side_scroll_generation = self.ui.side_scroll_generation.wrapping_add(1);
        let generation = self.ui.side_scroll_generation;
        let handle = self.ui.side_scroll.clone();
        self.ui._side_scroll_task = Some(motion::smooth_scroll_y(
            cx,
            handle,
            px(0.0),
            generation,
            |view| view.ui.side_scroll_generation,
        ));
        cx.notify();
    }

    fn on_send(&mut self, cx: &mut Context<Self>) {
        self.state.cycle_run_status();
        self.ui.conversation_scroll_generation =
            self.ui.conversation_scroll_generation.wrapping_add(1);
        let generation = self.ui.conversation_scroll_generation;
        let handle = self.ui.conversation_scroll.clone();
        let target = motion::scroll_bottom_y(&handle);
        self.ui._conversation_scroll_task = Some(motion::smooth_scroll_y(
            cx,
            handle,
            target,
            generation,
            |view| view.ui.conversation_scroll_generation,
        ));
        cx.notify();
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = &self.state;
        let ui = &self.ui;

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .font_family(theme::SANS)
            .text_color(theme::bone())
            .child(titlebar(state, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(sidebar(state, ui, cx))
                    .child(conversation(state, ui))
                    .child(inspector(state, ui, cx)),
            )
            .child(composer(state, cx))
    }
}

// ── Title bar ───────────────────────────────────────────────────────────────

fn titlebar(state: &DashboardState, cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .h(px(theme::TITLE_H))
        .px(px(theme::PAD_X))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .bg(theme::floor())
        .border_b_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .font_family(theme::DISPLAY)
                        .text_size(px(theme::T_WORDMARK))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(theme::bone())
                        .flex_shrink_0()
                        .child("pi"),
                )
                .child(
                    div()
                        .w(px(1.0))
                        .h(px(14.0))
                        .bg(theme::edge_hard())
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TITLE))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::bone())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(state.session_name),
                )
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::smoke())
                        .flex_shrink_0()
                        .child(state.branch_label),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .flex_shrink_0()
                .child(controls::meta_text(state.model.model))
                .child(controls::meta_sep())
                .child(controls::meta_text(state.model.thinking))
                .child(controls::meta_sep())
                .child(controls::meta_text(state.model.cost))
                .child(controls::meta_sep())
                .child(controls::status_button(
                    "run-status",
                    state.run_status.label(),
                    pier::status_color(state.run_status),
                    Box::new(cx.listener(|view, _, _, cx| {
                        view.state.cycle_run_status();
                        cx.notify();
                    })),
                )),
        )
}

// ── Sidebar ─────────────────────────────────────────────────────────────────

fn sidebar(state: &DashboardState, ui: &UiChrome, cx: &mut Context<RootView>) -> impl IntoElement {
    let side_scroll = ui.side_scroll.clone();

    div()
        .w(px(theme::SIDE_W))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme::floor())
        .border_r_1()
        .border_color(theme::edge_hard())
        .child(controls::side_tabs(state.side_section, |section| {
            Box::new(cx.listener(move |view, _, _, cx| {
                view.select_side(section, cx);
            }))
        }))
        .child(
            div()
                .id("sidebar-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&side_scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .children(side_body(state, cx)),
                ),
        )
        .child(
            div()
                .px(px(14.0))
                .py(px(12.0))
                .border_t_1()
                .border_color(theme::edge_soft())
                .child(pier::label("Folder"))
                .child(
                    div()
                        .mt(px(5.0))
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .line_height(relative(1.4))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::data())
                        .child(short_path(state.cwd)),
                ),
        )
}

fn side_body(state: &DashboardState, cx: &mut Context<RootView>) -> Vec<gpui::AnyElement> {
    match state.side_section {
        SideSection::Sessions => state
            .sessions
            .iter()
            .map(|session| {
                let id = session.id;
                controls::interactive_list_row(
                    SharedString::from(format!("session-{id}")),
                    Box::new(cx.listener(move |view, _, _, cx| {
                        view.state.select_session(id);
                        cx.notify();
                    })),
                    pier::sidebar::session_row(session),
                )
                .into_any_element()
            })
            .collect(),
        SideSection::Skills => state
            .skills
            .iter()
            .map(|item| controls::list_row(pier::sidebar::resource_row(item)).into_any_element())
            .collect(),
        SideSection::Extensions => state
            .extensions
            .iter()
            .chain(&state.packages)
            .map(|item| controls::list_row(pier::sidebar::resource_row(item)).into_any_element())
            .collect(),
    }
}

fn short_path(path: &str) -> String {
    let mut parts = path.rsplit(['\\', '/']).filter(|part| !part.is_empty());
    let Some(name) = parts.next() else {
        return path.to_string();
    };
    let Some(parent) = parts.next() else {
        return path.to_string();
    };

    if parts.next().is_none() {
        path.to_string()
    } else {
        format!("…\\{parent}\\{name}")
    }
}

// ── Conversation ────────────────────────────────────────────────────────────

fn conversation(state: &DashboardState, ui: &UiChrome) -> impl IntoElement {
    let scroll = ui.conversation_scroll.clone();
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .w_full()
        .flex()
        .flex_col()
        .bg(theme::canvas())
        .child(
            div()
                .id("conversation-scroll")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .w_full()
                        .px(px(theme::STREAM_PAD_X))
                        .pt(px(16.0))
                        .pb(px(32.0))
                        .child(pier::conversation::stream(&state.stream)),
                ),
        )
}

// ── Inspector ───────────────────────────────────────────────────────────────

fn inspector(
    state: &DashboardState,
    ui: &UiChrome,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let scroll = ui.inspector_scroll.clone();
    div()
        .w(px(theme::INSPECT_W))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme::floor())
        .border_l_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .px(px(16.0))
                .h(px(theme::TITLE_H))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::bone_dim())
                        .child("Inspector"),
                ),
        )
        .child(
            div()
                .id("inspector-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .w_full()
                        .px(px(14.0))
                        .pt(px(14.0))
                        .pb(px(22.0))
                        .flex()
                        .flex_col()
                        .gap(px(20.0))
                        .child(pier::inspector::context_block(
                            state.model.context_used_pct,
                            state.model.context_label,
                            state.model.tokens_in,
                            state.model.tokens_out,
                            state.model.cache,
                        ))
                        .child(section(
                            "Tasks",
                            format!("{} open", state.active_task_count()),
                            pier::inspector::divider_list().children(state.tasks.iter().map(
                                |task| {
                                    let id = task.id;
                                    let selected = state.selected_task_id == Some(id);
                                    controls::interactive_list_row(
                                        SharedString::from(format!("task-{id}")),
                                        Box::new(cx.listener(move |view, _, _, cx| {
                                            view.state.select_task(id);
                                            cx.notify();
                                        })),
                                        pier::inspector::task_row(task, selected),
                                    )
                                },
                            )),
                        ))
                        .child(section(
                            "Subagents",
                            format!("{} live", state.live_subagent_count()),
                            pier::inspector::divider_list().children(
                                state.subagents.iter().enumerate().map(|(i, agent)| {
                                    div()
                                        .when(i + 1 < state.subagents.len(), |el| {
                                            el.border_b_1().border_color(theme::edge_soft())
                                        })
                                        .child(pier::inspector::agent_row(agent))
                                }),
                            ),
                        ))
                        .child(section(
                            "Queue",
                            format!("{}", state.queue.len()),
                            pier::inspector::divider_list().children(
                                state.queue.iter().enumerate().map(|(i, item)| {
                                    div()
                                        .when(i + 1 < state.queue.len(), |el| {
                                            el.border_b_1().border_color(theme::edge_soft())
                                        })
                                        .child(pier::inspector::queue_row(item))
                                }),
                            ),
                        )),
                ),
        )
}

fn section(title: &'static str, count: String, body: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .child(pier::label(title))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::smoke())
                        .child(count),
                ),
        )
        .child(body)
}

// ── Composer ────────────────────────────────────────────────────────────────

fn composer(state: &DashboardState, cx: &mut Context<RootView>) -> impl IntoElement {
    let empty = state.composer.is_empty();
    let text = if empty {
        "Message Pi…  @ file  / command  ! shell".to_string()
    } else {
        state.composer.clone()
    };

    // Align the input with the conversation column.
    let inset_left = theme::SIDE_W + theme::STREAM_PAD_X;
    let inset_right = theme::INSPECT_W + theme::STREAM_PAD_X;

    div()
        .bg(theme::floor())
        .border_t_1()
        .border_color(theme::edge_hard())
        .pl(px(inset_left))
        .pr(px(inset_right))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(controls::composer_field(empty, text))
                .child(controls::primary_button(
                    "send",
                    "Send",
                    Box::new(cx.listener(|view, _, _, cx| view.on_send(cx))),
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(2.0))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::smoke())
                        .child(format!(
                            "{} · Enter steers · Alt+Enter follows",
                            state.model.provider
                        )),
                )
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child("/model  /tree  /compact  /session"),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::short_path;

    #[test]
    fn short_path_preserves_short_values_and_truncates_deep_values() {
        assert_eq!(short_path(r"C:\workspace"), r"C:\workspace");
        assert_eq!(short_path(r"C:\workspace\pi-gui"), r"…\workspace\pi-gui");
        assert_eq!(short_path("/work/pi-gui/src"), r"…\pi-gui\src");
    }
}
