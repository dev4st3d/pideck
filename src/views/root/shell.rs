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
    pub(super) workspace_diff_available: bool,
    pub(super) workspace_diff_open: bool,
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
        workspace_diff_available,
        workspace_diff_open,
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
                .child(titlebar_icon_toggle(
                    ChromeIconSpec {
                        id: "toggle-sidebar",
                        icon_path: "icons/sidebar.svg",
                        tooltip_label: "Workspace sidebar",
                        tooltip_hint: Some("Ctrl+B"),
                        on: sidebar_open,
                        enabled: true,
                        action: |view, window, cx| view.toggle_sidebar(window, cx),
                    },
                    cx,
                ))
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
                        .child(titlebar_icon_toggle(
                            ChromeIconSpec {
                                id: "rename-session",
                                icon_path: "icons/pencil.svg",
                                tooltip_label: "Rename session",
                                tooltip_hint: None,
                                on: rename_open,
                                enabled: rename_enabled,
                                action: |view, window, cx| view.toggle_session_rename(window, cx),
                            },
                            cx,
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
                .child(titlebar_icon_toggle(
                    ChromeIconSpec {
                        id: "toggle-terminal",
                        icon_path: "icons/terminal.svg",
                        tooltip_label: "Terminal",
                        tooltip_hint: Some("Ctrl+`"),
                        on: terminal_open,
                        enabled: true,
                        action: |view, window, cx| view.toggle_terminal(window, cx),
                    },
                    cx,
                ))
                .child(titlebar_icon_toggle(
                    ChromeIconSpec {
                        id: "toggle-workspace-diff",
                        icon_path: "icons/diff.svg",
                        tooltip_label: "Workspace changes",
                        tooltip_hint: None,
                        on: workspace_diff_open && workspace_diff_available,
                        enabled: workspace_diff_available,
                        action: |view, window, cx| view.toggle_workspace_diff_overlay(window, cx),
                    },
                    cx,
                ))
                .child(titlebar_icon_toggle(
                    ChromeIconSpec {
                        id: "toggle-inspector",
                        icon_path: "icons/inspector.svg",
                        tooltip_label: "Inspector",
                        tooltip_hint: Some("Ctrl+I"),
                        on: inspector_open,
                        enabled: true,
                        action: |view, window, cx| view.toggle_inspector(window, cx),
                    },
                    cx,
                ))
                .child(
                    div()
                        .relative()
                        .flex_shrink_0()
                        .child(
                            // Height/padding/type sit flush with the 28px
                            // titlebar icon toggles beside it.
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
                                .focus(|switcher| switcher.border_color(theme::focus()))
                                .tab_index(0)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.toggle_theme_menu(window, cx)
                                }))
                                .on_key_down(cx.listener(
                                    |view, event: &gpui::KeyDownEvent, window, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            cx.stop_propagation();
                                            view.toggle_theme_menu(window, cx);
                                        }
                                    },
                                ))
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

/// One keyboard-navigable destination in the workspace tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SidebarNode {
    Project(PathBuf),
    Thread { project: PathBuf, session: PathBuf },
}

/// Non-interactive status line under an expanded project; still consumes a
/// scroll slot so keyboard scroll-into-view matches painted children.
#[derive(Debug, Clone)]
pub(super) enum SidebarNote {
    Loading,
    Empty,
    Unavailable,
    Corrupt(usize),
}

/// One scroll child in the workspace list, in painted order.
#[derive(Debug, Clone)]
pub(super) enum SidebarRow {
    Node(SidebarNode),
    Note { project: PathBuf, note: SidebarNote },
    Error { project: PathBuf, message: String },
}

/// Shared resolution of "which sessions this project shows", so the painted
/// list and keyboard navigation can never disagree.
struct ResolvedProjectCatalog {
    status: CatalogStatus,
    sessions: Arc<Vec<SessionSummary>>,
    corrupt_count: usize,
    error: Option<String>,
}

