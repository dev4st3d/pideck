//! Live, turn-grouped conversation presentation and read-only selectable text.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, ClipboardItem, Context, CursorStyle, Entity,
    FocusHandle, Focusable, FontWeight, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Render, SharedString, StyledText, TextLayout, Window, div,
    ease_out_quint, prelude::*, pulsating_between, px, relative, svg,
};

use crate::actions::{TranscriptCopy, TranscriptSelectAll};
use crate::attachments::{FileDelivery, PromptFileMetadata, format_bytes};
use crate::controller::{AcceptedUserInput, ConversationProjection};
use crate::services::rpc::{SessionEpoch, ToolCallId};
use crate::state::runtime::{
    MessageBlock, MessageRole, MessageStopReason, RuntimeLifecycle, RuntimeMessage, SubmissionKind,
    ToolImage, sanitize_untrusted_text,
};
use crate::theme;
use crate::views::controls::ClickHandler;
use crate::views::markdown::MarkdownDocument;
use crate::views::markdown::render::{self, LeafInfo, LeafPoint};
use crate::views::tool_card::{
    CardStatus, ToolPresentation, presentation_for_bash_block, presentation_for_standalone_result,
    presentation_for_tool_call, render_tool_presentation, status_color, tail_presentations,
};

mod list;
mod scroll;

pub(super) use list::{ConversationDiffSummary, ConversationListModel, ConversationStreamEntities};
pub(super) use scroll::ConversationScrollMotion;

const MAX_CACHED_TRANSCRIPT_BLOCKS: usize = 256;
/// How long a code card's copy button stays in its acknowledged state.
const COPIED_FEEDBACK_MS: u64 = 1_600;
/// Open/close motion for the activity history disclosure.
const DISCLOSURE_MOTION_MS: u64 = 210;
/// Estimated row height used only while the disclosure is mid-animation.
const DISCLOSURE_STEP_ESTIMATE_PX: f32 = 52.0;
const DISCLOSURE_HISTORY_PAD_PX: f32 = 16.0;
const DISCLOSURE_HISTORY_MAX_PX: f32 = 420.0;

// Thread geometry shared by the prompt, activity, and reply sections of a
// turn. Every section hangs on one rail column so the turn reads as a single
// chain instead of a boxed card stack. Speaker nodes are larger than activity
// beads so the eye reads structure at a glance.
pub(super) const THREAD_RAIL_W: f32 = 12.0;
const NODE_SPEAKER: f32 = 7.0;
const NODE_STEP: f32 = 4.0;
/// Cap-height the node centers sit on inside their rows.
const NODE_ALIGN: f32 = 9.0;
pub(super) const THREAD_GAP: f32 = 12.0;
/// Whitespace between turns; the chain rests between links.
const TURN_GAP: f32 = 32.0;
/// Rhythm between sections inside one turn.
const TURN_SECTION_GAP: f32 = 12.0;

#[derive(Debug, Clone, Copy)]
struct DisclosureMotion {
    generation: u64,
    /// `true` while opening, `false` while closing.
    opening: bool,
}

pub(super) struct ActivityDisclosureState {
    epoch: SessionEpoch,
    expanded: HashSet<String>,
    motions: HashMap<String, DisclosureMotion>,
    next_generation: u64,
}

impl ActivityDisclosureState {
    pub(super) fn new(epoch: SessionEpoch) -> Self {
        Self {
            epoch,
            expanded: HashSet::new(),
            motions: HashMap::new(),
            next_generation: 0,
        }
    }

    pub(super) fn prepare_epoch(&mut self, epoch: SessionEpoch) {
        if self.epoch != epoch {
            self.reset(epoch);
        }
    }

    pub(super) fn reset(&mut self, epoch: SessionEpoch) {
        self.epoch = epoch;
        self.expanded.clear();
        self.motions.clear();
        self.next_generation = 0;
    }

    fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    /// Content stays mounted while collapsing so the exit animation can run.
    fn shows_history(&self, key: &str) -> bool {
        self.expanded.contains(key) || self.motions.get(key).is_some_and(|motion| !motion.opening)
    }

    fn motion(&self, key: &str) -> Option<DisclosureMotion> {
        self.motions.get(key).copied()
    }

    fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        let opening = !self.expanded.contains(key);
        if opening {
            self.expanded.insert(key.to_owned());
        } else {
            self.expanded.remove(key);
        }

        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        self.motions.insert(
            key.to_owned(),
            DisclosureMotion {
                generation,
                opening,
            },
        );

        let key = key.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(DISCLOSURE_MOTION_MS))
                .await;
            let _ = this.update(cx, |state, cx| {
                if state
                    .motions
                    .get(&key)
                    .is_some_and(|motion| motion.generation == generation)
                {
                    state.motions.remove(&key);
                    cx.notify();
                }
            });
        })
        .detach();
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
            self.reset(epoch);
        }
    }

    pub(super) fn reset(&mut self, epoch: SessionEpoch) {
        self.epoch = epoch;
        self.use_counter = 0;
        self.entries.clear();
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

/// Cap on cached stream band models (turns, preambles). Bigger values keep
/// long transcripts warm while scrolling far back at the cost of retaining
/// duplicated tool payloads in the precomputed details.
const MAX_CACHED_STREAM_BANDS: usize = 64;

/// Identity of the inputs a stream band derives from: the message `Arc`s in
/// its slice plus the shared tool/bash snapshots presentations read. The
/// runtime replaces message `Arc`s wholesale on change and writes tool/bash
/// state through `Arc::make_mut` while projections stay alive, so pointer
/// equality implies identical content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BandFingerprint {
    tools: usize,
    bash: usize,
    message_count: usize,
    message_fold: u64,
}

impl BandFingerprint {
    pub(super) fn capture(
        projection: &ConversationProjection,
        messages: &[Arc<RuntimeMessage>],
    ) -> Self {
        let mut fold = 0xcbf2_9ce4_8422_2325u64;
        for message in messages {
            fold ^= Arc::as_ptr(message) as usize as u64;
            fold = fold.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self {
            tools: Arc::as_ptr(&projection.tools) as usize,
            bash: Arc::as_ptr(&projection.bash_executions) as usize,
            message_count: messages.len(),
            message_fold: fold,
        }
    }
}

/// Precomputed render model for one stream band (turn or preamble): shared
/// text entities, grouped history rows, the trailing live step, the
/// collapsed-history summary, and the final assistant reply. Expensive
/// clones (tool payloads, sanitized details) happen once per fingerprint
/// change instead of once per frame per visible band.
pub(super) struct StreamBandModel {
    pub(super) texts: HashMap<String, Entity<TranscriptText>>,
    rows: Vec<BandRow>,
    /// Ungrouped step count behind the disclosure, so the "N earlier steps"
    /// label matches what folding produced before grouping.
    history_count: usize,
    latest: Option<ActivityStep>,
    summary: Option<String>,
    reply: Option<Arc<RuntimeMessage>>,
}

enum BandRow {
    Step(ActivityStep),
    Group(Arc<ToolGroupModel>),
}

struct ToolGroupModel {
    presentations: Vec<ToolPresentation>,
    detail: Arc<ActivityDetail>,
    trigger_id: SharedString,
    marker: gpui::Rgba,
    live: bool,
}

struct StreamBandEntry {
    fingerprint: BandFingerprint,
    model: Arc<StreamBandModel>,
    last_used: u64,
}

/// Fingerprint-keyed, LRU-bounded cache of stream band models. Sits next to
/// `TranscriptTextCache` so an entire streaming session only recomputes the
/// bands whose messages or tool state actually changed.
pub(super) struct StreamBandCache {
    epoch: SessionEpoch,
    use_counter: u64,
    entries: HashMap<String, StreamBandEntry>,
    tail: Option<(BandFingerprint, Arc<StreamBandModel>)>,
}

impl StreamBandCache {
    pub(super) fn new(epoch: SessionEpoch) -> Self {
        Self {
            epoch,
            use_counter: 0,
            entries: HashMap::new(),
            tail: None,
        }
    }

    pub(super) fn prepare_epoch(&mut self, epoch: SessionEpoch) {
        if self.epoch != epoch {
            self.reset(epoch);
        }
    }

    pub(super) fn reset(&mut self, epoch: SessionEpoch) {
        self.epoch = epoch;
        self.use_counter = 0;
        self.entries.clear();
        self.tail = None;
    }

