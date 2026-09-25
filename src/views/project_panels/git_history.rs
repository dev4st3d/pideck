use super::git_workflow::{git_control, git_quiet};
use super::*;

// Timeline geometry. Rows sit edge to edge so the graph line stays continuous.
const HISTORY_ROW_HEIGHT: f32 = 48.0;
const DAY_ROW_HEIGHT: f32 = 28.0;
const ROW_INSET: f32 = 8.0;
const GRAPH_WIDTH: f32 = 14.0;
const NODE_SIZE: f32 = 8.0;
// Center of the subject line inside a commit row's content box.
const NODE_CENTER: f32 = 14.0;

#[derive(Clone)]
enum HistoryRow {
    Day { label: String, unpushed: usize },
    Commit(usize),
}

pub(super) struct HistoryState {
    pub(super) visible: bool,
    pub(super) anchor: Option<String>,
    commits: Vec<workflow::CommitSummary>,
    rows: Vec<HistoryRow>,
    selected: usize,
    more: bool,
    loading: bool,
    generation: u64,
    today: Option<i64>,
    error: Option<String>,
    scroll: gpui::ListState,
}

impl HistoryState {
    pub(super) fn new() -> Self {
        Self {
            visible: false,
            anchor: None,
            commits: Vec::new(),
            rows: Vec::new(),
            selected: 0,
            more: false,
            loading: false,
            generation: 0,
            today: None,
            error: None,
            scroll: gpui::ListState::new(0, gpui::ListAlignment::Top, px(200.0)),
        }
    }

    fn rebuild(&mut self) {
        self.rows.clear();
        let mut day = "";
        for (index, commit) in self.commits.iter().enumerate() {
            if commit.day() != day {
                day = commit.day();
                let unpushed = self.commits[index..]
                    .iter()
                    .take_while(|item| item.day() == day)
                    .filter(|item| item.local == Some(true))
                    .count();
                self.rows.push(HistoryRow::Day {
                    label: workflow::day_label(day, self.today),
                    unpushed,
                });
            }
            self.rows.push(HistoryRow::Commit(index));
        }
        let offset = self.scroll.logical_scroll_top();
        self.scroll
            .splice(0..self.scroll.item_count(), self.rows.len());
        self.scroll.scroll_to(offset);
    }
}

