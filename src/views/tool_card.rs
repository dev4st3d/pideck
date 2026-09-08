mod data;

use gpui::{FontWeight, IntoElement, div, prelude::*, px};
use serde_json::Value;

pub(super) use self::data::{
    ToolPresentation, presentation_for_bash_block, presentation_for_standalone_result,
    presentation_for_tool_call, tail_presentations,
};
use crate::state::runtime::sanitize_untrusted_text;
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStatus {
    Pending,
    Running,
    Success,
    Error,
    Cancelled,
    Cancelling,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardImage {
    pub data: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolPayload {
    pub text: String,
    pub diff: Option<String>,
    pub images: Vec<CardImage>,
    pub details: Option<Value>,
    pub truncated: bool,
    pub truncation_note: Option<String>,
    pub full_output_path: Option<String>,
}

pub(super) fn render_tool_presentation(
    items: &[ToolPresentation],
    elapsed_ms: Option<u128>,
    context_excluded: bool,
    error: Option<&str>,
) -> impl IntoElement {
    let Some(first) = items.first() else {
        return div().into_any_element();
    };
    let rows = items.iter().flat_map(|item| item.rows.iter()).collect::<Vec<_>>();
    let status = group_status(items);
    div()
        .w_full().min_w_0().pl(px(4.0)).pr(px(35.0))
        .flex().flex_col()
        .child(
            div().w_full().h(px(20.0)).flex().items_center().gap(px(12.0))
                .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis()
                    .whitespace_nowrap().font_family(theme::mono())
                    .text_size(theme::text_size(12.0)).font_weight(FontWeight::MEDIUM)
                    .text_color(theme::bone()).child(first.title(items.len())))
                .child(div().flex_shrink_0().flex().items_center().gap(px(32.0))
                    .when_some(elapsed_ms, |row, elapsed| row.child(meta_text(format_elapsed(elapsed))))
                    .when(context_excluded, |row| row.child(meta_text("not in context".to_owned())))
                    .child(div().font_family(theme::mono()).text_size(theme::text_size(11.0))
                        .text_color(status_color(status)).child(status_label(status)))
                    .child(detail_hint())),
        )
        .child(div().w_full().mt(px(6.0)).flex().flex_col()
            .children(rows.iter().enumerate().map(|(index, row)| {
                // Draw connectors as geometry, never as platform-dependent box glyphs.
                let last = index + 1 == rows.len();
                div().w_full().h(px(22.0)).flex().items_start()
                    .child(div().relative().w(px(24.0)).h_full().flex_shrink_0()
                        .child(div().absolute().left(px(8.0)).top_0().w(px(1.0))
                            .h(px(if last { 11.0 } else { 22.0 })).bg(theme::tool_branch()))
                        .child(div().absolute().left(px(8.0)).top(px(10.0))
                            .w(px(8.0)).h(px(1.0)).bg(theme::tool_branch())))
                    .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap()
                        .font_family(theme::mono()).text_size(theme::text_size(12.0))
                        .line_height(px(20.0)).text_color(theme::ash()).child(row.label.clone()))
                    .when_some(row.detail.clone(), |line, detail| {
                        line.child(div().ml(px(8.0)).flex_shrink_0().font_family(theme::mono())
                            .text_size(theme::text_size(11.0)).line_height(px(20.0))
                            .text_color(theme::smoke()).child(format!("· {detail}")))
                    })
            })))
        .when_some(error.map(str::to_owned), |card, error| {
            card.child(div().font_family(theme::sans()).text_size(theme::text_size(12.0))
                .text_color(theme::error()).child(sanitize_untrusted_text(&error)))
        })
        .into_any_element()
}

fn group_status(items: &[ToolPresentation]) -> CardStatus {
    let mut status = CardStatus::Success;
    for item in items {
        status = worse_status(status, item.status);
    }
    status
}

fn worse_status(a: CardStatus, b: CardStatus) -> CardStatus {
    use CardStatus::*;
    let rank = |status: CardStatus| match status {
        Error => 6,
        Uncertain => 5,
        Cancelled => 4,
        Cancelling => 3,
        Running => 2,
        Pending => 1,
        Success => 0,
    };
    if rank(b) > rank(a) { b } else { a }
}

fn meta_text(text: String) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .whitespace_nowrap()
        .font_family(theme::mono())
        .text_size(theme::text_size(theme::T_TINY))
        .text_color(theme::smoke())
        .child(text)
}

/// The interactive Details hit target is installed by the conversation view.
fn detail_hint() -> impl IntoElement {
    div().w(px(66.0)).flex_shrink_0().flex().items_center().justify_between()
        .font_family(theme::sans()).text_size(theme::text_size(11.0))
        .text_color(theme::ash()).child("Details")
        .child(gpui::svg().path("icons/external.svg").size(px(16.0)).text_color(theme::ash()))
}

fn status_label(status: CardStatus) -> &'static str {
    match status {
        CardStatus::Pending => "pending",
        CardStatus::Running => "running",
        CardStatus::Success => "done",
        CardStatus::Error => "error",
        CardStatus::Cancelled => "cancelled",
        CardStatus::Cancelling => "cancelling",
        CardStatus::Uncertain => "unknown",
    }
}

pub(super) fn status_color(status: CardStatus) -> gpui::Rgba {
    match status {
        CardStatus::Success => theme::live(),
        CardStatus::Error => theme::error(),
        CardStatus::Cancelled | CardStatus::Uncertain => theme::signal(),
        CardStatus::Pending | CardStatus::Running | CardStatus::Cancelling => theme::working(),
    }
}

fn format_elapsed(elapsed_ms: u128) -> String {
    if elapsed_ms < 1_000 {
        format!("{elapsed_ms} ms")
    } else {
        format!("{:.1} s", elapsed_ms as f64 / 1_000.0)
    }
}
