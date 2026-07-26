use gpui::{AnyElement, SharedString, svg};

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
    opening_thread: bool,
    name_composer: &Entity<Composer>,
    rename_open: bool,
    rename_enabled: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let action = projection.action;
    let status = titlebar_status(projection, conversation, opening_thread);
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
                .min_w(px(290.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(10.0))
                .flex_shrink_0()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .flex_shrink_0()
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
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .flex_shrink_0()
                        .child("|"),
                )
                .child(
                    div()
                        .id("theme-switcher")
                        .px(px(5.0))
                        .py(px(3.0))
                        .rounded(px(2.0))
                        .font_family(theme::main())
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::ash())
                        .whitespace_nowrap()
                        .flex_shrink_0()
                        .cursor_pointer()
                        .hover(|switcher| switcher.bg(theme::panel()).text_color(theme::bone()))
                        .active(|switcher| switcher.bg(theme::panel_lift()))
                        .tab_index(0)
                        .on_click(cx.listener(|view, _, window, cx| view.cycle_theme(window, cx)))
                        .child(theme::active().label()),
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
    opening_thread: bool,
) -> TitlebarStatus {
    titlebar_status_for(TitlebarStatusInput {
        shell_lifecycle: &projection.lifecycle,
        lifecycle: conversation.lifecycle,
        conversation_loading: matches!(
            conversation.status,
            crate::state::runtime::FacetStatus::Loading
        ),
        has_error: conversation.error.is_some(),
        opening_thread,
        retry: &conversation.retry,
        compaction: &conversation.compaction,
        pending_operation: conversation.pending_operation.as_ref(),
    })
}

struct TitlebarStatusInput<'a> {
    shell_lifecycle: &'a str,
    lifecycle: RuntimeLifecycle,
    conversation_loading: bool,
    has_error: bool,
    opening_thread: bool,
    retry: &'a RetryState,
    compaction: &'a CompactionState,
    pending_operation: Option<&'a RuntimeOperation>,
}

