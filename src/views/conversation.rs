//! Live, turn-grouped conversation presentation and read-only selectable text.

use std::collections::HashMap;
use std::ops::Range;

use gpui::{
    AnyElement, ClipboardItem, Context, CursorStyle, Entity, FocusHandle, Focusable, FontStyle,
    FontWeight, HighlightStyle, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Render, SharedString, StrikethroughStyle, StyledText, TextLayout,
    UnderlineStyle, Window, div, prelude::*, px, relative,
};

use crate::actions::{TranscriptCopy, TranscriptSelectAll};
use crate::controller::{AcceptedUserInput, ConversationProjection};
use crate::services::rpc::SessionEpoch;
use crate::state::runtime::{
    CompactionKind, CompactionState, FacetStatus, MessageBlock, MessageRole, MessageStopReason,
    RetryState, RuntimeLifecycle, RuntimeMessage, SubmissionKind,
};
use crate::theme;
use crate::views::markdown::{MarkdownDocument, MarkdownStyle};
use crate::views::tool_card::{
    ToolCard, ToolPresentation, has_tool_call, presentation_for_bash_block,
    presentation_for_standalone_result, presentation_for_tool_call, render_tool_presentation,
    status_color, tail_presentations,
};

const FOLLOW_THRESHOLD: Pixels = px(72.0);

#[derive(Debug, Clone)]
pub(super) struct ScrollPinning {
    epoch: Option<SessionEpoch>,
    revision: u64,
    pinned: bool,
}

impl Default for ScrollPinning {
    fn default() -> Self {
        Self {
            epoch: None,
            revision: 0,
            pinned: true,
        }
    }
}

impl ScrollPinning {
    pub(super) fn should_follow(
        &mut self,
        epoch: SessionEpoch,
        revision: u64,
        offset_y: Pixels,
        max_offset_y: Pixels,
        selection_active: bool,
    ) -> bool {
        if self.epoch != Some(epoch) {
            self.epoch = Some(epoch);
            self.revision = revision;
            self.pinned = !selection_active;
            return self.pinned;
        }

        let near_bottom = (max_offset_y + offset_y).max(Pixels::ZERO) <= FOLLOW_THRESHOLD;
        if revision == self.revision {
            self.pinned = near_bottom && !selection_active;
            return false;
        }

        self.revision = revision;
        let follow = self.pinned && !selection_active;
        if selection_active {
            self.pinned = false;
        }
        follow
    }
}

pub(super) struct TranscriptText {
    id: SharedString,
    source: String,
    document: MarkdownDocument,
    selection: Range<usize>,
    selection_reversed: bool,
    is_selecting: bool,
    focus_handle: FocusHandle,
    layout: Option<TextLayout>,
}

impl TranscriptText {
    pub(super) fn new(id: String, text: String, cx: &mut Context<Self>) -> Self {
        let document = MarkdownDocument::parse(&text);
        Self {
            id: SharedString::from(id),
            source: text,
            document,
            selection: 0..0,
            selection_reversed: false,
            is_selecting: false,
            focus_handle: cx.focus_handle(),
            layout: None,
        }
    }

    pub(super) fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        if self.source == text {
            return;
        }

        let document = MarkdownDocument::parse(&text);
        self.selection =
            preserved_selection(&self.document.text, &document.text, self.selection.clone());
        if self.selection.is_empty() {
            self.selection_reversed = false;
        }
        self.source = text;
        self.document = document;
        cx.notify();
    }

    pub(super) fn selection_active(&self) -> bool {
        self.is_selecting || !self.selection.is_empty()
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let offset = self.index_for_position(event.position);
        if event.modifiers.shift {
            self.select_to(offset);
        } else {
            self.selection = offset..offset;
            self.selection_reversed = false;
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting && event.dragging() {
            let offset = self.index_for_position(event.position);
            self.select_to(offset);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.is_selecting = false;
            cx.notify();
        }
    }

    fn copy(&mut self, _: &TranscriptCopy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.document.text.get(self.selection.clone())
            && !text.is_empty()
        {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
        }
    }

    fn select_all(&mut self, _: &TranscriptSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selection = 0..self.document.text.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize) {
        let anchor = if self.selection_reversed {
            self.selection.end
        } else {
            self.selection.start
        };
        self.selection = anchor.min(offset)..anchor.max(offset);
        self.selection_reversed = offset < anchor;
    }

    fn index_for_position(&self, position: gpui::Point<Pixels>) -> usize {
        let Some(layout) = self.layout.as_ref() else {
            return 0;
        };
        let index = layout
            .index_for_position(position)
            .unwrap_or_else(|nearest| nearest);
        clamp_boundary(&self.document.text, index)
    }
}

impl Focusable for TranscriptText {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TranscriptText {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut text = StyledText::new(self.document.text.clone());
        let highlights = markdown_highlights(&self.document, self.selection.clone());
        if !highlights.is_empty() {
            text = text.with_highlights(highlights);
        }
        self.layout = Some(text.layout().clone());