    pub(super) fn model_for(
        &mut self,
        key: String,
        fingerprint: BandFingerprint,
        cx: &mut Context<Self>,
        build: impl FnOnce(&mut Context<Self>) -> StreamBandModel,
    ) -> Arc<StreamBandModel> {
        self.use_counter = self.use_counter.wrapping_add(1);
        if let Some(entry) = self.entries.get_mut(&key)
            && entry.fingerprint == fingerprint
        {
            entry.last_used = self.use_counter;
            return entry.model.clone();
        }

        let model = Arc::new(build(cx));
        self.entries.insert(
            key,
            StreamBandEntry {
                fingerprint,
                model: model.clone(),
                last_used: self.use_counter,
            },
        );
        self.trim();
        model
    }

    pub(super) fn tail_for(
        &mut self,
        fingerprint: BandFingerprint,
        cx: &mut Context<Self>,
        build: impl FnOnce(&mut Context<Self>) -> StreamBandModel,
    ) -> Arc<StreamBandModel> {
        if let Some((cached_fingerprint, model)) = self.tail.as_ref()
            && *cached_fingerprint == fingerprint
        {
            return model.clone();
        }
        let model = Arc::new(build(cx));
        self.tail = Some((fingerprint, model.clone()));
        model
    }

    fn trim(&mut self) {
        while self.entries.len() > MAX_CACHED_STREAM_BANDS {
            let eviction = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
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
    /// Shared snapshot of the sole prose block's text for the fast render path;
    /// cloning it into `StyledText` is an Arc bump instead of a full copy.
    sole_text: Option<SharedString>,
    /// Cached `document.plain_text()` for selection and copy math; recomputed
    /// only when the source text changes, not per pointer event.
    plain: String,
    /// Sole-prose selection, a byte range into `plain`.
    selection: Range<usize>,
    selection_reversed: bool,
    /// Multi-block selection endpoints `(leaf, byte offset)` into the
    /// render-ordered `leaves`; equal endpoints mean no selection.
    anchor: LeafPoint,
    head: LeafPoint,
    /// Text leaves captured by the latest multi-block render.
    leaves: Vec<LeafInfo>,
    /// Code card currently acknowledging a copy, tagged with a generation so
    /// the reset timer does not clear a newer press.
    copied: Option<usize>,
    copy_generation: u64,
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
            sole_text: sole_text_snapshot(&document),
            plain: document.plain_text(),
            document,
            selection: 0..0,
            selection_reversed: false,
            anchor: (0, 0),
            head: (0, 0),
            leaves: Vec::new(),
            copied: None,
            copy_generation: 0,
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
        let sole_text = sole_text_snapshot(&document);
        let plain = document.plain_text();
        if document.is_selectable_prose() && self.document.is_selectable_prose() {
            let old = std::mem::replace(&mut self.plain, plain);
            self.selection = preserved_selection(&old, &self.plain, self.selection.clone());
        } else {
            self.plain = plain;
            self.selection = 0..0;
        }
        if self.selection.is_empty() {
            self.selection_reversed = false;
        }
        // Block selection cannot survive a re-parse; leaf order may shift.
        self.anchor = (0, 0);
        self.head = (0, 0);
        self.copied = None;
        self.source_hash = next_hash;
        self.sole_text = sole_text;
        self.document = document;
        self.leaves.clear();
        self.layout = None;
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
        if self.document.is_selectable_prose() {
            let offset = self.index_for_position(event.position);
            if event.modifiers.shift {
                self.select_to(offset);
            } else {
                self.selection = offset..offset;
                self.selection_reversed = false;
            }
        } else if !self.leaves.is_empty() {
            let point = self.leaf_point_at(event.position);
            if event.modifiers.shift {
                self.head = point;
            } else {
                self.anchor = point;
                self.head = point;
            }
        } else {
            self.is_selecting = false;
            return;
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !(self.is_selecting && event.dragging()) {
            return;
        }
        if self.document.is_selectable_prose() {
            let offset = self.index_for_position(event.position);
            self.select_to(offset);
        } else if !self.leaves.is_empty() {
            self.head = self.leaf_point_at(event.position);
        } else {
            return;
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.is_selecting = false;
            cx.notify();
        }
    }

    fn copy(&mut self, _: &TranscriptCopy, _: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_selectable_prose() {
            if let Some(text) = self.plain.get(self.selection.clone())
                && !text.is_empty()
            {
                cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
            }
            return;
        }
        // Without an active block selection, copy the whole message.
        let text = self
            .selected_leaf_text()
            .unwrap_or_else(|| self.plain.clone());
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn select_all(&mut self, _: &TranscriptSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_selectable_prose() {
            self.selection = 0..self.plain.len();
            self.selection_reversed = false;
            cx.notify();
            return;
        }
        if let Some((last, leaf)) = self.leaves.iter().enumerate().next_back() {
            // Highlight all leaves, and copy the full plain text right away:
            // leaf selections cannot span non-text blocks such as tables.
            self.anchor = (0, 0);
            self.head = (last, leaf.text.len());
        }
        if !self.plain.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.plain.clone()));
        }
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
        clamp_boundary(&self.plain, index)
    }

    /// Nearest selectable leaf for a mouse position. Leaves stack in render
    /// order: `Err(0)` is above a leaf, any other `Err` is past its end (or
    /// past a line end), so the last "below" hit is the nearest block.
    fn leaf_point_at(&self, position: gpui::Point<Pixels>) -> LeafPoint {
        let mut below: Option<LeafPoint> = None;
        for (index, leaf) in self.leaves.iter().enumerate() {
            match leaf.layout.index_for_position(position) {
                Ok(offset) => return (index, clamp_boundary(&leaf.text, offset)),
                Err(0) => return below.unwrap_or((index, 0)),
                Err(offset) => below = Some((index, clamp_boundary(&leaf.text, offset))),
            }
        }
        below.unwrap_or((0, 0))
    }

    /// Ordered block selection; `None` when collapsed.
    fn normalized_block_selection(&self) -> Option<(LeafPoint, LeafPoint)> {
        if self.anchor == self.head {
            return None;
        }
        if self.anchor <= self.head {
            Some((self.anchor, self.head))
        } else {
            Some((self.head, self.anchor))
        }
    }

    /// Selected text across leaves; items of one list join with a newline,
    /// everything else with a blank line.
    fn selected_leaf_text(&self) -> Option<String> {
        let (start, end) = self.normalized_block_selection()?;
        let mut text = String::new();
        let mut previous_key = None;
        for index in start.0..=end.0 {
            let Some(leaf) = self.leaves.get(index) else {
                break;
            };
            let lo = if index == start.0 { start.1 } else { 0 };
            let hi = if index == end.0 {
                end.1
            } else {
                leaf.text.len()
            };
            let lo = clamp_boundary(&leaf.text, lo.min(leaf.text.len()));
            let hi = clamp_boundary(&leaf.text, hi.min(leaf.text.len()));
            if lo >= hi {
                continue;
            }
            if !text.is_empty() {
                text.push_str(
                    if leaf.list_key.is_some() && leaf.list_key == previous_key {
                        "\n"
                    } else {
                        "\n\n"
                    },
                );
            }
            previous_key = leaf.list_key;
            text.push_str(&leaf.text[lo..hi]);
        }
        (!text.is_empty()).then_some(text)
    }

    fn note_code_copied(&mut self, slot: usize, cx: &mut Context<Self>) {
        self.copied = Some(slot);
        self.copy_generation = self.copy_generation.wrapping_add(1);
        let generation = self.copy_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(COPIED_FEEDBACK_MS))
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.copy_generation == generation {
                    view.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}

/// Sole-prose documents render through `StyledText`; snapshot the text once
/// per parse so frame renders share it instead of copying.
fn sole_text_snapshot(document: &MarkdownDocument) -> Option<SharedString> {
    document
        .sole_prose()
        .map(|prose| SharedString::from(prose.text.clone()))
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
        let selectable = self.document.is_selectable_prose();
        let default_style = window.text_style();

        let content = if let (Some(prose), Some(sole_text)) =
            (self.document.sole_prose(), self.sole_text.as_ref())
        {
            let base_font_px = default_style.font_size.to_pixels(window.rem_size()).into();
            let runs =
                render::text_runs(prose, self.selection.clone(), &default_style, base_font_px);
            let text = StyledText::new(sole_text.clone()).with_runs(runs);
            self.layout = Some(text.layout().clone());
            text.into_any_element()
        } else {
            self.layout = None;
            let base_font_px = default_style.font_size.to_pixels(window.rem_size()).into();
            let selection = self.normalized_block_selection();
            let copied = self.copied;
            let this = cx.entity().downgrade();
            let copy_code = move |slot: usize, code: String| -> ClickHandler {
                let this = this.clone();
                Box::new(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                    let _ = this.update(cx, |view, cx| view.note_code_copied(slot, cx));
                })
            };
            let id = self.id.clone();
            let options = render::MarkdownRenderOptions {
                default_style: &default_style,
                base_font_px,
                selection,
                copied,
                copy_code: &copy_code,
                id_prefix: &id,
            };
            let (element, leaves) = render::render_document(&self.document.blocks, &options);
            self.leaves = leaves;
            element
        };
        let selectable = selectable || !self.leaves.is_empty();

