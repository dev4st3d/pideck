//! Read-only, aligned Git changes. Parsing is independent of GPUI and happens once per snapshot.

use std::path::PathBuf;

use gpui::{
    ClipboardItem, Context, EventEmitter, FocusHandle, FontWeight, IntoElement, KeyDownEvent,
    MouseButton, Render, ScrollStrategy, SharedString, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

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

pub(super) struct DiffView {
    workspace: PathBuf,
    path: PathBuf,
    raw: String,
    sections: Vec<Section>,
    active: usize,
    changed_files: usize,
    line_stats: Option<crate::services::project_git::GitLineStats>,
    pub(super) focus: FocusHandle,
    scroll: UniformListScrollHandle,
    selection: Option<(bool, usize, usize)>,
}

impl DiffView {
    pub(super) fn new(
        workspace: PathBuf,
        path: PathBuf,
        text: String,
        changed_files: usize,
        line_stats: Option<crate::services::project_git::GitLineStats>,
        cx: &mut Context<Self>,
    ) -> Self {
        let sections = parse(&text);
        let active = sections.len().saturating_sub(1);
        Self {
            workspace,
            path,
            raw: text,
            sections,
            active,
            changed_files,
            line_stats,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            selection: None,
        }
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
            cx.emit(self.path.clone());
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
                end.saturating_sub(self.first_hunk_offset()),
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
        line: Option<&Line>,
        right: bool,
        index: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected = self.selection.is_some_and(|(side, anchor, end)| {
            side == right && (anchor.min(end)..=anchor.max(end)).contains(&index)
        });
        let background = if selected {
            theme::selection()
        } else {
            match line {
                None => theme::diff_empty(),
                Some(line) if line.changed => {
                    if right {
                        theme::diff_added()
                    } else {
                        theme::diff_removed()
                    }
                }
                Some(_) => theme::canvas(),
            }
        };
        let (number, marker, text) = line.map_or((String::new(), "", String::new()), |line| {
            (
                line.number.to_string(),
                if line.changed {
                    if right { "+" } else { "−" }
                } else {
                    ""
                },
                line.text.replace('\t', "    "),
            )
        });
        div()
            .id(SharedString::from(format!("diff-{index}-{right}")))
            .h(px(24.0))
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .bg(background)
            .when(right, |cell| {
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
            .child(
                div()
                    .w(px(42.0))
                    .flex_shrink_0()
                    .pr(px(10.0))
                    .text_right()
                    .text_color(if selected {
                        theme::bone()
                    } else {
                        theme::ash()
                    })
                    .child(number),
            )
            .child(
                div()
                    .w(px(20.0))
                    .flex_shrink_0()
                    .text_color(if right {
                        theme::focus()
                    } else {
                        theme::error()
                    })
                    .child(marker),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(text),
            )
            .into_any_element()
    }

    fn first_hunk_offset(&self) -> usize {
        usize::from(matches!(
            self.sections[self.active].rows.first(),
            Some(Row::Hunk(_))
        ))
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        match &self.sections[self.active].rows[index] {
            Row::Hunk(label) => div()
                .h(px(24.0))
                .px(px(16.0))
                .flex()
                .items_center()
                .bg(theme::diff_empty())
                .text_color(theme::ash())
                .child(label.clone())
                .into_any_element(),
            Row::Lines { before, after } => div()
                .h(px(24.0))
                .flex()
                .child(self.cell(before.as_ref(), false, index, cx))
                .child(self.cell(after.as_ref(), true, index, cx))
                .into_any_element(),
        }
    }
}

impl EventEmitter<PathBuf> for DiffView {}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let section = &self.sections[self.active];
        let label = section.label;
        let rows = section.rows.len();
        let first_hunk = match section.rows.first() {
            Some(Row::Hunk(label)) => Some(label.clone()),
            _ => None,
        };
        let first_hunk_offset = self.first_hunk_offset();
        let project = self
            .workspace
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_uppercase();
        let relative = self
            .path
            .strip_prefix(&self.workspace)
            .unwrap_or(&self.path)
            .to_string_lossy()
            .replace('\\', " / ");
        let min_width = section.min_width;
        let message = if section.conflicted {
            "This file has merge conflicts. Open the file to review its conflict markers."
        } else if section.binary {
            "Binary file changed. Open the file to inspect it."
        } else {
            "No text changes in this snapshot."
        };
        let before_label = if label == "Staged"
            || !self
                .sections
                .iter()
                .any(|section| section.label == "Staged")
        {
            "BEFORE / HEAD"
        } else {
            "BEFORE / INDEX"
        };
        let after_label = if label == "Staged" {
            "AFTER / INDEX"
        } else {
            "AFTER / WORKING TREE"
        };
        let totals = self.line_stats.as_ref();
        div()
            .id("working-changes")
            .track_focus(&self.focus)
            .tab_index(0)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .px(px(chrome::CONTENT_INSET))
            .pt(px(23.0))
            .pb(px(20.0))
            .bg(theme::canvas())
            .font_family(chrome::CHROME_FONT)
            .text_size(px(13.0))
            .text_color(theme::bone())
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .flex_shrink_0()
                    .pb(px(16.0))
                    .border_b_1()
                    .border_color(theme::edge_soft())
                    .child(
                        div()
                            .mb(px(7.0))
                            .font_family(theme::mono())
                            .text_size(px(11.0))
                            .line_height(px(14.0))
                            .text_color(theme::ash())
                            .child(format!("{project} / {}", label.to_uppercase())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .font_family(chrome::HEADING_FONT)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_size(px(chrome::HEADING_SIZE))
                                    .line_height(px(chrome::HEADING_LINE_HEIGHT))
                                    .child("Working changes"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(20.0))
                                    .child(format!(
                                        "{} changed {}",
                                        self.changed_files,
                                        if self.changed_files == 1 {
                                            "file"
                                        } else {
                                            "files"
                                        }
                                    ))
                                    .when_some(totals, |summary, totals| {
                                        summary
                                            .child(
                                                div().text_color(theme::focus()).child(format!(
                                                    "+{} additions",
                                                    totals.additions
                                                )),
                                            )
                                            .child(
                                                div().text_color(theme::error()).child(format!(
                                                    "−{} deletions",
                                                    totals.deletions
                                                )),
                                            )
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(68.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .font_family(theme::mono())
                            .text_color(theme::ash())
                            .child(
                                self.path
                                    .extension()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                            ),
                    )
                    .child(
                        div()
                            .id("diff-current-file")
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .tooltip(text_tooltip(format!(
                                "Selected comparison: +{} additions, −{} deletions",
                                section.additions, section.deletions
                            )))
                            .child(relative),
                    )
                    .child(
                        div()
                            .id("diff-comparison")
                            .flex_shrink_0()
                            .px(px(8.0))
                            .py(px(6.0))
                            .when(self.sections.len() > 1, |button| {
                                button
                                    .tab_index(0)
                                    .cursor_pointer()
                                    .hover(|b| b.bg(theme::panel()))
                                    .focus(|b| b.bg(theme::selection()))
                                    .tooltip(text_tooltip(
                                        "Switch between staged and working tree changes",
                                    ))
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.active = (view.active + 1) % view.sections.len();
                                        view.selection = None;
                                        view.scroll = UniformListScrollHandle::new();
                                        cx.notify();
                                    }))
                            })
                            .child(if self.sections.len() > 1 {
                                format!("{label}  ⌄")
                            } else {
                                label.to_owned()
                            }),
                    )
                    .child(
                        div()
                            .id("diff-open-file")
                            .flex_shrink_0()
                            .py(px(6.0))
                            .tab_index(0)
                            .cursor_pointer()
                            .text_color(theme::focus())
                            .hover(|b| b.bg(theme::panel()))
                            .focus(|b| b.bg(theme::selection()))
                            .tooltip(text_tooltip("Open file · Ctrl+O"))
                            .on_click(cx.listener(|view, _, _, cx| cx.emit(view.path.clone())))
                            .child("Open file ↗"),
                    ),
            )
            .when(section.truncated, |view| {
                view.child(
                    div()
                        .pb(px(8.0))
                        .text_color(theme::error())
                        .child("Diff truncated. Use Git in a terminal to inspect the full change."),
                )
            })
            .when(!section.metadata.is_empty(), |view| {
                view.child(
                    div()
                        .pb(px(8.0))
                        .text_color(theme::ash())
                        .child(section.metadata.join(" · ")),
                )
            })
            .when(rows == 0, |view| {
                view.child(div().py(px(24.0)).text_color(theme::ash()).child(message))
            })
            .when(rows > 0, |view| {
                view.child(
                    div()
                        .id("diff-horizontal-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_x_scroll()
                        .child(
                            div()
                                .h_full()
                                .w_full()
                                .min_w(px(min_width.max(800.0)))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .h(px(36.0))
                                        .flex_shrink_0()
                                        .flex()
                                        .bg(theme::panel())
                                        .border_t_1()
                                        .border_b_1()
                                        .border_color(theme::edge_soft())
                                        .font_family(theme::mono())
                                        .text_size(px(11.0))
                                        .text_color(theme::ash())
                                        .child(
                                            div()
                                                .flex_1()
                                                .px(px(16.0))
                                                .flex()
                                                .items_center()
                                                .child(before_label),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .px(px(16.0))
                                                .flex()
                                                .items_center()
                                                .border_l_1()
                                                .border_color(theme::edge_soft())
                                                .child(after_label),
                                        ),
                                )
                                .when_some(first_hunk, |grid, label| {
                                    grid.child(
                                        div()
                                            .h(px(32.0))
                                            .flex_shrink_0()
                                            .px(px(16.0))
                                            .flex()
                                            .items_center()
                                            .font_family(theme::mono())
                                            .text_size(px(12.0))
                                            .bg(theme::diff_empty())
                                            .text_color(theme::ash())
                                            .child(label),
                                    )
                                })
                                .child(
                                    uniform_list(
                                        "diff-lines",
                                        rows - first_hunk_offset,
                                        cx.processor(
                                            |view, range: std::ops::Range<usize>, _, cx| {
                                                let offset = view.first_hunk_offset();
                                                range
                                                    .map(|index| view.row(index + offset, cx))
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(self.scroll.clone())
                                    .flex_1()
                                    .min_h_0()
                                    .font_family(theme::mono())
                                    .text_size(px(12.0))
                                    .line_height(px(24.0)),
                                ),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
