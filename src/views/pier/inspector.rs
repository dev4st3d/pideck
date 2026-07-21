//! Context, task, subagent, and queue presentation.

use gpui::{
    FontWeight, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px, relative, rgba,
};

use crate::{
    state::{QueueItem, SubagentItem, SubagentKind, TaskItem, TaskStatus},
    theme,
};

use super::{label, status_color};

fn task_color(status: TaskStatus) -> gpui::Rgba {
    match status {
        TaskStatus::Pending => theme::ash(),
        TaskStatus::Running => theme::live(),
        TaskStatus::Blocked => theme::signal(),
        TaskStatus::Done => theme::smoke(),
    }
}

pub(in crate::views) fn context_block(
    pct: f32,
    context_label: &'static str,
    tokens_in: &'static str,
    tokens_out: &'static str,
    cache: &'static str,
) -> impl IntoElement {
    let pct = pct.clamp(0.0, 1.0);
    let fill_color = if pct > 0.85 {
        theme::signal()
    } else {
        theme::live()
    };

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .px(px(2.0))
        .pb(px(4.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .child(label("Context"))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::bone_dim())
                        .child(format!("{:.0}% · {context_label}", pct * 100.0)),
                ),
        )
        .child(
            div()
                .h(px(4.0))
                .w_full()
                .rounded(px(2.0))
                .bg(theme::canvas())
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(pct))
                        .when(pct > 0.0, |fill| fill.min_w(px(2.0)))
                        .bg(fill_color),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .child(stat("in", tokens_in))
                .child(stat("out", tokens_out))
                .child(stat("cache", cache)),
        )
}

fn stat(k: &'static str, v: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ash())
                .child(k),
        )
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_MONO))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::bone_dim())
                .child(v),
        )
}

pub(in crate::views) fn task_row(task: &TaskItem, selected: bool) -> impl IntoElement {
    let color = task_color(task.status);
    div()
        .px(px(11.0))
        .py(px(10.0))
        .border_l_2()
        .border_color(if selected {
            theme::signal()
        } else {
            rgba(0x0000_0000)
        })
        .bg(if selected {
            theme::panel_lift()
        } else {
            rgba(0x0000_0000)
        })
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI))
                        .font_weight(if selected {
                            FontWeight::BOLD
                        } else {
                            FontWeight::SEMIBOLD
                        })
                        .text_color(theme::bone())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(task.subject),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(task.status.label()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .text_color(theme::ash())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(task.detail),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(task.owner.unwrap_or("unassigned")),
                ),
        )
}

pub(in crate::views) fn agent_row(agent: &SubagentItem) -> impl IntoElement {
    let kind = match agent.kind {
        SubagentKind::Explore => theme::live(),
        SubagentKind::Plan => theme::data(),
        SubagentKind::General => theme::signal(),
    };
    div()
        .px(px(11.0))
        .py(px(10.0))
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(kind)
                        .child(agent.kind.label()),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(status_color(agent.status))
                        .child(agent.status.label()),
                ),
        )
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI))
                .line_height(relative(1.45))
                .text_color(theme::bone_dim())
                .child(agent.brief),
        )
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(theme::ash())
                .child(format!("{} · {} turns", agent.id, agent.turns)),
        )
}

pub(in crate::views) fn queue_row(item: &QueueItem) -> impl IntoElement {
    div()
        .px(px(11.0))
        .py(px(10.0))
        .flex()
        .flex_row()
        .items_start()
        .gap(px(10.0))
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::data())
                .child(item.mode),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI))
                .line_height(relative(1.45))
                .text_color(theme::bone_dim())
                .child(item.preview),
        )
}

pub(in crate::views) fn divider_list() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme::edge_soft())
        .rounded(px(theme::RADIUS_SM))
        .overflow_hidden()
        .bg(theme::canvas())
}
