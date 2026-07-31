use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, SharedString, deferred, ease_out_quint, relative, svg,
};

use super::shared::{action_id, runtime_operation_label, short_path};
use super::*;

/// Subtle open/close duration; keep under 300ms per GPUI motion guidance.
const SIDEBAR_MOTION_MS: u64 = 220;

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

pub(super) struct TitlebarParams<'a> {
    pub(super) projection: &'a ShellProjection,
    pub(super) conversation: &'a ConversationProjection,
    pub(super) opening_thread: bool,
    pub(super) name_composer: &'a Entity<Composer>,
    pub(super) rename_open: bool,
    pub(super) rename_enabled: bool,
    pub(super) theme_menu_open: bool,
    pub(super) sidebar_open: bool,
    pub(super) terminal_open: bool,
    pub(super) inspector_open: bool,
    pub(super) app_update: &'a PiDeckUpdateState,
}

pub(super) fn titlebar(params: TitlebarParams<'_>, cx: &mut Context<RootView>) -> impl IntoElement {
    let TitlebarParams {
        projection,
        conversation,
        opening_thread,
        name_composer,
        rename_open,
        rename_enabled,
        theme_menu_open,
        sidebar_open,
        terminal_open,
        inspector_open,
        app_update,
    } = params;
    let action = projection.action;
    let status = titlebar_status(projection, conversation, opening_thread);
    let status_color = status.color();
    let active_theme = theme::active();
    let update_version = app_update.available_version().map(str::to_owned);
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
                .child(sidebar_toggle_button(sidebar_open, cx))
                .child(
                    // One unit with the row: same 28px chrome height as sidebar /
                    // rename. No baseline tricks — keep πdeck locked together and
                    // flex-center it with the neighboring controls.
                    div()
                        .h(px(28.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(1.0))
                        .flex_shrink_0()
                        .font_family(theme::main())
                        // Font metrics hang low vs the 28px icon boxes; lift the whole mark.
                        .mt(px(-2.0))
                        .child(
                            div()
                                .text_size(theme::text_size(19.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::live())
                                .line_height(relative(1.0))
                                .child("π"),
                        )
                        .child(
                            div()
                                .text_size(theme::text_size(theme::T_WORDMARK))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(theme::bone())
                                .line_height(relative(1.0))
                                .child("deck"),
                        ),
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
                                .text_size(theme::text_size(theme::T_TITLE))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::bone())
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(projection.session.label()),
                        )
                        .child(session_rename_button(rename_open, rename_enabled, cx))
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
                            .text_size(theme::text_size(theme::T_TINY))
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
                .when_some(update_version, |row, version| {
                    row.child(update_notice_button(version, cx))
                })
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
                                .text_size(theme::text_size(theme::T_UI_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(status_color)
                                .whitespace_nowrap()
                                .child(status.label),
                        ),
                )
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(theme::smoke())
                        .flex_shrink_0()
                        .child("|"),
                )
                .child(terminal_toggle_button(terminal_open, cx))
                .child(inspector_toggle_button(inspector_open, cx))
                .child(
                    div()
                        .relative()
                        .flex_shrink_0()
                        .child(
                            // Height/padding/type match `compact_select` and sit
                            // flush with the 28px titlebar icon toggles beside it.
                            div()
                                .id("theme-switcher")
                                .h(px(28.0))
                                .px(px(8.0))
                                .rounded(px(theme::RADIUS_SM))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .bg(if theme_menu_open {
                                    theme::panel_lift()
                                } else {
                                    theme::panel()
                                })
                                .border_1()
                                .border_color(if theme_menu_open {
                                    theme::edge_hard()
                                } else {
                                    theme::panel()
                                })
                                .font_family(theme::main())
                                .text_size(theme::text_size(theme::T_LABEL))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if theme_menu_open {
                                    theme::bone()
                                } else {
                                    theme::bone_dim()
                                })
                                .whitespace_nowrap()
                                .cursor_pointer()
                                .hover(|switcher| {
                                    switcher
                                        .bg(theme::panel_lift())
                                        .border_color(theme::panel_lift())
                                        .text_color(theme::bone())
                                })
                                .active(|switcher| switcher.bg(theme::panel_hover()))
                                .tab_index(0)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.toggle_theme_menu(window, cx)
                                }))
                                .child(
                                    svg()
                                        .path(match active_theme.mode() {
                                            theme::ThemeMode::Dark => "icons/moon.svg",
                                            theme::ThemeMode::Light => "icons/sun.svg",
                                        })
                                        .size(px(13.0))
                                        .flex_shrink_0()
                                        .text_color(
                                            if active_theme.mode() == theme::ThemeMode::Light {
                                                theme::data()
                                            } else {
                                                theme::smoke()
                                            },
                                        ),
                                )
                                .child(active_theme.label())
                                .child(
                                    svg()
                                        .path(if theme_menu_open {
                                            "icons/chevron-up.svg"
                                        } else {
                                            "icons/chevron-down.svg"
                                        })
                                        .size(px(12.0))
                                        .text_color(if theme_menu_open {
                                            theme::data()
                                        } else {
                                            theme::smoke()
                                        })
                                        .flex_shrink_0(),
                                ),
                        )
                        .when(theme_menu_open, |host| {
                            host.child(deferred(
                                div()
                                    .id("theme-select-host")
                                    .absolute()
                                    .top_full()
                                    .right_0()
                                    .mt(px(6.0))
                                    .w(px(196.0))
                                    .occlude()
                                    .child(theme_select_sheet(active_theme, cx)),
                            ))
                        }),
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

