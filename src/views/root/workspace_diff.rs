//! Workspace-diff drawer state and keyboard navigation.

use super::*;

impl RootView {
    pub(in crate::views) fn toggle_workspace_diff_files(&mut self, cx: &mut Context<Self>) {
        if self.workspace_diff.is_none() {
            return;
        }
        self.workspace_diff_files_expanded = !self.workspace_diff_files_expanded;
        self.conversation_list
            .refresh_trailing(&self.conversation_list_state);
        cx.notify();
    }

    pub(in crate::views) fn open_workspace_diff(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_diff.is_none() {
            return;
        }
        self.workspace_diff_selected = self.workspace_diff_selected.min(
            self.workspace_diff
                .as_ref()
                .map_or(0, |diff| diff.files.len().saturating_sub(1)),
        );
        self.workspace_diff_scroll = ScrollHandle::new();
        self.workspace_diff_open = true;
        window.focus(&self.workspace_diff_focus);
        cx.notify();
    }

    pub(in crate::views) fn select_workspace_diff_file(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = self.workspace_diff.clone() else {
            return;
        };
        let Some(file) = snapshot.files.get(index) else {
            return;
        };

        let selection_changed = index != self.workspace_diff_selected;
        let mut folder_path = String::new();
        let mut expanded = false;
        let target_path = file.path.rsplit(" → ").next().unwrap_or(&file.path);
        let mut parts = target_path.split('/').collect::<Vec<_>>();
        parts.pop();
        for folder in parts {
            if !folder_path.is_empty() {
                folder_path.push('/');
            }
            folder_path.push_str(folder);
            expanded |= self.workspace_diff_collapsed_folders.remove(&folder_path);
        }

        if !selection_changed && !expanded {
            return;
        }
        self.workspace_diff_selected = index;
        if let Some(row) = crate::views::diff_summary::file_tree_row_index(
            &snapshot,
            &self.workspace_diff_collapsed_folders,
            index,
        ) {
            self.workspace_diff_files_scroll.scroll_to_item(row);
        }
        if selection_changed {
            self.workspace_diff_scroll = ScrollHandle::new();
        }
        cx.notify();
    }

    pub(in crate::views) fn toggle_workspace_diff_folder(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_diff_collapsed_folders.remove(path) {
            self.workspace_diff_collapsed_folders
                .insert(path.to_owned());
        }
        cx.notify();
    }

    pub(super) fn set_selected_workspace_diff_folder_collapsed(
        &mut self,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self
            .workspace_diff
            .as_ref()
            .and_then(|snapshot| snapshot.files.get(self.workspace_diff_selected))
        else {
            return;
        };
        let target_path = file.path.rsplit(" → ").next().unwrap_or(&file.path);
        let mut parts = target_path.split('/').collect::<Vec<_>>();
        parts.pop();
        let mut folder_path = String::new();
        let mut folders = Vec::new();
        for folder in parts {
            if !folder_path.is_empty() {
                folder_path.push('/');
            }
            folder_path.push_str(folder);
            folders.push(folder_path.clone());
        }

        let changed = if collapsed {
            folders
                .last()
                .is_some_and(|folder| self.workspace_diff_collapsed_folders.insert(folder.clone()))
        } else {
            folders
                .iter()
                .find(|folder| self.workspace_diff_collapsed_folders.contains(*folder))
                .cloned()
                .is_some_and(|folder| self.workspace_diff_collapsed_folders.remove(&folder))
        };
        if changed {
            cx.notify();
        }
    }

    pub(super) fn move_workspace_diff_file(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(snapshot) = self.workspace_diff.as_ref() else {
            return;
        };
        let Some(next) = crate::views::diff_summary::adjacent_file_tree_index(
            snapshot,
            &self.workspace_diff_collapsed_folders,
            self.workspace_diff_selected,
            delta,
        ) else {
            return;
        };
        self.select_workspace_diff_file(next, cx);
    }

    pub(in crate::views) fn on_workspace_diff_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                self.close_workspace_diff(window, cx);
            }
            "up" | "k" => {
                cx.stop_propagation();
                self.move_workspace_diff_file(-1, cx);
            }
            "down" | "j" => {
                cx.stop_propagation();
                self.move_workspace_diff_file(1, cx);
            }
            "left" => {
                cx.stop_propagation();
                self.set_selected_workspace_diff_folder_collapsed(true, cx);
            }
            "right" => {
                cx.stop_propagation();
                self.set_selected_workspace_diff_folder_collapsed(false, cx);
            }
            "home" | "end" => {
                cx.stop_propagation();
                let last = event.keystroke.key == "end";
                let index = self.workspace_diff.as_ref().and_then(|snapshot| {
                    crate::views::diff_summary::edge_file_tree_index(
                        snapshot,
                        &self.workspace_diff_collapsed_folders,
                        last,
                    )
                });
                if let Some(index) = index {
                    self.select_workspace_diff_file(index, cx);
                }
            }
            "tab" => cx.stop_propagation(),
            _ => {}
        }
    }

    pub(in crate::views) fn close_workspace_diff(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_diff_open {
            return;
        }
        self.workspace_diff_open = false;
        window.focus(&self.composer.read(cx).focus_handle(cx));
        cx.notify();
    }

    pub(in crate::views) fn toggle_workspace_diff_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_diff_open {
            self.close_workspace_diff(window, cx);
        } else {
            self.open_workspace_diff(window, cx);
        }
    }

}