        div()
            .id(self.id.clone())
            .track_focus(&self.focus_handle)
            .key_context("TranscriptText")
            .tab_index(0)
            .w_full()
            .overflow_x_scroll()
            .scrollbar_width(px(4.0))
            .cursor(if selectable {
                CursorStyle::IBeam
            } else {
                CursorStyle::Arrow
            })
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::select_all))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(content)
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
        | MessageBlock::File { .. }
        | MessageBlock::ToolCall { .. }
        | MessageBlock::ToolResult { .. }
        | MessageBlock::Bash { .. }
        | MessageBlock::Unsupported { .. } => None,
    }
}

struct ConversationRenderContext<'a> {
    projection: &'a ConversationProjection,
    texts: &'a HashMap<String, Entity<TranscriptText>>,
    disclosures: &'a Entity<ActivityDisclosureState>,
    root: &'a Entity<crate::views::RootView>,
}

fn turn_card(
    number: usize,
    user: &RuntimeMessage,
    model: &StreamBandModel,
    render: &ConversationRenderContext<'_>,
    links_above: bool,
    links_below: bool,
    cx: &mut App,
) -> impl IntoElement {
    let has_activity = model.latest.is_some() || !model.rows.is_empty();
    let continues = has_activity || model.reply.is_some();
    div()
        .id(SharedString::from(format!("turn-{}", user.key.0)))
        .w_full()
        .flex()
        .flex_col()
        .when(!links_below, |turn| turn.pb(px(TURN_GAP)))
        .child(user_prompt(
            number,
            user,
            render.texts,
            links_above,
            continues,
        ))
        .when(has_activity, |turn| {
            turn.child(activity_band(
                &format!("turn:{}", user.key.0),
                model,
                model.reply.is_some(),
                render,
                cx,
            ))
        })
        .when_some(model.reply.as_deref(), |turn, message| {
            turn.child(assistant_reply(message, render.texts, true, links_below))
        })
        // The chain crosses the turn break: the rail spans the resting
        // whitespace so a follow-up grows straight out of the reply.
        .when(links_below, |turn| turn.child(turn_bridge()))
}

fn optimistic_turn(
    index: usize,
    input: &AcceptedUserInput,
    texts: &HashMap<String, Entity<TranscriptText>>,
    links_above: bool,
    continues: bool,
) -> impl IntoElement {
    let key = format!("optimistic:{}:text", input.request.as_str());
    let mut body = Vec::new();
    if !input.text.is_empty() {
        body.push(prompt_selectable(&key, texts));
    }
    let mut chips = Vec::new();
    for image in &input.images {
        let label = image.file_name.as_deref().map_or_else(
            || format!("Image · {}", image.mime_type),
            |name| format!("Image · {name}"),
        );
        chips.push(attachment_chip("icons/image.svg", label));
    }
    for file in &input.files {
        chips.push(file_attachment_chip(&file.metadata));
    }
    if !chips.is_empty() {
        body.push(attachment_chip_row(chips));
    }
    let meta = UserPromptMeta {
        id: SharedString::from(format!("optimistic-meta-{}", input.request.as_str())),
        rows: vec![
            ("Turn".to_owned(), format!("{index:02}")),
            (
                "Status".to_owned(),
                optimistic_status(input.kind).to_owned(),
            ),
        ],
        time_label: None,
        status_label: Some(optimistic_status(input.kind)),
    };
    div()
        .id(SharedString::from(format!(
            "optimistic-turn-{}",
            input.request.as_str()
        )))
        .w_full()
        .child(user_prompt_section(
            meta,
            true,
            links_above,
            continues,
            body,
        ))
}

fn user_prompt(
    index: usize,
    message: &RuntimeMessage,
    texts: &HashMap<String, Entity<TranscriptText>>,
    links_above: bool,
    continues: bool,
) -> impl IntoElement {
    let mut body = Vec::new();
    let mut chips = Vec::new();
    for block in &message.content {
        match block {
            MessageBlock::Text { .. } => {
                body.push(prompt_selectable(&fragment_key(message, block), texts));
            }
            MessageBlock::Image { mime_type, .. } => {
                chips.push(attachment_chip(
                    "icons/image.svg",
                    format!("Image · {mime_type}"),
                ));
            }
            MessageBlock::File { metadata, .. } => {
                chips.push(file_attachment_chip(metadata));
            }
            _ => {}
        }
    }
    if !chips.is_empty() {
        body.push(attachment_chip_row(chips));
    }
    let meta = UserPromptMeta {
        id: SharedString::from(format!("user-meta-{}", message.key.0)),
        rows: vec![
            ("Turn".to_owned(), format!("{index:02}")),
            ("Timestamp".to_owned(), format_timestamp(message.timestamp)),
        ],
        time_label: Some(format_timestamp(message.timestamp)),
        status_label: None,
    };
    user_prompt_section(meta, false, links_above, continues, body)
}

/// One node on a turn's thread: a centered dot plus, unless the turn ends
/// here, the hairline hanging down to the next section. A live node breathes
/// so in-flight work reads from the rail alone.
fn thread_rail(
    marker: gpui::Rgba,
    dot: f32,
    links_above: bool,
    continues: bool,
    pulse: Option<SharedString>,
) -> impl IntoElement {
    let node = div()
        .w(px(dot))
        .h(px(dot))
        .rounded_full()
        .bg(marker)
        .flex_shrink_0();
    // When the chain reaches in from an earlier row, the node hangs off the
    // incoming hairline instead of floating at the top of its own row.
    let node = if links_above {
        node
    } else {
        node.mt(px(NODE_ALIGN - dot / 2.0))
    };
    let node = match pulse {
        Some(id) => node
            .with_animation(
                id,
                Animation::new(Duration::from_millis(1600))
                    .repeat()
                    .with_easing(pulsating_between(0.45, 1.0)),
                |node, delta| node.opacity(delta),
            )
            .into_any_element(),
        None => node.into_any_element(),
    };
    div()
        .w(px(THREAD_RAIL_W))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .items_center()
        .when(links_above, |rail| {
            rail.child(
                div()
                    .w(px(1.0))
                    .h(px(NODE_ALIGN - dot / 2.0))
                    .bg(theme::edge()),
            )
        })
        .child(node)
        .when(continues, |rail| {
            rail.child(
                div()
                    .flex_1()
                    .w(px(1.0))
                    .min_h(px(8.0))
                    .mt(px(3.0))
                    .rounded_full()
                    .bg(theme::edge()),
            )
        })
}

/// A section of a turn hung on the shared thread: rail on the left, content
/// using the full stream width so the turn reads as one composed chain.
fn thread_section(
    marker: gpui::Rgba,
    dot: f32,
    links_above: bool,
    continues: bool,
    pulse: Option<SharedString>,
    body: impl IntoElement,
) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .gap(px(THREAD_GAP))
        .child(thread_rail(marker, dot, links_above, continues, pulse))
        .child(div().flex_1().min_w_0().w_full().child(body))
}

/// The rail continuing through the resting whitespace between turns, so one
/// turn's last section chains straight into the next prompt.
fn turn_bridge() -> impl IntoElement {
    div().w_full().h(px(TURN_GAP)).flex().flex_row().child(
        div()
            .w(px(THREAD_RAIL_W))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .child(div().w(px(1.0)).flex_1().bg(theme::edge())),
    )
}

/// Editorial prompt section: a quiet warm card for what was asked, hanging on
/// the thread instead of banding a boxed turn.
/// Header meta parked behind the (i) control: identity popup rows plus the
/// inline status/time labels.
struct UserPromptMeta {
    id: SharedString,
    rows: Vec<(String, String)>,
    time_label: Option<String>,
    status_label: Option<&'static str>,
}