        div()
            .id(self.id.clone())
            .track_focus(&self.focus_handle)
            .key_context("TranscriptText")
            .tab_index(0)
            .w_full()
            .cursor(CursorStyle::IBeam)
            .focus(|text| text.text_color(theme::focus()))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::select_all))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(text)
    }
}

fn markdown_highlights(
    document: &MarkdownDocument,
    selection: Range<usize>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut boundaries = vec![0, document.text.len()];
    for span in &document.spans {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    if !selection.is_empty() {
        boundaries.push(selection.start);
        boundaries.push(selection.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    boundaries
        .windows(2)
        .filter_map(|boundary| {
            let range = boundary[0]..boundary[1];
            if range.is_empty() {
                return None;
            }
            let markdown = document
                .spans
                .iter()
                .find(|span| span.range.start <= range.start && span.range.end >= range.end)
                .map(|span| span.style)
                .unwrap_or_default();
            let selected = !selection.is_empty()
                && selection.start <= range.start
                && selection.end >= range.end;
            let style = highlight_style(markdown, selected);
            (style != HighlightStyle::default()).then_some((range, style))
        })
        .collect()
}

fn highlight_style(markdown: MarkdownStyle, selected: bool) -> HighlightStyle {
    HighlightStyle {
        color: if markdown.code {
            Some(theme::focus().into())
        } else if markdown.link {
            Some(theme::data().into())
        } else if markdown.quote {
            Some(theme::ash().into())
        } else {
            None
        },
        font_weight: if markdown.heading {
            Some(FontWeight::BOLD)
        } else if markdown.strong {
            Some(FontWeight::SEMIBOLD)
        } else {
            None
        },
        font_style: markdown.emphasis.then_some(FontStyle::Italic),
        background_color: if selected {
            Some(theme::data_wash().into())
        } else if markdown.code {
            Some(theme::panel_lift().into())
        } else {
            None
        },
        underline: markdown.link.then_some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(theme::data().into()),
            ..Default::default()
        }),
        strikethrough: markdown.strikethrough.then_some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(theme::smoke().into()),
        }),
        fade_out: None,
    }
}

fn preserved_selection(old: &str, new: &str, selection: Range<usize>) -> Range<usize> {
    if selection.is_empty() {
        let cursor = clamp_boundary(new, selection.end);
        return cursor..cursor;
    }
    let Some(selected) = old.get(selection.clone()) else {
        let cursor = clamp_boundary(new, selection.start);
        return cursor..cursor;
    };
    if new
        .get(selection.clone())
        .is_some_and(|candidate| candidate == selected)
    {
        return selection;
    }
    if let Some(start) = new.find(selected) {
        return start..start + selected.len();
    }
    let cursor = clamp_boundary(new, selection.start);
    cursor..cursor
}

fn clamp_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset = offset.saturating_sub(1);
    }
    offset
}

pub(super) fn text_fragments(projection: &ConversationProjection) -> Vec<(String, String)> {
    let mut fragments = Vec::new();
    for message in &projection.messages {
        for block in &message.content {
            let text = match block {
                MessageBlock::Text { text, .. }
                | MessageBlock::Summary { text, .. }
                | MessageBlock::Custom { text, .. } => Some(text.clone()),
                MessageBlock::Thinking { text, redacted, .. } => (!redacted).then(|| text.clone()),
                MessageBlock::Image { .. }
                | MessageBlock::ToolCall { .. }
                | MessageBlock::ToolResult { .. }
                | MessageBlock::Bash { .. }
                | MessageBlock::Unsupported { .. } => None,
            };
            if let Some(text) = text {
                fragments.push((fragment_key(message, block), text));
            }
        }
    }
    fragments.extend(projection.accepted_user_inputs.iter().map(|input| {
        (
            format!("optimistic:{}:text", input.request.as_str()),
            input.text.clone(),
        )
    }));
    fragments
}

pub(super) fn stream(
    projection: &ConversationProjection,
    texts: &HashMap<String, Entity<TranscriptText>>,
    tool_cards: &HashMap<String, Entity<ToolCard>>,
) -> impl IntoElement {
    let segments = segment_messages(&projection.messages);
    let turn_count = segments
        .iter()
        .filter(|segment| matches!(segment, Segment::Turn { .. }))
        .count()
        + projection.accepted_user_inputs.len();

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .child(
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .pb(px(10.0))
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::ash())
                        .child("Conversation"),
                )
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(format!(
                            "{turn_count} turn{}",
                            if turn_count == 1 { "" } else { "s" }
                        )),
                ),
        )
        .children(segments.into_iter().map(|segment| match segment {
            Segment::Preamble(message) => preamble(message, projection, texts, tool_cards),
            Segment::Turn {
                index,
                user,
                messages,
            } => {
                turn_card(index, user, &messages, projection, texts, tool_cards).into_any_element()
            }
        }))
        .children(
            projection
                .accepted_user_inputs
                .iter()
                .enumerate()
                .map(|(index, input)| {
                    optimistic_turn(
                        turn_count - projection.accepted_user_inputs.len() + index + 1,
                        input,
                        texts,
                    )
                    .into_any_element()
                }),
        )
        .when_some(tail_activity(projection, tool_cards), |stream, tail| {
            stream.child(tail)
        })
        .children(notices(projection))
        .when(
            projection.messages.is_empty()
                && projection.accepted_user_inputs.is_empty()
                && !matches!(projection.status, FacetStatus::Loading),
            |stream| stream.child(empty_state(projection)),
        )
}

