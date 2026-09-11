//! Project-owned sidebar state. Directory and Git work runs on background tasks.

use super::terminal::TerminalView;
use crate::services::file_operations::{self, Operation};
use gpui::{ClipboardItem, Entity, ExternalPaths, MouseButton, Subscription, Task, WeakEntity};
use gpui_component::input::{Input, InputEvent, InputState};
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
use crate::services::project_git::{self, DiffContent, DiffKind, GitEntry, GitStatus, ReviewFile};
use crate::theme::{self, terminal_manager as chrome};

const FILE_ROW_HEIGHT: f32 = chrome::TREE_ROW_HEIGHT;
const GIT_ROW_HEIGHT: f32 = chrome::TREE_ROW_HEIGHT;
const ROW_RADIUS: f32 = 4.0;
const PANEL_ICON_SIZE: f32 = 14.0;

#[derive(Clone)]
pub(super) enum ProjectPanelEvent {
    OpenFile(PathBuf),
    OpenDiff {
        file: ReviewFile,
        content: DiffContent,
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
    recovery: bool,
}

struct FileBrowseState {
    expanded: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    marked: HashSet<PathBuf>,
    scroll: UniformListScrollHandle,
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
    root_collapsed: bool,
    filter_input: Entity<InputState>,
    query: String,
    filter_loading: bool,
    filter_generation: u64,
    filter_cancel: Option<Arc<AtomicBool>>,
    filtered_entries: Vec<DirectoryEntry>,
    filter_truncated: bool,
    browse_state: Option<FileBrowseState>,
    _filter_subscription: Subscription,
    context_menu: Option<(Entity<PopupMenu>, gpui::Point<gpui::Pixels>, Subscription)>,
}

impl FilesPanel {
    pub(super) fn new(
        root: PathBuf,
        terminal: WeakEntity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter files…"));
        let subscription = cx.subscribe(&filter_input, |view, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                view.set_filter(input.read(cx).value().to_string(), cx);
            }
        });
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
            root_collapsed: false,
            filter_input,
            query: String::new(),
            filter_loading: false,
            filter_generation: 0,
            filter_cancel: None,
            filtered_entries: Vec::new(),
            filter_truncated: false,
            browse_state: None,
            _filter_subscription: subscription,
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

    fn set_filter(&mut self, query: String, cx: &mut Context<Self>) {
        let query = query.trim().to_owned();
        if self.query == query {
            return;
        }
        if self.query.is_empty() && !query.is_empty() {
            self.root_collapsed = false;
            self.browse_state = Some(FileBrowseState {
                expanded: self.expanded.clone(),
                selected: self
                    .rows
                    .get(self.selected)
                    .map(|row| row.entry.path.clone()),
                marked: self.marked.clone(),
                scroll: self.scroll.clone(),
            });
            self.scroll = UniformListScrollHandle::new();
        }
        self.query = query;
        self.filter_generation += 1;
        if let Some(cancel) = self.filter_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        if self.query.is_empty() {
            self.filter_loading = false;
            self.filtered_entries.clear();
            if let Some(state) = self.browse_state.take() {
                self.expanded = state.expanded;
                self.scroll = state.scroll;
                self.rebuild_rows();
                self.marked = state.marked;
                self.selected = state
                    .selected
                    .and_then(|path| self.rows.iter().position(|row| row.entry.path == path))
                    .unwrap_or(0);
            }
            self.rebuild_rows();
        } else {
            self.search_files(cx);
        }
        cx.notify();
    }

