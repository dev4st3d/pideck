//! UI-independent Unicode editor with a bounded, reversible edit journal.
//!
//! Keystrokes retain the changed span, not 128 copies of the complete document.
//! Arc-backed payloads make session snapshots cheap without sharing mutable state.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

const MAX_UNDO_DEPTH: usize = 128;
const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Selection {
    range: Range<usize>,
    reversed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Edit {
    start: usize,
    deleted: Arc<str>,
    inserted: Arc<str>,
    before: Selection,
    after: Selection,
}

impl Edit {
    fn payload_bytes(&self) -> usize {
        self.deleted.len().saturating_add(self.inserted.len())
    }
}

#[derive(Debug, Clone)]
struct Composition {
    text: Arc<String>,
    selection: Selection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TextBuffer {
    text: Arc<String>,
    selection: Range<usize>,
    selection_reversed: bool,
    #[serde(skip)]
    marked_range: Option<Range<usize>>,
    #[serde(skip)]
    composition: Option<Composition>,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    revision: u64,
    #[serde(skip)]
    history_bytes: usize,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self {
            text: Arc::new(String::new()),
            selection: 0..0,
            selection_reversed: false,
            marked_range: None,
            composition: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            revision: 0,
            history_bytes: 0,
        }
    }
}

impl TextBuffer {
    pub(crate) fn text(&self) -> &str {
        self.text.as_str()
    }

    /// Content identity, not a text hash: undoing and retyping the same prompt
    /// must not allow a late acceptance to clear that newer edit.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn selection(&self) -> &Range<usize> {
        &self.selection
    }

    pub(crate) fn selection_reversed(&self) -> bool {
        self.selection_reversed
    }

    pub(crate) fn marked_range(&self) -> Option<&Range<usize>> {
        self.marked_range.as_ref()
    }

    pub(crate) fn cursor(&self) -> usize {
        if self.selection_reversed { self.selection.start } else { self.selection.end }
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(crate) fn previous_boundary(&self, offset: usize) -> usize {
        let offset = self.clamp_char_boundary(offset);
        GraphemeCursor::new(offset, self.text.len(), true)
            .prev_boundary(self.text(), 0)
            .ok().flatten().unwrap_or(0)
    }

    pub(crate) fn next_boundary(&self, offset: usize) -> usize {
        let offset = self.clamp_char_boundary(offset);
        GraphemeCursor::new(offset, self.text.len(), true)
            .next_boundary(self.text(), 0)
            .ok().flatten().unwrap_or(self.text.len())
    }

    pub(crate) fn nearest_boundary(&self, offset: usize) -> usize {
        let offset = self.clamp_char_boundary(offset);
        let mut cursor = GraphemeCursor::new(offset, self.text.len(), true);
        if cursor.is_boundary(self.text(), 0).unwrap_or(false) {
            return offset;
        }
        let previous = self.previous_boundary(offset);
        let next = self.next_boundary(offset);
        if offset - previous <= next - offset { previous } else { next }
    }

    pub(crate) fn move_to(&mut self, offset: usize) {
        let offset = self.nearest_boundary(offset);
        self.selection = offset..offset;
        self.selection_reversed = false;
    }

    pub(crate) fn select_to(&mut self, offset: usize) {
        let offset = self.nearest_boundary(offset);
        let anchor = if self.selection_reversed { self.selection.end } else { self.selection.start };
        self.selection = anchor.min(offset)..anchor.max(offset);
        self.selection_reversed = offset < anchor;
    }

    pub(crate) fn select_all(&mut self) {
        self.selection = 0..self.text.len();
        self.selection_reversed = false;
    }

    pub(crate) fn set_selection(&mut self, range: Range<usize>, reversed: bool) {
        let start = self.clamp_char_boundary(range.start);
        let end = self.clamp_char_boundary(range.end);
        self.selection = start.min(end)..start.max(end);
        self.selection_reversed = reversed && !self.selection.is_empty();
    }

    pub(crate) fn replace_selection(&mut self, text: &str) -> bool {
        self.replace_bytes(self.selection.clone(), text, true)
    }

    pub(crate) fn delete_backward(&mut self) -> bool {
        let range = if self.selection.is_empty() {
            self.previous_boundary(self.cursor())..self.cursor()
        } else {
            self.selection.clone()
        };
        self.replace_bytes(range, "", true)
    }

    pub(crate) fn delete_word_backward(&mut self) -> bool {
        let range = if self.selection.is_empty() {
            let cursor = self.cursor();
            let mut start = cursor;
            let mut segments = self.text[..cursor].split_word_bound_indices().rev().peekable();
            while let Some((index, _)) =
                segments.next_if(|(_, segment)| segment.chars().all(char::is_whitespace))
            {
                start = index;
            }
            if let Some((index, _)) = segments.next() {
                start = index;
            }
            start..cursor
        } else {
            self.selection.clone()
        };
        self.replace_bytes(range, "", true)
    }

    pub(crate) fn delete_forward(&mut self) -> bool {
        let range = if self.selection.is_empty() {
            self.cursor()..self.next_boundary(self.cursor())
        } else {
            self.selection.clone()
        };
        self.replace_bytes(range, "", true)
    }

    pub(crate) fn replace_text_utf16(&mut self, range_utf16: Option<Range<usize>>, text: &str) -> bool {
        let range = range_utf16.as_ref().map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone()).unwrap_or_else(|| self.selection.clone());
        let composing = self.composition.is_some();
        let changed = self.replace_bytes(range, text, !composing);
        self.marked_range = None;
        if composing { self.finish_composition(); }
        changed
    }

    pub(crate) fn replace_and_mark_text_utf16(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        selected_utf16: Option<Range<usize>>,
    ) -> bool {
        let range = range_utf16.as_ref().map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone()).unwrap_or_else(|| self.selection.clone());
        if self.composition.is_none() {
            self.composition = Some(Composition { text: self.text.clone(), selection: self.current_selection() });
        }
        let start = range.start;
        let normalized = normalize_newlines(new_text);
        let changed = self.replace_bytes(range, &normalized, false);
        let inserted_end = start + normalized.len();
        self.marked_range = (!normalized.is_empty()).then_some(start..inserted_end);
        if let Some(selected) = selected_utf16 {
            // IME offsets address its unnormalised text, including CRLF pairs.
            let relative_start = normalize_newlines(&new_text[..utf16_to_byte(new_text, selected.start)]).len();
            let relative_end = normalize_newlines(&new_text[..utf16_to_byte(new_text, selected.end)]).len();
            self.set_selection(start + relative_start..start + relative_end, selected.start > selected.end);
        } else {
            self.selection = inserted_end..inserted_end;
            self.selection_reversed = false;
        }
        changed
    }

    pub(crate) fn unmark_text(&mut self) {
        self.marked_range = None;
        self.finish_composition();
    }

    pub(crate) fn undo(&mut self) -> bool {
        self.unmark_text();
        let Some(edit) = self.undo.pop_back() else { return false; };
        Arc::make_mut(&mut self.text).replace_range(edit.start..edit.start + edit.inserted.len(), &edit.deleted);
        self.restore_selection(&edit.before);
        self.revision = self.revision.wrapping_add(1);
        self.redo.push(edit);
        true
    }

    pub(crate) fn redo(&mut self) -> bool {
        self.unmark_text();
        let Some(edit) = self.redo.pop() else { return false; };
        Arc::make_mut(&mut self.text).replace_range(edit.start..edit.start + edit.deleted.len(), &edit.inserted);
        self.restore_selection(&edit.after);
        self.revision = self.revision.wrapping_add(1);
        self.undo.push_back(edit);
        true
    }

    pub(crate) fn clear_if_matches(&mut self, expected: &str) -> bool {
        if self.text() != expected { return false; }
        self.replace_bytes(0..self.text.len(), "", true)
    }

    pub(crate) fn selected_text(&self) -> Option<&str> {
        (!self.selection.is_empty()).then(|| &self.text[self.selection.clone()])
    }

    pub(crate) fn text_for_utf16_range(&self, range_utf16: &Range<usize>) -> (String, Range<usize>) {
        let range = self.range_from_utf16(range_utf16);
        (self.text[range.clone()].to_owned(), self.range_to_utf16(&range))
    }

    pub(crate) fn offset_from_utf16(&self, offset: usize) -> usize { utf16_to_byte(self.text(), offset) }
    pub(crate) fn offset_to_utf16(&self, offset: usize) -> usize { byte_to_utf16(self.text(), offset) }

    pub(crate) fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let start = self.offset_from_utf16(range.start);
        let end = self.offset_from_utf16(range.end);
        start.min(end)..start.max(end)
    }

    pub(crate) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub(crate) fn hard_line_start(&self, offset: usize) -> usize {
        self.text[..self.clamp_char_boundary(offset)].rfind('\n').map_or(0, |index| index + 1)
    }

    pub(crate) fn hard_line_end(&self, offset: usize) -> usize {
        let offset = self.clamp_char_boundary(offset);
        self.text[offset..].find('\n').map_or(self.text.len(), |index| offset + index)
    }

    fn replace_bytes(&mut self, range: Range<usize>, text: &str, record_undo: bool) -> bool {
        if record_undo { self.unmark_text(); }
        let start = self.clamp_char_boundary(range.start);
        let end = self.clamp_char_boundary(range.end);
        let range = start.min(end)..start.max(end);
        let normalized = normalize_newlines(text);
        if self.text[range.clone()] == *normalized && (range.is_empty() || !record_undo) {
            self.selection = range.end..range.end;
            self.selection_reversed = false;
            return false;
        }
        let before = self.current_selection();
        let deleted = record_undo.then(|| Arc::<str>::from(&self.text[range.clone()]));
        Arc::make_mut(&mut self.text).replace_range(range.clone(), &normalized);
        let cursor = range.start + normalized.len();
        self.selection = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.revision = self.revision.wrapping_add(1);
        if let Some(deleted) = deleted {
            self.push_edit(Edit {
                start: range.start, deleted, inserted: Arc::from(normalized.as_ref()),
                before, after: self.current_selection(),
            });
        }
        true
    }

    fn current_selection(&self) -> Selection {
        Selection { range: self.selection.clone(), reversed: self.selection_reversed }
    }

    fn restore_selection(&mut self, selection: &Selection) {
        self.set_selection(selection.range.clone(), selection.reversed);
    }

    fn push_edit(&mut self, edit: Edit) {
        self.history_bytes = self.history_bytes.saturating_sub(self.redo.iter().map(Edit::payload_bytes).sum());
        self.redo.clear();
        self.history_bytes = self.history_bytes.saturating_add(edit.payload_bytes());
        self.undo.push_back(edit);
        // Retain at least the last operation, even if a single paste exceeds the
        // normal budget. Thus retained payload <= budget + one largest operation.
        while self.undo.len() > 1 && (self.undo.len() > MAX_UNDO_DEPTH || self.history_bytes > MAX_HISTORY_BYTES) {
            if let Some(old) = self.undo.pop_front() {
                self.history_bytes = self.history_bytes.saturating_sub(old.payload_bytes());
            }
        }
    }

    fn finish_composition(&mut self) {
        let Some(composition) = self.composition.take() else { return; };
        if composition.text == self.text { return; }
        let old = composition.text.as_str();
        let new = self.text();
        let mut start = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
        while !old.is_char_boundary(start) || !new.is_char_boundary(start) { start -= 1; }
        let mut suffix = old[start..].bytes().rev().zip(new[start..].bytes().rev()).take_while(|(a, b)| a == b).count();
        while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) { suffix -= 1; }
        let edit = Edit {
            start,
            deleted: Arc::from(&old[start..old.len() - suffix]),
            inserted: Arc::from(&new[start..new.len() - suffix]),
            before: composition.selection,
            after: self.current_selection(),
        };
        self.push_edit(edit);
    }

    /// Untrusted local state may retain its text, but cannot inject byte offsets
    /// that later panic in undo. Validate the entire reversible chain off-thread.
    pub(crate) fn sanitize_restored(&mut self) -> bool {
        self.marked_range = None;
        self.composition = None;
        self.set_selection(self.selection.clone(), self.selection_reversed);
        let count = self.undo.len() + self.redo.len();
        let bytes: usize = self.undo.iter().chain(self.redo.iter()).map(Edit::payload_bytes).sum();
        let valid = count <= MAX_UNDO_DEPTH
            && (bytes <= MAX_HISTORY_BYTES || count <= 1)
            && validate_chain(self.text(), self.undo.iter().rev(), true)
            && validate_chain(self.text(), self.redo.iter().rev(), false);
        if !valid { self.undo.clear(); self.redo.clear(); }
        self.history_bytes = self.undo.iter().chain(self.redo.iter()).map(Edit::payload_bytes).sum();
        valid
    }

    fn clamp_char_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) { offset = offset.saturating_sub(1); }
        offset
    }
}