enum Segment<'a> {
    Preamble(&'a RuntimeMessage),
    Turn {
        index: usize,
        user: &'a RuntimeMessage,
        messages: Vec<&'a RuntimeMessage>,
    },
}

fn segment_messages(messages: &[RuntimeMessage]) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let mut index = 0;
    let mut turn = 0;
    while index < messages.len() {
        if messages[index].role != MessageRole::User {
            segments.push(Segment::Preamble(&messages[index]));
            index += 1;
            continue;
        }

        turn += 1;
        let user = &messages[index];
        index += 1;
        let mut body = Vec::new();
        while index < messages.len() && messages[index].role != MessageRole::User {
            body.push(&messages[index]);
            index += 1;
        }
        segments.push(Segment::Turn {
            index: turn,
            user,
            messages: body,
        });
    }
    segments
}

fn turn_card(
    index: usize,
    user: &RuntimeMessage,
    messages: &[&RuntimeMessage],
    projection: &ConversationProjection,
    texts: &HashMap<String, Entity<TranscriptText>>,
    tool_cards: &HashMap<String, Entity<ToolCard>>,
) -> impl IntoElement {
    let (activity, reply) = split_turn(messages, projection);
    div()
        .id(SharedString::from(format!("turn-{}", user.key.0)))
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(theme::RADIUS))
        .overflow_hidden()
        .bg(theme::floor())
        .border_1()
        .border_color(theme::edge_soft())
        .child(user_prompt(index, user, texts))
        .when(!activity.is_empty(), |turn| {
            turn.child(activity_band(
                &activity,
                reply.is_some(),
                projection,
                texts,
                tool_cards,
            ))
        })
        .when_some(reply, |turn, message| {
            turn.child(assistant_reply(message, texts))
        })
}

fn optimistic_turn(
    index: usize,
    input: &AcceptedUserInput,
    texts: &HashMap<String, Entity<TranscriptText>>,
) -> impl IntoElement {
    let key = format!("optimistic:{}:text", input.request.as_str());
    div()
        .id(SharedString::from(format!(
            "optimistic-turn-{}",
            input.request.as_str()
        )))
        .w_full()
        .rounded(px(theme::RADIUS))
        .overflow_hidden()
        .border_1()
        .border_color(theme::edge_soft())
        .child(
            div()
                .w_full()
                .px(px(18.0))
                .py(px(14.0))
                .bg(theme::panel())
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(prompt_header(
                    index,
                    "You",
                    optimistic_status(input.kind).to_owned(),
                    theme::data(),
                ))
                .child(selectable(
                    &key,
                    texts,
                    theme::SANS,
                    theme::T_BODY,
                    theme::bone(),
                    FontWeight::MEDIUM,
                )),
        )
}

fn user_prompt(
    index: usize,
    message: &RuntimeMessage,
    texts: &HashMap<String, Entity<TranscriptText>>,
) -> impl IntoElement {
    div()
        .w_full()
        .px(px(18.0))
        .pt(px(14.0))
        .pb(px(14.0))
        .bg(theme::panel())
        .border_b_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(prompt_header(
            index,
            "You",
            format_timestamp(message.timestamp),
            theme::signal(),
        ))
        .children(message.content.iter().filter_map(|block| match block {
            MessageBlock::Text { .. } => Some(selectable(
                &fragment_key(message, block),
                texts,
                theme::SANS,
                theme::T_BODY,
                theme::bone(),
                FontWeight::MEDIUM,
            )),
            MessageBlock::Image { mime_type, .. } => {
                Some(compact_label(format!("Image · {mime_type}")))
            }
            _ => None,
        }))
}

fn prompt_header(
    index: usize,
    author: &'static str,
    detail: String,
    marker: gpui::Rgba,
) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_baseline()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap(px(8.0))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::BOLD)
                        .text_color(marker)
                        .child(format!("{index:02}")),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::ash())
                        .child(author),
                ),
        )
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(theme::smoke())
                .child(detail),
        )
}