fn user_prompt_section(
    meta: UserPromptMeta,
    pending: bool,
    links_above: bool,
    continues: bool,
    body: Vec<AnyElement>,
) -> impl IntoElement {
    let mark = if pending {
        theme::data()
    } else {
        theme::signal()
    };
    // Pending turns keep breathing on the thread until the transcript answers.
    let pulse = pending.then(|| SharedString::from(format!("thread-pulse:{}", meta.id)));
    thread_section(
        mark,
        NODE_SPEAKER,
        links_above,
        continues,
        pulse,
        div()
            .w_full()
            .flex()
            .flex_col()
            .pb(px(TURN_SECTION_GAP))
            .child(user_prompt_header(meta, pending))
            .when(!body.is_empty(), |section| {
                section.child(
                    div()
                        .w_full()
                        .mt(px(6.0))
                        .rounded(px(theme::RADIUS_MD))
                        .bg(theme::user_message())
                        .overflow_hidden()
                        .flex()
                        .flex_row()
                        .child(div().w(px(3.0)).flex_shrink_0().bg(if pending {
                            theme::data()
                        } else {
                            theme::signal()
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px(px(13.0))
                                .py(px(11.0))
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .children(body),
                        ),
                )
            }),
    )
}

fn user_prompt_header(meta: UserPromptMeta, pending: bool) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_LABEL))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if pending { theme::data() } else { theme::ash() })
                .child("You"),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .when_some(meta.status_label, |row, label| {
                    row.child(
                        div()
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_TINY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::data())
                            .child(label),
                    )
                })
                .when_some(meta.time_label, |row, time| {
                    row.child(
                        div()
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_TINY))
                            .text_color(theme::smoke())
                            .child(time),
                    )
                })
                .child(message_meta_info(meta.id, meta.rows)),
        )
}

/// One spine step inside a turn's activity band. Owned so a band model can
/// be cached across frames; details carry the expensive payload clones and
/// are shared behind `Arc` so hit-testing and overlays cost a refcount bump.
enum ActivityStep {
    Thinking {
        key: String,
        redacted: bool,
        detail: Arc<ActivityDetail>,
    },
    Text {
        key: String,
    },
    Image {
        mime_type: SharedString,
    },
    Tool {
        presentation: Box<ToolPresentation>,
        detail: Arc<ActivityDetail>,
    },
    Summary {
        label: &'static str,
        key: String,
    },
    Custom {
        kind: SharedString,
        key: String,
    },
    Unsupported {
        kind: SharedString,
    },
    Notice {
        text: String,
        error: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::views) struct ActivityDetail {
    pub(in crate::views) title: String,
    pub(in crate::views) prompt: Option<String>,
    pub(in crate::views) records: Vec<ActivityDetailRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::views) struct ActivityDetailRecord {
    pub(in crate::views) id: String,
    pub(in crate::views) kind: &'static str,
    pub(in crate::views) label: String,
    pub(in crate::views) parameters: Option<String>,
    pub(in crate::views) result: String,
    pub(in crate::views) metadata: Vec<(String, String)>,
}

type PersistedToolResult<'a> = (
    &'a str,
    &'a [ToolImage],
    Option<&'a serde_json::Value>,
    bool,
);