fn resolve_project_catalog(
    active: bool,
    active_catalog: &CatalogProjection,
    cached_catalog: Option<&ProjectCatalogCache>,
) -> ResolvedProjectCatalog {
    let cached_while_loading = cached_catalog.filter(|catalog| {
        active
            && active_catalog.status == CatalogStatus::Loading
            && active_catalog.sessions.is_empty()
            && !catalog.sessions.is_empty()
    });
    if let Some(catalog) = cached_while_loading {
        ResolvedProjectCatalog {
            status: CatalogStatus::Loading,
            sessions: Arc::clone(&catalog.sessions),
            corrupt_count: catalog.corrupt_count,
            error: None,
        }
    } else if active {
        ResolvedProjectCatalog {
            status: active_catalog.status,
            sessions: Arc::clone(&active_catalog.sessions),
            corrupt_count: active_catalog.corrupt.len(),
            error: active_catalog.error.clone(),
        }
    } else if let Some(catalog) = cached_catalog {
        ResolvedProjectCatalog {
            status: catalog.status,
            sessions: Arc::clone(&catalog.sessions),
            corrupt_count: catalog.corrupt_count,
            error: catalog.error.clone(),
        }
    } else {
        ResolvedProjectCatalog {
            status: CatalogStatus::Loading,
            sessions: Arc::new(Vec::new()),
            corrupt_count: 0,
            error: None,
        }
    }
}

/// Resolved per-project input for the flattened workspace rows.
pub(super) struct SidebarProjectSlice {
    path: PathBuf,
    expanded: bool,
    status: CatalogStatus,
    sessions: Arc<Vec<SessionSummary>>,
    error: Option<String>,
    corrupt_count: usize,
}

/// Resolves every project exactly once; the render loop and the keyboard
/// handlers both walk this same ordering.
pub(super) fn sidebar_project_slices(
    projects: &ProjectRegistry,
    active_catalog: &CatalogProjection,
    cached: &HashMap<String, ProjectCatalogCache>,
) -> Vec<SidebarProjectSlice> {
    projects
        .projects()
        .iter()
        .map(|project| {
            let active = projects.is_active(&project.path);
            let resolved = resolve_project_catalog(
                active,
                active_catalog,
                cached.get(&project_key(&project.path)),
            );
            SidebarProjectSlice {
                path: project.path.clone(),
                expanded: project.expanded,
                status: resolved.status,
                sessions: resolved.sessions,
                error: resolved.error,
                corrupt_count: resolved.corrupt_count,
            }
        })
        .collect()
}