fn validate_chain<'a>(text: &str, edits: impl Iterator<Item = &'a Edit>, undo: bool) -> bool {
    let mut text = text.to_owned();
    for edit in edits {
        let (expected, replacement, selection) = if undo {
            (&edit.inserted, &edit.deleted, &edit.before)
        } else {
            (&edit.deleted, &edit.inserted, &edit.after)
        };
        let Some(end) = edit.start.checked_add(expected.len()) else { return false; };
        if text.get(edit.start..end) != Some(expected.as_ref()) { return false; }
        text.replace_range(edit.start..end, replacement);
        if selection.range.start > selection.range.end
            || text.get(selection.range.clone()).is_none() { return false; }
    }
    true
}

fn normalize_newlines(text: &str) -> Cow<'_, str> {
    if text.contains('\r') { Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n")) }
    else { Cow::Borrowed(text) }
}

fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, character) in text.char_indices() {
        if utf16 >= offset { return byte; }
        let next = utf16 + character.len_utf16();
        if next > offset { return byte; }
        utf16 = next;
    }
    text.len()
}

fn byte_to_utf16(text: &str, offset: usize) -> usize {
    let mut byte_offset = offset.min(text.len());
    while !text.is_char_boundary(byte_offset) { byte_offset = byte_offset.saturating_sub(1); }
    text[..byte_offset].encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_selected_text_with_the_same_text_still_changes_identity() {
        let mut editor = TextBuffer::default();
        editor.replace_selection("same");
        let accepted_revision = editor.revision();
        editor.select_all();
        assert!(editor.replace_selection("same"));
        assert_ne!(editor.revision(), accepted_revision);
        assert!(editor.undo());
        assert_eq!(editor.selected_text(), Some("same"));
    }

    #[test]
    fn grapheme_navigation_keeps_emoji_and_combining_sequences_whole() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("A👩‍💻e\u{301}Z");
        buffer.move_to(buffer.text().len());

        let before_z = buffer.previous_boundary(buffer.cursor());
        buffer.move_to(before_z);
        assert_eq!(&buffer.text()[buffer.cursor()..], "Z");
        buffer.move_to(buffer.previous_boundary(buffer.cursor()));
        assert_eq!(&buffer.text()[buffer.cursor()..], "e\u{301}Z");
        buffer.move_to(buffer.previous_boundary(buffer.cursor()));
        assert_eq!(&buffer.text()[buffer.cursor()..], "👩‍💻e\u{301}Z");
    }

    #[test]
    fn utf16_offsets_round_trip_emoji_and_clamp_inside_surrogates() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("a😀文");

        assert_eq!(buffer.offset_to_utf16(1), 1);
        assert_eq!(buffer.offset_from_utf16(1), 1);
        assert_eq!(buffer.offset_from_utf16(2), 1);
        assert_eq!(buffer.offset_from_utf16(3), 5);
        assert_eq!(buffer.offset_to_utf16(5), 3);
        assert_eq!(buffer.text_for_utf16_range(&(1..3)).0, "😀");
    }

    #[test]
    fn ime_marking_uses_selection_relative_to_inserted_text() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("left right");
        buffer.set_selection(5..10, false);
        buffer.replace_and_mark_text_utf16(None, "日本😀", Some(2..4));

        assert_eq!(buffer.text(), "left 日本😀");
        assert_eq!(buffer.marked_range(), Some(&(5..15)));
        assert_eq!(&buffer.text()[buffer.selection().clone()], "😀");
    }

    #[test]
    fn word_backspace_deletes_unicode_words_whitespace_and_selections() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("alpha café   ");

        assert!(buffer.delete_word_backward());
        assert_eq!(buffer.text(), "alpha ");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "alpha café   ");

        buffer.set_selection(0..5, false);
        assert!(buffer.delete_word_backward());
        assert_eq!(buffer.text(), " café   ");
    }

    #[test]
    fn multiline_paste_normalizes_newlines_and_undo_restores_draft() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("one\r\ntwo\rthree");
        assert_eq!(buffer.text(), "one\ntwo\nthree");
        assert!(buffer.undo());
        assert!(buffer.is_empty());
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "one\ntwo\nthree");
    }

    #[test]
    fn ime_crlf_ranges_stay_inside_normalized_document() {
        let mut buffer = TextBuffer::default();
        buffer.replace_and_mark_text_utf16(None, "a\r\n😀", Some(3..5));
        assert_eq!(buffer.text(), "a\n😀");
        assert_eq!(buffer.marked_range(), Some(&(0..6)));
        assert_eq!(buffer.selected_text(), Some("😀"));
        buffer.unmark_text();
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "");
        assert!(buffer.redo());
        assert_eq!(buffer.selected_text(), Some("😀"));
    }

    #[test]
    fn an_ime_composition_is_one_reversible_edit() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("hello world");
        buffer.set_selection(6..11, true);
        buffer.replace_and_mark_text_utf16(None, "n", None);
        buffer.replace_and_mark_text_utf16(None, "ni", None);
        buffer.replace_text_utf16(None, "你");
        assert_eq!(buffer.text(), "hello 你");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "hello world");
        assert_eq!(buffer.selection(), &(6..11));
        assert!(buffer.selection_reversed());
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "hello 你");
    }

    #[test]
    fn retyping_equal_text_has_a_different_acceptance_identity() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("ship");
        let submitted = buffer.revision();
        buffer.select_all();
        buffer.replace_selection("changed");
        buffer.select_all();
        buffer.replace_selection("ship");
        assert_eq!(buffer.text(), "ship");
        assert_ne!(buffer.revision(), submitted);
    }

    #[test]
    fn history_retains_changed_bytes_not_full_document_copies() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection(&"x".repeat(1024 * 1024));
        for _ in 0..128 { buffer.replace_selection("y"); }
        assert_eq!(buffer.undo.len(), MAX_UNDO_DEPTH);
        assert_eq!(buffer.history_bytes, 128);
        for _ in 0..128 { assert!(buffer.undo()); }
        assert_eq!(buffer.text().len(), 1024 * 1024);
        assert!(!buffer.undo());
        for _ in 0..128 { assert!(buffer.redo()); }
        assert_eq!(buffer.text().len(), 1024 * 1024 + 128);
    }

    #[test]
    fn cloned_sessions_do_not_share_mutable_text_or_undo() {
        let mut first = TextBuffer::default();
        first.replace_selection("alpha");
        let mut second = first.clone();
        first.replace_selection(" one");
        second.replace_selection(" two");
        assert_eq!(first.text(), "alpha one");
        assert_eq!(second.text(), "alpha two");
        second.undo();
        assert_eq!(second.text(), "alpha");
        assert_eq!(first.text(), "alpha one");
    }

    #[test]
    fn persisted_editor_validates_undo_and_preserves_reversed_selection() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("hello 😀");
        buffer.select_all();
        buffer.replace_selection("next");
        buffer.undo();
        buffer.set_selection(0..5, true);
        let encoded = serde_json::to_vec(&buffer).unwrap();
        let mut restored: TextBuffer = serde_json::from_slice(&encoded).unwrap();
        assert!(restored.sanitize_restored());
        assert!(restored.selection_reversed());
        assert_eq!(restored.selected_text(), Some("hello"));
        assert!(restored.redo());
        assert_eq!(restored.text(), "next");
    }

    #[test]
    fn corrupt_undo_is_discarded_without_losing_current_text() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("safe");
        buffer.undo.back_mut().unwrap().start = usize::MAX;
        assert!(!buffer.sanitize_restored());
        assert_eq!(buffer.text(), "safe");
        assert!(!buffer.undo());
    }

    #[test]
    fn new_edit_after_undo_discards_only_redo_branch() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("A");
        buffer.replace_selection("B");
        buffer.undo();
        buffer.replace_selection("C");
        assert_eq!(buffer.text(), "AC");
        assert!(!buffer.redo());
        assert_eq!(buffer.history_bytes, 2);
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "A");
    }
}
