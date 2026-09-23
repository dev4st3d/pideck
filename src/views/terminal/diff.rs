//! Read-only, aligned Git changes. Parsing is independent of GPUI and happens once per snapshot.

use std::path::PathBuf;

use gpui::{
    ClipboardItem, Context, EventEmitter, FocusHandle, FontWeight, IntoElement, KeyDownEvent,
    MouseButton, Render, ScrollHandle, ScrollStrategy, SharedString, UniformListScrollHandle,
    Window, div, point, prelude::*, px, relative, svg, uniform_list,
};

use crate::services::project_git::{DiffContent, DiffKind, ReviewAction, ReviewFile};
use crate::theme::{self, terminal_manager as chrome};
use crate::views::terminal_manager::text_tooltip;

const ROW_HEIGHT: f32 = 22.0;

#[path = "diff_review.rs"]
mod review;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    number: usize,
    text: String,
    display: SharedString,
    changed: bool,
}

impl Line {
    fn new(number: usize, text: &str, changed: bool) -> Self {
        Self {
            number,
            text: text.to_owned(),
            display: SharedString::new(text.replace('\t', "    ")),
            changed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    Hunk(String),
    Lines {
        before: Option<Line>,
        after: Option<Line>,
    },
}

#[derive(Debug)]
struct Section {
    label: &'static str,
    rows: Vec<Row>,
    additions: usize,
    deletions: usize,
    binary: bool,
    conflicted: bool,
    truncated: bool,
    metadata: Vec<String>,
    code_width: f32,
}

impl Section {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            rows: Vec::new(),
            additions: 0,
            deletions: 0,
            binary: false,
            conflicted: false,
            truncated: false,
            metadata: Vec::new(),
            code_width: 0.0,
        }
    }
}

fn flush_changes(rows: &mut Vec<Row>, before: &mut Vec<Line>, after: &mut Vec<Line>) {
    let count = before.len().max(after.len());
    let mut before = before.drain(..);
    let mut after = after.drain(..);
    rows.extend((0..count).map(|_| Row::Lines {
        before: before.next(),
        after: after.next(),
    }));
}

fn hunk_start(header: &str) -> Option<(usize, usize)> {
    let mut parts = header.strip_prefix("@@ ")?.split_whitespace();
    let before = parts
        .next()?
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    let after = parts
        .next()?
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    (parts.next()? == "@@").then_some((before, after))
}

fn parse(text: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut section = Section::new("Working tree");
    let (mut old_number, mut new_number) = (0, 0);
    let (mut before, mut after) = (Vec::new(), Vec::new());
    let mut in_hunk = false;
    for line in text.lines() {
        if matches!(line, "Staged" | "Working tree") {
            flush_changes(&mut section.rows, &mut before, &mut after);
            if !section.rows.is_empty()
                || section.conflicted
                || section.binary
                || section.truncated
                || !section.metadata.is_empty()
            {
                sections.push(section);
            }
            section = Section::new(if line == "Staged" {
                "Staged"
            } else {
                "Working tree"
            });
            in_hunk = false;
        } else if line.starts_with("@@@ ") {
            flush_changes(&mut section.rows, &mut before, &mut after);
            section.conflicted = true;
            in_hunk = false;
        } else if let Some((old, new)) = hunk_start(line) {
            flush_changes(&mut section.rows, &mut before, &mut after);
            old_number = old;
            new_number = new;
            in_hunk = true;
            section.rows.push(Row::Hunk(line.to_owned()));
        } else if line.starts_with("Diff truncated:") {
            flush_changes(&mut section.rows, &mut before, &mut after);
            section.truncated = true;
            in_hunk = false;
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            section.binary = true;
            in_hunk = false;
        } else if line == "\\ No newline at end of file" {
            let note = if !after.is_empty() {
                "After: no newline at end of file"
            } else if !before.is_empty() {
                "Before: no newline at end of file"
            } else {
                "Both versions: no newline at end of file"
            };
            if !section.metadata.iter().any(|existing| existing == note) {
                section.metadata.push(note.to_owned());
            }
        } else if in_hunk {
            if let Some(text) = line.strip_prefix('-') {
                before.push(Line::new(old_number, text, true));
                old_number += 1;
                section.deletions += 1;
            } else if let Some(text) = line.strip_prefix('+') {
                after.push(Line::new(new_number, text, true));
                new_number += 1;
                section.additions += 1;
            } else if let Some(text) = line.strip_prefix(' ') {
                flush_changes(&mut section.rows, &mut before, &mut after);
                section.rows.push(Row::Lines {
                    before: Some(Line::new(old_number, text, false)),
                    after: Some(Line::new(new_number, text, false)),
                });
                old_number += 1;
                new_number += 1;
            } else if !line.starts_with("\\ No newline at end of file") {
                flush_changes(&mut section.rows, &mut before, &mut after);
                in_hunk = false;
            }
        } else if [
            "old mode ",
            "new mode ",
            "rename from ",
            "rename to ",
            "new file mode ",
            "deleted file mode ",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        {
            section.metadata.push(line.into());
        }
    }
    flush_changes(&mut section.rows, &mut before, &mut after);
    sections.push(section);
    for section in &mut sections {
        let columns = section
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::Lines { before, after } => Some(
                    before
                        .iter()
                        .chain(after)
                        .map(|line| {
                            line.text
                                .chars()
                                // Reserve two cells for non-ASCII fallback glyphs.
                                // This may leave extra scroll room for combining
                                // sequences, but never clips wide CJK/emoji text.
                                .map(|ch| {
                                    if ch == '\t' {
                                        4
                                    } else if ch.is_ascii() {
                                        1
                                    } else {
                                        2
                                    }
                                })
                                .sum::<usize>()
                        })
                        .max()
                        .unwrap_or(0),
                ),
                Row::Hunk(_) => None,
            })
            .max()
            .unwrap_or(0);
        // All visible rows use the same text extent so either split pane can
        // scroll to the same column without moving the divider.
        section.code_width = columns as f32 * 7.2 + 4.0;
    }
    sections
}

