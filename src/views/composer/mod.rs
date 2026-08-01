mod buffer;
mod element;
mod render;
#[cfg(test)]
mod tests;

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui::{
    ClipboardEntry, ClipboardItem, Context, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, Image, ImageFormat, IntoElement, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Render, ScrollWheelEvent, SharedString, Subscription, UTF16Selection, Window, px,
};
use image::GenericImageView;

use self::buffer::TextBuffer;
use self::element::EditorLayout;
use crate::actions::{
    AbortRun, AcceptInput, ComposerBackspace, ComposerCopy, ComposerCut, ComposerDelete,
    ComposerDeleteWordBackward, ComposerDown, ComposerLeft, ComposerLineEnd, ComposerLineStart,
    ComposerPaste, ComposerRedo, ComposerRight, ComposerSelectAll, ComposerSelectDown,
    ComposerSelectLeft, ComposerSelectLineEnd, ComposerSelectLineStart, ComposerSelectRight,
    ComposerSelectUp, ComposerUndo, ComposerUp, InsertNewline, QueueFollowUp,
};
use crate::attachments::{
    AttachmentLoadLimits, LoadedAttachment, LoadedAttachmentBatch, MAX_ATTACHMENTS,
    MAX_IMAGE_ATTACHMENTS, MAX_IMAGE_BYTES, MAX_TOTAL_TEXT_SNAPSHOT_BYTES, PromptFile,
};
use crate::state::runtime::{PromptImage, SubmissionKind};
/// Pixel edge length for cached square attachment thumbs (display is smaller).
const ATTACHMENT_THUMB_PX: u32 = 96;
/// Expand/collapse the multiline input between single-line and multi-line heights.
pub(super) const INPUT_HEIGHT_MOTION_MS: u64 = 200;

fn decoded_image_len(data: &str) -> usize {
    let padding = data
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    data.len()
        .saturating_div(4)
        .saturating_mul(3)
        .saturating_sub(padding)
}