    fn search_files(&mut self, cx: &mut Context<Self>) {
        self.filter_generation += 1;
        let generation = self.filter_generation;
        if let Some(cancel) = self.filter_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.filter_cancel = Some(cancel.clone());
        self.filter_loading = true;
        let root = self.root.clone();
        let query = self.query.clone();
        let task = cx
            .background_executor()
            .spawn(async move { project_files::filter_files(&root, &query, &cancel) });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                if generation != view.filter_generation {
                    return;
                }
                view.filter_loading = false;
                match result {
                    Ok(listing) => {
                        view.expanded.extend(
                            listing
                                .entries
                                .iter()
                                .filter(|entry| entry.is_dir)
                                .map(|entry| entry.path.clone()),
                        );
                        view.filtered_entries = listing.entries;
                        view.filter_truncated = listing.truncated;
                        view.rebuild_rows();
                    }
                    Err(error) => view.operation_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.query.is_empty() {
            self.search_files(cx);
        }
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
        if !self.query.is_empty() {
            let selected = self
                .rows
                .get(self.selected)
                .map(|row| row.entry.path.clone());
            self.rows = self
                .filtered_entries
                .iter()
                .filter(|entry| {
                    !self.hide_hidden
                        || !entry
                            .path
                            .strip_prefix(&self.root)
                            .unwrap_or(&entry.path)
                            .components()
                            .any(|p| p.as_os_str().to_string_lossy().starts_with('.'))
                })
                .filter(|entry| {
                    let mut parent = entry.path.parent();
                    while let Some(path) = parent {
                        if path == self.root {
                            break;
                        }
                        if !self.expanded.contains(path)
                            && self.filtered_entries.iter().any(|item| item.path == path)
                        {
                            return false;
                        }
                        parent = path.parent();
                    }
                    true
                })
                .map(|entry| FileRow {
                    entry: entry.clone(),
                    depth: entry
                        .path
                        .strip_prefix(&self.root)
                        .unwrap_or(&entry.path)
                        .components()
                        .count()
                        .saturating_sub(1),
                    recovery: false,
                })
                .collect();
            self.selected = selected
                .and_then(|path| self.rows.iter().position(|row| row.entry.path == path))
                .unwrap_or(0);
            return;
        }
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
                    recovery: false,
                });
                if entry.is_dir
                    && expanded.contains(&entry.path)
                    && directories
                        .get(&entry.path)
                        .is_some_and(|state| state.error.is_some())
                {
                    rows.push(FileRow {
                        entry: entry.clone(),
                        depth: depth + 1,
                        recovery: true,
                    });
                }
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
        if row.recovery {
            self.load(row.entry.path, cx);
            return;
        }
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
        if self.root_collapsed && matches!(event.keystroke.key.as_str(), "right" | "enter" | "down")
        {
            self.root_collapsed = false;
            cx.notify();
            cx.stop_propagation();
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
        if row.recovery {
            let path = row.entry.path.clone();
            return div()
                .h(px(FILE_ROW_HEIGHT))
                .pl(px(24.0 + row.depth as f32 * chrome::TREE_INDENT))
                .pr(px(8.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_size(px(11.0))
                        .text_color(theme::error())
                        .child("Folder unavailable"),
                )
                .child(file_actions::control(
                    "Retry",
                    cx.listener(move |view, _, _, cx| view.load(path.clone(), cx)),
                ))
                .into_any_element();
        }
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
                    .text_color(theme::bone())
                    .rounded(px(ROW_RADIUS))
                    .border_1()
                    .border_color(if focused && selected {
                        theme::focus()
                    } else {
                        rgba(0)
                    })
                    .cursor_pointer()
                    .bg(if selected && !focused {
                        theme::panel_hover()
                    } else if selected {
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
                            .path(if expanded {
                                "icons/folder-open.svg"
                            } else {
                                "icons/folder.svg"
                            })
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
                            .child(filename(&row.entry.path)),
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
                        item.child(row_metadata("Refreshing…", selected))
                    }),
            )
            .into_any_element()
    }
}