/// One spine step inside a turn's activity band.
enum ActivityStep<'a> {
    Thinking { key: String, redacted: bool },
    Text { key: String },
    Image { mime_type: &'a str },
    Tool { presentation: ToolPresentation },
    Summary { label: &'static str, key: String },
    Custom { kind: &'a str, key: String },
    Unsupported { kind: &'a str },
    Notice { text: String, error: bool },
}

fn split_turn<'a>(
    messages: &[&'a RuntimeMessage],
    projection: &ConversationProjection,
) -> (Vec<ActivityStep<'a>>, Option<&'a RuntimeMessage>) {
    let reply_index = messages.iter().rposition(|message| {
        message.role == MessageRole::Assistant
            && has_reply_text(message)
            && !is_tool_only_assistant(message)
    });

    let mut activity = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let is_reply = reply_index == Some(index);
        push_message_activity(&mut activity, message, projection, is_reply);
    }
    let reply = reply_index.map(|index| messages[index]);
    (activity, reply)
}

fn has_reply_text(message: &RuntimeMessage) -> bool {
    message
        .content
        .iter()
        .any(|block| matches!(block, MessageBlock::Text { text, .. } if !text.trim().is_empty()))
}

fn is_tool_only_assistant(message: &RuntimeMessage) -> bool {
    message.role == MessageRole::Assistant
        && message.stop_reason == Some(MessageStopReason::ToolUse)
        && !message.content.iter().any(
            |block| matches!(block, MessageBlock::Text { text, .. } if !text.trim().is_empty()),
        )
}

fn push_message_activity<'a>(
    activity: &mut Vec<ActivityStep<'a>>,
    message: &'a RuntimeMessage,
    projection: &ConversationProjection,
    is_reply: bool,
) {
    for block in &message.content {
        match block {
            MessageBlock::Text { .. } if is_reply => {}
            MessageBlock::Text { .. } => activity.push(ActivityStep::Text {
                key: fragment_key(message, block),
            }),
            MessageBlock::Thinking { redacted, .. } => activity.push(ActivityStep::Thinking {
                key: fragment_key(message, block),
                redacted: *redacted,
            }),
            MessageBlock::Image { mime_type, .. } => activity.push(ActivityStep::Image {
                mime_type: mime_type.as_str(),
            }),
            MessageBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => activity.push(ActivityStep::Tool {
                presentation: presentation_for_tool_call(projection, id, name, arguments),
            }),
            MessageBlock::ToolResult {
                id,
                name,
                content,
                images,
                details,
                is_error,
                ..
            } => {
                if !has_tool_call(projection, id) {
                    activity.push(ActivityStep::Tool {
                        presentation: presentation_for_standalone_result(
                            name,
                            content,
                            images,
                            details.as_ref(),
                            *is_error,
                        ),
                    });
                }
            }
            MessageBlock::Bash {
                command,
                output,
                cancelled,
                exit_code,
                ..
            } => activity.push(ActivityStep::Tool {
                presentation: presentation_for_bash_block(command, output, *cancelled, *exit_code),
            }),
            MessageBlock::Summary { .. } => activity.push(ActivityStep::Summary {
                label: match message.role {
                    MessageRole::BranchSummary => "Branch summary",
                    MessageRole::CompactionSummary => "Compaction summary",
                    _ => "Summary",
                },
                key: fragment_key(message, block),
            }),
            MessageBlock::Custom { kind, .. } => activity.push(ActivityStep::Custom {
                kind: kind.as_str(),
                key: fragment_key(message, block),
            }),
            MessageBlock::Unsupported { kind, .. } => activity.push(ActivityStep::Unsupported {
                kind: kind.as_str(),
            }),
        }
    }
    if let Some(error) = message.error.as_ref()
        && !is_reply
    {
        activity.push(ActivityStep::Notice {
            text: error.clone(),
            error: true,
        });
    }
}

fn activity_band(
    steps: &[ActivityStep<'_>],
    has_reply: bool,
    projection: &ConversationProjection,
    texts: &HashMap<String, Entity<TranscriptText>>,
    tool_cards: &HashMap<String, Entity<ToolCard>>,
) -> impl IntoElement {
    let mut children = Vec::new();
    let mut index = 0;
    while index < steps.len() {
        if let ActivityStep::Tool { presentation } = &steps[index]
            && presentation.groupable()
        {
            let mut group = vec![presentation.clone()];
            let mut end = index + 1;
            while end < steps.len() {
                let ActivityStep::Tool { presentation: next } = &steps[end] else {
                    break;
                };
                if next.name != presentation.name || !next.groupable() {
                    break;
                }
                group.push(next.clone());
                end += 1;
            }
            let is_last = end == steps.len();
            children.push(render_tool_group(&group, is_last));
            index = end;
            continue;
        }

        let is_last = index + 1 == steps.len();
        children.push(render_activity_step(
            &steps[index],
            is_last,
            projection,
            texts,
            tool_cards,
        ));
        index += 1;
    }

    div()
        .w_full()
        .px(px(18.0))
        .pt(px(12.0))
        .pb(if has_reply { px(4.0) } else { px(14.0) })
        .bg(theme::floor())
        .flex()
        .flex_col()
        .children(children)
}

fn render_tool_group(items: &[ToolPresentation], is_last: bool) -> AnyElement {
    let marker = items
        .iter()
        .map(|item| status_color(item.status))
        .reduce(|left, right| {
            // Prefer error/running over success for the rail marker.
            if left == theme::error() || right == theme::error() {
                theme::error()
            } else if left == theme::data() || right == theme::data() {
                theme::data()
            } else if left == theme::signal() || right == theme::signal() {
                theme::signal()
            } else {
                theme::live()
            }
        })
        .unwrap_or_else(theme::live);
    step_shell(
        is_last,
        marker,
        render_tool_presentation(items, None, false, None),
    )
    .into_any_element()
}

fn render_activity_step(
    step: &ActivityStep<'_>,
    is_last: bool,
    _projection: &ConversationProjection,
    texts: &HashMap<String, Entity<TranscriptText>>,
    _tool_cards: &HashMap<String, Entity<ToolCard>>,
) -> AnyElement {
    match step {
        ActivityStep::Thinking {
            key: _,
            redacted: true,
        } => step_shell(
            is_last,
            theme::smoke(),
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::smoke())
                .child("Thinking was redacted by the provider."),
        )
        .into_any_element(),
        ActivityStep::Thinking {
            key,
            redacted: false,
        } => step_shell(
            is_last,
            theme::smoke(),
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::smoke())
                        .child("thinking"),
                )
                .child(selectable(
                    key,
                    texts,
                    theme::SANS,
                    theme::T_UI,
                    theme::bone_dim(),
                    FontWeight::NORMAL,
                )),
        )
        .into_any_element(),
        ActivityStep::Text { key } => step_shell(
            is_last,
            theme::ash(),
            selectable(
                key,
                texts,
                theme::SANS,
                theme::T_UI,
                theme::bone_dim(),
                FontWeight::NORMAL,
            ),
        )
        .into_any_element(),
        ActivityStep::Image { mime_type } => step_shell(
            is_last,
            theme::ash(),
            compact_label(format!("Image · {mime_type}")),
        )
        .into_any_element(),
        ActivityStep::Tool { presentation } => step_shell(
            is_last,
            status_color(presentation.status),
            render_tool_presentation(std::slice::from_ref(presentation), None, false, None),
        )
        .into_any_element(),
        ActivityStep::Summary { label, key } => step_shell(
            is_last,
            theme::data(),
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::bone_dim())
                        .child(*label),
                )
                .child(selectable(
                    key,
                    texts,
                    theme::SANS,
                    theme::T_UI,
                    theme::bone_dim(),
                    FontWeight::NORMAL,
                )),
        )
        .into_any_element(),
        ActivityStep::Custom { kind, key } => step_shell(
            is_last,
            theme::smoke(),
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(compact_label(format!("Extension · {kind}")))
                .child(selectable(
                    key,
                    texts,
                    theme::SANS,
                    theme::T_UI,
                    theme::bone_dim(),
                    FontWeight::NORMAL,
                )),
        )
        .into_any_element(),
        ActivityStep::Unsupported { kind } => step_shell(
            is_last,
            theme::smoke(),
            compact_label(format!("Unsupported · {kind}")),
        )
        .into_any_element(),
        ActivityStep::Notice { text, error } => step_shell(
            is_last,
            if *error {
                theme::error()
            } else {
                theme::smoke()
            },
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if *error {
                    theme::error()
                } else {
                    theme::smoke()
                })
                .child(text.clone()),
        )
        .into_any_element(),
    }
}

