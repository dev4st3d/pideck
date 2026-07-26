//! Live, turn-grouped conversation presentation and read-only selectable text.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, ClipboardItem, Context, CursorStyle, Entity,
    FocusHandle, Focusable, FontStyle, FontWeight, HighlightStyle, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Render, SharedString, StrikethroughStyle,
    StyledText, TextLayout, TextRun, TextStyle, UnderlineStyle, Window, div, ease_out_quint,
    prelude::*, px, relative, svg,
};

use crate::actions::{TranscriptCopy, TranscriptSelectAll};
use crate::controller::{AcceptedUserInput, ConversationProjection};
use crate::services::rpc::{SessionEpoch, ToolCallId};
use crate::state::runtime::{
    MessageBlock, MessageRole, MessageStopReason, RuntimeLifecycle, RuntimeMessage, SubmissionKind,
    ToolImage,
};
use crate::theme;
use crate::views::markdown::{MarkdownDocument, MarkdownStyle};
use crate::views::tool_card::{
    CardStatus, ToolPresentation, presentation_for_bash_block, presentation_for_standalone_result,
    presentation_for_tool_call, render_tool_presentation, status_color, tail_presentations,
};

mod list;
mod scroll;

pub(super) use list::{ConversationDiffSummary, ConversationListModel};
pub(super) use scroll::ConversationScrollMotion;

const MAX_CACHED_TRANSCRIPT_BLOCKS: usize = 256;

pub(super) struct ActivityDisclosureState {
    epoch: SessionEpoch,
    expanded: HashSet<String>,
}

impl ActivityDisclosureState {
    pub(super) fn new(epoch: SessionEpoch) -> Self {
        Self {
            epoch,
            expanded: HashSet::new(),
        }
    }

    pub(super) fn prepare_epoch(&mut self, epoch: SessionEpoch) {
        if self.epoch != epoch {
            self.epoch = epoch;
            self.expanded.clear();
        }
    }

    fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        cx.notify();
    }
}

struct CachedTranscriptText {
    entity: Entity<TranscriptText>,
    last_used: u64,
}

pub(super) struct TranscriptTextCache {
    epoch: SessionEpoch,
    use_counter: u64,
    entries: HashMap<String, CachedTranscriptText>,
}

impl TranscriptTextCache {
    pub(super) fn new(epoch: SessionEpoch) -> Self {
        Self {
            epoch,
            use_counter: 0,
            entries: HashMap::new(),
        }
    }

    pub(super) fn prepare_epoch(&mut self, epoch: SessionEpoch) {
        if self.epoch != epoch {
            self.epoch = epoch;
            self.use_counter = 0;
            self.entries.clear();
        }
    }

    pub(super) fn entity_for(
        &mut self,
        key: String,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Entity<TranscriptText> {
        self.use_counter = self.use_counter.wrapping_add(1);
        if let Some(cached) = self.entries.get_mut(&key) {
            cached.last_used = self.use_counter;
            let next_hash = source_hash(text);
            if cached.entity.read(cx).source_hash != next_hash {
                cached
                    .entity
                    .update(cx, |text_view, cx| text_view.set_text(text, next_hash, cx));
            }
            return cached.entity.clone();
        }

        let entity = cx.new(|cx| TranscriptText::new(key.clone(), text, cx));
        self.entries.insert(
            key,
            CachedTranscriptText {
                entity: entity.clone(),
                last_used: self.use_counter,
            },
        );
        self.trim(cx);
        entity
    }

    fn trim(&mut self, cx: &App) {
        while self.entries.len() > MAX_CACHED_TRANSCRIPT_BLOCKS {
            let eviction = self
                .entries
                .iter()
                .filter(|(_, cached)| !cached.entity.read(cx).is_selecting)
                .min_by_key(|(_, cached)| cached.last_used)
                .map(|(key, _)| key.clone());
            let Some(eviction) = eviction else {
                break;
            };
            self.entries.remove(&eviction);
        }
    }
}

pub(super) struct TranscriptText {
    id: SharedString,
    source_hash: u64,
    document: MarkdownDocument,
    selection: Range<usize>,
    selection_reversed: bool,
    is_selecting: bool,
    focus_handle: FocusHandle,
    layout: Option<TextLayout>,
}

impl TranscriptText {
    pub(super) fn new(id: String, text: &str, cx: &mut Context<Self>) -> Self {
        let document = MarkdownDocument::parse(text);
        Self {
            id: SharedString::from(id),
            source_hash: source_hash(text),
            document,
            selection: 0..0,
            selection_reversed: false,
            is_selecting: false,
            focus_handle: cx.focus_handle(),
            layout: None,
        }
    }