#[derive(Clone)]
pub(super) enum DiffEvent {
    OpenFile,
    Review(ReviewAction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisplayRow {
    source: usize,
    side: Option<bool>,
}

fn display_rows(rows: &[Row], split: bool) -> Vec<DisplayRow> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < rows.len() {
        if split {
            output.push(DisplayRow {
                source: index,
                side: None,
            });
            index += 1;
            continue;
        }
        if matches!(&rows[index], Row::Lines { before, after } if before.as_ref().is_some_and(|line| line.changed) || after.as_ref().is_some_and(|line| line.changed))
        {
            let start = index;
            while index < rows.len()
                && matches!(&rows[index], Row::Lines { before, after } if before.as_ref().is_some_and(|line| line.changed) || after.as_ref().is_some_and(|line| line.changed))
            {
                index += 1;
            }
            for right in [false, true] {
                for (source, row) in rows.iter().enumerate().take(index).skip(start) {
                    if let Row::Lines { before, after } = row
                        && if right {
                            after.is_some()
                        } else {
                            before.is_some()
                        }
                    {
                        output.push(DisplayRow {
                            source,
                            side: Some(right),
                        });
                    }
                }
            }
        } else {
            output.push(DisplayRow {
                source: index,
                side: None,
            });
            index += 1;
        }
    }
    output
}

pub(super) struct DiffView {
    workspace: PathBuf,
    pub(super) file: ReviewFile,
    raw: String,
    sections: Vec<Section>,
    active: usize,
    content: DiffContent,
    split: bool,
    display: Vec<DisplayRow>,
    hunk_rows: Vec<usize>,
    hunk_display: Vec<usize>,
    pub(super) focus: FocusHandle,
    scroll: UniformListScrollHandle,
    code_scroll: ScrollHandle,
    selection: Option<(bool, usize, usize)>,
    operation_busy: bool,
    hunk_cursor: Option<usize>,
    files_expanded: bool,
    body_expanded: bool,
    parent_picker: bool,
    copied_hash: bool,
    copy_generation: u64,
    files_scroll: UniformListScrollHandle,
}

impl DiffView {
    pub(super) fn new(
        workspace: PathBuf,
        file: ReviewFile,
        content: DiffContent,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            workspace,
            file: file.clone(),
            raw: String::new(),
            sections: parse(""),
            active: 0,
            content: DiffContent::Loading,
            split: false,
            display: Vec::new(),
            hunk_rows: Vec::new(),
            hunk_display: Vec::new(),
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            code_scroll: ScrollHandle::new(),
            selection: None,
            operation_busy: false,
            hunk_cursor: None,
            files_expanded: false,
            body_expanded: false,
            parent_picker: false,
            copied_hash: false,
            copy_generation: 0,
            files_scroll: UniformListScrollHandle::new(),
        };
        view.update_snapshot(file, content, cx);
        view
    }

