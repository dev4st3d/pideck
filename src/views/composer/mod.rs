mod buffer;
mod element;
mod render;
#[cfg(test)]
mod tests;

use std::ops::Range;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui::{
    ClipboardEntry, ClipboardItem, Context, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, IntoElement, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Render,
    ScrollWheelEvent, SharedString, UTF16Selection, Window, px,
};

use self::buffer::TextBuffer;
use self::element::EditorLayout;
use crate::actions::{
    AbortRun, AcceptInput, ComposerBackspace, ComposerCopy, ComposerCut, ComposerDelete,
    ComposerDown, ComposerLeft, ComposerLineEnd, ComposerLineStart, ComposerPaste, ComposerRedo,
    ComposerRight, ComposerSelectAll, ComposerSelectDown, ComposerSelectLeft,
    ComposerSelectLineEnd, ComposerSelectLineStart, ComposerSelectRight, ComposerSelectUp,
    ComposerUndo, ComposerUp, InsertNewline, QueueFollowUp,
};
use crate::state::runtime::{PromptImage, SubmissionKind};

const MAX_IMAGE_ATTACHMENTS: usize = 4;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

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
    Rejected(String),
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerEvent {
    Accept {
        text: String,
        images: Vec<PromptImage>,
    },
    FollowUp {
        text: String,
        images: Vec<PromptImage>,
    },
    Abort,
    AbortBash,
    CommandNext,
    CommandPrevious,
    CommandAccept,
    CommandDismiss,
    PreviewImage(usize),
}

pub struct Composer {
    id_prefix: SharedString,
    action_label: SharedString,
    chrome: ComposerChrome,
    pub(super) focus_handle: FocusHandle,
    buffer: TextBuffer,
    images: Vec<PromptImage>,
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

    pub fn images(&self) -> &[PromptImage] {
        &self.images
    }

    pub fn has_images(&self) -> bool {
        !self.images.is_empty()
    }

    pub fn set_draft(&mut self, text: &str, cx: &mut Context<Self>) {
        let length = self.buffer.text().len();
        self.buffer.set_selection(0..length, false);
        if self.buffer.replace_selection(text) {
            self.after_edit(cx);
        }
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    pub fn availability(&self) -> ComposerAvailability {
        self.availability
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

    pub fn set_command_completion_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.command_completion_active != active {
            self.command_completion_active = active;
            cx.notify();
        }
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
        let images_cleared = draft_matches && !self.images.is_empty();
        let cleared = text_cleared || images_cleared;
        self.feedback = ComposerFeedback::Accepted(kind);
        self.update_disabled();
        if cleared {
            self.images.clear();
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
            ComposerFeedback::Pending(_) | ComposerFeedback::BashRunning { .. }
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
        } else if self.images.len() >= MAX_IMAGE_ATTACHMENTS {
            self.feedback = ComposerFeedback::Rejected(format!(
                "You can attach up to {MAX_IMAGE_ATTACHMENTS} images."
            ));
        } else if self.image_bytes.saturating_add(image.bytes.len()) > MAX_IMAGE_BYTES {
            self.feedback =
                ComposerFeedback::Rejected("Attached images must total 5 MB or less.".to_owned());
        } else {
            self.image_bytes += image.bytes.len();
            self.images.push(PromptImage {
                data: STANDARD.encode(&image.bytes),
                mime_type: image.format.mime_type().to_owned(),
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
        self.image_bytes = self
            .image_bytes
            .saturating_sub(decoded_image_len(&image.data));
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

    fn emit_accept(&mut self, follow_up: bool, cx: &mut Context<Self>) {
        if self.command_completion_active && self.images.is_empty() {
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
            && self.images.is_empty()
        {
            self.feedback = ComposerFeedback::Rejected(match self.chrome {
                ComposerChrome::Full => "Write a prompt or attach an image first.".to_owned(),
                ComposerChrome::Panel | ComposerChrome::Field => "Enter a value first.".to_owned(),
            });
            cx.notify();
            return;
        }
        let text = self.buffer.text().to_owned();
        let images = self.images.clone();
        if follow_up {
            if self.availability != ComposerAvailability::Running {
                self.feedback = ComposerFeedback::Rejected(
                    "Follow-ups can be queued while Pi is running.".to_owned(),
                );
                cx.notify();
                return;
            }
            cx.emit(ComposerEvent::FollowUp { text, images });
        } else {
            cx.emit(ComposerEvent::Accept { text, images });
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

    fn status_text(&self) -> String {
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

    fn hint_text(&self) -> &'static str {
        match self.availability {
            ComposerAvailability::Running => {
                "Enter steer · Alt+Enter follow up · Shift+Enter newline · Ctrl+V image · Esc abort"
            }
            ComposerAvailability::Idle => "Enter send · Shift+Enter newline · Ctrl+V image",
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_view(cx)
    }
}