/// Center-crop to a square and re-encode a small PNG for chip previews.
fn attachment_thumbnail(image: &PromptImage) -> Option<Arc<Image>> {
    let bytes = STANDARD.decode(&image.data).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let decoded = image::load_from_memory(&bytes).ok()?;
    let (width, height) = decoded.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let side = width.min(height);
    let x = (width - side) / 2;
    let y = (height - side) / 2;
    let square = decoded.crop_imm(x, y, side, side).resize_exact(
        ATTACHMENT_THUMB_PX,
        ATTACHMENT_THUMB_PX,
        image::imageops::FilterType::Triangle,
    );
    let mut encoded = std::io::Cursor::new(Vec::new());
    square
        .write_to(&mut encoded, image::ImageFormat::Png)
        .ok()?;
    let out = encoded.into_inner();
    (!out.is_empty()).then(|| Arc::new(Image::from_bytes(ImageFormat::Png, out)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerChrome {
    /// Full multiline desk composer with status + hints.
    Full,
    /// Secondary multiline panel input (compaction focus, etc.).
    Panel,
    /// Single-line sidebar field with inline submit.
    Field,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerAvailability {
    Unavailable,
    Idle,
    Running,
    Cancelling,
    BashRunning,
    BashCancelling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerFeedback {
    Ready,
    Pending(SubmissionKind),
    Accepted(SubmissionKind),
    BashRunning { exclude_from_context: bool },
    BashCompleted,
    LoadingAttachments,
    Rejected(String),
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerEvent {
    Accept {
        text: String,
        images: Vec<PromptImage>,
        files: Vec<PromptFile>,
    },
    FollowUp {
        text: String,
        images: Vec<PromptImage>,
        files: Vec<PromptFile>,
    },
    Abort,
    AbortBash,
    CommandNext,
    CommandPrevious,
    CommandAccept,
    CommandDismiss,
    PreviewImage(usize),
}

/// In-flight height animation for the multiline input shell.
#[derive(Debug, Clone, Copy)]
pub(super) struct InputHeightMotion {
    pub(super) generation: u64,
    pub(super) from: f32,
    pub(super) to: f32,
}

pub struct Composer {
    id_prefix: SharedString,
    action_label: SharedString,
    chrome: ComposerChrome,
    pub(super) focus_handle: FocusHandle,
    buffer: TextBuffer,
    images: Vec<PromptImage>,
    files: Vec<PromptFile>,
    /// Compressed square previews parallel to `images` (None when decode fails).
    thumbnails: Vec<Option<Arc<Image>>>,
    /// Stable ids for enter animations; bump only when a chip is newly attached.
    attach_tokens: Vec<u64>,
    file_attach_tokens: Vec<u64>,
    attach_seq: u64,
    /// Bumped when the attachment strip appears (0 → N) so the row can soft-enter.
    strip_motion_key: u64,
    image_bytes: usize,
    placeholder: SharedString,
    masked: bool,
    /// Optional override for Field chrome input height (defaults to 34).
    field_height: Option<f32>,
    allow_empty_submit: bool,
    disabled: bool,
    availability: ComposerAvailability,
    feedback: ComposerFeedback,
    is_selecting: bool,
    preferred_x: Option<Pixels>,
    scroll_y: Pixels,
    reveal_cursor: bool,
    last_layout: Option<EditorLayout>,
    command_completion_active: bool,
    /// Multiline chrome only: multi-line (focused / inputting) vs single-line (idle).
    input_expanded: bool,
    /// Root pins this while a prompt-owned floating sheet (model or thinking
    /// picker) owns focus, so blur cannot collapse the input beneath the sheet.
    height_hold: bool,
    /// User-pinned taller input; stays tall until toggled off (ignores blur collapse).
    input_enlarged: bool,
    input_height_motion: Option<InputHeightMotion>,
    input_height_motion_seq: u64,
    /// Keeps focus expand/collapse subscriptions alive for multiline chrome.
    _input_focus_subscriptions: Option<(Subscription, Subscription)>,
}

impl Composer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            id_prefix: "composer".into(),
            action_label: "Send".into(),
            chrome: ComposerChrome::Full,
            focus_handle: cx.focus_handle(),
            buffer: TextBuffer::default(),
            images: Vec::new(),
            files: Vec::new(),
            thumbnails: Vec::new(),
            attach_tokens: Vec::new(),
            file_attach_tokens: Vec::new(),
            attach_seq: 0,
            strip_motion_key: 0,
            image_bytes: 0,
            placeholder: "Message Pi…  @ file  / command  ! shell".into(),
            masked: false,
            field_height: None,
            allow_empty_submit: false,
            disabled: true,
            availability: ComposerAvailability::Unavailable,
            feedback: ComposerFeedback::Ready,
            is_selecting: false,
            preferred_x: None,
            scroll_y: Pixels::ZERO,
            reveal_cursor: true,
            last_layout: None,
            command_completion_active: false,
            // Multiline starts collapsed; focus tracking snaps/animates open when active.
            input_expanded: false,
            height_hold: false,
            input_enlarged: false,
            input_height_motion: None,
            input_height_motion_seq: 0,
            _input_focus_subscriptions: None,
        }
    }

    pub fn scoped(
        id: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        action_label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_chrome(id, placeholder, action_label, ComposerChrome::Panel, cx)
    }

    pub fn field(
        id: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        action_label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_chrome(id, placeholder, action_label, ComposerChrome::Field, cx)
    }

    pub fn secret_field(
        id: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        action_label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut composer =
            Self::with_chrome(id, placeholder, action_label, ComposerChrome::Field, cx);
        composer.masked = true;
        composer
    }

    pub fn with_field_height(mut self, height: f32) -> Self {
        self.field_height = Some(height);
        self
    }

    pub fn allowing_empty_submit(mut self) -> Self {
        self.allow_empty_submit = true;
        self
    }

    pub(super) fn field_height(&self) -> f32 {
        self.field_height.unwrap_or(34.0)
    }

    fn with_chrome(
        id: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        action_label: impl Into<SharedString>,
        chrome: ComposerChrome,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut composer = Self::new(cx);
        composer.id_prefix = id.into();
        composer.placeholder = placeholder.into();
        composer.action_label = action_label.into();
        composer.chrome = chrome;
        composer.availability = ComposerAvailability::Idle;
        composer.update_disabled();
        composer
    }

    pub fn chrome(&self) -> ComposerChrome {
        self.chrome
    }

    fn sanitize_input(&self, text: &str) -> String {
        if self.chrome == ComposerChrome::Field {
            text.replace(['\r', '\n'], " ")
        } else {
            text.to_owned()
        }
    }

    pub fn draft(&self) -> &str {
        self.buffer.text()
    }

    /// Byte offset of the active caret (grapheme-safe).
    pub fn cursor(&self) -> usize {
        self.buffer.cursor()
    }

    pub fn images(&self) -> &[PromptImage] {
        &self.images
    }

    pub fn files(&self) -> &[PromptFile] {
        &self.files
    }

    pub fn attachment_count(&self) -> usize {
        self.images.len().saturating_add(self.files.len())
    }

    pub fn can_add_attachments(&self) -> bool {
        self.chrome == ComposerChrome::Full
            && !self.disabled
            && self.attachment_count() < MAX_ATTACHMENTS
    }

    pub fn attachment_load_limits(&self) -> AttachmentLoadLimits {
        let existing_sources = self
            .images
            .iter()
            .filter_map(|image| image.source_path.clone())
            .chain(self.files.iter().map(|file| file.metadata.path.clone()))
            .collect();
        let snapshot_bytes = self
            .files
            .iter()
            .map(PromptFile::snapshot_bytes)
            .sum::<usize>();
        AttachmentLoadLimits {
            remaining_attachments: MAX_ATTACHMENTS.saturating_sub(self.attachment_count()),
            remaining_images: MAX_IMAGE_ATTACHMENTS.saturating_sub(self.images.len()),
            remaining_image_bytes: MAX_IMAGE_BYTES.saturating_sub(self.image_bytes),
            remaining_snapshot_bytes: MAX_TOTAL_TEXT_SNAPSHOT_BYTES.saturating_sub(snapshot_bytes),
            existing_sources,
        }
    }

    pub(super) fn thumbnail(&self, index: usize) -> Option<Arc<Image>> {
        self.thumbnails.get(index).and_then(|thumb| thumb.clone())
    }

    pub(super) fn attach_token(&self, index: usize) -> u64 {
        self.attach_tokens.get(index).copied().unwrap_or(0)
    }

    pub(super) fn file_attach_token(&self, index: usize) -> u64 {
        self.file_attach_tokens.get(index).copied().unwrap_or(0)
    }

    pub(super) fn strip_motion_key(&self) -> u64 {
        self.strip_motion_key
    }

    pub fn input_enlarged(&self) -> bool {
        self.input_enlarged
    }

    pub(super) fn input_height_motion(&self) -> Option<InputHeightMotion> {
        self.input_height_motion
    }

    /// Settled shell height for the current expand/enlarge state.
    pub(super) fn input_target_height(&self) -> f32 {
        if self.chrome == ComposerChrome::Field {
            return self.field_height();
        }
        let panel = self.chrome == ComposerChrome::Panel;
        let padding_y = 8.0;
        let line_height = if panel { 21.0 } else { 20.0 };
        let collapsed = line_height + padding_y * 2.0;
        let normal = if panel { 64.0 } else { 56.0 };
        // ~6 text rows: room for longer prompts without eating the whole stream.
        let enlarged = if panel { 128.0 } else { 152.0 };
        if self.input_enlarged {
            enlarged
        } else if self.input_expanded {
            normal
        } else {
            collapsed
        }
    }

    /// Pin or release the taller multiline input height.
    pub fn toggle_input_enlarged(&mut self, cx: &mut Context<Self>) {
        if self.chrome == ComposerChrome::Field {
            return;
        }
        // Prefer the last settled height if a blur-driven collapse is already mid-flight.
        let from = self.height_motion_origin();
        self.input_enlarged = !self.input_enlarged;
        // Enlarge control is height-only: root re-focuses the field after the click.
        // Always land on the multi-line shell so blur-before-click cannot target collapsed
        // height and cancel the motion into a snap.
        self.input_expanded = true;
        let to = self.input_target_height();
        self.begin_height_motion(from, to, cx);
    }

    /// Subscribe to focus so multiline chrome expands while inputting and collapses when idle.
    /// First install snaps to the current focus without animating (avoids a launch pop).
    pub(super) fn ensure_input_focus_tracking(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.chrome == ComposerChrome::Field || self._input_focus_subscriptions.is_some() {
            return;
        }

        let handle = self.focus_handle.clone();
        let on_focus = cx.on_focus(&handle, window, |this, _, cx| {
            this.set_input_expanded(true, cx);
        });
        let on_blur = cx.on_blur(&handle, window, |this, _, cx| {
            this.set_input_expanded(false, cx);
        });
        self._input_focus_subscriptions = Some((on_focus, on_blur));

        // Snap to whatever is already focused (root focuses the desk composer on open).
        self.input_expanded = self.focus_handle.is_focused(window);
        self.input_height_motion = None;
    }

    fn set_input_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.chrome == ComposerChrome::Field || self.input_expanded == expanded {
            return;
        }
        if self.height_hold && !expanded {
            // A prompt-owned sheet owns focus; stay expanded so panels anchored
            // above the prompt do not ride a resizing composer.
            return;
        }
        // Enlarged height is user-pinned; still track focus so leaving enlarge restores correctly.
        if self.input_enlarged {
            self.input_expanded = expanded;
            cx.notify();
            return;
        }
        let from = self.height_motion_origin();
        self.input_expanded = expanded;
        let to = self.input_target_height();
        self.begin_height_motion(from, to, cx);
    }

    /// Pin or release the expanded input while a prompt-owned sheet is open.
    ///
    /// Blur-driven collapse is suppressed during the hold; on release the input
    /// settles to whatever the current focus state implies.
    pub fn set_height_hold(&mut self, hold: bool, window: &Window, cx: &mut Context<Self>) {
        if self.chrome == ComposerChrome::Field || self.height_hold == hold {
            return;
        }
        self.height_hold = hold;
        if self.input_enlarged {
            cx.notify();
            return;
        }
        let expanded = hold || self.focus_handle.is_focused(window);
        if self.input_expanded == expanded {
            cx.notify();
            return;
        }
        let from = self.height_motion_origin();
        self.input_expanded = expanded;
        let to = self.input_target_height();
        self.begin_height_motion(from, to, cx);
    }

    /// Height to start a new motion from.
    /// If a blur-driven collapse is mid-flight, keep the pre-collapse height so a
    /// following enlarge toggle does not start from the trough and zap the shell.
    fn height_motion_origin(&self) -> f32 {
        match self.input_height_motion {
            Some(motion) if motion.to < motion.from => motion.from,
            _ => self.input_target_height(),
        }
    }

    fn begin_height_motion(&mut self, from: f32, to: f32, cx: &mut Context<Self>) {
        if (from - to).abs() < 0.5 {
            self.input_height_motion = None;
            cx.notify();
            return;
        }
        self.input_height_motion_seq = self.input_height_motion_seq.wrapping_add(1).max(1);
        let generation = self.input_height_motion_seq;
        self.input_height_motion = Some(InputHeightMotion {
            generation,
            from,
            to,
        });

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(INPUT_HEIGHT_MOTION_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .input_height_motion
                    .is_some_and(|motion| motion.generation == generation)
                {
                    this.input_height_motion = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn next_attach_token(&mut self) -> u64 {
        self.attach_seq = self.attach_seq.wrapping_add(1).max(1);
        self.attach_seq
    }

    fn note_strip_appeared(&mut self) {
        if !self.has_attachments() {
            self.strip_motion_key = self.strip_motion_key.wrapping_add(1).max(1);
        }
    }

    fn set_thumbnail_slot(&mut self, index: usize, thumb: Option<Arc<Image>>) {
        if let Some(slot) = self.thumbnails.get_mut(index) {
            *slot = thumb;
            return;
        }
        while self.thumbnails.len() < index {
            self.thumbnails.push(None);
        }
        self.thumbnails.push(thumb);
    }

    fn ensure_attach_token_slot(&mut self, index: usize) {
        while self.attach_tokens.len() <= index {
            let token = self.next_attach_token();
            self.attach_tokens.push(token);
        }
    }

    fn push_image_attachment(&mut self, prompt: PromptImage) {
        self.thumbnails.push(attachment_thumbnail(&prompt));
        let token = self.next_attach_token();
        self.attach_tokens.push(token);
        self.images.push(prompt);
    }

    fn push_file_attachment(&mut self, file: PromptFile) {
        let token = self.next_attach_token();
        self.file_attach_tokens.push(token);
        self.files.push(file);
    }

    pub fn replace_image(
        &mut self,
        index: usize,
        image: PromptImage,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        let Some(current) = self.images.get(index) else {
            return Err("The image is no longer attached.");
        };
        let current_bytes = decoded_image_len(&current.data);
        let replacement_bytes = decoded_image_len(&image.data);
        let next_total = self
            .image_bytes
            .saturating_sub(current_bytes)
            .saturating_add(replacement_bytes);
        if next_total > MAX_IMAGE_BYTES {
            return Err("The edited image would exceed the 5 MB attachment limit.");
        }

        let thumb = attachment_thumbnail(&image);
        self.images[index] = image;
        self.set_thumbnail_slot(index, thumb);
        // Keep attach_token stable so pencil edits do not re-pop the chip.
        self.ensure_attach_token_slot(index);
        self.image_bytes = next_total;
        cx.notify();
        Ok(())
    }

    pub fn has_images(&self) -> bool {
        !self.images.is_empty()
    }

    pub fn has_attachments(&self) -> bool {
        !self.images.is_empty() || !self.files.is_empty()
    }

    pub fn set_attachment_loading(&mut self, loading: bool, cx: &mut Context<Self>) {
        self.feedback = if loading {
            ComposerFeedback::LoadingAttachments
        } else {
            ComposerFeedback::Ready
        };
        self.update_disabled();
        cx.notify();
    }

    pub fn add_loaded_attachments(&mut self, batch: LoadedAttachmentBatch, cx: &mut Context<Self>) {
        let added = batch.attachments.len();
        for attachment in batch.attachments {
            self.note_strip_appeared();
            match attachment {
                LoadedAttachment::Image {
                    data,
                    mime_type,
                    file_name,
                    source_path,
                    bytes,
                } => {
                    self.image_bytes = self.image_bytes.saturating_add(bytes);
                    self.push_image_attachment(PromptImage {
                        data,
                        mime_type,
                        file_name: Some(file_name),
                        source_path: Some(source_path),
                    });
                }
                LoadedAttachment::File(file) => self.push_file_attachment(file),
            }
        }

        self.feedback = if let Some(issue) = batch.issues.first() {
            let prefix = if added == 0 {
                String::new()
            } else {
                format!("Added {added} file{}. ", if added == 1 { "" } else { "s" })
            };
            let extra = batch.issues.len().saturating_sub(1);
            ComposerFeedback::Rejected(format!(
                "{prefix}{}: {}{}",
                issue.name,
                issue.message,
                if extra == 0 {
                    String::new()
                } else {
                    format!(" ({extra} more skipped)")
                }
            ))
        } else {
            ComposerFeedback::Ready
        };
        self.update_disabled();
        cx.notify();
    }

    pub fn set_draft(&mut self, text: &str, cx: &mut Context<Self>) {
        let length = self.buffer.text().len();
        self.buffer.set_selection(0..length, false);
        if self.buffer.replace_selection(text) {
            self.after_edit(cx);
        }
    }

    /// Replace the full draft and place the caret at `cursor` (clamped).
    pub fn set_draft_with_cursor(&mut self, text: &str, cursor: usize, cx: &mut Context<Self>) {
        let length = self.buffer.text().len();
        self.buffer.set_selection(0..length, false);
        let _ = self.buffer.replace_selection(text);
        let cursor = cursor.min(self.buffer.text().len());
        self.buffer.move_to(cursor);
        self.after_edit(cx);
    }

    pub fn restore_draft(
        &mut self,
        text: &str,
        images: Vec<PromptImage>,
        files: Vec<PromptFile>,
        cx: &mut Context<Self>,
    ) {
        self.set_draft(text, cx);
        self.image_bytes = images
            .iter()
            .map(|image| decoded_image_len(&image.data))
            .sum();
        self.thumbnails = images.iter().map(attachment_thumbnail).collect();
        // Restored drafts should appear settled, not re-pop like a fresh selection.
        self.attach_tokens = images.iter().map(|_| 0).collect();
        self.file_attach_tokens = files.iter().map(|_| 0).collect();
        self.images = images;
        self.files = files;
        self.update_disabled();
        cx.notify();
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let placeholder = placeholder.into();
        if self.placeholder == placeholder {
            return;
        }
        self.placeholder = placeholder;
        cx.notify();
    }

    pub fn availability(&self) -> ComposerAvailability {
        self.availability
    }

    pub(crate) fn feedback(&self) -> &ComposerFeedback {
        &self.feedback
    }

    /// Whether the primary submit affordance is live right now.
    pub(crate) fn can_submit(&self) -> bool {
        !self.disabled
            && (self.allow_empty_submit
                || !self.buffer.text().trim().is_empty()
                || self.has_attachments())
    }

    pub fn set_availability(&mut self, availability: ComposerAvailability, cx: &mut Context<Self>) {
        if self.availability == availability {
            return;
        }
        self.availability = availability;
        self.update_disabled();
        cx.notify();
    }

    pub fn set_feedback(&mut self, feedback: ComposerFeedback, cx: &mut Context<Self>) {
        if self.feedback == feedback {
            return;
        }
        self.feedback = feedback;
        self.update_disabled();
        cx.notify();
    }

    /// When true, arrows / Enter / Escape route to completion menu events instead
    /// of caret motion or submit/abort. Shared by `/` command and `@` file menus.
    pub fn set_command_completion_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.command_completion_active != active {
            self.command_completion_active = active;
            cx.notify();
        }
    }

    pub fn command_completion_active(&self) -> bool {
        self.command_completion_active
    }

    pub fn clear_bash_accepted(
        &mut self,
        expected_draft: &str,
        exclude_from_context: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let cleared = self.buffer.clear_if_matches(expected_draft);
        self.feedback = ComposerFeedback::BashRunning {
            exclude_from_context,
        };
        self.update_disabled();
        if cleared {
            self.after_edit(cx);
        } else {
            cx.notify();
        }
        cleared
    }

    pub fn clear_accepted(
        &mut self,
        expected_draft: &str,
        kind: SubmissionKind,
        cx: &mut Context<Self>,
    ) -> bool {
        let draft_matches = self.buffer.text() == expected_draft;
        let text_cleared = self.buffer.clear_if_matches(expected_draft);
        let attachments_cleared = draft_matches && self.has_attachments();
        let cleared = text_cleared || attachments_cleared;
        self.feedback = ComposerFeedback::Accepted(kind);
        self.update_disabled();
        if cleared {
            self.images.clear();
            self.files.clear();
            self.thumbnails.clear();
            self.attach_tokens.clear();
            self.file_attach_tokens.clear();
            self.image_bytes = 0;
            self.after_edit(cx);
        } else {
            cx.notify();
        }
        cleared
    }

    fn update_disabled(&mut self) {
        self.disabled = matches!(
            self.availability,
            ComposerAvailability::Unavailable
                | ComposerAvailability::Cancelling
                | ComposerAvailability::BashRunning
                | ComposerAvailability::BashCancelling
        ) || matches!(
            self.feedback,
            ComposerFeedback::Pending(_)
                | ComposerFeedback::BashRunning { .. }
                | ComposerFeedback::LoadingAttachments
        );
    }

    fn backspace(&mut self, _: &ComposerBackspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.buffer.delete_backward() {
            self.after_edit(cx);
            window.refresh();
        }
    }

    fn delete_word_backward(
        &mut self,
        _: &ComposerDeleteWordBackward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        if self.buffer.delete_word_backward() {
            self.after_edit(cx);
            window.refresh();
        }
    }

    fn delete(&mut self, _: &ComposerDelete, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.buffer.delete_forward() {
            self.after_edit(cx);
            window.refresh();
        }
    }

    fn left(&mut self, _: &ComposerLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let target = if self.buffer.selection().is_empty() {
            self.buffer.previous_boundary(self.buffer.cursor())
        } else {
            self.buffer.selection().start
        };
        self.move_to(target, cx);
    }

    fn right(&mut self, _: &ComposerRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let target = if self.buffer.selection().is_empty() {
            self.buffer.next_boundary(self.buffer.cursor())
        } else {
            self.buffer.selection().end
        };
        self.move_to(target, cx);
    }

    fn select_left(&mut self, _: &ComposerSelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            let target = self.buffer.previous_boundary(self.buffer.cursor());
            self.select_to(target, cx);
        }
    }

    fn select_right(&mut self, _: &ComposerSelectRight, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            let target = self.buffer.next_boundary(self.buffer.cursor());
            self.select_to(target, cx);
        }
    }

    fn up(&mut self, _: &ComposerUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.command_completion_active {
            cx.emit(ComposerEvent::CommandPrevious);
            return;
        }
        self.move_vertical(-1, false, cx);
    }

    fn down(&mut self, _: &ComposerDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.command_completion_active {
            cx.emit(ComposerEvent::CommandNext);
            return;
        }
        self.move_vertical(1, false, cx);
    }

    fn select_up(&mut self, _: &ComposerSelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, true, cx);
    }

    fn select_down(&mut self, _: &ComposerSelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, true, cx);
    }

    fn line_start(&mut self, _: &ComposerLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.visual_line_range().start;
        self.move_to(target, cx);
    }

    fn line_end(&mut self, _: &ComposerLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.visual_line_range().end;
        self.move_to(target, cx);
    }

    fn select_line_start(
        &mut self,
        _: &ComposerSelectLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.visual_line_range().start;
        self.select_to(target, cx);
    }

    fn select_line_end(
        &mut self,
        _: &ComposerSelectLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.visual_line_range().end;
        self.select_to(target, cx);
    }

    fn select_all(&mut self, _: &ComposerSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            self.buffer.select_all();
            self.selection_changed(cx);
        }
    }

    fn copy(&mut self, _: &ComposerCopy, _: &mut Window, cx: &mut Context<Self>) {
        if self.masked {
            return;
        }
        if let Some(text) = self.buffer.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
        }
    }

    fn cut(&mut self, _: &ComposerCut, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled || self.masked {
            return;
        }
        self.copy(&ComposerCopy, window, cx);
        if !self.buffer.selection().is_empty() && self.buffer.replace_selection("") {
            self.after_edit(cx);
        }
    }

    fn paste(&mut self, _: &ComposerPaste, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if self.chrome == ComposerChrome::Full
            && let Some(image) = item.entries().iter().find_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(image),
                ClipboardEntry::String(_) => None,
            })
        {
            self.attach_image(image, cx);
            return;
        }
        if let Some(text) = item.text() {
            let text = self.sanitize_input(&text);
            if self.buffer.replace_selection(&text) {
                self.after_edit(cx);
            }
        }
    }

    fn attach_image(&mut self, image: &gpui::Image, cx: &mut Context<Self>) {
        if image.bytes.is_empty() {
            self.feedback = ComposerFeedback::Rejected(
                "The clipboard image is empty and could not be attached.".to_owned(),
            );
        } else if self.attachment_count() >= MAX_ATTACHMENTS {
            self.feedback = ComposerFeedback::Rejected(format!(
                "You can attach up to {MAX_ATTACHMENTS} files."
            ));
        } else if self.images.len() >= MAX_IMAGE_ATTACHMENTS {
            self.feedback = ComposerFeedback::Rejected(format!(
                "You can attach up to {MAX_IMAGE_ATTACHMENTS} images."
            ));
        } else if self.image_bytes.saturating_add(image.bytes.len()) > MAX_IMAGE_BYTES {
            self.feedback =
                ComposerFeedback::Rejected("Attached images must total 5 MB or less.".to_owned());
        } else {
            self.note_strip_appeared();
            self.image_bytes += image.bytes.len();
            self.push_image_attachment(PromptImage {
                data: STANDARD.encode(&image.bytes),
                mime_type: image.format.mime_type().to_owned(),
                file_name: None,
                source_path: None,
            });
        }
        cx.notify();
    }

    fn preview_image(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.images.len() {
            cx.emit(ComposerEvent::PreviewImage(index));
        }
    }

    fn remove_image(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.disabled || index >= self.images.len() {
            return;
        }
        let image = self.images.remove(index);
        if index < self.thumbnails.len() {
            self.thumbnails.remove(index);
        }
        if index < self.attach_tokens.len() {
            self.attach_tokens.remove(index);
        }
        self.image_bytes = self
            .image_bytes
            .saturating_sub(decoded_image_len(&image.data));
        cx.notify();
    }

    fn remove_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.disabled || index >= self.files.len() {
            return;
        }
        self.files.remove(index);
        if index < self.file_attach_tokens.len() {
            self.file_attach_tokens.remove(index);
        }
        cx.notify();
    }

    fn undo(&mut self, _: &ComposerUndo, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled && self.buffer.undo() {
            self.after_edit(cx);
        }
    }

    fn redo(&mut self, _: &ComposerRedo, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled && self.buffer.redo() {
            self.after_edit(cx);
        }
    }

    fn insert_newline(&mut self, _: &InsertNewline, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.chrome == ComposerChrome::Field {
            self.emit_accept(false, cx);
            return;
        }
        if self.buffer.replace_selection("\n") {
            self.after_edit(cx);
        }
    }

    fn accept(&mut self, _: &AcceptInput, _: &mut Window, cx: &mut Context<Self>) {
        self.emit_accept(false, cx);
    }

    fn follow_up(&mut self, _: &QueueFollowUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.chrome == ComposerChrome::Field {
            self.emit_accept(false, cx);
            return;
        }
        self.emit_accept(true, cx);
    }

    fn abort(&mut self, _: &AbortRun, _: &mut Window, cx: &mut Context<Self>) {
        self.request_abort(cx);
    }

    /// Esc/abort affordance shared by the key handler and tray controls.
    pub(crate) fn request_abort(&mut self, cx: &mut Context<Self>) {
        if self.command_completion_active {
            cx.emit(ComposerEvent::CommandDismiss);
            return;
        }
        match self.availability {
            ComposerAvailability::Running | ComposerAvailability::Cancelling => {
                cx.emit(ComposerEvent::Abort);
            }
            ComposerAvailability::BashRunning | ComposerAvailability::BashCancelling => {
                cx.emit(ComposerEvent::AbortBash);
            }
            ComposerAvailability::Unavailable | ComposerAvailability::Idle => {}
        }
    }

    pub(crate) fn emit_accept(&mut self, follow_up: bool, cx: &mut Context<Self>) {
        if self.command_completion_active && !self.has_attachments() {
            cx.emit(ComposerEvent::CommandAccept);
            return;
        }
        if self.disabled {
            return;
        }
        // Live filter fields have no submit action; Enter is a no-op.
        if self.chrome == ComposerChrome::Field && self.action_label.is_empty() {
            return;
        }
        if !self.allow_empty_submit
            && self.buffer.text().trim().is_empty()
            && !self.has_attachments()
        {
            self.feedback = ComposerFeedback::Rejected(match self.chrome {
                ComposerChrome::Full => "Write a prompt or attach a file first.".to_owned(),
                ComposerChrome::Panel | ComposerChrome::Field => "Enter a value first.".to_owned(),
            });
            cx.notify();
            return;
        }
        let text = self.buffer.text().to_owned();
        let images = self.images.clone();
        let files = self.files.clone();
        if follow_up {
            if self.availability != ComposerAvailability::Running {
                self.feedback = ComposerFeedback::Rejected(
                    "Follow-ups can be queued while Pi is running.".to_owned(),
                );
                cx.notify();
                return;
            }
            cx.emit(ComposerEvent::FollowUp {
                text,
                images,
                files,
            });
        } else {
            cx.emit(ComposerEvent::Accept {
                text,
                images,
                files,
            });
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let offset = self.index_for_position(event.position);
        if event.modifiers.shift {
            self.buffer.select_to(offset);
        } else {
            self.buffer.move_to(offset);
        }
        self.selection_changed(cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting && !self.disabled {
            let offset = self.index_for_position(event.position);
            self.buffer.select_to(offset);
            self.selection_changed(cx);
        }
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.last_layout.as_ref() else {
            return;
        };
        let max_scroll = (layout.content_height - layout.bounds.size.height).max(Pixels::ZERO);
        let delta = event.delta.pixel_delta(window.line_height()).y;
        let next = (self.scroll_y - delta).clamp(Pixels::ZERO, max_scroll);
        if next != self.scroll_y {
            self.scroll_y = next;
            self.reveal_cursor = false;
            cx.notify();
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.buffer.move_to(offset);
        self.selection_changed(cx);
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.buffer.select_to(offset);
        self.selection_changed(cx);
    }

    fn move_vertical(&mut self, direction: isize, select: bool, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let Some(layout) = self.last_layout.as_ref() else {
            return;
        };
        let cursor = self.buffer.cursor();
        let position = layout.position_for_offset(cursor);
        let preferred_x = self
            .preferred_x
            .get_or_insert(position.x - layout.bounds.left());
        let target = layout.offset_for_vertical_move(cursor, *preferred_x, direction);
        if select {
            self.buffer.select_to(target);
        } else {
            self.buffer.move_to(target);
        }
        self.reveal_cursor = true;
        cx.notify();
    }

    fn visual_line_range(&self) -> Range<usize> {
        self.last_layout.as_ref().map_or_else(
            || {
                self.buffer.hard_line_start(self.buffer.cursor())
                    ..self.buffer.hard_line_end(self.buffer.cursor())
            },
            |layout| layout.visual_line_range(self.buffer.cursor()),
        )
    }

    fn index_for_position(&self, position: gpui::Point<Pixels>) -> usize {
        self.last_layout
            .as_ref()
            .map_or(0, |layout| layout.index_for_position(position))
    }

    fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.reveal_cursor = true;
        cx.notify();
    }

    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.reveal_cursor = true;
        cx.notify();
    }

    pub(crate) fn status_text(&self) -> String {
        match &self.feedback {
            ComposerFeedback::Pending(SubmissionKind::Prompt) => match self.chrome {
                ComposerChrome::Full => "Sending to Pi…".to_owned(),
                ComposerChrome::Panel | ComposerChrome::Field => "Working…".to_owned(),
            },
            ComposerFeedback::Pending(SubmissionKind::Steer) => {
                "Sending steering input…".to_owned()
            }
            ComposerFeedback::Pending(SubmissionKind::FollowUp) => "Queueing follow-up…".to_owned(),
            ComposerFeedback::Accepted(SubmissionKind::Prompt) => match self.chrome {
                ComposerChrome::Full => "Prompt accepted.".to_owned(),
                ComposerChrome::Panel | ComposerChrome::Field => "Done.".to_owned(),
            },
            ComposerFeedback::Accepted(SubmissionKind::Steer) => {
                "Steering input accepted.".to_owned()
            }
            ComposerFeedback::Accepted(SubmissionKind::FollowUp) => "Follow-up queued.".to_owned(),
            ComposerFeedback::BashRunning {
                exclude_from_context: true,
            } => "Bash is running outside model context.".to_owned(),
            ComposerFeedback::BashRunning {
                exclude_from_context: false,
            } => "Bash is running.".to_owned(),
            ComposerFeedback::BashCompleted => "Bash finished.".to_owned(),
            ComposerFeedback::LoadingAttachments => "Adding files…".to_owned(),
            ComposerFeedback::Rejected(summary) => summary.clone(),
            ComposerFeedback::Uncertain => {
                "Delivery is uncertain. The draft was kept; reconnect before retrying.".to_owned()
            }
            ComposerFeedback::Ready => match (self.chrome, self.availability) {
                (ComposerChrome::Field | ComposerChrome::Panel, ComposerAvailability::Idle) => {
                    String::new()
                }
                (
                    ComposerChrome::Field | ComposerChrome::Panel,
                    ComposerAvailability::Unavailable,
                ) => "Unavailable".to_owned(),
                (_, ComposerAvailability::Unavailable) => {
                    "Connect Pi to start composing.".to_owned()
                }
                (_, ComposerAvailability::Idle) => "Ready to send.".to_owned(),
                (_, ComposerAvailability::Running) => {
                    "Pi is running. Steer or queue a follow-up.".to_owned()
                }
                (_, ComposerAvailability::Cancelling) => "Waiting for Pi to settle…".to_owned(),
                (_, ComposerAvailability::BashRunning) => "Bash is running.".to_owned(),
                (_, ComposerAvailability::BashCancelling) => "Cancelling Bash…".to_owned(),
            },
        }
    }

    pub(crate) fn hint_text(&self) -> &'static str {
        match self.availability {
            ComposerAvailability::Running => {
                "Enter steer · Alt+Enter follow up · Shift+Enter newline · Ctrl+O files · Esc abort"
            }
            ComposerAvailability::Idle => {
                "Enter send · Shift+Enter newline · Ctrl+O files · Ctrl+V image"
            }
            ComposerAvailability::BashRunning => "Esc aborts Bash only",
            ComposerAvailability::BashCancelling => "Waiting for Bash to stop",
            ComposerAvailability::Unavailable | ComposerAvailability::Cancelling => {
                "Draft stays on this device"
            }
        }
    }
}

