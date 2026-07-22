mod data;

use std::sync::Arc;

use base64::Engine as _;
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, Image, ImageFormat, IntoElement, Render,
    SharedString, StyledImage, Window, div, img, prelude::*, px, relative,
};
use serde_json::Value;

pub(super) use self::data::{
    bash_message_key, cards_for_projection, has_tool_call, standalone_result_key, tail_card_keys,
    tool_key,
};
use self::data::{bounded_preview, format_json, payload_copy_text};
use crate::services::path_actions::{PathAction, activate_untrusted_output_path};
use crate::state::runtime::sanitize_untrusted_text;
use crate::theme;

const COLLAPSED_PREVIEW_BYTES: usize = 24 * 1024;
const EXPANDED_PREVIEW_BYTES: usize = 96 * 1024;
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;

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
    expanded: bool,
    images: Vec<Result<Arc<Image>, String>>,
    action_error: Option<String>,
}

impl ToolCard {
    pub fn new(data: ToolCardData) -> Self {
        let images = decode_images(&data.payload.images);
        Self {
            data,
            expanded: false,
            images,
            action_error: None,
        }
    }

    pub fn set_data(&mut self, data: ToolCardData, cx: &mut Context<Self>) {
        if self.data == data {
            return;
        }
        if self.data.payload.images != data.payload.images {
            self.images = decode_images(&data.payload.images);
        }
        if self.data.payload.full_output_path != data.payload.full_output_path {
            self.action_error = None;
        }
        self.data = data;
        cx.notify();
    }

    fn toggle(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    fn reveal_output(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.activate_output_path(PathAction::Reveal, cx);
    }

    fn open_output_folder(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.activate_output_path(PathAction::OpenFolder, cx);
    }

    fn activate_output_path(&mut self, action: PathAction, cx: &mut Context<Self>) {
        let Some(path) = self.data.payload.full_output_path.as_deref() else {
            return;
        };
        self.action_error = activate_untrusted_output_path(path, action).err();
        cx.notify();
    }

    fn copy_result(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let text = payload_copy_text(&self.data.payload, self.data.error.as_deref());
        cx.write_to_clipboard(ClipboardItem::new_string(sanitize_untrusted_text(&text)));
    }

    fn copy_diff(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(diff) = self.data.payload.diff.as_deref() {
            cx.write_to_clipboard(ClipboardItem::new_string(sanitize_untrusted_text(diff)));
        }
    }
}

impl Render for ToolCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = status_label(self.data.status);
        let marker = status_color(self.data.status);
        let has_args = self
            .data
            .arguments
            .as_ref()
            .is_some_and(|arguments| !arguments.is_null());
        let args = self
            .expanded
            .then(|| self.data.arguments.as_ref().map(format_json))
            .flatten();
        let preview_limit = if self.expanded {
            EXPANDED_PREVIEW_BYTES
        } else {
            COLLAPSED_PREVIEW_BYTES
        };
        let (text_preview, text_omitted) = bounded_preview(&self.data.payload.text, preview_limit);
        let (diff_preview, diff_omitted) = self
            .data
            .payload
            .diff
            .as_deref()
            .map(|diff| bounded_preview(diff, preview_limit))
            .unwrap_or_default();
        let can_expand =
            has_args || self.data.payload.details.is_some() || text_omitted > 0 || diff_omitted > 0;
        let card_id = SharedString::from(format!("tool-card:{}", self.data.key));

