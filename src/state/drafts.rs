//! Durable, app-owned editor state. This is never a Pi session or a send queue.

use std::collections::VecDeque;
use serde::{Deserialize, Serialize};
use crate::attachments::{MAX_ATTACHMENTS, MAX_IMAGE_ATTACHMENTS, MAX_IMAGE_BYTES, MAX_TOTAL_TEXT_SNAPSHOT_BYTES, PromptFile};
use super::editor::TextBuffer;
use super::runtime::{PromptImage, RecoveredInput};

/// Both components are monotonic within one editor session. Text equality is
/// deliberately insufficient: edit/undo and attachment replacement are edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DraftRevision {
    pub text: u64,
    pub attachments: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorStamp {
    pub revision: DraftRevision,
    pub selection: std::ops::Range<usize>,
    pub reversed: bool,
    pub scroll: u32,
    pub preferred_x: Option<u32>,
    pub enlarged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredEditor {
    pub buffer: TextBuffer,
    pub attachment_revision: u64,
    pub scroll_y: f32,
    pub preferred_x: Option<f32>,
    pub enlarged: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct StoredScroll {
    pub item: usize,
    pub offset: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredDraft {
    pub editor: StoredEditor,
    pub images: Vec<PromptImage>,
    pub files: Vec<PromptFile>,
    pub scroll: Option<StoredScroll>,
    pub following: bool,
    pub saved_inputs: VecDeque<RecoveredInput>,
    /// A local note only. Never restore pending request ids or replay a prompt.
    pub acceptance_uncertain: bool,
}

impl StoredDraft {
    /// Parsing and journal validation belong on the persistence worker.
    pub(crate) fn validate(&mut self) -> Result<(), &'static str> {
        validate_attachments(&self.images, &self.files)?;
        for input in &self.saved_inputs {
            validate_attachments(&input.images, &input.files)?;
        }
        self.editor.buffer.sanitize_restored();
        self.editor.scroll_y = finite_offset(self.editor.scroll_y);
        self.editor.preferred_x = self.editor.preferred_x.filter(|x| x.is_finite());
        if let Some(scroll) = self.scroll.as_mut() {
            scroll.offset = finite_offset(scroll.offset);
        }
        Ok(())
    }

    pub(crate) fn into_recovered_input(self) -> RecoveredInput {
        RecoveredInput {
            text: self.editor.buffer.text().to_owned(),
            images: self.images,
            files: self.files,
        }
    }
}

fn finite_offset(value: f32) -> f32 {
    if value.is_finite() { value.max(0.0) } else { 0.0 }
}

fn validate_attachments(images: &[PromptImage], files: &[PromptFile]) -> Result<(), &'static str> {
    let encoded_limit = MAX_IMAGE_BYTES.div_ceil(3) * 4 + 4 * MAX_IMAGE_ATTACHMENTS;
    if images.len() > MAX_IMAGE_ATTACHMENTS || images.len() + files.len() > MAX_ATTACHMENTS {
        return Err("Saved draft exceeds the attachment count limit; the original file was kept.");
    }
    if images.iter().map(|image| image.data.len()).sum::<usize>() > encoded_limit
        || files.iter().map(PromptFile::snapshot_bytes).sum::<usize>() > MAX_TOTAL_TEXT_SNAPSHOT_BYTES {
        return Err("Saved draft exceeds the attachment size limit; the original file was kept.");
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn draft(text: &str) -> StoredDraft {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection(text);
        StoredDraft {
            editor: StoredEditor { buffer, attachment_revision: 0, scroll_y: 0.0,
                preferred_x: None, enlarged: false },
            images: vec![], files: vec![], scroll: None, following: true,
            saved_inputs: VecDeque::new(), acceptance_uncertain: false,
        }
    }

    #[test]
    fn persisted_editor_retains_selection_and_reversible_history() {
        let mut original = draft("first");
        original.editor.buffer.replace_selection(" second");
        original.editor.buffer.set_selection(0..5, true);
        let bytes = serde_json::to_vec(&original).unwrap();
        let mut restored: StoredDraft = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.editor.buffer.selected_text(), Some("first"));
        assert!(restored.editor.buffer.undo());
        assert_eq!(restored.editor.buffer.text(), "first");
    }

    #[test]
    fn recovery_data_cannot_request_execution() {
        let mut original = draft("do not replay me");
        original.acceptance_uncertain = true;
        let value = serde_json::to_value(&original).unwrap();
        assert!(value.get("request").is_none());
        assert!(value.get("command").is_none());
        assert!(value.get("send").is_none());
        assert_eq!(original.into_recovered_input().text, "do not replay me");
    }

    #[test]
    fn hostile_attachment_count_is_rejected_without_silent_truncation() {
        let mut original = draft("preserve me");
        original.images = vec![PromptImage { data: "".into(), mime_type: "image/png".into(),
            file_name: None, source_path: None }; MAX_IMAGE_ATTACHMENTS + 1];
        assert!(original.validate().is_err());
        assert_eq!(original.editor.buffer.text(), "preserve me");
    }
}