impl EventEmitter<ComposerEvent> for Composer {}

impl Focusable for Composer {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for Composer {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let (text, adjusted) = self.buffer.text_for_utf16_range(&range_utf16);
        actual_range.replace(adjusted);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.disabled && !ignore_disabled_input {
            return None;
        }
        Some(UTF16Selection {
            range: self.buffer.range_to_utf16(self.buffer.selection()),
            reversed: self.buffer.selection_reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer
            .marked_range()
            .map(|range| self.buffer.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.unmark_text();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.disabled {
            let text = self.sanitize_input(text);
            self.buffer.replace_text_utf16(range_utf16, &text);
            self.after_edit(cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.disabled {
            let new_text = self.sanitize_input(new_text);
            self.buffer
                .replace_and_mark_text_utf16(range_utf16, &new_text, selected_utf16);
            self.after_edit(cx);
        }
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: gpui::Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<gpui::Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.buffer.range_from_utf16(&range_utf16);
        let start = layout.position_for_offset(range.start);
        let end = layout.position_for_offset(range.end);
        let right = if start.y == end.y {
            end.x.max(start.x + px(2.0))
        } else {
            start.x + px(2.0)
        };
        Some(gpui::Bounds::from_corners(
            start,
            gpui::point(right, start.y + layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let byte = self.buffer.nearest_boundary(self.index_for_position(point));
        Some(self.buffer.offset_to_utf16(byte))
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_input_focus_tracking(window, cx);
        self.render_view(cx)
    }
}
