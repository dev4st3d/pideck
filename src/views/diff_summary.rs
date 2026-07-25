use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use gpui::{
    AnyElement, Context, Entity, FocusHandle, FontWeight, IntoElement, ScrollHandle, SharedString,
    div, prelude::*, px, relative, svg,
};

use super::{RootView, controls};
use crate::services::git_diff::{
    DiffFile, DiffFileKind, DiffHunk, DiffLine, DiffLineKind, WorkspaceDiff,
};
use crate::theme;

const MAX_EXPANDED_FILES: usize = 40;
const MAX_RENDERED_DIFF_LINES: usize = 2_400;
const DIFF_SIDEBAR_WIDTH: f32 = 292.0;
const LINE_NUMBER_WIDTH: f32 = 32.0;
const LINE_GUTTER_WIDTH: f32 = 76.0;

pub(in crate::views) fn summary_card(
    snapshot: &Arc<WorkspaceDiff>,
    expanded: bool,
    root: Entity<RootView>,
) -> impl IntoElement {
    let file_count = snapshot.files.len();
    let additions = snapshot.additions();
    let deletions = snapshot.deletions();
    let toggle_root = root.clone();
    let toggle_key_root = root.clone();
    let open_root = root.clone();
    let open_key_root = root;

    div()
        .id("workspace-diff-summary")
        .w_full()
        .overflow_hidden()
        .rounded(px(theme::RADIUS))
        .border_1()
        .border_color(theme::edge_hard())
        .bg(theme::floor())
        .child(
            div()
                .min_h(px(46.0))
                .px(px(12.0))
                .py(px(8.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(16.0))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .id("workspace-diff-files-toggle")
                                .tab_index(0)
                                .cursor_pointer()
                                .min_w_0()
                                .min_h(px(30.0))
                                .px(px(4.0))
                                .rounded(px(theme::RADIUS_SM))
                                .border_1()
                                .border_color(theme::floor())
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(9.0))
                                .text_color(theme::ash())
                                .hover(|button| button.bg(theme::panel()).text_color(theme::bone()))
                                .active(|button| button.bg(theme::panel_lift()))
                                .focus(|button| button.border_1().border_color(theme::focus()))
                                .on_click(move |_, _, cx| {
                                    toggle_root.update(cx, |view, cx| {
                                        view.toggle_workspace_diff_files(cx)
                                    });
                                })
                                .on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        cx.stop_propagation();
                                        toggle_key_root.update(cx, |view, cx| {
                                            view.toggle_workspace_diff_files(cx)
                                        });
                                    }
                                })
                                .child(
                                    svg()
                                        .path(if expanded {
                                            "icons/chevron-up.svg"
                                        } else {
                                            "icons/chevron-down.svg"
                                        })
                                        .size(px(12.0))
                                        .flex_shrink_0()
                                        .text_color(theme::smoke()),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .font_family(theme::sans())
                                        .text_size(px(theme::T_UI))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::bone_dim())
                                        .child(format!(
                                            "{file_count} changed file{}",
                                            if file_count == 1 { "" } else { "s" }
                                        )),
                                )
                                .child(stat_text(format!("+{additions}"), theme::live()))
                                .child(stat_text(format!("-{deletions}"), theme::error()))
                                .when(snapshot.counts_partial, |row| {
                                    row.child(
                                        div()
                                            .flex_shrink_0()
                                            .font_family(theme::sans())
                                            .text_size(px(theme::T_TINY))
                                            .text_color(theme::smoke())
                                            .child("partial counts"),
                                    )
                                })
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .font_family(theme::sans())
                                        .text_size(px(theme::T_UI_SM))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme::ash())
                                        .child(if expanded { "Hide files" } else { "Show files" }),
                                ),
                        ),
                )
                .child(
                    div()
                        .id("workspace-diff-open")
                        .tab_index(0)
                        .cursor_pointer()
                        .min_h(px(30.0))
                        .px(px(9.0))
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(theme::edge_hard())
                        .bg(theme::panel())
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .gap(px(6.0))
                        .text_color(theme::bone_dim())
                        .hover(|button| {
                            button
                                .bg(theme::panel_lift())
                                .border_color(theme::focus())
                                .text_color(theme::bone())
                        })
                        .active(|button| button.bg(theme::panel_hover()))
                        .focus(|button| button.border_color(theme::focus()))
                        .on_click(move |_, window, cx| {
                            open_root.update(cx, |view, cx| view.open_workspace_diff(window, cx));
                        })
                        .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                cx.stop_propagation();
                                open_key_root
                                    .update(cx, |view, cx| view.open_workspace_diff(window, cx));
                            }
                        })
                        .child(
                            svg()
                                .path("icons/diff.svg")
                                .size(px(14.0))
                                .text_color(theme::smoke()),
                        )
                        .child(
                            div()
                                .font_family(theme::sans())
                                .text_size(px(theme::T_UI_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Open diff"),
                        ),
                ),
        )
        .when(expanded, |card| {
            card.child(
                div()
                    .w_full()
                    .border_t_1()
                    .border_color(theme::edge_soft())
                    .bg(theme::canvas())
                    .flex()
                    .flex_col()
                    .children(
                        snapshot
                            .files
                            .iter()
                            .take(MAX_EXPANDED_FILES)
                            .enumerate()
                            .map(|(index, file)| file_row(index, file)),
                    )
                    .when(snapshot.files.len() > MAX_EXPANDED_FILES, |list| {
                        list.child(
                            div()
                                .px(px(15.0))
                                .py(px(8.0))
                                .border_t_1()
                                .border_color(theme::edge_soft())
                                .font_family(theme::sans())
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(format!(
                                    "{} more files are available in the full diff.",
                                    snapshot.files.len() - MAX_EXPANDED_FILES
                                )),
                        )
                    }),
            )
        })
}

