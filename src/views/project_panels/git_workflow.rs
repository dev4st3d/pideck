pub(super) use super::git_history::HistoryState;
use super::*;
use project_git::ReviewAction;

enum WriteAction {
    Stage(Vec<GitEntry>, bool),
    Commit(workflow::Repository, String),
    Push(workflow::PushPlan),
    Fetch(workflow::Repository, String),
    Discard(workflow::DiscardPlan),
}

impl WriteAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Stage(_, true) => "Staging…",
            Self::Stage(_, false) => "Unstaging…",
            Self::Commit(..) => "Committing…",
            Self::Push(..) => "Pushing…",
            Self::Fetch(..) => "Fetching…",
            Self::Discard(..) => "Discarding…",
        }
    }
    fn run(self, root: &std::path::Path) -> Result<String, String> {
        match self {
            Self::Stage(entries, staged) => workflow::stage(root, &entries, staged).map(|()| {
                if staged {
                    "Changes staged."
                } else {
                    "Changes unstaged."
                }
                .into()
            }),
            Self::Commit(repository, message) => workflow::commit(root, &repository, &message)
                .map(|id| format!("Commit saved · {}", &id[..7])),
            Self::Push(plan) => {
                workflow::push(&plan).map(|()| format!("Pushed to {}.", plan.label()))
            }
            Self::Fetch(repository, remote) => workflow::fetch(&repository, &remote)
                .map(|()| "Fetched. Review incoming commits before merging.".into()),
            Self::Discard(plan) => workflow::discard(&plan).map(|()| {
                if plan.untracked {
                    "New file moved to the Recycle Bin.".into()
                } else {
                    "Unstaged changes discarded.".into()
                }
            }),
        }
    }
}

impl GitPanel {
    pub(super) fn git_busy(&self) -> bool {
        self.operation.is_some() || self.switching || self.pending_discard.is_some()
    }