fn update_notice_button(version: String, cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .id("app-update-notice")
        .h(px(28.0))
        .px(px(9.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(theme::panel_lift())
        .border_1()
        .border_color(theme::edge_hard())
        .font_family(theme::main())
        .text_size(theme::text_size(theme::T_UI_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::data())
        .whitespace_nowrap()
        .tab_index(0)
        .key_context(APP_UPDATE_NOTICE_CONTEXT)
        .cursor_pointer()
        .hover(|button| button.bg(theme::panel_hover()).text_color(theme::bone()))
        // Match the App settings update button: accent border on focus, no
        // label recolor, press lifts instead of dimming.
        .focus(|button| button.border_color(theme::focus()))
        .active(|button| button.bg(theme::panel_hover()))
        .on_click(cx.listener(|view, _, window, cx| view.open_app_updates(window, cx)))
        .child(format!("Update {version}"))
}

fn theme_select_sheet(active: theme::ThemeId, cx: &mut Context<RootView>) -> impl IntoElement {
    use super::shared::popup_sheet;

    popup_sheet()
        .id("theme-select-sheet")
        .child(
            div()
                .h(px(28.0))
                .px(px(8.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .bg(theme::panel())
                .border_b_1()
                .border_color(theme::panel_hover())
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::ash())
                        .child("Theme"),
                )
                .child(controls::chrome_action(
                    "close-theme-select",
                    "Close",
                    true,
                    Box::new(cx.listener(|view, _, window, cx| view.close_theme_menu(window, cx))),
                )),
        )
        .child(theme_select_section(
            theme::ThemeMode::Dark,
            theme::ThemeId::for_mode(theme::ThemeMode::Dark),
            active,
            cx,
        ))
        .child(theme_select_section(
            theme::ThemeMode::Light,
            theme::ThemeId::for_mode(theme::ThemeMode::Light),
            active,
            cx,
        ))
}

fn theme_select_section(
    mode: theme::ThemeMode,
    themes: &'static [theme::ThemeId],
    active: theme::ThemeId,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(24.0))
                .px(px(8.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .bg(theme::canvas())
                .border_b_1()
                .border_color(theme::panel_hover())
                .child(
                    svg()
                        .path(match mode {
                            theme::ThemeMode::Dark => "icons/moon.svg",
                            theme::ThemeMode::Light => "icons/sun.svg",
                        })
                        .size(px(11.0))
                        .flex_shrink_0()
                        .text_color(theme::smoke()),
                )
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::smoke())
                        .child(mode.label()),
                ),
        )
        .children(themes.iter().copied().map(|theme_id| {
            let selected = theme_id == active;
            div()
                .id(SharedString::from(format!("theme-select-{theme_id:?}")))
                .h(px(28.0))
                .px(px(8.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .border_b_1()
                .border_color(theme::panel_hover())
                .bg(if selected {
                    theme::panel_lift()
                } else {
                    theme::panel()
                })
                .text_color(if selected {
                    theme::bone()
                } else {
                    theme::ash()
                })
                .when(!selected, |row| {
                    row.tab_index(0)
                        .cursor_pointer()
                        .hover(|row| row.bg(theme::panel_lift()).text_color(theme::bone()))
                        .active(|row| row.bg(theme::panel_hover()))
                        .focus(|row| row.bg(theme::panel_lift()))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.set_theme(theme_id, window, cx)
                        }))
                })
                .child(
                    div()
                        .font_family(theme::main())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .child(theme_id.label()),
                )
                .when(selected, |row| {
                    row.child(
                        div()
                            .w(px(5.0))
                            .h(px(5.0))
                            .rounded_full()
                            .bg(theme::data())
                            .flex_shrink_0(),
                    )
                })
        }))
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
    pub(super) thread_statuses: &'a HashMap<String, ThreadRuntimeStatus>,
    pub(super) hovered_thread_key: Option<&'a str>,
    pub(super) project_feedback: Option<&'a str>,
    pub(super) project_picker_pending: bool,
    pub(super) project_switch_enabled: bool,
    pub(super) conversation: &'a ConversationProjection,
    pub(super) history_open: bool,
    pub(super) sidebar_open: bool,
    pub(super) sidebar_motion_key: u64,
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
        thread_statuses,
        hovered_thread_key,
        project_feedback,
        project_picker_pending,
        project_switch_enabled,
        conversation,
        history_open,
        sidebar_open,
        sidebar_motion_key,
        scroll,
    } = params;
    let wheel_root = cx.entity();
    let session_actions_enabled = matches!(
        conversation.lifecycle,
        RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
    ) && conversation.pending_operation.is_none()
        && !catalog.switching;
    let new_thread_enabled = projects.active_path().is_dir();
    let current_path = catalog.current_session_file.as_ref();
    let pending_path = catalog.pending_session_file.as_ref();
    let project_count = projects.projects().len();
    let active_path = projects.active_path().to_path_buf();
    let can_remove_active = project_count > 1 && project_switch_enabled;
    let export_enabled = session_actions_enabled && catalog.current_session_file.is_some();
    const SIDE_PAD: f32 = 12.0;

    let expanded_w = theme::SIDE_W;
    let target_w = if sidebar_open { expanded_w } else { 0.0 };

    // Fixed-width body so collapse clips instead of reflowing labels mid-transition.
    let body = div()
        .id("sessions-panel-body")
        .w(px(expanded_w))
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
                .px(px(SIDE_PAD))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap(px(8.0))
                        .child(controls::section_label("Workspace"))
                        .child(
                            div()
                                .font_family(theme::mono())
                                .text_size(theme::text_size(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(project_count.to_string()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(controls::icon_button(
                            "add-project",
                            "+",
                            false,
                            !project_picker_pending,
                            Box::new(cx.listener(|view, _, _, cx| view.choose_projects(cx))),
                        ))
                        .child(controls::icon_button(
                            "refresh-projects",
                            "↻",
                            false,
                            catalog.status != CatalogStatus::Loading,
                            Box::new(cx.listener(|view, _, _, cx| view.refresh_sessions(cx))),
                        ))
                        .child(sidebar_collapse_icon_button(cx)),
                ),
        )
        .child(
            div()
                .px(px(SIDE_PAD))
                .pt(px(10.0))
                .pb(px(10.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(sidebar_new_thread_button(new_thread_enabled, cx))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(sidebar_secondary_button(
                            "toggle-history-inline",
                            "History",
                            history_open,
                            true,
                            Box::new(cx.listener(|view, _, window, cx| {
                                let _ =
                                    view.execute_native_action(NativeAction::Tree, "", window, cx);
                            })),
                        ))
                        .child(sidebar_secondary_button(
                            "export-session",
                            "Export",
                            false,
                            export_enabled,
                            Box::new(
                                cx.listener(|view, _, window, cx| view.export_session(window, cx)),
                            ),
                        )),
                ),
        )
        .when_some(
            project_feedback.map(ToOwned::to_owned),
            |panel, feedback| {
                panel.child(
                    div()
                        .mx(px(SIDE_PAD))
                        .mt(px(8.0))
                        .px(px(9.0))
                        .py(px(7.0))
                        .rounded(px(theme::RADIUS_SM))
                        .bg(theme::panel())
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
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
                        .pt(px(6.0))
                        .pb(px(8.0))
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
                                    project_switch_enabled,
                                    thread_statuses,
                                    hovered_thread_key,
                                    can_remove: projects.projects().len() > 1,
                                },
                                cx,
                            )
                        })),
                ),
        )
        .child(
            div()
                .h(px(40.0))
                .px(px(SIDE_PAD))
                .border_t_1()
                .border_color(theme::edge_soft())
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    svg()
                        .path("icons/folder.svg")
                        .size(px(12.0))
                        .flex_shrink_0()
                        .text_color(theme::data()),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::data())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(short_path(&active_path.to_string_lossy())),
                )
                .child(controls::quiet_button(
                    "remove-active-project",
                    "Remove",
                    can_remove_active,
                    {
                        let path = active_path;
                        Box::new(cx.listener(move |view, _, window, cx| {
                            view.remove_project(path.clone(), window, cx)
                        }))
                    },
                )),
        );

    let shell = div()
        .id("sessions-panel")
        .h_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(body);

    if sidebar_motion_key == 0 {
        shell.w(px(target_w)).into_any_element()
    } else {
        let open = sidebar_open;
        shell
            .with_animation(
                ("sessions-sidebar", sidebar_motion_key),
                Animation::new(Duration::from_millis(SIDEBAR_MOTION_MS))
                    .with_easing(ease_out_quint()),
                move |panel, delta| {
                    let (from, to) = if open {
                        (0.0, expanded_w)
                    } else {
                        (expanded_w, 0.0)
                    };
                    // Soft opacity with width so the rail doesn't hard-cut mid-slide.
                    let fade = if open {
                        0.55 + 0.45 * delta
                    } else {
                        1.0 - 0.45 * delta
                    };
                    panel.w(px(from + (to - from) * delta)).opacity(fade)
                },
            )
            .into_any_element()
    }
}