fn stat_text(text: String, color: gpui::Rgba) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .font_family(theme::mono())
        .text_size(px(theme::T_MONO_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(text)
}

fn file_row(index: usize, file: &DiffFile) -> AnyElement {
    div()
        .id(("workspace-diff-file", index))
        .min_h(px(32.0))
        .px(px(15.0))
        .py(px(6.0))
        .border_t_1()
        .border_color(if index == 0 {
            theme::canvas()
        } else {
            theme::edge_soft()
        })
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(14.0))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .font_family(theme::mono())
                .text_size(px(theme::T_MONO_SM))
                .text_color(theme::bone_dim())
                .child(file.path.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .when(file.untracked, |stats| {
                    stats.child(
                        div()
                            .font_family(theme::sans())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::data())
                            .child("new"),
                    )
                })
                .when(file.binary, |stats| {
                    stats.child(
                        div()
                            .font_family(theme::sans())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::smoke())
                            .child("binary"),
                    )
                })
                .when(file.additions > 0, |stats| {
                    stats.child(stat_text(format!("+{}", file.additions), theme::live()))
                })
                .when(file.deletions > 0, |stats| {
                    stats.child(stat_text(format!("-{}", file.deletions), theme::error()))
                }),
        )
        .into_any_element()
}

pub(in crate::views) fn diff_overlay(
    snapshot: &Arc<WorkspaceDiff>,
    selected_index: usize,
    collapsed_folders: &HashSet<String>,
    focus: &FocusHandle,
    files_scroll: &ScrollHandle,
    content_scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let selected_index = selected_index.min(snapshot.files.len().saturating_sub(1));
    let selected = snapshot.files.get(selected_index);
    let root = cx.entity();

    div()
        .id("workspace-diff-overlay")
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .occlude()
        .bg(gpui::rgba(0x0b0a_09f5))
        .p(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .id("workspace-diff-dialog")
                .track_focus(focus)
                .key_context("WorkspaceDiff")
                .tab_index(0)
                .on_key_down(cx.listener(RootView::on_workspace_diff_key_down))
                .w_full()
                .h_full()
                .max_w(px(1440.0))
                .rounded(px(theme::RADIUS))
                .overflow_hidden()
                .border_1()
                .border_color(theme::edge_hard())
                .focus(|dialog| dialog.border_color(theme::focus()))
                .bg(theme::floor())
                .flex()
                .flex_col()
                .child(diff_overlay_header(snapshot, cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .child(diff_file_sidebar(
                            snapshot,
                            selected_index,
                            collapsed_folders,
                            files_scroll,
                            root,
                        ))
                        .when_some(selected, |body, file| {
                            body.child(diff_file_panel(
                                snapshot,
                                file,
                                selected_index,
                                content_scroll,
                                cx,
                            ))
                        }),
                ),
        )
}

fn diff_overlay_header(snapshot: &WorkspaceDiff, cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .min_h(px(54.0))
        .px(px(16.0))
        .py(px(10.0))
        .border_b_1()
        .border_color(theme::edge_hard())
        .bg(theme::panel())
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_row()
                .items_baseline()
                .gap(px(10.0))
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(px(theme::T_BODY))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::bone())
                        .child("Workspace changes"),
                )
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(px(theme::T_UI_SM))
                        .text_color(theme::ash())
                        .child(format!(
                            "{} file{}",
                            snapshot.files.len(),
                            if snapshot.files.len() == 1 { "" } else { "s" }
                        )),
                )
                .child(stat_text(
                    format!("+{}", snapshot.additions()),
                    theme::live(),
                ))
                .child(stat_text(
                    format!("-{}", snapshot.deletions()),
                    theme::error(),
                ))
                .when(snapshot.counts_partial, |header| {
                    header.child(
                        div()
                            .font_family(theme::sans())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::smoke())
                            .child("counts partial"),
                    )
                }),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(14.0))
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child("↑↓ files · ←→ folders · Esc close"),
                )
                .child(controls::chrome_action(
                    "close-workspace-diff",
                    "Close",
                    true,
                    Box::new(
                        cx.listener(|view, _, window, cx| view.close_workspace_diff(window, cx)),
                    ),
                )),
        )
}