fn titlebar_status_for(input: TitlebarStatusInput<'_>) -> TitlebarStatus {
    let TitlebarStatusInput {
        shell_lifecycle,
        lifecycle,
        conversation_loading,
        has_error,
        opening_thread,
        retry,
        compaction,
        pending_operation,
    } = input;
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
    if opening_thread {
        return TitlebarStatus::new("Opening thread", TitlebarStatusTone::Working, true);
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
    pub(super) projects: &'a ProjectRegistry,
    pub(super) project_catalogs: &'a HashMap<String, ProjectCatalogCache>,
    pub(super) project_feedback: Option<&'a str>,
    pub(super) project_picker_pending: bool,
    pub(super) project_switch_enabled: bool,
    pub(super) conversation: &'a ConversationProjection,
    pub(super) history_open: bool,
    pub(super) menu_open: bool,
    pub(super) scroll: &'a ScrollHandle,
}

pub(super) fn sessions_panel(
    params: SessionsPanelParams<'_>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let SessionsPanelParams {
        catalog,
        projects,
        project_catalogs,
        project_feedback,
        project_picker_pending,
        project_switch_enabled,
        conversation,
        history_open,
        menu_open,
        scroll,
    } = params;
    let wheel_root = cx.entity();
    let session_actions_enabled = matches!(
        conversation.lifecycle,
        RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
    ) && conversation.pending_operation.is_none()
        && !catalog.switching;
    let current_path = catalog.current_session_file.as_ref();
    let pending_path = catalog.pending_session_file.as_ref();
    let project_count = projects.projects().len().to_string();

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
                .child(controls::section_label("Projects"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(1.0))
                        .child(
                            div()
                                .min_w(px(18.0))
                                .font_family(theme::mono())
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .text_align(gpui::TextAlign::Right)
                                .child(project_count),
                        )
                        .child(controls::quiet_button(
                            "add-project",
                            "Add",
                            !project_picker_pending,
                            Box::new(cx.listener(|view, _, _, cx| view.choose_projects(cx))),
                        ))
                        .child(controls::icon_button(
                            "refresh-projects",
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
                            "New thread",
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
        .when_some(
            project_feedback.map(ToOwned::to_owned),
            |panel, feedback| {
                panel.child(
                    div()
                        .mx(px(12.0))
                        .mb(px(8.0))
                        .px(px(9.0))
                        .py(px(7.0))
                        .rounded(px(theme::RADIUS_SM))
                        .bg(theme::panel())
                        .font_family(theme::sans())
                        .text_size(px(theme::T_TINY))
                        .line_height(gpui::relative(1.35))
                        .text_color(theme::bone_dim())
                        .child(feedback),
                )
            },
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    canvas(
                        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                        move |_, hitbox, window, _| {
                            window.on_mouse_event(
                                move |event: &ScrollWheelEvent, phase, window, cx| {
                                    if phase != DispatchPhase::Capture
                                        || !hitbox.should_handle_scroll(window)
                                    {
                                        return;
                                    }
                                    let handled = wheel_root.update(cx, |view, cx| {
                                        view.on_sessions_scroll_wheel(event, window, cx)
                                    });
                                    if handled {
                                        cx.stop_propagation();
                                    }
                                },
                            );
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    div()
                        .id("sessions-scroll")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .scrollbar_width(px(theme::SCROLLBAR))
                        .w_full()
                        .flex()
                        .flex_col()
                        .children(projects.projects().iter().map(|project| {
                            project_group(
                                ProjectGroupParams {
                                    project,
                                    active: projects.is_active(&project.path),
                                    active_catalog: catalog,
                                    cached_catalog: project_catalogs
                                        .get(&project_key(&project.path)),
                                    current_path,
                                    pending_path,
                                    session_actions_enabled,
                                    project_switch_enabled,
                                    can_remove: projects.projects().len() > 1,
                                },
                                cx,
                            )
                        })),
                ),
        )
        .child(
            div()
                .px(px(12.0))
                .py(px(10.0))
                .border_t_1()
                .border_color(theme::edge_soft())
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .child(controls::section_label("Active project"))
                        .child(controls::quiet_button(
                            "remove-active-project",
                            "Remove",
                            projects.projects().len() > 1 && project_switch_enabled,
                            {
                                let path = projects.active_path().to_path_buf();
                                Box::new(cx.listener(move |view, _, window, cx| {
                                    view.remove_project(path.clone(), window, cx)
                                }))
                            },
                        )),
                )
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .line_height(gpui::relative(1.35))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::data())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(short_path(&projects.active_path().to_string_lossy())),
                ),
        )
}

struct ProjectGroupParams<'a> {
    project: &'a ProjectEntry,
    active: bool,
    active_catalog: &'a CatalogProjection,
    cached_catalog: Option<&'a ProjectCatalogCache>,
    current_path: Option<&'a PathBuf>,
    pending_path: Option<&'a PathBuf>,
    session_actions_enabled: bool,
    project_switch_enabled: bool,
    can_remove: bool,
}