fn step_shell(is_last: bool, marker: gpui::Rgba, body: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .gap(px(11.0))
        .child(
            div()
                .w(px(10.0))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .mt(px(6.0))
                        .w(px(5.0))
                        .h(px(5.0))
                        .rounded_full()
                        .bg(marker)
                        .flex_shrink_0(),
                )
                .when(!is_last, |rail| {
                    rail.child(
                        div()
                            .flex_1()
                            .w(px(1.0))
                            .min_h(px(8.0))
                            .mt(px(3.0))
                            .bg(theme::edge()),
                    )
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pb(if is_last { px(6.0) } else { px(12.0) })
                .child(body),
        )
}

fn assistant_reply(
    message: &RuntimeMessage,
    texts: &HashMap<String, Entity<TranscriptText>>,
) -> impl IntoElement {
    let metadata = assistant_metadata(message);
    div()
        .id(SharedString::from(format!("message-{}", message.key.0)))
        .w_full()
        .px(px(18.0))
        .pt(px(14.0))
        .pb(px(16.0))
        .bg(theme::canvas())
        .border_t_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::ash())
                        .child("Pi"),
                )
                .child(
                    div()
                        .max_w(px(420.0))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(metadata),
                ),
        )
        .children(message.content.iter().filter_map(|block| match block {
            MessageBlock::Text { .. } => Some(selectable(
                &fragment_key(message, block),
                texts,
                theme::SANS,
                theme::T_BODY_SM,
                theme::bone(),
                FontWeight::NORMAL,
            )),
            MessageBlock::Image { mime_type, .. } => {
                Some(compact_label(format!("Image · {mime_type}")))
            }
            _ => None,
        }))
        .when_some(message.error.clone(), |reply, error| {
            reply.child(error_text(error))
        })
        .when_some(stop_label(message), |reply, stop| {
            reply.child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_UI_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(stop_color(message.stop_reason))
                    .child(stop),
            )
        })
}