    fn set_text(&mut self, text: &str, next_hash: u64, cx: &mut Context<Self>) {
        if self.source_hash == next_hash {
            return;
        }

        let document = MarkdownDocument::parse(text);
        self.selection =
            preserved_selection(&self.document.text, &document.text, self.selection.clone());
        if self.selection.is_empty() {
            self.selection_reversed = false;
        }
        self.source_hash = next_hash;
        self.document = document;
        cx.notify();
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

fn source_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

impl Focusable for TranscriptText {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TranscriptText {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let runs = markdown_runs(&self.document, self.selection.clone(), &window.text_style());
        let text = StyledText::new(self.document.text.clone()).with_runs(runs);
        self.layout = Some(text.layout().clone());

        div()
            .id(self.id.clone())
            .track_focus(&self.focus_handle)
            .key_context("TranscriptText")
            .tab_index(0)
            .w_full()
            .overflow_x_scroll()
            .scrollbar_width(px(4.0))
            .cursor(CursorStyle::IBeam)
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
            let markdown = markdown_style_for_range(document, &range);
            let selected = !selection.is_empty()
                && selection.start <= range.start
                && selection.end >= range.end;
            let style = highlight_style(markdown, selected);
            (style != HighlightStyle::default()).then_some((range, style))
        })
        .collect()
}

fn markdown_style_for_range(document: &MarkdownDocument, range: &Range<usize>) -> MarkdownStyle {
    document
        .spans
        .iter()
        .filter(|span| span.range.start <= range.start && span.range.end >= range.end)
        .fold(MarkdownStyle::default(), |mut combined, span| {
            combined.heading |= span.style.heading;
            if span.style.heading_level > 0
                && (combined.heading_level == 0
                    || span.style.heading_level < combined.heading_level)
            {
                combined.heading_level = span.style.heading_level;
            }
            combined.strong |= span.style.strong;
            combined.emphasis |= span.style.emphasis;
            combined.code |= span.style.code;
            combined.code_block |= span.style.code_block;
            combined.link |= span.style.link;
            combined.quote |= span.style.quote;
            combined.strikethrough |= span.style.strikethrough;
            combined.table |= span.style.table;
            combined.task_marker |= span.style.task_marker;
            combined
        })
}

fn markdown_runs(
    document: &MarkdownDocument,
    selection: Range<usize>,
    default_style: &TextStyle,
) -> Vec<TextRun> {
    let highlights = markdown_highlights(document, selection);
    let mut runs = Vec::new();
    let mut offset = 0;
    for (range, highlight) in highlights {
        if offset < range.start {
            runs.push(default_style.to_run(range.start - offset));
        }
        let markdown = markdown_style_for_range(document, &range);
        let mut style = default_style.clone().highlight(highlight);
        if markdown.table {
            style.font_family = theme::mono();
            style.font_size = theme::text_size(theme::T_UI_SM).into();
        } else if markdown.code {
            style.font_family = theme::mono();
            style.font_size = theme::text_size(if markdown.code_block {
                theme::T_MONO
            } else {
                theme::T_UI_SM
            })
            .into();
        }
        if markdown.heading_level > 0 {
            style.font_size = theme::text_size(match markdown.heading_level {
                1 => theme::T_WORDMARK,
                2 => theme::T_BODY,
                3 => theme::T_BODY_SM,
                _ => theme::T_UI,
            })
            .into();
        }
        runs.push(style.to_run(range.len()));
        offset = range.end;
    }
    if offset < document.text.len() {
        runs.push(default_style.to_run(document.text.len() - offset));
    }
    runs
}

fn highlight_style(markdown: MarkdownStyle, selected: bool) -> HighlightStyle {
    HighlightStyle {
        color: if markdown.task_marker {
            Some(theme::live().into())
        } else if markdown.code_block {
            Some(theme::bone_dim().into())
        } else if markdown.code {
            Some(theme::focus().into())
        } else if markdown.link {
            Some(theme::data().into())
        } else if markdown.heading {
            Some(theme::bone().into())
        } else if markdown.quote {
            Some(theme::ash().into())
        } else if markdown.table {
            Some(theme::bone_dim().into())
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
        } else if markdown.code_block {
            Some(theme::panel().into())
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

pub(super) fn cached_message_texts(
    projection: &ConversationProjection,
    range: Range<usize>,
    cache: &Entity<TranscriptTextCache>,
    cx: &mut App,
) -> HashMap<String, Entity<TranscriptText>> {
    cache.update(cx, |cache, cx| {
        let mut texts = HashMap::new();
        for message in &projection.messages[range] {
            for block in &message.content {
                let Some(text) = transcript_block_text(block) else {
                    continue;
                };
                let key = fragment_key(message, block);
                let entity = cache.entity_for(key.clone(), text, cx);
                texts.insert(key, entity);
            }
        }
        texts
    })
}

pub(super) fn cached_optimistic_texts(
    projection: &ConversationProjection,
    cache: &Entity<TranscriptTextCache>,
    cx: &mut App,
) -> HashMap<String, Entity<TranscriptText>> {
    cache.update(cx, |cache, cx| {
        projection
            .accepted_user_inputs
            .iter()
            .filter(|input| !input.text.is_empty())
            .map(|input| {
                let key = format!("optimistic:{}:text", input.request.as_str());
                let entity = cache.entity_for(key.clone(), &input.text, cx);
                (key, entity)
            })
            .collect()
    })
}

fn transcript_block_text(block: &MessageBlock) -> Option<&str> {
    match block {
        MessageBlock::Text { text, .. }
        | MessageBlock::Summary { text, .. }
        | MessageBlock::Custom { text, .. } => Some(text),
        MessageBlock::Thinking { text, redacted, .. } if !redacted => Some(text),
        MessageBlock::Thinking { .. }
        | MessageBlock::Image { .. }
        | MessageBlock::ToolCall { .. }
        | MessageBlock::ToolResult { .. }
        | MessageBlock::Bash { .. }
        | MessageBlock::Unsupported { .. } => None,
    }
}

struct TurnPosition {
    index: usize,
    is_last: bool,
}

fn turn_card(
    position: TurnPosition,
    user: &RuntimeMessage,
    messages: &[Arc<RuntimeMessage>],
    projection: &ConversationProjection,
    texts: &HashMap<String, Entity<TranscriptText>>,
    disclosures: &Entity<ActivityDisclosureState>,
    cx: &mut App,
) -> impl IntoElement {
    let (activity, reply) = split_turn(messages, projection);
    div()
        .id(SharedString::from(format!("turn-{}", user.key.0)))
        .w_full()
        .flex()
        .flex_col()
        .when(position.index == 1, |turn| {
            turn.rounded_tl(px(theme::RADIUS))
                .rounded_tr(px(theme::RADIUS))
        })
        .when(position.is_last, |turn| {
            turn.rounded_bl(px(theme::RADIUS))
                .rounded_br(px(theme::RADIUS))
        })
        .overflow_hidden()
        .bg(theme::floor())
        .border_1()
        .border_color(theme::edge_soft())
        .child(user_prompt(position.index, user, texts))
        .when(!activity.is_empty(), |turn| {
            turn.child(activity_band(
                &format!("turn:{}", user.key.0),
                &activity,
                reply.is_some(),
                projection,
                texts,
                disclosures,
                cx,
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
        .border_color(theme::user_message_edge())
        .child(
            div()
                .relative()
                .w_full()
                .px(px(18.0))
                .py(px(11.0))
                .bg(theme::user_message())
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(prompt_info(index, optimistic_status(input.kind).to_owned()))
                .when(!input.text.is_empty(), |turn| {
                    turn.child(prompt_selectable(&key, texts))
                })
                .children(
                    input
                        .images
                        .iter()
                        .map(|image| compact_label(format!("Image · {}", image.mime_type))),
                ),
        )
}

fn user_prompt(
    index: usize,
    message: &RuntimeMessage,
    texts: &HashMap<String, Entity<TranscriptText>>,
) -> impl IntoElement {
    div()
        .relative()
        .w_full()
        .px(px(18.0))
        .py(px(11.0))
        .bg(theme::user_message())
        .border_b_1()
        .border_color(theme::user_message_edge())
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(prompt_info(index, format_timestamp(message.timestamp)))
        .children(message.content.iter().filter_map(|block| match block {
            MessageBlock::Text { .. } => {
                Some(prompt_selectable(&fragment_key(message, block), texts))
            }
            MessageBlock::Image { mime_type, .. } => {
                Some(compact_label(format!("Image · {mime_type}")))
            }
            _ => None,
        }))
}

fn prompt_info(index: usize, detail: String) -> impl IntoElement {
    let tooltip = format!("Message {index:02} · You · {detail}");
    div()
        .id(("prompt-info", index))
        .absolute()
        .top(px(10.0))
        .right(px(17.0))
        .size(px(16.0))
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme::smoke())
        .hover(|info| info.text_color(theme::bone_dim()))
        .tooltip(move |_, cx| {
            cx.new(|_| PromptInfoTooltip {
                text: tooltip.clone(),
            })
            .into()
        })
        .child(
            svg()
                .path("icons/info.svg")
                .size(px(13.0))
                .text_color(theme::smoke()),
        )
}

struct PromptInfoTooltip {
    text: String,
}

impl Render for PromptInfoTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(theme::panel_lift())
            .border_1()
            .border_color(theme::edge_hard())
            .font_family(theme::mono())
            .text_size(theme::text_size(theme::T_TINY))
            .text_color(theme::bone_dim())
            .child(self.text.clone())
    }
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

type PersistedToolResult<'a> = (
    &'a str,
    &'a [ToolImage],
    Option<&'a serde_json::Value>,
    bool,
);

fn split_turn<'a, M>(
    messages: &'a [M],
    projection: &ConversationProjection,
) -> (Vec<ActivityStep<'a>>, Option<&'a RuntimeMessage>)
where
    M: Borrow<RuntimeMessage>,
{
    let call_ids = messages
        .iter()
        .flat_map(|message| &message.borrow().content)
        .filter_map(|block| match block {
            MessageBlock::ToolCall { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let persisted_results = messages
        .iter()
        .flat_map(|message| &message.borrow().content)
        .filter_map(|block| match block {
            MessageBlock::ToolResult {
                id,
                content,
                images,
                details,
                is_error,
                ..
            } => Some((
                id.clone(),
                (
                    content.as_str(),
                    images.as_slice(),
                    details.as_ref(),
                    *is_error,
                ),
            )),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let reply_index = messages
        .iter()
        .rposition(|message| is_final_assistant_reply(message.borrow()));

    let mut activity = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let is_reply = reply_index == Some(index);
        push_message_activity(
            &mut activity,
            message.borrow(),
            projection,
            &call_ids,
            &persisted_results,
            is_reply,
        );
    }
    let reply = reply_index.map(|index| messages[index].borrow());
    (activity, reply)
}

fn has_reply_text(message: &RuntimeMessage) -> bool {
    message
        .content
        .iter()
        .any(|block| matches!(block, MessageBlock::Text { text, .. } if !text.trim().is_empty()))
}

fn is_final_assistant_reply(message: &RuntimeMessage) -> bool {
    message.role == MessageRole::Assistant
        && message.terminal
        && message.stop_reason != Some(MessageStopReason::ToolUse)
        && has_reply_text(message)
}

pub(in crate::views) fn latest_completed_response_key(
    projection: &ConversationProjection,
) -> Option<String> {
    projection
        .messages
        .iter()
        .rev()
        .find(|message| is_final_assistant_reply(message))
        .map(|message| message.key.0.clone())
}

fn push_message_activity<'a>(
    activity: &mut Vec<ActivityStep<'a>>,
    message: &'a RuntimeMessage,
    projection: &ConversationProjection,
    call_ids: &HashSet<ToolCallId>,
    persisted_results: &HashMap<ToolCallId, PersistedToolResult<'a>>,
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
                presentation: presentation_for_tool_call(
                    projection,
                    id,
                    name,
                    arguments,
                    persisted_results.get(id).copied(),
                ),
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
                if !call_ids.contains(id) {
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
    disclosure_key: &str,
    steps: &[ActivityStep<'_>],
    has_reply: bool,
    projection: &ConversationProjection,
    texts: &HashMap<String, Entity<TranscriptText>>,
    disclosures: &Entity<ActivityDisclosureState>,
    cx: &mut App,
) -> impl IntoElement {
    let Some((latest_step, history)) = steps.split_last() else {
        return div().into_any_element();
    };
    let mut children = Vec::new();
    let mut index = 0;
    while index < history.len() {
        if let ActivityStep::Tool { presentation } = &history[index]
            && presentation.groupable()
        {
            let mut group = vec![presentation.clone()];
            let mut end = index + 1;
            while end < history.len() {
                let ActivityStep::Tool { presentation: next } = &history[end] else {
                    break;
                };
                if next.name != presentation.name || !next.groupable() {
                    break;
                }
                group.push(next.clone());
                end += 1;
            }
            let is_last = end == history.len();
            children.push(render_tool_group(&group, is_last));
            index = end;
            continue;
        }

        let is_last = index + 1 == history.len();
        children.push(render_activity_step(
            &history[index],
            is_last,
            projection,
            texts,
        ));
        index += 1;
    }

    let expanded = disclosures.read(cx).is_expanded(disclosure_key);
    let animate_latest = !expanded
        && matches!(
            latest_step,
            ActivityStep::Tool { presentation }
                if matches!(
                    presentation.status,
                    CardStatus::Pending | CardStatus::Running | CardStatus::Cancelling
                )
        );
    let latest = render_activity_step(latest_step, true, projection, texts);
    let latest = if animate_latest {
        div()
            .w_full()
            .child(latest)
            .with_animation(
                SharedString::from(format!(
                    "activity-latest-tool:{disclosure_key}:{}",
                    steps.len()
                )),
                Animation::new(Duration::from_millis(170)).with_easing(ease_out_quint()),
                |row, delta| row.ml(px(8.0 * (1.0 - delta))).opacity(0.72 + 0.28 * delta),
            )
            .into_any_element()
    } else {
        latest
    };
    div()
        .w_full()
        .bg(theme::floor())
        .flex()
        .flex_col()
        .when(!history.is_empty(), |band| {
            band.child(activity_disclosure(
                disclosure_key,
                history.len(),
                expanded,
                disclosures,
            ))
        })
        .when(expanded && !history.is_empty(), |band| {
            band.child(
                div()
                    .w_full()
                    .px(px(18.0))
                    .pt(px(9.0))
                    .pb(px(2.0))
                    .flex()
                    .flex_col()
                    .children(children),
            )
        })
        .child(
            div()
                .w_full()
                .px(px(18.0))
                .pt(px(6.0))
                .pb(if has_reply { px(6.0) } else { px(11.0) })
                .child(latest),
        )
        .into_any_element()
}

fn activity_disclosure(
    key: &str,
    history_count: usize,
    expanded: bool,
    disclosures: &Entity<ActivityDisclosureState>,
) -> impl IntoElement {
    let click_key = key.to_owned();
    let click_state = disclosures.clone();
    let keyboard_key = key.to_owned();
    let keyboard_state = disclosures.clone();

    div()
        .w_full()
        .bg(theme::canvas())
        .flex()
        .flex_col()
        .child(
            div()
                .id(SharedString::from(format!("activity-disclosure:{key}")))
                .tab_index(0)
                .cursor_pointer()
                .w_full()
                .min_h(px(34.0))
                .px(px(18.0))
                .py(px(6.0))
                .text_color(theme::ash())
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .hover(|row| row.bg(theme::panel()).text_color(theme::bone_dim()))
                .active(|row| row.bg(theme::panel_lift()))
                .focus(|row| row.bg(theme::panel()).text_color(theme::focus()))
                .on_click(move |_, _, cx| {
                    click_state.update(cx, |state, cx| state.toggle(&click_key, cx));
                })
                .on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        keyboard_state.update(cx, |state, cx| state.toggle(&keyboard_key, cx));
                    }
                })
                .child(
                    div()
                        .w(px(14.0))
                        .h(px(14.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .font_weight(FontWeight::BOLD)
                        .child(if expanded { "−" } else { "+" }),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::smoke())
                        .child(format!(
                            "{history_count:02} step{}",
                            if history_count == 1 { "" } else { "s" }
                        )),
                ),
        )
        .child(
            div().w_full().px(px(18.0)).child(
                div()
                    .w_full()
                    .h(px(1.0))
                    .rounded_full()
                    .bg(theme::edge_hard()),
            ),
        )
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
) -> AnyElement {
    match step {
        ActivityStep::Thinking {
            key: _,
            redacted: true,
            ..
        } => step_shell(
            is_last,
            theme::smoke(),
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::smoke())
                .child("Thinking was redacted by the provider."),
        )
        .into_any_element(),
        ActivityStep::Thinking {
            key,
            redacted: false,
            ..
        } => step_shell(
            is_last,
            theme::smoke(),
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::smoke())
                        .child("thinking"),
                )
                .child(activity_selectable(key, texts)),
        )
        .into_any_element(),
        ActivityStep::Text { key } => {
            step_shell(is_last, theme::ash(), activity_selectable(key, texts)).into_any_element()
        }
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
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::bone_dim())
                        .child(*label),
                )
                .child(activity_selectable(key, texts)),
        )
        .into_any_element(),
        ActivityStep::Custom { kind, key } => step_shell(
            is_last,
            theme::smoke(),
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(compact_label(format!("Extension · {kind}")))
                .child(activity_selectable(key, texts)),
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
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_UI_SM))
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
        .gap(px(9.0))
        .child(
            div()
                .w(px(10.0))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .mt(px(5.0))
                        .w(px(4.0))
                        .h(px(4.0))
                        .rounded_full()
                        .bg(marker)
                        .flex_shrink_0(),
                )
                .when(!is_last, |rail| {
                    rail.child(
                        div()
                            .flex_1()
                            .w(px(1.0))
                            .min_h(px(6.0))
                            .mt(px(2.0))
                            .bg(theme::edge()),
                    )
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pb(if is_last { px(4.0) } else { px(8.0) })
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
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
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
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(metadata),
                ),
        )
        .children(message.content.iter().filter_map(|block| match block {
            MessageBlock::Text { .. } => Some(selectable(
                &fragment_key(message, block),
                texts,
                theme::sans(),
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
                    .font_family(theme::sans())
                    .text_size(theme::text_size(theme::T_UI_SM))
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
    disclosures: &Entity<ActivityDisclosureState>,
    cx: &mut App,
) -> AnyElement {
    match message.role {
        MessageRole::Assistant if is_final_assistant_reply(message) => {
            let messages = [message];
            let (activity, _) = split_turn(&messages, projection);
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
                        &format!("preamble:{}", message.key.0),
                        &activity,
                        true,
                        projection,
                        texts,
                        disclosures,
                        cx,
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
            let messages = [message];
            let (activity, _) = split_turn(&messages, projection);
            if activity.is_empty() {
                return div().into_any_element();
            }
            div()
                .w_full()
                .px(px(2.0))
                .child(activity_band(
                    &format!("preamble:{}", message.key.0),
                    &activity,
                    false,
                    projection,
                    texts,
                    disclosures,
                    cx,
                ))
                .into_any_element()
        }
        MessageRole::User => div().into_any_element(),
    }
}

fn tail_activity(
    projection: &ConversationProjection,
    disclosures: &Entity<ActivityDisclosureState>,
    cx: &mut App,
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
                &format!("tail:{}", projection.epoch.value()),
                &activity,
                false,
                projection,
                &HashMap::new(),
                disclosures,
                cx,
            ))
            .into_any_element(),
    )
}

fn selectable(
    key: &str,
    texts: &HashMap<String, Entity<TranscriptText>>,
    font: gpui::SharedString,
    size: f32,
    color: gpui::Rgba,
    weight: FontWeight,
) -> AnyElement {
    selectable_with_leading(key, texts, font, size, color, weight, 1.58)
}

fn prompt_selectable(key: &str, texts: &HashMap<String, Entity<TranscriptText>>) -> AnyElement {
    div()
        .w_full()
        .pr(px(22.0))
        .child(selectable_with_leading(
            key,
            texts,
            theme::sans(),
            theme::T_BODY_SM,
            theme::bone(),
            FontWeight::MEDIUM,
            1.48,
        ))
        .into_any_element()
}

fn activity_selectable(key: &str, texts: &HashMap<String, Entity<TranscriptText>>) -> AnyElement {
    selectable_with_leading(
        key,
        texts,
        theme::sans(),
        theme::T_UI_SM,
        theme::bone_dim(),
        FontWeight::NORMAL,
        1.42,
    )
}

fn selectable_with_leading(
    key: &str,
    texts: &HashMap<String, Entity<TranscriptText>>,
    font: gpui::SharedString,
    size: f32,
    color: gpui::Rgba,
    weight: FontWeight,
    leading: f32,
) -> AnyElement {
    let Some(text) = texts.get(key) else {
        return div().into_any_element();
    };
    div()
        .w_full()
        .font_family(font)
        .text_size(theme::text_size(size))
        .font_weight(weight)
        .line_height(relative(leading))
        .text_color(color)
        .child(text.clone())
        .into_any_element()
}

fn compact_label(text: String) -> AnyElement {
    div()
        .font_family(theme::mono())
        .text_size(theme::text_size(theme::T_TINY))
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

fn error_text(error: String) -> impl IntoElement {
    div()
        .font_family(theme::sans())
        .text_size(theme::text_size(theme::T_UI_SM))
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
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_TITLE))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::bone_dim())
                .child(title),
        )
        .child(
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_UI))
                .text_color(theme::smoke())
                .child(body),
        )
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
    use crate::state::runtime::{CompactionState, FacetStatus, RetryState};

    #[test]
    fn activity_disclosures_reset_only_when_the_session_changes() {
        let first = SessionEpoch::new(1);
        let second = SessionEpoch::new(2);
        let mut state = ActivityDisclosureState::new(first);
        state.expanded.insert("turn:user-1".to_owned());

        state.prepare_epoch(first);
        assert!(state.is_expanded("turn:user-1"));

        state.prepare_epoch(second);
        assert!(!state.is_expanded("turn:user-1"));
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
                MessageBlock::Text {
                    key: BlockKey("interim".into()),
                    text: "I will inspect that now.".into(),
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
            message_structure_revision: 1,
            lifecycle: RuntimeLifecycle::Settled,
            status: FacetStatus::Ready,
            messages: vec![
                assistant_tools.clone().into(),
                assistant_reply.clone().into(),
            ],
            accepted_user_inputs: Vec::new(),
            tools: Default::default(),
            bash_executions: Arc::new(Vec::new()),
            queue: Arc::new(crate::state::runtime::QueueContents::Known {
                steering: Vec::new(),
                follow_up: Vec::new(),
            }),
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
        assert!(!is_final_assistant_reply(&assistant_tools));
        assert!(is_final_assistant_reply(&assistant_reply));
        let (activity, reply) = split_turn(&projection.messages, &projection);
        assert_eq!(activity.len(), 3);
        assert!(matches!(activity[0], ActivityStep::Thinking { .. }));
        assert!(matches!(activity[1], ActivityStep::Text { .. }));
        assert!(matches!(activity[2], ActivityStep::Tool { .. }));
        assert_eq!(reply.map(|m| m.key.0.as_str()), Some("a2"));
        assert_eq!(
            latest_completed_response_key(&projection).as_deref(),
            Some("a2")
        );
    }
}
