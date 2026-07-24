use super::shared::{action_id, runtime_operation_label, short_path};
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitlebarStatusTone {
    Idle,
    Working,
    Attention,
    Complete,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TitlebarStatus {
    label: String,
    tone: TitlebarStatusTone,
    animated: bool,
}

impl TitlebarStatus {
    fn new(label: impl Into<String>, tone: TitlebarStatusTone, animated: bool) -> Self {
        Self {
            label: label.into(),
            tone,
            animated,
        }
    }

    fn color(&self) -> gpui::Rgba {
        match self.tone {
            TitlebarStatusTone::Idle => theme::ash(),
            TitlebarStatusTone::Working => theme::working(),
            TitlebarStatusTone::Attention => theme::data(),
            TitlebarStatusTone::Complete => theme::live(),
            TitlebarStatusTone::Error => theme::error(),
        }
    }
}

pub(super) fn titlebar(
    projection: &ShellProjection,
    conversation: &ConversationProjection,
    name_composer: &Entity<Composer>,
    rename_open: bool,
    rename_enabled: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let action = projection.action;
    let status = titlebar_status(projection, conversation);
    let status_color = status.color();
    div()
        .h(px(theme::TITLE_H))
        .px(px(theme::PAD_X))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .bg(theme::floor())
        .border_b_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .font_family(theme::main())
                        .text_size(px(theme::T_WORDMARK))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(theme::bone())
                        .flex_shrink_0()
                        .child("Pideck"),
                )
                .child(
                    div()
                        .w(px(1.0))
                        .h(px(14.0))
                        .bg(theme::edge_hard())
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .relative()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            div()
                                .min_w_0()
                                .font_family(theme::sans())
                                .text_size(px(theme::T_TITLE))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::bone())
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(projection.session.label()),
                        )
                        .child(controls::icon_button(
                            "rename-session",
                            "✎",
                            rename_open,
                            rename_enabled,
                            Box::new(cx.listener(|view, _, window, cx| {
                                view.toggle_session_rename(window, cx)
                            })),
                        ))
                        .when(rename_open, |title| {
                            title.child(deferred(
                                div()
                                    .id("rename-session-popup")
                                    .absolute()
                                    .top(px(32.0))
                                    .left_0()
                                    .w(px(theme::SIDE_W))
                                    .p(px(10.0))
                                    .occlude()
                                    .rounded(px(theme::RADIUS_SM))
                                    .border_1()
                                    .border_color(theme::edge_hard())
                                    .bg(theme::panel_lift())
                                    .child(controls::section_label("Rename current session"))
                                    .child(div().mt(px(7.0)).child(name_composer.clone())),
                            ))
                        }),
                )
                .when(projection.has_stale_values, |row| {
                    row.child(
                        div()
                            .font_family(theme::mono())
                            .text_size(px(theme::T_TINY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::smoke())
                            .flex_shrink_0()
                            .child("stale"),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .flex_shrink_0()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(controls::square_status_indicator(
                            0,
                            status.animated,
                            Duration::from_millis(720),
                            status_color,
                        ))
                        .child(
                            div()
                                .font_family(theme::main())
                                .text_size(px(theme::T_UI_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(status_color)
                                .whitespace_nowrap()
                                .child(status.label),
                        ),
                )
                .when(action.is_some(), |row| row.child(controls::meta_sep()))
                .when_some(action, |row, action| {
                    row.child(controls::recovery_button(
                        action_id(action),
                        action.label().to_owned(),
                        action.shortcut(),
                        true,
                        Box::new(cx.listener(move |view, _, _, cx| {
                            view.activate_recovery(action, cx);
                        })),
                    ))
                }),
        )
}

fn titlebar_status(
    projection: &ShellProjection,
    conversation: &ConversationProjection,
) -> TitlebarStatus {
    titlebar_status_for(
        &projection.lifecycle,
        conversation.lifecycle,
        matches!(
            conversation.status,
            crate::state::runtime::FacetStatus::Loading
        ),
        conversation.error.is_some(),
        &conversation.retry,
        &conversation.compaction,
        conversation.pending_operation.as_ref(),
    )
}

fn titlebar_status_for(
    shell_lifecycle: &str,
    lifecycle: RuntimeLifecycle,
    conversation_loading: bool,
    has_error: bool,
    retry: &RetryState,
    compaction: &CompactionState,
    pending_operation: Option<&RuntimeOperation>,
) -> TitlebarStatus {
    match shell_lifecycle {
        "Not connected" | "Stopped" => {
            return TitlebarStatus::new("Disconnected", TitlebarStatusTone::Idle, false);
        }
        "Connecting" => {
            return TitlebarStatus::new("Connecting", TitlebarStatusTone::Working, true);
        }
        "Stopping" => {
            return TitlebarStatus::new("Stopping", TitlebarStatusTone::Attention, true);
        }
        "Connection error" => {
            return TitlebarStatus::new("Disconnected", TitlebarStatusTone::Error, false);
        }
        "No model" => {
            return TitlebarStatus::new("No model", TitlebarStatusTone::Error, false);
        }
        _ => {}
    }

    if lifecycle == RuntimeLifecycle::Disconnected {
        return TitlebarStatus::new("Disconnected", TitlebarStatusTone::Error, false);
    }
    if lifecycle == RuntimeLifecycle::Failed {
        return TitlebarStatus::new("Error", TitlebarStatusTone::Error, false);
    }

    match retry {
        RetryState::Waiting {
            attempt,
            max_attempts,
            ..
        } => {
            return TitlebarStatus::new(
                format!("Retrying {attempt}/{max_attempts}"),
                TitlebarStatusTone::Attention,
                true,
            );
        }
        RetryState::Cancelling => {
            return TitlebarStatus::new("Cancelling retry", TitlebarStatusTone::Attention, true);
        }
        RetryState::Idle | RetryState::Succeeded { .. } | RetryState::Failed { .. } => {}
    }

    match compaction {
        CompactionState::Running { .. } => {
            return TitlebarStatus::new("Compacting", TitlebarStatusTone::Attention, true);
        }
        CompactionState::Completed {
            will_retry: true, ..
        } if lifecycle == RuntimeLifecycle::Running && matches!(retry, RetryState::Idle) => {
            return TitlebarStatus::new(
                "Retrying after compaction",
                TitlebarStatusTone::Attention,
                true,
            );
        }
        CompactionState::Idle
        | CompactionState::Completed { .. }
        | CompactionState::Failed { .. }
        | CompactionState::Aborted { .. } => {}
    }

    if let Some(operation) = pending_operation {
        return TitlebarStatus::new(
            runtime_operation_label(operation),
            TitlebarStatusTone::Attention,
            true,
        );
    }
    if has_error {
        return TitlebarStatus::new("Conversation error", TitlebarStatusTone::Error, false);
    }
    if lifecycle == RuntimeLifecycle::Running {
        return TitlebarStatus::new("Working", TitlebarStatusTone::Working, true);
    }
    if let RetryState::Failed { attempt, .. } = retry {
        return TitlebarStatus::new(
            format!("Retry failed · attempt {attempt}"),
            TitlebarStatusTone::Error,
            false,
        );
    }
    if matches!(compaction, CompactionState::Failed { .. }) {
        return TitlebarStatus::new("Compaction failed", TitlebarStatusTone::Error, false);
    }
    if matches!(compaction, CompactionState::Aborted { .. }) {
        return TitlebarStatus::new("Compaction aborted", TitlebarStatusTone::Attention, false);
    }
    if conversation_loading {
        return TitlebarStatus::new("Loading conversation", TitlebarStatusTone::Working, true);
    }

    match lifecycle {
        RuntimeLifecycle::Loading => {
            TitlebarStatus::new("Loading", TitlebarStatusTone::Working, true)
        }
        RuntimeLifecycle::Ready => TitlebarStatus::new("Idle", TitlebarStatusTone::Idle, false),
        RuntimeLifecycle::Running => unreachable!("running status returns above"),
        RuntimeLifecycle::Cancelling => {
            TitlebarStatus::new("Cancelling", TitlebarStatusTone::Attention, true)
        }
        RuntimeLifecycle::Settled => {
            TitlebarStatus::new("Finished", TitlebarStatusTone::Complete, false)
        }
        RuntimeLifecycle::Disconnected => {
            TitlebarStatus::new("Disconnected", TitlebarStatusTone::Error, false)
        }
        RuntimeLifecycle::Failed => TitlebarStatus::new("Error", TitlebarStatusTone::Error, false),
    }
}

pub(super) struct SessionsPanelParams<'a> {
    pub(super) catalog: &'a CatalogProjection,
    pub(super) projection: &'a ShellProjection,
    pub(super) conversation: &'a ConversationProjection,
    pub(super) history_open: bool,
    pub(super) menu_open: bool,
}

pub(super) fn sessions_panel(
    params: SessionsPanelParams<'_>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let SessionsPanelParams {
        catalog,
        projection,
        conversation,
        history_open,
        menu_open,
    } = params;
    let session_actions_enabled = matches!(
        conversation.lifecycle,
        RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
    ) && conversation.pending_operation.is_none()
        && !catalog.switching;
    let current_path = catalog.current_session_file.as_ref();
    let state_copy = match catalog.status {
        CatalogStatus::Loading if catalog.sessions.is_empty() => "Scanning…".to_owned(),
        CatalogStatus::Loading => "Refreshing".to_owned(),
        CatalogStatus::Ready => format!("{}", catalog.sessions.len()),
        CatalogStatus::Empty => "0".to_owned(),
        CatalogStatus::Inaccessible => "Error".to_owned(),
        CatalogStatus::Stale => "Stale".to_owned(),
    };
    let folder = short_path(&projection.workspace);

    div()
        .w(px(theme::SIDE_W))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .relative()
        .bg(theme::floor())
        .border_r_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .h(px(42.0))
                .px(px(14.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(controls::section_label("Sessions"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(2.0))
                        .child(
                            div()
                                .min_w(px(20.0))
                                .font_family(theme::mono())
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .text_align(gpui::TextAlign::Right)
                                .child(state_copy),
                        )
                        .child(controls::icon_button(
                            "refresh-sessions",
                            "↻",
                            false,
                            catalog.status != CatalogStatus::Loading,
                            Box::new(cx.listener(|view, _, _, cx| view.refresh_sessions(cx))),
                        )),
                ),
        )
        .child(
            div()
                .px(px(10.0))
                .pb(px(10.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(controls::tone_button(
                            "new-session",
                            "New session",
                            session_actions_enabled,
                            controls::ControlTone::Normal,
                            Box::new(cx.listener(|view, _, window, cx| {
                                let _ = view.execute_native_action(
                                    NativeAction::NewSession,
                                    "",
                                    window,
                                    cx,
                                );
                            })),
                        ))
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .child(controls::tone_button(
                                    "session-tools",
                                    "History / export",
                                    true,
                                    controls::ControlTone::Normal,
                                    Box::new(
                                        cx.listener(|view, _, _, cx| view.toggle_session_menu(cx)),
                                    ),
                                ))
                                .when(menu_open, |host| {
                                    host.child(deferred(
                                        div()
                                            .id("session-tools-popup")
                                            .absolute()
                                            .top(px(32.0))
                                            .right_0()
                                            .w(px(172.0))
                                            .py(px(4.0))
                                            .occlude()
                                            .rounded(px(theme::RADIUS_SM))
                                            .border_1()
                                            .border_color(theme::edge_hard())
                                            .bg(theme::panel_lift())
                                            .child(controls::action_row(
                                                "toggle-history",
                                                if history_open {
                                                    "Hide history"
                                                } else {
                                                    "Show history"
                                                },
                                                "Conversation tree",
                                                true,
                                                controls::ControlTone::Normal,
                                                Box::new(cx.listener(|view, _, window, cx| {
                                                    let _ = view.execute_native_action(
                                                        NativeAction::Tree,
                                                        "",
                                                        window,
                                                        cx,
                                                    );
                                                })),
                                            ))
                                            .child(controls::action_row(
                                                "export-session",
                                                "Export HTML",
                                                "Save this session",
                                                session_actions_enabled
                                                    && catalog.current_session_file.is_some(),
                                                controls::ControlTone::Normal,
                                                Box::new(cx.listener(|view, _, window, cx| {
                                                    view.export_session_from_menu(window, cx)
                                                })),
                                            )),
                                    ))
                                }),
                        ),
                ),
        )
        .when(
            matches!(
                catalog.status,
                CatalogStatus::Inaccessible | CatalogStatus::Stale
            ),
            |panel| {
                panel.child(
                    div()
                        .mx(px(12.0))
                        .mb(px(8.0))
                        .px(px(10.0))
                        .py(px(8.0))
                        .rounded(px(theme::RADIUS_SM))
                        .bg(theme::panel())
                        .border_1()
                        .border_color(theme::edge_soft())
                        .font_family(theme::sans())
                        .text_size(px(theme::T_TINY))
                        .line_height(gpui::relative(1.4))
                        .text_color(theme::bone_dim())
                        .child(
                            catalog
                                .error
                                .clone()
                                .unwrap_or_else(|| "Session catalog needs attention.".to_owned()),
                        ),
                )
            },
        )
        .when(!catalog.corrupt.is_empty(), |panel| {
            panel.child(
                div()
                    .mx(px(12.0))
                    .mb(px(8.0))
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(theme::panel())
                    .border_1()
                    .border_color(theme::error())
                    .font_family(theme::sans())
                    .text_size(px(theme::T_TINY))
                    .line_height(gpui::relative(1.4))
                    .text_color(theme::bone_dim())
                    .child(format!(
                        "{} corrupt file{} skipped.",
                        catalog.corrupt.len(),
                        if catalog.corrupt.len() == 1 { "" } else { "s" }
                    )),
            )
        })
        .child(
            div()
                .id("sessions-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .when(catalog.sessions.is_empty(), |list| {
                            list.child(controls::empty_list_note(match catalog.status {
                                CatalogStatus::Loading => "Scanning sessions…",
                                CatalogStatus::Empty => "No saved sessions yet.",
                                CatalogStatus::Inaccessible => "Catalog unavailable.",
                                _ => "No sessions to show.",
                            }))
                        })
                        .children(catalog.sessions.iter().map(|session| {
                            let selected = current_path.is_some_and(|path| path == &session.path);
                            let path = session.path.clone();
                            let title = session
                                .name
                                .clone()
                                .or_else(|| session.first_user_summary.clone())
                                .unwrap_or_else(|| "Untitled session".to_owned());
                            let detail = format!(
                                "{} msg · v{} · {}",
                                session.counts.messages, session.version, session.updated_at
                            );
                            controls::interactive_list_row(
                                gpui::SharedString::from(format!("session-{}", session.id)),
                                session_actions_enabled && !selected,
                                Box::new(cx.listener(move |view, _, _, cx| {
                                    view.switch_session(path.clone(), cx)
                                })),
                                controls::session_row(title, detail, selected),
                            )
                        })),
                ),
        )
        .child(
            div()
                .px(px(14.0))
                .py(px(12.0))
                .border_t_1()
                .border_color(theme::edge_soft())
                .child(controls::section_label("Folder"))
                .child(
                    div()
                        .mt(px(5.0))
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .line_height(gpui::relative(1.4))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::data())
                        .child(folder),
                ),
        )
}