fn split_turn(
    messages: &[Arc<RuntimeMessage>],
    projection: &ConversationProjection,
    prompt: Option<&str>,
) -> (Vec<ActivityStep>, Option<Arc<RuntimeMessage>>) {
    let call_ids = messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            MessageBlock::ToolCall { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let persisted_results = messages
        .iter()
        .flat_map(|message| &message.content)
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
        .rposition(|message| is_final_assistant_reply(message));

    let mut activity = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let is_reply = reply_index == Some(index);
        push_message_activity(
            &mut activity,
            message,
            projection,
            &call_ids,
            &persisted_results,
            is_reply,
            prompt,
        );
    }
    let reply = reply_index.map(|index| messages[index].clone());
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
        && !matches!(
            message.stop_reason,
            Some(MessageStopReason::ToolUse | MessageStopReason::Deferred)
        )
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

fn push_message_activity(
    activity: &mut Vec<ActivityStep>,
    message: &RuntimeMessage,
    projection: &ConversationProjection,
    call_ids: &HashSet<ToolCallId>,
    persisted_results: &HashMap<ToolCallId, PersistedToolResult<'_>>,
    is_reply: bool,
    prompt: Option<&str>,
) {
    for block in &message.content {
        match block {
            MessageBlock::Text { .. } if is_reply => {}
            MessageBlock::Text { .. } => activity.push(ActivityStep::Text {
                key: fragment_key(message, block),
            }),
            MessageBlock::Thinking { text, redacted, .. } => {
                let key = fragment_key(message, block);
                activity.push(ActivityStep::Thinking {
                    detail: Arc::new(thinking_detail(message, &key, text, *redacted, prompt)),
                    key,
                    redacted: *redacted,
                })
            }
            MessageBlock::Image { mime_type, .. } => activity.push(ActivityStep::Image {
                mime_type: SharedString::from(mime_type.clone()),
            }),
            MessageBlock::File { .. } => {}
            MessageBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                let presentation = presentation_for_tool_call(
                    projection,
                    id,
                    name,
                    arguments,
                    persisted_results.get(id).copied(),
                );
                activity.push(ActivityStep::Tool {
                    detail: Arc::new(tool_detail(&presentation, Some(message), prompt, None)),
                    presentation: Box::new(presentation),
                });
            }
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
                    let presentation = presentation_for_standalone_result(
                        id,
                        name,
                        content,
                        images,
                        details.as_ref(),
                        *is_error,
                    );
                    activity.push(ActivityStep::Tool {
                        detail: Arc::new(tool_detail(&presentation, Some(message), prompt, None)),
                        presentation: Box::new(presentation),
                    });
                }
            }
            MessageBlock::Bash {
                command,
                output,
                cancelled,
                exit_code,
                exclude_from_context,
                ..
            } => {
                let presentation = presentation_for_bash_block(
                    command,
                    output,
                    *cancelled,
                    *exit_code,
                    *exclude_from_context,
                );
                let detail_id = fragment_key(message, block);
                activity.push(ActivityStep::Tool {
                    detail: Arc::new(tool_detail(
                        &presentation,
                        Some(message),
                        prompt,
                        Some(&detail_id),
                    )),
                    presentation: Box::new(presentation),
                });
            }
            MessageBlock::Summary { .. } => activity.push(ActivityStep::Summary {
                label: match message.role {
                    MessageRole::BranchSummary => "Branch summary",
                    MessageRole::CompactionSummary => "Compaction summary",
                    _ => "Summary",
                },
                key: fragment_key(message, block),
            }),
            MessageBlock::Custom { kind, .. } => activity.push(ActivityStep::Custom {
                kind: SharedString::from(kind.clone()),
                key: fragment_key(message, block),
            }),
            MessageBlock::Unsupported { kind, .. } => activity.push(ActivityStep::Unsupported {
                kind: SharedString::from(kind.clone()),
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

fn thinking_detail(
    message: &RuntimeMessage,
    id: &str,
    text: &str,
    redacted: bool,
    prompt: Option<&str>,
) -> ActivityDetail {
    let parameters = Some(message.assistant.as_ref().map_or_else(
        || "{}".to_owned(),
        |assistant| {
            pretty_json(&serde_json::json!({
                "api": assistant.api,
                "provider": assistant.provider,
                "model": assistant.model,
                "responseModel": assistant.response_model,
                "terminal": message.terminal,
                "stopReason": stop_reason_value(message.stop_reason),
            }))
        },
    ));
    let result = if redacted {
        "Thinking was redacted by the provider.".to_owned()
    } else if text.trim().is_empty() {
        "No thinking text was recorded.".to_owned()
    } else {
        sanitize_untrusted_text(text)
    };
    ActivityDetail {
        title: "Thinking details".to_owned(),
        prompt: Some(prompt.map(sanitize_untrusted_text).unwrap_or_else(|| {
            "The originating prompt was not recorded for this entry.".to_owned()
        })),
        records: vec![ActivityDetailRecord {
            id: format!("thinking:{id}"),
            kind: "Thinking",
            label: "Model reasoning".to_owned(),
            parameters,
            result,
            metadata: assistant_metadata_rows(message),
        }],
    }
}

fn tool_detail(
    presentation: &ToolPresentation,
    message: Option<&RuntimeMessage>,
    prompt: Option<&str>,
    id_override: Option<&str>,
) -> ActivityDetail {
    let parameters = presentation
        .arguments
        .as_ref()
        .map(pretty_json)
        .or_else(|| Some("{}".to_owned()));
    let result = tool_result_text(presentation);
    let id = id_override
        .map(str::to_owned)
        .or_else(|| presentation.call_id.clone())
        .unwrap_or_else(|| {
            format!(
                "tool:{}:{:016x}",
                presentation.name,
                source_hash(&format!(
                    "{}\n{result}",
                    parameters.as_deref().unwrap_or("")
                ))
            )
        });
    let mut metadata = vec![(
        "Status".to_owned(),
        activity_status_label(presentation.status).to_owned(),
    )];
    if let Some(call_id) = presentation.call_id.as_ref() {
        metadata.push(("Call ID".to_owned(), sanitize_untrusted_text(call_id)));
    }
    if let Some(elapsed) = presentation.elapsed_ms {
        metadata.push(("Elapsed".to_owned(), format_activity_elapsed(elapsed)));
    }
    metadata.push((
        "Context".to_owned(),
        if presentation.context_excluded {
            "Excluded".to_owned()
        } else {
            "Included".to_owned()
        },
    ));
    if presentation.payload.truncated {
        metadata.push(("Output".to_owned(), "Truncated".to_owned()));
    }
    if let Some(path) = presentation.payload.full_output_path.as_ref() {
        metadata.push(("Full output".to_owned(), sanitize_untrusted_text(path)));
    }
    if let Some(message) = message {
        metadata.extend(assistant_metadata_rows(message));
    }

    ActivityDetail {
        title: format!("{} details", sanitize_untrusted_text(&presentation.name)),
        prompt: Some(prompt.map(sanitize_untrusted_text).unwrap_or_else(|| {
            "The originating prompt was not recorded for this entry.".to_owned()
        })),
        records: vec![ActivityDetailRecord {
            id,
            kind: "Tool use",
            label: sanitize_untrusted_text(&presentation.name),
            parameters,
            result,
            metadata,
        }],
    }
}

fn grouped_tool_detail(details: &[Arc<ActivityDetail>]) -> ActivityDetail {
    let records = details
        .iter()
        .flat_map(|detail| detail.records.iter().cloned())
        .collect::<Vec<_>>();
    let name = records
        .first()
        .map(|record| record.label.as_str())
        .unwrap_or("Tool use");
    ActivityDetail {
        title: format!(
            "{} details · {} calls",
            sanitize_untrusted_text(name),
            records.len()
        ),
        prompt: details.iter().find_map(|detail| detail.prompt.clone()),
        records,
    }
}

fn tool_result_text(presentation: &ToolPresentation) -> String {
    let payload = &presentation.payload;
    let mut sections = Vec::new();
    if !payload.text.trim().is_empty() {
        sections.push(sanitize_untrusted_text(&payload.text));
    }
    if let Some(diff) = payload.diff.as_deref()
        && !diff.trim().is_empty()
        && !payload.text.contains(diff)
    {
        sections.push(format!("Diff\n{}", sanitize_untrusted_text(diff)));
    }
    if !payload.images.is_empty() {
        let images = payload
            .images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                format!(
                    "{}. {}",
                    index + 1,
                    sanitize_untrusted_text(&image.mime_type)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("Images\n{images}"));
    }
    if let Some(details) = payload.details.as_ref() {
        sections.push(format!("Details\n{}", pretty_json(details)));
    }
    if let Some(note) = payload.truncation_note.as_ref() {
        sections.push(sanitize_untrusted_text(note));
    } else if payload.truncated {
        sections.push("The displayed result is truncated.".to_owned());
    }
    if sections.is_empty() {
        if matches!(
            presentation.status,
            CardStatus::Pending | CardStatus::Running
        ) {
            "Waiting for the tool result.".to_owned()
        } else {
            "The tool returned no displayable result.".to_owned()
        }
    } else {
        sections.join("\n\n")
    }
}

fn pretty_json(value: &serde_json::Value) -> String {
    sanitize_untrusted_text(
        &serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    )
}

fn assistant_metadata_rows(message: &RuntimeMessage) -> Vec<(String, String)> {
    let mut rows = vec![
        ("Timestamp".to_owned(), format_timestamp(message.timestamp)),
        (
            "Response".to_owned(),
            if message.terminal {
                "Completed".to_owned()
            } else {
                "Streaming".to_owned()
            },
        ),
    ];
    let Some(assistant) = message.assistant.as_ref() else {
        return rows;
    };
    rows.extend([
        (
            "Model".to_owned(),
            assistant
                .response_model
                .as_deref()
                .filter(|model| !model.is_empty())
                .unwrap_or(&assistant.model)
                .to_owned(),
        ),
        ("Provider".to_owned(), assistant.provider.clone()),
        ("API".to_owned(), assistant.api.clone()),
        (
            "Tokens".to_owned(),
            format!(
                "{} in · {} out · {} total",
                format_count(assistant.usage.input),
                format_count(assistant.usage.output),
                format_count(assistant.usage.total_tokens)
            ),
        ),
        ("Cost".to_owned(), format_cost(assistant.usage.total_cost)),
    ]);
    if let Some(reasoning) = assistant.usage.reasoning {
        rows.push(("Reasoning tokens".to_owned(), format_count(reasoning)));
    }
    rows
}

fn activity_status_label(status: CardStatus) -> &'static str {
    match status {
        CardStatus::Pending => "Pending",
        CardStatus::Running => "Running",
        CardStatus::Success => "Succeeded",
        CardStatus::Error => "Failed",
        CardStatus::Cancelled => "Cancelled",
        CardStatus::Cancelling => "Cancelling",
        CardStatus::Uncertain => "Unknown outcome",
    }
}

fn stop_reason_value(reason: Option<MessageStopReason>) -> Option<&'static str> {
    reason.map(|reason| match reason {
        MessageStopReason::Stop => "stop",
        MessageStopReason::Length => "length",
        MessageStopReason::ToolUse => "tool_use",
        MessageStopReason::Error => "error",
        MessageStopReason::Aborted => "aborted",
        MessageStopReason::Deferred => "deferred",
    })
}

fn format_activity_elapsed(elapsed_ms: u128) -> String {
    if elapsed_ms < 1_000 {
        format!("{elapsed_ms} ms")
    } else {
        format!("{:.1} s", elapsed_ms as f64 / 1_000.0)
    }
}

/// Split finished steps into the trailing live step and the display rows for
/// the collapsed history, folding same-name groupable tool runs into a single
/// precomputed group row. Runs once per band fingerprint, not per frame.
fn assemble_band(
    mut steps: Vec<ActivityStep>,
) -> (Vec<BandRow>, usize, Option<ActivityStep>, Option<String>) {
    let latest = steps.pop();
    let history_count = steps.len();
    let summary = history_summary(&steps);
    let mut rows = Vec::new();
    let mut steps = steps.into_iter().peekable();
    while let Some(step) = steps.next() {
        if let ActivityStep::Tool {
            presentation,
            detail,
        } = &step
            && presentation.groupable()
        {
            let group_name = presentation.name.clone();
            let mut presentations = vec![presentation.as_ref().clone()];
            let mut details = vec![detail.clone()];
            while let Some(ActivityStep::Tool {
                presentation: next,
                detail: next_detail,
            }) = steps.peek()
            {
                if next.name != group_name || !next.groupable() {
                    break;
                }
                presentations.push(next.as_ref().clone());
                details.push(next_detail.clone());
                steps.next();
            }
            rows.push(BandRow::Group(Arc::new(ToolGroupModel::build(
                presentations,
                details,
            ))));
            continue;
        }

        rows.push(BandRow::Step(step));
    }
    (rows, history_count, latest, summary)
}

impl ToolGroupModel {
    fn build(presentations: Vec<ToolPresentation>, details: Vec<Arc<ActivityDetail>>) -> Self {
        let marker = presentations
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
        let detail = Arc::new(grouped_tool_detail(&details));
        let trigger_id = SharedString::from(format!(
            "tool-group:{:016x}",
            source_hash(
                &detail
                    .records
                    .iter()
                    .map(|record| record.id.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        ));
        let live = presentations.iter().any(|item| {
            matches!(
                item.status,
                CardStatus::Pending | CardStatus::Running | CardStatus::Cancelling
            )
        });
        Self {
            presentations,
            detail,
            trigger_id,
            marker,
            live,
        }
    }
}

fn build_turn_model(
    projection: &ConversationProjection,
    user_index: usize,
    body: Range<usize>,
    cache: &Entity<TranscriptTextCache>,
    cx: &mut App,
) -> StreamBandModel {
    let prompt = message_prompt_text(&projection.messages[user_index]);
    let texts = cached_message_texts(projection, user_index..body.end, cache, cx);
    let (steps, reply) = split_turn(&projection.messages[body], projection, Some(&prompt));
    let (rows, history_count, latest, summary) = assemble_band(steps);
    StreamBandModel {
        texts,
        rows,
        history_count,
        latest,
        summary,
        reply,
    }
}

fn build_preamble_model(
    projection: &ConversationProjection,
    message_index: usize,
    cache: &Entity<TranscriptTextCache>,
    cx: &mut App,
) -> StreamBandModel {
    let texts = cached_message_texts(projection, message_index..message_index + 1, cache, cx);
    let messages = std::slice::from_ref(&projection.messages[message_index]);
    let (steps, reply) = split_turn(messages, projection, None);
    let (rows, history_count, latest, summary) = assemble_band(steps);
    StreamBandModel {
        texts,
        rows,
        history_count,
        latest,
        summary,
        reply,
    }
}

fn build_tail_model(projection: &ConversationProjection) -> StreamBandModel {
    let steps = tail_presentations(projection)
        .into_iter()
        .map(|presentation| ActivityStep::Tool {
            detail: Arc::new(tool_detail(&presentation, None, None, None)),
            presentation: Box::new(presentation),
        })
        .collect::<Vec<_>>();
    let (rows, history_count, latest, summary) = assemble_band(steps);
    StreamBandModel {
        texts: HashMap::new(),
        rows,
        history_count,
        latest,
        summary,
        reply: None,
    }
}

fn activity_band(
    disclosure_key: &str,
    model: &StreamBandModel,
    has_reply: bool,
    render: &ConversationRenderContext<'_>,
    cx: &mut App,
) -> impl IntoElement {
    let Some(latest_step) = model.latest.as_ref() else {
        return div().into_any_element();
    };
    let step_count = model.history_count + 1;
    let children = model
        .rows
        .iter()
        .map(|row| match row {
            // History steps chain straight into the latest step below them.
            BandRow::Step(step) => render_activity_step(step, true, &model.texts, render.root),
            BandRow::Group(group) => render_tool_group(group, true, render.root),
        })
        .collect::<Vec<_>>();

    let expanded = render.disclosures.read(cx).is_expanded(disclosure_key);
    let show_history = render.disclosures.read(cx).shows_history(disclosure_key);
    let motion = render.disclosures.read(cx).motion(disclosure_key);

    let animate_latest = !show_history
        && matches!(
            latest_step,
            ActivityStep::Tool { presentation, .. }
                if matches!(
                    presentation.status,
                    CardStatus::Pending | CardStatus::Running | CardStatus::Cancelling
                )
        );
    let latest = render_activity_step(latest_step, has_reply, &model.texts, render.root);
    let latest = if animate_latest {
        div()
            .w_full()
            .child(latest)
            .with_animation(
                SharedString::from(format!(
                    "activity-latest-tool:{disclosure_key}:{step_count}"
                )),
                Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()),
                |row, delta| row.ml(px(6.0 * (1.0 - delta))).opacity(0.68 + 0.32 * delta),
            )
            .into_any_element()
    } else {
        latest
    };

    let history_panel = if show_history && model.history_count > 0 {
        Some(activity_history_panel(
            disclosure_key,
            model.history_count,
            expanded,
            motion,
            children,
        ))
    } else {
        None
    };

    div()
        .w_full()
        .flex()
        .flex_col()
        .when(model.history_count > 0, |band| {
            band.child(activity_disclosure(
                disclosure_key,
                model.history_count,
                model.summary.clone(),
                expanded,
                render.disclosures,
            ))
        })
        .children(history_panel)
        .child(latest)
        .into_any_element()
}

fn activity_history_panel(
    disclosure_key: &str,
    history_count: usize,
    expanded: bool,
    motion: Option<DisclosureMotion>,
    children: Vec<AnyElement>,
) -> AnyElement {
    let body = div()
        .w_full()
        .pt(px(2.0))
        .flex()
        .flex_col()
        .children(children);

    let Some(motion) = motion else {
        // Settled open — full natural height, no clip.
        return body.into_any_element();
    };

    let opening = motion.opening && expanded;
    // Closing starts from a roomier clip so multi-line steps do not snap-crop.
    let span_h = if opening {
        disclosure_history_estimate(history_count)
    } else {
        (disclosure_history_estimate(history_count) * 1.55).min(DISCLOSURE_HISTORY_MAX_PX + 96.0)
    };
    div()
        .w_full()
        .overflow_hidden()
        .child(body)
        .with_animation(
            SharedString::from(format!(
                "activity-history:{disclosure_key}:{}",
                motion.generation
            )),
            Animation::new(Duration::from_millis(DISCLOSURE_MOTION_MS))
                .with_easing(ease_out_quint()),
            move |panel, delta| {
                let t = if opening { delta } else { 1.0 - delta };
                let fade = 0.18 + 0.82 * t;
                let height = (span_h * t).max(0.5);
                panel
                    .max_h(px(height))
                    .opacity(fade)
                    .mt(px(-3.0 * (1.0 - t)))
            },
        )
        .into_any_element()
}

fn disclosure_history_estimate(history_count: usize) -> f32 {
    let steps = history_count.max(1) as f32;
    (steps * DISCLOSURE_STEP_ESTIMATE_PX + DISCLOSURE_HISTORY_PAD_PX)
        .clamp(36.0, DISCLOSURE_HISTORY_MAX_PX)
}

/// One-line "what is inside" for a collapsed activity history: step kinds in
/// order of first appearance with counts, trimmed so the row stays quiet.
fn history_summary(steps: &[ActivityStep]) -> Option<String> {
    if steps.is_empty() {
        return None;
    }
    let mut kinds: Vec<(String, usize)> = Vec::new();
    for step in steps {
        let token = match step {
            ActivityStep::Thinking { .. } => "thinking".to_owned(),
            ActivityStep::Text { .. } | ActivityStep::Notice { error: false, .. } => {
                "note".to_owned()
            }
            ActivityStep::Notice { error: true, .. } => "error".to_owned(),
            ActivityStep::Image { .. } => "image".to_owned(),
            ActivityStep::Summary { .. } => "summary".to_owned(),
            ActivityStep::Tool { presentation, .. } => sanitize_untrusted_text(&presentation.name),
            ActivityStep::Custom { kind, .. } | ActivityStep::Unsupported { kind } => {
                sanitize_untrusted_text(kind)
            }
        };
        match kinds.iter_mut().find(|(name, _)| *name == token) {
            Some((_, count)) => *count += 1,
            None => kinds.push((token, 1)),
        }
    }

    let mut shown = kinds
        .iter()
        .take(3)
        .map(|(name, count)| {
            if *count > 1 {
                format!("{name} ×{count}")
            } else {
                name.clone()
            }
        })
        .collect::<Vec<_>>();
    let remaining = kinds.len() - shown.len();
    if remaining > 0 {
        shown.push(format!("+{remaining}"));
    }
    Some(shown.join(" · "))
}

fn activity_disclosure(
    key: &str,
    history_count: usize,
    summary: Option<String>,
    expanded: bool,
    disclosures: &Entity<ActivityDisclosureState>,
) -> impl IntoElement {
    let click_key = key.to_owned();
    let click_state = disclosures.clone();
    let keyboard_key = key.to_owned();
    let keyboard_state = disclosures.clone();
    let label = if history_count == 1 {
        "1 earlier step".to_owned()
    } else {
        format!("{history_count} earlier steps")
    };

    // Quiet link-style control. The bare chevron sits on the rail column so
    // the thread reads unbroken; feedback is a color shift, not another box.
    div()
        .id(SharedString::from(format!("activity-disclosure:{key}")))
        .tab_index(0)
        .cursor_pointer()
        .w_full()
        .pb(px(6.0))
        .flex()
        .flex_row()
        .gap(px(THREAD_GAP))
        .text_color(theme::ash())
        .hover(|row| row.text_color(theme::bone_dim()))
        .focus(|row| row.text_color(theme::focus()))
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
                .w(px(THREAD_RAIL_W))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    svg()
                        .path(if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        })
                        .size(px(9.0))
                        .mt(px(4.5))
                        .text_color(theme::smoke()),
                )
                .child(
                    div()
                        .flex_1()
                        .w(px(1.0))
                        .min_h(px(6.0))
                        .mt(px(3.0))
                        .rounded_full()
                        .bg(theme::edge()),
                ),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .flex()
                .flex_row()
                .items_baseline()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label),
                )
                .when_some(summary, |row, summary| {
                    row.child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_TINY))
                            .text_color(theme::smoke())
                            .child(summary),
                    )
                }),
        )
}

