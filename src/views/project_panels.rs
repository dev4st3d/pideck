//! Project-owned sidebar state. Directory and Git work runs on background tasks.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, IntoElement, KeyDownEvent, Render,
    ScrollStrategy, SharedString, UniformListScrollHandle, Window, div, prelude::*, px, rgba, svg,
    uniform_list,
};

use super::terminal_manager::text_tooltip;
use crate::services::project_files::{self, DirectoryEntry, DirectoryListing};
use crate::services::project_git::{self, DiffKind, GitEntry, GitLineStats, GitStatus};
use crate::theme::{self, terminal_manager as chrome};

const FILE_ROW_HEIGHT: f32 = 28.0;
const GIT_ROW_HEIGHT: f32 = 48.0;
const ROW_RADIUS: f32 = 4.0;
const PANEL_ICON_SIZE: f32 = 14.0;

#[derive(Clone)]
pub(super) enum ProjectPanelEvent {
    OpenFile(PathBuf),
    OpenDiff {
        title: String,
        text: String,
        path: PathBuf,
        changed_files: usize,
        line_stats: Option<GitLineStats>,
    },
    ToggleSidebar,
}

#[derive(Default)]
struct DirectoryState {
    entries: Vec<DirectoryEntry>,
    loading: bool,
    generation: u64,
    error: Option<String>,
    truncated: bool,
}

#[derive(Clone)]
struct FileRow {
    entry: DirectoryEntry,
    depth: usize,
}

pub(super) struct FilesPanel {
    root: PathBuf,
    directories: HashMap<PathBuf, DirectoryState>,
    expanded: HashSet<PathBuf>,
    rows: Vec<FileRow>,
    selected: usize,
    selection_visible: bool,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    git_markers: HashMap<PathBuf, &'static str>,
}