/// Flattens projects and their expanded threads into painted row order.
/// Notes are real scroll children, so they consume slots here as well.
pub(super) fn sidebar_rows(slices: &[SidebarProjectSlice]) -> Vec<SidebarRow> {
    let mut rows = Vec::new();
    for slice in slices {
        let project = slice.path.clone();
        rows.push(SidebarRow::Node(SidebarNode::Project(project.clone())));
        if !slice.expanded {
            continue;
        }
        if slice.sessions.is_empty() {
            let note = match slice.status {
                CatalogStatus::Loading => SidebarNote::Loading,
                CatalogStatus::Empty | CatalogStatus::Ready => SidebarNote::Empty,
                CatalogStatus::Inaccessible | CatalogStatus::Stale => SidebarNote::Unavailable,
            };
            rows.push(SidebarRow::Note {
                project: project.clone(),
                note,
            });
        }
        rows.extend(slice.sessions.iter().map(|session| {
            SidebarRow::Node(SidebarNode::Thread {
                project: project.clone(),
                session: session.path.clone(),
            })
        }));
        if slice.error.is_some() {
            rows.push(SidebarRow::Error {
                project: project.clone(),
                message: slice.error.clone().unwrap_or_default(),
            });
        }
        if slice.corrupt_count > 0 {
            rows.push(SidebarRow::Note {
                project,
                note: SidebarNote::Corrupt(slice.corrupt_count),
            });
        }
    }
    rows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SidebarCursorMove {
    First,
    Previous,
    Next,
    Last,
}

/// Moves the workspace cursor across node rows; notes and error blocks are
/// never destinations. Returns the node plus its flat scroll-child index so
/// the caller can scroll it into view.
pub(super) fn sidebar_moved_cursor(
    rows: &[SidebarRow],
    current: Option<&SidebarNode>,
    movement: SidebarCursorMove,
) -> Option<(SidebarNode, usize)> {
    let mut node_slots: Vec<(usize, &SidebarNode)> = Vec::new();
    for (slot, row) in rows.iter().enumerate() {
        if let SidebarRow::Node(node) = row {
            node_slots.push((slot, node));
        }
    }
    if node_slots.is_empty() {
        return None;
    }
    let position =
        current.and_then(|current| node_slots.iter().position(|(_, node)| *node == current));
    let next = match (movement, position) {
        (SidebarCursorMove::First, _) => 0,
        (SidebarCursorMove::Last, _) => node_slots.len() - 1,
        (SidebarCursorMove::Next, None) => 0,
        (SidebarCursorMove::Next, Some(position)) => (position + 1).min(node_slots.len() - 1),
        (SidebarCursorMove::Previous, None) => node_slots.len() - 1,
        (SidebarCursorMove::Previous, Some(position)) => position.saturating_sub(1),
    };
    let (slot, node) = node_slots[next];
    Some((node.clone(), slot))
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
    pub(super) cursor: Option<&'a SidebarNode>,
    pub(super) tree_focused: bool,
    pub(super) tree_focus: &'a FocusHandle,
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
        cursor,
        tree_focused,
        tree_focus,
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

    // Painted rows and keyboard navigation share this flattened order.
    let slices = sidebar_project_slices(projects, catalog, project_catalogs);
    let slice_index: HashMap<String, usize> = slices
        .iter()
        .enumerate()
        .map(|(index, slice)| (project_key(&slice.path), index))
        .collect();
    let rows = sidebar_rows(&slices);

    let mut tree_children: Vec<AnyElement> = Vec::with_capacity(rows.len());
    for (slot, row) in rows.iter().enumerate() {
        let top_gap = if slot == 0 {
            0.0
        } else if matches!(row, SidebarRow::Node(SidebarNode::Project(_))) {
            TREE_GROUP_GAP
        } else {
            TREE_ROW_GAP
        };
        match row {
            SidebarRow::Node(SidebarNode::Project(path)) => {
                let key = project_key(path);
                let Some(&index) = slice_index.get(&key) else {
                    continue;
                };
                let slice = &slices[index];
                let entry = projects
                    .projects()
                    .iter()
                    .find(|entry| project_key(&entry.path) == key);
                let Some(entry) = entry else {
                    continue;
                };
                let active = projects.is_active(path);
                let working_count = thread_statuses
                    .values()
                    .filter(|status| {
                        status.project == key
                            && matches!(
                                status.activity,
                                ThreadActivity::Opening
                                    | ThreadActivity::Working
                                    | ThreadActivity::Cancelling
                            )
                    })
                    .count();
                tree_children.push(project_row(
                    ProjectRowParams {
                        path: path.clone(),
                        name: entry.name(),
                        expanded: entry.expanded,
                        active,
                        status: slice.status,
                        session_count: slice.sessions.len(),
                        working_count,
                        cursored: cursor == Some(&SidebarNode::Project(path.clone())),
                        tree_focused,
                        top_gap,
                    },
                    cx,
                ));
            }
            SidebarRow::Node(SidebarNode::Thread { project, session }) => {
                let key = project_key(project);
                let Some(&index) = slice_index.get(&key) else {
                    continue;
                };
                let summary = slices[index]
                    .sessions
                    .iter()
                    .find(|entry| project_key(&entry.path) == project_key(session));
                let Some(summary) = summary else {
                    continue;
                };
                let active_project = projects.is_active(project);
                let thread_key = format!("{}::{}", key, project_key(&summary.path));
                tree_children.push(project_thread_row(
                    ProjectThreadRowParams {
                        project_path: project.clone(),
                        session: summary,
                        active_project,
                        selected: active_project
                            && pending_path
                                .or(current_path)
                                .is_some_and(|path| sidebar_paths_match(path, &summary.path)),
                        switching: active_project
                            && pending_path
                                .is_some_and(|path| sidebar_paths_match(path, &summary.path)),
                        enabled: project_switch_enabled,
                        runtime_status: thread_statuses.get(&project_key(&summary.path)),
                        hovered: hovered_thread_key == Some(thread_key.as_str()),
                        cursored: cursor
                            == Some(&SidebarNode::Thread {
                                project: project.clone(),
                                session: session.clone(),
                            }),
                        tree_focused,
                        thread_key,
                        top_gap,
                    },
                    cx,
                ));
            }
            SidebarRow::Note { project, note } => {
                tree_children.push(sidebar_note_row(project, note, top_gap));
            }
            SidebarRow::Error { project, message } => {
                tree_children.push(sidebar_error_row(
                    project,
                    message,
                    !projects.is_active(project),
                    project_count > 1,
                    top_gap,
                    cx,
                ));
            }
        }
    }

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
                        .child(sidebar_header_icon_button(
                            ChromeIconSpec {
                                id: "add-project",
                                icon_path: "icons/plus.svg",
                                tooltip_label: "Add project folders",
                                tooltip_hint: None,
                                on: false,
                                enabled: !project_picker_pending,
                                action: |view, _window, cx| view.choose_projects(cx),
                            },
                            sidebar_open,
                            cx,
                        ))
                        .child(sidebar_header_icon_button(
                            ChromeIconSpec {
                                id: "refresh-projects",
                                icon_path: "icons/refresh.svg",
                                tooltip_label: "Refresh threads",
                                tooltip_hint: None,
                                on: false,
                                enabled: catalog.status != CatalogStatus::Loading,
                                action: |view, _window, cx| view.refresh_sessions(cx),
                            },
                            sidebar_open,
                            cx,
                        ))
                        .child(sidebar_header_icon_button(
                            ChromeIconSpec {
                                id: "collapse-sidebar",
                                icon_path: "icons/chevron-left.svg",
                                tooltip_label: "Collapse sidebar",
                                tooltip_hint: Some("Ctrl+B"),
                                on: false,
                                enabled: true,
                                action: |view, window, cx| view.toggle_sidebar(window, cx),
                            },
                            sidebar_open,
                            cx,
                        )),
                ),
        )
        .child(
            div()
                .px(px(SIDE_PAD))
                .pt(px(10.0))
                .pb(px(8.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(sidebar_new_thread_button(
                    new_thread_enabled,
                    sidebar_open,
                    cx,
                ))
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
                            sidebar_open,
                            |view, window, cx| {
                                let _ =
                                    view.execute_native_action(NativeAction::Tree, "", window, cx);
                            },
                            cx,
                        ))
                        .child(sidebar_secondary_button(
                            "export-session",
                            "Export",
                            false,
                            export_enabled,
                            sidebar_open,
                            |view, window, cx| view.export_session(window, cx),
                            cx,
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
            // The tree owns one tab stop and a roving cursor. Focus
            // indication lives on the cursor ROW and follows web
            // :focus-visible rules: clicking never flashes a section-sized
            // frame; only keyboard navigation earns the strong ring.
            div()
                .id("workspace-tree")
                .track_focus(tree_focus)
                .when(sidebar_open, |tree| tree.tab_index(0))
                .flex_1()
                .min_h_0()
                .relative()
                .my(px(4.0))
                .on_key_down(cx.listener(
                    |view: &mut RootView, event: &gpui::KeyDownEvent, window, cx| {
                        view.on_workspace_tree_key(event, window, cx);
                    },
                ))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _, window, _cx| {
                        view.sidebar_tree_pointer_focus = true;
                        let focus = view.sidebar_tree_focus.clone();
                        focus.focus(window);
                    }),
                )
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
                        .px(px(4.0))
                        .pt(px(2.0))
                        .pb(px(6.0))
                        .flex()
                        .flex_col()
                        .when(rows.is_empty(), |list| list.child(empty_projects_note()))
                        .children(tree_children),
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
                .child(sidebar_remove_project_button(
                    can_remove_active,
                    active_path,
                    sidebar_open,
                    cx,
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

/// View-method action shared by chrome buttons so click and Enter/Space can
/// never drift apart.
type ChromeAction = fn(&mut RootView, &mut Window, &mut Context<RootView>);

/// Parameters for an icon-only chrome button.
struct ChromeIconSpec {
    id: &'static str,
    icon_path: &'static str,
    tooltip_label: &'static str,
    tooltip_hint: Option<&'static str>,
    /// Toggle look (pressed-in); header actions always pass `false`.
    on: bool,
    enabled: bool,
    action: ChromeAction,
}

/// Icon-only chrome toggle: tooltip, focus ring, and a real keyboard path.
fn titlebar_icon_toggle(spec: ChromeIconSpec, cx: &mut Context<RootView>) -> impl IntoElement {
    let ChromeIconSpec {
        id,
        icon_path,
        tooltip_label,
        tooltip_hint,
        on,
        enabled,
        action,
    } = spec;
    let icon_color = if !enabled {
        theme::smoke()
    } else if on {
        theme::data()
    } else {
        theme::bone_dim()
    };
    div()
        .id(id)
        .size(px(28.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if on {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .border_1()
        .border_color(if on {
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
                .focus(|button| button.border_color(theme::focus()))
                .active(|button| button.bg(theme::panel_lift()))
                .tooltip(controls::text_tooltip(tooltip_label, tooltip_hint))
                .on_click(cx.listener(move |view, _, window, cx| action(view, window, cx)))
                .on_key_down(
                    cx.listener(move |view, event: &gpui::KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            action(view, window, cx);
                        }
                    }),
                )
        })
        .child(svg().path(icon_path).size(px(14.0)).text_color(icon_color))
}

/// Icon-only sidebar header action; drops out of the tab order while the
/// sidebar is collapsed so focus never lands on invisible chrome.
fn sidebar_header_icon_button(
    spec: ChromeIconSpec,
    sidebar_open: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let ChromeIconSpec {
        id,
        icon_path,
        tooltip_label,
        tooltip_hint,
        on: _,
        enabled,
        action,
    } = spec;
    let icon_color = if enabled {
        theme::ash()
    } else {
        theme::smoke()
    };
    div()
        .id(id)
        .size(px(28.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(gpui::rgba(0x0000_0000))
        .when(enabled, |button| {
            button
                .when(sidebar_open, |button| button.tab_index(0))
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel()))
                .focus(|button| button.border_color(theme::focus()))
                .active(|button| button.bg(theme::panel_lift()))
                .tooltip(controls::text_tooltip(tooltip_label, tooltip_hint))
                .on_click(cx.listener(move |view, _, window, cx| action(view, window, cx)))
                .on_key_down(
                    cx.listener(move |view, event: &gpui::KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            action(view, window, cx);
                        }
                    }),
                )
        })
        .child(svg().path(icon_path).size(px(13.0)).text_color(icon_color))
}

fn sidebar_new_thread_button(
    enabled: bool,
    sidebar_open: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let icon_color = if enabled {
        theme::signal()
    } else {
        theme::smoke()
    };
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
                .when(sidebar_open, |button| button.tab_index(0))
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_hover()).border_color(theme::edge()))
                .focus(|button| button.border_color(theme::focus()))
                .active(|button| button.bg(theme::panel()))
                .tooltip(controls::text_tooltip("New thread", None::<&str>))
                .on_click(cx.listener(|view, _, window, cx| {
                    let _ = view.execute_native_action(NativeAction::NewSession, "", window, cx);
                }))
                .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        let _ =
                            view.execute_native_action(NativeAction::NewSession, "", window, cx);
                    }
                }))
        })
        .child(
            svg()
                .path("icons/plus.svg")
                .size(px(12.0))
                .text_color(icon_color),
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
    sidebar_open: bool,
    action: ChromeAction,
    cx: &mut Context<RootView>,
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
                .when(sidebar_open, |button| button.tab_index(0))
                .cursor_pointer()
                .hover(|button| {
                    if selected {
                        button.bg(theme::panel_hover()).text_color(theme::bone())
                    } else {
                        button.bg(theme::panel()).text_color(theme::bone())
                    }
                })
                .focus(|button| button.border_color(theme::focus()))
                .active(|button| button.bg(theme::panel_lift()))
                .on_click(cx.listener(move |view, _, window, cx| action(view, window, cx)))
                .on_key_down(
                    cx.listener(move |view, event: &gpui::KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            action(view, window, cx);
                        }
                    }),
                )
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

