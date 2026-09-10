//! Project-owned sidebar state. Directory and Git work runs on background tasks.

use super::terminal::TerminalView;
use crate::services::file_operations::{self, Operation};
use gpui::{
    ClipboardItem, Entity, ExternalPaths, MouseButton, Subscription, Task, WeakEntity, img,
};
use gpui_component::input::InputState;
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
mod file_actions;

use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, IntoElement, KeyDownEvent, Render,
    ScrollStrategy, SharedString, UniformListScrollHandle, Window, div, prelude::*, px, rgba, svg,
    uniform_list,
};

use super::terminal_manager::text_tooltip;
use crate::services::project_files::{self, DirectoryEntry, DirectoryListing};
use crate::services::project_git::{self, DiffKind, GitEntry, GitLineStats, GitStatus};
use crate::theme::{self, terminal_manager as chrome};

const FILE_ROW_HEIGHT: f32 = chrome::TREE_ROW_HEIGHT;
const GIT_ROW_HEIGHT: f32 = chrome::TREE_ROW_HEIGHT;
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
    FilesChanged,
    BranchChanged,
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
    marked: HashSet<PathBuf>,
    anchor: usize,
    terminal: WeakEntity<TerminalView>,
    operation: Option<Arc<AtomicBool>>,
    operation_error: Option<String>,
    edit: Option<file_actions::NameEdit>,
    watcher: Option<notify::RecommendedWatcher>,
    watch_task: Option<Task<()>>,
    watching: bool,
    hide_hidden: bool,
    context_menu: Option<(Entity<PopupMenu>, gpui::Point<gpui::Pixels>, Subscription)>,
}

