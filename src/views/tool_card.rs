mod data;

use gpui::{FontWeight, IntoElement, div, prelude::*, px, relative};
use serde_json::Value;

pub(super) use self::data::{
    ToolPresentation, presentation_for_bash_block, presentation_for_standalone_result,
    presentation_for_tool_call, tail_presentations,
};
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

pub(super) fn render_tool_presentation(items: &[ToolPresentation]) -> impl IntoElement {
    let Some(first) = items.first() else {
        return div().into_any_element();
    };
    let title = first.title(items.len());
    let status = group_status(items);
    // Elapsed time rides only on work still in flight; settled durations live
    // in the detail panel's metadata rows. Context exclusion stays visible on
    // every card it applies to.
    let elapsed_ms = items
        .iter()
        .filter_map(|item| item.elapsed_ms)
        .max()
        .filter(|_| matches!(status, CardStatus::Pending | CardStatus::Running));
    let context_excluded = items.iter().any(|item| item.context_excluded);
    let rows = items
        .iter()
        .flat_map(|item| item.rows.iter().cloned())
        .collect::<Vec<_>>();

    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_MONO))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::bone())
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .flex_shrink_0()
                        .when_some(elapsed_ms, |row, elapsed| {
                            row.child(meta_text(format_elapsed(elapsed)))
                        })
                        .when(context_excluded, |row| {
                            row.child(meta_text("not in context".to_owned()))
                        })
                        // Settled successes carry no word: silence is the
                        // success signal, echoed by the caller's neutral dot.
                        .when_some(status_label(status), |row, label| {
                            row.child(
                                div()
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .font_family(theme::mono())
                                    .text_size(theme::text_size(theme::T_TINY))
                                    .text_color(status_color(status))
                                    .child(label),
                            )
                        }),
                ),
        )
        // Detail lines hang under the title; indentation alone carries the
        // hierarchy, so no tree glyphs are drawn.
        .children(rows.into_iter().map(|row| {
            div()
                .w_full()
                .pl(px(theme::PAD_X))
                .flex()
                .flex_row()
                .items_baseline()
                .gap(px(8.0))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_MONO_SM))
                        .line_height(relative(1.4))
                        .text_color(theme::ash())
                        .child(row.label),
                )
                .when_some(row.detail, |line, detail| {
                    line.child(
                        div()
                            .flex_shrink_0()
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_TINY))
                            .text_color(theme::smoke())
                            .child(format!("- {detail}")),
                    )
                })
        }))
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

/// Status words appear only where attention is needed; a settled success
/// stays silent, so a missing word is itself the success signal.
fn status_label(status: CardStatus) -> Option<&'static str> {
    match status {
        CardStatus::Pending => Some("pending"),
        CardStatus::Running => Some("running"),
        CardStatus::Success => None,
        CardStatus::Error => Some("error"),
        CardStatus::Cancelled => Some("cancelled"),
        CardStatus::Cancelling => Some("cancelling"),
        CardStatus::Uncertain => Some("unknown"),
    }
}

pub(super) fn status_color(status: CardStatus) -> gpui::Rgba {
    match status {
        CardStatus::Success => theme::live(),
        CardStatus::Error => theme::error(),
        CardStatus::Cancelled | CardStatus::Uncertain => theme::signal(),
        CardStatus::Pending | CardStatus::Running | CardStatus::Cancelling => theme::data(),
    }
}

fn format_elapsed(elapsed_ms: u128) -> String {
    if elapsed_ms < 1_000 {
        format!("{elapsed_ms} ms")
    } else {
        format!("{:.1} s", elapsed_ms as f64 / 1_000.0)
    }
}