    pub(super) fn update_snapshot(
        &mut self,
        file: ReviewFile,
        content: DiffContent,
        cx: &mut Context<Self>,
    ) {
        let same_commit = self
            .file
            .commit
            .as_ref()
            .map(|commit| (&commit.summary.id, &commit.parent))
            == file
                .commit
                .as_ref()
                .map(|commit| (&commit.summary.id, &commit.parent));
        let same_file = self.file.path == file.path && self.file.kind == file.kind && same_commit;
        if !same_commit {
            self.files_expanded = false;
            self.body_expanded = false;
            self.parent_picker = false;
            self.copied_hash = false;
            self.files_scroll = UniformListScrollHandle::new();
        }
        if file.total > 0 {
            self.files_scroll
                .scroll_to_item(file.position, ScrollStrategy::Center);
        }
        self.file = file;
        if same_file && self.content == content {
            cx.notify();
            return;
        }
        let raw = match &content {
            DiffContent::Ready(text) => text.clone(),
            _ => String::new(),
        };
        if !same_file || self.raw != raw {
            self.selection = None;
            self.hunk_cursor = None;
            self.scroll = UniformListScrollHandle::new();
            self.reset_code_scroll();
        }
        self.raw = raw;
        self.sections = parse(&self.raw);
        self.active = self.sections.len().saturating_sub(1);
        self.display = display_rows(&self.sections[self.active].rows, self.split);
        self.cache_hunks();
        self.content = content;
        cx.notify();
    }

    fn set_split(&mut self, split: bool, cx: &mut Context<Self>) {
        if self.split == split {
            return;
        }
        let top = (-f32::from(self.scroll.0.borrow().base_handle.offset().y) / ROW_HEIGHT).max(0.0)
            as usize;
        let source = self.display.get(top).map(|row| row.source);
        self.split = split;
        self.reset_code_scroll();
        self.display = display_rows(&self.sections[self.active].rows, split);
        self.cache_hunks();
        if let Some(source) = source
            && let Some(index) = self.display.iter().position(|row| row.source == source)
        {
            self.scroll
                .scroll_to_item_strict(index, ScrollStrategy::Top);
        }
        cx.notify();
    }

    fn reset_code_scroll(&self) {
        let origin = point(px(0.0), px(0.0));
        self.code_scroll.set_offset(origin);
    }

