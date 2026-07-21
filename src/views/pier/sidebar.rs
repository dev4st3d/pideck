//! Session and resource rows for the sidebar.

use gpui::{FontWeight, IntoElement, ParentElement, Styled, div, px, rgba};

use crate::{
    state::{ResourceItem, SessionSummary},
    theme,
};

pub(in crate::views) fn session_row(session: &SessionSummary) -> impl IntoElement {
    let active = session.active;
    div()
        .px(px(14.0))
        .py(px(10.0))
        .border_l_2()
        .border_color(if active {
            theme::signal()
        } else {
            rgba(0x0000_0000)
        })
        .bg(if active {
            theme::panel_lift()
        } else {
            rgba(0x0000_0000)
        })
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI))
                .font_weight(if active {
                    FontWeight::BOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if active {
                    theme::bone()
                } else {
                    theme::bone_dim()
                })
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(session.name),
        )
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(if active {
                    theme::signal()
                } else {
                    theme::ash()
                })
                .child(if active {
                    format!("{} · live", session.project)
                } else {
                    format!("{} · {}", session.project, session.updated)
                }),
        )
}

pub(in crate::views) fn resource_row(item: &ResourceItem) -> impl IntoElement {
    div()
        .px(px(14.0))
        .py(px(9.0))
        .flex()
        .flex_row()
        .items_baseline()
        .justify_between()
        .gap(px(10.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::bone())
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(item.name),
        )
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(theme::ash())
                .child(format!("{} · {}", item.kind, item.scope)),
        )
}