/// Row metrics for the flattened workspace tree.
const TREE_ROW_H: f32 = 32.0;
const TREE_ROW_GAP: f32 = 2.0;
const TREE_GROUP_GAP: f32 = 6.0;

/// Cursor ring policy, mirroring :focus-visible: a strong accent ring requires
/// keyboard focus in the tree; a cursor parked by the pointer reads as a quiet
/// outline (and collapses into the existing active/selected edge).
fn cursor_border(cursored: bool, tree_keyboard_focused: bool, emphasized: bool) -> gpui::Rgba {
    if cursored && tree_keyboard_focused {
        theme::focus()
    } else if cursored || emphasized {
        theme::edge_soft()
    } else {
        gpui::rgba(0x0000_0000)
    }
}

struct ProjectRowParams {
    path: PathBuf,
    name: String,
    expanded: bool,
    active: bool,
    status: CatalogStatus,
    session_count: usize,
    working_count: usize,
    cursored: bool,
    tree_focused: bool,
    top_gap: f32,
}

fn project_row(params: ProjectRowParams, cx: &mut Context<RootView>) -> AnyElement {
    let ProjectRowParams {
        path,
        name,
        expanded,
        active,
        status,
        session_count,
        working_count,
        cursored,
        tree_focused,
        top_gap,
    } = params;
    let count = match status {
        CatalogStatus::Inaccessible => "!".to_owned(),
        CatalogStatus::Stale => format!("{session_count}!"),
        CatalogStatus::Loading | CatalogStatus::Ready | CatalogStatus::Empty => {
            session_count.to_string()
        }
    };
    let project_id = project_key(&path);
    let activity_key = list_animation_key(&project_id);

    let row_bg = if active {
        theme::panel()
    } else {
        gpui::rgba(0x0000_0000)
    };
    let hover_bg = if active {
        theme::panel_hover()
    } else {
        theme::panel()
    };
    let row_border = cursor_border(cursored, tree_focused, active);

    let row_id = SharedString::from(format!("project-{project_id}"));
    let toggle_id = SharedString::from(format!("toggle-project-{project_id}"));
    let click_path = path.clone();
    let toggle_path = path;
    let click_root = cx.entity();
    let toggle_root = click_root.clone();

    div()
        .id(row_id)
        .h(px(TREE_ROW_H))
        .mt(px(top_gap))
        .pl(px(4.0))
        .pr(px(8.0))
        .rounded(px(theme::RADIUS))
        .border_1()
        .border_color(row_border)
        .bg(row_bg)
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .hover(|row| row.bg(hover_bg))
        .on_click(move |_, window, cx| {
            click_root.update(cx, |view, cx| {
                view.activate_project(click_path.clone(), None, window, cx)
            });
        })
        .child(
            div()
                .id(toggle_id)
                .size(px(18.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_hover()))
                .tooltip(controls::text_tooltip(
                    if expanded {
                        "Collapse project"
                    } else {
                        "Expand project"
                    },
                    None::<&str>,
                ))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    toggle_root.update(cx, |view, cx| view.toggle_project(toggle_path.clone(), cx));
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
                .child(name),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .when(
                    working_count > 0 || (status == CatalogStatus::Loading && session_count > 0),
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
                            if matches!(status, CatalogStatus::Inaccessible | CatalogStatus::Stale)
                            {
                                theme::error()
                            } else {
                                theme::smoke()
                            },
                        )
                        .child(count),
                ),
        )
        .into_any_element()
}