fn session_rename_button(
    open: bool,
    enabled: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let icon_color = if !enabled {
        theme::smoke()
    } else if open {
        theme::data()
    } else {
        theme::bone_dim()
    };
    div()
        .id("rename-session")
        .size(px(28.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if open {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .border_1()
        .border_color(if open {
            theme::edge_hard()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .text_color(icon_color)
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel()).text_color(theme::bone()))
                .active(|button| button.bg(theme::panel_lift()))
                .on_click(cx.listener(|view, _, window, cx| view.toggle_session_rename(window, cx)))
        })
        .child(
            svg()
                .path("icons/pencil.svg")
                .size(px(14.0))
                .text_color(icon_color),
        )
}

fn sidebar_toggle_button(open: bool, cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .id("toggle-sidebar")
        .size(px(28.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if open {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .border_1()
        .border_color(if open {
            theme::edge_hard()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .text_color(theme::bone_dim())
        .tab_index(0)
        .cursor_pointer()
        .hover(|button| button.bg(theme::panel()).text_color(theme::bone()))
        .active(|button| button.bg(theme::panel_lift()))
        .on_click(cx.listener(|view, _, window, cx| view.toggle_sidebar(window, cx)))
        .child(
            svg()
                .path("icons/sidebar.svg")
                .size(px(14.0))
                .text_color(if open {
                    theme::data()
                } else {
                    theme::bone_dim()
                }),
        )
}

fn terminal_toggle_button(open: bool, cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .id("toggle-terminal")
        .size(px(28.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if open {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .border_1()
        .border_color(if open {
            theme::edge_hard()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .text_color(theme::bone_dim())
        .tab_index(0)
        .cursor_pointer()
        .hover(|button| button.bg(theme::panel()).text_color(theme::bone()))
        .active(|button| button.bg(theme::panel_lift()))
        .focus(|button| button.border_color(theme::focus()))
        .on_click(cx.listener(|view, _, window, cx| view.toggle_terminal(window, cx)))
        .child(
            svg()
                .path("icons/terminal.svg")
                .size(px(14.0))
                .text_color(if open {
                    theme::data()
                } else {
                    theme::bone_dim()
                }),
        )
}

fn sidebar_collapse_icon_button(cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .id("collapse-sidebar")
        .size(px(28.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme::bone_dim())
        .tab_index(0)
        .cursor_pointer()
        .hover(|button| button.bg(theme::panel()).text_color(theme::bone()))
        .active(|button| button.bg(theme::panel_lift()))
        .on_click(cx.listener(|view, _, window, cx| view.toggle_sidebar(window, cx)))
        .child(
            svg()
                .path("icons/chevron-left.svg")
                .size(px(12.0))
                .text_color(theme::ash()),
        )
}

pub(super) fn inspector_toggle_button(open: bool, cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .id("toggle-inspector")
        .size(px(28.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if open {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .border_1()
        .border_color(if open {
            theme::edge_hard()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .text_color(theme::bone_dim())
        .tab_index(0)
        .cursor_pointer()
        .hover(|button| button.bg(theme::panel()).text_color(theme::bone()))
        .active(|button| button.bg(theme::panel_lift()))
        .on_click(cx.listener(|view, _, window, cx| view.toggle_inspector(window, cx)))
        .child(
            svg()
                .path("icons/inspector.svg")
                .size(px(14.0))
                .text_color(if open {
                    theme::data()
                } else {
                    theme::bone_dim()
                }),
        )
}

fn sidebar_new_thread_button(enabled: bool, cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .id("new-session")
        .h(px(30.0))
        .w_full()
        .px(px(10.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .bg(if enabled {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .border_1()
        .border_color(if enabled {
            theme::edge_hard()
        } else {
            theme::edge_soft()
        })
        .text_color(if enabled {
            theme::bone()
        } else {
            theme::smoke()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_hover()).border_color(theme::edge()))
                .active(|button| button.bg(theme::panel()))
                .on_click(cx.listener(|view, _, window, cx| {
                    let _ = view.execute_native_action(NativeAction::NewSession, "", window, cx);
                }))
        })
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if enabled {
                    theme::signal()
                } else {
                    theme::smoke()
                })
                .child("+"),
        )
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .child("New thread"),
        )
}

fn sidebar_secondary_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    selected: bool,
    enabled: bool,
    on_click: controls::ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(28.0))
        .flex_1()
        .min_w_0()
        .px(px(8.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .bg(if selected {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .border_1()
        .border_color(if selected {
            theme::edge_hard()
        } else {
            theme::edge_soft()
        })
        .text_color(if !enabled {
            theme::smoke()
        } else if selected {
            theme::bone()
        } else {
            theme::ash()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| {
                    if selected {
                        button.bg(theme::panel_hover()).text_color(theme::bone())
                    } else {
                        button.bg(theme::panel()).text_color(theme::bone())
                    }
                })
                .active(|button| button.bg(theme::panel_lift()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::SEMIBOLD
                })
                .child(label.into()),
        )
}

struct ProjectGroupParams<'a> {
    project: &'a ProjectEntry,
    active: bool,
    active_catalog: &'a CatalogProjection,
    cached_catalog: Option<&'a ProjectCatalogCache>,
    current_path: Option<&'a PathBuf>,
    pending_path: Option<&'a PathBuf>,
    project_switch_enabled: bool,
    can_remove: bool,
    thread_statuses: &'a HashMap<String, ThreadRuntimeStatus>,
    hovered_thread_key: Option<&'a str>,
}

fn project_group(params: ProjectGroupParams<'_>, cx: &mut Context<RootView>) -> AnyElement {
    let ProjectGroupParams {
        project,
        active,
        active_catalog,
        cached_catalog,
        current_path,
        pending_path,
        project_switch_enabled,
        can_remove,
        thread_statuses,
        hovered_thread_key,
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
    let project_runtime_key = project_key(&path);
    let working_count = thread_statuses
        .values()
        .filter(|status| {
            status.project == project_runtime_key
                && matches!(
                    status.activity,
                    ThreadActivity::Opening | ThreadActivity::Working | ThreadActivity::Cancelling
                )
        })
        .count();
    let activity_key = list_animation_key(&project_runtime_key);

    div()
        .w_full()
        .flex()
        .flex_col()
        .mb(px(2.0))
        .child(
            div()
                .id(SharedString::from(format!(
                    "project-{}",
                    project_key(&path)
                )))
                .h(px(32.0))
                .pl(px(8.0))
                .pr(px(10.0))
                .tab_index(0)
                .cursor_pointer()
                .relative()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .bg(if active {
                    theme::panel()
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
                .when(active, |row| {
                    row.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(8.0))
                            .bottom(px(8.0))
                            .w(px(2.0))
                            .bg(theme::signal()),
                    )
                })
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "toggle-project-{}",
                            project_key(&path)
                        )))
                        .size(px(18.0))
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
                        .size(px(13.0))
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
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
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
                        .gap(px(6.0))
                        .when(
                            working_count > 0
                                || (status == CatalogStatus::Loading && !sessions.is_empty()),
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
                                .text_size(theme::text_size(theme::T_TINY))
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
                    let thread_key = format!(
                        "{}::{}",
                        project_key(&project.path),
                        project_key(&session.path)
                    );
                    project_thread_row(
                        ProjectThreadRowParams {
                            project_path: project.path.clone(),
                            session,
                            active_project: active,
                            selected: active
                                && pending_path
                                    .or(current_path)
                                    .is_some_and(|path| sidebar_paths_match(path, &session.path)),
                            switching: active
                                && pending_path
                                    .is_some_and(|path| sidebar_paths_match(path, &session.path)),
                            enabled: project_switch_enabled,
                            runtime_status: thread_statuses.get(&project_key(&session.path)),
                            hovered: hovered_thread_key == Some(thread_key.as_str()),
                            thread_key,
                        },
                        cx,
                    )
                }))
                .when_some(error, |group, error| {
                    let remove_path = project.path.clone();
                    group.child(
                        div()
                            .pl(px(32.0))
                            .pr(px(12.0))
                            .py(px(6.0))
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .font_family(theme::sans())
                                    .text_size(theme::text_size(theme::T_TINY))
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

struct ProjectThreadRowParams<'a> {
    project_path: PathBuf,
    session: &'a SessionSummary,
    active_project: bool,
    selected: bool,
    switching: bool,
    enabled: bool,
    runtime_status: Option<&'a ThreadRuntimeStatus>,
    hovered: bool,
    thread_key: String,
}

fn project_thread_row(
    params: ProjectThreadRowParams<'_>,
    cx: &mut Context<RootView>,
) -> AnyElement {
    let ProjectThreadRowParams {
        project_path,
        session,
        active_project,
        selected,
        switching,
        enabled,
        runtime_status,
        hovered,
        thread_key,
    } = params;
    let session_path = session.path.clone();
    let click_project = project_path.clone();
    let click_session = session_path.clone();
    let key_project = project_path.clone();
    let key_session = session_path.clone();
    let trash_project = project_path;
    let trash_session = session_path;
    let hover_key = thread_key.clone();
    let click_root = cx.entity();
    let key_root = click_root.clone();
    let hover_root = click_root.clone();
    let trash_root = click_root.clone();
    let title = sidebar_thread_title(
        session.name.as_deref(),
        session.first_user_summary.as_deref(),
    );
    let activity_key = list_animation_key(&session.id);
    let selected = selected || runtime_status.is_some_and(|status| status.active);
    let runtime_activity = runtime_status.map(|status| status.activity);
    let switching = switching || runtime_activity == Some(ThreadActivity::Opening);
    let trailing = thread_row_trailing(runtime_activity, selected, switching, &session.updated_at);
    let show_activity = switching
        || matches!(
            runtime_activity,
            Some(ThreadActivity::Working | ThreadActivity::Cancelling | ThreadActivity::Attention)
        );
    // Active/busy rows keep the date; only idle non-active threads expose delete on hover.
    let can_delete = crate::services::session_catalog::reversible_trash_available()
        && !selected
        && !switching
        && !show_activity;
    let show_delete = can_delete && hovered;
    let row_id = SharedString::from(format!(
        "project-thread-{}-{}",
        project_key(&click_project),
        session.id
    ));

    div()
        .id(row_id)
        .h(px(34.0))
        .pl(px(32.0))
        .pr(px(10.0))
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .bg(if selected {
            theme::panel_lift()
        } else if hovered {
            theme::panel()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .on_hover(move |hovered, _, cx| {
            let key = hover_key.clone();
            hover_root.update(cx, |view, cx| {
                if *hovered {
                    view.set_hovered_thread(Some(key), cx);
                } else if view.hovered_thread_key.as_deref() == Some(key.as_str()) {
                    view.set_hovered_thread(None, cx);
                }
            });
        })
        .when(selected, |row| {
            row.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(8.0))
                    .bottom(px(8.0))
                    .w(px(2.0))
                    .bg(theme::signal()),
            )
        })
        .when(enabled && !selected, |row| {
            row.tab_index(0)
                .cursor_pointer()
                .active(|row| row.bg(theme::panel_hover()))
                .focus(|row| row.bg(theme::panel_lift()).text_color(theme::focus()))
                .on_click(move |_, window, cx| {
                    click_root.update(cx, |view, cx| {
                        if active_project {
                            view.switch_session(click_session.clone(), window, cx);
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
                                view.switch_session(key_session.clone(), window, cx);
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
                .flex_1()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
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
        .when(show_activity, |row| {
            row.child(controls::square_status_indicator(
                activity_key,
                true,
                Duration::from_millis(720),
                match runtime_activity {
                    Some(ThreadActivity::Cancelling) => theme::data(),
                    Some(ThreadActivity::Attention) => theme::error(),
                    _ => theme::working(),
                },
            ))
        })
        .child(
            div()
                .flex_shrink_0()
                .h(px(22.0))
                // Keep the trailing column stable so the date ↔ delete swap does not shove the title.
                .when(can_delete, |slot| slot.min_w(px(36.0)))
                .flex()
                .items_center()
                .justify_end()
                .when(!show_delete, |slot| {
                    slot.child(
                        div()
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_TINY))
                            .whitespace_nowrap()
                            .text_color(trailing.color)
                            .child(trailing.label),
                    )
                })
                .when(show_delete, |slot| {
                    let trash_id = SharedString::from(format!(
                        "project-thread-trash-{}-{}",
                        project_key(&trash_project),
                        session.id
                    ));
                    let trash_group = trash_id.clone();
                    let key_trash_project = trash_project.clone();
                    let key_trash_session = trash_session.clone();
                    let key_trash_root = trash_root.clone();
                    slot.child(
                        div()
                            .id(trash_id)
                            .group(trash_group.clone())
                            .size(px(22.0))
                            .rounded(px(theme::RADIUS_SM))
                            .flex()
                            .items_center()
                            .justify_center()
                            .tab_index(0)
                            .cursor_pointer()
                            .hover(|button| button.bg(theme::canvas()))
                            .active(|button| button.bg(theme::panel_hover()))
                            .focus(|button| {
                                button
                                    .bg(theme::canvas())
                                    .border_1()
                                    .border_color(theme::focus())
                            })
                            .on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
                                if matches!(
                                    event.keystroke.key.as_str(),
                                    "enter" | "space" | "delete" | "backspace"
                                ) {
                                    cx.stop_propagation();
                                    key_trash_root.update(cx, |view, cx| {
                                        view.trash_thread(
                                            key_trash_project.clone(),
                                            key_trash_session.clone(),
                                            cx,
                                        );
                                    });
                                }
                            })
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                trash_root.update(cx, |view, cx| {
                                    view.trash_thread(
                                        trash_project.clone(),
                                        trash_session.clone(),
                                        cx,
                                    );
                                });
                            })
                            .child(
                                svg()
                                    .path("icons/trash.svg")
                                    .size(px(13.0))
                                    .text_color(theme::smoke())
                                    .group_hover(trash_group, |style| {
                                        style.text_color(theme::error())
                                    }),
                            ),
                    )
                }),
        )
        .into_any_element()
}

struct ThreadRowTrailing {
    label: String,
    color: gpui::Rgba,
}

fn thread_row_trailing(
    activity: Option<ThreadActivity>,
    selected: bool,
    switching: bool,
    updated_at: &str,
) -> ThreadRowTrailing {
    if switching {
        return ThreadRowTrailing {
            label: "Opening".to_owned(),
            color: theme::working(),
        };
    }
    match activity {
        Some(ThreadActivity::Working) => ThreadRowTrailing {
            label: if selected {
                "Working".to_owned()
            } else {
                "Busy".to_owned()
            },
            color: theme::working(),
        },
        Some(ThreadActivity::Cancelling) => ThreadRowTrailing {
            label: "Stop".to_owned(),
            color: theme::data(),
        },
        Some(ThreadActivity::Attention) => ThreadRowTrailing {
            label: "Alert".to_owned(),
            color: theme::error(),
        },
        Some(ThreadActivity::Idle | ThreadActivity::Opening) | None => ThreadRowTrailing {
            label: compact_session_day(updated_at),
            color: if selected {
                theme::ash()
            } else {
                theme::smoke()
            },
        },
    }
}

/// Prefer session name, else first-user summary. Collapse whitespace and drop a
/// stored trailing ellipsis so the row's own truncation owns the "…".
fn sidebar_thread_title(name: Option<&str>, first_user_summary: Option<&str>) -> String {
    let raw = name.or(first_user_summary).unwrap_or("Untitled thread");
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = strip_trailing_ellipsis(collapsed.trim());
    if trimmed.is_empty() {
        "Untitled thread".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn strip_trailing_ellipsis(value: &str) -> &str {
    let value = value.trim_end();
    value
        .strip_suffix('…')
        .or_else(|| value.strip_suffix("..."))
        .map(str::trim_end)
        .unwrap_or(value)
}

fn sidebar_paths_match(left: &std::path::Path, right: &std::path::Path) -> bool {
    project_key(left) == project_key(right)
}

fn list_animation_key(value: &str) -> usize {
    value.bytes().fold(1_009_usize, |key, byte| {
        key.wrapping_mul(131).wrapping_add(byte as usize)
    })
}

fn compact_session_day(timestamp: &str) -> String {
    let bytes = timestamp.as_bytes();
    let iso_shape = bytes.len() >= 10 && bytes.get(4) == Some(&b'-') && bytes.get(7) == Some(&b'-');
    if !iso_shape {
        return compact_session_timestamp(timestamp);
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
    let Some(day) = timestamp.get(8..10) else {
        return timestamp.to_owned();
    };
    let day = day.trim_start_matches('0');
    if day.is_empty() {
        return timestamp.to_owned();
    }
    format!("{month} {day}")
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

    let day = compact_session_day(timestamp);
    let Some(time) = timestamp.get(11..16) else {
        return day;
    };
    if day == timestamp {
        return timestamp.to_owned();
    }
    format!("{day}, {time}")
}

fn project_tree_loading_note(animation_key: usize, text: &'static str) -> AnyElement {
    div()
        .h(px(32.0))
        .pl(px(32.0))
        .pr(px(12.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .font_family(theme::sans())
        .text_size(theme::text_size(theme::T_TINY))
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
        .h(px(32.0))
        .pl(px(32.0))
        .pr(px(12.0))
        .flex()
        .items_center()
        .font_family(theme::sans())
        .text_size(theme::text_size(theme::T_TINY))
        .text_color(theme::smoke())
        .child(text.into())
        .into_any_element()
}

pub(super) struct HistoryPanelParams<'a> {
    pub(super) projection: &'a HistoryProjection,
    pub(super) bridge: &'a BridgeProjection,
    pub(super) browser: &'a HistoryBrowser,
    pub(super) focus: &'a FocusHandle,
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
        label,
        import_path,
        confirmation,
        summarize,
    } = params;
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
    let tip_status = match projection.status {
        crate::state::runtime::FacetStatus::Loading => Some("Loading session tip…"),
        crate::state::runtime::FacetStatus::Failed(_) => Some("Session tip unavailable."),
        crate::state::runtime::FacetStatus::Ready if details.is_none() => {
            Some("No active session tip.")
        }
        crate::state::runtime::FacetStatus::Ready => None,
    };

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
                .pb(px(10.0))
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(controls::section_label("History")),
        )
        .child(
            div()
                .id("history-tools-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .scrollbar_width(px(theme::SCROLLBAR))
                .px(px(10.0))
                .py(px(10.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .when_some(tip_status, |panel, status| {
                    panel.child(controls::panel_note(status, controls::ControlTone::Normal))
                })
                .when_some(details, |panel, details| {
                    let body = details.body.chars().take(120).collect::<String>();
                    let meta = format!(
                        "{} · {} child{}",
                        details.kind,
                        details.child_count,
                        if details.child_count == 1 { "" } else { "ren" },
                    );
                    let label_suffix = details
                        .label
                        .as_deref()
                        .filter(|label| !label.is_empty())
                        .map(|label| format!(" · {label}"))
                        .unwrap_or_default();
                    panel.child(
                        div()
                            .px(px(10.0))
                            .py(px(10.0))
                            .rounded(px(theme::RADIUS_SM))
                            .bg(theme::panel())
                            .border_1()
                            .border_color(theme::edge_soft())
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .font_family(theme::sans())
                                    .text_size(theme::text_size(theme::T_TINY))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::ash())
                                    .child(if details.active_leaf {
                                        "Active tip"
                                    } else {
                                        "Selected entry"
                                    }),
                            )
                            .child(
                                div()
                                    .font_family(theme::sans())
                                    .text_size(theme::text_size(theme::T_UI_SM))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::bone())
                                    .child(format!("{}{}", details.title, label_suffix)),
                            )
                            .child(
                                div()
                                    .font_family(theme::mono())
                                    .text_size(theme::text_size(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .child(meta),
                            )
                            .when(!body.is_empty(), |block| {
                                block.child(
                                    div()
                                        .mt(px(2.0))
                                        .text_size(theme::text_size(theme::T_TINY))
                                        .line_height(gpui::relative(1.4))
                                        .text_color(theme::bone_dim())
                                        .overflow_hidden()
                                        .child(body),
                                )
                            })
                            .child(
                                div()
                                    .mt(px(2.0))
                                    .font_family(theme::mono())
                                    .text_size(theme::text_size(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .child("↑↓ step · Enter navigate"),
                            ),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .child(controls::section_label("Session tools"))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap(px(6.0))
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
                                        Box::new(cx.listener(|view, _, _, cx| {
                                            view.request_navigation(cx)
                                        })),
                                    ))
                                })
                                .when(export_available, |actions| {
                                    actions.child(controls::quiet_button(
                                        "history-export-jsonl",
                                        "Export",
                                        ready,
                                        Box::new(
                                            cx.listener(|view, _, _, cx| view.export_jsonl(cx)),
                                        ),
                                    ))
                                })
                                .when(bridge.pending.is_some(), |actions| {
                                    actions.child(controls::quiet_button(
                                        "history-cancel-bridge",
                                        "Cancel",
                                        true,
                                        Box::new(
                                            cx.listener(|view, _, _, cx| view.cancel_bridge(cx)),
                                        ),
                                    ))
                                }),
                        )
                        .when(navigation_available && summary_available, |block| {
                            block.child(controls::chip_button(
                                "history-summary",
                                "Branch summary",
                                summarize,
                                ready,
                                Box::new(
                                    cx.listener(|view, _, _, cx| {
                                        view.toggle_navigation_summary(cx)
                                    }),
                                ),
                            ))
                        }),
                )
                .when(labels_available && selected.is_some(), |panel| {
                    panel.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(controls::section_label("Label tip"))
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
                                        Box::new(cx.listener(|view, _, _, cx| {
                                            view.clear_selected_label(cx)
                                        })),
                                    )),
                            ),
                    )
                })
                .when(import_available, |panel| {
                    panel.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
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
                            "Fork before tip?",
                            "Creates a new session file. Message text returns to the composer.",
                        ),
                        HistoryConfirmation::Clone => (
                            "Clone current path?",
                            "New file gets this path. Abandoned branches stay in the original.",
                        ),
                    };
                    panel.child(
                        div()
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
                                    .text_size(theme::text_size(theme::T_UI_SM))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::bone())
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(theme::text_size(theme::T_TINY))
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
                                        Box::new(cx.listener(|view, _, _, cx| {
                                            view.confirm_history_operation(cx)
                                        })),
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
                .when_some(bridge.unavailable.clone(), |panel, unavailable| {
                    panel.child(
                        div()
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
                }),
        )
        .when_some(bridge.feedback.clone(), |panel, feedback| {
            panel.child(controls::panel_footer_status(feedback))
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
    fn thread_rows_distinguish_foreground_and_background_work() {
        let updated = "2026-07-26T14:08:51.000Z";
        assert_eq!(
            thread_row_trailing(Some(ThreadActivity::Working), false, false, updated).label,
            "Busy"
        );
        assert_eq!(
            thread_row_trailing(Some(ThreadActivity::Working), true, false, updated).label,
            "Working"
        );
        assert_eq!(
            thread_row_trailing(Some(ThreadActivity::Attention), false, false, updated).label,
            "Alert"
        );
        assert_eq!(
            thread_row_trailing(None, false, false, updated).label,
            "Jul 26"
        );
    }

    #[test]
    fn sidebar_thread_title_is_single_line_without_stored_ellipsis() {
        let title = sidebar_thread_title(None, Some("for the current diff.\nI think we should…"));
        assert_eq!(title, "for the current diff. I think we should");
        assert!(!title.ends_with('…'));
        assert!(!title.ends_with("..."));
        assert_eq!(sidebar_thread_title(Some("hey"), Some("ignored")), "hey");
        assert_eq!(sidebar_thread_title(None, Some("...")), "Untitled thread");
        assert_eq!(sidebar_thread_title(None, Some("v1.0")), "v1.0");
        assert_eq!(strip_trailing_ellipsis("done..."), "done");
        assert_eq!(strip_trailing_ellipsis("done…"), "done");
    }

    #[test]
    fn session_timestamp_is_compact_without_losing_unknown_formats() {
        assert_eq!(
            compact_session_timestamp("2026-07-26T14:08:51.000Z"),
            "Jul 26, 14:08"
        );
        assert_eq!(compact_session_day("2026-07-26T14:08:51.000Z"), "Jul 26");
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