#[derive(Default)]
struct DiffTreeNode {
    folders: BTreeMap<String, DiffTreeNode>,
    files: Vec<(String, usize)>,
}

enum DiffTreeRow {
    Folder {
        name: String,
        path: String,
        depth: usize,
        file_count: usize,
        collapsed: bool,
    },
    File {
        name: String,
        index: usize,
        depth: usize,
    },
}

fn diff_file_sidebar(
    snapshot: &Arc<WorkspaceDiff>,
    selected_index: usize,
    collapsed_folders: &HashSet<String>,
    scroll: &ScrollHandle,
    root: Entity<RootView>,
) -> impl IntoElement {
    let rows = diff_tree_rows(snapshot, collapsed_folders);

    div()
        .w(px(DIFF_SIDEBAR_WIDTH))
        .h_full()
        .min_h_0()
        .flex_shrink_0()
        .border_r_1()
        .border_color(theme::edge_hard())
        .bg(theme::floor())
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(42.0))
                .px(px(12.0))
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::bone_dim())
                        .child("Changed files"),
                )
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(snapshot.files.len().to_string()),
                ),
        )
        .child(
            div()
                .id("workspace-diff-file-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .children(rows.into_iter().map(|row| match row {
                    DiffTreeRow::Folder {
                        name,
                        path,
                        depth,
                        file_count,
                        collapsed,
                    } => diff_folder_row(name, path, depth, file_count, collapsed, root.clone()),
                    DiffTreeRow::File { name, index, depth } => diff_tree_file_row(
                        name,
                        index,
                        depth,
                        &snapshot.files[index],
                        index == selected_index,
                        root.clone(),
                    ),
                })),
        )
}