/// Non-interactive status line inside the flattened tree.
fn sidebar_note_row(project: &std::path::Path, note: &SidebarNote, top_gap: f32) -> AnyElement {
    // Reserved transparent border keeps note text aligned with thread rows,
    // which carry the same 1px frame.
    fn frame(top_gap: f32) -> gpui::Div {
        div()
            .h(px(TREE_ROW_H))
            .mt(px(top_gap))
            .pl(px(28.0))
            .pr(px(12.0))
            .border_1()
            .border_color(gpui::rgba(0x0000_0000))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .font_family(theme::sans())
            .text_size(theme::text_size(theme::T_TINY))
            .text_color(theme::smoke())
    }
    match note {
        SidebarNote::Loading => frame(top_gap)
            .child(controls::square_status_indicator(
                list_animation_key(&format!("loading-{}", project_key(project))),
                true,
                Duration::from_millis(900),
                theme::working(),
            ))
            .child("Scanning threads")
            .into_any_element(),
        SidebarNote::Empty => frame(top_gap)
            .child("No saved threads yet.")
            .into_any_element(),
        SidebarNote::Unavailable => frame(top_gap)
            .child("Project threads are unavailable.")
            .into_any_element(),
        SidebarNote::Corrupt(count) => frame(top_gap)
            .child(format!(
                "{count} corrupt thread{} skipped.",
                if *count == 1 { "" } else { "s" }
            ))
            .into_any_element(),
    }
}