fn render_tool_group(
    group: &Arc<ToolGroupModel>,
    continues: bool,
    root: &Entity<crate::views::RootView>,
) -> AnyElement {
    let pulse = group
        .live
        .then(|| SharedString::from(format!("thread-pulse:{}", group.trigger_id)));
    step_shell(
        continues,
        group.marker,
        pulse,
        tool_detail_trigger(
            &group.trigger_id,
            render_tool_presentation(&group.presentations, None, false, None),
            group.detail.clone(),
            root,
        ),
    )
    .into_any_element()
}

fn render_activity_step(
    step: &ActivityStep,
    continues: bool,
    texts: &HashMap<String, Entity<TranscriptText>>,
    root: &Entity<crate::views::RootView>,
) -> AnyElement {
    match step {
        ActivityStep::Thinking {
            key: _,
            redacted: true,
            detail,
        } => step_shell(
            continues,
            theme::smoke(),
            None,
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(thinking_detail_header(detail, root))
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::smoke())
                        .child("Thinking was redacted by the provider."),
                ),
        )
        .into_any_element(),
        ActivityStep::Thinking {
            key,
            redacted: false,
            detail,
        } => step_shell(
            continues,
            theme::smoke(),
            None,
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(thinking_detail_header(detail, root))
                .child(activity_selectable(key, texts)),
        )
        .into_any_element(),
        ActivityStep::Text { key } => step_shell(
            continues,
            theme::ash(),
            None,
            activity_selectable(key, texts),
        )
        .into_any_element(),
        ActivityStep::Image { mime_type } => step_shell(
            continues,
            theme::ash(),
            None,
            compact_label(format!("Image · {mime_type}")),
        )
        .into_any_element(),
        ActivityStep::Tool {
            presentation,
            detail,
        } => {
            let detail_id = detail
                .records
                .first()
                .map(|record| record.id.as_str())
                .unwrap_or("tool");
            let live = matches!(
                presentation.status,
                CardStatus::Pending | CardStatus::Running | CardStatus::Cancelling
            );
            let pulse = live.then(|| SharedString::from(format!("thread-pulse:{detail_id}")));
            step_shell(
                continues,
                status_color(presentation.status),
                pulse,
                tool_detail_trigger(
                    detail_id,
                    render_tool_presentation(
                        std::slice::from_ref(presentation.as_ref()),
                        None,
                        false,
                        None,
                    ),
                    detail.clone(),
                    root,
                ),
            )
            .into_any_element()
        }
        ActivityStep::Summary { label, key } => step_shell(
            continues,
            theme::data(),
            None,
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
            continues,
            theme::smoke(),
            None,
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(compact_label(format!("Extension · {kind}")))
                .child(activity_selectable(key, texts)),
        )
        .into_any_element(),
        ActivityStep::Unsupported { kind } => step_shell(
            continues,
            theme::smoke(),
            None,
            compact_label(format!("Unsupported · {kind}")),
        )
        .into_any_element(),
        ActivityStep::Notice { text, error } => step_shell(
            continues,
            if *error {
                theme::error()
            } else {
                theme::smoke()
            },
            None,
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

fn thinking_detail_header(
    detail: &Arc<ActivityDetail>,
    root: &Entity<crate::views::RootView>,
) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::smoke())
                .child("thinking"),
        )
        .child(activity_detail_link(detail, root))
}

