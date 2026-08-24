use super::composer_bar::{ComposerBarParams, composer_bar};
use super::conversation_panel::{ConversationAreaParams, conversation_area};
use super::inspector::subagent_dialog;
use super::inspector_drawer::{
    SessionInspectorDrawerParams, session_inspector_drawer,
};
use super::model_panels::{
    ModelSettingsPanelParams, ProviderAuthModalParams, model_settings_panel, provider_auth_modal,
};
use super::overlays::{
    PastedImageOverlayParams, activity_detail_overlay, command_palette_overlay, compaction_dialog,
    extension_dialog_overlay,
    hotkey_help_overlay, pasted_image_overlay, runtime_notification_stack,
};
use super::shell::{
    HistoryPanelParams, SessionsPanelParams, TitlebarParams, history_panel, sessions_panel,
    titlebar,
};
use super::sidebar::workspace_rail;
use super::terminal_dock::terminal_splitter;
use super::*;
use crate::views::diff_summary::diff_overlay;

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        theme::set_active(self.active_theme);
        if self.terminal_open {
            let max_terminal_height =
                (f32::from(window.viewport_size().height) - theme::TITLE_H - 210.0).max(180.0);
            self.terminal_height = self.terminal_height.clamp(180.0, max_terminal_height);
            let workspace = PathBuf::from(&self.render_projections.shell.workspace);
            let terminal_size = self.terminal_size(window);
            self.terminal.update(cx, |terminal, cx| {
                terminal.set_workspace(workspace, cx);
                terminal.resize(terminal_size, cx);
            });
        }
        let thread_statuses = self.thread_statuses();
        let projection = &self.render_projections.shell;
        let catalog = &self.render_projections.catalog;
        let history = &self.render_projections.history;
        let bridge = &self.render_projections.bridge;
        let models = &self.render_projections.models;
        let resources = &self.render_projections.resources;
        let orchestration = &self.render_projections.orchestration;
        let pasted_image_preview = self.pasted_image_preview.and_then(|index| {
            let images = self.composer.read(cx).images();
            images
                .get(index)
                .cloned()
                .map(|image| (image, index, images.len()))
        });

        div()
            .id("runtime-shell")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_connect))
            .on_action(cx.listener(Self::on_retry))
            .on_action(cx.listener(Self::on_stop))
            .on_action(cx.listener(Self::on_abort_run))
            .on_action(cx.listener(Self::on_attach_files))
            .on_action(cx.listener(Self::on_activate_recovery))
            .on_action(cx.listener(Self::on_focus_composer))
            .on_action(cx.listener(Self::on_focus_next))
            .on_action(cx.listener(Self::on_focus_previous))
            .on_action(cx.listener(Self::on_open_command_palette))
            .on_action(cx.listener(Self::on_open_app_updates))
            .on_action(cx.listener(Self::on_activate_app_update))
            .on_action(cx.listener(Self::on_show_hotkeys))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_terminal))
            .on_action(cx.listener(Self::on_toggle_inspector))
            .on_action(cx.listener(Self::on_increase_font_size))
            .on_action(cx.listener(Self::on_decrease_font_size))
            .on_action(cx.listener(Self::on_image_preview_previous))
            .on_action(cx.listener(Self::on_image_preview_next))
            .on_action(cx.listener(Self::on_image_preview_close))
            .on_action(cx.listener(Self::on_history_next))
            .on_action(cx.listener(Self::on_history_previous))
            .on_action(cx.listener(Self::on_history_first))
            .on_action(cx.listener(Self::on_history_last))
            .on_action(cx.listener(Self::on_history_fold))
            .on_action(cx.listener(Self::on_history_unfold))
            .on_action(cx.listener(Self::on_history_activate))
            .when(pasted_image_preview.is_some(), |shell| {
                shell.key_context("ImagePreview")
            })
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .font_family(theme::sans())
            .text_size(theme::text_size(theme::T_BODY))
            .text_color(theme::bone())
            .child(titlebar(
                TitlebarParams {
                    projection,
                    conversation: &self.conversation,
                    opening_thread: catalog.pending_session_file.is_some(),
                    name_composer: &self.session_name_composer,
                    rename_open: self.session_rename_open,
                    rename_enabled: matches!(
                        self.conversation.lifecycle,
                        RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
                    ) && self.conversation.pending_operation.is_none()
                        && catalog.current_session_file.is_some()
                        && !catalog.switching,
                    theme_menu_open: self.theme_menu_open,
                    sidebar_open: self.sidebar_open,
                    terminal_open: self.terminal_open,
                    inspector_open: self.session_rail_visible(),
                    workspace_diff_available: self.workspace_diff.is_some(),
                    workspace_diff_loading: self.workspace_diff_loading,
                    workspace_diff_error: self.workspace_diff_error.is_some(),
                    workspace_diff_open: self.workspace_diff_open,
                    app_update: &self.app_update,
                },
                cx,
            ))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .when(self.sidebar_mounted, |layout| {
                        layout.child(workspace_rail(
                            self.sidebar_open,
                            self.sidebar_motion_key,
                            sessions_panel(
                                SessionsPanelParams {
                                    catalog,
                                    projects: &self.projects,
                                    project_catalogs: &self.project_catalogs,
                                    thread_statuses: &thread_statuses,
                                    hovered_thread_key: self.hovered_thread_key.as_deref(),
                                    project_feedback: self.project_feedback.as_deref(),
                                    project_picker_pending: self.project_picker_pending,
                                    conversation: &self.conversation,
                                    history_open: self.history_open,
                                    sidebar_open: self.sidebar_open,
                                    cursor: self.sidebar_cursor.as_ref(),
                                    tree_focused: self.sidebar_tree_focus.is_focused(window)
                                        && !self.sidebar_tree_pointer_focus,
                                    tree_focus: &self.sidebar_tree_focus,
                                    scroll: &self.sessions_scroll,
                                },
                                cx,
                            )
                            .into_any_element(),
                        ))
                    })
                    .when(
                        self.history_open,
                        |layout| {
                            layout.child(history_panel(
                                HistoryPanelParams {
                                    projection: history,
                                    bridge,
                                    browser: &self.history,
                                    focus: &self.history_focus,
                                    label: &self.history_label_composer,
                                    import_path: &self.import_path_composer,
                                    confirmation: self.history_confirmation.as_ref(),
                                    summarize: self.summarize_navigation,
                                },
                                cx,
                            ))
                        },
                    )
                    .child(match self.model_panel {
                        Some(ModelPanel::Settings(tab)) => model_settings_panel(
                            ModelSettingsPanelParams {
                                projection: models,
                                resources,
                                tab,
                                resource_scope_filter: self.resource_scope_filter,
                                resource_state_filter: self.resource_state_filter,
                                search: &self.model_search_composer,
                                font_search: &self.font_search_composer,
                                font_catalog: &self.font_catalog,
                                font_role: self.font_role,
                                font_feedback: self.font_feedback.as_deref(),
                                app_update: &self.app_update,
                                pi_scroll: &self.pi_settings_scroll,
                            },
                            cx,
                        )
                        .into_any_element(),
                        Some(ModelPanel::Switcher) | Some(ModelPanel::Thinking) | None => div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(conversation_area(ConversationAreaParams {
                                projection: Arc::clone(&self.conversation),
                                list: Arc::clone(&self.conversation_list),
                                list_state: self.conversation_list_state.clone(),
                                transcript_cache: self.transcript_cache.clone(),
                                stream_bands: self.stream_bands.clone(),
                                activity_disclosures: self.activity_disclosures.clone(),
                                workspace_diff: self.workspace_diff.clone(),
                                workspace_diff_files_expanded: self.workspace_diff_files_expanded,
                                thread_open: self.thread_is_open(),
                                follow: self.conversation_follow.get(),
                                root: cx.entity(),
                            }))
                            .child(composer_bar(
                                ComposerBarParams {
                                    composer: &self.composer,
                                    attachment_picker_pending: self.attachment_picker_pending,
                                    models,
                                    projection,
                                    panel: self.model_panel,
                                    provider_filter: self.model_provider_filter.as_deref(),
                                    search: &self.model_search_composer,
                                    slash_commands: &self.slash_command_matches,
                                    command_selection: self.command_selection,
                                    command_scroll: &self.slash_command_scroll,
                                    file_matches: &self.file_completion_matches,
                                    file_selection: self.file_completion_selection,
                                    file_scroll: &self.file_completion_scroll,
                                    model_scroll: &self.model_switcher_scroll,
                                    provider_scroll: &self.model_provider_scroll,
                                    thinking_scroll: &self.thinking_select_scroll,
                                    slash_dismissed: self.dismissed_slash_draft.as_deref()
                                        == Some(self.composer.read(cx).draft()),
                                    extension_ui: &self.extension_ui,
                                },
                                window,
                                cx,
                            ))
                            .when(self.terminal_open, |column| {
                                column
                                    .child(terminal_splitter(
                                        cx.entity(),
                                        self.terminal_drag_origin.is_some(),
                                        window.viewport_size().height,
                                    ))
                                    .child(
                                        div()
                                            .h(px(self.terminal_height))
                                            .min_h(px(180.0))
                                            .flex_shrink_0()
                                            .child(self.terminal.clone()),
                                    )
                            })
                            .into_any_element(),
                    }),
            )
            .when(self.inspector_mounted, |shell| {
                shell.child(session_inspector_drawer(
                    SessionInspectorDrawerParams {
                        open: self.inspector_open,
                        motion_key: self.inspector_motion_key,
                        projection,
                        conversation: &self.conversation,
                        orchestration,
                        selected_task_id: self.selected_task_id.as_deref(),
                        goal_edit_composer: &self.goal_edit_composer,
                        delivery_focus: self.delivery_focus,
                        usage_tooltip_hovered: self.usage_tooltip_hovered,
                        usage_tooltip_visible: self.usage_tooltip_visible,
                        usage_tooltip_epoch: self.usage_tooltip_epoch,
                        inspector_focus: &self.inspector_focus,
                    },
                    cx,
                ))
            })
            .when_some(self.activity_detail.clone(), |shell, detail| {
                shell.child(activity_detail_overlay(
                    &detail,
                    &self.activity_detail_focus,
                    &self.activity_detail_scroll,
                    cx,
                ))
            })
            .when_some(pasted_image_preview, |shell, (image, index, count)| {
                shell.child(pasted_image_overlay(
                    PastedImageOverlayParams {
                        prompt_image: &image,
                        index,
                        count,
                        pencil_enabled: self.pencil_enabled,
                        pencil_color: self.pencil_color,
                        pencil_size: self.pencil_size,
                        pencil_stroke: self.pencil_stroke.clone(),
                        can_undo: !self.pencil_undo.is_empty(),
                        pencil_error: self.pencil_error.as_deref(),
                    },
                    cx,
                ))
            })
            .when(self.compaction_modal_open, |shell| {
                shell.child(compaction_dialog(&self.compaction_composer, cx))
            })
            .when(self.command_palette_open, |shell| {
                shell.child(command_palette_overlay(
                    &self.command_palette_matches,
                    &self.command_search_composer,
                    self.command_selection,
                    &self.command_palette_scroll,
                    &self.command_palette_focus,
                    cx,
                ))
            })
            .when(self.hotkey_help_open, |shell| {
                shell.child(hotkey_help_overlay(&self.hotkey_help_focus, cx))
            })
            .when(!self.runtime_notifications.is_empty(), |shell| {
                shell.child(runtime_notification_stack(&self.runtime_notifications, cx))
            })
            .when_some(
                self.workspace_diff_open
                    .then(|| self.workspace_diff.clone())
                    .flatten(),
                |shell, snapshot| {
                    shell.child(diff_overlay(
                        &snapshot,
                        self.workspace_diff_selected,
                        &self.workspace_diff_collapsed_folders,
                        &self.workspace_diff_focus,
                        &self.workspace_diff_files_scroll,
                        &self.workspace_diff_scroll,
                        cx,
                    ))
                },
            )
            .when_some(self.selected_subagent_id.clone(), |shell, agent_id| {
                shell.child(subagent_dialog(
                    orchestration
                        .snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.subagent(&agent_id)),
                    &agent_id,
                    &self.subagent_dialog_focus,
                    &self.subagent_dialog_scroll,
                    &self.subagent_composer,
                    &self.transcript_cache,
                    cx,
                ))
            })
            .when_some(self.extension_ui.active_dialog.clone(), |shell, dialog| {
                shell.child(extension_dialog_overlay(
                    &dialog,
                    self.extension_ui.queued_dialogs,
                    self.extension_dialog_selection,
                    &self.extension_dialog_focus,
                    &self.extension_input_composer,
                    &self.extension_editor_composer,
                    cx,
                ))
            })
            .when_some(models.auth.as_ref(), |shell, auth| {
                shell.child(provider_auth_modal(
                    ProviderAuthModalParams {
                        auth,
                        provider_name: models
                            .catalog
                            .as_ref()
                            .and_then(|catalog| {
                                catalog
                                    .providers
                                    .iter()
                                    .find(|provider| provider.id == auth.provider)
                                    .map(|provider| provider.name.as_str())
                            })
                            .unwrap_or(&auth.provider),
                        auth_input: &self.auth_input_composer,
                        auth_secret: &self.auth_secret_composer,
                        focus: &self.provider_auth_focus,
                        browser_retry_url: self
                            .last_auth_browser_launch
                            .as_ref()
                            .filter(|(operation, _)| *operation == auth.operation)
                            .map(|(_, url)| url.as_str()),
                        browser_feedback: self
                            .auth_browser_feedback
                            .as_ref()
                            .map(|(message, tone)| (message.as_str(), *tone)),
                        provider_feedback: models.feedback.as_deref(),
                    },
                    cx,
                ))
            })
    }
}
