//! Presentation modules for the harness desk regions.

pub(super) mod conversation;
pub(super) mod inspector;
pub(super) mod sidebar;

use gpui::{FontWeight, IntoElement, ParentElement, Styled, div, px};

use crate::{state::RunStatus, theme};

pub(super) fn status_color(status: RunStatus) -> gpui::Rgba {
    match status {
        RunStatus::Idle => theme::ash(),
        RunStatus::Thinking => theme::data(),
        RunStatus::Tooling => theme::live(),
        RunStatus::Waiting => theme::signal(),
        RunStatus::Blocked => theme::signal(),
    }
}

pub(super) fn label(text: &'static str) -> impl IntoElement {
    div()
        .font_family(theme::SANS)
        .text_size(px(theme::T_LABEL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::ash())
        .child(text)
}
