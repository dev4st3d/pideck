use super::*;
use gpui_component::input::{Input, InputEvent};
use notify::Watcher;
use std::{sync::atomic::Ordering, time::Duration};

#[derive(Clone)]
pub(super) struct FileDrag {
    pub(super) paths: Vec<PathBuf>,
}
impl Render for FileDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.0))
            .py(px(4.0))
            .bg(theme::panel())
            .text_color(theme::bone())
            .child(format!("{} item(s)", self.paths.len()))
    }
}

#[derive(Clone, Copy)]
enum FileAction {
    Open,
    NewFile,
    NewFolder,
    Rename,
    Copy,
    Cut,
    Paste,
    Duplicate,
    Trash,
    CopyPath,
    CopyRelative,
    Reveal,
    RevealProject,
    CopyProjectPath,
    Collapse,
    Hidden,
    Refresh,
    HideSidebar,
}

pub(super) struct NameEdit {
    input: Entity<InputState>,
    parent: PathBuf,
    source: Option<PathBuf>,
    directory: bool,
    _subscription: Subscription,
}
impl NameEdit {
    pub(super) fn render(
        &self,
        pending: bool,
        error: Option<&str>,
        cx: &mut Context<FilesPanel>,
    ) -> impl IntoElement {
        div()
            .px(px(chrome::TREE_INSET))
            .py(px(4.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(if self.source.is_some() {
                "Rename"
            } else if self.directory {
                "New folder"
            } else {
                "New file"
            })
            .child(
                Input::new(&self.input)
                    .h(px(32.0))
                    .disabled(pending)
                    .when(error.is_some(), |input| input.border_color(theme::error())),
            )
            .when_some(error, |form, error| {
                form.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme::error())
                        .child(error.to_owned()),
                )
            })
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme::ash())
                    .child("Enter to save · Esc to cancel"),
            )
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && view.operation.is_none() {
                    view.edit = None;
                    view.operation_error = None;
                    window.focus(&view.focus);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(control(
                        "Apply",
                        cx.listener(|view, _, window, cx| view.finish_name(window, cx)),
                    ))
                    .child(control(
                        "Cancel",
                        cx.listener(|view, _, window, cx| {
                            view.edit = None;
                            window.focus(&view.focus);
                            cx.notify();
                        }),
                    )),
            )
    }
}

