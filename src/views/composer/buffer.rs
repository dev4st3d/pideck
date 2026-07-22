use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

const MAX_UNDO_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    text: String,
    selection: Range<usize>,
    selection_reversed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TextBuffer {
    text: String,
    selection: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self {
            text: String::new(),
            selection: 0..0,
            selection_reversed: false,
            marked_range: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl TextBuffer {
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn selection(&self) -> &Range<usize> {
        &self.selection
    }

    pub(super) fn selection_reversed(&self) -> bool {
        self.selection_reversed
    }

    pub(super) fn marked_range(&self) -> Option<&Range<usize>> {
        self.marked_range.as_ref()
    }

    pub(super) fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(super) fn previous_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.text.len())
    }

    pub(super) fn nearest_boundary(&self, offset: usize) -> usize {
        let offset = self.clamp_char_boundary(offset);
        if offset == 0 || offset == self.text.len() {
            return offset;
        }
        let previous = self.previous_boundary(offset.saturating_add(1));
        let next = self.next_boundary(previous);
        if offset.saturating_sub(previous) <= next.saturating_sub(offset) {
            previous
        } else {
            next
        }
    }

    pub(super) fn move_to(&mut self, offset: usize) {
        let offset = self.nearest_boundary(offset);
        self.selection = offset..offset;
        self.selection_reversed = false;
    }

    pub(super) fn select_to(&mut self, offset: usize) {
        let offset = self.nearest_boundary(offset);
        let anchor = if self.selection_reversed {
            self.selection.end
        } else {
            self.selection.start
        };
        self.selection = anchor.min(offset)..anchor.max(offset);
        self.selection_reversed = offset < anchor;
    }

    pub(super) fn select_all(&mut self) {
        self.selection = 0..self.text.len();
        self.selection_reversed = false;
    }

    pub(super) fn set_selection(&mut self, range: Range<usize>, reversed: bool) {
        let start = self.clamp_char_boundary(range.start.min(self.text.len()));
        let end = self.clamp_char_boundary(range.end.min(self.text.len()));
        self.selection = start.min(end)..start.max(end);
        self.selection_reversed = reversed && !self.selection.is_empty();
    }

    pub(super) fn replace_selection(&mut self, text: &str) -> bool {
        let range = self.selection.clone();
        self.replace_bytes(range, text, true)
    }

    pub(super) fn delete_backward(&mut self) -> bool {
        let range = if self.selection.is_empty() {
            self.previous_boundary(self.cursor())..self.cursor()
        } else {
            self.selection.clone()
        };
        self.replace_bytes(range, "", true)
    }

    pub(super) fn delete_forward(&mut self) -> bool {
        let range = if self.selection.is_empty() {
            self.cursor()..self.next_boundary(self.cursor())
        } else {
            self.selection.clone()
        };
        self.replace_bytes(range, "", true)
    }

    pub(super) fn replace_text_utf16(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
    ) -> bool {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selection.clone());
        let record_undo = self.marked_range.is_none();
        let changed = self.replace_bytes(range, text, record_undo);
        self.marked_range = None;
        changed
    }

    pub(super) fn replace_and_mark_text_utf16(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        selected_utf16: Option<Range<usize>>,
    ) -> bool {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selection.clone());
        let start = range.start;
        let record_undo = self.marked_range.is_none();
        let changed = self.replace_bytes(range, new_text, record_undo);
        let inserted_end = start.saturating_add(new_text.len());
        self.marked_range = (!new_text.is_empty()).then_some(start..inserted_end);

        if let Some(selected_utf16) = selected_utf16 {
            let relative_start = utf16_to_byte(new_text, selected_utf16.start);
            let relative_end = utf16_to_byte(new_text, selected_utf16.end);
            self.set_selection(
                start + relative_start..start + relative_end,
                selected_utf16.start > selected_utf16.end,
            );
        } else {
            self.selection = inserted_end..inserted_end;
            self.selection_reversed = false;
        }
        changed
    }

    pub(super) fn unmark_text(&mut self) {
        self.marked_range = None;
    }

    pub(super) fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(snapshot);
        true
    }

    pub(super) fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.push_undo_snapshot(self.snapshot());
        self.restore(snapshot);
        true
    }

    pub(super) fn clear_if_matches(&mut self, expected: &str) -> bool {
        if self.text != expected {
            return false;
        }
        self.replace_bytes(0..self.text.len(), "", true)
    }

    pub(super) fn selected_text(&self) -> Option<&str> {
        (!self.selection.is_empty()).then(|| &self.text[self.selection.clone()])
    }

    pub(super) fn text_for_utf16_range(
        &self,
        range_utf16: &Range<usize>,
    ) -> (String, Range<usize>) {
        let range = self.range_from_utf16(range_utf16);
        (
            self.text[range.clone()].to_owned(),
            self.range_to_utf16(&range),
        )
    }

    pub(super) fn offset_from_utf16(&self, offset: usize) -> usize {
        utf16_to_byte(&self.text, offset)
    }

    pub(super) fn offset_to_utf16(&self, offset: usize) -> usize {
        byte_to_utf16(&self.text, offset)
    }

    pub(super) fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let start = self.offset_from_utf16(range.start);
        let end = self.offset_from_utf16(range.end);
        start.min(end)..start.max(end)
    }

    pub(super) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub(super) fn hard_line_start(&self, offset: usize) -> usize {
        self.text[..self.clamp_char_boundary(offset)]
            .rfind('\n')
            .map_or(0, |index| index + 1)
    }

    pub(super) fn hard_line_end(&self, offset: usize) -> usize {
        let offset = self.clamp_char_boundary(offset);
        self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |index| offset + index)
    }

    fn replace_bytes(&mut self, range: Range<usize>, text: &str, record_undo: bool) -> bool {
        let start = self.clamp_char_boundary(range.start.min(self.text.len()));
        let end = self.clamp_char_boundary(range.end.min(self.text.len()));
        let range = start.min(end)..start.max(end);
        let normalized = normalize_newlines(text);
        if self.text[range.clone()] == normalized {
            self.selection = range.end..range.end;
            self.selection_reversed = false;
            return false;
        }

        if record_undo {
            self.push_undo_snapshot(self.snapshot());
            self.redo.clear();
        }
        self.text.replace_range(range.clone(), &normalized);
        let cursor = range.start + normalized.len();
        self.selection = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        true
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            selection: self.selection.clone(),
            selection_reversed: self.selection_reversed,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.text = snapshot.text;
        self.selection = snapshot.selection;
        self.selection_reversed = snapshot.selection_reversed;
        self.marked_range = None;
    }

    fn push_undo_snapshot(&mut self, snapshot: Snapshot) {
        if self.undo.last() == Some(&snapshot) {
            return;
        }
        if self.undo.len() == MAX_UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.undo.push(snapshot);
    }

    fn clamp_char_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset = offset.saturating_sub(1);
        }
        offset
    }
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, character) in text.char_indices() {
        if utf16 >= offset {
            return byte;
        }
        let next = utf16 + character.len_utf16();
        if next > offset {
            return byte;
        }
        utf16 = next;
    }
    text.len()
}

fn byte_to_utf16(text: &str, offset: usize) -> usize {
    let mut byte_offset = offset.min(text.len());
    while !text.is_char_boundary(byte_offset) {
        byte_offset = byte_offset.saturating_sub(1);
    }
    text[..byte_offset].encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn multiline_paste_normalizes_newlines_and_undo_restores_draft() {
        let mut buffer = TextBuffer::default();
        buffer.replace_selection("one\r\ntwo\rthree");
        assert_eq!(buffer.text(), "one\ntwo\nthree");
        assert!(buffer.undo());
        assert!(buffer.is_empty());
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "one\ntwo\nthree");
    }
}