fn diff_tree_rows(
    snapshot: &WorkspaceDiff,
    collapsed_folders: &HashSet<String>,
) -> Vec<DiffTreeRow> {
    let mut tree = DiffTreeNode::default();
    for (index, file) in snapshot.files.iter().enumerate() {
        let path = tree_path(&file.path);
        let parts = path
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let Some((file_name, folders)) = parts.split_last() else {
            continue;
        };
        let mut node = &mut tree;
        for folder in folders {
            node = node.folders.entry((*folder).to_owned()).or_default();
        }
        node.files.push(((*file_name).to_owned(), index));
    }

    let mut rows = Vec::new();
    flatten_diff_tree(&tree, "", 0, collapsed_folders, &mut rows);
    rows
}

fn flatten_diff_tree(
    node: &DiffTreeNode,
    parent_path: &str,
    depth: usize,
    collapsed_folders: &HashSet<String>,
    rows: &mut Vec<DiffTreeRow>,
) {
    for (name, child) in &node.folders {
        let path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{parent_path}/{name}")
        };
        let collapsed = collapsed_folders.contains(&path);
        rows.push(DiffTreeRow::Folder {
            name: name.clone(),
            path: path.clone(),
            depth,
            file_count: descendant_file_count(child),
            collapsed,
        });
        if !collapsed {
            flatten_diff_tree(child, &path, depth + 1, collapsed_folders, rows);
        }
    }

    let mut files = node.files.clone();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    rows.extend(
        files
            .into_iter()
            .map(|(name, index)| DiffTreeRow::File { name, index, depth }),
    );
}

fn descendant_file_count(node: &DiffTreeNode) -> usize {
    node.files.len()
        + node
            .folders
            .values()
            .map(descendant_file_count)
            .sum::<usize>()
}

pub(in crate::views) fn file_tree_row_index(
    snapshot: &WorkspaceDiff,
    collapsed_folders: &HashSet<String>,
    file_index: usize,
) -> Option<usize> {
    diff_tree_rows(snapshot, collapsed_folders)
        .iter()
        .position(|row| matches!(row, DiffTreeRow::File { index, .. } if *index == file_index))
}

pub(in crate::views) fn adjacent_file_tree_index(
    snapshot: &WorkspaceDiff,
    collapsed_folders: &HashSet<String>,
    current: usize,
    delta: isize,
) -> Option<usize> {
    let files = diff_tree_rows(snapshot, collapsed_folders)
        .into_iter()
        .filter_map(|row| match row {
            DiffTreeRow::File { index, .. } => Some(index),
            DiffTreeRow::Folder { .. } => None,
        })
        .collect::<Vec<_>>();
    let position = files.iter().position(|index| *index == current);
    let next = match (position, delta.cmp(&0)) {
        (_, std::cmp::Ordering::Equal) => position?,
        (Some(position), std::cmp::Ordering::Less) => position.saturating_sub(1),
        (Some(position), std::cmp::Ordering::Greater) => (position + 1).min(files.len() - 1),
        (None, std::cmp::Ordering::Less) => files.len().checked_sub(1)?,
        (None, std::cmp::Ordering::Greater) => 0,
    };
    files.get(next).copied()
}

pub(in crate::views) fn edge_file_tree_index(
    snapshot: &WorkspaceDiff,
    collapsed_folders: &HashSet<String>,
    last: bool,
) -> Option<usize> {
    let files = diff_tree_rows(snapshot, collapsed_folders)
        .into_iter()
        .filter_map(|row| match row {
            DiffTreeRow::File { index, .. } => Some(index),
            DiffTreeRow::Folder { .. } => None,
        })
        .collect::<Vec<_>>();
    if last {
        files.last().copied()
    } else {
        files.first().copied()
    }
}

fn tree_path(path: &str) -> &str {
    path.rsplit(" → ").next().unwrap_or(path)
}