/// Per-project catalog failure, with a way out of the sidebar for dead folders.
fn sidebar_error_row(
    project: &std::path::Path,
    message: &str,
    show_remove: bool,
    can_remove: bool,
    top_gap: f32,
    cx: &mut Context<RootView>,
) -> AnyElement {
    let remove_path: PathBuf = project.to_path_buf();
    div()
        .mt(px(top_gap))
        .pl(px(28.0))
        .pr(px(12.0))
        .py(px(6.0))
        .border_1()
        .border_color(gpui::rgba(0x0000_0000))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_TINY))
                .line_height(gpui::relative(1.35))
                .text_color(theme::error())
                .child(message.to_owned()),
        )
        .when(show_remove, |note| {
            note.child(controls::quiet_button(
                SharedString::from(format!("remove-project-{}", project_key(&remove_path))),
                "Remove from sidebar",
                can_remove,
                Box::new(cx.listener(move |view, _, window, cx| {
                    view.remove_project(remove_path.clone(), window, cx)
                })),
            ))
        })
        .into_any_element()
}

/// Guidance shown instead of the tree when the sidebar has no projects.
fn empty_projects_note() -> AnyElement {
    div()
        .mx(px(2.0))
        .mt(px(6.0))
        .px(px(10.0))
        .py(px(9.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::bone_dim())
                .child("No projects yet."),
        )
        .child(
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_TINY))
                .line_height(gpui::relative(1.4))
                .text_color(theme::smoke())
                .child("Add a folder with the + button above to keep its threads here."),
        )
        .into_any_element()
}

