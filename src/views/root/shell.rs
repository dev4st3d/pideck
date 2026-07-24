use super::shared::{action_id, lifecycle_color, short_path};
use super::*;

pub(super) fn titlebar(
    projection: &ShellProjection,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let action = projection.action;
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
                        .font_family(theme::DISPLAY)
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
                        .min_w_0()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TITLE))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::bone())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(projection.session.label()),
                )
                .when(projection.has_stale_values, |row| {
                    row.child(
                        div()
                            .font_family(theme::MONO)
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
                .child(controls::meta_text(projection.cost.label()))
                .child(controls::meta_sep())
                .child(controls::status_pill(
                    projection.lifecycle.clone(),
                    lifecycle_color(projection),
                ))
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

pub(super) struct SessionsPanelParams<'a> {
    pub(super) catalog: &'a CatalogProjection,
    pub(super) projection: &'a ShellProjection,
    pub(super) conversation: &'a ConversationProjection,
    pub(super) name_composer: &'a Entity<Composer>,
    pub(super) history_open: bool,
}

pub(super) fn sessions_panel(
    params: SessionsPanelParams<'_>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let SessionsPanelParams {
        catalog,
        projection,
        conversation,
        name_composer,
        history_open,
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
        .bg(theme::floor())
        .border_r_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .px(px(14.0))
                .pt(px(14.0))
                .pb(px(10.0))
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Sessions"))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(state_copy),
                ),
        )
        .child(
            div()
                .px(px(10.0))
                .pb(px(8.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .child(controls::quiet_button(
                            "new-session",
                            "New",
                            session_actions_enabled,
                            Box::new(cx.listener(|view, _, window, cx| {
                                let _ = view.execute_native_action(
                                    NativeAction::NewSession,
                                    "",
                                    window,
                                    cx,
                                );
                            })),
                        ))
                        .child(controls::quiet_button(
                            "refresh-sessions",
                            "Refresh",
                            catalog.status != CatalogStatus::Loading,
                            Box::new(cx.listener(|view, _, _, cx| view.refresh_sessions(cx))),
                        ))
                        .child(controls::quiet_button(
                            "export-session",
                            "Export",
                            session_actions_enabled && catalog.current_session_file.is_some(),
                            Box::new(cx.listener(|view, _, window, cx| {
                                let _ = view.execute_native_action(
                                    NativeAction::ExportHtml,
                                    "",
                                    window,
                                    cx,
                                );
                            })),
                        )),
                )
                .child(controls::chip_button(
                    "toggle-history",
                    "History",
                    history_open,
                    true,
                    Box::new(cx.listener(|view, _, window, cx| {
                        let _ = view.execute_native_action(NativeAction::Tree, "", window, cx);
                    })),
                )),
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
                        .font_family(theme::SANS)
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
                    .font_family(theme::SANS)
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
        .when(catalog.current_session_file.is_some(), |panel| {
            panel.child(div().px(px(10.0)).pb(px(8.0)).child(name_composer.clone()))
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
                        .font_family(theme::MONO)
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
                        .font_family(theme::MONO)
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
                                    .font_family(theme::MONO)
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
                            .font_family(theme::MONO)
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
                            .font_family(theme::SANS)
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
