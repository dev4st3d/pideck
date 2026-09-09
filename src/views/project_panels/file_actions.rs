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
    Collapse,
    Hidden,
    Refresh,
}

pub(super) struct NameEdit {
    input: Entity<InputState>,
    parent: PathBuf,
    source: Option<PathBuf>,
    directory: bool,
    _subscription: Subscription,
}
impl NameEdit {
    pub(super) fn render(&self, cx: &mut Context<FilesPanel>) -> impl IntoElement {
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
            .child(Input::new(&self.input))
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
                self.edit = None;
                window.focus(&self.focus);
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
            FileAction::Collapse => {
                self.expanded.clear();
                self.rebuild_rows();
            }
            FileAction::Hidden => {
                self.hide_hidden = !self.hide_hidden;
                self.rebuild_rows();
            }
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
    pub(super) fn menu(
        &self,
        mut menu: PopupMenu,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> PopupMenu {
        menu = menu.action_context(self.focus.clone());
        for (label, action) in [
            ("New file   Ctrl+N", FileAction::NewFile),
            ("New folder   Ctrl+Shift+N", FileAction::NewFolder),
            ("Rename   F2", FileAction::Rename),
            ("Copy   Ctrl+C", FileAction::Copy),
            ("Cut   Ctrl+X", FileAction::Cut),
            ("Paste   Ctrl+V", FileAction::Paste),
            ("Duplicate   Ctrl+D", FileAction::Duplicate),
            ("Move to Recycle Bin   Delete", FileAction::Trash),
            ("Copy path", FileAction::CopyPath),
            ("Copy relative path", FileAction::CopyRelative),
            ("Reveal in Windows Explorer", FileAction::Reveal),
            ("Collapse all", FileAction::Collapse),
            ("Toggle hidden files", FileAction::Hidden),
            ("Refresh   F5", FileAction::Refresh),
        ] {
            menu =
                menu.item(PopupMenuItem::new(label).on_click(
                    cx.listener(move |view, _, window, cx| view.action(action, window, cx)),
                ));
        }
        menu
    }
    pub(super) fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(chrome::TREE_INSET))
            .pb(px(4.0))
            .flex()
            .flex_wrap()
            .gap(px(4.0))
            .child(control(
                "+ File",
                cx.listener(|view, _, window, cx| view.action(FileAction::NewFile, window, cx)),
            ))
            .child(control(
                "+ Folder",
                cx.listener(|view, _, window, cx| view.action(FileAction::NewFolder, window, cx)),
            ))
            .child(control(
                "Collapse",
                cx.listener(|view, _, window, cx| view.action(FileAction::Collapse, window, cx)),
            ))
            .when(self.operation.is_some(), |bar| {
                bar.child(control(
                    "Cancel operation",
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
        if let Some(cancel) = &self.operation {
            cancel.store(true, Ordering::Release);
        }
    }
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
        .text_size(px(chrome::DETAIL_TEXT_SIZE))
        .cursor_pointer()
        .rounded(px(3.0))
        .hover(|style| style.bg(theme::panel_hover()))
        .focus(|style| style.bg(theme::selection()).text_color(theme::focus()))
        .on_click(listener)
        .child(label)
}