impl EventEmitter<ProjectPanelEvent> for FilesPanel {}

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.directories.get(&self.root);
        let loading = status.is_none_or(|state| state.loading);
        let error = status.and_then(|state| state.error.clone());
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
            .font_weight(FontWeight::NORMAL)
            .text_size(px(chrome::CHROME_TEXT_SIZE))
            .line_height(px(chrome::CHROME_LINE_HEIGHT))
            .text_color(theme::bone())
            .on_key_down(cx.listener(Self::on_key))
            .child(filter_field(&self.filter_input, 12.0, 8.0))
            .when_some(self.edit.as_ref(), |panel, edit| {
                panel.child(edit.render(self.operation.is_some(), self.operation_error.as_deref(), cx))
            })
            .child(
                div()
                    .id("project-root")
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.root_collapsed = !view.root_collapsed;
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
                    .h(px(34.0))
                    .flex_shrink_0()
                    .px(px(chrome::SIDEBAR_INSET))
                    .flex()
                    .items_center()
                    .gap(px(chrome::SMALL_GAP))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::bone())
                    .child(panel_icon(if self.root_collapsed { "icons/chevron-right.svg" } else { "icons/chevron-down.svg" }))
                    .child(
                        div().flex_1().min_w_0().truncate().child(
                            self.root
                                .file_name()
                                .unwrap_or(self.root.as_os_str())
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    )
                    .when(loading, |root| root.child(row_metadata("Refreshing…", false)))
                    .child(self.toolbar(cx)),
            )
            .child(
                uniform_list(
                    "file-tree",
                    if self.root_collapsed { 0 } else { self.rows.len() },
                    cx.processor(|view, range: std::ops::Range<usize>, window, cx| {
                        let focused = view.focus.is_focused(window);
                        range
                            .map(|index| view.row(index, focused, cx).into_any_element())
                            .collect::<Vec<_>>()
                    }),
                )
                .pt(px(4.0))
                .track_scroll(self.scroll.clone())
                .flex_1()
                .min_h_0(),
            )
            .when(self.rows.is_empty(), |panel| {
                panel.child(message(if loading || self.filter_loading { "Loading files…".to_owned() }
                    else if !self.query.is_empty() { format!("No files match “{}”. Try another name or clear the filter.", self.query) }
                    else { "This folder is empty.".to_owned() }))
            })
            .when(self.filter_loading, |panel| panel.child(message("Filtering files…")))
            .when(self.filter_truncated && !self.query.is_empty(), |panel| panel.child(message("Search limit reached or a folder was unavailable. Refine the filter to narrow the results.")))
            .when(error.is_some(), |panel| panel.child(file_actions::control("Retry folder", cx.listener(|view, _, _, cx| view.load(view.root.clone(), cx)))))
            .when(!self.query.is_empty() && self.rows.is_empty(), |panel| panel.child(file_actions::control("Clear filter", cx.listener(|view, _, window, cx| {
                view.filter_input.update(cx, |input, cx| input.set_value("", window, cx));
            }))))
            .when_some(self.operation_error.clone().filter(|_| self.edit.is_none()).or(error), |panel, error| {
                panel.child(div().px(px(12.0)).py(px(8.0)).text_color(theme::error()).text_size(px(11.0)).child(error))
                    .child(file_actions::control("Refresh files", cx.listener(|view, _, _, cx| view.refresh(cx))))
            })
            .child(div().h(px(24.0)).flex_shrink_0().px(px(12.0)).flex().items_center().font_weight(FontWeight::NORMAL).text_size(px(11.0)).text_color(theme::ash()).child("↑↓ Move · Enter Open · F2 Rename"))
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
    filter_input: Entity<InputState>,
    query: String,
    browse_state: Option<(
        HashSet<(DiffKind, PathBuf)>,
        Option<(DiffKind, PathBuf)>,
        gpui::ListState,
    )>,
    _filter_subscription: Subscription,
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
    scroll: gpui::ListState,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter changed files…"));
        let subscription = cx.subscribe(&filter_input, |view, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                view.set_filter(input.read(cx).value().to_string(), cx);
            }
        });
        Self {
            root,
            filter_input,
            query: String::new(),
            browse_state: None,
            _filter_subscription: subscription,
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
            scroll: gpui::ListState::new(0, gpui::ListAlignment::Top, px(200.0)),
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
        let generation = self.diff_generation;
        let path = entry.path.clone();
        self.diff_loading = Some(path.clone());
        let visible = self.review_entries(kind);
        let position = visible
            .iter()
            .position(|candidate| candidate.path == path)
            .unwrap_or(0);
        let file = ReviewFile {
            path,
            kind,
            position,
            total: visible.len(),
            marker: entry.marker(kind),
        };
        cx.emit(ProjectPanelEvent::OpenDiff {
            file: file.clone(),
            content: DiffContent::Loading,
        });
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            let diff = if entry.untracked {
                project_git::untracked_diff(&root, &entry.path)?
            } else {
                project_git::file_diff(&root, &entry.path, kind)?
            };
            let label = if kind == DiffKind::Staged {
                "Staged"
            } else {
                "Working tree"
            };
            let mut text = format!("{label}\n\n{}\n", diff.text);
            if diff.truncated {
                text.push_str("\nDiff truncated: open the file to inspect the full change.\n");
            }
            Ok::<_, project_git::GitError>(text)
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                if view.diff_generation != generation {
                    return;
                }
                view.diff_loading = None;
                let content = match result {
                    Ok(text) => DiffContent::Ready(text),
                    Err(error) => DiffContent::Error(error.to_string()),
                };
                cx.emit(ProjectPanelEvent::OpenDiff { file, content });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn review_entries(&self, kind: DiffKind) -> Vec<GitEntry> {
        self.status
            .as_ref()
            .map(|status| {
                project_git::filtered_change_rows(status, &HashSet::new(), &self.query)
                    .iter()
                    .filter(|row| row.kind == kind)
                    .filter_map(|row| row.entry.map(|index| status.entries[index].clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn navigate_review(
        &mut self,
        path: &std::path::Path,
        kind: DiffKind,
        direction: i32,
        cx: &mut Context<Self>,
    ) {
        let entries = self.review_entries(kind);
        let Some(position) = entries.iter().position(|entry| entry.path == path) else {
            return;
        };
        let next = position.saturating_add_signed(direction as isize);
        let Some(entry) = entries.get(next).cloned() else {
            return;
        };
        let mut parent = entry.relative_path.parent();
        self.collapsed.remove(&(kind, PathBuf::new()));
        while let Some(path) = parent {
            self.collapsed.remove(&(kind, path.to_path_buf()));
            parent = path.parent();
        }
        self.rebuild_changes();
        self.selected = self
            .rows
            .iter()
            .position(|row| row.kind == kind && row.path == entry.relative_path)
            .unwrap_or(0);
        self.selection_visible = true;
        self.scroll.scroll_to_reveal_item(self.selected);
        self.open_entry(entry, kind, cx);
    }

    fn set_filter(&mut self, query: String, cx: &mut Context<Self>) {
        let query = query.trim().to_owned();
        if self.query == query {
            return;
        }
        if self.query.is_empty() && !query.is_empty() {
            self.browse_state = Some((
                self.collapsed.clone(),
                self.rows
                    .get(self.selected)
                    .map(|row| (row.kind, row.path.clone())),
                self.scroll.clone(),
            ));
            self.scroll = gpui::ListState::new(0, gpui::ListAlignment::Top, px(200.0));
            self.collapsed.clear();
        }
        self.query = query;
        if self.query.is_empty() {
            if let Some((collapsed, selected, scroll)) = self.browse_state.take() {
                self.collapsed = collapsed;
                let offset = scroll.logical_scroll_top();
                self.scroll = scroll;
                self.rebuild_changes();
                self.scroll.scroll_to(offset);
                self.selected = selected
                    .and_then(|key| {
                        self.rows
                            .iter()
                            .position(|row| (row.kind, row.path.clone()) == key)
                    })
                    .unwrap_or(0);
            }
        } else {
            self.collapsed.clear();
            self.rebuild_changes();
        }
        cx.notify();
    }

    fn collapse_all(&mut self, cx: &mut Context<Self>) {
        if let Some(status) = &self.status {
            for row in project_git::change_rows(status, &HashSet::new()) {
                if row.entry.is_none() && !row.path.as_os_str().is_empty() {
                    self.collapsed.insert((row.kind, row.path));
                }
            }
        }
        self.rebuild_changes();
        cx.notify();
    }

    fn rebuild_changes(&mut self) {
        let scroll_top = self.scroll.logical_scroll_top();
        let top_key = self
            .rows
            .get(scroll_top.item_ix)
            .map(|row| (row.kind, row.path.clone()));
        let selected = self
            .rows
            .get(self.selected)
            .map(|row| (row.kind, row.path.clone()));
        self.rows = self
            .status
            .as_ref()
            .map(|status| project_git::filtered_change_rows(status, &self.collapsed, &self.query))
            .unwrap_or_default();
        self.scroll
            .splice(0..self.scroll.item_count(), self.rows.len());
        let top = top_key
            .and_then(|key| {
                self.rows
                    .iter()
                    .position(|row| (row.kind, row.path.clone()) == key)
            })
            .unwrap_or(scroll_top.item_ix.min(self.rows.len().saturating_sub(1)));
        self.scroll.scroll_to(gpui::ListOffset {
            item_ix: top,
            offset_in_item: scroll_top.offset_in_item,
        });
        self.selected = selected
            .and_then(|(kind, mut path)| {
                loop {
                    if let Some(index) = self
                        .rows
                        .iter()
                        .position(|row| row.kind == kind && row.path == path)
                    {
                        return Some(index);
                    }
                    if !path.pop() {
                        return self
                            .rows
                            .iter()
                            .position(|row| row.kind == kind && row.path.as_os_str().is_empty());
                    }
                }
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
        self.scroll.scroll_to_reveal_item(self.selected);
        cx.stop_propagation();
        cx.notify();
    }

    fn row(&self, index: usize, focused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[index];
        let entry = row
            .entry
            .and_then(|index| self.status.as_ref()?.entries.get(index));
        let expanded = !self.collapsed.contains(&(row.kind, row.path.clone()));
        let section = row.path.as_os_str().is_empty();
        let selected = (self.selection_visible || focused) && self.selected == index;
        div()
            .h(px(if section { 32.0 } else { GIT_ROW_HEIGHT }))
            .px(px(8.0))
            .relative()
            .children((1..row.depth).map(|depth| {
                div()
                    .absolute()
                    .left(px(8.0 + (depth - 1) as f32 * chrome::TREE_INDENT))
                    .top_0()
                    .bottom_0()
                    .w(px(1.0))
                    .bg(theme::edge())
            }))
            .child(
                div()
                    .id(("git-row", index))
                    .debug_selector(move || format!("git-row-{index}"))
                    .size_full()
                    .min_w_0()
                    .pl(px(
                        4.0 + row.depth.saturating_sub(1) as f32 * chrome::TREE_INDENT
                    ))
                    .pr(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(px(4.0))
                    .text_size(px(if section { 12.0 } else { 13.0 }))
                    .font_weight(if section {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(theme::bone())
                    .border_1()
                    .border_color(if selected && focused {
                        theme::focus()
                    } else {
                        rgba(0)
                    })
                    .bg(if selected && focused {
                        theme::selection()
                    } else if selected {
                        theme::panel_hover()
                    } else {
                        rgba(0)
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
                            .w(px(14.0))
                            .flex_shrink_0()
                            .when(entry.is_none(), |item| {
                                item.child(panel_icon(if expanded {
                                    "icons/chevron-down.svg"
                                } else {
                                    "icons/chevron-right.svg"
                                }))
                            }),
                    )
                    .when(!section, |item| {
                        item.child(project_icon(&row.path, entry.is_none(), expanded))
                    })
                    .child(if section {
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(if row.kind == DiffKind::Staged {
                                "Staged"
                            } else {
                                "Unstaged"
                            })
                            .into_any_element()
                    } else {
                        filename(&row.path).into_any_element()
                    })
                    .when(entry.is_none(), |item| {
                        item.child(
                            div()
                                .flex_shrink_0()
                                .font_family(theme::mono())
                                .text_size(px(11.0))
                                .text_color(theme::ash())
                                .child(row.count.to_string()),
                        )
                    })
                    .when_some(entry, |item, entry| {
                        item.child(
                            div()
                                .w(px(16.0))
                                .flex_shrink_0()
                                .font_family(theme::mono())
                                .text_size(px(11.0))
                                .text_color(theme::ash())
                                .child(entry.marker(row.kind)),
                        )
                        .child(
                            div()
                                .w(px(68.0))
                                .flex_shrink_0()
                                .flex()
                                .justify_end()
                                .gap(px(4.0))
                                .font_family(theme::mono())
                                .text_size(px(10.0))
                                .when_some(entry.stats(row.kind), |stats, lines| {
                                    stats
                                        .child(
                                            div()
                                                .text_color(theme::success())
                                                .child(format!("+{}", lines.additions)),
                                        )
                                        .child(
                                            div()
                                                .text_color(theme::error())
                                                .child(format!("−{}", lines.deletions)),
                                        )
                                })
                                .when(entry.stats(row.kind).is_none(), |stats| {
                                    stats.child(div().text_color(theme::ash()).child("—"))
                                }),
                        )
                    }),
            )
    }
}

impl EventEmitter<ProjectPanelEvent> for GitPanel {}

impl Render for GitPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.change_count().unwrap_or(0);
        let branch = self.branch().map(str::to_owned);
        div()
            .id("git-panel")
            .track_focus(&self.focus)
            .tab_index(0)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .font_family(chrome::CHROME_FONT)
            .font_weight(FontWeight::NORMAL)
            .text_size(px(chrome::CHROME_TEXT_SIZE))
            .line_height(px(chrome::CHROME_LINE_HEIGHT))
            .text_color(theme::bone())
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .h(px(48.0))
                    .px(px(16.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Working changes"),
                    )
                    .when(self.loading, |header| {
                        header.child(row_metadata("Refreshing…", false))
                    })
                    .child(
                        icon_control(
                            "collapse-git",
                            "Collapse all folders",
                            "icons/collapse-all.svg",
                        )
                        .on_click(cx.listener(|view, _, _, cx| view.collapse_all(cx))),
                    )
                    .child(
                        icon_control("refresh-git", "Refresh changes · F5", "icons/refresh.svg")
                            .on_click(cx.listener(|view, _, _, cx| view.refresh(cx))),
                    ),
            )
            .when_some(branch, |panel, branch| {
                panel.child(
                    div()
                        .id("choose-branch")
                        .mx(px(12.0))
                        .mb(px(12.0))
                        .h(px(32.0))
                        .flex_shrink_0()
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .bg(theme::canvas())
                        .border_1()
                        .border_color(theme::edge())
                        .rounded(px(5.0))
                        .tab_index(0)
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::panel_hover()))
                        .focus(|style| style.border_color(theme::focus()))
                        .tooltip(text_tooltip(format!("Choose branch · B\n{branch}")))
                        .on_click(cx.listener(|view, _, _, cx| {
                            if !view.switching {
                                view.branch_picker = !view.branch_picker;
                                cx.notify();
                            }
                        }))
                        .child(panel_icon("icons/branch.svg"))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(theme::mono())
                                .text_size(px(12.0))
                                .child(branch),
                        )
                        .child(panel_icon("icons/chevron-down.svg")),
                )
            })
            .when(self.switching, |panel| {
                panel.child(message("Switching branch…"))
            })
            .when(self.branch_picker, |panel| {
                panel.child(
                    div()
                        .id("branch-list")
                        .max_h(px(240.0))
                        .overflow_y_scroll()
                        .px(px(12.0))
                        .children(self.branches.iter().enumerate().map(|(index, branch)| {
                            let branch = branch.clone();
                            div()
                                .id(("branch-option", index))
                                .tab_index(0)
                                .h(px(32.0))
                                .px(px(8.0))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .hover(|style| style.bg(theme::panel_hover()))
                                .focus(|style| {
                                    style.bg(theme::selection()).border_color(theme::focus())
                                })
                                .child(div().truncate().child(branch.clone()))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.choose_branch(branch.clone(), cx)
                                }))
                        }))
                        .when(self.branches.is_empty(), |list| {
                            list.child(message("No local branches yet."))
                        }),
                )
            })
            .child(filter_field(&self.filter_input, 0.0, 8.0))
            .child(
                gpui::list(
                    self.scroll.clone(),
                    cx.processor(|view, index, window, cx| {
                        let focused = view.focus.is_focused(window);
                        view.row(index, focused, cx).into_any_element()
                    }),
                )
                .flex_1()
                .min_h_0(),
            )
            .when(count == 0 && self.error.is_none(), |panel| {
                panel.child(message(if self.loading {
                    "Reading Git status…"
                } else {
                    "No working changes"
                }))
            })
            .when(count > 0 && self.rows.is_empty(), |panel| {
                panel.child(message(format!(
                    "No changes match “{}”. Clear the filter to see all changes.",
                    self.query
                )))
            })
            .when(!self.query.is_empty() && self.rows.is_empty(), |panel| {
                panel.child(file_actions::control(
                    "Clear filter",
                    cx.listener(|view, _, window, cx| {
                        view.filter_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                    }),
                ))
            })
            .when_some(self.error.clone(), |panel, error| {
                panel.child(
                    div()
                        .p(px(12.0))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_color(theme::error())
                                .child(if self.status.is_some() {
                                    "Could not refresh changes. Showing the last successful list."
                                        .to_owned()
                                } else {
                                    error
                                }),
                        )
                        .child(file_actions::control(
                            "Retry",
                            cx.listener(|view, _, _, cx| view.refresh(cx)),
                        )),
                )
            })
            .when(
                self.status.as_ref().is_some_and(|status| status.truncated),
                |panel| panel.child(message("Large change list: some entries are hidden.")),
            )
            .child(
                div()
                    .h(px(28.0))
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(theme::edge())
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .text_size(px(11.0))
                    .text_color(theme::ash())
                    .child("M Modified   U Untracked"),
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
    svg()
        .path(crate::assets::project_icon(path, directory, expanded))
        .size(px(16.0))
        .flex_shrink_0()
        .text_color(theme::ash())
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

pub(super) fn filter_field(input: &Entity<InputState>, top: f32, bottom: f32) -> impl IntoElement {
    div()
        .mx(px(12.0))
        .mt(px(top))
        .mb(px(bottom))
        .h(px(32.0))
        .flex_shrink_0()
        .child(
            Input::new(input)
                .h(px(32.0))
                .font_family(chrome::CHROME_FONT)
                .font_weight(FontWeight::NORMAL)
                .text_size(px(12.0))
                .cleanable(true)
                .prefix(
                    svg()
                        .path("icons/search.svg")
                        .size(px(14.0))
                        .text_color(theme::ash()),
                ),
        )
}

fn filename(path: &std::path::Path) -> impl IntoElement {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    let (stem, extension) = name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map_or((name.as_ref(), String::new()), |(stem, ext)| {
            (stem, format!(".{ext}"))
        });
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .child(div().min_w_0().truncate().child(stem.to_owned()))
        .when(!extension.is_empty(), |label| {
            label.child(div().flex_shrink_0().child(extension))
        })
}

pub(super) fn icon_control(
    id: &'static str,
    label: &'static str,
    icon: &'static str,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(28.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .border_1()
        .border_color(rgba(0))
        .tab_index(0)
        .cursor_pointer()
        .hover(|style| style.bg(theme::panel_hover()))
        .focus(|style| style.border_color(theme::focus()))
        .tooltip(text_tooltip(label))
        .child(panel_icon(icon))
}

#[cfg(test)]
mod reimagined_tests {
    use super::*;

    fn entry(path: &str, index_status: char, worktree_status: char) -> GitEntry {
        GitEntry {
            path: PathBuf::from("project").join(path),
            relative_path: path.into(),
            original_path: None,
            index_status,
            worktree_status,
            untracked: false,
            conflicted: false,
            line_stats: None,
            staged_stats: None,
            working_stats: None,
        }
    }

    #[gpui::test]
    fn git_filter_and_collapse_preserve_section_identity(cx: &mut gpui::TestAppContext) {
        cx.update(super::super::file_editor::FileEditor::initialize);
        let window = cx.add_window(|window, cx| {
            let terminal = cx.new(|cx| TerminalView::new("project".into(), cx));
            let mut panel = GitPanel::new("project".into(), terminal.downgrade(), window, cx);
            panel.status = Some(GitStatus {
                branch: "main".into(),
                entries: vec![
                    entry("src/app/a.rs", 'M', 'M'),
                    entry("src/app/b.rs", ' ', 'M'),
                ],
                truncated: false,
            });
            panel.rebuild_changes();
            panel
        });
        window
            .update(cx, |panel, _, cx| {
                panel.selected = panel
                    .rows
                    .iter()
                    .position(|row| {
                        row.kind == DiffKind::WorkingTree
                            && row.path == std::path::Path::new("src/app/a.rs")
                    })
                    .unwrap();
                panel
                    .collapsed
                    .insert((DiffKind::WorkingTree, "src".into()));
                panel.rebuild_changes();
                assert_eq!(panel.rows[panel.selected].path, PathBuf::from("src"));
                assert_eq!(panel.rows[panel.selected].kind, DiffKind::WorkingTree);
                let saved = panel.collapsed.clone();
                panel.set_filter("a.rs".into(), cx);
                assert_eq!(
                    panel.rows.iter().filter(|row| row.entry.is_some()).count(),
                    2
                );
                assert!(
                    panel
                        .rows
                        .iter()
                        .any(|row| row.path == std::path::Path::new("src/app"))
                );
                panel.set_filter("".into(), cx);
                assert_eq!(panel.collapsed, saved);
                assert_eq!(panel.rows[panel.selected].path, PathBuf::from("src"));
            })
            .unwrap();
    }
}