fn preamble(
    message: &RuntimeMessage,
    projection: &ConversationProjection,
    texts: &HashMap<String, Entity<TranscriptText>>,
    tool_cards: &HashMap<String, Entity<ToolCard>>,
) -> AnyElement {
    match message.role {
        MessageRole::Assistant if has_reply_text(message) && !is_tool_only_assistant(message) => {
            let (activity, _) = split_turn(&[message], projection);
            if activity.is_empty() {
                assistant_reply(message, texts).into_any_element()
            } else {
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .rounded(px(theme::RADIUS))
                    .overflow_hidden()
                    .bg(theme::floor())
                    .border_1()
                    .border_color(theme::edge_soft())
                    .child(activity_band(
                        &activity, true, projection, texts, tool_cards,
                    ))
                    .child(assistant_reply(message, texts))
                    .into_any_element()
            }
        }
        MessageRole::Assistant
        | MessageRole::ToolResult
        | MessageRole::BashExecution
        | MessageRole::Custom
        | MessageRole::BranchSummary
        | MessageRole::CompactionSummary
        | MessageRole::Unknown => {
            let (activity, _) = split_turn(&[message], projection);
            if activity.is_empty() {
                return div().into_any_element();
            }
            div()
                .w_full()
                .px(px(2.0))
                .child(activity_band(
                    &activity, false, projection, texts, tool_cards,
                ))
                .into_any_element()
        }
        MessageRole::User => div().into_any_element(),
    }
}

fn tail_activity(
    projection: &ConversationProjection,
    tool_cards: &HashMap<String, Entity<ToolCard>>,
) -> Option<AnyElement> {
    let activity = tail_presentations(projection)
        .into_iter()
        .map(|presentation| ActivityStep::Tool { presentation })
        .collect::<Vec<_>>();
    if activity.is_empty() {
        return None;
    }

    Some(
        div()
            .w_full()
            .rounded(px(theme::RADIUS))
            .overflow_hidden()
            .bg(theme::floor())
            .border_1()
            .border_color(theme::edge_soft())
            .child(activity_band(
                &activity,
                false,
                projection,
                &HashMap::new(),
                tool_cards,
            ))
            .into_any_element(),
    )
}

fn selectable(
    key: &str,
    texts: &HashMap<String, Entity<TranscriptText>>,
    font: &'static str,
    size: f32,
    color: gpui::Rgba,
    weight: FontWeight,
) -> AnyElement {
    let Some(text) = texts.get(key) else {
        return div().into_any_element();
    };
    div()
        .w_full()
        .font_family(font)
        .text_size(px(size))
        .font_weight(weight)
        .line_height(relative(1.58))
        .text_color(color)
        .child(text.clone())
        .into_any_element()
}