impl GitPanel {
    pub(super) fn toggle_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.history.visible && self.repository.is_none() {
            return;
        }
        self.history.visible = !self.history.visible;
        self.branch_picker = false;
        self.publish_picker = false;
        self.diff_generation += 1;
        window.focus(&self.focus);
        if self.history.visible {
            if self.history.commits.is_empty()
                || self.history.anchor
                    != self
                        .repository
                        .as_ref()
                        .and_then(|repository| repository.head.clone())
            {
                self.load_history(false, cx);
            } else if let Some(commit) = self.history.commits.get(self.history.selected).cloned() {
                self.open_history_commit(commit, 0, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn load_history(&mut self, append: bool, cx: &mut Context<Self>) {
        if append && (self.history.loading || !self.history.more) {
            return;
        }
        let Some(repository) = self.repository.clone() else {
            self.refresh(cx);
            return;
        };
        self.history.generation += 1;
        let generation = self.history.generation;
        self.history.loading = true;
        self.history.error = None;
        let root = self.root.clone();
        let anchor = append.then(|| self.history.anchor.clone()).flatten();
        let skip = if append {
            self.history.commits.len()
        } else {
            0
        };
        let selected = self
            .history
            .commits
            .get(self.history.selected)
            .map(|commit| commit.id.clone());
        let work = cx.background_executor().spawn(async move {
            workflow::history_page(&root, &repository, anchor.as_deref(), skip)
        });
        cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                if view.history.generation != generation {
                    return;
                }
                view.history.loading = false;
                match result {
                    Ok(page) => {
                        view.history.anchor = page.anchor;
                        view.history.more = page.more;
                        if page.today.is_some() {
                            view.history.today = page.today;
                        }
                        if append {
                            view.history.commits.extend(page.commits);
                        } else {
                            view.history.commits = page.commits;
                        }
                        view.history.selected = selected
                            .and_then(|id| {
                                view.history
                                    .commits
                                    .iter()
                                    .position(|commit| commit.id == id)
                            })
                            .unwrap_or(0);
                        view.history.rebuild();
                        if !append
                            && view.history.visible
                            && let Some(commit) =
                                view.history.commits.get(view.history.selected).cloned()
                        {
                            view.open_history_commit(commit, 0, cx);
                        }
                    }
                    Err(error) => view.history.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn open_history_commit(
        &mut self,
        summary: workflow::CommitSummary,
        parent: usize,
        cx: &mut Context<Self>,
    ) {
        self.diff_generation += 1;
        let generation = self.diff_generation;
        let loading = Arc::new(workflow::CommitDetails {
            summary: summary.clone(),
            parent: summary.parents.get(parent).cloned(),
            parent_index: parent,
            files: Vec::new(),
        });
        let file = ReviewFile {
            path: self.root.clone(),
            kind: DiffKind::WorkingTree,
            position: 0,
            total: 0,
            marker: "",
            commit: Some(loading),
        };
        self.review_file = Some(file.clone());
        cx.emit(ProjectPanelEvent::OpenDiff {
            file: file.clone(),
            content: DiffContent::Loading,
            activate: true,
        });
        let root = self.root.clone();
        let work = cx
            .background_executor()
            .spawn(async move { workflow::commit_details(&root, &summary, parent).map(Arc::new) });
        cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                if view.diff_generation != generation {
                    return;
                }
                match result {
                    Ok(details) => view.open_history_file(details, 0, false, cx),
                    Err(error) => cx.emit(ProjectPanelEvent::OpenDiff {
                        file,
                        content: DiffContent::Error(error),
                        activate: false,
                    }),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn open_history_file(
        &mut self,
        details: Arc<workflow::CommitDetails>,
        position: usize,
        activate: bool,
        cx: &mut Context<Self>,
    ) {
        if !details.files.is_empty() && position >= details.files.len() {
            return;
        }
        self.diff_generation += 1;
        let generation = self.diff_generation;
        let file = ReviewFile {
            path: details
                .files
                .get(position)
                .map(|file| file.path.clone())
                .unwrap_or_else(|| self.root.clone()),
            kind: DiffKind::WorkingTree,
            position,
            total: details.files.len(),
            marker: "",
            commit: Some(details.clone()),
        };
        self.review_file = Some(file.clone());
        if details.files.is_empty() {
            cx.emit(ProjectPanelEvent::OpenDiff {
                file,
                content: DiffContent::Ready(String::new()),
                activate,
            });
            return;
        }
        cx.emit(ProjectPanelEvent::OpenDiff {
            file: file.clone(),
            content: DiffContent::Loading,
            activate,
        });
        let root = self.root.clone();
        let work = cx.background_executor().spawn(async move {
            let diff = workflow::commit_file_diff(&root, &details, position)?;
            let mut text = diff.text;
            if diff.truncated {
                text.push_str("\nDiff truncated: some changes are not shown.\n");
            }
            Ok::<_, String>(text)
        });
        cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                if view.diff_generation != generation {
                    return;
                }
                let content = match result {
                    Ok(text) => DiffContent::Ready(text),
                    Err(error) => DiffContent::Error(error),
                };
                cx.emit(ProjectPanelEvent::OpenDiff {
                    file,
                    content,
                    activate: false,
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn select_history(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(commit) = self.history.commits.get(index).cloned() else {
            return;
        };
        self.history.selected = index;
        if let Some(row) = self
            .history
            .rows
            .iter()
            .position(|row| matches!(row, HistoryRow::Commit(candidate) if *candidate == index))
        {
            self.history.scroll.scroll_to_reveal_item(row);
        }
        self.open_history_commit(commit, 0, cx);
    }

    pub(super) fn history_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" => self.toggle_history(window, cx),
            "f5" => {
                self.refresh(cx);
                self.load_history(false, cx);
            }
            "up" => self.select_history(self.history.selected.saturating_sub(1), cx),
            "down" => self.select_history(
                (self.history.selected + 1).min(self.history.commits.len().saturating_sub(1)),
                cx,
            ),
            "home" => self.select_history(0, cx),
            "end" => self.select_history(self.history.commits.len().saturating_sub(1), cx),
            "enter" => self.select_history(self.history.selected, cx),
            "b" if !self.git_busy() => self.branch_picker = !self.branch_picker,
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn history_row(&self, row: usize, focused: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        match &self.history.rows[row] {
            HistoryRow::Day { label, unpushed } => {
                let unpushed = *unpushed;
                div()
                    .h(px(DAY_ROW_HEIGHT))
                    .w_full()
                    .min_w_0()
                    .px(px(ROW_INSET))
                    .child(
                        div()
                            .size_full()
                            .min_w_0()
                            .pl(px(6.0))
                            .pr(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            // Matches the commit rows' focus border so the graph stays aligned.
                            .border_1()
                            .border_color(rgba(0))
                            .child(timeline(row > 0, row > 0, None))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                    .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme::ash())
                                    .child(label.clone()),
                            )
                            .when(unpushed > 0, |line| {
                                line.child(
                                    div()
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                        .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                                        .text_color(theme::success())
                                        .child(
                                            svg()
                                                .path("icons/arrow-up.svg")
                                                .size(px(10.0))
                                                .text_color(theme::success()),
                                        )
                                        .child(format!("{unpushed} unpushed")),
                                )
                            }),
                    )
                    .into_any_element()
            }
            HistoryRow::Commit(index) => {
                let index = *index;
                let commit = &self.history.commits[index];
                let selected = self.history.selected == index;
                let prefix = commit.prefix();
                let upstream = commit
                    .remote_tip
                    .then(|| self.repository.as_ref()?.upstream.as_ref())
                    .flatten()
                    .map(|upstream| upstream.label.clone());
                let mut tooltip = format!(
                    "{}\n{} · {}\n{}",
                    commit.subject, commit.author, commit.date, commit.id
                );
                if commit.local == Some(true) {
                    tooltip.push_str("\nNot pushed yet");
                }
                div()
                    .h(px(HISTORY_ROW_HEIGHT))
                    .w_full()
                    .min_w_0()
                    .px(px(ROW_INSET))
                    .child(
                        div()
                            .id(("git-history-commit", index))
                            .debug_selector(move || format!("git-history-commit-{index}"))
                            .size_full()
                            .min_w_0()
                            .pl(px(6.0))
                            .pr(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .rounded(px(ROW_RADIUS))
                            .border_1()
                            .border_color(if selected && focused {
                                theme::focus()
                            } else {
                                rgba(0)
                            })
                            .bg(if selected {
                                theme::selection()
                            } else {
                                rgba(0)
                            })
                            .when(!selected, |row| {
                                row.hover(|style| style.bg(theme::panel_hover()))
                            })
                            .cursor_pointer()
                            .tooltip(text_tooltip(tooltip))
                            .on_click(cx.listener(move |view, _, window, cx| {
                                window.focus(&view.focus);
                                view.select_history(index, cx);
                            }))
                            .child(timeline(
                                index > 0,
                                index + 1 < self.history.commits.len() || self.history.more,
                                Some(commit),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.0))
                                    .child(
                                        div()
                                            .h(px(20.0))
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .when_some(prefix, |line, prefix| {
                                                line.child(prefix_badge(prefix))
                                            })
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_size(px(13.0))
                                                    .line_height(px(20.0))
                                                    .text_color(theme::bone())
                                                    .child(
                                                        prefix
                                                            .map_or(commit.subject.as_str(), |p| {
                                                                p.summary
                                                            })
                                                            .to_owned(),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .h(px(chrome::DETAIL_LINE_HEIGHT))
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                            .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                                            .text_color(theme::ash())
                                            .child(div().flex_1().min_w_0().truncate().child(
                                                format!("{} · {}", commit.author, commit.time()),
                                            ))
                                            .when(commit.head, |line| {
                                                line.child(ref_chip(None, "HEAD".into()))
                                            })
                                            .when_some(upstream, |line, upstream| {
                                                line.child(ref_chip(
                                                    Some("icons/branch.svg"),
                                                    upstream,
                                                ))
                                            })
                                            .child(
                                                div()
                                                    .flex_shrink_0()
                                                    .font_family(theme::mono())
                                                    .child(commit.short_id().to_owned()),
                                            ),
                                    ),
                            ),
                    )
                    .into_any_element()
            }
        }
    }

    pub(super) fn render_history_list(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let loading = self.history.loading;
        let count = self.history.commits.len();
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .when(count == 0, |panel| {
                panel.child(message(if loading {
                    "Loading history…"
                } else if self.history.error.is_some() {
                    "History is unavailable."
                } else {
                    "No commits yet"
                }))
            })
            .child(
                gpui::list(
                    self.history.scroll.clone(),
                    cx.processor(|view, index, window, cx| {
                        view.history_row(index, view.focus.is_focused(window), cx)
                    }),
                )
                .flex_1()
                .min_h_0()
                .min_w_0()
                .pb(px(4.0)),
            )
            .when_some(self.history.error.clone(), |panel, error| {
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
                            git_control(
                                "retry-history",
                                "Retry history",
                                Some("icons/refresh.svg"),
                                !loading,
                            )
                            .on_click(cx.listener(|view, _, _, cx| view.load_history(false, cx))),
                        ),
                )
            })
            .when(count > 0, |panel| {
                panel.child(
                    div()
                        .h(px(32.0))
                        .flex_shrink_0()
                        .pl(px(12.0))
                        .pr(px(8.0))
                        .border_t_1()
                        .border_color(theme::edge())
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                                .text_color(theme::ash())
                                .child(match (count, self.history.more) {
                                    (1, false) => "1 commit".to_owned(),
                                    (count, false) => format!("{count} commits"),
                                    (count, true) => format!("Latest {count} commits"),
                                }),
                        )
                        .when(self.history.more || loading, |footer| {
                            footer.child(
                                git_quiet(git_control(
                                    "load-older-commits",
                                    if loading { "Loading…" } else { "Load older" },
                                    Some("icons/chevron-down.svg"),
                                    !loading,
                                ))
                                .h(px(24.0))
                                .px(px(8.0))
                                .on_click(
                                    cx.listener(|view, _, _, cx| view.load_history(true, cx)),
                                ),
                            )
                        }),
                )
            })
            .into_any_element()
    }
}

/// One graph cell. The line skips the newest commit's top and the oldest commit's bottom,
/// and stops at the node edge so hollow nodes read cleanly on any row background.
fn timeline(above: bool, below: bool, commit: Option<&workflow::CommitSummary>) -> gpui::Div {
    let segment = || {
        div()
            .absolute()
            .left(px((GRAPH_WIDTH - 1.0) / 2.0))
            .w(px(1.0))
            .bg(theme::edge())
    };
    let column = div().w(px(GRAPH_WIDTH)).h_full().flex_shrink_0().relative();
    // Offsets of -1 reach across the row's 1px border so adjacent rows join seamlessly.
    let Some(commit) = commit else {
        return column.when(above && below, |column| {
            column.child(segment().top(px(-1.0)).bottom(px(-1.0)))
        });
    };
    let top = NODE_CENTER - NODE_SIZE / 2.0;
    let unpushed = commit.local == Some(true);
    let tone = if unpushed {
        theme::success()
    } else {
        theme::ash()
    };
    column
        .when(above, |column| {
            column.child(segment().top(px(-1.0)).h(px(top + 1.0)))
        })
        .when(below, |column| {
            column.child(segment().top(px(top + NODE_SIZE)).bottom(px(-1.0)))
        })
        .child(
            div()
                .absolute()
                .left(px((GRAPH_WIDTH - NODE_SIZE) / 2.0))
                .top(px(top))
                .size(px(NODE_SIZE))
                .rounded_full()
                .border_color(tone)
                .when(commit.head, |node| node.border_2())
                .when(!commit.head, |node| node.border_1())
                .when(unpushed, |node| node.bg(tone)),
        )
}

fn tint(color: gpui::Rgba, alpha: f32) -> gpui::Rgba {
    gpui::Rgba { a: alpha, ..color }
}

/// Badge for a Conventional Commits prefix. Tone only reinforces the label text.
fn prefix_badge(prefix: workflow::CommitPrefix<'_>) -> gpui::Div {
    let kind = prefix.kind.to_ascii_lowercase();
    let tone = match kind.as_str() {
        _ if prefix.breaking => theme::error(),
        "feat" => theme::success(),
        "fix" | "hotfix" => theme::modified(),
        "revert" => theme::error(),
        _ => theme::ash(),
    };
    div()
        .flex_shrink_0()
        .max_w(px(128.0))
        .h(px(18.0))
        .px(px(5.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .overflow_hidden()
        .rounded(px(3.0))
        .border_1()
        .border_color(tint(tone, 0.32))
        .bg(tint(tone, 0.12))
        .font_family(theme::mono())
        .text_size(px(10.0))
        .line_height(px(14.0))
        .text_color(tone)
        .child(div().flex_shrink_0().child(kind))
        .when_some(prefix.scope, |badge, scope| {
            badge
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(1.0))
                        .h(px(10.0))
                        .bg(tint(tone, 0.32)),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(tint(tone, 0.72))
                        .child(scope.to_owned()),
                )
        })
        .when(prefix.breaking, |badge| {
            badge.child(div().flex_shrink_0().child("!"))
        })
}

/// Neutral reference label such as `HEAD` or the upstream branch.
fn ref_chip(icon: Option<&'static str>, label: String) -> gpui::Div {
    div()
        .flex_shrink_0()
        .max_w(px(112.0))
        .h(px(chrome::DETAIL_LINE_HEIGHT))
        .px(px(4.0))
        .flex()
        .items_center()
        .gap(px(3.0))
        .overflow_hidden()
        .rounded(px(3.0))
        .border_1()
        .border_color(theme::edge())
        .font_family(theme::mono())
        .text_size(px(10.0))
        .line_height(px(14.0))
        .text_color(theme::ash())
        .when_some(icon, |chip, icon| {
            chip.child(
                svg()
                    .path(icon)
                    .size(px(10.0))
                    .flex_shrink_0()
                    .text_color(theme::ash()),
            )
        })
        .child(div().min_w_0().truncate().child(label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn history_renders_and_escape_preserves_the_tree_and_commit_draft(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            crate::fonts::initialize(cx);
            super::super::super::file_editor::FileEditor::initialize(cx);
        });
        let terminal = cx.new(|cx| TerminalView::new("synthetic-project".into(), cx));
        let terminal_weak = terminal.downgrade();
        let (panel, cx) = cx.add_window_view(move |window, cx| {
            let mut panel = GitPanel::new("synthetic-project".into(), terminal_weak, window, cx);
            panel.repository = Some(workflow::Repository {
                root: "synthetic-project".into(),
                branch: Some("main".into()),
                head: Some("a".repeat(40)),
                ..Default::default()
            });
            panel.history.visible = true;
            panel.history.anchor = Some("a".repeat(40));
            panel.history.commits.push(workflow::CommitSummary {
                id: "a".repeat(40),
                parents: Vec::new(),
                author: "Example Developer".into(),
                timestamp: 0,
                date: "2026-09-14 14:32".into(),
                subject: "Example commit".into(),
                body: "A commit body".into(),
                local: Some(true),
                head: true,
                remote_tip: false,
            });
            panel.history.rebuild();
            panel
                .collapsed
                .insert((DiffKind::WorkingTree, "src".into()));
            panel.commit_input.update(cx, |input, cx| {
                input.set_value("Keep this draft", window, cx)
            });
            panel
        });
        cx.simulate_resize(gpui::size(px(360.0), px(720.0)));
        cx.refresh().unwrap();
        cx.run_until_parked();
        assert_eq!(
            cx.debug_bounds("git-history-commit-0").unwrap().size.height,
            px(HISTORY_ROW_HEIGHT)
        );
        cx.update(|window, cx| window.focus(&panel.read(cx).focus));
        cx.simulate_keystrokes("escape");
        cx.refresh().unwrap();
        cx.run_until_parked();
        panel.read_with(cx, |panel, cx| {
            assert!(!panel.history.visible);
            assert!(
                panel
                    .collapsed
                    .contains(&(DiffKind::WorkingTree, "src".into()))
            );
            assert_eq!(
                panel.commit_input.read(cx).value().to_string(),
                "Keep this draft"
            );
        });
    }
}
