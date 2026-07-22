mod data;

use gpui::{
    Context, FontWeight, IntoElement, Render, SharedString, Window, div, prelude::*, px, relative,
};
use serde_json::Value;

pub(super) use self::data::{
    ToolPresentation, cards_for_projection, has_tool_call, presentation_for_bash_block,
    presentation_for_standalone_result, presentation_for_tool_call, tail_presentations,
};
use crate::state::runtime::sanitize_untrusted_text;
use crate::theme;

// Shared with data::bounded_preview and its unit tests.
pub(super) const COLLAPSED_PREVIEW_BYTES: usize = 3 * 1024;

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

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCardData {
    pub key: String,
    pub name: String,
    pub status: CardStatus,
    pub arguments: Option<Value>,
    pub payload: ToolPayload,
    pub elapsed_ms: Option<u128>,
    pub context_excluded: bool,
    pub error: Option<String>,
}

pub struct ToolCard {
    data: ToolCardData,
}

impl ToolCard {
    pub fn new(data: ToolCardData) -> Self {
        Self { data }
    }

    pub fn set_data(&mut self, data: ToolCardData, cx: &mut Context<Self>) {
        if self.data == data {
            return;
        }
        self.data = data;
        cx.notify();
    }
}

impl Render for ToolCard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let presentation = ToolPresentation::from_card(&self.data);
        let card_id = SharedString::from(format!("tool-card:{}", self.data.key));
        div()
            .id(card_id)
            .w_full()
            .min_w_0()
            .child(render_tool_presentation(
                std::slice::from_ref(&presentation),
                self.data.elapsed_ms,
                self.data.context_excluded,
                self.data.error.as_deref(),
            ))
    }
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
    let title = first.title(items.len());
    let status = group_status(items);
    let marker = status_color(status);
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
                .items_baseline()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_MONO))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::bone())
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .when_some(elapsed_ms, |row, elapsed| {
                            row.child(meta_text(format_elapsed(elapsed)))
                        })
                        .when(context_excluded, |row| {
                            row.child(meta_text("not in context".to_owned()))
                        })
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .text_color(marker)
                                .child(status_label(status)),
                        ),
                ),
        )
        .children(rows.iter().enumerate().map(|(index, row)| {
            let branch = if index + 1 == rows.len() {
                "└ "
            } else {
                "├ "
            };
            div()
                .w_full()
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
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_MONO_SM))
                        .line_height(relative(1.4))
                        .text_color(theme::ash())
                        .child(format!("{branch}{}", row.label)),
                )
                .when_some(row.detail.clone(), |line, detail| {
                    line.child(
                        div()
                            .flex_shrink_0()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::smoke())
                            .child(format!("- {detail}")),
                    )
                })
        }))
        .when_some(error.map(str::to_owned), |card, error| {
            card.child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_UI_SM))
                    .text_color(theme::error())
                    .child(sanitize_untrusted_text(&error)),
            )
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
        .font_family(theme::MONO)
        .text_size(px(theme::T_TINY))
        .text_color(theme::smoke())
        .child(text)
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