impl FilesPanel {
    pub(super) fn new(
        root: PathBuf,
        terminal: WeakEntity<TerminalView>,
        cx: &mut Context<Self>,
    ) -> Self {
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
            marked: HashSet::new(),
            anchor: 0,
            terminal,
            operation: None,
            operation_error: None,
            edit: None,
            watcher: None,
            watch_task: None,
            watching: false,
            hide_hidden: false,
            context_menu: None,
        }
    }

    pub(super) fn activate(&mut self, cx: &mut Context<Self>) {
        self.start_watching(cx);
        if !self.directories.contains_key(&self.root) {
            self.load(self.root.clone(), cx);
        }
    }

    pub(super) fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(cx);
        self.selection_visible = true;
        if self.marked.is_empty() {
            self.select(self.selected, false, false, self.selected);
        }
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
        for entry in &status.entries {
            let mut parent = entry.path.parent();
            while let Some(path) = parent {
                if path == self.root || !path.starts_with(&self.root) {
                    break;
                }
                self.git_markers.entry(path.to_path_buf()).or_insert("M");
                parent = path.parent();
            }
        }
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
        if self.hide_hidden {
            self.rows.retain(|row| {
                !row.entry
                    .path
                    .strip_prefix(&self.root)
                    .unwrap_or(&row.entry.path)
                    .components()
                    .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
            });
        }
        self.marked
            .retain(|path| self.rows.iter().any(|row| &row.entry.path == path));
        self.selected = selected_path
            .and_then(|path| self.rows.iter().position(|row| row.entry.path == path))
            .unwrap_or(self.selected.min(self.rows.len().saturating_sub(1)));
        if self.selection_visible && self.marked.is_empty() {
            self.select(self.selected, false, false, self.selected);
        }
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
        if self.file_shortcut(event, window, cx) {
            return;
        }
        let previous = self.selected;
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
        self.select(
            self.selected,
            false,
            event.keystroke.modifiers.shift,
            previous,
        );
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
        let selected = self.marked.contains(&row.entry.path)
            || ((self.selection_visible || focused) && index == self.selected);
        let drag = file_actions::FileDrag {
            paths: if self.marked.contains(&row.entry.path) {
                self.selected_paths()
            } else {
                vec![row.entry.path.clone()]
            },
        };
        let destination = if row.entry.is_dir {
            row.entry.path.clone()
        } else {
            row.entry.path.parent().unwrap_or(&self.root).to_path_buf()
        };
        let external_destination = destination.clone();
        div()
            .h(px(FILE_ROW_HEIGHT))
            .w_full()
            .px(px(chrome::COMPACT_GAP))
            .child(
                div()
                    .id(("file-row", index))
                    .debug_selector(move || format!("explorer-row-{index}"))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |view, event: &gpui::MouseDownEvent, window, cx| {
                            view.open_context_menu(index, event.position, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .size_full()
                    .min_w_0()
                    .overflow_hidden()
                    .pl(px(
                        chrome::SMALL_GAP + row.depth as f32 * chrome::TREE_INDENT
                    ))
                    .pr(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(chrome::SMALL_GAP))
                    .font_family(chrome::CHROME_FONT)
                    .font_weight(FontWeight::NORMAL)
                    .text_size(px(chrome::CHROME_TEXT_SIZE))
                    .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                    .text_color(
                        self.git_markers
                            .get(&row.entry.path)
                            .map_or(theme::bone(), |marker| marker_color(marker)),
                    )
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
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, event: &gpui::MouseDownEvent, window, cx| {
                            window.focus(&view.focus);
                            if event.modifiers.control
                                || event.modifiers.shift
                                || !view.marked.contains(&view.rows[index].entry.path)
                            {
                                view.select(
                                    index,
                                    event.modifiers.control,
                                    event.modifiers.shift,
                                    view.selected,
                                );
                            }
                            view.selection_visible = true;
                            cx.notify();
                        }),
                    )
                    .on_click(cx.listener(move |view, event: &gpui::ClickEvent, _, cx| {
                        if !event.modifiers().control && !event.modifiers().shift {
                            view.select(index, false, false, index);
                            let directory =
                                view.rows.get(index).is_some_and(|row| row.entry.is_dir);
                            if directory || event.click_count() == 2 {
                                view.open_row(index, cx);
                            }
                        }
                        cx.notify();
                    }))
                    .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                    .drag_over::<file_actions::FileDrag>(|style, _, _, _| {
                        style.bg(theme::selection())
                    })
                    .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(theme::selection()))
                    .on_drop(
                        cx.listener(move |view, drag: &file_actions::FileDrag, window, cx| {
                            view.perform(
                                Operation::Transfer {
                                    paths: drag.paths.clone(),
                                    destination: destination.clone(),
                                    cut: !window.modifiers().control,
                                },
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .on_drop(cx.listener(move |view, drag: &ExternalPaths, window, cx| {
                        view.perform(
                            Operation::Transfer {
                                paths: drag.paths().to_vec(),
                                destination: external_destination.clone(),
                                cut: false,
                            },
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }))
                    .child(
                        div()
                            .w(px(12.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .when(row.entry.is_dir, |icon| {
                                icon.child(panel_icon(if expanded {
                                    "icons/chevron-down.svg"
                                } else {
                                    "icons/chevron-right.svg"
                                }))
                            }),
                    )
                    .child(if row.entry.is_dir {
                        svg()
                            .path("icons/folder.svg")
                            .size(px(PANEL_ICON_SIZE))
                            .flex_shrink_0()
                            .text_color(if expanded {
                                theme::focus()
                            } else {
                                theme::ash()
                            })
                            .into_any_element()
                    } else {
                        div()
                            .flex_shrink_0()
                            .opacity(0.8)
                            .child(project_icon(&row.entry.path, false, false))
                            .into_any_element()
                    })
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
            .child(self.toolbar(cx))
            .when_some(self.edit.as_ref(), |panel, edit| {
                panel.child(edit.render(cx))
            })
            .child(
                div()
                    .id("project-root")
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.marked.clear();
                        view.selection_visible = false;
                        window.focus(&view.focus);
                        cx.notify();
                    }))
                    .on_drop(
                        cx.listener(|view, drag: &file_actions::FileDrag, window, cx| {
                            view.perform(
                                Operation::Transfer {
                                    paths: drag.paths.clone(),
                                    destination: view.root.clone(),
                                    cut: !window.modifiers().control,
                                },
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .on_drop(cx.listener(|view, drag: &ExternalPaths, window, cx| {
                        view.perform(
                            Operation::Transfer {
                                paths: drag.paths().to_vec(),
                                destination: view.root.clone(),
                                cut: false,
                            },
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }))
                    .h(px(chrome::CONTROL_HEIGHT))
                    .flex_shrink_0()
                    .px(px(chrome::SIDEBAR_INSET))
                    .flex()
                    .items_center()
                    .gap(px(chrome::SMALL_GAP))
                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                    .text_color(theme::ash())
                    .child(panel_icon("icons/folder.svg"))
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
            .when_some(self.operation_error.clone().or(error), |panel, error| {
                panel.child(message(error))
            })
            .when(truncated, |panel| {
                panel.child(message("Large folder: showing the first 2,000 entries."))
            })
            .when_some(self.context_menu.as_ref(), |panel, (menu, position, _)| {
                panel.child(
                    gpui::deferred(
                        gpui::anchored()
                            .position(*position)
                            .snap_to_window_with_margin(px(8.0))
                            .child(
                                div()
                                    .id("explorer-context-menu")
                                    .debug_selector(|| "explorer-context-menu".into())
                                    .child(menu.clone()),
                            ),
                    )
                    .with_priority(1),
                )
            })
    }
}

pub(super) struct GitPanel {
    root: PathBuf,
    terminal: WeakEntity<TerminalView>,
    rows: Vec<project_git::ChangeRow>,
    collapsed: HashSet<(DiffKind, PathBuf)>,
    branches: Vec<String>,
    branch_picker: bool,
    switching: bool,
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

    pub(super) fn new(
        root: PathBuf,
        terminal: WeakEntity<TerminalView>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            root,
            terminal,
            rows: Vec::new(),
            collapsed: HashSet::new(),
            branches: Vec::new(),
            branch_picker: false,
            switching: false,
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
        let task = cx.background_executor().spawn(async move {
            let status = project_git::read_status(&root)?;
            let branches = project_git::branches(&root)?;
            Ok::<_, project_git::GitError>((status, branches))
        });
        cx.spawn(async move |view, cx| {
            let status = task.await;
            let _ = view.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                view.loading = false;
                match status {
                    Ok((status, branches)) => {
                        view.branches = branches;
                        view.status = Some(status);
                        view.rebuild_changes();
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

    fn open_entry(&mut self, entry: GitEntry, kind: DiffKind, cx: &mut Context<Self>) {
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
            let label = if kind == DiffKind::Staged {
                "Staged"
            } else {
                "Working tree"
            };
            let diff = project_git::file_diff(&root, &entry.path, kind)?;
            text.push_str(&format!("{label}\n\n{}\n", diff.text));
            if diff.truncated {
                text.push_str("\nDiff truncated: inspect the full change in a terminal.\n");
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

    fn rebuild_changes(&mut self) {
        let selected = self
            .rows
            .get(self.selected)
            .map(|row| (row.kind, row.path.clone()));
        self.rows = self
            .status
            .as_ref()
            .map(|status| project_git::change_rows(status, &self.collapsed))
            .unwrap_or_default();
        self.selected = selected
            .and_then(|key| {
                self.rows
                    .iter()
                    .position(|row| (row.kind, row.path.clone()) == key)
            })
            .unwrap_or(self.selected.min(self.rows.len().saturating_sub(1)));
    }

    fn open_change(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };
        self.selected = index;
        if let Some(entry) = row
            .entry
            .and_then(|index| self.status.as_ref()?.entries.get(index))
            .cloned()
        {
            self.open_entry(entry, row.kind, cx);
        } else {
            let key = (row.kind, row.path);
            if !self.collapsed.remove(&key) {
                self.collapsed.insert(key);
            }
            self.rebuild_changes();
        }
        cx.notify();
    }

    fn choose_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        if self.switching || self.branch() == Some(branch.as_str()) {
            self.branch_picker = false;
            cx.notify();
            return;
        }
        let Some(terminal) = self.terminal.upgrade() else {
            return;
        };
        if !terminal.update(cx, |terminal, cx| terminal.begin_file_change(false, cx)) {
            self.error =
                Some("Save or close changed editor files before switching branches.".into());
            cx.notify();
            return;
        }
        self.switching = true;
        self.error = None;
        self.branch_picker = false;
        self.diff_generation += 1;
        self.diff_loading = None;
        let root = self.root.clone();
        let task = cx
            .background_executor()
            .spawn(async move { project_git::switch_branch(&root, &branch) });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = terminal.update(cx, |terminal, cx| terminal.end_file_change(cx));
            let _ = view.update(cx, |view, cx| {
                view.switching = false;
                match result {
                    Ok(()) => {
                        view.refresh(cx);
                        cx.emit(ProjectPanelEvent::BranchChanged);
                    }
                    Err(error) => {
                        view.error = Some(error.to_string());
                    }
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
        match event.keystroke.key.as_str() {
            "f5" => self.refresh(cx),
            "b" => self.branch_picker = !self.branch_picker,
            "escape" => self.branch_picker = false,
            "up" => self.selected = self.selected.saturating_sub(1),
            "down" => self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1)),
            "home" => self.selected = 0,
            "end" => self.selected = self.rows.len().saturating_sub(1),
            "enter" | "space" => self.open_change(self.selected, cx),
            "left" | "right" => {
                if let Some(row) = self.rows.get(self.selected) {
                    let key = (row.kind, row.path.clone());
                    if row.entry.is_none()
                        && (self.collapsed.contains(&key) == (event.keystroke.key == "right"))
                    {
                        self.open_change(self.selected, cx);
                    } else if event.keystroke.key == "left" {
                        let depth = row.depth;
                        if let Some(index) = (0..self.selected)
                            .rev()
                            .find(|index| self.rows[*index].depth < depth)
                        {
                            self.selected = index;
                        }
                    } else {
                        self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1));
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
        let entry = row
            .entry
            .and_then(|index| self.status.as_ref()?.entries.get(index));
        let marker = entry.map(git_marker);
        let expanded = !self.collapsed.contains(&(row.kind, row.path.clone()));
        let label = if row.path.as_os_str().is_empty() {
            if row.kind == DiffKind::Staged {
                "Staged".to_owned()
            } else {
                "Unstaged".to_owned()
            }
        } else {
            row.path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        };
        let selected = (self.selection_visible || focused) && self.selected == index;
        div()
            .id(("git-row", index))
            .h(px(GIT_ROW_HEIGHT))
            .mx(px(chrome::TREE_INSET))
            .pl(px(row.depth as f32 * chrome::TREE_INDENT))
            .pr(px(6.0))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(4.0))
            .text_size(px(chrome::CHROME_TEXT_SIZE))
            .text_color(marker.map_or(theme::bone(), marker_color))
            .border_1()
            .border_color(if selected && focused {
                theme::focus()
            } else {
                rgba(0)
            })
            .bg(if selected {
                theme::selection()
            } else {
                theme::floor()
            })
            .hover(|style| style.bg(theme::panel_hover()))
            .cursor_pointer()
            .tooltip(text_tooltip(row.path.to_string_lossy().into_owned()))
            .on_click(cx.listener(move |view, _, window, cx| {
                window.focus(&view.focus);
                view.selection_visible = true;
                view.open_change(index, cx);
            }))
            .child(
                div()
                    .w(px(12.0))
                    .flex_shrink_0()
                    .when(entry.is_none(), |item| {
                        item.child(panel_icon(if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        }))
                    }),
            )
            .when(!row.path.as_os_str().is_empty(), |item| {
                item.child(project_icon(&row.path, entry.is_none(), expanded))
            })
            .child(div().min_w_0().flex_1().truncate().child(label))
            .when_some(marker, |item, marker| {
                item.child(row_metadata(marker, selected))
            })
            .when_some(
                entry
                    .filter(|entry| !(entry.staged() && entry.unstaged()))
                    .and_then(|entry| entry.line_stats),
                |item, stats| {
                    item.child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(chrome::DETAIL_TEXT_SIZE))
                            .font_family(theme::mono())
                            .child(format!("+{} ?{}", stats.additions, stats.deletions)),
                    )
                },
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
                        .child(
                            div()
                                .id("choose-branch")
                                .tab_index(0)
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .cursor_pointer()
                                .focus(|style| style.bg(theme::selection()))
                                .tooltip(text_tooltip("Choose branch ? B"))
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.branch_picker = !view.branch_picker;
                                    cx.notify();
                                }))
                                .child(format!("{branch} ?")),
                        )
                        .child(div().flex_shrink_0().child(format!("{count} FILES"))),
                )
            })
            .when(self.switching, |panel| {
                panel.child(message("Switching branch?"))
            })
            .when(self.branch_picker, |panel| {
                panel.child(
                    div()
                        .id("branch-list")
                        .max_h(px(240.0))
                        .overflow_y_scroll()
                        .px(px(6.0))
                        .children(self.branches.iter().enumerate().map(|(index, branch)| {
                            let branch = branch.clone();
                            div()
                                .id(("branch-option", index))
                                .tab_index(0)
                                .h(px(28.0))
                                .px(px(6.0))
                                .cursor_pointer()
                                .hover(|style| style.bg(theme::panel_hover()))
                                .focus(|style| style.bg(theme::selection()))
                                .child(branch.clone())
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.choose_branch(branch.clone(), cx)
                                }))
                        }))
                        .when(self.branches.is_empty(), |list| {
                            list.child(message("No local branches yet."))
                        }),
                )
            })
            .child(
                uniform_list(
                    "git-files",
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

fn project_icon(path: &std::path::Path, directory: bool, expanded: bool) -> impl IntoElement {
    img(crate::assets::project_icon(path, directory, expanded))
        .size(px(16.0))
        .flex_shrink_0()
}

fn marker_color(marker: &str) -> gpui::Rgba {
    match marker {
        "!" | "D" => theme::error(),
        "A" | "U" => theme::focus(),
        _ => theme::working(),
    }
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
