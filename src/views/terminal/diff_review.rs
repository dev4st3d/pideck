use super::*;
use crate::services::project_git::{GitLineStats, workflow::CommitDetails};

fn stats(value: Option<GitLineStats>) -> gpui::AnyElement {
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(5.0))
        .font_family(theme::mono())
        .text_size(px(10.0))
        .when_some(value, |row, value| {
            row.child(
                div()
                    .text_color(theme::success())
                    .child(format!("+{}", value.additions)),
            )
            .child(
                div()
                    .text_color(theme::error())
                    .child(format!("−{}", value.deletions)),
            )
        })
        .when(value.is_none(), |row| {
            row.child(div().text_color(theme::ash()).child("—"))
        })
        .into_any_element()
}

impl DiffView {
    fn copy_hash(&mut self, cx: &mut Context<Self>) {
        let Some(commit) = &self.file.commit else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(commit.summary.id.clone()));
        self.copied_hash = true;
        self.copy_generation += 1;
        let generation = self.copy_generation;
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_secs(2));
        cx.spawn(async move |view, cx| {
            timer.await;
            let _ = view.update(cx, |view, cx| {
                if view.copy_generation == generation {
                    view.copied_hash = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn commit_header(&self, commit: &CommitDetails, cx: &mut Context<Self>) -> gpui::AnyElement {
        let summary = &commit.summary;
        let parent_label = if commit.parent.is_none() {
            "Empty tree".into()
        } else if summary.parents.len() > 1 {
            format!(
                "Parent {} / {}",
                commit.parent_index + 1,
                summary.parents.len()
            )
        } else {
            "Parent".into()
        };
        div()
            .px(px(12.0))
            .py(px(6.0))
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.0))
                            .line_height(px(20.0))
                            .font_weight(FontWeight::MEDIUM)
                            .id("commit-subject")
                            .tooltip(text_tooltip(summary.subject.clone()))
                            .child(summary.subject.clone()),
                    )
                    .when(summary.local == Some(true), |row| {
                        row.child(
                            svg()
                                .path("icons/arrow-up.svg")
                                .size(px(12.0))
                                .text_color(theme::success()),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme::success())
                                .child("Unpushed"),
                        )
                    })
                    .child(
                        Self::control("commit-details", "Show commit details", true)
                            .h(px(24.0))
                            .px(px(6.0))
                            .child(if self.body_expanded {
                                "Less"
                            } else {
                                "Details"
                            })
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.body_expanded = !view.body_expanded;
                                if !view.body_expanded {
                                    view.parent_picker = false;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .when(self.body_expanded, |panel| {
                panel.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_size(px(11.0))
                        .text_color(theme::ash())
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(format!("{} · {}", summary.author, summary.date)),
                        )
                        .child(div().flex_1())
                        .child(
                            Self::control(
                                "commit-parent",
                                "Choose comparison parent",
                                summary.parents.len() > 1,
                            )
                            .h(px(22.0))
                            .px(px(6.0))
                            .child(parent_label)
                            .when(summary.parents.len() > 1, |button| {
                                button.child(
                                    svg()
                                        .path("icons/chevron-down.svg")
                                        .size(px(11.0))
                                        .text_color(theme::ash()),
                                )
                            })
                            .on_click(cx.listener(|view, _, _, cx| {
                                if view
                                    .file
                                    .commit
                                    .as_ref()
                                    .is_some_and(|commit| commit.summary.parents.len() > 1)
                                {
                                    view.parent_picker = !view.parent_picker;
                                    cx.notify();
                                }
                            })),
                        )
                        .child(
                            Self::control("copy-commit-hash", "Copy full commit hash", true)
                                .h(px(22.0))
                                .px(px(6.0))
                                .font_family(theme::mono())
                                .child(if self.copied_hash {
                                    "Copied".to_owned()
                                } else {
                                    summary.short_id().to_owned()
                                })
                                .on_click(cx.listener(|view, _, _, cx| view.copy_hash(cx))),
                        ),
                )
            })
            .when(self.body_expanded && self.parent_picker, |panel| {
                panel.child(
                    div()
                        .id("commit-parent-options")
                        .max_h(px(140.0))
                        .overflow_y_scroll()
                        .children(summary.parents.iter().enumerate().map(|(index, parent)| {
                            Self::control(
                                ("choose-commit-parent", index),
                                format!("Compare with parent {}", index + 1),
                                true,
                            )
                            .w_full()
                            .child(format!("Parent {} · {}", index + 1, &parent[..7]))
                            .on_click(cx.listener(
                                move |view, _, _, cx| {
                                    view.parent_picker = false;
                                    cx.emit(DiffEvent::Review(ReviewAction::SelectParent(index)));
                                },
                            ))
                        })),
                )
            })
            .when(
                self.body_expanded && !summary.body.trim().is_empty(),
                |panel| {
                    panel.child(
                        div().flex().items_start().gap(px(8.0)).child(
                            div()
                                .id("commit-message-body")
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.0))
                                .text_color(theme::ash())
                                .max_h(px(120.0))
                                .overflow_y_scroll()
                                .child(summary.body.clone()),
                        ),
                    )
                },
            )
            .into_any_element()
    }

    fn committed_file_row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(file) = self
            .file
            .commit
            .as_ref()
            .and_then(|commit| commit.files.get(index))
        else {
            return div().into_any_element();
        };
        let selected = index == self.file.position;
        Self::control(
            ("committed-file", index),
            file.relative_path.to_string_lossy().into_owned(),
            true,
        )
        .debug_selector(move || format!("committed-file-{index}"))
        .h(px(28.0))
        .w_full()
        .min_w_0()
        .px(px(16.0))
        .gap(px(9.0))
        .rounded_none()
        .bg(if selected {
            theme::selection()
        } else {
            theme::floor()
        })
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.emit(DiffEvent::Review(ReviewAction::SelectFile(index)))
        }))
        .child(
            gpui::img(crate::assets::project_icon(&file.path, false, false))
                .size(px(16.0))
                .flex_shrink_0(),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(chrome::CHROME_FONT)
                .text_size(px(12.0))
                .child(file.relative_path.to_string_lossy().into_owned()),
        )
        .child(stats(file.stats))
        .child(
            div()
                .w(px(32.0))
                .flex_shrink_0()
                .flex()
                .gap(px(2.0))
                .when_some(file.stats, |bar, stats| {
                    let fraction = stats.additions as f32
                        / (stats.additions as f32 + stats.deletions as f32).max(1.0);
                    bar.child(div().w(px(30.0 * fraction)).h(px(4.0)).bg(theme::success()))
                        .child(
                            div()
                                .w(px(30.0 * (1.0 - fraction)))
                                .h(px(4.0))
                                .bg(theme::error()),
                        )
                }),
        )
        .into_any_element()
    }

    fn commit_files(
        &self,
        commit: &CommitDetails,
        ready: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let count = commit.files.len();
        let selected_path = commit
            .files
            .get(self.file.position)
            .map(|file| file.relative_path.to_string_lossy().into_owned())
            .unwrap_or_default();
        div()
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .flex_col()
            .border_y_1()
            .border_color(theme::edge())
            .bg(theme::floor())
            .child(
                Self::control(
                    "toggle-commit-files",
                    "Expand or collapse changed files",
                    true,
                )
                .h(px(24.0))
                .w_full()
                .px(px(16.0))
                .rounded_none()
                .child(
                    svg()
                        .path(if self.files_expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        })
                        .size(px(12.0))
                        .text_color(theme::ash()),
                )
                .child(if ready {
                    format!("Changed files · {count}")
                } else {
                    "Changed files".into()
                })
                .child(
                    div()
                        .id("commit-selected-file")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme::ash())
                        .tooltip(text_tooltip(selected_path.clone()))
                        .child(selected_path),
                )
                .when(ready, |row| row.child(stats(commit.stats())))
                .on_click(cx.listener(|view, _, _, cx| {
                    view.files_expanded = !view.files_expanded;
                    cx.notify();
                })),
            )
            .when(self.files_expanded && count > 0, |panel| {
                panel.child(
                    uniform_list(
                        "committed-files",
                        count,
                        cx.processor(|view, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|index| view.committed_file_row(index, cx))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(self.files_scroll.clone())
                    .h(px(count.min(4) as f32 * 28.0)),
                )
            })
            .into_any_element()
    }

    fn working_header(
        &self,
        section: &Section,
        ready: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let relative = self
            .file
            .path
            .strip_prefix(&self.workspace)
            .unwrap_or(&self.file.path)
            .to_string_lossy()
            .replace('\\', " / ")
            .replace('/', " / ")
            .replace("  /  ", " / ");
        let staged = self.file.kind == DiffKind::Staged;
        let enabled = ready && !self.operation_busy && self.file.total > 0;
        div()
            .h(px(34.0))
            .flex_shrink_0()
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .border_b_1()
            .border_color(theme::edge())
            .child(
                gpui::img(crate::assets::project_icon(&self.file.path, false, false))
                    .size(px(16.0))
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.0))
                    .id("diff-file-path")
                    .tooltip(text_tooltip(format!(
                        "{} — {relative}",
                        if staged { "Staged" } else { section.label }
                    )))
                    .child(relative),
            )
            .when(ready, |header| {
                header.child(stats(Some(GitLineStats {
                    additions: section.additions,
                    deletions: section.deletions,
                })))
            })
            .when(!staged, |header| {
                header.child(
                    Self::control(
                        "diff-discard-file",
                        "Discard unstaged file changes",
                        self.can_discard(),
                    )
                    .h(px(26.0))
                    .w(px(26.0))
                    .justify_center()
                    .child(
                        svg()
                            .path("icons/undo.svg")
                            .size(px(13.0))
                            .text_color(theme::ash()),
                    )
                    .when(self.can_discard(), |button| {
                        button.on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(DiffEvent::Review(ReviewAction::Discard(None)))
                        }))
                    }),
                )
            })
            .child(
                Self::control(
                    "diff-stage-file",
                    if staged {
                        "Unstage this file"
                    } else {
                        "Stage this file"
                    },
                    enabled,
                )
                .h(px(26.0))
                .px(px(7.0))
                .child(if staged { "Unstage" } else { "Stage" })
                .when(enabled, |button| {
                    button.on_click(
                        cx.listener(|_, _, _, cx| cx.emit(DiffEvent::Review(ReviewAction::Stage))),
                    )
                }),
            )
            .child(
                Self::control("diff-open-file", "Open file · Ctrl+O", !self.operation_busy)
                    .h(px(26.0))
                    .px(px(7.0))
                    .child("Open")
                    .when(!self.operation_busy, |button| {
                        button.on_click(cx.listener(|_, _, _, cx| cx.emit(DiffEvent::OpenFile)))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn render_review(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let section = &self.sections[self.active];
        let history = self.file.commit.is_some();
        let ready = matches!(self.content, DiffContent::Ready(_));
        let rows = self.display.len();
        let hunks = self.hunk_rows.len();
        let previous = self.file.position > 0;
        let next = self.file.position + 1 < self.file.total;
        div()
            .id("git-diff-review")
            .track_focus(&self.focus)
            .tab_index(0)
            .tab_group()
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .font_family(chrome::CHROME_FONT)
            .font_weight(FontWeight::NORMAL)
            .text_size(px(13.0))
            .line_height(px(20.0))
            .text_color(theme::bone())
            .on_key_down(cx.listener(Self::on_key))
            .child(if let Some(commit) = &self.file.commit {
                div()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .child(self.commit_header(commit, cx))
                    .child(self.commit_files(commit, ready, cx))
                    .into_any_element()
            } else {
                self.working_header(section, ready, cx)
            })
            .child(
                div()
                    .h(px(30.0))
                    .flex_shrink_0()
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .border_b_1()
                    .border_color(theme::edge())
                    .child(
                        Self::control("diff-unified", "Unified diff · Alt+U", true)
                            .h(px(24.0))
                            .bg(if !self.split {
                                theme::panel_hover()
                            } else {
                                gpui::rgba(0)
                            })
                            .child("Unified")
                            .on_click(cx.listener(|view, _, _, cx| view.set_split(false, cx))),
                    )
                    .child(
                        Self::control("diff-split", "Split diff · Alt+S", true)
                            .h(px(24.0))
                            .bg(if self.split {
                                theme::panel_hover()
                            } else {
                                gpui::rgba(0)
                            })
                            .child("Split")
                            .on_click(cx.listener(|view, _, _, cx| view.set_split(true, cx))),
                    )
                    .child(div().flex_1())
                    .child(div().text_size(px(10.0)).text_color(theme::ash()).child(
                        if hunks == 0 {
                            "0 hunks".into()
                        } else {
                            format!("{}/{} hunks", self.hunk_cursor.unwrap_or(0) + 1, hunks)
                        },
                    ))
                    .child(
                        Self::control("previous-hunk", "Previous hunk · Shift+F7", hunks > 0)
                            .h(px(24.0))
                            .px(px(6.0))
                            .child(
                                svg()
                                    .path("icons/chevron-up.svg")
                                    .size(px(12.0))
                                    .text_color(theme::ash()),
                            )
                            .when(hunks > 0, |button| {
                                button.on_click(
                                    cx.listener(|view, _, _, cx| view.navigate_hunk(false, cx)),
                                )
                            }),
                    )
                    .child(
                        Self::control("next-hunk", "Next hunk · F7", hunks > 0)
                            .h(px(24.0))
                            .px(px(6.0))
                            .child(
                                svg()
                                    .path("icons/chevron-down.svg")
                                    .size(px(12.0))
                                    .text_color(theme::ash()),
                            )
                            .when(hunks > 0, |button| {
                                button.on_click(
                                    cx.listener(|view, _, _, cx| view.navigate_hunk(true, cx)),
                                )
                            }),
                    )
                    .child(
                        Self::control(
                            "copy-diff",
                            "Copy selected lines or the complete diff · Ctrl+C",
                            ready,
                        )
                        .h(px(24.0))
                        .px(px(6.0))
                        .child("Copy")
                        .when(ready, |button| {
                            button.on_click(cx.listener(|view, _, _, cx| view.copy(cx)))
                        }),
                    ),
            )
            .when(matches!(self.content, DiffContent::Loading), |panel| {
                panel.child(
                    div()
                        .p(px(16.0))
                        .text_color(theme::ash())
                        .child("Loading diff…"),
                )
            })
            .when_some(
                match &self.content {
                    DiffContent::Error(error) => Some(error.clone()),
                    _ => None,
                },
                |panel, error| {
                    panel.child(
                        div()
                            .p(px(16.0))
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .child(div().text_color(theme::error()).child(error))
                            .child(
                                Self::control("retry-diff", "Retry diff · F5", true)
                                    .child("Retry")
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(DiffEvent::Review(ReviewAction::Navigate(0)))
                                    })),
                            ),
                    )
                },
            )
            .when(ready && !section.metadata.is_empty(), |panel| {
                panel.child(
                    div()
                        .px(px(16.0))
                        .py(px(6.0))
                        .text_size(px(11.0))
                        .text_color(theme::ash())
                        .child(section.metadata.join(" · ")),
                )
            })
            .when(ready && section.conflicted, |panel| {
                panel.child(
                    div()
                        .px(px(16.0))
                        .py(px(6.0))
                        .text_color(theme::error())
                        .child("Resolve this file's conflict before committing."),
                )
            })
            .when(ready && section.truncated, |panel| {
                panel.child(
                    div()
                        .px(px(16.0))
                        .py(px(6.0))
                        .text_size(px(11.0))
                        .text_color(theme::ash())
                        .child("Large diff: some changes are not shown."),
                )
            })
            .when(ready && rows == 0, |panel| {
                panel.child(
                    div()
                        .p(px(16.0))
                        .text_color(theme::ash())
                        .child(if section.binary {
                            "Binary changes cannot be shown as text."
                        } else if history && self.file.total == 0 {
                            "This commit has no file changes in this project."
                        } else {
                            "No text changes"
                        }),
                )
            })
            .when(ready && rows > 0, |panel| {
                panel.child(
                    div()
                        .id("diff-horizontal-scroll")
                        .debug_selector(|| "diff-horizontal-scroll".into())
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w_full()
                                .flex()
                                .flex_col()
                                .when(self.split, |grid| {
                                    grid.child(
                                        div()
                                            .h(px(26.0))
                                            .flex_shrink_0()
                                            .flex()
                                            .bg(theme::chrome())
                                            .text_size(px(10.0))
                                            .text_color(theme::ash())
                                            .child(div().flex_1().px(px(16.0)).child(if history {
                                                "Parent"
                                            } else if self.file.kind == DiffKind::Staged {
                                                "HEAD"
                                            } else {
                                                "Index"
                                            }))
                                            .child(div().flex_1().px(px(16.0)).child(if history {
                                                "Commit"
                                            } else if self.file.kind == DiffKind::Staged {
                                                "Index"
                                            } else {
                                                "Working tree"
                                            })),
                                    )
                                })
                                .child({
                                    let mut lines = uniform_list(
                                        "diff-lines",
                                        rows,
                                        cx.processor(
                                            |view, range: std::ops::Range<usize>, _, cx| {
                                                range
                                                    .map(|index| view.row(index, cx))
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(self.scroll.clone())
                                    .flex_1()
                                    .min_h_0()
                                    .font_family(theme::mono())
                                    .text_size(px(12.0))
                                    .line_height(px(ROW_HEIGHT));
                                    lines.style().restrict_scroll_to_axis = Some(true);
                                    lines
                                }),
                        ),
                )
            })
            .when(!ready || rows == 0, |panel| panel.child(div().flex_1()))
            .child(
                div()
                    .h(px(26.0))
                    .flex_shrink_0()
                    .px(px(12.0))
                    .border_t_1()
                    .border_color(theme::edge())
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(div().text_size(px(10.0)).text_color(theme::ash()).child(
                        if self.file.total == 0 {
                            "0 files".into()
                        } else {
                            format!("{} / {} files", self.file.position + 1, self.file.total)
                        },
                    ))
                    .child(div().flex_1())
                    .when(self.code_scroll.max_offset().width > px(0.0), |bar| {
                        bar.child(
                            div()
                                .id("diff-pan-help")
                                .flex_shrink_0()
                                .text_size(px(10.0))
                                .text_color(theme::ash())
                                .tooltip(text_tooltip(
                                    "Pan horizontally: Shift+wheel or Ctrl+Shift+Left/Right",
                                ))
                                .child("Pan"),
                        )
                    })
                    .child(
                        Self::control("previous-change", "Previous file · Alt+Up", previous)
                            .h(px(22.0))
                            .px(px(6.0))
                            .child("Previous")
                            .when(previous, |button| {
                                button.on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(DiffEvent::Review(ReviewAction::Navigate(-1)))
                                }))
                            }),
                    )
                    .child(
                        Self::control("next-change", "Next file · Alt+Down", next)
                            .h(px(22.0))
                            .px(px(6.0))
                            .child("Next")
                            .when(next, |button| {
                                button.on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(DiffEvent::Review(ReviewAction::Navigate(1)))
                                }))
                            }),
                    ),
            )
            .into_any_element()
    }
}