fn diff_folder_row(
    name: String,
    path: String,
    depth: usize,
    file_count: usize,
    collapsed: bool,
    root: Entity<RootView>,
) -> AnyElement {
    let click_path = path.clone();
    let key_path = path.clone();
    let click_root = root.clone();

    div()
        .id(SharedString::from(format!("workspace-diff-folder-{path}")))
        .tab_index(0)
        .h(px(34.0))
        .pl(px(8.0 + depth as f32 * 14.0))
        .pr(px(10.0))
        .border_1()
        .border_color(theme::floor())
        .cursor_pointer()
        .hover(|row| row.bg(theme::panel_lift()))
        .focus(|row| row.border_color(theme::focus()))
        .on_click(move |_, _, cx| {
            click_root.update(cx, |view, cx| {
                view.toggle_workspace_diff_folder(&click_path, cx)
            });
        })
        .on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
            let key = event.keystroke.key.as_str();
            let should_toggle = matches!(key, "enter" | "space")
                || (key == "right" && collapsed)
                || (key == "left" && !collapsed);
            if should_toggle {
                cx.stop_propagation();
                root.update(cx, |view, cx| {
                    view.toggle_workspace_diff_folder(&key_path, cx)
                });
            }
        })
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(
            svg()
                .path(if collapsed {
                    "icons/chevron-right.svg"
                } else {
                    "icons/chevron-down.svg"
                })
                .size(px(12.0))
                .flex_shrink_0()
                .text_color(theme::smoke()),
        )
        .child(
            svg()
                .path("icons/folder.svg")
                .size(px(14.0))
                .flex_shrink_0()
                .text_color(theme::ash()),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .font_family(theme::sans())
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::bone_dim())
                .child(name),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_family(theme::mono())
                .text_size(px(theme::T_TINY))
                .text_color(theme::smoke())
                .child(file_count.to_string()),
        )
        .into_any_element()
}

fn diff_tree_file_row(
    file_name: String,
    index: usize,
    depth: usize,
    file: &DiffFile,
    selected: bool,
    root: Entity<RootView>,
) -> AnyElement {
    let (marker, _, marker_color) = file_kind_meta(file.kind);
    let click_root = root.clone();
    let key_root = root;

    div()
        .id(SharedString::from(format!(
            "workspace-diff-select-{}",
            file.path
        )))
        .tab_index(0)
        .min_h(px(38.0))
        .pl(px(14.0 + depth as f32 * 14.0))
        .pr(px(10.0))
        .border_1()
        .border_color(if selected {
            theme::panel_hover()
        } else {
            theme::floor()
        })
        .bg(if selected {
            theme::panel_hover()
        } else {
            theme::floor()
        })
        .cursor_pointer()
        .hover(|row| row.bg(theme::panel_lift()))
        .focus(|row| row.border_color(theme::focus()))
        .on_click(move |_, _, cx| {
            click_root.update(cx, |view, cx| view.select_workspace_diff_file(index, cx));
        })
        .on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                key_root.update(cx, |view, cx| view.select_workspace_diff_file(index, cx));
            }
        })
        .flex()
        .flex_row()
        .items_center()
        .gap(px(7.0))
        .child(
            div()
                .w(px(14.0))
                .flex_shrink_0()
                .font_family(theme::mono())
                .text_size(px(theme::T_MONO_SM))
                .font_weight(FontWeight::BOLD)
                .text_color(marker_color)
                .child(marker),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .font_family(theme::sans())
                .text_size(px(theme::T_UI_SM))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if selected {
                    theme::bone()
                } else {
                    theme::bone_dim()
                })
                .child(file_name),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .when(file.additions > 0, |stats| {
                    stats.child(stat_text(format!("+{}", file.additions), theme::live()))
                })
                .when(file.deletions > 0, |stats| {
                    stats.child(stat_text(format!("-{}", file.deletions), theme::error()))
                })
                .when(file.binary, |stats| {
                    stats.child(
                        div()
                            .font_family(theme::sans())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::smoke())
                            .child("bin"),
                    )
                }),
        )
        .into_any_element()
}