/// What a menu was opened on; it decides which actions are offered.
#[derive(Clone, Copy, PartialEq)]
enum MenuTarget {
    Panel,
    File,
    Folder,
    Many(usize),
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FileClipboard {
    paths: Vec<PathBuf>,
    cut: bool,
}

impl FilesPanel {
    pub(super) fn select(&mut self, index: usize, toggle: bool, range: bool, previous: usize) {
        if index >= self.rows.len() {
            return;
        }
        if range {
            if self.marked.is_empty() {
                self.anchor = previous;
            }
            self.anchor = self.anchor.min(self.rows.len() - 1);
            self.marked.clear();
            for row in &self.rows[self.anchor.min(index)..=self.anchor.max(index)] {
                self.marked.insert(row.entry.path.clone());
            }
        } else if toggle {
            let path = self.rows[index].entry.path.clone();
            if !self.marked.remove(&path) {
                self.marked.insert(path);
            }
            self.anchor = index;
        } else {
            self.marked.clear();
            self.marked.insert(self.rows[index].entry.path.clone());
            self.anchor = index;
        }
        self.selected = index;
    }
    pub(super) fn selected_paths(&self) -> Vec<PathBuf> {
        file_operations::unique_roots(
            self.rows
                .iter()
                .filter(|row| self.marked.contains(&row.entry.path))
                .map(|row| row.entry.path.clone())
                .collect(),
        )
    }
    fn destination(&self) -> PathBuf {
        self.rows
            .get(self.selected)
            .filter(|row| self.marked.contains(&row.entry.path))
            .map(|row| {
                if row.entry.is_dir {
                    row.entry.path.clone()
                } else {
                    row.entry.path.parent().unwrap_or(&self.root).to_path_buf()
                }
            })
            .unwrap_or_else(|| self.root.clone())
    }
    pub(super) fn perform(&mut self, operation: Operation, _: &mut Window, cx: &mut Context<Self>) {
        if self.operation.is_some() {
            return;
        }
        let affected = match &operation {
            Operation::Rename { from, .. } => vec![from.clone()],
            Operation::Transfer {
                paths, cut: true, ..
            }
            | Operation::Trash(paths) => paths.clone(),
            _ => Vec::new(),
        };
        if self.terminal.upgrade().is_some_and(|terminal| {
            terminal
                .read(cx)
                .paths_busy(&affected, matches!(operation, Operation::Trash(_)), cx)
        }) {
            self.operation_error = Some("Save changed files and wait for open files to finish loading or saving, then retry.".into());
            cx.notify();
            return;
        }
        self.operation_error = None;
        let Some(terminal) = self.terminal.upgrade() else {
            return;
        };
        if !terminal.update(cx, |terminal, cx| terminal.begin_file_change(true, cx)) {
            self.operation_error =
                Some("Wait for the current file operation to finish, then retry.".into());
            cx.notify();
            return;
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        self.operation = Some(cancellation.clone());
        let root = self.root.clone();
        let work = cx
            .background_executor()
            .spawn(async move { file_operations::run(&root, operation, &cancellation) });
        cx.spawn(async move |view, cx| {
            let outcome = work.await;
            let _ = terminal.update(cx, |terminal, cx| {
                terminal.files_moved(&outcome.moved, cx);
                terminal.end_file_change(cx);
            });
            let _ = view.update(cx, |view, cx| {
                view.operation = None;
                view.operation_error = outcome.error;
                if view.operation_error.is_none() {
                    view.edit = None;
                }
                view.expanded = view
                    .expanded
                    .drain()
                    .filter_map(|path| {
                        if outcome
                            .removed
                            .iter()
                            .any(|removed| path.starts_with(removed))
                        {
                            return None;
                        }
                        Some(
                            outcome
                                .moved
                                .iter()
                                .find_map(|(from, to)| {
                                    path.strip_prefix(from)
                                        .ok()
                                        .map(|relative| to.join(relative))
                                })
                                .unwrap_or(path),
                        )
                    })
                    .collect();
                view.directories.retain(|path, _| {
                    !outcome
                        .removed
                        .iter()
                        .chain(outcome.moved.iter().map(|(from, _)| from))
                        .any(|removed| path.starts_with(removed))
                });
                if !outcome.moved.is_empty() {
                    if let Some(clipboard) = cx.read_from_clipboard().and_then(|item| {
                        item.metadata().and_then(|metadata| {
                            serde_json::from_str::<FileClipboard>(metadata).ok()
                        })
                    }) {
                        if clipboard.cut
                            && clipboard
                                .paths
                                .iter()
                                .all(|path| outcome.moved.iter().any(|(from, _)| from == path))
                        {
                            cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
                        }
                    }
                }
                view.refresh(cx);
                cx.emit(ProjectPanelEvent::FilesChanged);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn begin_name(
        &mut self,
        directory: bool,
        rename: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation.is_some() {
            return;
        }
        let source = if rename {
            self.selected_paths().into_iter().next()
        } else {
            None
        };
        if rename && source.is_none() {
            return;
        }
        let parent = source
            .as_ref()
            .and_then(|path| path.parent())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.destination());
        let name = source
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let subscription = cx.subscribe_in(&input, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                view.finish_name(window, cx);
            }
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.edit = Some(NameEdit {
            input,
            parent,
            source,
            directory,
            _subscription: subscription,
        });
        self.operation_error = None;
        cx.notify();
    }
    fn finish_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation.is_some() {
            return;
        }
        let Some(edit) = &self.edit else {
            return;
        };
        let name = edit.input.read(cx).value().to_string();
        match file_operations::named_path(&edit.parent, &name) {
            Ok(path) => {
                let operation = if let Some(from) = &edit.source {
                    Operation::Rename {
                        from: from.clone(),
                        to: path,
                    }
                } else {
                    Operation::Create {
                        path,
                        directory: edit.directory,
                    }
                };
                self.perform(operation, window, cx);
            }
            Err(error) => {
                self.operation_error = Some(error);
                cx.notify();
            }
        }
    }
    fn action(&mut self, action: FileAction, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self.selected_paths();
        match action {
            FileAction::Open => self.open_row(self.selected, cx),
            FileAction::NewFile | FileAction::NewFolder => {
                self.begin_name(matches!(action, FileAction::NewFolder), false, window, cx)
            }
            FileAction::Rename => self.begin_name(false, true, window, cx),
            FileAction::Copy | FileAction::Cut if !paths.is_empty() => {
                let text = paths
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("\n");
                cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(
                    text,
                    FileClipboard {
                        paths: paths.clone(),
                        cut: matches!(action, FileAction::Cut),
                    },
                ));
                #[cfg(windows)]
                if matches!(action, FileAction::Copy) {
                    self.operation_error = file_operations::copy_to_system_clipboard(&paths).err();
                }
            }
            FileAction::Paste => {
                if let Some(clipboard) = cx.read_from_clipboard().and_then(|item| {
                    item.metadata()
                        .and_then(|text| serde_json::from_str::<FileClipboard>(text).ok())
                }) {
                    self.perform(
                        Operation::Transfer {
                            paths: clipboard.paths,
                            destination: self.destination(),
                            cut: clipboard.cut,
                        },
                        window,
                        cx,
                    );
                } else {
                    #[cfg(windows)]
                    match file_operations::system_clipboard_files() {
                        Ok(paths) => self.perform(
                            Operation::Transfer {
                                paths,
                                destination: self.destination(),
                                cut: false,
                            },
                            window,
                            cx,
                        ),
                        Err(error) => self.operation_error = Some(error),
                    }
                    #[cfg(not(windows))]
                    {
                        self.operation_error =
                            Some("Copy files in the explorer first, or drag files here.".into());
                    }
                }
            }
            FileAction::Duplicate if !paths.is_empty() => {
                let destination = paths[0].parent().unwrap_or(&self.root).to_path_buf();
                self.perform(
                    Operation::Transfer {
                        paths,
                        destination,
                        cut: false,
                    },
                    window,
                    cx,
                );
            }
            FileAction::Trash if !paths.is_empty() && self.operation.is_none() => {
                let prompt = window.prompt(
                    gpui::PromptLevel::Warning,
                    &format!("Move {} item(s) to the Recycle Bin?", paths.len()),
                    Some("You can restore them from the Recycle Bin."),
                    &["Cancel", "Move to Recycle Bin"],
                    cx,
                );
                cx.spawn_in(window, async move |view, cx| {
                    if prompt.await == Ok(1) {
                        let _ = view.update_in(cx, |view, window, cx| {
                            view.perform(Operation::Trash(paths), window, cx)
                        });
                    }
                })
                .detach();
            }
            FileAction::CopyPath | FileAction::CopyRelative => {
                let text = paths
                    .iter()
                    .map(|path| {
                        if matches!(action, FileAction::CopyRelative) {
                            path.strip_prefix(&self.root).unwrap_or(path)
                        } else {
                            path.as_path()
                        }
                        .to_string_lossy()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            FileAction::Reveal => cx.reveal_path(paths.first().unwrap_or(&self.root)),
            FileAction::RevealProject => cx.reveal_path(&self.root),
            FileAction::CopyProjectPath => cx.write_to_clipboard(ClipboardItem::new_string(
                self.root.to_string_lossy().into_owned(),
            )),
            FileAction::Collapse => {
                self.expanded.clear();
                self.rebuild_rows();
            }
            FileAction::Hidden => {
                self.hide_hidden = !self.hide_hidden;
                self.rebuild_rows();
            }
            FileAction::HideSidebar => cx.emit(ProjectPanelEvent::ToggleSidebar),
            FileAction::Refresh => {
                self.refresh(cx);
                cx.emit(ProjectPanelEvent::FilesChanged);
            }
            _ => {}
        }
        cx.notify();
    }
    pub(super) fn file_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        let action = match (modifiers.control, modifiers.shift, key) {
            (true, false, "c") => Some(FileAction::Copy),
            (true, false, "x") => Some(FileAction::Cut),
            (true, false, "v") => Some(FileAction::Paste),
            (true, false, "d") => Some(FileAction::Duplicate),
            (true, false, "n") => Some(FileAction::NewFile),
            (true, true, "n") => Some(FileAction::NewFolder),
            (true, true, "c") => Some(FileAction::CopyRelative),
            (false, false, "f2") => Some(FileAction::Rename),
            (false, false, "delete") => Some(FileAction::Trash),
            (false, false, "f5") => Some(FileAction::Refresh),
            _ => None,
        };
        if self.operation.is_some() && key != "escape" {
            cx.stop_propagation();
            return true;
        }
        if let Some(action) = action {
            self.action(action, window, cx);
        } else if modifiers.control && key == "a" {
            self.marked = self.rows.iter().map(|row| row.entry.path.clone()).collect();
        } else if key == "escape" {
            if let Some(cancel) = &self.operation {
                cancel.store(true, Ordering::Release);
            }
            self.edit = None;
            self.operation_error = None;
        } else {
            return false;
        }
        cx.stop_propagation();
        cx.notify();
        true
    }
    pub(super) fn open_context_menu(
        &mut self,
        index: usize,
        position: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let is_dir = row.entry.is_dir;
        if !self.marked.contains(&row.entry.path) {
            self.select(index, false, false, index);
        }
        self.selection_visible = true;
        let target = match self.marked.len() {
            0 | 1 if is_dir => MenuTarget::Folder,
            0 | 1 => MenuTarget::File,
            count => MenuTarget::Many(count),
        };
        let can_paste = Self::can_paste(cx);
        let owner = cx.weak_entity();
        let focus = self.focus.clone();
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            Self::menu(menu.action_context(focus), owner, target, can_paste, false)
        });
        let subscription = cx.subscribe(&menu, |view, _, _: &gpui::DismissEvent, cx| {
            view.context_menu = None;
            cx.notify();
        });
        use gpui::Focusable;
        window.focus(&menu.read(cx).focus_handle(cx));
        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    /// Paste is offered only when the app or Windows clipboard holds files.
    fn can_paste(cx: &gpui::App) -> bool {
        let internal = cx.read_from_clipboard().is_some_and(|item| {
            item.metadata()
                .and_then(|text| serde_json::from_str::<FileClipboard>(text).ok())
                .is_some_and(|clipboard| !clipboard.paths.is_empty())
        });
        #[cfg(windows)]
        let internal = internal
            || file_operations::system_clipboard_files().is_ok_and(|paths| !paths.is_empty());
        internal
    }

    fn menu(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        target: MenuTarget,
        can_paste: bool,
        show_hidden: bool,
    ) -> PopupMenu {
        let item = |label: SharedString, shortcut: Option<&'static str>, action: FileAction| {
            let owner = owner.clone();
            menu_item(label, shortcut).on_click(move |_, window, cx| {
                let owner = owner.clone();
                // PopupMenu restores focus after invoking handlers. Run after dismissal
                // so inline naming and native prompts retain their intended focus.
                window.defer(cx, move |window, cx| {
                    let _ = owner.update(cx, |view, cx| view.action(action, window, cx));
                });
            })
        };
        if target == MenuTarget::Panel {
            return menu
                .item(
                    item("Show hidden files".into(), None, FileAction::Hidden).checked(show_hidden),
                )
                .item(item("Refresh".into(), Some("F5"), FileAction::Refresh))
                .separator()
                .item(item(
                    "Reveal project in File Explorer".into(),
                    None,
                    FileAction::RevealProject,
                ))
                .item(item(
                    "Copy project path".into(),
                    None,
                    FileAction::CopyProjectPath,
                ))
                .separator()
                .item(item(
                    "Hide sidebar".into(),
                    Some("Ctrl+Shift+B"),
                    FileAction::HideSidebar,
                ));
        }
        let many = match target {
            MenuTarget::Many(count) => Some(count),
            _ => None,
        };
        menu.when_some(many, |menu, count| {
            menu.item(PopupMenuItem::label(format!("{count} items selected")))
                .separator()
        })
        .when(target == MenuTarget::File, |menu| {
            menu.item(item("Open".into(), Some("Enter"), FileAction::Open))
                .separator()
        })
        .when(target == MenuTarget::Folder, |menu| {
            menu.item(item("New file".into(), Some("Ctrl+N"), FileAction::NewFile))
                .item(item(
                    "New folder".into(),
                    Some("Ctrl+Shift+N"),
                    FileAction::NewFolder,
                ))
                .separator()
        })
        .item(item("Cut".into(), Some("Ctrl+X"), FileAction::Cut))
        .item(item("Copy".into(), Some("Ctrl+C"), FileAction::Copy))
        .when(many.is_none(), |menu| {
            menu.item(item("Paste".into(), Some("Ctrl+V"), FileAction::Paste).disabled(!can_paste))
        })
        .item(item(
            "Duplicate".into(),
            Some("Ctrl+D"),
            FileAction::Duplicate,
        ))
        .separator()
        .item(item(
            if many.is_some() {
                "Copy paths"
            } else {
                "Copy path"
            }
            .into(),
            None,
            FileAction::CopyPath,
        ))
        .item(item(
            if many.is_some() {
                "Copy relative paths"
            } else {
                "Copy relative path"
            }
            .into(),
            Some("Ctrl+Shift+C"),
            FileAction::CopyRelative,
        ))
        .when(many.is_none(), |menu| {
            menu.item(item(
                "Reveal in File Explorer".into(),
                None,
                FileAction::Reveal,
            ))
        })
        .separator()
        .when(many.is_none(), |menu| {
            menu.item(item("Rename".into(), Some("F2"), FileAction::Rename))
        })
        .item(item(
            match many {
                Some(count) => format!("Move {count} items to Recycle Bin").into(),
                None => "Move to Recycle Bin".into(),
            },
            Some("Delete"),
            FileAction::Trash,
        ))
    }
    pub(super) fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::{
            button::{Button, ButtonVariants},
            menu::DropdownMenu,
        };
        let owner = cx.weak_entity();
        let pending = self.operation.is_some();
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(
                Button::new("new-file-menu")
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .font_family(chrome::CHROME_FONT)
                            .font_weight(FontWeight::NORMAL)
                            .text_size(px(12.0))
                            .text_color(theme::ash())
                            .child(
                                svg()
                                    .path("icons/plus.svg")
                                    .size(px(12.0))
                                    .text_color(theme::ash()),
                            )
                            .child("New"),
                    )
                    .px(px(4.0))
                    .ghost()
                    .h(px(28.0))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .dropdown_menu(move |menu, _, _| {
                        let file_owner = owner.clone();
                        let folder_owner = owner.clone();
                        menu.item(PopupMenuItem::new("New file").disabled(pending).on_click(
                            move |_, window, cx| {
                                let owner = file_owner.clone();
                                window.defer(cx, move |window, cx| {
                                    let _ = owner.update(cx, |view, cx| {
                                        view.action(FileAction::NewFile, window, cx)
                                    });
                                });
                            },
                        ))
                        .item(
                            PopupMenuItem::new("New folder").disabled(pending).on_click(
                                move |_, window, cx| {
                                    let owner = folder_owner.clone();
                                    window.defer(cx, move |window, cx| {
                                        let _ = owner.update(cx, |view, cx| {
                                            view.action(FileAction::NewFolder, window, cx)
                                        });
                                    });
                                },
                            ),
                        )
                    }),
            )
            .child(
                super::icon_control(
                    "collapse-files",
                    "Collapse all folders",
                    "icons/collapse-all.svg",
                )
                .on_click(cx.listener(|view, _, window, cx| {
                    cx.stop_propagation();
                    view.action(FileAction::Collapse, window, cx);
                })),
            )
            .child({
                let owner = cx.weak_entity();
                let show_hidden = !self.hide_hidden;
                Button::new("file-tree-actions")
                    .label("⋯")
                    .ghost()
                    .w(px(24.0))
                    .h(px(28.0))
                    .tooltip("More actions")
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .dropdown_menu(move |menu, _, _| {
                        Self::menu(menu, owner.clone(), MenuTarget::Panel, false, show_hidden)
                    })
            })
            .when(pending, |bar| {
                bar.child(control(
                    "Cancel",
                    cx.listener(|view, _, _, cx| {
                        if let Some(cancel) = &view.operation {
                            cancel.store(true, Ordering::Release);
                        }
                        cx.notify();
                    }),
                ))
            })
    }
    pub(super) fn start_watching(&mut self, cx: &mut Context<Self>) {
        if self.watching {
            return;
        }
        self.watching = true;
        let root = self.root.clone();
        let (sender, receiver) = async_channel::bounded(1);
        let work = cx.background_executor().spawn(async move {
            let mut watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if event
                        .as_ref()
                        .is_ok_and(|event| matches!(event.kind, notify::EventKind::Access(_)))
                    {
                        return;
                    }
                    let _ = sender.try_send(event.is_err());
                })?;
            watcher.watch(&root, notify::RecursiveMode::Recursive)?;
            Ok::<_, notify::Error>(watcher)
        });
        self.watch_task = Some(cx.spawn(async move |view, cx| {
            let watcher = work.await;
            if view
                .update(cx, |view, cx| match watcher {
                    Ok(watcher) => view.watcher = Some(watcher),
                    Err(_) => {
                        view.operation_error =
                            Some("Automatic refresh is unavailable. Press F5 to refresh.".into());
                        cx.notify();
                    }
                })
                .is_err()
            {
                return;
            }
            while let Ok(failed) = receiver.recv().await {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                while receiver.try_recv().is_ok() {}
                if view
                    .update(cx, |view, cx| {
                        if failed {
                            view.operation_error =
                                Some("File watching missed a change. Press F5 to refresh.".into());
                        }
                        view.refresh(cx);
                        cx.emit(ProjectPanelEvent::FilesChanged);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}

impl Drop for FilesPanel {
    fn drop(&mut self) {
        if let Some(cancel) = &self.filter_cancel {
            cancel.store(true, Ordering::Release);
        }
        if let Some(cancel) = &self.operation {
            cancel.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Focusable, TestAppContext, VisualTestContext};

    struct ExplorerWindow {
        files: Entity<FilesPanel>,
        _terminal: Entity<TerminalView>,
    }

    impl Render for ExplorerWindow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .child(div().w(px(256.0)).h_full().child(self.files.clone()))
                .child(div().flex_1().h_full().child(self._terminal.clone()))
        }
    }

    fn open_menu(cx: &mut VisualTestContext, row: usize) -> gpui::Bounds<gpui::Pixels> {
        let row = cx
            .debug_bounds(["explorer-row-0", "explorer-row-1"][row])
            .unwrap();
        let position = gpui::point(row.right() - px(8.0), row.center().y);
        cx.simulate_mouse_down(position, MouseButton::Right, gpui::Modifiers::none());
        cx.simulate_mouse_up(position, MouseButton::Right, gpui::Modifiers::none());
        cx.run_until_parked();
        cx.debug_bounds("explorer-context-menu").unwrap()
    }

    fn click_item(cx: &mut VisualTestContext, selector: &'static str) {
        let item = cx.debug_bounds(selector).expect("menu item is rendered");
        cx.simulate_click(item.center(), gpui::Modifiers::none());
        cx.run_until_parked();
    }

    #[test]
    fn drops_into_self_or_current_parent_are_rejected() {
        let root = PathBuf::from("project");
        let folder = root.join("folder");
        let file = folder.join("a.txt");
        assert!(!accepts_drop(&[folder.clone()], &folder, true));
        assert!(!accepts_drop(
            &[folder.clone()],
            &folder.join("child"),
            false
        ));
        assert!(!accepts_drop(&[file.clone()], &folder, true));
        assert!(accepts_drop(&[file.clone()], &folder, false));
        assert!(accepts_drop(&[file], &root, true));
        assert!(!accepts_drop(&[], &root, true));
    }

    #[gpui::test]
    fn context_menu_clicks_outside_sidebar_dispatch_and_keep_name_focus(cx: &mut TestAppContext) {
        cx.update(super::super::super::file_editor::FileEditor::initialize);
        let root = std::env::temp_dir().join(format!("pideck-menu-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("folder")).unwrap();
        std::fs::write(root.join("a.txt"), "fixture").unwrap();
        let entries = project_files::list_directory(&root, &root).unwrap().entries;
        let (window_root, cx) = cx.add_window_view(|window, cx| {
            let terminal = cx.new(|cx| TerminalView::new(root.clone(), cx));
            let files = cx.new(|cx| {
                let mut files = FilesPanel::new(root.clone(), terminal.downgrade(), window, cx);
                files.directories.insert(
                    root.clone(),
                    DirectoryState {
                        entries,
                        ..Default::default()
                    },
                );
                files.rebuild_rows();
                files
            });
            let host = cx.new(|_| ExplorerWindow {
                files,
                _terminal: terminal,
            });
            gpui_component::Root::new(host, window, cx)
        });
        let host = window_root.read_with(cx, |root, _| {
            root.view()
                .clone()
                .downcast::<ExplorerWindow>()
                .ok()
                .unwrap()
        });
        cx.simulate_resize(gpui::size(px(800.0), px(600.0)));
        let files = host.read_with(cx, |host, _| host.files.clone());
        for (selector, rename, directory) in [
            ("file-menu-New file", false, false),
            ("file-menu-New folder", false, true),
            ("file-menu-Rename", true, false),
        ] {
            let menu = open_menu(cx, 0);
            assert!(menu.right() > px(256.0));
            click_item(cx, selector);
            cx.update(|window, cx| {
                files.update(cx, |files, cx| {
                    assert!(files.context_menu.is_none());
                    let edit = files
                        .edit
                        .as_ref()
                        .expect("menu click must open naming input");
                    assert!(edit.input.read(cx).focus_handle(cx).is_focused(window));
                    assert_eq!(edit.source.is_some(), rename);
                    assert_eq!(edit.directory, directory);
                    files.edit = None;
                    cx.notify();
                });
            });
        }
        // A failed rename must retain both the entered name and its keyboard focus.
        cx.update(|window, cx| {
            files.update(cx, |files, cx| {
                let index = files
                    .rows
                    .iter()
                    .position(|row| row.entry.name == "a.txt")
                    .unwrap();
                files.select(index, false, false, index);
                files.begin_name(false, true, window, cx);
                files
                    .edit
                    .as_ref()
                    .unwrap()
                    .input
                    .update(cx, |input, cx| input.set_value("folder", window, cx));
                files.finish_name(window, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            files.update(cx, |files, cx| {
                let edit = files.edit.as_ref().expect("failed rename retains the form");
                assert_eq!(edit.input.read(cx).value(), "folder");
                assert!(edit.input.read(cx).focus_handle(cx).is_focused(window));
                assert!(files.operation_error.is_some());
                files.edit = None;
                files.operation_error = None;
                cx.notify();
            });
        });
        open_menu(cx, 0);
        click_item(cx, "file-menu-Copy relative path");
        cx.update(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some("folder".into())
            )
        });
        open_menu(cx, 1);
        click_item(cx, "file-menu-Duplicate");
        assert_eq!(
            std::fs::read_to_string(root.join("a copy.txt")).unwrap(),
            "fixture"
        );
        assert!(files.read_with(cx, |files, _| files.operation_error.is_none()));
        open_menu(cx, 0);
        cx.simulate_keystrokes("escape");
        assert!(files.read_with(cx, |files, _| files.context_menu.is_none()));
        open_menu(cx, 0);
        cx.simulate_click(gpui::point(px(750.0), px(550.0)), gpui::Modifiers::none());
        assert!(files.read_with(cx, |files, _| files.context_menu.is_none()));
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// Rejects drops that would nest a folder in itself or move items where they
/// already are, so the tree never highlights a target that would only fail.
pub(super) fn accepts_drop(paths: &[PathBuf], destination: &std::path::Path, cut: bool) -> bool {
    !paths.is_empty()
        && paths.iter().all(|path| {
            !destination.starts_with(path) && !(cut && path.parent() == Some(destination))
        })
}

/// Menu row with its keyboard shortcut right-aligned and muted.
fn menu_item(label: SharedString, shortcut: Option<&'static str>) -> PopupMenuItem {
    PopupMenuItem::element(move |_, _| {
        let selector = format!("file-menu-{label}");
        div()
            .debug_selector(move || selector.clone())
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(24.0))
            .child(label.clone())
            .when_some(shortcut, |row, shortcut| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(chrome::DETAIL_TEXT_SIZE))
                        .text_color(theme::ash())
                        .child(shortcut),
                )
            })
    })
}

pub(super) fn control(
    label: &'static str,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(label)
        .tab_index(0)
        .px(px(6.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .font_weight(FontWeight::NORMAL)
        .text_size(px(chrome::DETAIL_TEXT_SIZE))
        .text_color(theme::ash())
        .cursor_pointer()
        .rounded(px(3.0))
        .hover(|style| style.bg(theme::panel_hover()))
        .active(|style| style.bg(theme::selection()))
        .focus(|style| style.bg(theme::selection()).text_color(theme::focus()))
        .on_click(listener)
        .when(matches!(label, "New file" | "New folder"), |button| {
            button.child(
                svg()
                    .path("icons/plus.svg")
                    .size(px(10.0))
                    .text_color(theme::ash()),
            )
        })
        .child(label)
}