fn compact_label(text: String) -> AnyElement {
    div()
        .font_family(theme::MONO)
        .text_size(px(theme::T_TINY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::smoke())
        .child(text)
        .into_any_element()
}

fn fragment_key(message: &RuntimeMessage, block: &MessageBlock) -> String {
    format!("{}:{}", message.key.0, block.key().0)
}

fn assistant_metadata(message: &RuntimeMessage) -> String {
    let time = format_timestamp(message.timestamp);
    let Some(metadata) = message.assistant.as_ref() else {
        return time;
    };
    let model = metadata
        .response_model
        .as_deref()
        .filter(|model| !model.is_empty())
        .unwrap_or(&metadata.model);
    let usage = &metadata.usage;
    let state = if message.terminal {
        format!(
            "{} in · {} out · {}",
            format_count(usage.input),
            format_count(usage.output),
            format_cost(usage.total_cost)
        )
    } else {
        "streaming".to_owned()
    };
    format!("{}/{} · {state} · {time}", metadata.provider, model)
}

fn stop_label(message: &RuntimeMessage) -> Option<String> {
    if !message.terminal {
        return None;
    }
    match message.stop_reason {
        Some(MessageStopReason::Length) => Some("Stopped at the model length limit.".to_owned()),
        Some(MessageStopReason::Error) => {
            Some("The provider ended this response with an error.".to_owned())
        }
        Some(MessageStopReason::Aborted) => Some("Response aborted.".to_owned()),
        Some(MessageStopReason::ToolUse | MessageStopReason::Stop) | None => None,
    }
}

fn stop_color(reason: Option<MessageStopReason>) -> gpui::Rgba {
    match reason {
        Some(MessageStopReason::Length | MessageStopReason::Error) => theme::error(),
        Some(MessageStopReason::Aborted) => theme::data(),
        Some(MessageStopReason::ToolUse | MessageStopReason::Stop) | None => theme::smoke(),
    }
}

fn notices(projection: &ConversationProjection) -> Vec<AnyElement> {
    let mut notices = Vec::new();
    if matches!(projection.status, FacetStatus::Loading) {
        notices.push(system_notice(
            "conversation-loading",
            "Loading conversation",
            if projection.messages.is_empty() {
                "Reading the current Pi transcript."
            } else {
                "Refreshing the authoritative transcript. Existing messages remain visible."
            },
            false,
        ));
    }

    match &projection.retry {
        RetryState::Waiting {
            attempt,
            max_attempts,
            ..
        } => notices.push(system_notice(
            &format!("retry-{attempt}"),
            "Provider retry",
            &format!(
                "Attempt {attempt} of {max_attempts} starts in {} ms. A timeout does not cancel the run.",
                projection.retry.remaining_ms(std::time::Instant::now()).unwrap_or_default()
            ),
            false,
        )),
        RetryState::Succeeded { attempt } => notices.push(system_notice(
            &format!("retry-succeeded-{attempt}"),
            "Provider retry succeeded",
            "Pi is continuing the run.",
            false,
        )),
        RetryState::Failed { attempt, summary } => notices.push(system_notice(
            &format!("retry-failed-{attempt}"),
            "Provider retry failed",
            summary,
            true,
        )),
        RetryState::Cancelling => notices.push(system_notice(
            "retry-cancelling",
            "Cancelling retry",
            "Waiting for Pi to settle.",
            false,
        )),
        RetryState::Idle => {}
    }

    match &projection.compaction {
        CompactionState::Running { reason } => notices.push(system_notice(
            "compaction-running",
            "Compacting conversation",
            compaction_reason(*reason),
            false,
        )),
        CompactionState::Completed {
            reason,
            summary,
            will_retry,
        } if !has_summary(&projection.messages, summary) => notices.push(system_notice(
            "compaction-completed",
            "Conversation compacted",
            if *will_retry {
                "Context was compacted. Pi is retrying the interrupted request."
            } else {
                compaction_reason(*reason)
            },
            false,
        )),
        CompactionState::Failed { summary, .. } => notices.push(system_notice(
            "compaction-failed",
            "Compaction failed",
            summary,
            true,
        )),
        CompactionState::Aborted { .. } => notices.push(system_notice(
            "compaction-aborted",
            "Compaction aborted",
            "The existing conversation remains available.",
            false,
        )),
        CompactionState::Idle | CompactionState::Completed { .. } => {}
    }

    if projection.context_awaiting_fresh_usage {
        notices.push(system_notice(
            "context-awaiting-usage",
            "Context usage pending",
            "Pi compacted context. Fresh usage will appear after the next provider response.",
            false,
        ));
    }
    if projection.lifecycle == RuntimeLifecycle::Cancelling {
        notices.push(system_notice(
            "run-cancelling",
            "Cancelling response",
            "Partial content remains visible while Pi settles.",
            false,
        ));
    }
    if projection.lifecycle == RuntimeLifecycle::Settled {
        notices.push(system_notice(
            "agent-settled",
            "Run settled",
            "The agent has finished this run. Send the next prompt or compact before continuing.",
            false,
        ));
    }
    if projection.lifecycle == RuntimeLifecycle::Disconnected {
        notices.push(system_notice(
            "transcript-read-only",
            "Transcript is read-only",
            "Reconnect to resync from the last durable entry. Uncertain prompts are never replayed automatically.",
            true,
        ));
    }
    if let Some(error) = projection.error.as_ref() {
        notices.push(system_notice(
            "conversation-error",
            if projection.lifecycle == RuntimeLifecycle::Disconnected {
                "Pi disconnected"
            } else {
                "Conversation unavailable"
            },
            &error.summary,
            true,
        ));
    }
    notices
}

fn system_notice(id: &str, title: &str, body: &str, error: bool) -> AnyElement {
    div()
        .id(SharedString::from(id.to_owned()))
        .w_full()
        .px(px(14.0))
        .py(px(11.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(if error {
            theme::panel()
        } else {
            theme::data_wash()
        })
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if error {
                    theme::error()
                } else {
                    theme::bone_dim()
                })
                .child(title.to_owned()),
        )
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .line_height(relative(1.45))
                .text_color(theme::smoke())
                .child(body.to_owned()),
        )
        .into_any_element()
}

fn error_text(error: String) -> impl IntoElement {
    div()
        .font_family(theme::SANS)
        .text_size(px(theme::T_UI_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::error())
        .child(error)
}

fn empty_state(projection: &ConversationProjection) -> impl IntoElement {
    let (title, body) = if projection.lifecycle == RuntimeLifecycle::Disconnected {
        (
            "No connected conversation",
            "Reconnect Pi to hydrate the current transcript.",
        )
    } else {
        (
            "No messages yet",
            "Send a prompt to begin the conversation.",
        )
    };
    div()
        .w_full()
        .min_h(px(220.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TITLE))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::bone_dim())
                .child(title),
        )
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI))
                .text_color(theme::smoke())
                .child(body),
        )
}

fn has_summary(messages: &[RuntimeMessage], summary: &str) -> bool {
    !summary.is_empty()
        && messages.iter().any(|message| {
            message
                .content
                .iter()
                .any(|block| matches!(block, MessageBlock::Summary { text, .. } if text == summary))
        })
}

fn compaction_reason(reason: CompactionKind) -> &'static str {
    match reason {
        CompactionKind::Manual => "Pi compacted earlier context on request.",
        CompactionKind::Threshold => "Pi compacted earlier context near the context limit.",
        CompactionKind::Overflow => "Pi compacted context after the provider limit was reached.",
    }
}