fn project_group(params: ProjectGroupParams<'_>, cx: &mut Context<RootView>) -> AnyElement {
    let ProjectGroupParams {
        project,
        active,
        active_catalog,
        cached_catalog,
        current_path,
        pending_path,
        session_actions_enabled,
        project_switch_enabled,
        can_remove,
    } = params;
    let cached_while_loading = cached_catalog.filter(|catalog| {
        active
            && active_catalog.status == CatalogStatus::Loading
            && active_catalog.sessions.is_empty()
            && !catalog.sessions.is_empty()
    });
    let (status, sessions, corrupt_count, error) = if let Some(catalog) = cached_while_loading {
        (
            CatalogStatus::Loading,
            Arc::clone(&catalog.sessions),
            catalog.corrupt_count,
            None,
        )
    } else if active {
        (
            active_catalog.status,
            Arc::clone(&active_catalog.sessions),
            active_catalog.corrupt.len(),
            active_catalog.error.clone(),
        )
    } else if let Some(catalog) = cached_catalog {
        (
            catalog.status,
            Arc::clone(&catalog.sessions),
            catalog.corrupt_count,
            catalog.error.clone(),
        )
    } else {
        (CatalogStatus::Loading, Arc::new(Vec::new()), 0, None)
    };
    let count = match status {
        CatalogStatus::Inaccessible => "!".to_owned(),
        CatalogStatus::Stale => format!("{}!", sessions.len()),
        CatalogStatus::Loading | CatalogStatus::Ready | CatalogStatus::Empty => {
            sessions.len().to_string()
        }
    };
    let path = project.path.clone();
    let click_path = path.clone();
    let key_path = path.clone();
    let left_path = path.clone();
    let right_path = path.clone();
    let toggle_path = path.clone();
    let click_root = cx.entity();
    let key_root = click_root.clone();
    let left_root = click_root.clone();
    let right_root = click_root.clone();
    let toggle_root = click_root.clone();
    let expanded = project.expanded;
    let activity_key = list_animation_key(&project_key(&path));

    div()
        .w_full()
        .flex()
        .flex_col()
        .mb(px(3.0))
        .child(
            div()
                .id(SharedString::from(format!(
                    "project-{}",
                    project_key(&path)
                )))
                .h(px(36.0))
                .px(px(10.0))
                .tab_index(0)
                .cursor_pointer()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(7.0))
                .bg(if active {
                    theme::panel_lift()
                } else {
                    gpui::rgba(0x0000_0000)
                })
                .hover(|row| row.bg(theme::panel()))
                .focus(|row| row.bg(theme::panel_lift()).text_color(theme::focus()))
                .on_click(move |_, window, cx| {
                    click_root.update(cx, |view, cx| {
                        view.activate_project(click_path.clone(), None, window, cx)
                    });
                })
                .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "enter" | "space" => {
                            cx.stop_propagation();
                            key_root.update(cx, |view, cx| {
                                view.activate_project(key_path.clone(), None, window, cx)
                            });
                        }
                        "left" if expanded => {
                            cx.stop_propagation();
                            left_root.update(cx, |view, cx| {
                                view.set_project_expanded(left_path.clone(), false, cx)
                            });
                        }
                        "right" if !expanded => {
                            cx.stop_propagation();
                            right_root.update(cx, |view, cx| {
                                view.set_project_expanded(right_path.clone(), true, cx)
                            });
                        }
                        _ => {}
                    }
                })
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "toggle-project-{}",
                            project_key(&path)
                        )))
                        .ml(px(-4.0))
                        .size(px(22.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(theme::RADIUS_SM))
                        .hover(|button| button.bg(theme::panel_hover()))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            toggle_root.update(cx, |view, cx| {
                                view.toggle_project(toggle_path.clone(), cx)
                            });
                        })
                        .child(
                            svg()
                                .path(if expanded {
                                    "icons/chevron-down.svg"
                                } else {
                                    "icons/chevron-right.svg"
                                })
                                .size(px(10.0))
                                .text_color(theme::smoke()),
                        ),
                )
                .child(
                    svg()
                        .path("icons/folder.svg")
                        .size(px(14.0))
                        .flex_shrink_0()
                        .text_color(if active { theme::data() } else { theme::ash() }),
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
                        .font_weight(if active {
                            FontWeight::BOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if active {
                            theme::bone()
                        } else {
                            theme::bone_dim()
                        })
                        .child(project.name()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(7.0))
                        .when(
                            status == CatalogStatus::Loading && !sessions.is_empty(),
                            |meta| {
                                meta.child(controls::square_status_indicator(
                                    activity_key,
                                    true,
                                    Duration::from_millis(900),
                                    theme::working(),
                                ))
                            },
                        )
                        .child(
                            div()
                                .font_family(theme::mono())
                                .text_size(px(theme::T_TINY))
                                .text_color(
                                    if matches!(
                                        status,
                                        CatalogStatus::Inaccessible | CatalogStatus::Stale
                                    ) {
                                        theme::error()
                                    } else {
                                        theme::smoke()
                                    },
                                )
                                .child(count),
                        ),
                ),
        )
        .when(project.expanded, |group| {
            group
                .when(sessions.is_empty(), |group| match status {
                    CatalogStatus::Loading => {
                        group.child(project_tree_loading_note(activity_key, "Scanning threads"))
                    }
                    CatalogStatus::Empty | CatalogStatus::Ready => {
                        group.child(project_tree_note("No saved threads yet."))
                    }
                    CatalogStatus::Inaccessible | CatalogStatus::Stale => {
                        group.child(project_tree_note("Project threads are unavailable."))
                    }
                })
                .children(sessions.iter().map(|session| {
                    project_thread_row(
                        project.path.clone(),
                        session,
                        active,
                        active
                            && pending_path
                                .or(current_path)
                                .is_some_and(|path| sidebar_paths_match(path, &session.path)),
                        active
                            && pending_path
                                .is_some_and(|path| sidebar_paths_match(path, &session.path)),
                        if active {
                            session_actions_enabled
                        } else {
                            project_switch_enabled
                        },
                        cx,
                    )
                }))
                .when_some(error, |group, error| {
                    let remove_path = project.path.clone();
                    group.child(
                        div()
                            .pl(px(34.0))
                            .pr(px(8.0))
                            .pb(px(6.0))
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .font_family(theme::sans())
                                    .text_size(px(theme::T_TINY))
                                    .line_height(gpui::relative(1.35))
                                    .text_color(theme::error())
                                    .child(error),
                            )
                            .when(!active, |note| {
                                note.child(controls::quiet_button(
                                    SharedString::from(format!(
                                        "remove-project-{}",
                                        project_key(&remove_path)
                                    )),
                                    "Remove from sidebar",
                                    can_remove,
                                    Box::new(cx.listener(move |view, _, window, cx| {
                                        view.remove_project(remove_path.clone(), window, cx)
                                    })),
                                ))
                            }),
                    )
                })
                .when(corrupt_count > 0, |group| {
                    group.child(project_tree_note(format!(
                        "{corrupt_count} corrupt thread{} skipped.",
                        if corrupt_count == 1 { "" } else { "s" }
                    )))
                })
        })
        .into_any_element()
}