    fn perform_git(&mut self, action: WriteAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.git_busy() {
            return;
        }
        let Some(terminal) = self.terminal.upgrade() else {
            return;
        };
        let allow_dirty = matches!(action, WriteAction::Push(_) | WriteAction::Fetch(..));
        if !terminal.update(cx, |terminal, cx| {
            terminal.begin_file_change(allow_dirty, cx)
        }) {
            self.operation_error = Some(
                "Save changed editor files and wait for other file operations, then retry.".into(),
            );
            cx.notify();
            return;
        }
        let is_commit = matches!(action, WriteAction::Commit(..));
        let changes_disk = matches!(action, WriteAction::Discard(_));
        self.operation = Some(action.label());
        self.operation_error = None;
        self.notice = None;
        self.publish_picker = false;
        self.generation += 1;
        self.diff_generation += 1;
        self.loading = false;
        self.refresh_pending = false;
        let root = self.root.clone();
        let work = cx
            .background_executor()
            .spawn(async move { action.run(&root) });
        cx.spawn_in(window, async move |view, cx| {
            let result = work.await;
            let _ = terminal.update(cx, |terminal, cx| terminal.end_file_change(cx));
            let _ = view.update_in(cx, |view, window, cx| {
                view.operation = None;
                match result {
                    Ok(notice) => {
                        view.notice = Some(notice);
                        if is_commit {
                            view.commit_input
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        if changes_disk {
                            cx.emit(ProjectPanelEvent::WorktreeChanged);
                        }
                    }
                    Err(error) => view.operation_error = Some(error),
                }
                view.refresh_review = true;
                view.refresh(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn stage_entries(
        &mut self,
        entries: Vec<GitEntry>,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading {
            return;
        }
        self.perform_git(WriteAction::Stage(entries, staged), window, cx);
    }

    pub(super) fn commit_staged(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_commit(cx) {
            return;
        }
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let message = self.commit_input.read(cx).value().to_string();
        self.perform_git(WriteAction::Commit(repository, message), window, cx);
    }

    fn can_commit(&self, cx: &gpui::App) -> bool {
        !self.git_busy()
            && !self.loading
            && self.repository.is_some()
            && !self.commit_input.read(cx).value().trim().is_empty()
            && self.status.as_ref().is_some_and(|status| {
                !status.truncated
                    && status.entries.iter().any(GitEntry::staged)
                    && !status.entries.iter().any(|entry| entry.conflicted)
            })
    }

    pub(super) fn push_current(
        &mut self,
        remote: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy() || self.loading {
            return;
        }
        let Some(repository) = self.repository.clone() else {
            return;
        };
        if repository.head.is_none() || repository.remotes.is_empty() || repository.ahead == Some(0)
        {
            return;
        }
        if repository.upstream.is_none() && remote.is_none() {
            self.publish_picker = !self.publish_picker;
            cx.notify();
            return;
        }
        match workflow::push_plan(&repository, remote.as_deref()) {
            Ok(plan) => self.perform_git(WriteAction::Push(plan), window, cx),
            Err(error) => {
                self.operation_error = Some(error);
                cx.notify();
            }
        }
    }

    pub(super) fn fetch_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let remote = repository
            .upstream
            .as_ref()
            .map(|upstream| upstream.remote.clone())
            .or_else(|| (repository.remotes.len() == 1).then(|| repository.remotes[0].clone()));
        if let Some(remote) = remote {
            self.perform_git(WriteAction::Fetch(repository, remote), window, cx);
        } else {
            self.operation_error = Some("Choose a branch with an upstream remote to fetch.".into());
            cx.notify();
        }
    }

    pub(super) fn request_discard(
        &mut self,
        entry: GitEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy() || self.loading {
            return;
        }
        self.operation = Some("Preparing discard…");
        self.operation_error = None;
        self.notice = None;
        window.focus(&self.focus);
        let root = self.root.clone();
        let work = cx
            .background_executor()
            .spawn(async move { workflow::prepare_discard(&root, &entry) });
        cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                view.operation = None;
                match result {
                    Ok(plan) => view.pending_discard = Some(plan),
                    Err(error) => view.operation_error = Some(error),
                }
                if view.refresh_pending {
                    view.refresh(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn confirm_discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(plan) = self.pending_discard.take() {
            self.perform_git(WriteAction::Discard(plan), window, cx);
        }
    }

    pub(super) fn refresh_active_review(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self
            .review_file
            .clone()
            .filter(|file| file.commit.is_none())
        else {
            return;
        };
        let entry = self
            .status
            .as_ref()
            .and_then(|status| status.entries.iter().find(|entry| entry.path == file.path))
            .cloned();
        if let Some(entry) = entry {
            let kind = if (file.kind == DiffKind::Staged && entry.staged()) || !entry.unstaged() {
                DiffKind::Staged
            } else {
                DiffKind::WorkingTree
            };
            self.open_entry(entry, kind, false, cx);
        } else {
            let file = ReviewFile {
                total: 0,
                position: 0,
                ..file
            };
            self.review_file = Some(file.clone());
            cx.emit(ProjectPanelEvent::OpenDiff {
                file,
                content: DiffContent::Ready(String::new()),
                activate: false,
            });
        }
    }

    pub(in crate::views) fn dispatch_review(
        &mut self,
        file: &ReviewFile,
        action: &ReviewAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(details) = &file.commit {
            match action {
                ReviewAction::Navigate(direction) => {
                    if *direction == 0 {
                        self.open_history_file(details.clone(), file.position, false, cx);
                    } else if let Some(index) = file
                        .position
                        .checked_add_signed(*direction as isize)
                        .filter(|index| *index < details.files.len())
                    {
                        self.open_history_file(details.clone(), index, false, cx);
                    }
                }
                ReviewAction::SelectFile(index) => {
                    self.open_history_file(details.clone(), *index, false, cx)
                }
                ReviewAction::SelectParent(index) => {
                    self.open_history_commit(details.summary.clone(), *index, cx)
                }
                ReviewAction::Stage | ReviewAction::Discard => {}
            }
            return;
        }
        match action {
            ReviewAction::Navigate(direction) => {
                self.navigate_review(&file.path, file.kind, *direction, cx)
            }
            ReviewAction::Stage | ReviewAction::Discard => {
                let entry = self
                    .status
                    .as_ref()
                    .and_then(|status| status.entries.iter().find(|entry| entry.path == file.path))
                    .cloned();
                if let Some(entry) = entry {
                    if *action == ReviewAction::Discard {
                        self.request_discard(entry, window, cx);
                    } else {
                        self.stage_entries(
                            vec![entry],
                            file.kind == DiffKind::WorkingTree,
                            window,
                            cx,
                        );
                    }
                }
            }
            ReviewAction::SelectFile(_) | ReviewAction::SelectParent(_) => {}
        }
    }
}

pub(super) fn git_control(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    icon: Option<&'static str>,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .h(px(32.0))
        .px(px(10.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(theme::edge())
        .bg(theme::chrome())
        .text_size(px(12.0))
        .text_color(if enabled { theme::bone() } else { theme::ash() })
        .when(!label.is_empty(), |control| {
            control.tooltip(text_tooltip(label.to_string()))
        })
        .when(enabled, |control| {
            control
                .tab_index(0)
                .cursor_pointer()
                .hover(|style| style.bg(theme::panel_hover()))
                .focus(|style| style.border_color(theme::focus()))
        })
        .when_some(icon, |control, icon| {
            control.child(svg().path(icon).size(px(14.0)).text_color(theme::ash()))
        })
        .when(!label.is_empty(), |control| control.child(label))
}

pub(super) fn git_quiet(control: gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div> {
    control.border_color(rgba(0)).bg(rgba(0))
}

fn git_icon_button(
    id: impl Into<gpui::ElementId>,
    icon: Option<&'static str>,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    git_quiet(git_control(id, "", None, enabled))
        .size(px(28.0))
        .flex_shrink_0()
        .px_0()
        .when_some(icon, |control, icon| {
            control.child(svg().path(icon).size(px(16.0)).text_color(if enabled {
                theme::bone_dim()
            } else {
                theme::smoke()
            }))
        })
}

fn stage_icon(kind: DiffKind) -> &'static str {
    if kind == DiffKind::WorkingTree {
        "icons/plus.svg"
    } else {
        "icons/minus.svg"
    }
}

impl GitPanel {
    pub(super) fn render_change_row(
        &self,
        index: usize,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let row = &self.rows[index];
        let entry = row
            .entry
            .and_then(|index| self.status.as_ref()?.entries.get(index));
        let expanded = !self.collapsed.contains(&(row.kind, row.path.clone()));
        let section = row.path.as_os_str().is_empty();
        let selected = (self.selection_visible || focused) && self.selected == index;
        let kind = row.kind;
        let enabled = !self.git_busy() && !self.loading;
        let section_entries = if section {
            self.review_entries(kind)
        } else {
            Vec::new()
        };
        let stage_label = if kind == DiffKind::WorkingTree {
            "Stage"
        } else {
            "Unstage"
        };
        div()
            .h(px(if section { 32.0 } else { GIT_ROW_HEIGHT }))
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .px(px(8.0))
            .relative()
            // Guides sit under each ancestor's chevron: row inset, border,
            // content inset, and half the 14px chevron slot.
            .children((1..row.depth).map(|depth| {
                div()
                    .absolute()
                    .left(px(19.5 + (depth - 1) as f32 * chrome::TREE_INDENT))
                    .top_0()
                    .bottom_0()
                    .w(px(1.0))
                    .bg(theme::edge_soft())
            }))
            .child(
                div()
                    .id(("git-row", index))
                    .group("git-change-row")
                    .debug_selector(move || format!("git-row-{index}"))
                    .size_full()
                    .min_w_0()
                    .overflow_hidden()
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
                    .text_color(if entry.is_none() && !section {
                        theme::bone_dim()
                    } else {
                        theme::bone()
                    })
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
                            .child(if kind == DiffKind::Staged {
                                "Staged"
                            } else {
                                "Unstaged"
                            })
                            .into_any_element()
                    } else {
                        filename(&row.path).into_any_element()
                    })
                    .when(section, |item| {
                        item.child(
                            div()
                                .flex_shrink_0()
                                .font_family(theme::mono())
                                .text_size(px(11.0))
                                .text_color(theme::ash())
                                .child(row.count.to_string()),
                        )
                    })
                    .when(section, |item| {
                        item.child(
                            git_icon_button(
                                ("git-section-stage", index),
                                Some(stage_icon(kind)),
                                enabled
                                    && !section_entries.is_empty()
                                    && self.status.as_ref().is_some_and(|s| !s.truncated),
                            )
                            .tooltip(text_tooltip(format!(
                                "{stage_label} all files in this section"
                            )))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(
                                move |view, _, window, cx| {
                                    cx.stop_propagation();
                                    if view.status.as_ref().is_none_or(|status| status.truncated) {
                                        return;
                                    }
                                    view.stage_entries(
                                        section_entries.clone(),
                                        kind == DiffKind::WorkingTree,
                                        window,
                                        cx,
                                    );
                                },
                            )),
                        )
                    })
                    .when_some(entry.cloned(), |item, entry| {
                        let undo_entry = entry.clone();
                        item.child(line_stats(entry.stats(kind), entry.conflicted))
                            .child(
                                // Row actions stay out of the way until the row is
                                // hovered or selected; keyboard selection reveals them.
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .when(!selected, |actions| actions.invisible())
                                    .group_hover("git-change-row", |style| style.visible())
                                    .child(if kind == DiffKind::WorkingTree {
                                        git_icon_button(
                                            ("git-discard", index),
                                            Some("icons/undo.svg"),
                                            enabled && !entry.conflicted,
                                        )
                                        .tooltip(text_tooltip(if entry.untracked {
                                            "Discard new file · move to Recycle Bin"
                                        } else {
                                            "Discard unstaged changes"
                                        }))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            cx.stop_propagation();
                                            view.request_discard(undo_entry.clone(), window, cx);
                                        }))
                                        .into_any_element()
                                    } else {
                                        div().w(px(28.0)).flex_shrink_0().into_any_element()
                                    })
                                    .child(
                                        git_icon_button(
                                            ("git-stage", index),
                                            Some(stage_icon(kind)),
                                            enabled,
                                        )
                                        .tooltip(text_tooltip(format!(
                                            "{stage_label} {}",
                                            entry.relative_path.display()
                                        )))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(
                                            cx.listener(move |view, _, window, cx| {
                                                cx.stop_propagation();
                                                view.stage_entries(
                                                    vec![entry.clone()],
                                                    kind == DiffKind::WorkingTree,
                                                    window,
                                                    cx,
                                                );
                                            }),
                                        ),
                                    ),
                            )
                    }),
            )
            .into_any_element()
    }

    fn can_push(&self) -> bool {
        let repository = self.repository.as_ref();
        !self.git_busy()
            && !self.loading
            && repository.is_some_and(|repo| {
                repo.head.is_some() && !repo.remotes.is_empty() && repo.ahead != Some(0)
            })
    }

    fn push_label(&self) -> String {
        let repository = self.repository.as_ref();
        if self.operation == Some("Pushing…") {
            "Pushing…".to_string()
        } else if repository.is_some_and(|repo| repo.upstream.is_none()) {
            "Publish".to_string()
        } else if let Some(count) = repository
            .and_then(|repo| repo.ahead)
            .filter(|count| *count > 0)
        {
            format!("Push {count}")
        } else {
            "Push".to_string()
        }
    }

    fn push_tooltip(&self) -> String {
        let repository = self.repository.as_ref();
        if repository.is_some_and(|repo| repo.remotes.is_empty()) {
            "No Git remote configured. Add a remote before pushing.".to_string()
        } else {
            repository
                .and_then(|repo| repo.upstream.as_ref())
                .map(|upstream| format!("Push to {}", upstream.label))
                .unwrap_or_else(|| "Publish this branch to a configured Git remote".to_string())
        }
    }

    fn push_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        git_control("git-push", "", Some("icons/arrow-up.svg"), self.can_push())
            .h(px(28.0))
            .child(self.push_label())
            .tooltip(text_tooltip(self.push_tooltip()))
            .on_click(cx.listener(|view, _, window, cx| view.push_current(None, window, cx)))
            .into_any_element()
    }

    /// Branch select in the panel header; the option list floats over the panel.
    fn branch_select(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let enabled = !self.git_busy() && !self.loading;
        let branch = self.branch().unwrap_or("No repository").to_owned();
        let open = self.branch_picker;
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .on_children_prepainted(cx.processor(|view, bounds: Vec<Bounds<Pixels>>, _, _| {
                view.branch_bounds = bounds.first().copied();
            }))
            .child(
                div()
                    .id("choose-branch")
                    .h(px(28.0))
                    .w_full()
                    .min_w_0()
                    .pl(px(6.0))
                    .pr(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(if open { theme::edge_hard() } else { rgba(0) })
                    .bg(if open { theme::selection() } else { rgba(0) })
                    .when(enabled, |control| {
                        control
                            .tab_index(0)
                            .cursor_pointer()
                            .hover(move |style| {
                                style.bg(if open {
                                    theme::selection()
                                } else {
                                    theme::panel_hover()
                                })
                            })
                            .active(|style| style.bg(theme::selection()))
                            .focus(|style| style.border_color(theme::focus()))
                    })
                    .tooltip(text_tooltip(format!("Switch branch · B\n{branch}")))
                    .on_click(cx.listener(|view, _, window, cx| {
                        if view.git_busy() || view.loading {
                            return;
                        }
                        view.toggle_branch_picker();
                        window.focus(&view.focus);
                        cx.notify();
                    }))
                    .child(panel_icon("icons/branch.svg"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(theme::mono())
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(12.0))
                            .line_height(px(20.0))
                            .child(branch),
                    )
                    .when_some(
                        self.repository
                            .as_ref()
                            .and_then(|repo| repo.ahead)
                            .filter(|ahead| *ahead > 0),
                        |control, ahead| {
                            control.child(
                                div()
                                    .flex_shrink_0()
                                    .font_family(theme::mono())
                                    .text_size(px(11.0))
                                    .line_height(px(16.0))
                                    .text_color(theme::success())
                                    .child(format!("↑{ahead}")),
                            )
                        },
                    )
                    .child(
                        svg()
                            .path(if open {
                                "icons/chevron-up.svg"
                            } else {
                                "icons/chevron-down.svg"
                            })
                            .size(px(12.0))
                            .flex_shrink_0()
                            .text_color(theme::ash()),
                    ),
            )
            .when(open, |select| {
                select.child(
                    div()
                        .absolute()
                        .top(px(28.0 + chrome::MENU_GAP))
                        .left_0()
                        .child(
                            deferred(anchored().snap_to_window().child(self.branch_menu(cx)))
                                .with_priority(1),
                        ),
                )
            })
            .into_any_element()
    }

    fn branch_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = !self.git_busy() && !self.loading;
        let width = self
            .branch_bounds
            .map_or(chrome::MENU_WIDTH, |bounds| f32::from(bounds.size.width))
            .max(chrome::MENU_WIDTH);
        div()
            .id("branch-list")
            .occlude()
            .w(px(width))
            .max_h(px(chrome::MENU_MAX_HEIGHT))
            .overflow_y_scroll()
            .p(px(chrome::MENU_INSET))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .font_family(chrome::CHROME_FONT)
            .text_color(theme::bone())
            .bg(theme::panel_lift())
            .border_1()
            .border_color(theme::edge_hard())
            .rounded(px(chrome::CONTROL_RADIUS))
            .on_mouse_down_out(cx.listener(|view, event: &gpui::MouseDownEvent, _, cx| {
                // The trigger toggles on its own; closing here would reopen it.
                if view
                    .branch_bounds
                    .is_some_and(|bounds| bounds.contains(&event.position))
                {
                    return;
                }
                view.branch_picker = false;
                cx.notify();
            }))
            .child(
                div()
                    .px(px(8.0))
                    .pt(px(4.0))
                    .pb(px(2.0))
                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                    .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                    .text_color(theme::ash())
                    .child("Switch branch"),
            )
            .children(self.branches.iter().enumerate().map(|(index, name)| {
                let chosen = name.clone();
                let current = self.branch() == Some(name.as_str());
                let highlighted = index == self.branch_selected;
                div()
                    .id(("branch-option", index))
                    .h(px(chrome::OPTION_ROW_HEIGHT))
                    .w_full()
                    .flex_shrink_0()
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(if highlighted {
                        theme::edge_hard()
                    } else {
                        rgba(0)
                    })
                    .font_family(theme::mono())
                    .text_size(px(12.0))
                    .line_height(px(20.0))
                    .text_color(if current {
                        theme::bone()
                    } else {
                        theme::bone_dim()
                    })
                    .bg(if highlighted {
                        theme::selection()
                    } else {
                        rgba(0)
                    })
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(|style| style.bg(theme::panel_hover()).text_color(theme::bone()))
                            .on_click(cx.listener(move |view, _, window, cx| {
                                window.focus(&view.focus);
                                view.choose_branch(chosen.clone(), cx)
                            }))
                    })
                    .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                    .when(current, |row| row.child(check_mark()))
            }))
            .when(self.branches.is_empty(), |list| {
                list.child(
                    div()
                        .px(px(8.0))
                        .py(px(6.0))
                        .text_size(px(12.0))
                        .text_color(theme::ash())
                        .child("No local branches yet."),
                )
            })
    }

    fn operation_feedback(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .flex_col()
            .when_some(self.pending_discard.as_ref(), |panel, plan| {
                let path = plan.path.strip_prefix(&self.root).unwrap_or(&plan.path);
                panel.child(
                    div()
                        .p(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .border_t_1()
                        .border_color(theme::edge())
                        .bg(theme::chrome())
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child("Discard file changes?"),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.0))
                                .child(path.to_string_lossy().into_owned()),
                        )
                        .child(div().text_size(px(11.0)).text_color(theme::ash()).child(
                            if plan.untracked {
                                "Move this new file to the Recycle Bin."
                            } else {
                                "Unstaged edits will be removed. Staged content is kept."
                            },
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(6.0))
                                .child(
                                    git_control("cancel-git-discard", "Cancel", None, true)
                                        .on_click(cx.listener(|view, _, window, cx| {
                                            view.pending_discard = None;
                                            window.focus(&view.focus);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    git_control(
                                        "confirm-git-discard",
                                        if plan.untracked {
                                            "Move to Recycle Bin"
                                        } else {
                                            "Discard changes"
                                        },
                                        Some("icons/undo.svg"),
                                        true,
                                    )
                                    .text_color(theme::error())
                                    .border_color(theme::error())
                                    .on_click(cx.listener(
                                        |view, _, window, cx| view.confirm_discard(window, cx),
                                    )),
                                ),
                        ),
                )
            })
            .when_some(self.operation_error.clone(), |panel, error| {
                panel.child(
                    div()
                        .p(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme::error())
                                .child(error),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(6.0))
                                .child(
                                    git_control(
                                        "git-error-refresh",
                                        "Refresh",
                                        Some("icons/refresh.svg"),
                                        !self.git_busy(),
                                    )
                                    .on_click(cx.listener(|view, _, _, cx| view.refresh(cx))),
                                )
                                .child(
                                    git_control(
                                        "git-error-fetch",
                                        "Fetch",
                                        Some("icons/arrow-up.svg"),
                                        !self.git_busy() && self.repository.is_some(),
                                    )
                                    .on_click(cx.listener(
                                        |view, _, window, cx| view.fetch_current(window, cx),
                                    )),
                                )
                                .child(
                                    git_control(
                                        "dismiss-git-error",
                                        "Dismiss",
                                        None,
                                        !self.git_busy(),
                                    )
                                    .on_click(cx.listener(
                                        |view, _, _, cx| {
                                            view.operation_error = None;
                                            cx.notify();
                                        },
                                    )),
                                ),
                        ),
                )
            })
            .when_some(
                self.notice
                    .as_ref()
                    .filter(|_| self.operation_error.is_none()),
                |panel, notice| {
                    panel.child(
                        div()
                            .px(px(12.0))
                            .py(px(6.0))
                            .text_size(px(11.0))
                            .text_color(theme::success())
                            .child(notice.clone()),
                    )
                },
            )
            .into_any_element()
    }