        div()
            .id(card_id)
            .w_full()
            .flex()
            .flex_row()
            .gap(px(11.0))
            .py(px(4.0))
            .child(
                div()
                    .w(px(10.0))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .mt(px(7.0))
                            .w(px(5.0))
                            .h(px(5.0))
                            .rounded(px(1.0))
                            .bg(marker),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(7.0))
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
                                    .flex()
                                    .flex_row()
                                    .items_baseline()
                                    .gap(px(9.0))
                                    .child(
                                        div()
                                            .max_w(px(300.0))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .font_family(theme::MONO)
                                            .text_size(px(theme::T_MONO))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme::bone())
                                            .child(sanitize_untrusted_text(&self.data.name)),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme::SANS)
                                            .text_size(px(theme::T_TINY))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(marker)
                                            .child(status),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(10.0))
                                    .when_some(self.data.elapsed_ms, |row, elapsed| {
                                        row.child(meta_text(format_elapsed(elapsed)))
                                    })
                                    .when(self.data.context_excluded, |row| {
                                        row.child(meta_text("not in context".to_owned()))
                                    })
                                    .when(can_expand, |row| {
                                        row.child(text_button(
                                            format!("tool-expand:{}", self.data.key),
                                            if self.expanded { "Collapse" } else { "Details" },
                                            cx.listener(Self::toggle),
                                        ))
                                    }),
                            ),
                    )
                    .when_some(self.data.error.clone(), |card, error| {
                        card.child(
                            div()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_UI_SM))
                                .text_color(theme::error())
                                .child(sanitize_untrusted_text(&error)),
                        )
                    })
                    .when(!text_preview.is_empty(), |card| {
                        card.child(body_surface(
                            plain_lines(&text_preview),
                            Some(copy_action(
                                format!("tool-copy-result:{}", self.data.key),
                                "Copy result",
                                cx.listener(Self::copy_result),
                            )),
                        ))
                    })
                    .when(!diff_preview.is_empty(), |card| {
                        card.child(body_surface(
                            diff_lines(&diff_preview),
                            Some(copy_action(
                                format!("tool-copy-diff:{}", self.data.key),
                                "Copy diff",
                                cx.listener(Self::copy_diff),
                            )),
                        ))
                    })
                    .children(
                        self.images
                            .iter()
                            .enumerate()
                            .map(|(index, image)| image_element(&self.data.key, index, image)),
                    )
                    .when(
                        self.data.payload.truncated || text_omitted > 0 || diff_omitted > 0,
                        |card| {
                            card.child(truncation_row(
                                self.data.payload.truncation_note.as_deref(),
                                text_omitted.saturating_add(diff_omitted),
                            ))
                        },
                    )
                    .when_some(self.data.payload.full_output_path.clone(), |card, path| {
                        card.child(path_actions(
                            &self.data.key,
                            path,
                            cx.listener(Self::reveal_output),
                            cx.listener(Self::open_output_folder),
                        ))
                    })
                    .when_some(self.action_error.clone(), |card, error| {
                        card.child(
                            div()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::error())
                                .child(error),
                        )
                    })
                    .when(self.expanded, |card| {
                        card.when_some(args, |card, arguments| {
                            let copy = arguments.clone();
                            card.child(detail_section(
                                "Arguments",
                                arguments,
                                Some(copy_text_action(
                                    format!("tool-copy-args:{}", self.data.key),
                                    "Copy args",
                                    copy,
                                )),
                            ))
                        })
                        .when_some(
                            self.data.payload.details.as_ref().map(format_json),
                            |card, details| {
                                let copy = details.clone();
                                card.child(detail_section(
                                    "Details",
                                    details,
                                    Some(copy_text_action(
                                        format!("tool-copy-details:{}", self.data.key),
                                        "Copy details",
                                        copy,
                                    )),
                                ))
                            },
                        )
                    }),
            )
    }
}

fn text_button(
    id: String,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(id))
        .tab_index(0)
        .cursor_pointer()
        .min_h(px(28.0))
        .px(px(4.0))
        .flex()
        .items_center()
        .font_family(theme::SANS)
        .text_size(px(theme::T_TINY))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::bone_dim())
        .hover(|button| button.text_color(theme::bone()))
        .active(|button| button.text_color(theme::data()))
        .focus(|button| button.text_color(theme::focus()))
        .on_click(on_click)
        .child(label)
}

fn meta_text(text: String) -> impl IntoElement {
    div()
        .font_family(theme::MONO)
        .text_size(px(theme::T_TINY))
        .text_color(theme::smoke())
        .child(text)
}

fn body_surface(lines: Vec<AnyElement>, copy: Option<AnyElement>) -> impl IntoElement {
    div()
        .w_full()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::canvas())
        .flex()
        .flex_col()
        .gap(px(2.0))
        .when_some(copy, |body, copy| body.child(copy))
        .children(lines)
}

fn copy_action(
    id: String,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .w_full()
        .flex()
        .justify_end()
        .child(text_button(id, label, on_click))
        .into_any_element()
}

fn copy_text_action(id: String, label: &'static str, text: String) -> AnyElement {
    copy_action(id, label, move |_, _, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(sanitize_untrusted_text(&text)));
    })
}