fn activity_detail_link(
    detail: &Arc<ActivityDetail>,
    root: &Entity<crate::views::RootView>,
) -> AnyElement {
    let id = detail
        .records
        .first()
        .map(|record| record.id.clone())
        .unwrap_or_else(|| "activity".to_owned());
    let click_detail = detail.clone();
    let click_root = root.clone();
    let keyboard_detail = detail.clone();
    let keyboard_root = root.clone();
    div()
        .id(SharedString::from(format!("activity-detail-link:{id}")))
        .tab_index(0)
        .cursor_pointer()
        .h(px(20.0))
        .px(px(2.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(theme::mono())
        .text_size(theme::text_size(theme::T_TINY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::smoke())
        .hover(|link| link.text_color(theme::bone_dim()))
        .focus(|link| link.text_color(theme::bone_dim()))
        // Skip GPUI's mouse-down focus + active refresh on this chip. Click still
        // fires; keyboard focus via Tab is unchanged. Prevents a one-frame pop
        // before the detail overlay steals focus.
        .capture_any_mouse_down(|event, window, _| {
            if event.button == MouseButton::Left {
                window.prevent_default();
            }
        })
        .on_click(move |_, window, cx| {
            click_root.update(cx, |view, cx| {
                view.open_activity_detail(click_detail.clone(), window, cx)
            });
        })
        .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                keyboard_root.update(cx, |view, cx| {
                    view.open_activity_detail(keyboard_detail.clone(), window, cx)
                });
            }
        })
        .child("details ↗")
        .into_any_element()
}

fn tool_detail_trigger(
    id: &str,
    body: impl IntoElement,
    detail: Arc<ActivityDetail>,
    root: &Entity<crate::views::RootView>,
) -> AnyElement {
    let click_detail = detail.clone();
    let click_root = root.clone();
    let keyboard_detail = detail;
    let keyboard_root = root.clone();
    // Flat trigger: rested transparent so the step reads as text on the
    // canvas; hover and keyboard focus lift the wash instead of drawing a box.
    // The invisible border keeps geometry stable when focus retints it.
    div()
        .id(SharedString::from(format!("activity-detail-tool:{id}")))
        .tab_index(0)
        .cursor_pointer()
        .w_full()
        .min_h(px(26.0))
        .px(px(4.0))
        .py(px(2.0))
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(gpui::rgba(0x0000_0000))
        .hover(|trigger| trigger.bg(theme::panel()))
        // Recess on press — a lighter active fill makes nested chips look bigger.
        .active(|trigger| trigger.bg(theme::canvas()))
        .focus(|trigger| trigger.bg(theme::panel()).border_color(theme::edge()))
        .on_click(move |_, window, cx| {
            click_root.update(cx, |view, cx| {
                view.open_activity_detail(click_detail.clone(), window, cx)
            });
        })
        .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                let detail = keyboard_detail.clone();
                keyboard_root.update(cx, |view, cx| view.open_activity_detail(detail, window, cx));
            }
        })
        .child(body)
        .into_any_element()
}

/// One spine step inside a turn's activity band, hanging on the shared thread.
fn step_shell(
    continues: bool,
    marker: gpui::Rgba,
    pulse: Option<SharedString>,
    body: impl IntoElement,
) -> impl IntoElement {
    thread_section(
        marker,
        NODE_STEP,
        false,
        continues,
        pulse,
        div()
            .pb(if continues {
                px(TURN_SECTION_GAP)
            } else {
                px(4.0)
            })
            .child(body),
    )
}