fn diff_file_panel(
    snapshot: &Arc<WorkspaceDiff>,
    file: &DiffFile,
    selected_index: usize,
    scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let (_, kind_label, kind_color) = file_kind_meta(file.kind);
    let (rows, omitted) = rendered_file_rows(file);

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .bg(theme::canvas())
        .flex()
        .flex_col()
        .child(
            div()
                .min_h(px(54.0))
                .px(px(14.0))
                .py(px(8.0))
                .flex_shrink_0()
                .border_b_1()
                .border_color(theme::edge_hard())
                .bg(theme::floor())
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(14.0))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .font_family(theme::mono())
                                .text_size(px(theme::T_MONO))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::bone())
                                .child(file.path.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(9.0))
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(px(theme::T_TINY))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(kind_color)
                                        .child(kind_label),
                                )
                                .when(file.binary, |meta| {
                                    meta.child(
                                        div()
                                            .font_family(theme::sans())
                                            .text_size(px(theme::T_TINY))
                                            .text_color(theme::smoke())
                                            .child("Binary"),
                                    )
                                })
                                .when(file.additions > 0, |meta| {
                                    meta.child(stat_text(
                                        format!("+{}", file.additions),
                                        theme::live(),
                                    ))
                                })
                                .when(file.deletions > 0, |meta| {
                                    meta.child(stat_text(
                                        format!("-{}", file.deletions),
                                        theme::error(),
                                    ))
                                }),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(7.0))
                        .child(
                            div()
                                .font_family(theme::mono())
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(format!(
                                    "{} / {}",
                                    selected_index + 1,
                                    snapshot.files.len()
                                )),
                        )
                        .child(file_navigation_button(
                            "workspace-diff-previous",
                            "Previous",
                            selected_index > 0,
                            selected_index.saturating_sub(1),
                            cx.entity(),
                        ))
                        .child(file_navigation_button(
                            "workspace-diff-next",
                            "Next",
                            selected_index + 1 < snapshot.files.len(),
                            selected_index.saturating_add(1),
                            cx.entity(),
                        )),
                ),
        )
        .child(
            div()
                .id("workspace-diff-content-scroll")
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_x_scroll()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .w_full()
                        .min_w(px(720.0))
                        .py(px(8.0))
                        .when(file.hunks.is_empty(), |body| {
                            body.child(diff_empty_file(file, snapshot.patch_truncated))
                        })
                        .children(rows)
                        .when(omitted > 0, |body| {
                            body.child(diff_limit_notice(format!(
                                "Preview stopped after {MAX_RENDERED_DIFF_LINES} changed and context lines. {omitted} more lines are not rendered."
                            )))
                        })
                        .when(snapshot.patch_truncated, |body| {
                            body.child(diff_limit_notice(
                                "The workspace patch reached the safe preview-size limit. This file may be incomplete."
                                    .to_owned(),
                            ))
                        }),
                ),
        )
}

fn file_navigation_button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    index: usize,
    root: Entity<RootView>,
) -> AnyElement {
    let key_root = root.clone();
    div()
        .id(id)
        .tab_index(if enabled { 0 } else { -1 })
        .h(px(28.0))
        .px(px(8.0))
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(if enabled {
            theme::edge_hard()
        } else {
            theme::edge_soft()
        })
        .bg(theme::panel())
        .opacity(if enabled { 1.0 } else { 0.45 })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_lift()).border_color(theme::focus()))
                .focus(|button| button.border_color(theme::focus()))
                .on_click(move |_, _, cx| {
                    root.update(cx, |view, cx| view.select_workspace_diff_file(index, cx));
                })
                .on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        key_root.update(cx, |view, cx| view.select_workspace_diff_file(index, cx));
                    }
                })
        })
        .flex()
        .items_center()
        .justify_center()
        .font_family(theme::sans())
        .text_size(px(theme::T_TINY))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if enabled {
            theme::bone_dim()
        } else {
            theme::smoke()
        })
        .child(label)
        .into_any_element()
}

