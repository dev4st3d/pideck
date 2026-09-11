//! Read-only, aligned Git changes. Parsing is independent of GPUI and happens once per snapshot.

use std::path::PathBuf;

use gpui::{
    ClipboardItem, Context, EventEmitter, FocusHandle, FontWeight, IntoElement, KeyDownEvent,
    MouseButton, Render, ScrollStrategy, SharedString, UniformListScrollHandle, Window, div,
    prelude::*, px, svg, uniform_list,
};

use crate::services::project_git::{DiffContent, ReviewFile};
use crate::theme::{self, terminal_manager as chrome};
use crate::views::terminal_manager::text_tooltip;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    number: usize,
    text: String,
    changed: bool,
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
    min_width: f32,
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
            min_width: 800.0,
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
                before.push(Line {
                    number: old_number,
                    text: text.into(),
                    changed: true,
                });
                old_number += 1;
                section.deletions += 1;
            } else if let Some(text) = line.strip_prefix('+') {
                after.push(Line {
                    number: new_number,
                    text: text.into(),
                    changed: true,
                });
                new_number += 1;
                section.additions += 1;
            } else if let Some(text) = line.strip_prefix(' ') {
                flush_changes(&mut section.rows, &mut before, &mut after);
                section.rows.push(Row::Lines {
                    before: Some(Line {
                        number: old_number,
                        text: text.into(),
                        changed: false,
                    }),
                    after: Some(Line {
                        number: new_number,
                        text: text.into(),
                        changed: false,
                    }),
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
        section.min_width = (columns as f32 * 7.2 * 2.0 + 124.0).max(800.0);
    }
    sections
}

#[derive(Clone, Copy)]
pub(super) enum DiffEvent {
    OpenFile,
    Navigate(i32),
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
    pub(super) focus: FocusHandle,
    scroll: UniformListScrollHandle,
    selection: Option<(bool, usize, usize)>,
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
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            selection: None,
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
        let same_file = self.file.path == file.path && self.file.kind == file.kind;
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
            self.scroll = UniformListScrollHandle::new();
        }
        self.raw = raw;
        self.sections = parse(&self.raw);
        self.active = self.sections.len().saturating_sub(1);
        self.display = display_rows(&self.sections[self.active].rows, self.split);
        self.content = content;
        cx.notify();
    }

    fn set_split(&mut self, split: bool, cx: &mut Context<Self>) {
        if self.split == split {
            return;
        }
        let top =
            (-f32::from(self.scroll.0.borrow().base_handle.offset().y) / 30.0).max(0.0) as usize;
        let source = self
            .display
            .get(top + self.first_hunk_offset())
            .map(|row| row.source);
        self.split = split;
        self.display = display_rows(&self.sections[self.active].rows, split);
        if let Some(source) = source
            && let Some(index) = self.display.iter().position(|row| row.source == source)
        {
            self.scroll.scroll_to_item_strict(
                index.saturating_sub(self.first_hunk_offset()),
                ScrollStrategy::Top,
            );
        }
        cx.notify();
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
        } else if key.modifiers.control && key.key == "o" {
            cx.emit(DiffEvent::OpenFile);
        } else if key.modifiers.alt && key.key == "up" {
            cx.emit(DiffEvent::Navigate(-1));
        } else if key.modifiers.alt && key.key == "down" {
            cx.emit(DiffEvent::Navigate(1));
        } else if key.key == "f5" {
            cx.emit(DiffEvent::Navigate(0));
        } else if key.modifiers.alt && key.key == "u" {
            self.set_split(false, cx);
        } else if key.modifiers.alt && key.key == "s" {
            self.set_split(true, cx);
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
                    .unwrap_or(0)
                    .saturating_sub(self.first_hunk_offset()),
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
            .id(SharedString::from(format!("diff-{index}-{right}")))
            .h(px(30.0))
            .flex_1()
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
                cell.child(div().w(px(16.0)).flex_shrink_0())
                    .child(gutter(number(before)))
            })
            .child(gutter(number(if self.split { line } else { after })))
            .child(div().w(px(20.0)).flex_shrink_0().child(if changed {
                if right { "+" } else { "−" }
            } else {
                ""
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(px(if self.split { 24.0 } else { 16.0 }))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(line.map_or_else(String::new, |line| line.text.replace('\t', "    "))),
            )
            .into_any_element()
    }

    fn first_hunk_offset(&self) -> usize {
        usize::from(
            self.display.first().is_some_and(|row| {
                matches!(self.sections[self.active].rows[row.source], Row::Hunk(_))
            }),
        )
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let display = self.display[index];
        match &self.sections[self.active].rows[display.source] {
            Row::Hunk(label) => div()
                .h(px(30.0))
                .px(px(24.0))
                .flex()
                .items_center()
                .bg(theme::chrome())
                .text_color(theme::ash())
                .child(label.clone())
                .into_any_element(),
            Row::Lines { before, after } if self.split => div()
                .h(px(30.0))
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

    fn control(id: &'static str, label: &'static str, enabled: bool) -> gpui::Stateful<gpui::Div> {
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
            .tooltip(text_tooltip(label))
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
        let section = &self.sections[self.active];
        let relative = self
            .file
            .path
            .strip_prefix(&self.workspace)
            .unwrap_or(&self.file.path);
        let name = relative
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let parent = relative
            .parent()
            .unwrap_or(std::path::Path::new(""))
            .to_string_lossy()
            .replace('\\', " / ")
            .replace('/', " / ")
            .replace("  /  ", " / ");
        let first_hunk = match section.rows.first() {
            Some(Row::Hunk(label)) => Some(label.clone()),
            _ => None,
        };
        let offset = self.first_hunk_offset();
        let rows = self.display.len();
        let can_previous = self.file.position > 0;
        let can_next = self.file.position + 1 < self.file.total;
        let ready = matches!(self.content, DiffContent::Ready(_));
        let state = match self.file.marker {
            "A" => "Added",
            "D" => "Deleted",
            "R" => "Renamed",
            "U" => "Untracked",
            "!" => "Conflict",
            _ => "Modified",
        };
        let staged = section.label == "Staged";
        div().id("working-changes").track_focus(&self.focus).tab_index(0).size_full().min_h_0().flex().flex_col()
            .bg(theme::canvas()).font_family(chrome::CHROME_FONT).font_weight(FontWeight::NORMAL)
            .text_size(px(13.0)).line_height(px(20.0)).text_color(theme::bone())
            .on_key_down(cx.listener(Self::on_key))
            .child(div().h(px(92.0)).flex_shrink_0().px(px(24.0)).py(px(16.0)).flex().flex_col().justify_between().border_b_1().border_color(theme::edge())
                .child(div().flex().items_center().gap(px(16.0))
                    .child(svg().path("icons/file-code.svg").size(px(20.0)).flex_shrink_0().text_color(theme::bone()))
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::MEDIUM).child(name))
                    .child(Self::control("previous-change", "Previous change · Alt+Up", can_previous)
                        .when(can_previous, |button| button.on_click(cx.listener(|_, _, _, cx| cx.emit(DiffEvent::Navigate(-1)))))
                        .child(svg().path("icons/chevron-up.svg").size(px(14.0)).text_color(theme::ash())))
                    .child(div().font_family(theme::mono()).text_size(px(12.0)).text_color(theme::ash()).child(format!("{} of {}", self.file.position + 1, self.file.total)))
                    .child(Self::control("next-change", "Next change · Alt+Down", can_next)
                        .when(can_next, |button| button.on_click(cx.listener(|_, _, _, cx| cx.emit(DiffEvent::Navigate(1)))))
                        .child(svg().path("icons/chevron-down.svg").size(px(14.0)).text_color(theme::ash()))))
                .child(div().flex().items_center().gap(px(16.0)).text_size(px(12.0)).text_color(theme::ash())
                    .child(div().id("diff-relative-path").flex_1().min_w_0().truncate().tooltip(text_tooltip(relative.to_string_lossy().into_owned())).child(parent))
                    .child(div().flex_shrink_0().child(format!("{state} · {}", if staged { "Staged" } else { "Unstaged" })))
                    .when(ready, |summary| summary
                        .child(div().font_family(theme::mono()).text_size(px(11.0)).text_color(theme::success()).child(format!("+{}", section.additions)))
                        .child(div().font_family(theme::mono()).text_size(px(11.0)).text_color(theme::error()).child(format!("−{}", section.deletions))))))
            .child(div().h(px(44.0)).flex_shrink_0().px(px(24.0)).flex().items_center().gap(px(12.0)).border_b_1().border_color(theme::edge())
                .child(div().flex().rounded(px(4.0)).bg(theme::floor())
                    .child(Self::control("diff-unified", "Unified diff · Alt+U", true).bg(if !self.split { theme::panel_hover() } else { theme::floor() })
                        .on_click(cx.listener(|view, _, _, cx| view.set_split(false, cx))).child("Unified"))
                    .child(Self::control("diff-split", "Split diff · Alt+S", true).bg(if self.split { theme::panel_hover() } else { theme::floor() })
                        .on_click(cx.listener(|view, _, _, cx| view.set_split(true, cx))).child("Split")))
                .child(div().text_size(px(11.0)).text_color(theme::ash()).child({
                    let count = section.rows.iter().filter(|row| matches!(row, Row::Hunk(_))).count();
                    format!("{count} hunk{}", if count == 1 { "" } else { "s" })
                }))
                .child(div().flex_1())
                .child(Self::control("diff-open-file", "Open file · Ctrl+O", true)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(DiffEvent::OpenFile)))
                    .child(svg().path("icons/pencil.svg").size(px(14.0)).text_color(theme::ash())).child("Open file"))
                .child({
                    use gpui_component::{button::{Button, ButtonVariants}, menu::{DropdownMenu, PopupMenuItem}};
                    let owner = cx.weak_entity();
                    Button::new("diff-menu").label("⋯").ghost().w(px(24.0)).h(px(28.0)).tooltip("Diff actions")
                        .dropdown_menu(move |menu, _, _| {
                            let copy_owner = owner.clone();
                            let retry_owner = owner.clone();
                            menu.item(PopupMenuItem::new("Copy diff").on_click(move |_, _, cx| { let _ = copy_owner.update(cx, |view, cx| view.copy(cx)); }))
                                .item(PopupMenuItem::new("Refresh diff").on_click(move |_, _, cx| { let _ = retry_owner.update(cx, |_, cx| cx.emit(DiffEvent::Navigate(0))); }))
                        })
                }))
            .when(matches!(self.content, DiffContent::Loading), |view| view.child(div().p(px(24.0)).text_color(theme::ash()).child("Loading changes…")))
            .when_some(match &self.content { DiffContent::Error(error) => Some(error.clone()), _ => None }, |view, error| view
                .child(div().p(px(24.0)).flex().flex_col().gap(px(12.0)).text_color(theme::ash()).child(error)
                    .child(Self::control("retry-diff", "Retry diff · F5", true).on_click(cx.listener(|_, _, _, cx| cx.emit(DiffEvent::Navigate(0)))).child("Retry"))))
            .when(ready && (section.truncated || section.binary || section.conflicted || rows == 0), |view| view.child(
                div().p(px(24.0)).text_color(theme::ash()).child(if section.truncated { "This diff exceeds the preview limit. Open file to inspect it." }
                    else if section.binary { "Binary file changed. Open file to inspect it." }
                    else if section.conflicted { "This file has merge conflicts. Open file to review its conflict markers." }
                    else { "No text changes in this snapshot." })))
            .when(ready && !section.metadata.is_empty(), |view| view.child(div().px(px(24.0)).py(px(8.0)).text_size(px(11.0)).text_color(theme::ash()).child(section.metadata.join(" · "))))
            .when(ready && rows > 0, |view| view.child(
                div().id("diff-horizontal-scroll").flex_1().min_h_0().overflow_x_scroll()
                    .child(div().h_full().w_full().min_w(px(if self.split { section.min_width } else { (section.min_width / 2.0 + 80.0).max(480.0) })).flex().flex_col()
                        .when_some(first_hunk, |grid, label| grid.child(div().h(px(40.0)).flex_shrink_0().px(px(24.0)).flex().items_center().font_family(theme::mono()).text_size(px(12.0)).bg(theme::chrome()).text_color(theme::ash()).child(label)))
                        .when(self.split, |grid| grid.child(div().h(px(34.0)).flex_shrink_0().flex().bg(theme::chrome()).text_size(px(11.0)).text_color(theme::ash())
                            .child(div().flex_1().px(px(20.0)).child("Before"))
                            .child(div().flex_1().px(px(20.0)).child(if staged { "Index" } else { "Working tree" }))))
                        .child(uniform_list("diff-lines", rows - offset, cx.processor(|view, range: std::ops::Range<usize>, _, cx| {
                            let offset = view.first_hunk_offset();
                            range.map(|index| view.row(index + offset, cx)).collect::<Vec<_>>()
                        })).track_scroll(self.scroll.clone()).flex_1().min_h_0().font_family(theme::mono()).text_size(px(12.0)).line_height(px(30.0))))
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let next = ReviewFile {
                path: "project/b.rs".into(),
                position: 1,
                ..file
            };
            view.update_snapshot(next.clone(), DiffContent::Loading, cx);
            assert!(view.raw.is_empty());
            assert!(view.display.is_empty());
            assert!(view.split);
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
        assert!(sections[0].min_width >= 2.0 * (80.0 * 2.0 * 7.2 + 62.0));
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