fn project_thread_row(
    project_path: PathBuf,
    session: &SessionSummary,
    active_project: bool,
    selected: bool,
    switching: bool,
    enabled: bool,
    cx: &mut Context<RootView>,
) -> AnyElement {
    let session_path = session.path.clone();
    let click_project = project_path.clone();
    let click_session = session_path.clone();
    let key_project = project_path;
    let key_session = session_path;
    let click_root = cx.entity();
    let key_root = click_root.clone();
    let title = session
        .name
        .clone()
        .or_else(|| session.first_user_summary.clone())
        .unwrap_or_else(|| "Untitled thread".to_owned());
    let activity_key = list_animation_key(&session.id);
    let message_count = match session.counts.messages {
        1 => "1 message".to_owned(),
        count => format!("{count} messages"),
    };
    let status = if switching {
        "Opening thread".to_owned()
    } else if selected {
        "Current thread".to_owned()
    } else {
        message_count
    };
    let updated = compact_session_timestamp(&session.updated_at);

    div()
        .id(SharedString::from(format!(
            "project-thread-{}-{}",
            project_key(&click_project),
            session.id
        )))
        .min_h(px(52.0))
        .pl(px(34.0))
        .pr(px(10.0))
        .py(px(7.0))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(4.0))
        .bg(if selected {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .when(enabled && !selected, |row| {
            row.tab_index(0)
                .cursor_pointer()
                .hover(|row| row.bg(theme::panel()))
                .active(|row| row.bg(theme::panel_hover()))
                .focus(|row| row.bg(theme::panel_lift()).text_color(theme::focus()))
                .on_click(move |_, window, cx| {
                    click_root.update(cx, |view, cx| {
                        if active_project {
                            view.switch_session(click_session.clone(), cx);
                        } else {
                            view.activate_project(
                                click_project.clone(),
                                Some(click_session.clone()),
                                window,
                                cx,
                            );
                        }
                    });
                })
                .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        key_root.update(cx, |view, cx| {
                            if active_project {
                                view.switch_session(key_session.clone(), cx);
                            } else {
                                view.activate_project(
                                    key_project.clone(),
                                    Some(key_session.clone()),
                                    window,
                                    cx,
                                );
                            }
                        });
                    }
                })
        })
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
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
                            FontWeight::BOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if selected {
                            theme::bone()
                        } else if enabled {
                            theme::bone_dim()
                        } else {
                            theme::ash()
                        })
                        .child(title),
                )
                .when(switching, |title| {
                    title.child(controls::square_status_indicator(
                        activity_key,
                        true,
                        Duration::from_millis(720),
                        theme::working(),
                    ))
                }),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .font_family(theme::mono())
                .text_size(px(theme::T_TINY))
                .text_color(if switching {
                    theme::working()
                } else if selected {
                    theme::data()
                } else {
                    theme::smoke()
                })
                .child(div().whitespace_nowrap().child(status))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(updated),
                ),
        )
        .into_any_element()
}