fn plain_lines(text: &str) -> Vec<AnyElement> {
    text.lines()
        .map(|line| {
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_MONO_SM))
                .line_height(relative(1.48))
                .text_color(theme::ash())
                .child(line.to_owned())
                .into_any_element()
        })
        .collect()
}

fn diff_lines(text: &str) -> Vec<AnyElement> {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let (color, weight) = if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
                (theme::live(), FontWeight::MEDIUM)
            } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
                (theme::signal(), FontWeight::MEDIUM)
            } else if trimmed.starts_with("@@") {
                (theme::data(), FontWeight::MEDIUM)
            } else {
                (theme::ash(), FontWeight::NORMAL)
            };
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_MONO_SM))
                .font_weight(weight)
                .line_height(relative(1.48))
                .text_color(color)
                .child(line.to_owned())
                .into_any_element()
        })
        .collect()
}

fn detail_section(label: &'static str, text: String, copy: Option<AnyElement>) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::smoke())
                .child(label),
        )
        .child(body_surface(plain_lines(&text), copy))
}

fn truncation_row(note: Option<&str>, omitted: usize) -> impl IntoElement {
    let text = note.map(sanitize_untrusted_text).unwrap_or_else(|| {
        if omitted > 0 {
            format!("Preview limited for responsiveness; {omitted} bytes are not shown.")
        } else {
            "Pi truncated this result.".to_owned()
        }
    });
    div()
        .font_family(theme::SANS)
        .text_size(px(theme::T_TINY))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::data())
        .child(text)
}

fn path_actions(
    key: &str,
    path: String,
    reveal: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    open_folder: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let safe_path = sanitize_untrusted_text(&path);
    let copy_path = safe_path.clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(theme::smoke())
                .child(format!("Full output: {safe_path}")),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(12.0))
                .child(text_button(
                    format!("tool-copy-path:{key}"),
                    "Copy path",
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone()));
                    },
                ))
                .child(text_button(format!("tool-reveal:{key}"), "Reveal", reveal))
                .child(text_button(
                    format!("tool-open-folder:{key}"),
                    "Open folder",
                    open_folder,
                )),
        )
}

fn image_element(key: &str, index: usize, image: &Result<Arc<Image>, String>) -> AnyElement {
    match image {
        Ok(image) => img(Arc::clone(image))
            .id(SharedString::from(format!("tool-image:{key}:{index}")))
            .max_w(px(640.0))
            .max_h(px(320.0))
            .rounded(px(theme::RADIUS_SM))
            .with_loading(|| {
                div()
                    .text_size(px(theme::T_UI_SM))
                    .text_color(theme::smoke())
                    .child("Loading image…")
                    .into_any_element()
            })
            .with_fallback(|| {
                div()
                    .text_size(px(theme::T_UI_SM))
                    .text_color(theme::error())
                    .child("Image could not be decoded.")
                    .into_any_element()
            })
            .into_any_element(),
        Err(error) => div()
            .font_family(theme::SANS)
            .text_size(px(theme::T_UI_SM))
            .text_color(theme::error())
            .child(error.clone())
            .into_any_element(),
    }
}

fn decode_images(images: &[CardImage]) -> Vec<Result<Arc<Image>, String>> {
    images
        .iter()
        .map(|image| {
            let format = ImageFormat::from_mime_type(&image.mime_type)
                .ok_or_else(|| format!("Unsupported image type: {}", image.mime_type))?;
            if image.data.len() > MAX_IMAGE_BYTES.saturating_mul(4) / 3 + 4 {
                return Err("Image is too large to preview safely.".to_owned());
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&image.data)
                .map_err(|_| "Image data is malformed.".to_owned())?;
            if bytes.len() > MAX_IMAGE_BYTES {
                return Err("Image is too large to preview safely.".to_owned());
            }
            Ok(Arc::new(Image::from_bytes(format, bytes)))
        })
        .collect()
}

fn status_label(status: CardStatus) -> &'static str {
    match status {
        CardStatus::Pending => "pending",
        CardStatus::Running => "running",
        CardStatus::Success => "complete",
        CardStatus::Error => "error",
        CardStatus::Cancelled => "cancelled",
        CardStatus::Cancelling => "cancelling",
        CardStatus::Uncertain => "outcome unknown",
    }
}

fn status_color(status: CardStatus) -> gpui::Rgba {
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