fn assistant_reply(
    message: &RuntimeMessage,
    texts: &HashMap<String, Entity<TranscriptText>>,
    links_above: bool,
    continues: bool,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("message-{}", message.key.0)))
        .w_full()
        .child(thread_section(
            // Quiet cool dot — pairs with the warm user node.
            theme::working(),
            NODE_SPEAKER,
            links_above,
            continues,
            None,
            div()
                .w_full()
                .pb(px(2.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div().flex().flex_row().items_center().gap(px(8.0)).child(
                        div()
                            .font_family(theme::sans())
                            .text_size(theme::text_size(theme::T_LABEL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::ash())
                            .child("Pi"),
                    ),
                )
                .children(message.content.iter().filter_map(|block| match block {
                    MessageBlock::Text { .. } => Some(selectable(
                        &fragment_key(message, block),
                        texts,
                        theme::sans(),
                        theme::T_BODY,
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
                }),
        ))
}

fn preamble(
    message: &RuntimeMessage,
    model: &StreamBandModel,
    render: &ConversationRenderContext<'_>,
    cx: &mut App,
) -> AnyElement {
    let has_activity = model.latest.is_some() || !model.rows.is_empty();
    match message.role {
        MessageRole::Assistant if model.reply.is_some() => {
            if !has_activity {
                assistant_reply(message, render.texts, false, false).into_any_element()
            } else {
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(activity_band(
                        &format!("preamble:{}", message.key.0),
                        model,
                        true,
                        render,
                        cx,
                    ))
                    .child(assistant_reply(message, render.texts, true, false))
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
            if !has_activity {
                return div().into_any_element();
            }
            activity_band(
                &format!("preamble:{}", message.key.0),
                model,
                false,
                render,
                cx,
            )
            .into_any_element()
        }
        MessageRole::User => div().into_any_element(),
    }
}

fn tail_activity(
    render: &ConversationRenderContext<'_>,
    bands: &Entity<StreamBandCache>,
    cx: &mut App,
) -> Option<AnyElement> {
    let fingerprint = BandFingerprint::capture(render.projection, &render.projection.messages);
    let model = bands.update(cx, |bands, cx| {
        bands.tail_for(fingerprint, cx, |_cx| build_tail_model(render.projection))
    });
    if model.latest.is_none() && model.rows.is_empty() {
        return None;
    }

    Some(
        activity_band(
            &format!("tail:{}", render.projection.epoch.value()),
            &model,
            false,
            render,
            cx,
        )
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
    // Compact long-read rhythm; tighter than the shell's 1.58 chat default.
    selectable_with_leading(key, texts, font, size, color, weight, 1.52)
}

fn prompt_selectable(key: &str, texts: &HashMap<String, Entity<TranscriptText>>) -> AnyElement {
    selectable_with_leading(
        key,
        texts,
        theme::sans(),
        theme::T_BODY_SM,
        theme::bone(),
        FontWeight::NORMAL,
        1.48,
    )
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

fn file_message_label(metadata: &PromptFileMetadata) -> String {
    format!(
        "File · {} · {} · {}",
        sanitize_untrusted_text(&metadata.name),
        metadata.delivery.label(),
        format_bytes(metadata.size)
    )
}

fn file_attachment_chip(metadata: &PromptFileMetadata) -> AnyElement {
    let (icon, kind) = match metadata.delivery {
        FileDelivery::Snapshot => ("icons/file.svg", "Snapshot"),
        FileDelivery::PathReference => ("icons/link.svg", "Path ref"),
    };
    let label = format!(
        "{} · {} · {}",
        sanitize_untrusted_text(&metadata.name),
        kind,
        format_bytes(metadata.size)
    );
    attachment_chip(icon, label)
}

fn attachment_chip_row(chips: Vec<AnyElement>) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_start()
        .gap(px(6.0))
        .children(chips)
        .into_any_element()
}

fn attachment_chip(icon: &'static str, label: String) -> AnyElement {
    div()
        .max_w(px(280.0))
        .h(px(26.0))
        .px(px(8.0))
        .rounded(px(theme::RADIUS_MD))
        .bg(theme::panel())
        .flex()
        .flex_row()
        .items_center()
        .gap(px(7.0))
        .child(
            svg()
                .path(icon)
                .size(px(13.0))
                .text_color(theme::ash())
                .flex_shrink_0(),
        )
        .child(
            div()
                .min_w_0()
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::smoke())
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(label),
        )
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

fn message_prompt_text(message: &RuntimeMessage) -> String {
    let parts = message
        .content
        .iter()
        .filter_map(|block| match block {
            MessageBlock::Text { text, .. } => Some(sanitize_untrusted_text(text)),
            MessageBlock::Image { mime_type, .. } => {
                Some(format!("[Image: {}]", sanitize_untrusted_text(mime_type)))
            }
            MessageBlock::File { metadata, .. } => {
                Some(format!("[{}]", file_message_label(metadata)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "No user prompt text was recorded.".to_owned()
    } else {
        parts.join("\n\n")
    }
}

fn fragment_key(message: &RuntimeMessage, block: &MessageBlock) -> String {
    format!("{}:{}", message.key.0, block.key().0)
}

/// Compact (i) control that parks user turn/time meta in a hover popup.
fn message_meta_info(id: SharedString, rows: Vec<(String, String)>) -> impl IntoElement {
    let tooltip_rows = rows.clone();
    div()
        .id(id)
        .size(px(20.0))
        .flex_shrink_0()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(|icon| icon.bg(theme::panel_hover()))
        .tooltip(move |_, cx| {
            cx.new(|_| MessageMetaTooltip {
                rows: tooltip_rows.clone(),
            })
            .into()
        })
        // GPUI SVGs resolve `currentColor` from the svg element's own text_color,
        // not from a parent container — omit it and the glyph is invisible.
        .child(
            svg()
                .path("icons/info.svg")
                .size(px(15.0))
                .text_color(theme::ash()),
        )
}

struct MessageMetaTooltip {
    rows: Vec<(String, String)>,
}

impl Render for MessageMetaTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .min_w(px(200.0))
            .max_w(px(320.0))
            .p(px(12.0))
            .rounded(px(theme::RADIUS_MD))
            .bg(theme::panel_lift())
            .border_1()
            .border_color(theme::edge())
            .shadow(theme::dock_shadow())
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(self.rows.iter().map(|(label, value)| {
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_start()
                    .justify_between()
                    .gap(px(14.0))
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(theme::sans())
                            .text_size(theme::text_size(theme::T_TINY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::ash())
                            .child(label.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_right()
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_TINY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::bone())
                            .child(value.clone()),
                    )
            }))
    }
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
        Some(MessageStopReason::Deferred) => Some("Response deferred by the provider.".to_owned()),
        Some(MessageStopReason::ToolUse | MessageStopReason::Stop) | None => None,
    }
}

fn stop_color(reason: Option<MessageStopReason>) -> gpui::Rgba {
    match reason {
        Some(MessageStopReason::Length | MessageStopReason::Error) => theme::error(),
        Some(MessageStopReason::Aborted | MessageStopReason::Deferred) => theme::data(),
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
        .min_h(px(240.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .child(
            div()
                .w(px(28.0))
                .h(px(2.0))
                .rounded_full()
                .bg(theme::signal())
                .opacity(0.7),
        )
        .child(
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::bone())
                .child(title),
        )
        .child(
            div()
                .max_w(px(360.0))
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_UI))
                .line_height(relative(1.45))
                .text_color(theme::smoke())
                .child(body),
        )
}

fn optimistic_status(kind: SubmissionKind) -> &'static str {
    match kind {
        SubmissionKind::Prompt => "Awaiting transcript",
        SubmissionKind::Steer => "Steering…",
        SubmissionKind::FollowUp => "Queued…",
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
        assert!(state.motions.is_empty());

        state.expanded.insert("turn:user-2".to_owned());
        state.reset(second);
        assert!(!state.is_expanded("turn:user-2"));
        assert!(!state.shows_history("turn:user-2"));
    }

    #[test]
    fn disclosure_history_estimate_stays_bounded() {
        assert!(disclosure_history_estimate(1) >= 36.0);
        assert!(disclosure_history_estimate(100) <= DISCLOSURE_HISTORY_MAX_PX);
        assert!(disclosure_history_estimate(3) > disclosure_history_estimate(1));
    }

    #[test]
    fn history_summary_counts_kinds_in_first_seen_order() {
        let detail = Arc::new(ActivityDetail {
            title: "tool".to_owned(),
            prompt: None,
            records: Vec::new(),
        });
        let tool = |name: &str, command: &str| {
            let mut presentation = presentation_for_bash_block(command, "", false, Some(0), false);
            presentation.name = name.to_owned();
            ActivityStep::Tool {
                presentation: Box::new(presentation),
                detail: detail.clone(),
            }
        };

        let steps = vec![
            ActivityStep::Text { key: "k".into() },
            tool("read", "a"),
            tool("read", "b"),
            tool("bash", "c"),
        ];
        assert_eq!(
            history_summary(&steps),
            Some("note · read ×2 · bash".to_owned())
        );

        let crowded = vec![
            tool("read", "a"),
            tool("bash", "b"),
            tool("edit", "c"),
            tool("write", "d"),
            ActivityStep::Text { key: "k".into() },
        ];
        assert_eq!(
            history_summary(&crowded),
            Some("read · bash · edit · +2".to_owned())
        );
        assert_eq!(history_summary(&[]), None);
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
    fn tool_activity_details_keep_prompt_parameters_and_result() {
        let presentation =
            presentation_for_bash_block("printf complete", "complete", false, Some(0), true);
        let detail = tool_detail(
            &presentation,
            None,
            Some("Run the diagnostic"),
            Some("message:block"),
        );

        assert_eq!(detail.prompt.as_deref(), Some("Run the diagnostic"));
        assert_eq!(detail.records[0].id, "message:block");
        assert_eq!(detail.records.len(), 1);
        assert!(
            detail.records[0]
                .parameters
                .as_deref()
                .is_some_and(|parameters| parameters.contains("printf complete"))
        );
        assert!(detail.records[0].result.contains("complete"));
        assert!(
            detail.records[0]
                .metadata
                .contains(&("Context".to_owned(), "Excluded".to_owned()))
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
        let (activity, reply) = split_turn(&projection.messages, &projection, Some("show files"));
        assert_eq!(activity.len(), 3);
        assert!(matches!(activity[0], ActivityStep::Thinking { .. }));
        assert!(matches!(activity[1], ActivityStep::Text { .. }));
        assert!(matches!(activity[2], ActivityStep::Tool { .. }));
        let ActivityStep::Thinking { detail, .. } = &activity[0] else {
            unreachable!();
        };
        assert_eq!(detail.prompt.as_deref(), Some("show files"));
        assert_eq!(detail.records[0].id, "thinking:a1:th");
        assert_eq!(detail.records[0].result, "plan");
        let ActivityStep::Tool { detail, .. } = &activity[2] else {
            unreachable!();
        };
        assert_eq!(detail.prompt.as_deref(), Some("show files"));
        assert_eq!(detail.records[0].parameters.as_deref(), Some("{}"));
        assert_eq!(reply.as_deref().map(|m| m.key.0.as_str()), Some("a2"));
        assert_eq!(
            latest_completed_response_key(&projection).as_deref(),
            Some("a2")
        );
    }
}