fn optimistic_status(kind: SubmissionKind) -> &'static str {
    match kind {
        SubmissionKind::Prompt => "Accepted · awaiting transcript",
        SubmissionKind::Steer => "Steering accepted · awaiting delivery",
        SubmissionKind::FollowUp => "Follow-up queued · awaiting delivery",
    }
}

fn format_timestamp(timestamp_ms: u64) -> String {
    let seconds = (timestamp_ms / 1_000) % 86_400;
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    format!("{hours:02}:{minutes:02} UTC")
}

fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted
}

fn format_cost(cost: f64) -> String {
    if cost < 0.01 {
        format!("${cost:.4}")
    } else {
        format!("${cost:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_policy_preserves_manual_scroll_and_selection() {
        let epoch = SessionEpoch::new(1);
        let mut policy = ScrollPinning::default();
        assert!(policy.should_follow(epoch, 1, px(0.0), px(0.0), false));

        assert!(!policy.should_follow(epoch, 1, px(-400.0), px(900.0), false));
        assert!(!policy.should_follow(epoch, 2, px(-400.0), px(900.0), false));

        assert!(!policy.should_follow(epoch, 2, px(-890.0), px(900.0), false));
        assert!(policy.should_follow(epoch, 3, px(-890.0), px(900.0), false));

        assert!(!policy.should_follow(epoch, 4, px(-900.0), px(900.0), true));
        assert!(!policy.should_follow(epoch, 5, px(-900.0), px(900.0), false));
    }

    #[test]
    fn selection_survives_accumulated_stream_replacement() {
        assert_eq!(
            preserved_selection("Hello world", "Hello world, still streaming", 6..11),
            6..11
        );
        assert_eq!(
            preserved_selection("Hello world", "Prefix: Hello world", 6..11),
            14..19
        );
        assert_eq!(
            preserved_selection("Hello world", "Completely replaced", 6..11),
            6..6
        );
    }

    #[test]
    fn session_epoch_replacement_reenables_follow() {
        let mut policy = ScrollPinning::default();
        assert!(policy.should_follow(SessionEpoch::new(1), 1, px(0.0), px(0.0), false));
        policy.should_follow(SessionEpoch::new(1), 1, px(0.0), px(500.0), false);
        assert!(policy.should_follow(SessionEpoch::new(2), 1, px(0.0), px(0.0), false));
    }

    #[test]
    fn split_turn_puts_tools_and_thinking_on_spine_before_reply() {
        use crate::services::rpc::ToolCallId;
        use crate::state::runtime::{
            BlockKey, MessageBlock, MessageKey, MessageRole, RuntimeMessage,
        };

        let assistant_tools = RuntimeMessage {
            key: MessageKey("a1".into()),
            role: MessageRole::Assistant,
            timestamp: 1,
            content: vec![
                MessageBlock::Thinking {
                    key: BlockKey("th".into()),
                    text: "plan".into(),
                    redacted: false,
                },
                MessageBlock::ToolCall {
                    key: BlockKey("tc".into()),
                    id: ToolCallId::new("call-1"),
                    name: "read".into(),
                    arguments: serde_json::json!({}),
                },
            ],
            visible: true,
            terminal: true,
            stop_reason: Some(MessageStopReason::ToolUse),
            error: None,
            assistant: None,
        };
        let assistant_reply = RuntimeMessage {
            key: MessageKey("a2".into()),
            role: MessageRole::Assistant,
            timestamp: 2,
            content: vec![MessageBlock::Text {
                key: BlockKey("a2t".into()),
                text: "done".into(),
            }],
            visible: true,
            terminal: true,
            stop_reason: Some(MessageStopReason::Stop),
            error: None,
            assistant: None,
        };
        let projection = ConversationProjection {
            epoch: SessionEpoch::new(1),
            revision: 1,
            lifecycle: RuntimeLifecycle::Settled,
            status: FacetStatus::Ready,
            messages: vec![assistant_tools.clone(), assistant_reply.clone()],
            accepted_user_inputs: Vec::new(),
            tools: Default::default(),
            bash_executions: Vec::new(),
            queue: crate::state::runtime::QueueContents::Known {
                steering: Vec::new(),
                follow_up: Vec::new(),
            },
            steering_mode: None,
            follow_up_mode: None,
            auto_compaction_enabled: None,
            auto_retry_enabled: None,
            pending_operation: None,
            context_awaiting_fresh_usage: false,
            retry: RetryState::Idle,
            compaction: CompactionState::Idle,
            error: None,
        };
        let messages = vec![&assistant_tools, &assistant_reply];
        let (activity, reply) = split_turn(&messages, &projection);
        assert_eq!(activity.len(), 2);
        assert!(matches!(activity[0], ActivityStep::Thinking { .. }));
        assert!(matches!(activity[1], ActivityStep::Tool { .. }));
        assert_eq!(reply.map(|m| m.key.0.as_str()), Some("a2"));
    }
}