impl FilesPanel {
    pub(super) fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        Self {
            root,
            directories: HashMap::new(),
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected: 0,
            selection_visible: false,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            git_markers: HashMap::new(),
        }
    }

    pub(super) fn activate(&mut self, cx: &mut Context<Self>) {
        if !self.directories.contains_key(&self.root) {
            self.load(self.root.clone(), cx);
        }
    }

    pub(super) fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(cx);
        self.selection_visible = true;
        window.focus(&self.focus);
        cx.notify();
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(self.root.clone(), cx);
        for directory in self.expanded.clone() {
            self.load(directory, cx);
        }
    }

    pub(super) fn set_git_status(&mut self, status: &GitStatus, cx: &mut Context<Self>) {
        self.git_markers = status
            .entries
            .iter()
            .map(|entry| (entry.path.clone(), git_marker(entry)))
            .collect();
        cx.notify();
    }

    fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let state = self.directories.entry(path.clone()).or_default();
        state.generation += 1;
        let generation = state.generation;
        state.loading = true;
        state.error = None;
        let root = self.root.clone();
        let directory = path.clone();
        let work = cx
            .background_executor()
            .spawn(async move { project_files::list_directory(&root, &directory) });
        cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                let Some(state) = view.directories.get_mut(&path) else {
                    return;
                };
                if state.generation != generation {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(DirectoryListing { entries, truncated }) => {
                        state.entries = entries;
                        state.truncated = truncated;
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
                view.rebuild_rows();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn rebuild_rows(&mut self) {
        fn append(
            path: &PathBuf,
            depth: usize,
            directories: &HashMap<PathBuf, DirectoryState>,
            expanded: &HashSet<PathBuf>,
            rows: &mut Vec<FileRow>,
        ) {
            let Some(directory) = directories.get(path) else {
                return;
            };
            for entry in &directory.entries {
                rows.push(FileRow {
                    entry: entry.clone(),
                    depth,
                });
                if entry.is_dir && !entry.is_symlink && expanded.contains(&entry.path) {
                    append(&entry.path, depth + 1, directories, expanded, rows);
                }
            }
        }
        let selected_path = self
            .rows
            .get(self.selected)
            .map(|row| row.entry.path.clone());
        self.rows.clear();
        append(
            &self.root,
            0,
            &self.directories,
            &self.expanded,
            &mut self.rows,
        );
        self.selected = selected_path
            .and_then(|path| self.rows.iter().position(|row| row.entry.path == path))
            .unwrap_or(self.selected.min(self.rows.len().saturating_sub(1)));
    }

    fn open_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };
        self.selected = index;
        if row.entry.is_dir {
            if row.entry.is_symlink {
                return;
            }
            if !self.expanded.remove(&row.entry.path) {
                self.expanded.insert(row.entry.path.clone());
                if !self.directories.contains_key(&row.entry.path) {
                    self.load(row.entry.path, cx);
                }
            }
            self.rebuild_rows();
        } else {
            cx.emit(ProjectPanelEvent::OpenFile(row.entry.path));
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        match event.keystroke.key.as_str() {
            "f5" => self.refresh(cx),
            "up" => self.selected = self.selected.saturating_sub(1),
            "down" => self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1)),
            "home" => self.selected = 0,
            "end" => self.selected = self.rows.len().saturating_sub(1),
            "enter" => self.open_row(self.selected, cx),
            "right" => {
                if let Some(row) = self.rows.get(self.selected) {
                    if row.entry.is_dir && !self.expanded.contains(&row.entry.path) {
                        self.open_row(self.selected, cx);
                    } else if row.entry.is_dir {
                        self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1));
                    }
                }
            }
            "left" => {
                if let Some(row) = self.rows.get(self.selected) {
                    if self.expanded.contains(&row.entry.path) {
                        self.open_row(self.selected, cx);
                    } else if let Some(parent) = row.entry.path.parent()
                        && let Some(index) =
                            self.rows.iter().position(|row| row.entry.path == parent)
                    {
                        self.selected = index;
                    }
                }
            }
            _ => return,
        }
        self.selection_visible = true;
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Center);
        cx.stop_propagation();
        cx.notify();
    }

    fn row(&self, index: usize, focused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[index];
        let path = row.entry.path.to_string_lossy().into_owned();
        let loading = self
            .directories
            .get(&row.entry.path)
            .is_some_and(|state| state.loading);
        let expanded = self.expanded.contains(&row.entry.path);
        let selected = (self.selection_visible || focused) && index == self.selected;
        div()
            .h(px(FILE_ROW_HEIGHT))
            .w_full()
            .px(px(chrome::SIDEBAR_INSET))
            .child(
                div()
                    .id(("file-row", index))
                    .size_full()
                    .min_w_0()
                    .overflow_hidden()
                    .pl(px(14.0 + row.depth as f32 * 14.0))
                    .pr(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(0.0))
                    .font_family(chrome::CHROME_FONT)
                    .font_weight(FontWeight::NORMAL)
                    .text_size(px(chrome::CHROME_TEXT_SIZE))
                    .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                    .text_color(theme::bone())
                    .rounded(px(ROW_RADIUS))
                    .border_1()
                    .border_color(if focused && selected {
                        theme::focus()
                    } else {
                        rgba(0)
                    })
                    .cursor_pointer()
                    .bg(if selected {
                        theme::selection()
                    } else {
                        theme::floor()
                    })
                    .hover(move |style| {
                        style.bg(if selected {
                            theme::selection()
                        } else {
                            theme::panel_hover()
                        })
                    })
                    .tooltip(text_tooltip(path))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        window.focus(&view.focus);
                        view.selection_visible = true;
                        view.open_row(index, cx);
                    }))
                    .child(
                        div()
                            .w(px(24.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .when(row.entry.is_dir, |icon| {
                                icon.child(panel_icon(if expanded {
                                    "icons/chevron-down.svg"
                                } else {
                                    "icons/chevron-right.svg"
                                }))
                            })
                            .when(!row.entry.is_dir, |icon| {
                                icon.child(file_marker(&row.entry.path))
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(row.entry.name.clone()),
                    )
                    .when(row.entry.is_symlink, |item| {
                        item.child(row_metadata("link", selected))
                    })
                    .when_some(
                        self.git_markers.get(&row.entry.path).copied(),
                        |item, marker| {
                            item.child(
                                div()
                                    .font_family(theme::mono())
                                    .text_size(px(11.0))
                                    .text_color(theme::focus())
                                    .child(marker),
                            )
                        },
                    )
                    .when(loading, |item| {
                        item.child(row_metadata("Loading", selected))
                    }),
            )
    }
}

impl EventEmitter<ProjectPanelEvent> for FilesPanel {}

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.directories.get(&self.root);
        let loading = status.is_none_or(|state| state.loading);
        let error = self
            .directories
            .values()
            .find_map(|state| state.error.clone());
        let truncated = self.directories.values().any(|state| state.truncated);
        div()
            .id("files-panel")
            .track_focus(&self.focus)
            .tab_index(0)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .font_family(chrome::CHROME_FONT)
            .font_weight(FontWeight::MEDIUM)
            .text_size(px(chrome::CHROME_TEXT_SIZE))
            .line_height(px(chrome::CHROME_LINE_HEIGHT))
            .text_color(theme::bone())
            .on_key_down(cx.listener(Self::on_key))
            .child(panel_header(
                "Explorer",
                loading,
                Some("icons/sidebar.svg"),
                cx.listener(|_, _, _, cx| cx.emit(ProjectPanelEvent::ToggleSidebar)),
            ))
            .child(
                div()
                    .h(px(FILE_ROW_HEIGHT))
                    .flex_shrink_0()
                    .px(px(chrome::SIDEBAR_INSET))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(panel_icon("icons/chevron-down.svg"))
                    .child(
                        div().flex_1().min_w_0().truncate().child(
                            self.root
                                .file_name()
                                .unwrap_or(self.root.as_os_str())
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    ),
            )
            .child(
                uniform_list(
                    "file-tree",
                    self.rows.len(),
                    cx.processor(|view, range: std::ops::Range<usize>, window, cx| {
                        let focused = view.focus.is_focused(window);
                        range
                            .map(|index| view.row(index, focused, cx).into_any_element())
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.scroll.clone())
                .flex_1()
                .min_h_0(),
            )
            .when(self.rows.is_empty(), |panel| {
                panel.child(message(if loading {
                    "Loading files…"
                } else {
                    "This folder is empty."
                }))
            })
            .when_some(error, |panel, error| panel.child(message(error)))
            .when(truncated, |panel| {
                panel.child(message("Large folder: showing the first 2,000 entries."))
            })
    }
}

pub(super) struct GitPanel {
    root: PathBuf,
    status: Option<GitStatus>,
    loading: bool,
    refresh_pending: bool,
    generation: u64,
    diff_generation: u64,
    diff_loading: Option<PathBuf>,
    error: Option<String>,
    selected: usize,
    selection_visible: bool,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl GitPanel {
    pub(super) fn status(&self) -> Option<&GitStatus> {
        self.status.as_ref()
    }

    pub(super) fn branch(&self) -> Option<&str> {
        self.status.as_ref().map(|status| status.branch.as_str())
    }

    pub(super) fn change_count(&self) -> Option<usize> {
        self.status.as_ref().map(|status| status.entries.len())
    }

    pub(super) fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        Self {
            root,
            status: None,
            loading: false,
            refresh_pending: false,
            generation: 0,
            diff_generation: 0,
            diff_loading: None,
            error: None,
            selected: 0,
            selection_visible: false,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
        }
    }

    pub(super) fn activate(&mut self, cx: &mut Context<Self>) {
        if self.status.is_none() && !self.loading {
            self.refresh(cx);
        }
    }

    pub(super) fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(cx);
        self.selection_visible = true;
        window.focus(&self.focus);
        cx.notify();
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            self.refresh_pending = true;
            return;
        }
        self.refresh_pending = false;
        self.generation += 1;
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        let root = self.root.clone();
        let task = cx
            .background_executor()
            .spawn(async move { project_git::read_status(&root) });
        cx.spawn(async move |view, cx| {
            let status = task.await;
            let _ = view.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                view.loading = false;
                match status {
                    Ok(status) => {
                        view.selected = view.selected.min(status.entries.len().saturating_sub(1));
                        view.status = Some(status);
                    }
                    Err(error) => view.error = Some(error.to_string()),
                }
                // A save may finish while Git is reading the previous state.
                if view.refresh_pending {
                    view.refresh(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn open_entry(&mut self, entry: GitEntry, cx: &mut Context<Self>) {
        self.diff_generation += 1;
        if entry.untracked {
            self.diff_loading = None;
            cx.emit(ProjectPanelEvent::OpenFile(entry.path));
            cx.notify();
            return;
        }
        let generation = self.diff_generation;
        self.diff_loading = Some(entry.path.clone());
        let path = entry.path.clone();
        let changed_files = self.change_count().unwrap_or(0);
        let line_stats = self.status.as_ref().and_then(GitStatus::line_stats);
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            let mut text = String::new();
            for (needed, kind, label) in [
                (entry.staged(), DiffKind::Staged, "Staged"),
                (entry.unstaged(), DiffKind::WorkingTree, "Working tree"),
            ] {
                if !needed {
                    continue;
                }
                let diff = project_git::file_diff(&root, &entry.path, kind)?;
                text.push_str(&format!("{label}\n\n{}\n", diff.text));
                if diff.truncated {
                    text.push_str(
                        "\nDiff truncated: use Git in a terminal to inspect the full change.\n",
                    );
                }
            }
            Ok::<_, project_git::GitError>((
                format!("{} — Changes", entry.relative_path.display()),
                text,
            ))
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                if view.diff_generation != generation {
                    return;
                }
                view.diff_loading = None;
                match result {
                    Ok((title, text)) => cx.emit(ProjectPanelEvent::OpenDiff {
                        title,
                        text,
                        path,
                        changed_files,
                        line_stats,
                    }),
                    Err(error) => view.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        if event.keystroke.key == "f5" {
            self.refresh(cx);
            cx.stop_propagation();
            return;
        }
        let Some(status) = &self.status else {
            return;
        };
        match event.keystroke.key.as_str() {
            "up" => self.selected = self.selected.saturating_sub(1),
            "down" => {
                self.selected = (self.selected + 1).min(status.entries.len().saturating_sub(1))
            }
            "enter" => {
                if let Some(entry) = status.entries.get(self.selected).cloned() {
                    self.open_entry(entry, cx);
                }
            }
            _ => return,
        }
        self.selection_visible = true;
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Center);
        cx.stop_propagation();
        cx.notify();
    }

    fn row(&self, index: usize, focused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let entry = self
            .status
            .as_ref()
            .expect("rows require loaded Git status")
            .entries[index]
            .clone();
        let path = entry.relative_path.to_string_lossy().replace('\\', "/");
        let stats = entry.line_stats;
        let marker = git_marker(&entry);
        let loading = self.diff_loading.as_ref() == Some(&entry.path);
        let selected = (self.selection_visible || focused) && index == self.selected;
        div()
            .h(px(GIT_ROW_HEIGHT))
            .w_full()
            .px(px(chrome::SIDEBAR_INSET))
            .pb(px(8.0))
            .child(
                div()
                    .id(("git-row", index))
                    .size_full()
                    .min_w_0()
                    .overflow_hidden()
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .font_family(chrome::CHROME_FONT)
                    .font_weight(FontWeight::NORMAL)
                    .text_size(px(chrome::CHROME_TEXT_SIZE))
                    .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                    .text_color(theme::bone())
                    .rounded(px(ROW_RADIUS))
                    .border_1()
                    .border_color(if focused && selected {
                        theme::focus()
                    } else {
                        rgba(0)
                    })
                    .cursor_pointer()
                    .bg(if selected {
                        theme::selection()
                    } else {
                        theme::floor()
                    })
                    .hover(move |style| {
                        style.bg(if selected {
                            theme::selection()
                        } else {
                            theme::panel_hover()
                        })
                    })
                    .tooltip(text_tooltip(path.clone()))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        window.focus(&view.focus);
                        view.selection_visible = true;
                        view.selected = index;
                        view.open_entry(entry.clone(), cx);
                    }))
                    .child(div().min_w_0().flex_1().truncate().child(path))
                    .when_some(stats, |row, stats| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .font_family(theme::mono())
                                .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                                .text_color(theme::focus())
                                .child(format!("+{} -{}", stats.additions, stats.deletions)),
                        )
                    })
                    .when(stats.is_none(), |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .font_family(theme::mono())
                                .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                                .text_color(theme::focus())
                                .child(marker),
                        )
                    })
                    .when(loading, |item| {
                        item.child(row_metadata("Loading", selected))
                    }),
            )
    }
}