    fn cache_hunks(&mut self) {
        self.hunk_rows = self.sections[self.active]
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| matches!(row, Row::Hunk(_)).then_some(index))
            .collect();
        self.hunk_display = self
            .display
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                self.hunk_rows
                    .binary_search(&row.source)
                    .is_ok()
                    .then_some(index)
            })
            .collect();
    }

    fn copy(&self, cx: &mut Context<Self>) {
        let text = match self.selection {
            Some((right, anchor, end)) => self.sections[self.active].rows
                [anchor.min(end)..=anchor.max(end)]
                .iter()
                .filter_map(|row| match row {
                    Row::Lines { before, after } => if right {
                        after.as_ref()
                    } else {
                        before.as_ref()
                    }
                    .map(|line| line.text.as_str()),
                    Row::Hunk(_) => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            None => self.raw.clone(),
        };
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if key.modifiers.control && key.key == "c" {
            self.copy(cx);
        } else if key.modifiers.control && key.key == "o" && self.file.commit.is_none() {
            cx.emit(DiffEvent::OpenFile);
        } else if key.modifiers.alt && key.key == "up" {
            cx.emit(DiffEvent::Review(ReviewAction::Navigate(-1)));
        } else if key.modifiers.alt && key.key == "down" {
            cx.emit(DiffEvent::Review(ReviewAction::Navigate(1)));
        } else if key.key == "f7" {
            self.navigate_hunk(!key.modifiers.shift, cx);
        } else if key.key == "f5" {
            cx.emit(DiffEvent::Review(ReviewAction::Navigate(0)));
        } else if key.modifiers.alt && key.key == "u" {
            self.set_split(false, cx);
        } else if key.modifiers.alt && key.key == "s" {
            self.set_split(true, cx);
        } else if key.modifiers.control
            && key.modifiers.shift
            && matches!(key.key.as_str(), "left" | "right")
        {
            let step = if key.key == "left" { 120.0 } else { -120.0 };
            let x = (self.code_scroll.offset().x + px(step))
                .clamp(-self.code_scroll.max_offset().width, px(0.0));
            self.code_scroll.set_offset(point(x, px(0.0)));
            cx.notify();
        } else if key.key == "escape" {
            self.selection = None;
        } else if matches!(
            key.key.as_str(),
            "up" | "down" | "home" | "end" | "left" | "right"
        ) {
            let count = self.sections[self.active].rows.len();
            if count == 0 {
                return;
            }
            let (mut right, anchor, end) = self.selection.unwrap_or((true, 0, 0));
            let end = match key.key.as_str() {
                "up" => end.saturating_sub(1),
                "down" => (end + 1).min(count - 1),
                "home" => 0,
                "end" => count - 1,
                "left" => {
                    right = false;
                    end
                }
                "right" => {
                    right = true;
                    end
                }
                _ => end,
            };
            self.selection = Some((right, if key.modifiers.shift { anchor } else { end }, end));
            self.scroll.scroll_to_item(
                self.display
                    .iter()
                    .position(|row| row.source == end && row.side.is_none_or(|side| side == right))
                    .unwrap_or(0),
                ScrollStrategy::Center,
            );
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn cell(
        &self,
        before: Option<&Line>,
        after: Option<&Line>,
        right: bool,
        index: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let line = if right { after } else { before };
        let selected = self.selection.is_some_and(|(side, anchor, end)| {
            side == right && (anchor.min(end)..=anchor.max(end)).contains(&index)
        });
        let changed = line.is_some_and(|line| line.changed);
        let background = if selected {
            theme::selection()
        } else if changed {
            if right {
                theme::diff_added()
            } else {
                theme::diff_removed()
            }
        } else if line.is_none() {
            theme::diff_empty()
        } else {
            theme::canvas()
        };
        let foreground = if selected {
            theme::bone()
        } else if changed {
            if right {
                theme::success()
            } else {
                theme::error()
            }
        } else {
            theme::bone()
        };
        let number =
            |line: Option<&Line>| line.map_or_else(String::new, |line| line.number.to_string());
        let gutter = |value: String| {
            div()
                .w(px(44.0))
                .flex_shrink_0()
                .pr(px(12.0))
                .text_right()
                .text_color(theme::ash())
                .child(value)
        };
        div()
            .id((if right { "diff-after" } else { "diff-before" }, index))
            .debug_selector(move || format!("diff-{index}-{right}"))
            .h(px(ROW_HEIGHT))
            .when(self.split, |cell| cell.w(relative(0.5)).flex_shrink_0())
            .when(!self.split, |cell| cell.w_full())
            .min_w_0()
            .flex()
            .items_center()
            .bg(background)
            .text_color(foreground)
            .when(self.split && right, |cell| {
                cell.border_l_1().border_color(theme::edge_soft())
            })
            .cursor(gpui::CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &gpui::MouseDownEvent, window, cx| {
                    window.focus(&view.focus);
                    let anchor = if event.modifiers.shift {
                        view.selection
                            .filter(|(side, _, _)| *side == right)
                            .map_or(index, |(_, anchor, _)| anchor)
                    } else {
                        index
                    };
                    view.selection = Some((right, anchor, index));
                    cx.notify();
                }),
            )
            .when(!self.split, |cell| {
                cell.child(div().w(px(4.0)).flex_shrink_0())
                    .child(gutter(number(before)))
            })
            .child(gutter(number(if self.split { line } else { after })))
            .child(div().w(px(20.0)).flex_shrink_0().child(if changed {
                if right { "+" } else { "−" }
            } else {
                ""
            }))
            .child({
                let mut code = div()
                    .id((
                        if right {
                            "diff-code-after"
                        } else {
                            "diff-code-before"
                        },
                        index,
                    ))
                    .debug_selector(move || format!("diff-code-{index}-{right}"))
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .track_scroll(&self.code_scroll)
                    .child(
                        div()
                            .debug_selector(move || format!("diff-text-{index}-{right}"))
                            .w(px(self.sections[self.active].code_width))
                            .pl(px(4.0))
                            .whitespace_nowrap()
                            .child(line.map_or_else(
                                || SharedString::new_static(""),
                                |line| line.display.clone(),
                            )),
                    );
                // GPUI otherwise turns a plain vertical wheel into horizontal
                // movement on an x-only scroll element inside the vertical list.
                code.style().restrict_scroll_to_axis = Some(true);
                code
            })
            .into_any_element()
    }

    pub(super) fn set_operation_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.operation_busy != busy {
            self.operation_busy = busy;
            cx.notify();
        }
    }

    fn navigate_hunk(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.hunk_rows.is_empty() {
            return;
        }
        let index = match self.hunk_cursor {
            Some(index) if forward => (index + 1) % self.hunk_rows.len(),
            Some(index) => (index + self.hunk_rows.len() - 1) % self.hunk_rows.len(),
            None if forward => 0,
            None => self.hunk_rows.len() - 1,
        };
        self.hunk_cursor = Some(index);
        let source = self.hunk_rows[index];
        self.selection = Some((true, source, source));
        self.scroll
            .scroll_to_item_strict(self.hunk_display[index], ScrollStrategy::Top);
        cx.notify();
    }

    fn can_discard(&self) -> bool {
        !self.operation_busy
            && self.file.commit.is_none()
            && self.file.total > 0
            && self.file.kind == DiffKind::WorkingTree
            && self.file.marker != "!"
            && matches!(self.content, DiffContent::Ready(_))
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let display = self.display[index];
        match &self.sections[self.active].rows[display.source] {
            Row::Hunk(label) => {
                let ordinal = self.hunk_rows.binary_search(&display.source).unwrap_or(0);
                let undo = self.can_discard() && self.file.marker == "M";
                div()
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .bg(theme::chrome())
                    .text_color(theme::ash())
                    .child(
                        div()
                            .id(("diff-hunk-label", display.source))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .tooltip(text_tooltip(label.clone()))
                            .child(label.clone()),
                    )
                    .when(undo, |row| {
                        row.child(
                            Self::control(("undo-hunk", ordinal), "Discard this hunk", true)
                                .h(px(20.0))
                                .px(px(6.0))
                                .child(
                                    svg()
                                        .path("icons/undo.svg")
                                        .size(px(12.0))
                                        .text_color(theme::ash()),
                                )
                                .child("Undo hunk")
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    if view.can_discard() {
                                        cx.emit(DiffEvent::Review(ReviewAction::Discard(Some((
                                            ordinal,
                                            view.raw.clone(),
                                        )))));
                                    }
                                })),
                        )
                    })
                    .into_any_element()
            }
            Row::Lines { before, after } if self.split => div()
                .h(px(ROW_HEIGHT))
                .w_full()
                .flex()
                .child(self.cell(before.as_ref(), None, false, display.source, cx))
                .child(self.cell(None, after.as_ref(), true, display.source, cx))
                .into_any_element(),
            Row::Lines { before, after } => {
                let (before, after, right) = match display.side {
                    Some(false) => (before.as_ref(), None, false),
                    Some(true) => (None, after.as_ref(), true),
                    None => (before.as_ref(), after.as_ref(), true),
                };
                self.cell(before, after, right, display.source, cx)
            }
        }
    }

    fn control(
        id: impl Into<gpui::ElementId>,
        label: impl Into<SharedString>,
        enabled: bool,
    ) -> gpui::Stateful<gpui::Div> {
        let label = label.into();
        div()
            .id(id)
            .h(px(28.0))
            .px(px(10.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(8.0))
            .text_size(px(11.0))
            .rounded(px(4.0))
            .border_1()
            .border_color(gpui::rgba(0))
            .tooltip(text_tooltip(label.to_string()))
            .when(!enabled, |control| control.opacity(0.4))
            .when(enabled, |control| {
                control
                    .tab_index(0)
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::panel_hover()))
                    .focus(|style| style.border_color(theme::focus()))
            })
    }
}