fn rendered_file_rows(file: &DiffFile) -> (Vec<AnyElement>, usize) {
    let total_lines = file
        .hunks
        .iter()
        .map(|hunk| hunk.lines.len())
        .sum::<usize>();
    let mut rendered = 0usize;
    let mut row_index = 0usize;
    let mut rows = Vec::new();

    for (hunk_index, hunk) in file.hunks.iter().enumerate() {
        if rendered >= MAX_RENDERED_DIFF_LINES {
            break;
        }
        rows.push(diff_hunk_header(hunk_index, hunk));
        for line in &hunk.lines {
            if rendered >= MAX_RENDERED_DIFF_LINES {
                break;
            }
            rows.push(diff_code_line(row_index, line));
            rendered += 1;
            row_index += 1;
        }
    }

    (rows, total_lines.saturating_sub(rendered))
}

fn diff_hunk_header(index: usize, hunk: &DiffHunk) -> AnyElement {
    div()
        .id(("workspace-diff-hunk", index))
        .min_w_full()
        .min_h(px(32.0))
        .mt(px(if index == 0 { 0.0 } else { 10.0 }))
        .pl(px(LINE_GUTTER_WIDTH + 26.0))
        .pr(px(16.0))
        .flex()
        .flex_row()
        .items_center()
        .bg(theme::data_wash())
        .font_family(theme::sans())
        .text_size(px(theme::T_UI_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::data())
        .child(hunk_context(&hunk.header))
        .into_any_element()
}

fn hunk_context(header: &str) -> String {
    header
        .strip_prefix("@@")
        .and_then(|header| header.split_once("@@"))
        .map(|(_, context)| context.trim())
        .filter(|context| !context.is_empty())
        .unwrap_or("Changed lines")
        .to_owned()
}

fn diff_code_line(index: usize, line: &DiffLine) -> AnyElement {
    let (marker, color, background) = match line.kind {
        DiffLineKind::Addition => ("+", theme::live(), theme::live_wash()),
        DiffLineKind::Deletion => ("-", theme::error(), theme::error_wash()),
        DiffLineKind::Context => (" ", theme::bone_dim(), theme::canvas()),
        DiffLineKind::Notice => ("·", theme::smoke(), theme::canvas()),
    };

    div()
        .id(("workspace-diff-code-line", index))
        .min_w_full()
        .min_h(px(21.0))
        .flex()
        .flex_row()
        .bg(background)
        .child(line_number_gutter(line.old_line, line.new_line))
        .child(
            div()
                .w(px(20.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .font_family(theme::mono())
                .text_size(px(theme::T_MONO_SM))
                .font_weight(FontWeight::BOLD)
                .text_color(color)
                .child(marker),
        )
        .child(
            div()
                .flex_1()
                .pl(px(6.0))
                .pr(px(18.0))
                .font_family(theme::mono())
                .text_size(px(theme::T_MONO_SM))
                .line_height(relative(1.62))
                .whitespace_nowrap()
                .text_color(color)
                .child(SharedString::from(if line.text.is_empty() {
                    " ".to_owned()
                } else {
                    line.text.clone()
                })),
        )
        .into_any_element()
}

fn line_number_gutter(old_line: Option<u64>, new_line: Option<u64>) -> impl IntoElement {
    div()
        .w(px(LINE_GUTTER_WIDTH))
        .flex_shrink_0()
        .px(px(4.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .bg(theme::floor())
        .child(line_number(old_line))
        .child(line_number(new_line))
}

fn line_number(value: Option<u64>) -> impl IntoElement {
    div()
        .w(px(LINE_NUMBER_WIDTH))
        .flex_shrink_0()
        .pr(px(3.0))
        .flex()
        .items_center()
        .justify_end()
        .font_family(theme::mono())
        .text_size(px(theme::T_TINY))
        .text_color(theme::smoke())
        .child(value.map(|line| line.to_string()).unwrap_or_default())
}

fn diff_empty_file(file: &DiffFile, patch_truncated: bool) -> impl IntoElement {
    let (title, detail) = if file.binary {
        (
            "Binary content changed",
            "This file cannot be displayed as a line-by-line text diff.",
        )
    } else if patch_truncated {
        (
            "Diff content unavailable",
            "The workspace preview limit was reached before this file's text could be loaded.",
        )
    } else {
        (
            "No textual changes to display",
            "Git reported this file without any line hunks.",
        )
    };

    div()
        .px(px(28.0))
        .py(px(30.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .font_family(theme::sans())
                .text_size(px(theme::T_BODY_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::bone_dim())
                .child(title),
        )
        .child(
            div()
                .font_family(theme::sans())
                .text_size(px(theme::T_UI_SM))
                .text_color(theme::smoke())
                .child(detail),
        )
}

fn diff_limit_notice(message: String) -> impl IntoElement {
    div()
        .px(px(14.0))
        .py(px(10.0))
        .font_family(theme::sans())
        .text_size(px(theme::T_UI_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::data())
        .child(message)
}

fn file_kind_meta(kind: DiffFileKind) -> (&'static str, &'static str, gpui::Rgba) {
    match kind {
        DiffFileKind::Modified => ("M", "Modified", theme::data()),
        DiffFileKind::Added => ("A", "Added", theme::live()),
        DiffFileKind::Deleted => ("D", "Deleted", theme::error()),
        DiffFileKind::Renamed => ("R", "Renamed", theme::working()),
        DiffFileKind::Untracked => ("U", "Untracked", theme::live()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn diff_file(path: &str) -> DiffFile {
        DiffFile {
            path: path.to_owned(),
            additions: 1,
            deletions: 0,
            binary: false,
            untracked: false,
            kind: DiffFileKind::Modified,
            hunks: Vec::new(),
        }
    }

    #[test]
    fn file_tree_groups_folders_and_hides_collapsed_descendants() {
        let snapshot = WorkspaceDiff {
            files: vec![
                diff_file("README.md"),
                diff_file("src/app.rs"),
                diff_file("src/views/root.rs"),
            ],
            patch_truncated: false,
            counts_partial: false,
        };
        let expanded = diff_tree_rows(&snapshot, &HashSet::new());
        assert!(matches!(
            expanded.first(),
            Some(DiffTreeRow::Folder { name, file_count: 2, .. }) if name == "src"
        ));
        assert_eq!(
            expanded
                .iter()
                .filter(|row| matches!(row, DiffTreeRow::File { .. }))
                .count(),
            3
        );
        assert_eq!(
            adjacent_file_tree_index(&snapshot, &HashSet::new(), 2, 1),
            Some(1)
        );
        assert_eq!(
            adjacent_file_tree_index(&snapshot, &HashSet::new(), 1, 1),
            Some(0)
        );
        assert_eq!(
            edge_file_tree_index(&snapshot, &HashSet::new(), false),
            Some(2)
        );
        assert_eq!(
            edge_file_tree_index(&snapshot, &HashSet::new(), true),
            Some(0)
        );

        let collapsed = HashSet::from(["src".to_owned()]);
        let rows = diff_tree_rows(&snapshot, &collapsed);
        assert_eq!(
            rows.iter()
                .filter(|row| matches!(row, DiffTreeRow::File { .. }))
                .count(),
            1
        );
        assert_eq!(file_tree_row_index(&snapshot, &collapsed, 1), None);
    }

    #[test]
    fn hunk_header_keeps_context_and_hides_patch_coordinates() {
        assert_eq!(hunk_context("@@ -10,3 +10,4 @@ fn render()"), "fn render()");
        assert_eq!(hunk_context("@@ -1 +1 @@"), "Changed lines");
    }
}