impl EventEmitter<ProjectPanelEvent> for GitPanel {}

impl Render for GitPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self
            .status
            .as_ref()
            .map_or(0, |status| status.entries.len());
        let branch = self.status.as_ref().map(|status| status.branch.clone());
        div()
            .id("git-panel")
            .track_focus(&self.focus)
            .tab_index(0)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .font_family(chrome::CHROME_FONT)
            .font_weight(FontWeight::MEDIUM)
            .text_size(px(chrome::CHROME_TEXT_SIZE))
            .line_height(px(chrome::CHROME_LINE_HEIGHT))
            .text_color(theme::bone())
            .on_key_down(cx.listener(Self::on_key))
            .child(panel_header(
                "Working changes",
                self.loading,
                None,
                cx.listener(|view, _, _, cx| view.refresh(cx)),
            ))
            .when_some(branch, |panel, branch| {
                panel.child(
                    div()
                        .mx(px(chrome::SIDEBAR_INSET))
                        .h(px(34.0))
                        .mb(px(8.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .border_b_1()
                        .border_color(theme::edge())
                        .font_family(theme::mono())
                        .font_weight(FontWeight::NORMAL)
                        .text_size(px(chrome::TECH_TEXT_SIZE))
                        .text_color(theme::ash())
                        .child(panel_icon("icons/branch.svg"))
                        .child(div().min_w_0().flex_1().truncate().child(branch))
                        .child(div().flex_shrink_0().child(format!("{count} FILES"))),
                )
            })
            .child(
                uniform_list(
                    "git-files",
                    count,
                    cx.processor(|view, range: std::ops::Range<usize>, window, cx| {
                        let focused = view.focus.is_focused(window);
                        range
                            .map(|index| view.row(index, focused, cx).into_any_element())
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.scroll.clone())
                .flex_1()
                .min_h_0(),
            )
            .when(count == 0 && self.error.is_none(), |panel| {
                panel.child(message(if self.loading {
                    "Reading Git status…"
                } else {
                    "Working tree clean."
                }))
            })
            .when_some(self.error.clone(), |panel, error| {
                panel.child(message(error))
            })
            .when(
                self.status.as_ref().is_some_and(|status| status.truncated),
                |panel| panel.child(message("Large change list: some entries are hidden.")),
            )
    }
}

fn message(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .p(px(chrome::SIDEBAR_INSET))
        .text_color(theme::ash())
        .font_weight(FontWeight::NORMAL)
        .text_size(px(chrome::DETAIL_TEXT_SIZE))
        .line_height(px(chrome::DETAIL_LINE_HEIGHT))
        .whitespace_normal()
        .child(text.into())
}

fn panel_icon(path: &'static str) -> impl IntoElement {
    svg()
        .path(path)
        .size(px(PANEL_ICON_SIZE))
        .flex_shrink_0()
        .text_color(theme::ash())
}

fn row_metadata(text: &'static str, selected: bool) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .font_family(theme::mono())
        .font_weight(FontWeight::NORMAL)
        .text_size(px(chrome::DETAIL_TEXT_SIZE))
        .line_height(px(chrome::DETAIL_LINE_HEIGHT))
        .text_color(if selected {
            theme::bone()
        } else {
            theme::ash()
        })
        .child(text)
}

fn file_marker(path: &std::path::Path) -> impl IntoElement {
    let marker = match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => "rs",
        Some("js" | "jsx") => "js",
        Some("ts" | "tsx") => "ts",
        Some("py") => "py",
        Some("json") => "{}",
        Some("md") => "↓",
        _ => "↳",
    };
    div()
        .font_family(theme::mono())
        .text_size(px(chrome::DETAIL_TEXT_SIZE))
        .line_height(px(chrome::DETAIL_LINE_HEIGHT))
        .font_weight(FontWeight::NORMAL)
        .text_color(theme::ash())
        .child(marker)
}

fn git_marker(entry: &GitEntry) -> &'static str {
    if entry.conflicted {
        "!"
    } else if entry.untracked {
        "U"
    } else if entry.index_status == 'A' {
        "A"
    } else if entry.index_status == 'D' || entry.worktree_status == 'D' {
        "D"
    } else if entry.index_status == 'R' || entry.worktree_status == 'R' {
        "R"
    } else {
        "M"
    }
}

fn panel_header(
    label: &'static str,
    loading: bool,
    action_icon: Option<&'static str>,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .h(px(56.0))
        .px(px(chrome::SIDEBAR_INSET))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(chrome::SMALL_GAP))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .font_family(chrome::CHROME_FONT)
                .text_size(px(chrome::CHROME_TEXT_SIZE))
                .font_weight(FontWeight::MEDIUM)
                .truncate()
                .child(label),
        )
        .when(loading, |header| {
            header.child(row_metadata("Loading", false))
        })
        .when_some(action_icon, |header, icon| {
            header.child(
                div()
                    .id("collapse-panel")
                    .size(px(chrome::CONTROL_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(chrome::CONTROL_RADIUS))
                    .border_1()
                    .border_color(rgba(0))
                    .tab_index(0)
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::panel_hover()))
                    .focus(|style| style.bg(theme::selection()).border_color(theme::focus()))
                    .tooltip(text_tooltip(
                        "Hide sidebar · Ctrl+Shift+B\nRefresh files · F5",
                    ))
                    .on_click(listener)
                    .child(panel_icon(icon)),
            )
        })
}