impl EventEmitter<DiffEvent> for DiffView {}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_review(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn historical_diff_renders_file_rows_and_has_no_working_tree_undo(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::services::project_git::{
            GitLineStats,
            workflow::{CommitDetails, CommitSummary},
        };
        cx.update(|cx| {
            crate::fonts::initialize(cx);
            crate::views::file_editor::FileEditor::initialize(cx);
        });
        let summary = CommitSummary {
            id: "a".repeat(40),
            parents: Vec::new(),
            author: "Example Developer".into(),
            timestamp: 0,
            date: "2026-09-14 14:32".into(),
            subject: "Example commit".into(),
            body: "A commit body".into(),
            local: Some(true),
            head: true,
            remote_tip: false,
        };
        let details = std::sync::Arc::new(CommitDetails {
            summary,
            parent: None,
            parent_index: 0,
            files: vec![crate::services::project_git::workflow::CommitFile {
                path: "project/file.rs".into(),
                relative_path: "file.rs".into(),
                stats: Some(GitLineStats {
                    additions: 1,
                    deletions: 1,
                }),
            }],
        });
        let file = ReviewFile {
            path: "project/file.rs".into(),
            kind: DiffKind::WorkingTree,
            position: 0,
            total: 1,
            marker: "M",
            commit: Some(details),
        };
        let (view, cx) = cx.add_window_view(|_, cx| {
            DiffView::new(
                "project".into(),
                file,
                DiffContent::Ready("@@ -1 +1 @@\n-before\n+after\n".into()),
                cx,
            )
        });
        cx.simulate_resize(gpui::size(px(800.0), px(620.0)));
        cx.refresh().unwrap();
        cx.run_until_parked();
        assert!(cx.debug_bounds("committed-file-0").is_none());
        assert!(!view.read_with(cx, |view, _| view.files_expanded));
        view.update(cx, |view, cx| {
            view.files_expanded = true;
            cx.notify();
        });
        cx.refresh().unwrap();
        assert_eq!(
            cx.debug_bounds("committed-file-0").unwrap().size.height,
            px(28.0)
        );
        assert!(!view.read_with(cx, |view, _| view.can_discard()));
    }

    #[gpui::test]
    fn split_rows_keep_a_fixed_divider_and_unified_rows_fill_the_viewer(
        cx: &mut gpui::TestAppContext,
    ) {
        let file = ReviewFile {
            path: "project/file.rs".into(),
            kind: DiffKind::WorkingTree,
            position: 0,
            total: 1,
            marker: "M",
            commit: None,
        };
        let (view, cx) = cx.add_window_view(|_, cx| {
            DiffView::new(
                "project".into(),
                file,
                DiffContent::Ready(format!(
                    "@@ -1,83 +1,83 @@\n short\n {}\n last\n{}",
                    "long line ".repeat(40),
                    " context\n".repeat(80)
                )),
                cx,
            )
        });
        for width in [380.0, 540.0, 900.0, 1200.0] {
            cx.simulate_resize(gpui::size(px(width), px(640.0)));
            view.update(cx, |view, cx| view.set_split(true, cx));
            cx.refresh().unwrap();
            let left_short = cx.debug_bounds("diff-1-false").unwrap();
            let right_short = cx.debug_bounds("diff-1-true").unwrap();
            let left_long = cx.debug_bounds("diff-2-false").unwrap();
            let right_long = cx.debug_bounds("diff-2-true").unwrap();
            assert_eq!(left_short.right(), right_short.left());
            assert_eq!(left_long.right(), right_long.left());
            assert_eq!(left_short.size.width, right_short.size.width);
            assert_eq!(left_short.left(), left_long.left());
            assert_eq!(right_short.left(), right_long.left());
            assert_eq!(right_short.right(), right_long.right());
            let viewer = cx.debug_bounds("diff-horizontal-scroll").unwrap();
            assert_eq!(right_short.right(), viewer.right());

            if width == 900.0 {
                let code_before = cx.debug_bounds("diff-code-2-false").unwrap();
                let wheel_position =
                    point(code_before.left() + px(24.0), code_before.top() + px(8.0));
                let y_before =
                    view.read_with(cx, |view, _| view.scroll.0.borrow().base_handle.offset().y);
                cx.simulate_event(gpui::ScrollWheelEvent {
                    position: wheel_position,
                    delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-40.0))),
                    ..Default::default()
                });
                cx.refresh().unwrap();
                assert_eq!(
                    view.read_with(cx, |view, _| view.code_scroll.offset().x),
                    px(0.0)
                );
                assert!(
                    view.read_with(cx, |view, _| view.scroll.0.borrow().base_handle.offset().y)
                        < y_before
                );
                view.update(cx, |view, cx| {
                    view.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
                    cx.notify();
                });
                cx.refresh().unwrap();
                cx.simulate_event(gpui::ScrollWheelEvent {
                    position: wheel_position,
                    delta: gpui::ScrollDelta::Pixels(point(px(-40.0), px(0.0))),
                    ..Default::default()
                });
                cx.refresh().unwrap();
                assert!(view.read_with(cx, |view, _| view.code_scroll.offset().x < px(0.0)));
                let y_after =
                    view.read_with(cx, |view, _| view.scroll.0.borrow().base_handle.offset().y);
                assert_eq!(y_after, y_before);
                let left_before = cx.debug_bounds("diff-text-2-false").unwrap();
                let right_before = cx.debug_bounds("diff-text-2-true").unwrap();
                view.update(cx, |view, cx| {
                    view.code_scroll.set_offset(point(px(-320.0), px(0.0)));
                    cx.notify();
                });
                cx.refresh().unwrap();
                let code_after = cx.debug_bounds("diff-code-2-false").unwrap();
                let left_after = cx.debug_bounds("diff-text-2-false").unwrap();
                let right_after = cx.debug_bounds("diff-text-2-true").unwrap();
                assert_eq!(code_after, code_before);
                assert!(left_after.left() <= left_before.left());
                assert_eq!(
                    left_before.left() - left_after.left(),
                    right_before.left() - right_after.left()
                );
                assert_eq!(
                    right_short.left(),
                    cx.debug_bounds("diff-1-true").unwrap().left()
                );
                assert!(view.read_with(cx, |view, _| view.code_scroll.offset().x < px(0.0)));
            }

            view.update(cx, |view, cx| view.set_split(false, cx));
            cx.refresh().unwrap();
            assert_eq!(
                view.read_with(cx, |view, _| view.code_scroll.offset().x),
                px(0.0)
            );
            let unified_short = cx.debug_bounds("diff-1-true").unwrap();
            let unified_long = cx.debug_bounds("diff-2-true").unwrap();
            assert_eq!(unified_short.left(), viewer.left());
            assert_eq!(unified_short.right(), viewer.right());
            assert_eq!(unified_long.size.width, unified_short.size.width);
        }
    }

    #[gpui::test]
    fn hunk_navigation_wraps_and_preserves_source_rows_in_both_layouts(
        cx: &mut gpui::TestAppContext,
    ) {
        let file = ReviewFile {
            path: "project/file.rs".into(),
            kind: DiffKind::WorkingTree,
            position: 0,
            total: 1,
            marker: "M",
            commit: None,
        };
        let view = cx.new(|cx| {
            DiffView::new(
                "project".into(),
                file,
                DiffContent::Ready(
                    "@@ -1 +1 @@\n-old\n+new\n@@ -9 +9 @@\n-before\n+after\n".into(),
                ),
                cx,
            )
        });
        view.update(cx, |view, cx| {
            for split in [false, true] {
                view.set_split(split, cx);
                view.hunk_cursor = None;
                for source in [0, 2, 0] {
                    view.navigate_hunk(true, cx);
                    assert_eq!(view.selection, Some((true, source, source)));
                }
                view.navigate_hunk(false, cx);
                assert_eq!(view.selection, Some((true, 2, 2)));
            }
            let file = view.file.clone();
            view.update_snapshot(file, DiffContent::Ready(String::new()), cx);
            view.navigate_hunk(true, cx);
            assert_eq!(view.selection, None);
        });
    }

    #[gpui::test]
    fn switching_files_clears_old_diff_until_the_matching_snapshot_arrives(
        cx: &mut gpui::TestAppContext,
    ) {
        let file = ReviewFile {
            path: PathBuf::from("project/a.rs"),
            kind: crate::services::project_git::DiffKind::WorkingTree,
            position: 0,
            total: 2,
            marker: "M",
            commit: None,
        };
        let view = cx.new(|cx| {
            DiffView::new(
                "project".into(),
                file.clone(),
                DiffContent::Ready("@@ -1 +1 @@\n-old\n+new\n".into()),
                cx,
            )
        });
        view.update(cx, |view, cx| {
            view.set_split(true, cx);
            view.code_scroll.set_offset(point(px(-100.0), px(0.0)));
            let next = ReviewFile {
                path: "project/b.rs".into(),
                position: 1,
                ..file
            };
            view.update_snapshot(next.clone(), DiffContent::Loading, cx);
            assert!(view.raw.is_empty());
            assert!(view.display.is_empty());
            assert!(view.split);
            assert_eq!(view.code_scroll.offset().x, px(0.0));
            view.update_snapshot(
                next,
                DiffContent::Ready("@@ -1 +1 @@\n-before\n+after\n".into()),
                cx,
            );
            assert!(view.raw.contains("after"));
            assert!(!view.raw.contains("old"));
        });
    }

    #[test]
    fn unified_view_keeps_deletions_before_additions_and_context_once() {
        let sections = parse("@@ -1,3 +1,3 @@\n-old one\n-old two\n+new one\n+new two\n context\n");
        let rows = display_rows(&sections[0].rows, false);
        assert_eq!(
            rows.iter().map(|row| row.side).collect::<Vec<_>>(),
            vec![None, Some(false), Some(false), Some(true), Some(true), None]
        );
        assert_eq!(display_rows(&sections[0].rows, true).len(), 4);
    }

    #[gpui::test]
    fn far_rows_remain_reachable_in_the_virtualized_view(cx: &mut gpui::TestAppContext) {
        let file = ReviewFile {
            path: "project/large.rs".into(),
            kind: DiffKind::WorkingTree,
            position: 0,
            total: 1,
            marker: "M",
            commit: None,
        };
        let mut diff = "@@ -1,300 +1,300 @@\n".to_owned();
        for index in 0..300 {
            diff.push_str(&format!(" line {index}\n"));
        }
        let (view, cx) = cx.add_window_view(|_, cx| {
            DiffView::new("project".into(), file, DiffContent::Ready(diff), cx)
        });
        cx.simulate_resize(gpui::size(px(900.0), px(600.0)));
        cx.refresh().unwrap();
        assert!(cx.debug_bounds("diff-290-true").is_none());
        view.update(cx, |view, cx| {
            view.scroll.scroll_to_item_strict(290, ScrollStrategy::Top);
            cx.notify();
        });
        cx.refresh().unwrap();
        let far_row = cx.debug_bounds("diff-290-true").unwrap();
        let viewport = cx.debug_bounds("diff-horizontal-scroll").unwrap();
        assert!(far_row.top() >= viewport.top());
        assert!(far_row.bottom() <= viewport.bottom());
        // GPUI retains past debug bounds; the scroll handle identifies the current viewport.
        assert!(
            view.read_with(cx, |view, _| view.scroll.0.borrow().base_handle.offset().y)
                < -px(250.0 * ROW_HEIGHT)
        );
    }

    #[test]
    fn combined_conflicts_and_newline_only_changes_remain_explicit() {
        let sections = parse("Working tree\n@@@ -1,1 -1,1 +1,5 @@@\n++<<<<<<< HEAD\n");
        assert!(sections[0].conflicted);
        assert!(sections[0].rows.is_empty());
        let sections = parse("@@ -1 +1 @@\n-same\n\\ No newline at end of file\n+same\n");
        assert_eq!(sections[0].metadata, ["Before: no newline at end of file"]);
        let sections = parse("@@ -1 +1 @@\n-same\n+same\n\\ No newline at end of file\n");
        assert_eq!(sections[0].metadata, ["After: no newline at end of file"]);
    }

    #[test]
    fn horizontal_extent_reserves_space_for_wide_unicode() {
        let sections = parse(&format!("@@ -0,0 +1 @@\n+{}\n", "界".repeat(80)));
        assert!(sections[0].code_width >= 80.0 * 2.0 * 7.2);
    }

    #[test]
    fn replacements_align_and_preserve_context_line_numbers() {
        let sections = parse(
            "Working tree\n\ndiff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -42,4 +42,5 @@ function\n context\n-old\n+new\n+extra\n tail\n",
        );
        let section = &sections[0];
        assert_eq!((section.additions, section.deletions), (2, 1));
        assert_eq!(section.rows.len(), 5);
        assert!(
            matches!(&section.rows[2], Row::Lines { before: Some(before), after: Some(after) } if before.number == 43 && before.text == "old" && after.number == 43 && after.text == "new")
        );
        assert!(
            matches!(&section.rows[3], Row::Lines { before: None, after: Some(after) } if after.number == 44 && after.text == "extra")
        );
        assert!(
            matches!(&section.rows[4], Row::Lines { before: Some(before), after: Some(after) } if before.number == 44 && after.number == 45 && !before.changed)
        );
    }

    #[test]
    fn staged_and_worktree_sections_remain_distinct() {
        let sections = parse(
            "Staged\n\n@@ -0,0 +1 @@\n+first\n\\ No newline at end of file\n\nWorking tree\n\n@@ -1 +1 @@\n-first\n+second\n",
        );
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].label, "Staged");
        assert_eq!(sections[1].label, "Working tree");
        assert_eq!((sections[0].additions, sections[0].deletions), (1, 0));
        assert_eq!((sections[1].additions, sections[1].deletions), (1, 1));
    }

    #[test]
    fn file_headers_binary_metadata_and_truncation_are_not_code() {
        let sections = parse(
            "Working tree\n\n--- a/image\n+++ b/image\nBinary files a/image and b/image differ\n\nDiff truncated: use Git in a terminal to inspect the full change.\n",
        );
        assert!(sections[0].binary && sections[0].truncated);
        assert!(sections[0].rows.is_empty());
        assert_eq!(sections[0].additions, 0);
        assert_eq!(
            parse("Working tree\nold mode 100644\nnew mode 100755\n")[0]
                .metadata
                .len(),
            2
        );
    }

    #[test]
    fn multiple_hunks_flush_uneven_deletions_and_keep_unicode() {
        let sections = parse("@@ -1,2 +0,0 @@\n-α\n-β\n@@ -10 +8 @@\n café\n");
        assert_eq!(sections[0].rows.len(), 5);
        assert!(
            matches!(&sections[0].rows[2], Row::Lines { before: Some(line), after: None } if line.number == 2 && line.text == "β")
        );
        assert!(
            matches!(&sections[0].rows[4], Row::Lines { before: Some(before), after: Some(after) } if before.number == 10 && after.number == 8 && after.text == "café")
        );
        assert_eq!(hunk_start("@@ invalid @@"), None);
        assert_eq!(parse("")[0].rows.len(), 0);
    }
}