/// Footer action: drops the active project from the sidebar (folders on disk
/// are never touched).
fn sidebar_remove_project_button(
    enabled: bool,
    path: PathBuf,
    sidebar_open: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let click_path = path.clone();
    let key_path = path;
    div()
        .id("remove-active-project")
        .h(px(28.0))
        .px(px(8.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(gpui::rgba(0x0000_0000))
        .text_color(if enabled {
            theme::bone_dim()
        } else {
            theme::smoke()
        })
        .when(enabled, |button| {
            button
                .when(sidebar_open, |button| button.tab_index(0))
                .cursor_pointer()
                .hover(|button| button.bg(theme::error_wash()).text_color(theme::error()))
                .focus(|button| button.border_color(theme::focus()))
                .active(|button| button.bg(theme::panel_lift()))
                .tooltip(controls::text_tooltip(
                    "Remove project from sidebar",
                    None::<&str>,
                ))
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.remove_project(click_path.clone(), window, cx)
                }))
                .on_key_down(
                    cx.listener(move |view, event: &gpui::KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            view.remove_project(key_path.clone(), window, cx);
                        }
                    }),
                )
        })
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Remove"),
        )
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
    cursored: bool,
    tree_focused: bool,
    thread_key: String,
    top_gap: f32,
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
        cursored,
        tree_focused,
        thread_key,
        top_gap,
    } = params;
    let click_project = project_path.clone();
    let click_session = session.path.clone();
    let trash_project = project_path;
    let trash_session = session.path.clone();
    let hover_key = thread_key.clone();
    let click_root = cx.entity();
    let trash_root = click_root.clone();
    let hover_root = click_root.clone();
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

    let row_bg = if selected {
        theme::panel_lift()
    } else if hovered {
        theme::panel()
    } else {
        gpui::rgba(0x0000_0000)
    };
    let row_border = cursor_border(cursored, tree_focused, selected);

    div()
        .id(row_id)
        .h(px(TREE_ROW_H))
        .mt(px(top_gap))
        .pl(px(28.0))
        .pr(px(8.0))
        .relative()
        .rounded(px(theme::RADIUS))
        .border_1()
        .border_color(row_border)
        .bg(row_bg)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
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
        .when(enabled && !selected, |row| {
            row.cursor_pointer()
                .active(|row| row.bg(theme::panel_hover()))
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
                            .tooltip(controls::text_tooltip(
                                "Move thread to the Recycle Bin",
                                None::<&str>,
                            ))
                            .hover(|button| button.bg(theme::error_wash()))
                            .active(|button| button.bg(theme::panel_hover()))
                            .focus(|button| {
                                button
                                    .bg(theme::error_wash())
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
    use std::path::PathBuf;

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

    fn project_slice(
        path: &str,
        expanded: bool,
        sessions: Vec<(&str, &str)>,
        error: Option<&str>,
        corrupt: usize,
        status: CatalogStatus,
    ) -> SidebarProjectSlice {
        SidebarProjectSlice {
            path: PathBuf::from(path),
            expanded,
            status,
            sessions: Arc::new(
                sessions
                    .into_iter()
                    .map(|(id, path)| SessionSummary::test_stub(id, PathBuf::from(path)))
                    .collect(),
            ),
            error: error.map(str::to_owned),
            corrupt_count: corrupt,
        }
    }

    #[test]
    fn sidebar_rows_flatten_projects_threads_and_notes() {
        let rows = sidebar_rows(&[
            project_slice(
                "p1",
                true,
                vec![("s1", "p1/a.jsonl"), ("s2", "p1/b.jsonl")],
                None,
                0,
                CatalogStatus::Ready,
            ),
            project_slice("p2", false, vec![], None, 0, CatalogStatus::Ready),
            project_slice(
                "p3",
                true,
                vec![],
                Some("unreadable"),
                2,
                CatalogStatus::Inaccessible,
            ),
        ]);
        let projects: Vec<_> = rows
            .iter()
            .filter_map(|row| match row {
                SidebarRow::Node(SidebarNode::Project(path)) => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            projects,
            [
                PathBuf::from("p1"),
                PathBuf::from("p2"),
                PathBuf::from("p3")
            ]
        );
        let threads: Vec<_> = rows
            .iter()
            .filter_map(|row| match row {
                SidebarRow::Node(SidebarNode::Thread { session, .. }) => Some(session.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            threads,
            [PathBuf::from("p1/a.jsonl"), PathBuf::from("p1/b.jsonl")]
        );
        // Collapsed p2 emits no note; p3 lists note, error, then corrupt count.
        assert_eq!(rows.len(), 8, "unexpected row layout: {rows:?}");
        assert!(matches!(
            &rows[5],
            SidebarRow::Note {
                note: SidebarNote::Unavailable,
                ..
            }
        ));
        assert!(matches!(
            &rows[6],
            SidebarRow::Error { message, .. } if message == "unreadable"
        ));
        assert!(matches!(
            &rows[7],
            SidebarRow::Note {
                note: SidebarNote::Corrupt(2),
                ..
            }
        ));
    }

    #[test]
    fn sidebar_cursor_skips_notes_and_clamps_at_ends() {
        let rows = sidebar_rows(&[
            project_slice(
                "p1",
                true,
                vec![("s1", "p1/a.jsonl")],
                None,
                0,
                CatalogStatus::Ready,
            ),
            project_slice("p2", true, vec![], None, 0, CatalogStatus::Loading),
            project_slice("p3", false, vec![], None, 0, CatalogStatus::Empty),
        ]);
        let first = SidebarNode::Project(PathBuf::from("p1"));
        let thread = SidebarNode::Thread {
            project: PathBuf::from("p1"),
            session: PathBuf::from("p1/a.jsonl"),
        };
        let second = SidebarNode::Project(PathBuf::from("p2"));
        let last = SidebarNode::Project(PathBuf::from("p3"));

        assert_eq!(
            sidebar_moved_cursor(&rows, None, SidebarCursorMove::Next).map(|(node, _)| node),
            Some(first.clone())
        );
        assert_eq!(
            sidebar_moved_cursor(&rows, None, SidebarCursorMove::Previous).map(|(node, _)| node),
            Some(last.clone())
        );
        // From the thread, Next must pass over the loading note row: node is
        // p2's project but the scroll slot is 2, not the note's 3.
        let (node, slot) =
            sidebar_moved_cursor(&rows, Some(&thread), SidebarCursorMove::Next).unwrap();
        assert_eq!(node, second);
        assert_eq!(slot, 2);
        // Ends clamp instead of wrapping.
        assert_eq!(
            sidebar_moved_cursor(&rows, Some(&last), SidebarCursorMove::Next).map(|(node, _)| node),
            Some(last.clone())
        );
        assert_eq!(
            sidebar_moved_cursor(&rows, Some(&first), SidebarCursorMove::Previous)
                .map(|(node, _)| node),
            Some(first.clone())
        );
        // A cursor pointing at a removed row reseeds from the edge.
        assert_eq!(
            sidebar_moved_cursor(
                &rows,
                Some(&SidebarNode::Project(PathBuf::from("gone"))),
                SidebarCursorMove::First,
            )
            .map(|(node, _)| node),
            Some(first)
        );
        assert_eq!(
            sidebar_moved_cursor(&[], Some(&last), SidebarCursorMove::First),
            None
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