    fn composer(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let staged = self.status.as_ref().map_or(0, |status| {
            status.entries.iter().filter(|entry| entry.staged()).count()
        });
        let can_commit = self.can_commit(cx);
        let can_push = self.can_push();
        let commit_label = if self.operation == Some("Committing…") {
            "Committing…".to_string()
        } else if staged == 0 {
            "Commit".to_string()
        } else {
            format!("Commit {staged}")
        };
        div()
            .px(px(12.0))
            .pt(px(12.0))
            .pb(px(12.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .border_t_1()
            .border_color(theme::edge())
            .child(
                Input::new(&self.commit_input)
                    .h(px(56.0))
                    .disabled(self.git_busy())
                    .font_family(chrome::CHROME_FONT)
                    .text_size(px(13.0))
                    .bg(theme::canvas())
                    .border_color(theme::edge()),
            )
            .child(
                div()
                    .h(px(32.0))
                    .flex()
                    .rounded(px(4.0))
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme::edge())
                    .bg(theme::chrome())
                    .child(
                        div()
                            .id("git-commit")
                            .h_full()
                            .flex_1()
                            .min_w_0()
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(8.0))
                            .border_r_1()
                            .border_color(theme::edge())
                            .text_size(px(12.0))
                            .bg(if can_commit { theme::focus() } else { rgba(0) })
                            .text_color(if can_commit {
                                theme::on_accent()
                            } else {
                                theme::ash()
                            })
                            .tooltip(text_tooltip(if can_commit {
                                "Commit staged changes · Ctrl+Enter"
                            } else {
                                "Write a message and stage changes to commit. Resolve any conflicts first."
                            }))
                            .when(can_commit, |button| {
                                button
                                    .tab_index(0)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(theme::accent_hover()))
                                    .focus(|style| style.bg(theme::accent_pressed()))
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.commit_staged(window, cx)
                                    }))
                            })
                            .when(can_commit, |button| {
                                button.child(
                                    svg()
                                        .path("icons/check.svg")
                                        .size(px(14.0))
                                        .flex_shrink_0()
                                        .text_color(theme::on_accent()),
                                )
                            })
                            .child(div().min_w_0().truncate().child(commit_label)),
                    )
                    .child(
                        div()
                            .id("git-push")
                            .h_full()
                            .px(px(12.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(6.0))
                            .text_size(px(12.0))
                            .text_color(if can_push { theme::bone() } else { theme::ash() })
                            .tooltip(text_tooltip(self.push_tooltip()))
                            .when(can_push, |button| {
                                button
                                    .tab_index(0)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(theme::panel_hover()))
                                    .focus(|style| style.bg(theme::selection()))
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.push_current(None, window, cx)
                                    }))
                            })
                            .child(
                                svg()
                                    .path("icons/arrow-up.svg")
                                    .size(px(14.0))
                                    .flex_shrink_0()
                                    .text_color(theme::ash()),
                            )
                            .child(self.push_label()),
                    ),
            )
            .into_any_element()
    }

    /// Empty change list: confirm the tree is clean and offer the next useful step.
    fn clean_state(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let busy = self.git_busy();
        let repository = self.repository.as_ref();
        let behind = repository.and_then(|repo| repo.behind).unwrap_or(0);
        let ahead = repository.and_then(|repo| repo.ahead).unwrap_or(0);
        let sync = match (ahead, behind) {
            _ if repository.is_some_and(|repo| repo.remotes.is_empty()) => {
                "No remote configured.".to_owned()
            }
            _ if repository.is_some_and(|repo| repo.upstream.is_none()) => {
                "This branch is not published yet.".to_owned()
            }
            (0, 0) => repository
                .and_then(|repo| repo.upstream.as_ref())
                .map_or_else(String::new, |upstream| {
                    format!("In sync with {}.", upstream.label)
                }),
            (ahead, 0) => format!("{ahead} commit{} to push.", plural(ahead)),
            (0, behind) => format!("{behind} commit{} to pull.", plural(behind)),
            (ahead, behind) => format!("{ahead} to push · {behind} to pull."),
        };
        div()
            .flex_1()
            .min_h_0()
            .px(px(24.0))
            .pb(px(40.0))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.0))
            .text_center()
            .when(self.loading, |state| {
                state
                    .text_size(px(12.0))
                    .text_color(theme::ash())
                    .child("Reading Git status…")
            })
            .when(!self.loading, |state| {
                state
                    .child(
                        svg()
                            .path("icons/check.svg")
                            .mb(px(4.0))
                            .size(px(20.0))
                            .text_color(theme::success()),
                    )
                    .child(div().font_weight(FontWeight::MEDIUM).child("No changes"))
                    .when(!sync.is_empty(), |state| {
                        state.child(
                            div()
                                .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                                .text_color(theme::ash())
                                .child(sync),
                        )
                    })
                    .when(repository.is_some(), |state| {
                        state.child(
                            div()
                                .mt(px(12.0))
                                .flex()
                                .flex_wrap()
                                .justify_center()
                                .gap(px(6.0))
                                .when(
                                    repository.is_some_and(|repo| repo.upstream.is_some()),
                                    |actions| {
                                        actions.child(
                                            git_control(
                                                "git-clean-fetch",
                                                "Fetch",
                                                Some("icons/refresh.svg"),
                                                !busy,
                                            )
                                            .on_click(
                                                cx.listener(|view, _, window, cx| {
                                                    if !view.git_busy() {
                                                        view.fetch_current(window, cx);
                                                    }
                                                }),
                                            ),
                                        )
                                    },
                                ),
                        )
                    })
            })
            .into_any_element()
    }

    pub(super) fn render_git_panel(
        &self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let count = self.change_count().unwrap_or(0);
        let busy = self.git_busy();
        div()
            .id("git-panel")
            .track_focus(&self.focus)
            .tab_index(0)
            .tab_group()
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .font_family(chrome::CHROME_FONT)
            .font_weight(FontWeight::NORMAL)
            .text_size(px(13.0))
            .line_height(px(20.0))
            .text_color(theme::bone())
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .h(px(44.0))
                    .px(px(8.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .when(self.history.visible, |header| {
                        header.child(
                            git_icon_button(
                                "git-back-to-changes",
                                Some("icons/chevron-left.svg"),
                                true,
                            )
                            .tooltip(text_tooltip("Back to changes"))
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.toggle_history(window, cx)
                            })),
                        )
                    })
                    .child(self.branch_select(cx))
                    .when_some(
                        self.operation
                            .or((self.loading && self.operation.is_none()).then_some("Refreshing…")),
                        |header, status| {
                            header.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                    .text_color(theme::ash())
                                    .child(status),
                            )
                        },
                    )
                    .when(self.history.visible, |header| {
                        header.child(self.push_button(cx))
                    })
                    .when(!self.history.visible && count > 0, |header| {
                        header.child(
                            git_icon_button(
                                "collapse-git",
                                Some("icons/collapse-all.svg"),
                                true,
                            )
                            .tooltip(text_tooltip("Collapse all folders"))
                            .on_click(cx.listener(|view, _, _, cx| view.collapse_all(cx))),
                        )
                    })
                    .when(!self.history.visible, |header| {
                        header.child(
                            git_icon_button(
                                "git-history",
                                Some("icons/history.svg"),
                                self.repository.is_some(),
                            )
                            .tooltip(text_tooltip("History"))
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.toggle_history(window, cx)
                            })),
                        )
                    })
                    .child(
                        git_icon_button("refresh-git", Some("icons/refresh.svg"), !busy)
                            .tooltip(text_tooltip("Refresh · F5"))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.refresh(cx);
                                if view.history.visible {
                                    view.load_history(false, cx);
                                }
                            })),
                    ),
            )
            .when(self.publish_picker, |panel| {
                panel.child(
                    div()
                        .p(px(12.0))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .child(div().text_size(px(12.0)).child("Publish branch"))
                        .children(
                            self.repository
                                .as_ref()
                                .into_iter()
                                .flat_map(|repo| repo.remotes.iter())
                                .enumerate()
                                .map(|(index, remote)| {
                                    let remote = remote.clone();
                                    let branch_name = self.branch().unwrap_or("branch").to_owned();
                                    git_quiet(git_control(
                                        ("publish-remote", index),
                                        "",
                                        Some("icons/arrow-up.svg"),
                                        !busy,
                                    ))
                                    .w_full()
                                    .justify_start()
                                    .child(remote.clone())
                                    .tooltip(text_tooltip(format!(
                                        "Publish to {remote}/{branch_name}"
                                    )))
                                    .on_click(cx.listener(
                                        move |view, _, window, cx| {
                                            view.push_current(Some(remote.clone()), window, cx)
                                        },
                                    ))
                                }),
                        )
                        .child(
                            git_control("cancel-publish", "Cancel", None, !busy).on_click(
                                cx.listener(|view, _, _, cx| {
                                    view.publish_picker = false;
                                    cx.notify();
                                }),
                            ),
                        ),
                )
            })
            .child(if self.history.visible {
                self.render_history_list(cx)
            } else if count == 0 && self.error.is_none() {
                self.clean_state(cx)
            } else {
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        gpui::list(
                            self.scroll.clone(),
                            cx.processor(|view, index, window, cx| {
                                view.row(index, view.focus.is_focused(window), cx)
                                    .into_any_element()
                            }),
                        )
                        .flex_1()
                        .min_h_0()
                        .min_w_0(),
                    )
                    .when(
                        self.status.as_ref().is_some_and(|status| status.truncated),
                        |panel| panel.child(message("Large change list: some entries are hidden.")),
                    )
                    .into_any_element()
            })
            .when_some(self.error.clone(), |panel, error| {
                panel.child(
                    div()
                        .p(px(12.0))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme::error())
                                .child(error),
                        )
                        .child(
                            git_control(
                                "retry-git-status",
                                "Retry",
                                Some("icons/refresh.svg"),
                                !busy,
                            )
                            .on_click(cx.listener(|view, _, _, cx| view.refresh(cx))),
                        ),
                )
            })
            .child(self.operation_feedback(cx))
            .when(!self.history.visible, |panel| {
                panel.child(self.composer(cx))
            })
            .into_any_element()
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn line_stats(stats: Option<project_git::GitLineStats>, conflict: bool) -> gpui::AnyElement {
    div()
        .w(px(68.0))
        .flex_shrink_0()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .justify_end()
        .gap(px(4.0))
        .font_family(theme::mono())
        .text_size(px(10.0))
        .when_some(stats.filter(|_| !conflict), |row, stats| {
            row.child(
                div()
                    .text_color(theme::success())
                    .child(format!("+{}", stats.additions)),
            )
            .child(
                div()
                    .text_color(theme::error())
                    .child(format!("−{}", stats.deletions)),
            )
        })
        .when(conflict || stats.is_none(), |row| {
            row.child(
                div()
                    .text_color(if conflict {
                        theme::error()
                    } else {
                        theme::ash()
                    })
                    .child(if conflict { "Conflict" } else { "—" }),
            )
        })
        .into_any_element()
}