pub(super) struct HistoryPanelParams<'a> {
    pub(super) projection: &'a HistoryProjection,
    pub(super) bridge: &'a BridgeProjection,
    pub(super) browser: &'a HistoryBrowser,
    pub(super) focus: &'a FocusHandle,
    pub(super) search: &'a Entity<Composer>,
    pub(super) label: &'a Entity<Composer>,
    pub(super) import_path: &'a Entity<Composer>,
    pub(super) confirmation: Option<&'a HistoryConfirmation>,
    pub(super) summarize: bool,
}

pub(super) fn history_panel(
    params: HistoryPanelParams<'_>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let HistoryPanelParams {
        projection,
        bridge,
        browser,
        focus,
        search,
        label,
        import_path,
        confirmation,
        summarize,
    } = params;
    let rows = browser.rows(&projection.tree, projection.leaf_id.as_ref());
    let details = browser.details(&projection.tree, projection.leaf_id.as_ref());
    let selected = browser.selected().cloned();
    let selected_is_forkable = selected.as_ref().is_some_and(|selected| {
        projection
            .fork_messages
            .iter()
            .any(|message| &message.entry_id == selected)
    });
    let ready = matches!(
        projection.lifecycle,
        RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
    ) && !projection.switching
        && bridge.pending.is_none();
    let capabilities = bridge.capabilities.as_ref();
    let navigation_available = capabilities.is_some_and(|capabilities| capabilities.navigate_tree);
    let labels_available = capabilities.is_some_and(|capabilities| capabilities.labels);
    let export_available = capabilities.is_some_and(|capabilities| capabilities.jsonl_export);
    let import_available = capabilities.is_some_and(|capabilities| capabilities.jsonl_import);
    let summary_available = capabilities.is_some_and(|capabilities| capabilities.branch_summary);
    let filter = browser.filter();

    div()
        .id("history-tree")
        .track_focus(focus)
        .tab_index(0)
        .key_context("HistoryTree")
        .w(px(theme::HISTORY_W))
        .flex_shrink_0()
        .h_full()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme::floor())
        .border_r_1()
        .border_color(theme::edge_hard())
        .focus(|panel| panel.border_color(theme::focus()))
        .child(
            div()
                .px(px(12.0))
                .pt(px(12.0))
                .pb(px(8.0))
                .flex()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("History"))
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(format!("{}", rows.len())),
                ),
        )
        .child(div().px(px(10.0)).pb(px(6.0)).child(search.clone()))
        .child(
            div()
                .px(px(10.0))
                .pb(px(8.0))
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(4.0))
                .child(controls::chip_button(
                    "history-all",
                    "All",
                    filter == HistoryFilter::All,
                    true,
                    Box::new(cx.listener(|view, _, _, cx| {
                        view.set_history_filter(HistoryFilter::All, cx)
                    })),
                ))
                .child(controls::chip_button(
                    "history-messages",
                    "Messages",
                    filter == HistoryFilter::Messages,
                    true,
                    Box::new(cx.listener(|view, _, _, cx| {
                        view.set_history_filter(HistoryFilter::Messages, cx)
                    })),
                ))
                .child(controls::chip_button(
                    "history-summaries",
                    "Summaries",
                    filter == HistoryFilter::Summaries,
                    true,
                    Box::new(cx.listener(|view, _, _, cx| {
                        view.set_history_filter(HistoryFilter::Summaries, cx)
                    })),
                ))
                .child(controls::chip_button(
                    "history-labels",
                    "Labels",
                    filter == HistoryFilter::Labels,
                    true,
                    Box::new(cx.listener(|view, _, _, cx| {
                        view.set_history_filter(HistoryFilter::Labels, cx)
                    })),
                )),
        )
        .child(
            div()
                .id("history-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .scrollbar_width(px(theme::SCROLLBAR))
                .when(rows.is_empty(), |list| {
                    list.child(controls::empty_list_note(match projection.status {
                        crate::state::runtime::FacetStatus::Loading => "Loading history…",
                        crate::state::runtime::FacetStatus::Failed(_) => "History unavailable.",
                        crate::state::runtime::FacetStatus::Ready => "No matching entries.",
                    }))
                })
                .children(rows.into_iter().map(|row| {
                    let entry = row.id.clone();
                    let selected = browser.selected() == Some(&row.id);
                    let marker = if row.active_leaf {
                        "●"
                    } else if row.active_path {
                        "│"
                    } else if row.has_children && row.folded {
                        "▸"
                    } else if row.has_children {
                        "▾"
                    } else {
                        "·"
                    };
                    let label_copy = row
                        .label
                        .as_deref()
                        .map(|label| format!(" · {label}"))
                        .unwrap_or_default();
                    controls::interactive_list_row(
                        gpui::SharedString::from(format!("history-{}", row.id)),
                        true,
                        Box::new(cx.listener(move |view, _, window, cx| {
                            view.select_history(entry.clone(), window, cx)
                        })),
                        div()
                            .w_full()
                            .pl(px(6.0 + row.depth as f32 * 10.0))
                            .py(px(5.0))
                            .flex()
                            .gap(px(6.0))
                            .when(selected, |row| row.bg(theme::panel()))
                            .child(
                                div()
                                    .w(px(10.0))
                                    .font_family(theme::mono())
                                    .text_size(px(theme::T_TINY))
                                    .text_color(if row.active_path {
                                        theme::signal_hot()
                                    } else {
                                        theme::smoke()
                                    })
                                    .child(marker),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_size(px(theme::T_TINY))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(if row.contextual {
                                                theme::smoke()
                                            } else {
                                                theme::bone()
                                            })
                                            .child(format!("{}{}", row.title, label_copy)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(theme::T_TINY))
                                            .text_color(theme::bone_dim())
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child(row.detail),
                                    ),
                            ),
                    )
                })),
        )
        .when_some(details, |panel, details| {
            let body = details.body.chars().take(160).collect::<String>();
            panel.child(
                div()
                    .px(px(12.0))
                    .py(px(10.0))
                    .border_t_1()
                    .border_color(theme::edge_soft())
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .font_family(theme::mono())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::data())
                            .child(format!(
                                "{} · {} child{} · {}",
                                details.kind,
                                details.child_count,
                                if details.child_count == 1 { "" } else { "ren" },
                                details.timestamp
                            )),
                    )
                    .when(!body.is_empty(), |block| {
                        block.child(
                            div()
                                .text_size(px(theme::T_TINY))
                                .line_height(gpui::relative(1.4))
                                .text_color(theme::bone_dim())
                                .child(body),
                        )
                    }),
            )
        })
        .child(
            div()
                .px(px(10.0))
                .py(px(8.0))
                .border_t_1()
                .border_color(theme::edge_soft())
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(4.0))
                        .child(controls::quiet_button(
                            "history-fork",
                            "Fork",
                            ready && selected_is_forkable,
                            Box::new(cx.listener(|view, _, _, cx| view.request_fork(cx))),
                        ))
                        .child(controls::quiet_button(
                            "history-clone",
                            "Clone",
                            ready && projection.leaf_id.is_some(),
                            Box::new(cx.listener(|view, _, _, cx| view.request_clone(cx))),
                        ))
                        .when(navigation_available, |actions| {
                            actions.child(controls::quiet_button(
                                "history-navigate",
                                "Navigate",
                                ready
                                    && selected.is_some()
                                    && selected.as_ref() != projection.leaf_id.as_ref(),
                                Box::new(cx.listener(|view, _, _, cx| view.request_navigation(cx))),
                            ))
                        })
                        .when(export_available, |actions| {
                            actions.child(controls::quiet_button(
                                "history-export-jsonl",
                                "Export",
                                ready,
                                Box::new(cx.listener(|view, _, _, cx| view.export_jsonl(cx))),
                            ))
                        })
                        .when(bridge.pending.is_some(), |actions| {
                            actions.child(controls::quiet_button(
                                "history-cancel-bridge",
                                "Cancel",
                                true,
                                Box::new(cx.listener(|view, _, _, cx| view.cancel_bridge(cx))),
                            ))
                        }),
                )
                .when(navigation_available && summary_available, |block| {
                    block.child(controls::chip_button(
                        "history-summary",
                        "Branch summary",
                        summarize,
                        ready,
                        Box::new(cx.listener(|view, _, _, cx| view.toggle_navigation_summary(cx))),
                    ))
                }),
        )
        .when(labels_available && selected.is_some(), |panel| {
            panel.child(
                div()
                    .px(px(10.0))
                    .pb(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(controls::section_label("Label"))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_start()
                            .gap(px(6.0))
                            .child(div().flex_1().min_w_0().child(label.clone()))
                            .child(controls::quiet_button(
                                "history-clear-label",
                                "Clear",
                                ready,
                                Box::new(
                                    cx.listener(|view, _, _, cx| view.clear_selected_label(cx)),
                                ),
                            )),
                    ),
            )
        })
        .when(import_available, |panel| {
            panel.child(
                div()
                    .px(px(10.0))
                    .pb(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(controls::section_label("Import JSONL"))
                    .child(import_path.clone()),
            )
        })
        .when_some(confirmation.cloned(), |panel, confirmation| {
            let (title, copy) = match confirmation {
                HistoryConfirmation::Navigate(_) => (
                    "Navigate here?",
                    "Same file keeps every branch. Only the active leaf changes.",
                ),
                HistoryConfirmation::Fork(_) => (
                    "Fork before message?",
                    "Creates a new session file. Message text returns to the composer.",
                ),
                HistoryConfirmation::Clone => (
                    "Clone current path?",
                    "New file gets this path. Abandoned branches stay in the original.",
                ),
            };
            panel.child(
                div()
                    .mx(px(10.0))
                    .mb(px(8.0))
                    .px(px(10.0))
                    .py(px(10.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(theme::data_wash())
                    .border_1()
                    .border_color(theme::data())
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .font_family(theme::sans())
                            .text_size(px(theme::T_UI_SM))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::bone())
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(theme::T_TINY))
                            .line_height(gpui::relative(1.4))
                            .text_color(theme::bone_dim())
                            .child(copy),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(4.0))
                            .child(controls::quiet_button(
                                "history-confirm",
                                "Confirm",
                                ready,
                                Box::new(
                                    cx.listener(|view, _, _, cx| {
                                        view.confirm_history_operation(cx)
                                    }),
                                ),
                            ))
                            .child(controls::quiet_button(
                                "history-confirm-cancel",
                                "Cancel",
                                true,
                                Box::new(cx.listener(|view, _, _, cx| {
                                    view.cancel_history_confirmation(cx)
                                })),
                            )),
                    ),
            )
        })
        .when_some(bridge.feedback.clone(), |panel, feedback| {
            panel.child(controls::panel_footer_status(feedback))
        })
        .when_some(bridge.unavailable.clone(), |panel, unavailable| {
            panel.child(
                div()
                    .px(px(10.0))
                    .pb(px(10.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(controls::panel_note(
                        unavailable,
                        controls::ControlTone::Normal,
                    ))
                    .child(controls::quiet_button(
                        "history-restart-bridge",
                        "Restart bridge",
                        bridge.pending.is_none(),
                        Box::new(cx.listener(|view, _, _, cx| view.restart_bridge(cx))),
                    )),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::runtime::CompactionKind;

    fn runtime_status(
        lifecycle: RuntimeLifecycle,
        retry: RetryState,
        compaction: CompactionState,
    ) -> TitlebarStatus {
        titlebar_status_for("Ready", lifecycle, false, false, &retry, &compaction, None)
    }

    #[test]
    fn titlebar_distinguishes_working_finished_and_idle() {
        assert_eq!(
            runtime_status(
                RuntimeLifecycle::Running,
                RetryState::Idle,
                CompactionState::Idle,
            ),
            TitlebarStatus::new("Working", TitlebarStatusTone::Working, true)
        );
        assert_eq!(
            runtime_status(
                RuntimeLifecycle::Settled,
                RetryState::Idle,
                CompactionState::Idle,
            ),
            TitlebarStatus::new("Finished", TitlebarStatusTone::Complete, false)
        );
        assert_eq!(
            runtime_status(
                RuntimeLifecycle::Ready,
                RetryState::Idle,
                CompactionState::Idle,
            ),
            TitlebarStatus::new("Idle", TitlebarStatusTone::Idle, false)
        );
    }

    #[test]
    fn retry_attempt_overrides_generic_working_state() {
        let status = runtime_status(
            RuntimeLifecycle::Running,
            RetryState::Waiting {
                attempt: 2,
                max_attempts: 3,
                delay_ms: 500,
                started_at: Instant::now(),
            },
            CompactionState::Idle,
        );
        assert_eq!(
            status,
            TitlebarStatus::new("Retrying 2/3", TitlebarStatusTone::Attention, true)
        );
    }

    #[test]
    fn failures_and_disconnection_override_active_states() {
        let compaction_failure = runtime_status(
            RuntimeLifecycle::Settled,
            RetryState::Idle,
            CompactionState::Failed {
                reason: CompactionKind::Manual,
                summary: "provider refused compaction".to_owned(),
            },
        );
        assert_eq!(
            compaction_failure,
            TitlebarStatus::new("Compaction failed", TitlebarStatusTone::Error, false)
        );

        let disconnected = titlebar_status_for(
            "Connection error",
            RuntimeLifecycle::Disconnected,
            false,
            true,
            &RetryState::Waiting {
                attempt: 1,
                max_attempts: 3,
                delay_ms: 500,
                started_at: Instant::now(),
            },
            &CompactionState::Idle,
            None,
        );
        assert_eq!(
            disconnected,
            TitlebarStatus::new("Disconnected", TitlebarStatusTone::Error, false)
        );
    }

    #[test]
    fn current_work_overrides_stale_terminal_retry_state() {
        let status = runtime_status(
            RuntimeLifecycle::Running,
            RetryState::Failed {
                attempt: 3,
                summary: "old provider failure".to_owned(),
            },
            CompactionState::Idle,
        );
        assert_eq!(
            status,
            TitlebarStatus::new("Working", TitlebarStatusTone::Working, true)
        );
    }
}