fn sidebar_paths_match(left: &std::path::Path, right: &std::path::Path) -> bool {
    project_key(left) == project_key(right)
}

fn list_animation_key(value: &str) -> usize {
    value.bytes().fold(1_009_usize, |key, byte| {
        key.wrapping_mul(131).wrapping_add(byte as usize)
    })
}

fn compact_session_timestamp(timestamp: &str) -> String {
    let bytes = timestamp.as_bytes();
    let iso_shape = bytes.len() >= 16
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && matches!(bytes.get(10), Some(b'T' | b' '))
        && bytes.get(13) == Some(&b':');
    if !iso_shape {
        return timestamp.to_owned();
    }

    let Some(month_digits) = timestamp.get(5..7) else {
        return timestamp.to_owned();
    };
    let month = match month_digits {
        "01" => "Jan",
        "02" => "Feb",
        "03" => "Mar",
        "04" => "Apr",
        "05" => "May",
        "06" => "Jun",
        "07" => "Jul",
        "08" => "Aug",
        "09" => "Sep",
        "10" => "Oct",
        "11" => "Nov",
        "12" => "Dec",
        _ => return timestamp.to_owned(),
    };
    let (Some(day), Some(time)) = (timestamp.get(8..10), timestamp.get(11..16)) else {
        return timestamp.to_owned();
    };
    let day = day.trim_start_matches('0');
    if day.is_empty() {
        return timestamp.to_owned();
    }
    format!("{month} {day}, {time}")
}

fn project_tree_loading_note(animation_key: usize, text: &'static str) -> AnyElement {
    div()
        .pl(px(34.0))
        .pr(px(10.0))
        .py(px(8.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .font_family(theme::sans())
        .text_size(px(theme::T_TINY))
        .text_color(theme::smoke())
        .child(controls::square_status_indicator(
            animation_key.wrapping_add(1),
            true,
            Duration::from_millis(900),
            theme::working(),
        ))
        .child(text)
        .into_any_element()
}

fn project_tree_note(text: impl Into<SharedString>) -> AnyElement {
    div()
        .pl(px(34.0))
        .pr(px(10.0))
        .py(px(7.0))
        .font_family(theme::sans())
        .text_size(px(theme::T_TINY))
        .line_height(gpui::relative(1.35))
        .text_color(theme::smoke())
        .child(text.into())
        .into_any_element()
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
        titlebar_status_for(TitlebarStatusInput {
            shell_lifecycle: "Ready",
            lifecycle,
            conversation_loading: false,
            has_error: false,
            opening_thread: false,
            retry: &retry,
            compaction: &compaction,
            pending_operation: None,
        })
    }

    #[test]
    fn opening_thread_has_specific_animated_feedback() {
        let status = titlebar_status_for(TitlebarStatusInput {
            shell_lifecycle: "Ready",
            lifecycle: RuntimeLifecycle::Loading,
            conversation_loading: true,
            has_error: false,
            opening_thread: true,
            retry: &RetryState::Idle,
            compaction: &CompactionState::Idle,
            pending_operation: None,
        });
        assert_eq!(
            status,
            TitlebarStatus::new("Opening thread", TitlebarStatusTone::Working, true)
        );
    }

    #[test]
    fn session_timestamp_is_compact_without_losing_unknown_formats() {
        assert_eq!(
            compact_session_timestamp("2026-07-26T14:08:51.000Z"),
            "Jul 26, 14:08"
        );
        assert_eq!(compact_session_timestamp("recently"), "recently");
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

        let disconnected = titlebar_status_for(TitlebarStatusInput {
            shell_lifecycle: "Connection error",
            lifecycle: RuntimeLifecycle::Disconnected,
            conversation_loading: false,
            has_error: true,
            opening_thread: false,
            retry: &RetryState::Waiting {
                attempt: 1,
                max_attempts: 3,
                delay_ms: 500,
                started_at: Instant::now(),
            },
            compaction: &CompactionState::Idle,
            pending_operation: None,
        });
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
