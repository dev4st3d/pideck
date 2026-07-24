//! Live Pi shell with an authoritative streaming conversation.

use std::cell::Cell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    ClipboardItem, Context, DispatchPhase, Entity, FocusHandle, Focusable, FontWeight, IntoElement,
    ListAlignment, ListOffset, ListState, Render, ScrollHandle, ScrollWheelEvent, Subscription,
    Task, Window, canvas, div, list, prelude::*, px,
};

use crate::actions::{
    AbortRun, ActivateRecovery, Connect, FocusNext, FocusPrevious, HistoryActivate, HistoryFirst,
    HistoryFold, HistoryLast, HistoryNext, HistoryPrevious, HistoryUnfold,
    ORCHESTRATION_ROW_CONTEXT, OpenCommandPalette, OrchestrationActivate, Retry, ShowHotkeys, Stop,
};
use crate::command_catalog::{
    CommandCatalog, CommandEntry, CommandTarget, InvocationResolution, NativeAction,
};
use crate::controller::{
    AcceptedSubmission, AcceptedSubmissionKind, BridgeProjection, CatalogProjection, CatalogStatus,
    ComposerRuntime, ConversationProjection, ExtensionUiProjection, HistoryProjection,
    ModelRuntimeProjection, OrchestrationProjection, ResourceCenterProjection, RuntimeController,
    SubmissionPreference,
};
use crate::model_runtime::{
    AuthMethod, AuthPromptKind, AuthStage, CatalogPhase, ModelCatalogEntry, ModelChangePolicy,
    ModelIdentity, ThinkingLevel,
};
use crate::orchestration::{
    GoalItemSnapshot, OrchestrationAction, OrchestrationPhase, SubagentSnapshot, SubagentStatus,
    TaskSnapshot, TaskStatus, TranscriptRole,
};
use crate::resource_center::{
    ResourceLoadState, ResourcePhase, ResourceScopeFilter, ResourceStateFilter,
};
use crate::state::history::{HistoryBrowser, HistoryFilter};
use crate::state::runtime::{
    BashStatus, CompactionState, DialogAnswer, DialogRequest, MessageBlock, MessageRole,
    ModelSummary, NotificationKind, PromptDelivery, PromptImage, QueueContents, QueueDeliveryMode,
    RetryState, RuntimeLifecycle, RuntimeNotification, RuntimeOperation, SubmissionKind,
    WidgetPlacement, sanitize_untrusted_text,
};
use crate::state::{RecoveryAction, ShellProjection};
use crate::theme;
use crate::views::composer::{Composer, ComposerAvailability, ComposerEvent, ComposerFeedback};
use crate::views::controls;
use crate::views::conversation::{
    ConversationListModel, ConversationScrollMotion, TranscriptTextCache,
};

struct PendingDraft {
    request: crate::services::rpc::RequestId,
    text: String,
}

const MAX_VISIBLE_RUNTIME_NOTIFICATIONS: usize = 3;
type RootClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum HistoryConfirmation {
    Navigate(crate::services::rpc::EntryId),
    Fork(crate::services::rpc::EntryId),
    Clone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelPanel {
    /// Attached model list above the prompt box.
    Switcher,
    /// Thinking effort select menu on the prompt chrome.
    Thinking,
    /// Full center settings workspace.
    Settings(ModelSettingsTab),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelSettingsTab {
    Providers,
    Models,
    Thinking,
    Usage,
    Resources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtensionDialogKey {
    Cancel,
    ContainFocus,
    Move(isize),
    AcceptSelection,
}

pub struct RootView {
    controller: Entity<RuntimeController>,
    composer: Entity<Composer>,
    compaction_composer: Entity<Composer>,
    session_name_composer: Entity<Composer>,
    history_search_composer: Entity<Composer>,
    history_label_composer: Entity<Composer>,
    import_path_composer: Entity<Composer>,
    model_search_composer: Entity<Composer>,
    command_search_composer: Entity<Composer>,
    auth_input_composer: Entity<Composer>,
    auth_secret_composer: Entity<Composer>,
    extension_input_composer: Entity<Composer>,
    extension_editor_composer: Entity<Composer>,
    subagent_composer: Entity<Composer>,
    goal_edit_composer: Entity<Composer>,
    model_panel: Option<ModelPanel>,
    resource_scope_filter: ResourceScopeFilter,
    resource_state_filter: ResourceStateFilter,
    command_palette_open: bool,
    hotkey_help_open: bool,
    command_selection: usize,
    command_palette_scroll: ScrollHandle,
    slash_command_scroll: ScrollHandle,
    runtime_notifications: VecDeque<RuntimeNotification>,
    dismissed_slash_draft: Option<String>,
    last_slash_draft: String,
    active_auth_prompt_id: Option<String>,
    extension_ui: ExtensionUiProjection,
    active_extension_dialog_id: Option<crate::services::rpc::RequestId>,
    extension_dialog_selection: usize,
    extension_dialog_focus: FocusHandle,
    extension_dialog_timeout_task: Option<Task<()>>,
    selected_task_id: Option<String>,
    selected_subagent_id: Option<String>,
    subagent_dialog_focus: FocusHandle,
    subagent_dialog_scroll: ScrollHandle,
    window_title: String,
    history: HistoryBrowser,
    history_focus: FocusHandle,
    history_open: bool,
    history_confirmation: Option<HistoryConfirmation>,
    summarize_navigation: bool,
    conversation: Arc<ConversationProjection>,
    conversation_list: Arc<ConversationListModel>,
    conversation_list_state: ListState,
    conversation_follow: Rc<Cell<bool>>,
    conversation_scroll_motion: ConversationScrollMotion,
    transcript_cache: Entity<TranscriptTextCache>,
    pending_draft: Option<PendingDraft>,
    pending_bash: Option<crate::services::rpc::RequestId>,
    pending_compaction_focus: Option<String>,
    pending_session_name: Option<String>,
    retry_tick_task: Option<Task<()>>,
    focus_handle: FocusHandle,
    _controller_observation: Subscription,
    _composer_subscription: Subscription,
    _compaction_subscription: Subscription,
    _session_name_subscription: Subscription,
    _history_search_observation: Subscription,
    _history_label_subscription: Subscription,
    _import_path_subscription: Subscription,
    _model_search_observation: Subscription,
    _composer_observation: Subscription,
    _command_search_observation: Subscription,
    _command_search_subscription: Subscription,
    _auth_input_subscription: Subscription,
    _auth_secret_subscription: Subscription,
    _extension_input_subscription: Subscription,
    _extension_editor_subscription: Subscription,
    _subagent_subscription: Subscription,
    _goal_edit_subscription: Subscription,
}

impl RootView {
    pub fn new(
        window: &mut Window,
        controller: Entity<RuntimeController>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let composer = cx.new(Composer::new);
        let compaction_composer = cx.new(|cx| {
            Composer::scoped(
                "compaction-focus",
                "Optional summary focus instructions…",
                "Compact",
                cx,
            )
        });
        let session_name_composer =
            cx.new(|cx| Composer::field("session-name", "Rename session…", "Rename", cx));
        let history_search_composer =
            cx.new(|cx| Composer::field("history-search", "Search history…", "", cx));
        let history_label_composer =
            cx.new(|cx| Composer::field("history-label", "Label selected entry…", "Set", cx));
        let import_path_composer =
            cx.new(|cx| Composer::field("import-jsonl", "JSONL path to import…", "Import", cx));
        let model_search_composer = cx.new(|cx| {
            Composer::field("model-search", "Search models…", "", cx).with_field_height(30.0)
        });
        let command_search_composer = cx.new(|cx| {
            Composer::field("command-search", "Search commands…", "", cx).with_field_height(34.0)
        });
        let auth_input_composer = cx.new(|cx| {
            Composer::field(
                "provider-auth-input",
                "Provider response...",
                "Continue",
                cx,
            )
        });
        let auth_secret_composer = cx.new(|cx| {
            Composer::secret_field("provider-auth-secret", "Credential...", "Continue", cx)
        });
        let extension_input_composer = cx
            .new(|cx| Composer::field("extension-dialog-input", "Enter a value...", "Submit", cx));
        let extension_editor_composer =
            cx.new(|cx| Composer::scoped("extension-dialog-editor", "Edit text...", "Submit", cx));
        let subagent_composer = cx.new(|cx| {
            Composer::scoped(
                "subagent-message",
                "Steer this agent or resume it with a new prompt…",
                "Send",
                cx,
            )
        });
        let goal_edit_composer = cx.new(|cx| {
            Composer::field(
                "goal-objective",
                "Edit the active goal objective…",
                "Update",
                cx,
            )
        });
        let history_focus = cx.focus_handle();
        let extension_dialog_focus = cx.focus_handle();
        let subagent_dialog_focus = cx.focus_handle();
        let conversation = controller.read(cx).conversation_projection();
        let conversation_list = ConversationListModel::new(&conversation);
        let conversation_list_state = ListState::new(
            conversation_list.item_count(),
            ListAlignment::Top,
            px(800.0),
        );
        let conversation_follow = Rc::new(Cell::new(true));
        conversation_list_state.set_scroll_handler({
            let conversation_follow = Rc::clone(&conversation_follow);
            move |event, _, _| {
                conversation_follow.set(event.visible_range.end >= event.count);
            }
        });
        conversation_list_state.scroll_to(ListOffset {
            item_ix: conversation_list.item_count(),
            offset_in_item: px(0.0),
        });
        let transcript_cache = cx.new(|_| TranscriptTextCache::new(conversation.epoch));
        let extension_ui = controller.read(cx).extension_ui_projection();
        window.focus(&composer.read(cx).focus_handle(cx));
        let controller_observation = cx.observe_in(&controller, window, |view, _, window, cx| {
            view.sync_runtime(window, cx)
        });
        let composer_subscription =
            cx.subscribe_in(&composer, window, |view, _, event, window, cx| {
                view.on_composer_event(event, window, cx)
            });
        let compaction_subscription =
            cx.subscribe_in(&compaction_composer, window, |view, _, event, _, cx| {
                view.on_compaction_event(event, cx)
            });
        let session_name_subscription =
            cx.subscribe_in(&session_name_composer, window, |view, _, event, _, cx| {
                view.on_session_name_event(event, cx)
            });
        let history_search_observation =
            cx.observe_in(&history_search_composer, window, |view, _, _, cx| {
                let query = view.history_search_composer.read(cx).draft().to_owned();
                view.history.set_query(query);
                cx.notify();
            });
        let history_label_subscription =
            cx.subscribe_in(&history_label_composer, window, |view, _, event, _, cx| {
                view.on_history_label_event(event, cx)
            });
        let import_path_subscription =
            cx.subscribe_in(&import_path_composer, window, |view, _, event, _, cx| {
                view.on_import_path_event(event, cx)
            });
        let model_search_observation =
            cx.observe_in(&model_search_composer, window, |_, _, _, cx| cx.notify());
        let composer_observation = cx.observe_in(&composer, window, |view, _, _, cx| {
            view.sync_slash_completion(cx)
        });
        let command_search_observation =
            cx.observe_in(&command_search_composer, window, |view, _, _, cx| {
                view.command_selection = 0;
                cx.notify();
            });
        let command_search_subscription = cx.subscribe_in(
            &command_search_composer,
            window,
            |view, _, event, window, cx| view.on_palette_composer_event(event, window, cx),
        );
        let auth_input_subscription =
            cx.subscribe_in(&auth_input_composer, window, |view, _, event, _, cx| {
                view.on_auth_input_event(event, false, cx)
            });
        let auth_secret_subscription =
            cx.subscribe_in(&auth_secret_composer, window, |view, _, event, _, cx| {
                view.on_auth_input_event(event, true, cx)
            });
        let extension_input_subscription = cx.subscribe_in(
            &extension_input_composer,
            window,
            |view, _, event, window, cx| view.on_extension_composer_event(event, false, window, cx),
        );
        let extension_editor_subscription = cx.subscribe_in(
            &extension_editor_composer,
            window,
            |view, _, event, window, cx| view.on_extension_composer_event(event, true, window, cx),
        );
        let subagent_subscription =
            cx.subscribe_in(&subagent_composer, window, |view, _, event, window, cx| {
                view.on_subagent_composer_event(event, window, cx)
            });
        let goal_edit_subscription =
            cx.subscribe_in(&goal_edit_composer, window, |view, _, event, _, cx| {
                view.on_goal_edit_event(event, cx)
            });
        window.set_window_title("Pi GUI");
        Self {
            controller,
            composer,
            compaction_composer,
            session_name_composer,
            history_search_composer,
            history_label_composer,
            import_path_composer,
            model_search_composer,
            command_search_composer,
            auth_input_composer,
            auth_secret_composer,
            extension_input_composer,
            extension_editor_composer,
            subagent_composer,
            goal_edit_composer,
            model_panel: None,
            resource_scope_filter: ResourceScopeFilter::All,
            resource_state_filter: ResourceStateFilter::All,
            command_palette_open: false,
            hotkey_help_open: false,
            command_selection: 0,
            command_palette_scroll: ScrollHandle::new(),
            slash_command_scroll: ScrollHandle::new(),
            runtime_notifications: VecDeque::new(),
            dismissed_slash_draft: None,
            last_slash_draft: String::new(),
            active_auth_prompt_id: None,
            extension_ui,
            active_extension_dialog_id: None,
            extension_dialog_selection: 0,
            extension_dialog_focus,
            extension_dialog_timeout_task: None,
            selected_task_id: None,
            selected_subagent_id: None,
            subagent_dialog_focus,
            subagent_dialog_scroll: ScrollHandle::new(),
            window_title: "Pi GUI".to_owned(),
            history: HistoryBrowser::default(),
            history_focus,
            history_open: false,
            history_confirmation: None,
            summarize_navigation: false,
            conversation: Arc::new(conversation),
            conversation_list: Arc::new(conversation_list),
            conversation_list_state,
            conversation_follow,
            conversation_scroll_motion: ConversationScrollMotion::default(),
            transcript_cache,
            pending_draft: None,
            pending_bash: None,
            pending_compaction_focus: None,
            pending_session_name: None,
            retry_tick_task: None,
            focus_handle,
            _controller_observation: controller_observation,
            _composer_subscription: composer_subscription,
            _compaction_subscription: compaction_subscription,
            _session_name_subscription: session_name_subscription,
            _history_search_observation: history_search_observation,
            _history_label_subscription: history_label_subscription,
            _import_path_subscription: import_path_subscription,
            _model_search_observation: model_search_observation,
            _composer_observation: composer_observation,
            _command_search_observation: command_search_observation,
            _command_search_subscription: command_search_subscription,
            _auth_input_subscription: auth_input_subscription,
            _auth_secret_subscription: auth_secret_subscription,
            _extension_input_subscription: extension_input_subscription,
            _extension_editor_subscription: extension_editor_subscription,
            _subagent_subscription: subagent_subscription,
            _goal_edit_subscription: goal_edit_subscription,
        }
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        self.controller
            .update(cx, |controller, cx| controller.connect(cx));
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.controller
            .update(cx, |controller, cx| controller.stop(cx));
    }

    fn activate_recovery(&mut self, action: RecoveryAction, cx: &mut Context<Self>) {
        match action {
            RecoveryAction::Connect | RecoveryAction::Retry => self.connect(cx),
            RecoveryAction::Stop => self.stop(cx),
        }
    }

    fn on_connect(&mut self, _: &Connect, _: &mut Window, cx: &mut Context<Self>) {
        self.connect(cx);
    }

    fn on_retry(&mut self, _: &Retry, _: &mut Window, cx: &mut Context<Self>) {
        self.connect(cx);
    }

    fn on_stop(&mut self, _: &Stop, _: &mut Window, cx: &mut Context<Self>) {
        self.stop(cx);
    }

    fn on_abort_run(&mut self, _: &AbortRun, window: &mut Window, cx: &mut Context<Self>) {
        if self.cancel_extension_dialog(window, cx) {
            return;
        }
        let _ = self.execute_native_action(NativeAction::Abort, "", window, cx);
    }

    fn on_composer_event(
        &mut self,
        event: &ComposerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComposerEvent::Accept { text, images } => self.execute_composer_text(
                text.clone(),
                images.clone(),
                SubmissionPreference::Default,
                window,
                cx,
            ),
            ComposerEvent::FollowUp { text, images } => self.execute_composer_text(
                text.clone(),
                images.clone(),
                SubmissionPreference::FollowUp,
                window,
                cx,
            ),
            ComposerEvent::Abort => {
                if self.hotkey_help_open {
                    self.hotkey_help_open = false;
                    window.focus(&self.composer.read(cx).focus_handle(cx));
                    cx.notify();
                } else {
                    let _ = self.execute_native_action(NativeAction::Abort, "", window, cx);
                }
            }
            ComposerEvent::AbortBash => {
                let _ = self.execute_native_action(NativeAction::Abort, "", window, cx);
            }
            ComposerEvent::CommandNext => self.move_command_selection(1, false, cx),
            ComposerEvent::CommandPrevious => self.move_command_selection(-1, false, cx),
            ComposerEvent::CommandAccept => self.accept_slash_completion(window, cx),
            ComposerEvent::CommandDismiss => {
                self.dismissed_slash_draft = Some(self.composer.read(cx).draft().to_owned());
                self.composer.update(cx, |composer, cx| {
                    composer.set_command_completion_active(false, cx)
                });
            }
        }
    }

    fn command_catalog(&self, cx: &Context<Self>) -> CommandCatalog {
        let projection = self.controller.read(cx).command_catalog_projection();
        CommandCatalog::build(&projection.status, &projection.commands)
    }

    fn sync_slash_completion(&mut self, cx: &mut Context<Self>) {
        let draft = self.composer.read(cx).draft().to_owned();
        if self.last_slash_draft != draft {
            self.command_selection = 0;
            self.slash_command_scroll.scroll_to_item(0);
            self.last_slash_draft.clone_from(&draft);
        }
        if self
            .dismissed_slash_draft
            .as_deref()
            .is_some_and(|dismissed| dismissed != draft)
        {
            self.dismissed_slash_draft = None;
        }
        let active = !self.composer.read(cx).has_images()
            && self.dismissed_slash_draft.as_deref() != Some(draft.as_str())
            && self
                .command_catalog(cx)
                .slash_completion(&draft)
                .is_some_and(|completion| completion.intercept_enter);
        if active {
            let count = self
                .command_catalog(cx)
                .slash_completion(&draft)
                .map_or(0, |completion| completion.matches.len());
            self.command_selection = self.command_selection.min(count.saturating_sub(1));
        } else {
            self.command_selection = 0;
        }
        self.composer.update(cx, |composer, cx| {
            composer.set_command_completion_active(active, cx)
        });
        cx.notify();
    }

    fn move_command_selection(&mut self, delta: isize, palette: bool, cx: &mut Context<Self>) {
        let count = if palette {
            let query = self.command_search_composer.read(cx).draft();
            self.command_catalog(cx).filtered(query).len()
        } else {
            let draft = self.composer.read(cx).draft();
            self.command_catalog(cx)
                .slash_completion(draft)
                .map_or(0, |completion| completion.matches.len())
        };
        if count == 0 {
            self.command_selection = 0;
            return;
        }
        self.command_selection =
            (self.command_selection as isize + delta).rem_euclid(count as isize) as usize;
        if !palette {
            self.slash_command_scroll
                .scroll_to_item(self.command_selection);
        }
        cx.notify();
    }

    fn accept_slash_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.composer.read(cx).draft().to_owned();
        let catalog = self.command_catalog(cx);
        let Some(completion) = catalog.slash_completion(&draft) else {
            return;
        };
        let Some(entry) = completion
            .matches
            .get(self.command_selection)
            .cloned()
            .cloned()
        else {
            return;
        };
        self.choose_command_entry(entry, window, cx);
    }

    fn choose_command_entry(
        &mut self,
        entry: CommandEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entry.argument_hint.is_some() {
            self.command_palette_open = false;
            self.composer.update(cx, |composer, cx| {
                composer.set_draft(&format!("/{} ", entry.name), cx);
                composer.set_command_completion_active(false, cx);
            });
            self.dismissed_slash_draft = Some(self.composer.read(cx).draft().to_owned());
            window.focus(&self.composer.read(cx).focus_handle(cx));
        } else {
            self.execute_entry(entry, String::new(), window, cx);
        }
    }

    fn execute_composer_text(
        &mut self,
        text: String,
        images: Vec<PromptImage>,
        preference: SubmissionPreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !images.is_empty() {
            self.submit(text, images, preference, cx);
            return;
        }
        let catalog = self.command_catalog(cx);
        match catalog.resolve(&text) {
            InvocationResolution::Command { entry, invocation } => {
                let entry = entry.clone();
                self.execute_entry_with_preference(
                    entry,
                    invocation.arguments,
                    preference,
                    window,
                    cx,
                );
            }
            InvocationResolution::UnsupportedBuiltin(name) => {
                self.command_error(
                    format!(
                        "/{name} is a TUI-only command and cannot run in the native RPC client."
                    ),
                    window,
                    cx,
                );
            }
            InvocationResolution::NotACommand => self.submit(text, images, preference, cx),
        }
    }

    fn execute_entry(
        &mut self,
        entry: CommandEntry,
        arguments: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.execute_entry_with_preference(
            entry,
            arguments,
            SubmissionPreference::Default,
            window,
            cx,
        );
    }

    fn execute_entry_with_preference(
        &mut self,
        entry: CommandEntry,
        arguments: String,
        preference: SubmissionPreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !entry.enabled {
            self.command_error(
                entry
                    .disabled_reason
                    .unwrap_or_else(|| "That command is unavailable.".to_owned()),
                window,
                cx,
            );
            return;
        }
        self.close_command_palette(window, cx);
        match entry.target {
            CommandTarget::Native(action) => {
                match self.execute_native_action(action, &arguments, window, cx) {
                    Ok(()) => {
                        self.composer
                            .update(cx, |composer, cx| composer.set_draft("", cx));
                        if !matches!(
                            action,
                            NativeAction::Model
                                | NativeAction::Sessions
                                | NativeAction::Tree
                                | NativeAction::Fork
                                | NativeAction::Clone
                                | NativeAction::Settings
                                | NativeAction::Hotkeys
                        ) {
                            window.focus(&self.composer.read(cx).focus_handle(cx));
                        }
                    }
                    Err(error) => self.command_error(error, window, cx),
                }
            }
            CommandTarget::Dynamic(source) => {
                let text = entry.invocation(&arguments);
                self.submit_dynamic(text, source, preference, window, cx);
            }
        }
    }

    fn submit(
        &mut self,
        text: String,
        images: Vec<PromptImage>,
        preference: SubmissionPreference,
        cx: &mut Context<Self>,
    ) {
        if self.pending_draft.is_some() {
            self.composer.update(cx, |composer, cx| {
                composer.set_feedback(
                    ComposerFeedback::Rejected(
                        "The previous acceptance is still pending.".to_owned(),
                    ),
                    cx,
                );
            });
            return;
        }

        let result = self.controller.update(cx, |controller, cx| {
            controller.submit_with_images(text.clone(), images.clone(), preference, cx)
        });
        match result {
            Ok(AcceptedSubmission {
                request,
                kind: AcceptedSubmissionKind::Prompt(kind),
            }) => {
                self.pending_draft = Some(PendingDraft { request, text });
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(ComposerFeedback::Pending(kind), cx)
                });
            }
            Ok(AcceptedSubmission {
                request,
                kind:
                    AcceptedSubmissionKind::Bash {
                        exclude_from_context,
                    },
            }) => {
                self.pending_bash = Some(request);
                self.composer.update(cx, |composer, cx| {
                    composer.clear_bash_accepted(&text, exclude_from_context, cx);
                });
            }
            Err(rejection) => {
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(
                        ComposerFeedback::Rejected(rejection.message().to_owned()),
                        cx,
                    )
                });
            }
        }
    }

    fn submit_dynamic(
        &mut self,
        text: String,
        source: crate::state::runtime::CommandSource,
        preference: SubmissionPreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_draft.is_some() {
            self.command_error(
                "The previous acceptance is still pending.".to_owned(),
                window,
                cx,
            );
            return;
        }
        let result = self.controller.update(cx, |controller, cx| {
            controller.invoke_dynamic_command(text.clone(), source, preference, cx)
        });
        match result {
            Ok(AcceptedSubmission {
                request,
                kind: AcceptedSubmissionKind::Prompt(kind),
            }) => {
                self.pending_draft = Some(PendingDraft { request, text });
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(ComposerFeedback::Pending(kind), cx)
                });
            }
            Ok(AcceptedSubmission {
                kind: AcceptedSubmissionKind::Bash { .. },
                ..
            }) => unreachable!("dynamic commands always use prompt transport"),
            Err(rejection) => {
                self.command_error(rejection.message().to_owned(), window, cx);
            }
        }
    }

    fn command_error(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |composer, cx| {
            composer.set_feedback(ComposerFeedback::Rejected(message), cx)
        });
        window.focus(&self.composer.read(cx).focus_handle(cx));
    }

    fn dismiss_runtime_notification(&mut self, index: usize, cx: &mut Context<Self>) {
        self.runtime_notifications.remove(index);
        cx.notify();
    }

    fn collect_runtime_notifications(&mut self, cx: &mut Context<Self>) {
        let notifications = self
            .controller
            .update(cx, |controller, _| controller.take_runtime_notifications());
        if notifications.is_empty() {
            return;
        }
        for notification in notifications {
            self.runtime_notifications.push_back(notification);
            while self.runtime_notifications.len() > MAX_VISIBLE_RUNTIME_NOTIFICATIONS {
                self.runtime_notifications.pop_front();
            }
        }
        cx.notify();
    }

    fn answer_extension_dialog(
        &mut self,
        answer: DialogAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.extension_ui.active_dialog.as_ref() else {
            return;
        };
        let request = dialog.id.clone();
        let answered = self.controller.update(cx, |controller, cx| {
            controller.answer_dialog(request, answer, cx)
        });
        if answered {
            self.extension_dialog_timeout_task = None;
            window.focus(&self.composer.read(cx).focus_handle(cx));
        }
    }

    fn cancel_extension_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.extension_ui.active_dialog.is_none() {
            return false;
        }
        self.answer_extension_dialog(DialogAnswer::Cancelled, window, cx);
        true
    }

    fn on_extension_composer_event(
        &mut self,
        event: &ComposerEvent,
        editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComposerEvent::Accept { text, .. } => {
                let expected = if editor {
                    matches!(
                        self.extension_ui
                            .active_dialog
                            .as_ref()
                            .map(|dialog| &dialog.request),
                        Some(DialogRequest::Editor { .. })
                    )
                } else {
                    matches!(
                        self.extension_ui
                            .active_dialog
                            .as_ref()
                            .map(|dialog| &dialog.request),
                        Some(DialogRequest::Input { .. })
                    )
                };
                if expected {
                    self.answer_extension_dialog(DialogAnswer::Value(text.clone()), window, cx);
                }
            }
            ComposerEvent::Abort | ComposerEvent::AbortBash => {
                self.cancel_extension_dialog(window, cx);
            }
            ComposerEvent::FollowUp { .. }
            | ComposerEvent::CommandNext
            | ComposerEvent::CommandPrevious
            | ComposerEvent::CommandAccept
            | ComposerEvent::CommandDismiss => {}
        }
    }

    fn move_extension_dialog_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = match self
            .extension_ui
            .active_dialog
            .as_ref()
            .map(|dialog| &dialog.request)
        {
            Some(DialogRequest::Select { options, .. }) => options.len(),
            Some(DialogRequest::Confirm { .. }) => 2,
            _ => 0,
        };
        if count == 0 {
            return;
        }
        self.extension_dialog_selection =
            (self.extension_dialog_selection as isize + delta).rem_euclid(count as isize) as usize;
        cx.notify();
    }

    fn accept_extension_dialog_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let answer = match self
            .extension_ui
            .active_dialog
            .as_ref()
            .map(|dialog| &dialog.request)
        {
            Some(DialogRequest::Select { options, .. }) => options
                .get(self.extension_dialog_selection)
                .cloned()
                .map(DialogAnswer::Value),
            Some(DialogRequest::Confirm { .. }) => Some(DialogAnswer::Confirmed(
                self.extension_dialog_selection == 1,
            )),
            _ => None,
        };
        if let Some(answer) = answer {
            self.answer_extension_dialog(answer, window, cx);
        }
    }

    fn on_extension_dialog_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dialog_kind = self
            .extension_ui
            .active_dialog
            .as_ref()
            .map(|dialog| dialog.kind());
        match extension_dialog_key(dialog_kind, event.keystroke.key.as_str()) {
            Some(ExtensionDialogKey::Cancel) => {
                cx.stop_propagation();
                self.cancel_extension_dialog(window, cx);
            }
            Some(ExtensionDialogKey::ContainFocus) => {
                cx.stop_propagation();
                self.focus_active_extension_dialog(window, cx);
            }
            Some(ExtensionDialogKey::Move(delta)) => {
                cx.stop_propagation();
                self.move_extension_dialog_selection(delta, cx);
            }
            Some(ExtensionDialogKey::AcceptSelection) => {
                cx.stop_propagation();
                self.accept_extension_dialog_selection(window, cx);
            }
            None => {}
        }
    }

    fn focus_active_extension_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.extension_ui.active_dialog.as_ref() else {
            return;
        };
        match dialog.request {
            DialogRequest::Input { .. } => {
                window.focus(&self.extension_input_composer.read(cx).focus_handle(cx));
            }
            DialogRequest::Editor { .. } => {
                window.focus(&self.extension_editor_composer.read(cx).focus_handle(cx));
            }
            DialogRequest::Select { .. } | DialogRequest::Confirm { .. } => {
                window.focus(&self.extension_dialog_focus);
            }
        }
    }

    fn sync_extension_ui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let projection = self.controller.read(cx).extension_ui_projection();
        let dialog_id = projection
            .active_dialog
            .as_ref()
            .map(|dialog| dialog.id.clone());
        let dialog_changed = dialog_id != self.active_extension_dialog_id;
        self.extension_ui = projection;

        let title = self
            .extension_ui
            .title
            .as_deref()
            .map(single_line_title)
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "Pi GUI".to_owned());
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }

        if !dialog_changed {
            return;
        }
        self.active_extension_dialog_id = dialog_id;
        self.extension_dialog_selection = 0;
        self.extension_dialog_timeout_task = None;
        let Some(dialog) = self.extension_ui.active_dialog.clone() else {
            window.focus(&self.composer.read(cx).focus_handle(cx));
            return;
        };

        self.command_palette_open = false;
        self.hotkey_help_open = false;
        self.model_panel = None;
        match &dialog.request {
            DialogRequest::Input { placeholder, .. } => {
                let placeholder = placeholder
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .unwrap_or("Enter a value...")
                    .to_owned();
                self.extension_input_composer
                    .update(cx, move |composer, cx| {
                        composer.set_draft("", cx);
                        composer.set_placeholder(placeholder, cx);
                        composer.set_availability(ComposerAvailability::Idle, cx);
                    });
            }
            DialogRequest::Editor { prefill, .. } => {
                self.extension_editor_composer.update(cx, |composer, cx| {
                    composer.set_draft(prefill.as_deref().unwrap_or_default(), cx);
                    composer.set_availability(ComposerAvailability::Idle, cx);
                });
            }
            DialogRequest::Select { .. } | DialogRequest::Confirm { .. } => {}
        }
        self.focus_active_extension_dialog(window, cx);

        if let Some(deadline) = dialog.deadline {
            let request = dialog.id;
            let delay = deadline.saturating_duration_since(std::time::Instant::now());
            self.extension_dialog_timeout_task = Some(cx.spawn(async move |view, cx| {
                cx.background_executor().timer(delay).await;
                let _ = view.update(cx, |view, cx| {
                    view.controller.update(cx, |controller, cx| {
                        controller.expire_dialog(request, cx);
                    });
                });
            }));
        }
    }

    fn execute_native_action(
        &mut self,
        action: NativeAction,
        raw_arguments: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let arguments = raw_arguments.trim();
        match action {
            NativeAction::Model => {
                self.show_model_panel(ModelPanel::Switcher, window, cx);
                Ok(())
            }
            NativeAction::NewSession => self
                .controller
                .update(cx, |controller, cx| controller.new_session(cx))
                .then_some(())
                .ok_or_else(|| "A new session cannot start in the current state.".to_owned()),
            NativeAction::Sessions => {
                window.focus(&self.session_name_composer.read(cx).focus_handle(cx));
                Ok(())
            }
            NativeAction::Tree => {
                self.history_open = !self.history_open;
                self.history_confirmation = None;
                if self.history_open {
                    window.focus(&self.history_focus);
                } else {
                    window.focus(&self.composer.read(cx).focus_handle(cx));
                }
                cx.notify();
                Ok(())
            }
            NativeAction::Fork => {
                self.history_open = true;
                self.history_confirmation = None;
                window.focus(&self.history_focus);
                cx.notify();
                Ok(())
            }
            NativeAction::Clone => {
                self.history_open = true;
                self.history_confirmation = Some(HistoryConfirmation::Clone);
                window.focus(&self.history_focus);
                cx.notify();
                Ok(())
            }
            NativeAction::Compact => self
                .controller
                .update(cx, |controller, cx| {
                    controller.compact((!arguments.is_empty()).then(|| arguments.to_owned()), cx)
                })
                .then_some(())
                .ok_or_else(|| "Pi must be idle before compacting context.".to_owned()),
            NativeAction::ExportHtml => self
                .controller
                .update(cx, |controller, cx| {
                    controller
                        .export_html((!arguments.is_empty()).then(|| arguments.to_owned()), cx)
                })
                .then_some(())
                .ok_or_else(|| "The current session cannot be exported yet.".to_owned()),
            NativeAction::ExportJsonl => self
                .controller
                .update(cx, |controller, cx| {
                    controller
                        .export_jsonl((!arguments.is_empty()).then(|| arguments.to_owned()), cx)
                })
                .then_some(())
                .ok_or_else(|| "JSONL export is unavailable for this session.".to_owned()),
            NativeAction::CopyLastResponse => {
                let text = self
                    .conversation
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.role == MessageRole::Assistant)
                    .map(|message| {
                        message
                            .content
                            .iter()
                            .filter_map(|block| match block {
                                MessageBlock::Text { text, .. } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| "There is no assistant response to copy.".to_owned())?;
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                Ok(())
            }
            NativeAction::Abort => match self.controller.read(cx).composer_projection().runtime {
                ComposerRuntime::BashRunning | ComposerRuntime::BashCancelling => self
                    .controller
                    .update(cx, |controller, cx| controller.abort_bash(cx))
                    .then_some(())
                    .ok_or_else(|| "Bash is not running.".to_owned()),
                ComposerRuntime::Running | ComposerRuntime::Cancelling => self
                    .controller
                    .update(cx, |controller, cx| controller.abort(cx))
                    .then_some(())
                    .ok_or_else(|| "Pi is not running.".to_owned()),
                ComposerRuntime::Unavailable | ComposerRuntime::Idle => {
                    Err("There is no active run to abort.".to_owned())
                }
            },
            NativeAction::RenameSession => {
                if arguments.is_empty() {
                    return Err("Use /name <name> to rename the session.".to_owned());
                }
                self.controller
                    .update(cx, |controller, cx| {
                        controller.set_session_name(arguments.to_owned(), cx)
                    })
                    .then_some(())
                    .ok_or_else(|| "The current session cannot be renamed yet.".to_owned())
            }
            NativeAction::Settings => {
                self.show_model_panel(
                    ModelPanel::Settings(ModelSettingsTab::Providers),
                    window,
                    cx,
                );
                Ok(())
            }
            NativeAction::Hotkeys => {
                self.hotkey_help_open = true;
                cx.notify();
                Ok(())
            }
            NativeAction::RefreshCommands => self
                .controller
                .update(cx, |controller, cx| controller.refresh_commands(cx))
                .then_some(())
                .ok_or_else(|| "Command refresh is unavailable while disconnected.".to_owned()),
        }
    }

    fn abort_retry(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.abort_retry(cx);
        });
    }

    fn set_steering_mode(&mut self, mode: QueueDeliveryMode, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.set_steering_mode(mode, cx);
        });
    }

    fn set_follow_up_mode(&mut self, mode: QueueDeliveryMode, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.set_follow_up_mode(mode, cx);
        });
    }

    fn set_auto_compaction(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.set_auto_compaction(enabled, cx);
        });
    }

    fn set_auto_retry(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.set_auto_retry(enabled, cx);
        });
    }

    fn on_compaction_event(&mut self, event: &ComposerEvent, cx: &mut Context<Self>) {
        let ComposerEvent::Accept { text, .. } = event else {
            return;
        };
        let focus = text.trim().to_owned();
        if focus.is_empty() || self.pending_compaction_focus.is_some() {
            return;
        }
        let accepted = self.controller.update(cx, |controller, cx| {
            controller.compact(Some(focus.clone()), cx)
        });
        if accepted {
            self.pending_compaction_focus = Some(focus);
            self.compaction_composer.update(cx, |composer, cx| {
                composer.set_feedback(ComposerFeedback::Pending(SubmissionKind::Prompt), cx)
            });
        }
    }

    fn on_session_name_event(&mut self, event: &ComposerEvent, cx: &mut Context<Self>) {
        let ComposerEvent::Accept { text, .. } = event else {
            return;
        };
        let name = text.trim().to_owned();
        if name.is_empty() || self.pending_session_name.is_some() {
            return;
        }
        let accepted = self.controller.update(cx, |controller, cx| {
            controller.set_session_name(name.clone(), cx)
        });
        if accepted {
            self.pending_session_name = Some(name);
            self.session_name_composer.update(cx, |composer, cx| {
                composer.set_feedback(ComposerFeedback::Pending(SubmissionKind::Prompt), cx)
            });
        }
    }

    fn on_history_label_event(&mut self, event: &ComposerEvent, cx: &mut Context<Self>) {
        let ComposerEvent::Accept { text, .. } = event else {
            return;
        };
        let Some(entry) = self.history.selected().cloned() else {
            return;
        };
        let label = text.trim().to_owned();
        if label.is_empty() {
            return;
        }
        self.controller.update(cx, |controller, cx| {
            controller.set_tree_label(entry, Some(label), cx);
        });
    }

    fn on_import_path_event(&mut self, event: &ComposerEvent, cx: &mut Context<Self>) {
        let ComposerEvent::Accept { text, .. } = event else {
            return;
        };
        let path = text.trim().to_owned();
        if path.is_empty() {
            return;
        }
        self.controller.update(cx, |controller, cx| {
            controller.import_jsonl(path, cx);
        });
    }

    fn on_auth_input_event(&mut self, event: &ComposerEvent, secret: bool, cx: &mut Context<Self>) {
        let ComposerEvent::Accept { text, .. } = event else {
            return;
        };
        let Some(AuthStage::Prompt(prompt)) = self
            .controller
            .read(cx)
            .model_runtime_projection()
            .auth
            .map(|flow| flow.stage)
        else {
            return;
        };
        if secret != (prompt.kind == AuthPromptKind::Secret) || text.is_empty() {
            return;
        }
        let accepted = self.controller.update(cx, |controller, cx| {
            controller.answer_auth_prompt(&prompt, text.clone(), cx)
        });
        if accepted {
            let composer = if secret {
                &self.auth_secret_composer
            } else {
                &self.auth_input_composer
            };
            composer.update(cx, |composer, cx| composer.set_draft("", cx));
        }
    }

    fn show_model_panel(&mut self, panel: ModelPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.model_panel = Some(panel);
        match panel {
            ModelPanel::Switcher | ModelPanel::Settings(ModelSettingsTab::Models) => {
                window.focus(&self.model_search_composer.read(cx).focus_handle(cx));
            }
            ModelPanel::Thinking | ModelPanel::Settings(_) => {}
        }
        cx.notify();
    }

    fn toggle_model_panel(
        &mut self,
        panel: ModelPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model_panel == Some(panel) {
            self.close_model_panel(window, cx);
        } else {
            self.show_model_panel(panel, window, cx);
        }
    }

    fn close_model_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model_panel = None;
        window.focus(&self.composer.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn set_model_settings_tab(&mut self, tab: ModelSettingsTab, cx: &mut Context<Self>) {
        self.model_panel = Some(ModelPanel::Settings(tab));
        cx.notify();
    }

    fn select_model(
        &mut self,
        identity: ModelIdentity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let accepted = self.controller.update(cx, |controller, cx| {
            controller.set_active_model(identity, cx)
        });
        if accepted && matches!(self.model_panel, Some(ModelPanel::Switcher)) {
            self.close_model_panel(window, cx);
        } else {
            cx.notify();
        }
    }

    fn set_thinking(&mut self, level: ThinkingLevel, window: &mut Window, cx: &mut Context<Self>) {
        let accepted = self.controller.update(cx, |controller, cx| {
            controller.set_active_thinking(level, cx)
        });
        if accepted && matches!(self.model_panel, Some(ModelPanel::Thinking)) {
            self.close_model_panel(window, cx);
        } else {
            cx.notify();
        }
    }

    fn refresh_models(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.refresh_model_catalog(cx);
        });
    }

    fn reload_resources(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.reload_resources(cx);
        });
    }

    fn set_resource_scope_filter(&mut self, filter: ResourceScopeFilter, cx: &mut Context<Self>) {
        self.resource_scope_filter = filter;
        cx.notify();
    }

    fn set_resource_state_filter(&mut self, filter: ResourceStateFilter, cx: &mut Context<Self>) {
        self.resource_state_filter = filter;
        cx.notify();
    }

    fn login_provider(&mut self, provider: String, method: AuthMethod, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.login_provider(provider, method, cx);
        });
    }

    fn logout_provider(&mut self, provider: String, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.logout_provider(provider, cx);
        });
    }

    fn cancel_provider_auth(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.cancel_provider_auth(cx);
        });
    }

    fn answer_auth_select(
        &mut self,
        prompt: crate::model_runtime::AuthPrompt,
        value: String,
        cx: &mut Context<Self>,
    ) {
        self.controller.update(cx, |controller, cx| {
            controller.answer_auth_prompt(&prompt, value, cx);
        });
    }

    fn set_default_model(&mut self, identity: ModelIdentity, cx: &mut Context<Self>) {
        let thinking = self
            .controller
            .read(cx)
            .model_runtime_projection()
            .catalog
            .and_then(|catalog| catalog.defaults.thinking);
        self.controller.update(cx, |controller, cx| {
            controller.set_model_defaults(Some(identity), thinking, cx);
        });
    }

    fn set_default_thinking(&mut self, thinking: ThinkingLevel, cx: &mut Context<Self>) {
        let model = self
            .controller
            .read(cx)
            .model_runtime_projection()
            .catalog
            .and_then(|catalog| catalog.defaults.model);
        self.controller.update(cx, |controller, cx| {
            controller.set_model_defaults(model, Some(thinking), cx);
        });
    }

    fn toggle_model_scope(&mut self, identity: ModelIdentity, cx: &mut Context<Self>) {
        let Some(catalog) = self.controller.read(cx).model_runtime_projection().catalog else {
            return;
        };
        let mut scope = catalog.defaults.scoped_models;
        if let Some(index) = scope.iter().position(|model| model == &identity) {
            scope.remove(index);
        } else {
            scope.push(identity);
        }
        self.controller.update(cx, |controller, cx| {
            controller.set_model_scope(scope, cx);
        });
    }

    fn history_projection(&self, cx: &Context<Self>) -> HistoryProjection {
        self.controller.read(cx).history_projection()
    }

    fn select_history(
        &mut self,
        entry: crate::services::rpc::EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projection = self.history_projection(cx);
        if self.history.select(entry, &projection.tree) {
            window.focus(&self.history_focus);
            self.history_confirmation = None;
            cx.notify();
        }
    }

    fn set_history_filter(&mut self, filter: HistoryFilter, cx: &mut Context<Self>) {
        self.history.set_filter(filter);
        self.history_confirmation = None;
        cx.notify();
    }

    fn on_history_next(&mut self, _: &HistoryNext, _: &mut Window, cx: &mut Context<Self>) {
        let projection = self.history_projection(cx);
        let rows = self
            .history
            .rows(&projection.tree, projection.leaf_id.as_ref());
        if self.history.move_next(&rows) {
            cx.notify();
        }
    }

    fn on_history_previous(&mut self, _: &HistoryPrevious, _: &mut Window, cx: &mut Context<Self>) {
        let projection = self.history_projection(cx);
        let rows = self
            .history
            .rows(&projection.tree, projection.leaf_id.as_ref());
        if self.history.move_previous(&rows) {
            cx.notify();
        }
    }

    fn on_history_first(&mut self, _: &HistoryFirst, _: &mut Window, cx: &mut Context<Self>) {
        let projection = self.history_projection(cx);
        let rows = self
            .history
            .rows(&projection.tree, projection.leaf_id.as_ref());
        if self.history.move_first(&rows) {
            cx.notify();
        }
    }

    fn on_history_last(&mut self, _: &HistoryLast, _: &mut Window, cx: &mut Context<Self>) {
        let projection = self.history_projection(cx);
        let rows = self
            .history
            .rows(&projection.tree, projection.leaf_id.as_ref());
        if self.history.move_last(&rows) {
            cx.notify();
        }
    }

    fn on_history_fold(&mut self, _: &HistoryFold, _: &mut Window, cx: &mut Context<Self>) {
        let projection = self.history_projection(cx);
        if self.history.fold_or_parent(&projection.tree) {
            cx.notify();
        }
    }

    fn on_history_unfold(&mut self, _: &HistoryUnfold, _: &mut Window, cx: &mut Context<Self>) {
        let projection = self.history_projection(cx);
        if self.history.unfold_or_child(&projection.tree) {
            cx.notify();
        }
    }

    fn on_history_activate(&mut self, _: &HistoryActivate, _: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.history.selected().cloned() else {
            return;
        };
        if self.history_confirmation == Some(HistoryConfirmation::Navigate(entry.clone())) {
            self.confirm_history_operation(cx);
        } else {
            self.history_confirmation = Some(HistoryConfirmation::Navigate(entry));
            cx.notify();
        }
    }

    fn request_fork(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.history.selected().cloned() else {
            return;
        };
        self.history_confirmation = Some(HistoryConfirmation::Fork(entry));
        cx.notify();
    }

    fn request_clone(&mut self, cx: &mut Context<Self>) {
        self.history_confirmation = Some(HistoryConfirmation::Clone);
        cx.notify();
    }

    fn request_navigation(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.history.selected().cloned() else {
            return;
        };
        self.history_confirmation = Some(HistoryConfirmation::Navigate(entry));
        cx.notify();
    }

    fn cancel_history_confirmation(&mut self, cx: &mut Context<Self>) {
        self.history_confirmation = None;
        cx.notify();
    }

    fn confirm_history_operation(&mut self, cx: &mut Context<Self>) {
        let Some(confirmation) = self.history_confirmation.take() else {
            return;
        };
        self.controller
            .update(cx, |controller, cx| match confirmation {
                HistoryConfirmation::Navigate(entry) => {
                    controller.navigate_tree(entry, self.summarize_navigation, None, None, cx);
                }
                HistoryConfirmation::Fork(entry) => {
                    controller.fork_before(entry, cx);
                }
                HistoryConfirmation::Clone => {
                    controller.clone_current_path(cx);
                }
            });
        cx.notify();
    }

    fn clear_selected_label(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.history.selected().cloned() else {
            return;
        };
        self.controller.update(cx, |controller, cx| {
            controller.set_tree_label(entry, None, cx);
        });
    }

    fn toggle_navigation_summary(&mut self, cx: &mut Context<Self>) {
        self.summarize_navigation = !self.summarize_navigation;
        cx.notify();
    }

    fn export_jsonl(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.export_jsonl(None, cx);
        });
    }

    fn cancel_bridge(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.cancel_bridge_operation(cx);
        });
    }

    fn restart_bridge(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.restart_bridge(cx);
        });
    }

    fn switch_session(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.switch_session(path, cx);
        });
    }

    fn refresh_sessions(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.refresh_sessions(cx);
        });
    }

    fn on_conversation_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.delta.precise() {
            // Pixel deltas already carry the platform's touchpad precision and
            // momentum. Never layer synthetic motion on top of them.
            self.conversation_scroll_motion.cancel();
            return false;
        }

        let distance = -event.delta.pixel_delta(px(20.0)).y;
        if distance == px(0.0) {
            return false;
        }
        if distance > px(0.0) && self.conversation_follow.get() {
            self.conversation_scroll_motion.cancel();
            self.conversation_list_state.scroll_to(ListOffset {
                item_ix: self.conversation_list.item_count(),
                offset_in_item: px(0.0),
            });
            return true;
        }

        self.conversation_follow.set(false);
        let now = Instant::now();
        if self.conversation_scroll_motion.push(distance, now) {
            self.advance_conversation_scroll(now, cx);
            self.schedule_conversation_scroll_frame(window, cx);
        }
        true
    }

    fn advance_conversation_scroll(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(step) = self.conversation_scroll_motion.advance(now) else {
            return;
        };
        let before = self.conversation_list_state.logical_scroll_top();
        self.conversation_list_state.scroll_by(step);
        let after = self.conversation_list_state.logical_scroll_top();
        let stalled = before.item_ix == after.item_ix
            && (f32::from(before.offset_in_item) - f32::from(after.offset_in_item)).abs() < 0.01;
        let at_top =
            step < px(0.0) && after.item_ix == 0 && f32::from(after.offset_in_item) <= 0.01;
        let at_bottom =
            step > px(0.0) && (after.item_ix >= self.conversation_list.item_count() || stalled);

        if at_bottom {
            self.conversation_list_state.scroll_to(ListOffset {
                item_ix: self.conversation_list.item_count(),
                offset_in_item: px(0.0),
            });
            self.conversation_follow.set(true);
            self.conversation_scroll_motion.cancel();
        } else if at_top || stalled {
            self.conversation_scroll_motion.cancel();
        }
        cx.notify();
    }

    fn schedule_conversation_scroll_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.conversation_scroll_motion.schedule_frame() {
            return;
        }
        cx.on_next_frame(window, |view, window, cx| {
            view.conversation_scroll_motion.begin_frame();
            view.advance_conversation_scroll(Instant::now(), cx);
            view.schedule_conversation_scroll_frame(window, cx);
        });
    }

    fn sync_runtime(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.defer_in(window, |view, _, cx| view.collect_runtime_notifications(cx));
        self.sync_extension_ui(window, cx);
        let conversation = self.controller.read(cx).conversation_projection();
        let epoch_changed = conversation.epoch != self.conversation.epoch;
        if epoch_changed {
            self.conversation_scroll_motion.cancel();
            self.conversation_follow.set(true);
        }
        self.transcript_cache
            .update(cx, |cache, _| cache.prepare_epoch(conversation.epoch));
        let requested_editor_text = self
            .controller
            .update(cx, |controller, _| controller.take_requested_editor_text());
        if let Some(text) = requested_editor_text {
            self.composer
                .update(cx, |composer, cx| composer.set_draft(&text, cx));
        }
        let conversation_list =
            ConversationListModel::updated(&self.conversation_list, &conversation, epoch_changed);
        conversation_list.reconcile(
            &self.conversation_list,
            &self.conversation_list_state,
            epoch_changed,
        );
        if self.conversation_follow.get() {
            self.conversation_list_state.scroll_to(ListOffset {
                item_ix: conversation_list.item_count(),
                offset_in_item: px(0.0),
            });
        }
        self.conversation = Arc::new(conversation);
        self.conversation_list = Arc::new(conversation_list);
        self.sync_retry_tick(cx);

        let compact_available = matches!(
            self.conversation.lifecycle,
            RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
        ) && self.conversation.pending_operation.is_none();
        self.compaction_composer.update(cx, |composer, cx| {
            composer.set_availability(
                if compact_available {
                    ComposerAvailability::Idle
                } else {
                    ComposerAvailability::Unavailable
                },
                cx,
            )
        });
        let catalog = self.controller.read(cx).catalog_projection();
        let bridge = self.controller.read(cx).bridge_projection();
        let rename_available =
            compact_available && catalog.current_session_file.is_some() && !catalog.switching;
        self.session_name_composer.update(cx, |composer, cx| {
            composer.set_availability(
                if rename_available {
                    ComposerAvailability::Idle
                } else {
                    ComposerAvailability::Unavailable
                },
                cx,
            )
        });
        let bridge_input_available =
            compact_available && !catalog.switching && bridge.pending.is_none();
        let label_available = bridge_input_available
            && bridge
                .capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.labels);
        self.history_label_composer.update(cx, |composer, cx| {
            composer.set_availability(
                if label_available {
                    ComposerAvailability::Idle
                } else {
                    ComposerAvailability::Unavailable
                },
                cx,
            )
        });
        let import_available = bridge_input_available
            && catalog.root.is_some()
            && bridge
                .capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.jsonl_import);
        self.import_path_composer.update(cx, |composer, cx| {
            composer.set_availability(
                if import_available {
                    ComposerAvailability::Idle
                } else {
                    ComposerAvailability::Unavailable
                },
                cx,
            )
        });
        let auth_prompt = self
            .controller
            .read(cx)
            .model_runtime_projection()
            .auth
            .and_then(|flow| match flow.stage {
                AuthStage::Prompt(prompt) => Some(prompt),
                _ => None,
            });
        let prompt_id = auth_prompt.as_ref().map(|prompt| prompt.prompt_id.clone());
        if prompt_id != self.active_auth_prompt_id {
            self.active_auth_prompt_id = prompt_id;
            self.auth_input_composer
                .update(cx, |composer, cx| composer.set_draft("", cx));
            self.auth_secret_composer
                .update(cx, |composer, cx| composer.set_draft("", cx));
            if let Some(prompt) = auth_prompt
                && self.model_panel.is_some()
                && prompt.kind != AuthPromptKind::Select
            {
                let composer = if prompt.kind == AuthPromptKind::Secret {
                    &self.auth_secret_composer
                } else {
                    &self.auth_input_composer
                };
                window.focus(&composer.read(cx).focus_handle(cx));
            }
        }
        self.reconcile_scoped_operations(&catalog, cx);

        let projection = self.controller.read(cx).composer_projection();
        let availability = match projection.runtime {
            ComposerRuntime::Unavailable => ComposerAvailability::Unavailable,
            ComposerRuntime::Idle => ComposerAvailability::Idle,
            ComposerRuntime::Running => ComposerAvailability::Running,
            ComposerRuntime::Cancelling => ComposerAvailability::Cancelling,
            ComposerRuntime::BashRunning => ComposerAvailability::BashRunning,
            ComposerRuntime::BashCancelling => ComposerAvailability::BashCancelling,
        };
        let was_available = matches!(
            self.composer.read(cx).availability(),
            ComposerAvailability::Idle | ComposerAvailability::Running
        );
        self.composer.update(cx, |composer, cx| {
            composer.set_availability(availability, cx)
        });
        if self.extension_ui.active_dialog.is_none()
            && self.model_panel.is_none()
            && !was_available
            && matches!(
                availability,
                ComposerAvailability::Idle | ComposerAvailability::Running
            )
        {
            window.focus(&self.composer.read(cx).focus_handle(cx));
        }

        if epoch_changed
            && self.pending_bash.as_ref().is_some_and(|request| {
                !self
                    .conversation
                    .bash_executions
                    .iter()
                    .any(|execution| &execution.request == request)
            })
        {
            self.pending_bash = None;
            self.composer.update(cx, |composer, cx| {
                composer.set_feedback(
                    ComposerFeedback::Rejected(
                        "The session changed before Bash could be reconciled.".to_owned(),
                    ),
                    cx,
                )
            });
        }

        if let Some(request) = self.pending_bash.as_ref()
            && let Some(execution) = self
                .conversation
                .bash_executions
                .iter()
                .find(|execution| &execution.request == request)
        {
            match execution.status {
                BashStatus::Running | BashStatus::Cancelling => {}
                BashStatus::Succeeded | BashStatus::Cancelled => {
                    self.composer.update(cx, |composer, cx| {
                        composer.set_feedback(ComposerFeedback::BashCompleted, cx)
                    });
                    self.pending_bash = None;
                }
                BashStatus::Failed | BashStatus::Uncertain => {
                    let summary = execution.error.clone().unwrap_or_else(|| {
                        execution.exit_code.map_or_else(
                            || "Bash did not complete successfully.".to_owned(),
                            |code| format!("Bash exited with code {code}."),
                        )
                    });
                    self.composer.update(cx, |composer, cx| {
                        composer.set_feedback(ComposerFeedback::Rejected(summary), cx)
                    });
                    self.pending_bash = None;
                }
            }
        }

        let Some(pending) = self.pending_draft.as_ref() else {
            if matches!(projection.delivery, PromptDelivery::Uncertain { .. }) {
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(ComposerFeedback::Uncertain, cx)
                });
            }
            cx.notify();
            return;
        };
        let request_matches = match &projection.delivery {
            PromptDelivery::Pending { request, .. }
            | PromptDelivery::Accepted { request, .. }
            | PromptDelivery::Rejected { request, .. }
            | PromptDelivery::Uncertain { request, .. } => request == &pending.request,
            PromptDelivery::None => false,
        };
        if !request_matches {
            cx.notify();
            return;
        }

        match projection.delivery {
            PromptDelivery::Pending { .. } => {}
            PromptDelivery::Accepted { kind, .. } => {
                let expected = pending.text.clone();
                self.composer.update(cx, |composer, cx| {
                    composer.clear_accepted(&expected, kind, cx);
                });
                self.pending_draft = None;
            }
            PromptDelivery::Rejected { summary, .. } => {
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(ComposerFeedback::Rejected(summary), cx)
                });
                self.pending_draft = None;
            }
            PromptDelivery::Uncertain { .. } => {
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(ComposerFeedback::Uncertain, cx)
                });
                self.pending_draft = None;
            }
            PromptDelivery::None => {}
        }
        cx.notify();
    }

    fn sync_retry_tick(&mut self, cx: &mut Context<Self>) {
        let waiting = matches!(self.conversation.retry, RetryState::Waiting { .. });
        if !waiting {
            self.retry_tick_task = None;
            return;
        }
        if self.retry_tick_task.is_some() {
            return;
        }
        self.retry_tick_task = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let keep_ticking = view
                    .update(cx, |view, cx| {
                        let waiting = matches!(view.conversation.retry, RetryState::Waiting { .. });
                        if waiting {
                            cx.notify();
                        }
                        waiting
                    })
                    .unwrap_or(false);
                if !keep_ticking {
                    break;
                }
            }
        }));
    }

    fn reconcile_scoped_operations(&mut self, catalog: &CatalogProjection, cx: &mut Context<Self>) {
        if let Some(focus) = self.pending_compaction_focus.clone()
            && self.conversation.pending_operation.is_none()
        {
            let completed = matches!(
                self.conversation.compaction,
                CompactionState::Completed { .. }
            );
            self.compaction_composer.update(cx, |composer, cx| {
                if completed {
                    composer.clear_accepted(&focus, SubmissionKind::Prompt, cx);
                } else {
                    composer.set_feedback(
                        ComposerFeedback::Rejected("Compaction did not complete.".to_owned()),
                        cx,
                    );
                }
            });
            self.pending_compaction_focus = None;
        }
        if let Some(name) = self.pending_session_name.clone()
            && self.conversation.pending_operation.is_none()
        {
            let renamed = catalog.current_session_name.as_deref() == Some(name.as_str());
            self.session_name_composer.update(cx, |composer, cx| {
                if renamed {
                    composer.clear_accepted(&name, SubmissionKind::Prompt, cx);
                } else {
                    composer.set_feedback(
                        ComposerFeedback::Rejected("Session rename did not complete.".to_owned()),
                        cx,
                    );
                }
            });
            self.pending_session_name = None;
        }
    }

    fn on_activate_recovery(
        &mut self,
        _: &ActivateRecovery,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = self.controller.read(cx).projection().action;
        if let Some(action) = action {
            self.activate_recovery(action, cx);
        }
    }

    fn on_focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        if self.extension_ui.active_dialog.is_some() {
            // Extension requests are modal: global focus traversal must not escape behind them.
            self.focus_active_extension_dialog(window, cx);
            return;
        }
        window.focus_next();
    }

    fn on_focus_previous(
        &mut self,
        _: &FocusPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.extension_ui.active_dialog.is_some() {
            self.focus_active_extension_dialog(window, cx);
            return;
        }
        window.focus_prev();
    }

    fn on_open_command_palette(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.extension_ui.active_dialog.is_some() {
            return;
        }
        if self.command_palette_open {
            self.close_command_palette(window, cx);
        } else {
            self.open_command_palette(window, cx);
        }
    }

    fn on_show_hotkeys(&mut self, _: &ShowHotkeys, window: &mut Window, cx: &mut Context<Self>) {
        if self.extension_ui.active_dialog.is_some() {
            return;
        }
        if self.hotkey_help_open {
            self.hotkey_help_open = false;
            window.focus(&self.composer.read(cx).focus_handle(cx));
            cx.notify();
        } else {
            let _ = self.execute_native_action(NativeAction::Hotkeys, "", window, cx);
        }
    }

    fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_palette_open = true;
        self.hotkey_help_open = false;
        self.command_selection = 0;
        self.command_search_composer.update(cx, |composer, cx| {
            composer.set_draft("", cx);
            composer.set_command_completion_active(true, cx);
        });
        window.focus(&self.command_search_composer.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.command_palette_open {
            return;
        }
        self.command_palette_open = false;
        self.command_search_composer.update(cx, |composer, cx| {
            composer.set_command_completion_active(false, cx)
        });
        window.focus(&self.composer.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn on_palette_composer_event(
        &mut self,
        event: &ComposerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComposerEvent::CommandNext => self.move_command_selection(1, true, cx),
            ComposerEvent::CommandPrevious => self.move_command_selection(-1, true, cx),
            ComposerEvent::CommandAccept => {
                let query = self.command_search_composer.read(cx).draft().to_owned();
                let catalog = self.command_catalog(cx);
                let Some(entry) = catalog
                    .filtered(&query)
                    .get(self.command_selection)
                    .cloned()
                    .cloned()
                else {
                    return;
                };
                self.choose_command_entry(entry, window, cx);
            }
            ComposerEvent::CommandDismiss => self.close_command_palette(window, cx),
            ComposerEvent::Accept { .. }
            | ComposerEvent::FollowUp { .. }
            | ComposerEvent::Abort
            | ComposerEvent::AbortBash => {}
        }
    }

    fn dispatch_orchestration_action(
        &mut self,
        action: OrchestrationAction,
        cx: &mut Context<Self>,
    ) -> bool {
        self.controller.update(cx, |controller, cx| {
            controller.orchestration_action(action, cx)
        })
    }

    fn open_subagent(&mut self, agent_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_subagent_id = Some(agent_id);
        self.subagent_dialog_scroll.scroll_to_bottom();
        window.focus(&self.subagent_dialog_focus);
        cx.notify();
    }

    fn close_subagent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_subagent_id = None;
        self.subagent_composer
            .update(cx, |composer, cx| composer.set_draft("", cx));
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn on_subagent_dialog_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            self.close_subagent(window, cx);
        }
    }

    fn on_subagent_composer_event(
        &mut self,
        event: &ComposerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComposerEvent::Accept { text, .. } if !text.trim().is_empty() => {
                let Some(agent_id) = self.selected_subagent_id.clone() else {
                    return;
                };
                let active = self
                    .controller
                    .read(cx)
                    .orchestration_projection()
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.subagent(&agent_id))
                    .is_some_and(|agent| agent.status.is_active());
                let action = if active {
                    OrchestrationAction::SubagentSteer {
                        agent_id,
                        message: text.clone(),
                    }
                } else {
                    OrchestrationAction::SubagentResume {
                        agent_id,
                        prompt: text.clone(),
                    }
                };
                if self.dispatch_orchestration_action(action, cx) {
                    self.subagent_composer
                        .update(cx, |composer, cx| composer.set_draft("", cx));
                }
            }
            ComposerEvent::Abort | ComposerEvent::AbortBash => self.close_subagent(window, cx),
            ComposerEvent::Accept { .. }
            | ComposerEvent::FollowUp { .. }
            | ComposerEvent::CommandNext
            | ComposerEvent::CommandPrevious
            | ComposerEvent::CommandAccept
            | ComposerEvent::CommandDismiss => {}
        }
    }

    fn on_goal_edit_event(&mut self, event: &ComposerEvent, cx: &mut Context<Self>) {
        let ComposerEvent::Accept { text, .. } = event else {
            return;
        };
        let objective = text.trim();
        if objective.is_empty() {
            return;
        }
        let Some(goal) = self
            .controller
            .read(cx)
            .orchestration_projection()
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.goal.as_ref())
            .and_then(|goal| goal.active.clone())
        else {
            return;
        };
        if self.dispatch_orchestration_action(
            OrchestrationAction::GoalEdit {
                goal_id: goal.id,
                objective: objective.to_owned(),
                token_budget: goal.token_budget,
            },
            cx,
        ) {
            self.goal_edit_composer
                .update(cx, |composer, cx| composer.set_draft("", cx));
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let projection = self.controller.read(cx).projection();
        let catalog = self.controller.read(cx).catalog_projection();
        let history = self
            .history_open
            .then(|| self.controller.read(cx).history_projection());
        if let Some(history) = history.as_ref() {
            self.history
                .synchronize(&history.tree, history.leaf_id.as_ref());
        }
        let bridge = self.controller.read(cx).bridge_projection();
        let models = self.controller.read(cx).model_runtime_projection();
        let resources = self.controller.read(cx).resource_center_projection();
        let orchestration = self.controller.read(cx).orchestration_projection();
        let command_catalog = self.command_catalog(cx);

        div()
            .id("runtime-shell")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_connect))
            .on_action(cx.listener(Self::on_retry))
            .on_action(cx.listener(Self::on_stop))
            .on_action(cx.listener(Self::on_abort_run))
            .on_action(cx.listener(Self::on_activate_recovery))
            .on_action(cx.listener(Self::on_focus_next))
            .on_action(cx.listener(Self::on_focus_previous))
            .on_action(cx.listener(Self::on_open_command_palette))
            .on_action(cx.listener(Self::on_show_hotkeys))
            .on_action(cx.listener(Self::on_history_next))
            .on_action(cx.listener(Self::on_history_previous))
            .on_action(cx.listener(Self::on_history_first))
            .on_action(cx.listener(Self::on_history_last))
            .on_action(cx.listener(Self::on_history_fold))
            .on_action(cx.listener(Self::on_history_unfold))
            .on_action(cx.listener(Self::on_history_activate))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .font_family(theme::SANS)
            .text_color(theme::bone())
            .child(titlebar(&projection, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(sessions_panel(
                        SessionsPanelParams {
                            catalog: &catalog,
                            projection: &projection,
                            conversation: &self.conversation,
                            name_composer: &self.session_name_composer,
                            history_open: self.history_open,
                        },
                        cx,
                    ))
                    .when(self.history_open, |layout| {
                        layout.child(history_panel(
                            HistoryPanelParams {
                                projection: history
                                    .as_ref()
                                    .expect("history is projected while the panel is open"),
                                bridge: &bridge,
                                browser: &self.history,
                                focus: &self.history_focus,
                                search: &self.history_search_composer,
                                label: &self.history_label_composer,
                                import_path: &self.import_path_composer,
                                confirmation: self.history_confirmation.as_ref(),
                                summarize: self.summarize_navigation,
                            },
                            cx,
                        ))
                    })
                    .child(match self.model_panel {
                        Some(ModelPanel::Settings(tab)) => model_settings_panel(
                            ModelSettingsPanelParams {
                                projection: &models,
                                resources: &resources,
                                tab,
                                resource_scope_filter: self.resource_scope_filter,
                                resource_state_filter: self.resource_state_filter,
                                search: &self.model_search_composer,
                                auth_input: &self.auth_input_composer,
                                auth_secret: &self.auth_secret_composer,
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
                            .child(conversation_area(
                                &projection,
                                Arc::clone(&self.conversation),
                                Arc::clone(&self.conversation_list),
                                self.conversation_list_state.clone(),
                                self.transcript_cache.clone(),
                                cx.entity(),
                            ))
                            .child(composer_bar(
                                ComposerBarParams {
                                    composer: &self.composer,
                                    models: &models,
                                    projection: &projection,
                                    panel: self.model_panel,
                                    search: &self.model_search_composer,
                                    command_catalog: &command_catalog,
                                    command_selection: self.command_selection,
                                    command_scroll: &self.slash_command_scroll,
                                    slash_dismissed: self.dismissed_slash_draft.as_deref()
                                        == Some(self.composer.read(cx).draft()),
                                    extension_ui: &self.extension_ui,
                                },
                                cx,
                            ))
                            .into_any_element(),
                    })
                    .child(inspector(
                        InspectorParams {
                            projection: &projection,
                            conversation: &self.conversation,
                            extension_ui: &self.extension_ui,
                            compaction_composer: &self.compaction_composer,
                            orchestration: &orchestration,
                            selected_task_id: self.selected_task_id.as_deref(),
                            goal_edit_composer: &self.goal_edit_composer,
                        },
                        cx,
                    )),
            )
            .when(self.command_palette_open, |shell| {
                shell.child(command_palette_overlay(
                    &command_catalog,
                    &self.command_search_composer,
                    self.command_selection,
                    &self.command_palette_scroll,
                    cx,
                ))
            })
            .when(self.hotkey_help_open, |shell| {
                shell.child(hotkey_help_overlay(cx))
            })
            .when(!self.runtime_notifications.is_empty(), |shell| {
                shell.child(runtime_notification_stack(&self.runtime_notifications, cx))
            })
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
    }
}

fn titlebar(projection: &ShellProjection, cx: &mut Context<RootView>) -> impl IntoElement {
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
                        .child("pi"),
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

struct SessionsPanelParams<'a> {
    catalog: &'a CatalogProjection,
    projection: &'a ShellProjection,
    conversation: &'a ConversationProjection,
    name_composer: &'a Entity<Composer>,
    history_open: bool,
}

fn sessions_panel(params: SessionsPanelParams<'_>, cx: &mut Context<RootView>) -> impl IntoElement {
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

struct HistoryPanelParams<'a> {
    projection: &'a HistoryProjection,
    bridge: &'a BridgeProjection,
    browser: &'a HistoryBrowser,
    focus: &'a FocusHandle,
    search: &'a Entity<Composer>,
    label: &'a Entity<Composer>,
    import_path: &'a Entity<Composer>,
    confirmation: Option<&'a HistoryConfirmation>,
    summarize: bool,
}

fn history_panel(params: HistoryPanelParams<'_>, cx: &mut Context<RootView>) -> impl IntoElement {
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

struct ModelSettingsPanelParams<'a> {
    projection: &'a ModelRuntimeProjection,
    resources: &'a ResourceCenterProjection,
    tab: ModelSettingsTab,
    resource_scope_filter: ResourceScopeFilter,
    resource_state_filter: ResourceStateFilter,
    search: &'a Entity<Composer>,
    auth_input: &'a Entity<Composer>,
    auth_secret: &'a Entity<Composer>,
}

fn model_settings_panel(
    params: ModelSettingsPanelParams<'_>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let ModelSettingsPanelParams {
        projection,
        resources,
        tab,
        resource_scope_filter,
        resource_state_filter,
        search,
        auth_input,
        auth_secret,
    } = params;
    let refreshing = if tab == ModelSettingsTab::Resources {
        matches!(resources.phase, ResourcePhase::Refreshing)
    } else {
        matches!(projection.phase, CatalogPhase::Refreshing)
    };
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(theme::canvas())
        .child(
            div()
                .px(px(18.0))
                .pt(px(14.0))
                .pb(px(12.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_UI))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::bone())
                                        .child(if tab == ModelSettingsTab::Resources {
                                            "Resource Center"
                                        } else {
                                            "Model settings"
                                        }),
                                )
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_TINY))
                                        .text_color(theme::ash())
                                        .child(
                                            if tab == ModelSettingsTab::Resources {
                                                "Audited Pi resources, provenance, trust, load state, and active tools."
                                            } else {
                                                "Providers, defaults, cycle order, and usage. Session model and thinking live in the prompt box."
                                            },
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .flex_shrink_0()
                                .child(controls::quiet_button(
                                    "refresh-model-catalog",
                                    if refreshing {
                                        "Refreshing…"
                                    } else if tab == ModelSettingsTab::Resources {
                                        "Reload"
                                    } else {
                                        "Refresh"
                                    },
                                    !refreshing,
                                    Box::new(cx.listener(move |view, _, _, cx| {
                                        if tab == ModelSettingsTab::Resources {
                                            view.reload_resources(cx);
                                        } else {
                                            view.refresh_models(cx);
                                        }
                                    })),
                                ))
                                .child(controls::quiet_button(
                                    "close-model-settings",
                                    "Done",
                                    true,
                                    Box::new(cx.listener(|view, _, window, cx| {
                                        view.close_model_panel(window, cx)
                                    })),
                                )),
                        ),
                )
                .child(
                    controls::tab_track().children(
                        [
                            (ModelSettingsTab::Providers, "Providers"),
                            (ModelSettingsTab::Models, "Models"),
                            (ModelSettingsTab::Thinking, "Thinking"),
                            (ModelSettingsTab::Usage, "Usage"),
                            (ModelSettingsTab::Resources, "Resources"),
                        ]
                        .into_iter()
                        .map(|(target, label)| {
                            controls::tab_button(
                                gpui::SharedString::from(format!("model-tab-{label}")),
                                label,
                                tab == target,
                                Box::new(cx.listener(move |view, _, _, cx| {
                                    view.set_model_settings_tab(target, cx)
                                })),
                            )
                        }),
                    ),
                ),
        )
        .child(match tab {
            ModelSettingsTab::Providers => {
                providers_settings(projection, auth_input, auth_secret, cx).into_any_element()
            }
            ModelSettingsTab::Models => models_settings(projection, search, cx).into_any_element(),
            ModelSettingsTab::Thinking => thinking_settings(projection, cx).into_any_element(),
            ModelSettingsTab::Usage => usage_settings(projection).into_any_element(),
            ModelSettingsTab::Resources => resource_center_settings(
                resources,
                resource_scope_filter,
                resource_state_filter,
                cx,
            )
            .into_any_element(),
        })
        .when(tab != ModelSettingsTab::Resources, |panel| {
            panel
                .when_some(catalog_phase_note(&projection.phase), |panel, note| {
                    panel.child(controls::panel_footer_status(note))
                })
                .when_some(projection.feedback.clone(), |panel, feedback| {
                    panel.child(controls::panel_footer_status(feedback))
                })
        })
        .when(tab == ModelSettingsTab::Resources, |panel| {
            panel.when_some(resources.feedback.clone(), |panel, feedback| {
                panel.child(controls::panel_footer_status(feedback))
            })
        })
}

fn popup_sheet() -> gpui::Div {
    // Fully opaque fill so conversation chrome cannot show through the overlay.
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(theme::panel_hover())
        .bg(theme::panel())
        .overflow_hidden()
}

fn popup_sheet_header(
    title: &'static str,
    close_id: &'static str,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
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
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ash())
                .child(title),
        )
        .child(controls::chrome_action(
            close_id,
            "Close",
            true,
            Box::new(cx.listener(|view, _, window, cx| view.close_model_panel(window, cx))),
        ))
}

fn model_switcher_sheet(
    projection: &ModelRuntimeProjection,
    search: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let query = search.read(cx).draft().to_owned();
    let models = model_choices(projection, &query);
    let active = projection.active_model.clone();
    let can_change = projection.model_change_policy == ModelChangePolicy::Allowed;

    popup_sheet()
        .id("model-switcher-sheet")
        .max_h(px(300.0))
        .child(
            div()
                .px(px(10.0))
                .py(px(8.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .bg(theme::panel())
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_LABEL))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::bone_dim())
                        .flex_shrink_0()
                        .child("Model"),
                )
                .child(div().flex_1().min_w_0().child(search.clone()))
                .child(controls::chrome_action(
                    "close-model-switcher",
                    "Close",
                    true,
                    Box::new(cx.listener(|view, _, window, cx| view.close_model_panel(window, cx))),
                )),
        )
        .when(!can_change, |sheet| {
            sheet.child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .bg(theme::panel())
                    .border_b_1()
                    .border_color(theme::panel_hover())
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_TINY))
                    .text_color(theme::smoke())
                    .child("Settle stream to change"),
            )
        })
        .child(
            div()
                .id("model-switcher-scroll")
                .flex_1()
                .min_h_0()
                .bg(theme::panel())
                .overflow_y_scroll()
                .scrollbar_width(px(6.0))
                .when(models.is_empty(), |list| {
                    list.child(
                        div()
                            .px(px(12.0))
                            .py(px(18.0))
                            .font_family(theme::SANS)
                            .text_size(px(theme::T_UI_SM))
                            .text_color(theme::smoke())
                            .child(match projection.phase {
                                CatalogPhase::Loading => "Loading models…",
                                _ => "No matching models.",
                            }),
                    )
                })
                .children(models.into_iter().map(|model| {
                    let identity = model.identity.clone();
                    let selected = active.as_ref() == Some(&identity);
                    let context = format!("{} ctx", compact_count(model.context_window));
                    let monogram = model
                        .name
                        .chars()
                        .next()
                        .unwrap_or('M')
                        .to_uppercase()
                        .to_string();
                    div()
                        .id(gpui::SharedString::from(format!(
                            "switch-model-{}-{}",
                            identity.provider, identity.id
                        )))
                        .h(px(48.0))
                        .mx(px(6.0))
                        .my(px(2.0))
                        .px(px(10.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(if selected {
                            theme::data()
                        } else {
                            theme::edge_soft()
                        })
                        .bg(if selected {
                            theme::data_wash()
                        } else {
                            theme::panel()
                        })
                        .when(can_change && !selected, |row| {
                            let identity = identity.clone();
                            row.tab_index(0)
                                .cursor_pointer()
                                .hover(|row| {
                                    row.bg(theme::panel_lift()).border_color(theme::edge())
                                })
                                .active(|row| row.bg(theme::panel_hover()))
                                .focus(|row| {
                                    row.bg(theme::panel_lift()).border_color(theme::focus())
                                })
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.select_model(identity.clone(), window, cx)
                                }))
                        })
                        .child(
                            div()
                                .size(px(28.0))
                                .rounded(px(theme::RADIUS_SM))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .border_1()
                                .border_color(if selected {
                                    theme::data()
                                } else {
                                    theme::edge_soft()
                                })
                                .bg(if selected {
                                    theme::data_wash()
                                } else {
                                    theme::canvas()
                                })
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_LABEL))
                                .font_weight(FontWeight::BOLD)
                                .text_color(if selected {
                                    theme::data()
                                } else {
                                    theme::ash()
                                })
                                .child(monogram),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(1.0))
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_UI_SM))
                                        .font_weight(if selected {
                                            FontWeight::BOLD
                                        } else {
                                            FontWeight::SEMIBOLD
                                        })
                                        .text_color(if selected {
                                            theme::bone()
                                        } else {
                                            theme::bone_dim()
                                        })
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(model.name),
                                )
                                .child(
                                    div()
                                        .font_family(theme::MONO)
                                        .text_size(px(10.0))
                                        .text_color(theme::smoke())
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(identity.provider),
                                ),
                        )
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(10.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::ash())
                                .flex_shrink_0()
                                .child(context),
                        )
                        .when(selected, |row| {
                            row.child(
                                div()
                                    .px(px(6.0))
                                    .py(px(3.0))
                                    .rounded(px(theme::RADIUS_SM))
                                    .bg(theme::data_wash())
                                    .font_family(theme::SANS)
                                    .text_size(px(9.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::data())
                                    .flex_shrink_0()
                                    .child("Active"),
                            )
                        })
                })),
        )
        .when_some(projection.feedback.clone(), |sheet, feedback| {
            sheet.child(
                div()
                    .px(px(8.0))
                    .py(px(5.0))
                    .bg(theme::panel())
                    .border_t_1()
                    .border_color(theme::panel_hover())
                    .font_family(theme::MONO)
                    .text_size(px(theme::T_TINY))
                    .text_color(theme::smoke())
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(feedback),
            )
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelChoice {
    identity: ModelIdentity,
    name: String,
    context_window: u64,
}

fn model_choices(projection: &ModelRuntimeProjection, query: &str) -> Vec<ModelChoice> {
    if let Some(catalog) = projection.catalog.as_ref() {
        return catalog
            .models
            .iter()
            .filter(|model| model.available && model.search_matches(query))
            .take(48)
            .map(|model| ModelChoice {
                identity: model.identity.clone(),
                name: model.name.clone(),
                context_window: model.context_window,
            })
            .collect();
    }

    projection
        .stock_models
        .iter()
        .filter(|model| stock_model_matches(model, query))
        .take(48)
        .map(|model| ModelChoice {
            identity: ModelIdentity {
                provider: model.provider.clone(),
                id: model.id.clone(),
            },
            name: model.name.clone(),
            context_window: model.context_window,
        })
        .collect()
}

fn stock_model_matches(model: &ModelSummary, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || model.name.to_lowercase().contains(&query)
        || model.provider.to_lowercase().contains(&query)
        || model.id.to_lowercase().contains(&query)
}

fn thinking_select_sheet(
    projection: &ModelRuntimeProjection,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let levels = thinking_choices(projection);
    let can_change = projection.model_change_policy == ModelChangePolicy::Allowed;
    let active = projection
        .effective_thinking
        .or(projection.active_thinking)
        .or(projection.requested_thinking);

    popup_sheet()
        .id("thinking-select-sheet")
        .max_h(px(168.0))
        .child(popup_sheet_header("Thinking", "close-thinking-select", cx))
        .when(!can_change, |sheet| {
            sheet.child(
                div()
                    .px(px(8.0))
                    .py(px(5.0))
                    .bg(theme::panel())
                    .border_b_1()
                    .border_color(theme::panel_hover())
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_TINY))
                    .text_color(theme::smoke())
                    .child("Settle stream to change"),
            )
        })
        .child(
            div()
                .id("thinking-select-scroll")
                .flex_1()
                .min_h_0()
                .bg(theme::panel())
                .overflow_y_scroll()
                .scrollbar_width(px(6.0))
                .children(levels.into_iter().map(|level| {
                    let selected = active == Some(level);
                    div()
                        .id(gpui::SharedString::from(format!(
                            "thinking-select-{level:?}"
                        )))
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
                        .text_color(if !can_change {
                            theme::smoke()
                        } else if selected {
                            theme::bone()
                        } else {
                            theme::ash()
                        })
                        .when(can_change && !selected, |row| {
                            row.tab_index(0)
                                .cursor_pointer()
                                .hover(|row| row.bg(theme::panel_lift()).text_color(theme::bone()))
                                .active(|row| row.bg(theme::panel_hover()))
                                .focus(|row| row.bg(theme::panel_lift()))
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.set_thinking(level, window, cx)
                                }))
                        })
                        .child(
                            div()
                                .font_family(theme::CONTROL)
                                .text_size(px(theme::T_TINY))
                                .font_weight(if selected {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::MEDIUM
                                })
                                .child(level.label()),
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
                })),
        )
}

fn thinking_choices(projection: &ModelRuntimeProjection) -> Vec<ThinkingLevel> {
    let Some(active) = projection.active_model.as_ref() else {
        return vec![ThinkingLevel::Off];
    };
    if let Some(levels) = projection
        .catalog
        .as_ref()
        .and_then(|catalog| catalog.model(active))
        .map(|model| model.supported_thinking.clone())
    {
        return levels;
    }

    projection
        .stock_models
        .iter()
        .find(|model| model.provider == active.provider && model.id == active.id)
        .map(|model| {
            model
                .supported_thinking
                .iter()
                .copied()
                .map(|level| match level {
                    crate::state::runtime::RuntimeThinkingLevel::Off => ThinkingLevel::Off,
                    crate::state::runtime::RuntimeThinkingLevel::Minimal => ThinkingLevel::Minimal,
                    crate::state::runtime::RuntimeThinkingLevel::Low => ThinkingLevel::Low,
                    crate::state::runtime::RuntimeThinkingLevel::Medium => ThinkingLevel::Medium,
                    crate::state::runtime::RuntimeThinkingLevel::High => ThinkingLevel::High,
                    crate::state::runtime::RuntimeThinkingLevel::Xhigh => ThinkingLevel::Xhigh,
                    crate::state::runtime::RuntimeThinkingLevel::Max => ThinkingLevel::Max,
                })
                .collect()
        })
        .unwrap_or_else(|| vec![ThinkingLevel::Off])
}

fn providers_settings(
    projection: &ModelRuntimeProjection,
    auth_input: &Entity<Composer>,
    auth_secret: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let providers = projection
        .catalog
        .as_ref()
        .map(|catalog| catalog.providers.clone())
        .unwrap_or_default();
    div()
        .id("provider-settings-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .scrollbar_width(px(theme::SCROLLBAR))
        .child(div().w_full().px(px(18.0))
        .pb(px(22.0))
        .pt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .when_some(projection.auth.as_ref(), |panel, auth| {
            panel.child(auth_flow_panel(auth, auth_input, auth_secret, cx))
        })
        .child(controls::panel_note(
            "Pi owns credentials. The GUI only hosts provider prompts and never stores secret values in catalog state.",
            controls::ControlTone::Normal,
        ))
        .child(
            controls::divider_list()
                .when(providers.is_empty(), |list| {
                    list.child(controls::empty_list_note(
                        "No provider catalog is available. Connect to Pi or refresh catalogs.",
                    ))
                })
                .children(providers.into_iter().map(|provider| {
                    let provider_id = provider.id.clone();
                    let auth_busy = projection.auth.is_some();
                    let status = if provider.auth.configured {
                        provider
                            .auth
                            .source
                            .map(|source| source.label())
                            .unwrap_or("Configured")
                    } else {
                        "Not configured"
                    };
                    div()
                        .px(px(12.0))
                        .py(px(12.0))
                        .border_b_1()
                        .border_color(theme::edge_soft())
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_baseline()
                                .justify_between()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_UI_SM))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::bone())
                                        .child(provider.name),
                                )
                                .child(controls::meta_text(format!(
                                    "{status} · {}/{} models",
                                    provider.available_model_count, provider.model_count
                                ))),
                        )
                        .when_some(provider.refresh_error, |row, error| {
                            row.child(controls::panel_note(error, controls::ControlTone::Danger))
                        })
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap(px(6.0))
                                .children(provider.auth_methods.into_iter().map(|method| {
                                    let id = provider_id.clone();
                                    controls::chip_button(
                                        gpui::SharedString::from(format!(
                                            "login-{}-{method:?}",
                                            id
                                        )),
                                        method.label(),
                                        false,
                                        !auth_busy,
                                        Box::new(cx.listener(move |view, _, _, cx| {
                                            view.login_provider(id.clone(), method, cx)
                                        })),
                                    )
                                }))
                                .when(provider.auth.configured, |buttons| {
                                    let id = provider_id.clone();
                                    buttons.child(controls::chip_button(
                                        gpui::SharedString::from(format!("logout-{id}")),
                                        "Log out",
                                        false,
                                        !auth_busy,
                                        Box::new(cx.listener(move |view, _, _, cx| {
                                            view.logout_provider(id.clone(), cx)
                                        })),
                                    ))
                                }),
                        )
                })),
        )
        )
}

fn auth_flow_panel(
    auth: &crate::model_runtime::AuthFlow,
    auth_input: &Entity<Composer>,
    auth_secret: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let stage = match &auth.stage {
        AuthStage::Starting => controls::panel_note(
            "Starting provider-owned authentication...",
            controls::ControlTone::Normal,
        )
        .into_any_element(),
        AuthStage::Info { message, links } => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(controls::panel_note(
                message.clone(),
                controls::ControlTone::Normal,
            ))
            .children(links.iter().cloned().map(|link| {
                let url = link.url;
                controls::quiet_button(
                    gpui::SharedString::from(format!("auth-link-{url}")),
                    link.label
                        .unwrap_or_else(|| "Open provider page".to_owned()),
                    true,
                    Box::new(move |_, _, _| {
                        let _ = crate::services::path_actions::open_provider_auth_url(&url);
                    }),
                )
            }))
            .into_any_element(),
        AuthStage::Browser { url, instructions } => {
            let url = url.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(controls::panel_note(
                    instructions
                        .clone()
                        .unwrap_or_else(|| "Continue authentication in your browser.".to_owned()),
                    controls::ControlTone::Normal,
                ))
                .child(controls::quiet_button(
                    "open-provider-auth-url",
                    "Open browser",
                    true,
                    Box::new(move |_, _, _| {
                        let _ = crate::services::path_actions::open_provider_auth_url(&url);
                    }),
                ))
                .into_any_element()
        }
        AuthStage::DeviceCode {
            user_code,
            verification_uri,
            expires_in_seconds,
        } => {
            let url = verification_uri.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(controls::panel_note(
                    format!(
                        "Enter device code {}{}",
                        user_code,
                        expires_in_seconds
                            .map(|seconds| format!(" · expires in {seconds}s"))
                            .unwrap_or_default()
                    ),
                    controls::ControlTone::Normal,
                ))
                .child(controls::quiet_button(
                    "open-device-code-url",
                    "Open verification page",
                    true,
                    Box::new(move |_, _, _| {
                        let _ = crate::services::path_actions::open_provider_auth_url(&url);
                    }),
                ))
                .into_any_element()
        }
        AuthStage::Progress { message } => {
            controls::panel_note(message.clone(), controls::ControlTone::Normal).into_any_element()
        }
        AuthStage::Prompt(prompt) if prompt.kind == AuthPromptKind::Select => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(controls::panel_note(
                prompt.message.clone(),
                controls::ControlTone::Normal,
            ))
            .children(prompt.options.iter().cloned().map(|option| {
                let prompt = prompt.clone();
                let value = option.id;
                controls::action_row(
                    gpui::SharedString::from(format!("auth-select-{value}")),
                    option.label,
                    option.description.unwrap_or_default(),
                    true,
                    controls::ControlTone::Normal,
                    Box::new(cx.listener(move |view, _, _, cx| {
                        view.answer_auth_select(prompt.clone(), value.clone(), cx)
                    })),
                )
            }))
            .into_any_element(),
        AuthStage::Prompt(prompt) => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(controls::panel_note(
                prompt.message.clone(),
                controls::ControlTone::Normal,
            ))
            .child(if prompt.kind == AuthPromptKind::Secret {
                auth_secret.clone().into_any_element()
            } else {
                auth_input.clone().into_any_element()
            })
            .into_any_element(),
        AuthStage::Cancelling => controls::panel_note(
            "Cancelling authentication...",
            controls::ControlTone::Normal,
        )
        .into_any_element(),
    };
    div()
        .p(px(10.0))
        .border_1()
        .border_color(theme::signal())
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(controls::section_label(format!(
            "Authenticate {} · {}",
            auth.provider,
            auth.method.label()
        )))
        .child(stage)
        .child(controls::quiet_button(
            "cancel-provider-auth",
            "Cancel",
            !matches!(auth.stage, AuthStage::Cancelling),
            Box::new(cx.listener(|view, _, _, cx| view.cancel_provider_auth(cx))),
        ))
}

fn models_settings(
    projection: &ModelRuntimeProjection,
    search: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let query = search.read(cx).draft().to_owned();
    let (models, defaults) = projection
        .catalog
        .as_ref()
        .map(|catalog| {
            (
                catalog
                    .models
                    .iter()
                    .filter(|model| model.search_matches(&query))
                    .take(200)
                    .cloned()
                    .collect::<Vec<_>>(),
                catalog.defaults.clone(),
            )
        })
        .unwrap_or_default();
    div()
        .id("models-settings-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .scrollbar_width(px(theme::SCROLLBAR))
        .child(div().w_full().px(px(18.0))
        .pb(px(22.0))
        .pt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(search.clone())
        .child(controls::panel_note(
            "Defaults and cycle order are saved by Pi for future sessions. Switch the active model from the prompt box.",
            controls::ControlTone::Normal,
        ))
        .child(
            controls::divider_list()
                .when(models.is_empty(), |list| {
                    list.child(controls::empty_list_note("No models match this search."))
                })
                .children(
                    models
                        .into_iter()
                        .map(|model| model_settings_row(model, defaults.clone(), cx)),
                ),
        )
        )
}

fn model_settings_row(
    model: ModelCatalogEntry,
    defaults: crate::model_runtime::ModelDefaults,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let identity = model.identity.clone();
    let is_default = defaults.model.as_ref() == Some(&identity);
    let in_scope = defaults.scoped_models.contains(&identity);
    let default_identity = identity.clone();
    let scope_identity = identity.clone();
    let availability = if model.available {
        "Available"
    } else {
        "Unavailable"
    };
    div()
        .px(px(12.0))
        .py(px(11.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(7.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_UI_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::bone())
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(model.name),
                        )
                        .child(controls::meta_text(format!(
                            "{} · {} · {} ctx · max {}",
                            identity.display(),
                            model.api,
                            compact_count(model.context_window),
                            compact_count(model.max_tokens),
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .flex_shrink_0()
                        .when(is_default, |row| {
                            row.child(controls::chip_button(
                                gpui::SharedString::from(format!(
                                    "badge-default-{}-{}",
                                    identity.provider, identity.id
                                )),
                                "Default",
                                true,
                                false,
                                Box::new(|_, _, _| {}),
                            ))
                        })
                        .when(in_scope, |row| {
                            row.child(controls::chip_button(
                                gpui::SharedString::from(format!(
                                    "badge-cycle-{}-{}",
                                    identity.provider, identity.id
                                )),
                                "Cycle",
                                true,
                                false,
                                Box::new(|_, _, _| {}),
                            ))
                        }),
                ),
        )
        .child(controls::meta_text(format!(
            "{availability} · {}",
            model.pricing.label()
        )))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(6.0))
                .child(controls::chip_button(
                    gpui::SharedString::from(format!(
                        "default-{}-{}",
                        identity.provider, identity.id
                    )),
                    if is_default {
                        "Default model"
                    } else {
                        "Set as default"
                    },
                    is_default,
                    !is_default,
                    Box::new(cx.listener(move |view, _, _, cx| {
                        view.set_default_model(default_identity.clone(), cx)
                    })),
                ))
                .child(controls::chip_button(
                    gpui::SharedString::from(format!(
                        "scope-{}-{}",
                        identity.provider, identity.id
                    )),
                    if in_scope {
                        "Remove from cycle"
                    } else {
                        "Add to cycle"
                    },
                    in_scope,
                    true,
                    Box::new(cx.listener(move |view, _, _, cx| {
                        view.toggle_model_scope(scope_identity.clone(), cx)
                    })),
                )),
        )
        .into_any_element()
}

fn thinking_settings(
    projection: &ModelRuntimeProjection,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let defaults = projection
        .catalog
        .as_ref()
        .map(|catalog| catalog.defaults.clone());
    let session_label = format!(
        "Session: requested {} · effective {}",
        projection
            .requested_thinking
            .or(projection.active_thinking)
            .map(ThinkingLevel::label)
            .unwrap_or("Unknown"),
        projection
            .effective_thinking
            .or(projection.active_thinking)
            .map(ThinkingLevel::label)
            .unwrap_or("Unknown")
    );
    div()
        .id("thinking-settings-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(div().w_full().px(px(18.0))
        .pb(px(22.0))
        .pt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(controls::panel_note(
            "Change the active session thinking level from the prompt box. This page only sets Pi's default for future sessions. Levels are discrete and model-specific; unsupported values are never invented.",
            controls::ControlTone::Normal,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(theme::edge_soft())
                .bg(theme::panel())
                .child(controls::section_label("Current session"))
                .child(controls::meta_text(session_label))
                .when_some(projection.clamp_notice.clone(), |panel, notice| {
                    panel.child(controls::panel_note(notice, controls::ControlTone::Normal))
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(controls::section_label("Default for new sessions"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(6.0))
                        .children(ThinkingLevel::ALL.into_iter().map(|level| {
                            let selected =
                                defaults.as_ref().and_then(|value| value.thinking) == Some(level);
                            controls::chip_button(
                                gpui::SharedString::from(format!("default-thinking-{level:?}")),
                                level.label(),
                                selected,
                                !selected,
                                Box::new(cx.listener(move |view, _, _, cx| {
                                    view.set_default_thinking(level, cx)
                                })),
                            )
                        })),
                ),
        )
        )
}

fn usage_settings(projection: &ModelRuntimeProjection) -> impl IntoElement {
    let usage = &projection.usage;
    let context = match (usage.context_tokens, usage.context_window) {
        (Some(tokens), Some(window)) => {
            format!("{} / {}", compact_count(tokens), compact_count(window))
        }
        _ => "Unknown until Pi reports current context".to_owned(),
    };
    let cost = usage.estimated_cost.map_or_else(
        || "Unknown".to_owned(),
        |cost| {
            if usage.pricing_known {
                format!("${cost:.4} estimated")
            } else {
                format!("${cost:.4} estimated · pricing may be unavailable")
            }
        },
    );
    div()
        .id("usage-settings-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(div().w_full().px(px(18.0))
        .pb(px(22.0))
        .pt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(controls::panel_note(
            "Current context is nullable and separate from lifetime session totals. Cost is an estimate; zero catalog rates mean unpriced, not free.",
            controls::ControlTone::Normal,
        ))
        .child(
            controls::divider_list()
                .child(controls::metric_row("Current context", context))
                .child(controls::metric_row(
                    "Lifetime input",
                    optional_count(usage.input_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime output",
                    optional_count(usage.output_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime cache read",
                    optional_count(usage.cache_read_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime cache write",
                    optional_count(usage.cache_write_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime reasoning",
                    optional_count(usage.reasoning_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime total",
                    optional_count(usage.total_tokens),
                ))
                .child(controls::metric_row("Estimated cost", cost)),
        )
        )
}

fn resource_center_settings(
    projection: &ResourceCenterProjection,
    scope_filter: ResourceScopeFilter,
    state_filter: ResourceStateFilter,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let snapshot = projection.snapshot.as_ref();
    let items = snapshot
        .map(|snapshot| {
            snapshot
                .items
                .iter()
                .filter(|item| scope_filter.matches(item) && state_filter.matches(item))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let summary = snapshot.map(|snapshot| {
        let loaded = snapshot
            .items
            .iter()
            .filter(|item| item.state == ResourceLoadState::Loaded)
            .count();
        let disabled = snapshot
            .items
            .iter()
            .filter(|item| item.state == ResourceLoadState::Disabled)
            .count();
        let errors = snapshot
            .items
            .iter()
            .filter(|item| item.state == ResourceLoadState::Error)
            .count();
        format!(
            "{} total · {loaded} loaded · {disabled} disabled · {errors} error{}",
            snapshot.items.len(),
            if errors == 1 { "" } else { "s" }
        )
    });

    div()
        .id("resource-center-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
            div()
                .w_full()
                .px(px(18.0))
                .pb(px(22.0))
                .pt(px(14.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(controls::panel_note(
                    snapshot
                        .map(|snapshot| snapshot.project_trust_reason.clone())
                        .unwrap_or_else(|| {
                            "Loading the capability-gated Pi resource inventory…".to_owned()
                        }),
                    controls::ControlTone::Normal,
                ))
                .when_some(summary, |panel, summary| {
                    panel.child(controls::meta_text(summary))
                })
                .child(
                    div().flex().flex_row().flex_wrap().gap(px(6.0)).children(
                        [
                            (ResourceScopeFilter::All, "All scopes"),
                            (ResourceScopeFilter::Global, "Global"),
                            (ResourceScopeFilter::Project, "Project"),
                            (ResourceScopeFilter::Package, "Package"),
                        ]
                        .into_iter()
                        .map(|(filter, label)| {
                            controls::chip_button(
                                gpui::SharedString::from(format!("resource-scope-{filter:?}")),
                                label,
                                scope_filter == filter,
                                true,
                                Box::new(cx.listener(move |view, _, _, cx| {
                                    view.set_resource_scope_filter(filter, cx)
                                })),
                            )
                        }),
                    ),
                )
                .child(
                    div().flex().flex_row().flex_wrap().gap(px(6.0)).children(
                        [
                            (ResourceStateFilter::All, "All states"),
                            (ResourceStateFilter::Loaded, "Loaded"),
                            (ResourceStateFilter::Disabled, "Disabled"),
                            (ResourceStateFilter::Error, "Errors"),
                        ]
                        .into_iter()
                        .map(|(filter, label)| {
                            controls::chip_button(
                                gpui::SharedString::from(format!("resource-state-{filter:?}")),
                                label,
                                state_filter == filter,
                                true,
                                Box::new(cx.listener(move |view, _, _, cx| {
                                    view.set_resource_state_filter(filter, cx)
                                })),
                            )
                        }),
                    ),
                )
                .when_some(snapshot, |panel, snapshot| {
                    panel
                        .child(
                            controls::divider_list()
                                .child(controls::metric_row(
                                    "Skill commands",
                                    if snapshot.settings.enable_skill_commands {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    },
                                ))
                                .child(controls::metric_row(
                                    "Pi theme",
                                    snapshot
                                        .settings
                                        .theme
                                        .clone()
                                        .unwrap_or_else(|| "Default".to_owned()),
                                ))
                                .child(controls::metric_row(
                                    "Default project trust",
                                    snapshot.settings.default_project_trust.clone(),
                                )),
                        )
                        .child(controls::panel_note(
                            snapshot.package_mutations.reason.clone(),
                            controls::ControlTone::Danger,
                        ))
                })
                .when(items.is_empty(), |panel| {
                    panel.child(controls::empty_list_note(match projection.phase {
                        ResourcePhase::Loading | ResourcePhase::Refreshing => "Loading resources…",
                        ResourcePhase::Failed(_) => "Resource inventory unavailable.",
                        ResourcePhase::Ready => "No resources match these filters.",
                    }))
                })
                .children(items.into_iter().map(resource_center_row))
                .when_some(snapshot, |panel, snapshot| {
                    panel.children(snapshot.diagnostics.iter().cloned().map(|diagnostic| {
                        controls::panel_note(diagnostic, controls::ControlTone::Normal)
                    }))
                }),
        )
}

fn resource_center_row(item: crate::resource_center::ResourceItem) -> impl IntoElement {
    let state_color = match item.state {
        ResourceLoadState::Loaded => theme::live(),
        ResourceLoadState::Disabled => theme::ash(),
        ResourceLoadState::Error => theme::error(),
    };
    let active = item.active.map(|active| {
        if active {
            " · active tool"
        } else {
            " · inactive tool"
        }
    });
    let package_flags = match (item.pinned, item.filtered) {
        (Some(true), Some(true)) => " · pinned · filtered",
        (Some(true), _) => " · pinned",
        (_, Some(true)) => " · filtered",
        _ => "",
    };
    let metadata = format!(
        "{} · {} · {} · {}{}{}",
        item.kind.label(),
        item.scope.label(),
        item.state.label(),
        item.trust.label(),
        active.unwrap_or_default(),
        package_flags
    );
    div()
        .id(gpui::SharedString::from(format!("resource-{}", item.id)))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(theme::edge_soft())
        .bg(theme::panel())
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::bone())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(item.name),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(state_color)
                        .child(item.state.label()),
                ),
        )
        .child(controls::meta_text(metadata))
        .when_some(item.description, |row, description| {
            row.child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_TINY))
                    .line_height(gpui::relative(1.4))
                    .text_color(theme::bone_dim())
                    .child(description),
            )
        })
        .when_some(item.path, |row, path| row.child(controls::meta_text(path)))
        .child(controls::meta_text(format!("Source: {}", item.source)))
        .children(item.diagnostics.into_iter().map(|diagnostic| {
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .line_height(gpui::relative(1.4))
                .text_color(theme::error())
                .child(diagnostic)
        }))
}

fn catalog_phase_note(phase: &CatalogPhase) -> Option<String> {
    match phase {
        CatalogPhase::Loading => Some("Loading cached model catalogs...".to_owned()),
        CatalogPhase::Refreshing => {
            Some("Refreshing provider catalogs; cached models remain visible.".to_owned())
        }
        CatalogPhase::Stale(summary) | CatalogPhase::Failed(summary) => Some(summary.clone()),
        CatalogPhase::Ready => None,
    }
}

fn optional_count(value: Option<u64>) -> String {
    value
        .map(compact_count)
        .unwrap_or_else(|| "Unknown".to_owned())
}

fn compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn conversation_area(
    projection: &ShellProjection,
    conversation_projection: Arc<ConversationProjection>,
    conversation_list: Arc<ConversationListModel>,
    conversation_list_state: ListState,
    transcript_cache: Entity<TranscriptTextCache>,
    root: Entity<RootView>,
) -> impl IntoElement {
    // The transcript shares the center column with the composer and must yield
    // height to it on every lifecycle-driven rerender.
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme::canvas())
        .when(
            matches!(
                projection.lifecycle.as_str(),
                "Connection error" | "No model"
            ),
            |area| area.child(runtime_error_notice(projection)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .w_full()
                .relative()
                .overflow_hidden()
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            window.on_mouse_event(
                                move |event: &ScrollWheelEvent, phase, window, cx| {
                                    if phase != DispatchPhase::Capture
                                        || !bounds.contains(&event.position)
                                    {
                                        return;
                                    }
                                    let handled = root.update(cx, |view, cx| {
                                        view.on_conversation_scroll_wheel(event, window, cx)
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
                    list(conversation_list_state, move |item_index, _, cx| {
                        conversation_list.render_item(
                            item_index,
                            &conversation_projection,
                            &transcript_cache,
                            cx,
                        )
                    })
                    .size_full()
                    .min_w_0()
                    .pt(px(16.0))
                    .pb(px(16.0)),
                ),
        )
}

fn command_suggestion_sheet(
    entries: &[&CommandEntry],
    selected: usize,
    scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let rows = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| command_row(entry, index == selected, cx))
        .collect::<Vec<_>>();
    popup_sheet()
        .max_w(px(920.0))
        .max_h(px(310.0))
        .child(
            div()
                .px(px(10.0))
                .py(px(7.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(controls::section_label("Commands"))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child("↑↓ choose · Enter run · Esc close"),
                ),
        )
        .child(
            div()
                .id("slash-command-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .children(rows),
        )
}

fn command_row(
    entry: &CommandEntry,
    selected: bool,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let entry = entry.clone();
    let click_entry = entry.clone();
    let provenance = entry.provenance_label();
    let hint = entry
        .argument_hint
        .as_deref()
        .map(|hint| format!(" {hint}"))
        .unwrap_or_default();
    div()
        .id(gpui::SharedString::from(format!(
            "command-row-{}",
            entry.id
        )))
        .px(px(10.0))
        .py(px(7.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .when(selected, |row| row.bg(theme::panel_hover()))
        .when(entry.enabled, |row| {
            row.cursor_pointer()
                .hover(|row| row.bg(theme::panel_hover()))
        })
        .when(!entry.enabled, |row| row.opacity(0.55))
        .on_click(cx.listener(move |view, _, window, cx| {
            view.choose_command_entry(click_entry.clone(), window, cx)
        }))
        .child(
            div()
                .w(px(76.0))
                .flex_shrink_0()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(theme::data())
                .child(entry.group.label()),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_BODY))
                        .text_color(theme::bone())
                        .child(format!("/{}{}", entry.name, hint)),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(entry.description.clone()),
                ),
        )
        .child(
            div()
                .max_w(px(260.0))
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(theme::ash())
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(provenance),
        )
        .into_any_element()
}

fn runtime_notification_stack(
    notifications: &VecDeque<RuntimeNotification>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let cards = notifications
        .iter()
        .enumerate()
        .map(|(index, notification)| {
            let (label, color) = match notification.kind {
                NotificationKind::Info => ("Pi", theme::data()),
                NotificationKind::Warning => ("Pi warning", theme::focus()),
                NotificationKind::Error => ("Pi error", theme::error()),
            };
            div()
                .occlude()
                .w_full()
                .p(px(10.0))
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(color)
                .bg(theme::panel_lift())
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(theme::T_TINY))
                                .text_color(color)
                                .child(label),
                        )
                        .child(controls::chrome_action(
                            format!("dismiss-runtime-notification-{index}"),
                            "Dismiss",
                            true,
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.dismiss_runtime_notification(index, cx)
                            })),
                        )),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .line_height(px(18.0))
                        .text_color(theme::bone_dim())
                        .child(notification.message.clone()),
                )
        })
        .collect::<Vec<_>>();

    div()
        .absolute()
        .top(px(theme::TITLE_H + 12.0))
        .right(px(18.0))
        .w(px(420.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .children(cards)
}

fn extension_widgets(
    extension_ui: &ExtensionUiProjection,
    placement: WidgetPlacement,
) -> impl IntoElement {
    div()
        .px(px(12.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(7.0))
        .children(
            extension_ui
                .widgets
                .iter()
                .filter(move |(_, widget)| widget.placement == placement)
                .map(|(key, widget)| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::data())
                                .child(sanitize_untrusted_text(key)),
                        )
                        .children(widget.lines.iter().map(|line| {
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_MONO_SM))
                                .line_height(px(17.0))
                                .text_color(theme::bone_dim())
                                .child(line.clone())
                        }))
                }),
        )
}

fn extension_status_bar(extension_ui: &ExtensionUiProjection) -> impl IntoElement {
    div()
        .px(px(12.0))
        .py(px(6.0))
        .border_t_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .children(extension_ui.statuses.iter().map(|(key, status)| {
            div()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::data())
                        .child(sanitize_untrusted_text(key)),
                )
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::ash())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(status.text.clone()),
                )
        }))
}

fn extension_diagnostics_panel(extension_ui: &ExtensionUiProjection) -> impl IntoElement {
    const UNSUPPORTED: [&str; 6] = [
        "custom() overlays and components",
        "component-factory widgets",
        "custom editor, header, and footer",
        "TUI message and entry renderers",
        "theme enumeration and switching",
        "process-local extension event bus",
    ];

    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(controls::section_label("Extensions"))
        .when(!extension_ui.errors.is_empty(), |panel| {
            panel.child(controls::divider_list().children(
                extension_ui.errors.iter().rev().take(4).map(|error| {
                    div()
                        .py(px(6.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::error())
                                .child(format!("{} · {}", error.extension, error.event)),
                        )
                        .child(
                            div()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::ash())
                                .child(error.summary.clone()),
                        )
                }),
            ))
        })
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .line_height(px(16.0))
                .text_color(theme::smoke())
                .child("Stock RPC support only. Explicitly unsupported:"),
        )
        .children(UNSUPPORTED.into_iter().map(|item| {
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .line_height(px(16.0))
                .text_color(theme::ash())
                .child(format!("— {item}"))
        }))
}

fn extension_dialog_overlay(
    dialog: &crate::state::runtime::ExtensionDialog,
    queued_dialogs: usize,
    selected: usize,
    focus: &FocusHandle,
    input: &Entity<Composer>,
    editor: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let deadline_copy = dialog.deadline.map(|deadline| {
        let seconds = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_secs_f32();
        format!("Auto-closes in {:.1}s", seconds.max(0.0))
    });
    let request = dialog.request.clone();
    let body = match &request {
        DialogRequest::Select { options, .. } => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .when(options.is_empty(), |list| {
                list.child(
                    div()
                        .text_size(px(theme::T_UI_SM))
                        .text_color(theme::error())
                        .child("This request has no selectable options."),
                )
            })
            .children(options.iter().enumerate().map(|(index, option)| {
                let option_answer = option.clone();
                div()
                    .id(("extension-dialog-option", index))
                    .px(px(12.0))
                    .py(px(9.0))
                    .rounded(px(theme::RADIUS_SM))
                    .border_1()
                    .border_color(if index == selected {
                        theme::focus()
                    } else {
                        theme::edge_soft()
                    })
                    .bg(if index == selected {
                        theme::panel_hover()
                    } else {
                        theme::canvas()
                    })
                    .cursor_pointer()
                    .hover(|row| row.bg(theme::panel_lift()).border_color(theme::edge()))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.answer_extension_dialog(
                            DialogAnswer::Value(option_answer.clone()),
                            window,
                            cx,
                        )
                    }))
                    .child(
                        div()
                            .font_family(theme::SANS)
                            .text_size(px(theme::T_UI))
                            .text_color(theme::bone())
                            .child(option.clone()),
                    )
            }))
            .into_any_element(),
        DialogRequest::Confirm { message, .. } => div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_BODY_SM))
                    .line_height(px(21.0))
                    .text_color(theme::bone_dim())
                    .child(message.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.0))
                    .child(extension_dialog_button(
                        "extension-confirm-no",
                        "No",
                        selected == 0,
                        Box::new(cx.listener(|view, _, window, cx| {
                            view.answer_extension_dialog(DialogAnswer::Confirmed(false), window, cx)
                        })),
                    ))
                    .child(extension_dialog_button(
                        "extension-confirm-yes",
                        "Confirm",
                        selected == 1,
                        Box::new(cx.listener(|view, _, window, cx| {
                            view.answer_extension_dialog(DialogAnswer::Confirmed(true), window, cx)
                        })),
                    )),
            )
            .into_any_element(),
        DialogRequest::Input { .. } => input.clone().into_any_element(),
        DialogRequest::Editor { .. } => editor.clone().into_any_element(),
    };

    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .occlude()
        .bg(gpui::rgba(0x0b0a_09e6))
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.0))
        .track_focus(focus)
        .tab_index(0)
        .on_key_down(cx.listener(RootView::on_extension_dialog_key_down))
        .child(
            div()
                .id("extension-dialog-card")
                .w_full()
                .max_w(px(620.0))
                .max_h(px(620.0))
                .overflow_y_scroll()
                .p(px(18.0))
                .rounded(px(theme::RADIUS))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::edge_hard())
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .font_family(theme::MONO)
                                        .text_size(px(theme::T_TINY))
                                        .text_color(theme::focus())
                                        .child(format!(
                                            "Extension {} · untrusted UI",
                                            dialog.kind()
                                        )),
                                )
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_TITLE))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::bone())
                                        .child(dialog.title().to_owned()),
                                ),
                        )
                        .child(controls::chrome_action(
                            "cancel-extension-dialog",
                            "Cancel · Esc",
                            true,
                            Box::new(cx.listener(|view, _, window, cx| {
                                view.cancel_extension_dialog(window, cx);
                            })),
                        )),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .line_height(px(18.0))
                        .text_color(theme::smoke())
                        .child(
                            "This content comes from an extension. It is not a secure permission prompt and has no verified provenance.",
                        ),
                )
                .child(body)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(10.0))
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(match queued_dialogs {
                                    0 => "No queued extension dialogs".to_owned(),
                                    count => format!(
                                        "{count} queued extension dialog{}",
                                        plural(count as u64)
                                    ),
                                }),
                        )
                        .when_some(deadline_copy, |row, deadline| {
                            row.child(
                                div()
                                    .font_family(theme::MONO)
                                    .text_size(px(theme::T_TINY))
                                    .text_color(theme::data())
                                    .child(deadline),
                            )
                        }),
                ),
        )
}

fn extension_dialog_button(
    id: impl Into<gpui::SharedString>,
    label: impl Into<gpui::SharedString>,
    selected: bool,
    on_click: RootClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(32.0))
        .px(px(14.0))
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(if selected {
            theme::focus()
        } else {
            theme::edge()
        })
        .bg(if selected {
            theme::panel_hover()
        } else {
            theme::canvas()
        })
        .cursor_pointer()
        .hover(|button| button.bg(theme::panel_lift()).border_color(theme::focus()))
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .font_family(theme::CONTROL)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(theme::T_UI_SM))
                .text_color(theme::bone())
                .child(label.into()),
        )
}

fn single_line_title(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

fn extension_dialog_key(kind: Option<&str>, key: &str) -> Option<ExtensionDialogKey> {
    match key {
        "escape" => Some(ExtensionDialogKey::Cancel),
        "tab" => Some(ExtensionDialogKey::ContainFocus),
        "up" | "left" if matches!(kind, Some("select" | "confirm")) => {
            Some(ExtensionDialogKey::Move(-1))
        }
        "down" | "right" if matches!(kind, Some("select" | "confirm")) => {
            Some(ExtensionDialogKey::Move(1))
        }
        "enter" | "space" if matches!(kind, Some("select" | "confirm")) => {
            Some(ExtensionDialogKey::AcceptSelection)
        }
        _ => None,
    }
}

fn command_palette_overlay(
    catalog: &CommandCatalog,
    search: &Entity<Composer>,
    selected: usize,
    scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let query = search.read(cx).draft();
    let matches = catalog.filtered(query);
    let mut rows = Vec::new();
    let mut previous_group = None;
    for (index, entry) in matches.iter().take(60).enumerate() {
        if previous_group != Some(entry.group) {
            previous_group = Some(entry.group);
            rows.push(
                div()
                    .px(px(10.0))
                    .pt(px(10.0))
                    .pb(px(5.0))
                    .font_family(theme::SANS)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(theme::T_TINY))
                    .text_color(theme::data())
                    .child(entry.group.label())
                    .into_any_element(),
            );
        }
        rows.push(command_row(entry, index == selected, cx).into_any_element());
    }

    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .occlude()
        .bg(theme::canvas())
        .pt(px(76.0))
        .items_center()
        .child(
            div()
                .w(px(760.0))
                .h_full()
                .max_h(px(620.0))
                .flex()
                .flex_col()
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(theme::edge_hard())
                .bg(theme::panel())
                .overflow_hidden()
                .child(
                    div()
                        .px(px(12.0))
                        .py(px(10.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_family(theme::SANS)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(theme::T_BODY))
                                .child("Command palette"),
                        )
                        .child(controls::chrome_action(
                            "close-command-palette",
                            "Esc",
                            true,
                            Box::new(cx.listener(|view, _, window, cx| {
                                view.close_command_palette(window, cx)
                            })),
                        )),
                )
                .child(search.clone())
                .child(
                    div()
                        .id("command-palette-results")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .scrollbar_width(px(theme::SCROLLBAR))
                        .children(rows)
                        .when(matches.is_empty(), |list| {
                            list.child(
                                div()
                                    .p(px(18.0))
                                    .text_size(px(theme::T_BODY))
                                    .text_color(theme::smoke())
                                    .child("No matching commands."),
                            )
                        }),
                ),
        )
}

fn hotkey_help_overlay(cx: &mut Context<RootView>) -> impl IntoElement {
    let shortcuts = [
        ("Command palette", "Ctrl+Shift+P"),
        ("Hotkey help", "Ctrl+/"),
        ("Send / steer", "Enter"),
        ("Queue follow-up", "Alt+Enter"),
        ("Insert newline", "Shift+Enter"),
        ("Abort run or Bash", "Esc"),
        ("Move focus", "Tab / Shift+Tab"),
        ("Copy transcript selection", "Ctrl+C"),
        ("History navigation", "↑ ↓ ← → Home End"),
    ];
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .occlude()
        .bg(theme::canvas())
        .pt(px(96.0))
        .items_center()
        .child(
            popup_sheet()
                .w(px(560.0))
                .child(
                    div()
                        .px(px(12.0))
                        .py(px(10.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(theme::T_BODY))
                                .child("Native hotkeys"),
                        )
                        .child(controls::chrome_action(
                            "close-hotkey-help",
                            "Close",
                            true,
                            Box::new(cx.listener(|view, _, window, cx| {
                                view.hotkey_help_open = false;
                                window.focus(&view.composer.read(cx).focus_handle(cx));
                                cx.notify();
                            })),
                        )),
                )
                .children(shortcuts.into_iter().map(|(label, keys)| {
                    div()
                        .px(px(12.0))
                        .py(px(8.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .border_t_1()
                        .border_color(theme::edge_soft())
                        .child(
                            div()
                                .text_size(px(theme::T_BODY))
                                .text_color(theme::bone())
                                .child(label),
                        )
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_UI_SM))
                                .text_color(theme::data())
                                .child(keys),
                        )
                })),
        )
}

struct ComposerBarParams<'a> {
    composer: &'a Entity<Composer>,
    models: &'a ModelRuntimeProjection,
    projection: &'a ShellProjection,
    panel: Option<ModelPanel>,
    search: &'a Entity<Composer>,
    command_catalog: &'a CommandCatalog,
    command_selection: usize,
    command_scroll: &'a ScrollHandle,
    slash_dismissed: bool,
    extension_ui: &'a ExtensionUiProjection,
}

fn composer_bar(params: ComposerBarParams<'_>, cx: &mut Context<RootView>) -> impl IntoElement {
    let ComposerBarParams {
        composer,
        models,
        projection,
        panel,
        search,
        command_catalog,
        command_selection,
        command_scroll,
        slash_dismissed,
        extension_ui,
    } = params;
    let model_open = matches!(panel, Some(ModelPanel::Switcher));
    let thinking_open = matches!(panel, Some(ModelPanel::Thinking));
    let model_label = short_model_label(projection, models);
    let thinking_label = short_thinking_label(projection, models);
    let catalog_ready = models.catalog.is_some();
    let can_pick_model = catalog_ready || !models.stock_models.is_empty();
    let can_pick_thinking = catalog_ready || models.active_thinking.is_some();
    let slash_completion = (!slash_dismissed)
        .then(|| command_catalog.slash_completion(composer.read(cx).draft()))
        .flatten();

    div()
        .flex_shrink_0()
        .bg(theme::floor())
        .border_t_1()
        .border_color(theme::edge_hard())
        .px(px(theme::STREAM_PAD_X))
        .pt(px(10.0))
        .pb(px(14.0))
        // Overlay host: popups are absolute and must not grow this bar's layout height.
        .relative()
        .child(
            div()
                .w_full()
                .relative()
                .when_some(slash_completion, |host, completion| {
                    host.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_full()
                            .pb(px(10.0))
                            .occlude()
                            .flex()
                            .justify_center()
                            .child(command_suggestion_sheet(
                                &completion.matches,
                                command_selection,
                                command_scroll,
                                cx,
                            )),
                    )
                })
                .when(model_open || thinking_open, |host| {
                    // Clear gap so popup bottom border never stacks on the prompt top border.
                    host.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_full()
                            .pb(px(10.0))
                            .occlude()
                            .child(if model_open {
                                model_switcher_sheet(models, search, cx).into_any_element()
                            } else {
                                thinking_select_sheet(models, cx).into_any_element()
                            }),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(theme::edge_hard())
                        .bg(theme::panel())
                        .overflow_hidden()
                        .child(
                            div()
                                .px(px(8.0))
                                .pt(px(6.0))
                                .pb(px(6.0))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.0))
                                .border_b_1()
                                .border_color(theme::edge_soft())
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(3.0))
                                        .flex_shrink_0()
                                        .child(controls::compact_select(
                                            "prompt-model-picker",
                                            model_label,
                                            model_open,
                                            can_pick_model,
                                            148.0,
                                            Box::new(cx.listener(|view, _, window, cx| {
                                                view.toggle_model_panel(
                                                    ModelPanel::Switcher,
                                                    window,
                                                    cx,
                                                )
                                            })),
                                        ))
                                        .child(controls::compact_select(
                                            "prompt-thinking-select",
                                            thinking_label,
                                            thinking_open,
                                            can_pick_thinking,
                                            86.0,
                                            Box::new(cx.listener(|view, _, window, cx| {
                                                view.toggle_model_panel(
                                                    ModelPanel::Thinking,
                                                    window,
                                                    cx,
                                                )
                                            })),
                                        )),
                                )
                                .child(div().flex_1().min_w_0())
                                .when_some(models.clamp_notice.clone(), |row, notice| {
                                    row.child(
                                        div()
                                            .min_w_0()
                                            .max_w(px(220.0))
                                            .font_family(theme::SANS)
                                            .text_size(px(theme::T_TINY))
                                            .text_color(theme::data())
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child(notice),
                                    )
                                })
                                .when_some(
                                    models
                                        .feedback
                                        .clone()
                                        .filter(|_| !model_open && !thinking_open),
                                    |row, fb| {
                                        row.child(
                                            div()
                                                .min_w_0()
                                                .max_w(px(180.0))
                                                .font_family(theme::MONO)
                                                .text_size(px(theme::T_TINY))
                                                .text_color(theme::smoke())
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                                .child(fb),
                                        )
                                    },
                                )
                                .child(controls::chrome_action(
                                    "prompt-model-settings",
                                    "Settings",
                                    true,
                                    Box::new(cx.listener(|view, _, window, cx| {
                                        view.show_model_panel(
                                            ModelPanel::Settings(ModelSettingsTab::Providers),
                                            window,
                                            cx,
                                        )
                                    })),
                                )),
                        )
                        .when(
                            extension_ui.widgets.iter().any(|(_, widget)| {
                                widget.placement == WidgetPlacement::AboveEditor
                            }),
                            |panel| {
                                panel.child(extension_widgets(
                                    extension_ui,
                                    WidgetPlacement::AboveEditor,
                                ))
                            },
                        )
                        .child(composer.clone())
                        .when(
                            extension_ui.widgets.iter().any(|(_, widget)| {
                                widget.placement == WidgetPlacement::BelowEditor
                            }),
                            |panel| {
                                panel.child(extension_widgets(
                                    extension_ui,
                                    WidgetPlacement::BelowEditor,
                                ))
                            },
                        )
                        .when(!extension_ui.statuses.is_empty(), |panel| {
                            panel.child(extension_status_bar(extension_ui))
                        }),
                ),
        )
}

fn short_model_label(projection: &ShellProjection, models: &ModelRuntimeProjection) -> String {
    let raw = if let Some(identity) = models.active_model.as_ref() {
        if let Some(entry) = models
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.model(identity))
        {
            entry.name.clone()
        } else {
            identity.id.clone()
        }
    } else {
        let label = projection.model.label();
        if label == "Unknown" || label == "Loading" || label == "Awaiting" {
            "Model".to_owned()
        } else {
            label
        }
    };
    compact_label(&raw, 22)
}

fn short_thinking_label(projection: &ShellProjection, models: &ModelRuntimeProjection) -> String {
    let level = models
        .effective_thinking
        .or(models.active_thinking)
        .or(models.requested_thinking);
    if let Some(level) = level {
        return thinking_short(level);
    }
    let label = projection.thinking.label();
    if label == "Unknown" || label == "Loading" || label == "Awaiting" {
        "Off".to_owned()
    } else {
        compact_label(&label, 10)
    }
}

fn thinking_short(level: ThinkingLevel) -> String {
    match level {
        ThinkingLevel::Off => "Off".to_owned(),
        ThinkingLevel::Minimal => "Min".to_owned(),
        ThinkingLevel::Low => "Low".to_owned(),
        ThinkingLevel::Medium => "Med".to_owned(),
        ThinkingLevel::High => "High".to_owned(),
        ThinkingLevel::Xhigh => "XHigh".to_owned(),
        ThinkingLevel::Max => "Max".to_owned(),
    }
}

fn compact_label(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_owned();
    }
    let mut out = trimmed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

struct InspectorParams<'a> {
    projection: &'a ShellProjection,
    conversation: &'a ConversationProjection,
    extension_ui: &'a ExtensionUiProjection,
    compaction_composer: &'a Entity<Composer>,
    orchestration: &'a OrchestrationProjection,
    selected_task_id: Option<&'a str>,
    goal_edit_composer: &'a Entity<Composer>,
}

fn inspector(params: InspectorParams<'_>, cx: &mut Context<RootView>) -> impl IntoElement {
    let InspectorParams {
        projection,
        conversation,
        extension_ui,
        compaction_composer,
        orchestration,
        selected_task_id,
        goal_edit_composer,
    } = params;
    div()
        .w(px(theme::INSPECT_W))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(theme::floor())
        .border_l_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .px(px(16.0))
                .h(px(theme::TITLE_H))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::bone_dim())
                        .child("Inspector"),
                ),
        )
        .child(
            div()
                .id("inspector-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .w_full()
                        .px(px(14.0))
                        .pt(px(14.0))
                        .pb(px(22.0))
                        .flex()
                        .flex_col()
                        .gap(px(18.0))
                        .child(controls::context_block(
                            "Context",
                            projection.context.label(),
                            context_pct(&projection.context.label()),
                            projection.input_tokens.label(),
                            projection.output_tokens.label(),
                            projection.cache_read.label(),
                            projection.cache_write.label(),
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(controls::metric_row("Model", projection.model.label()))
                                .child(controls::metric_row(
                                    "Thinking",
                                    projection.thinking.label(),
                                ))
                                .child(controls::metric_row("Cost", projection.cost.label())),
                        )
                        .child(orchestration_panel(
                            orchestration,
                            selected_task_id,
                            goal_edit_composer,
                            cx,
                        ))
                        .child(run_controls(conversation, compaction_composer, cx))
                        .child(queue_panel(conversation))
                        .child(extension_diagnostics_panel(extension_ui)),
                ),
        )
}

fn orchestration_panel(
    orchestration: &OrchestrationProjection,
    selected_task_id: Option<&str>,
    goal_edit_composer: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let body = match (&orchestration.phase, orchestration.snapshot.as_ref()) {
        (OrchestrationPhase::Loading, None) => orchestration_state_note(
            "Connecting to Pi tasks, subagents, and goal…",
            theme::data(),
            false,
            cx,
        )
        .into_any_element(),
        (OrchestrationPhase::Error, None) => orchestration_state_note(
            orchestration
                .feedback
                .as_deref()
                .unwrap_or("Pi's orchestration state could not be loaded."),
            theme::error(),
            true,
            cx,
        )
        .into_any_element(),
        (OrchestrationPhase::Disconnected, None) => orchestration_state_note(
            "The orchestration adapter is disconnected.",
            theme::error(),
            true,
            cx,
        )
        .into_any_element(),
        (OrchestrationPhase::Empty, Some(_)) => controls::empty_list_note(
            "No task, subagent, schedule, or active goal in this Pi session.",
        )
        .into_any_element(),
        (_, Some(snapshot)) => {
            div()
                .flex()
                .flex_col()
                .gap(px(16.0))
                .when(
                    matches!(
                        orchestration.phase,
                        OrchestrationPhase::Stale | OrchestrationPhase::Disconnected
                    ),
                    |panel| {
                        panel.child(orchestration_state_note(
                            orchestration
                                .feedback
                                .as_deref()
                                .unwrap_or("Showing the last authoritative snapshot."),
                            theme::signal(),
                            true,
                            cx,
                        ))
                    },
                )
                .child(task_list(snapshot.tasks.as_slice(), selected_task_id, cx))
                .child(subagent_list(snapshot.subagents.as_slice(), cx))
                .when(!snapshot.schedules.is_empty(), |panel| {
                    panel.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(7.0))
                            .child(controls::section_label("Schedules"))
                            .child(controls::divider_list().children(
                                snapshot.schedules.iter().map(|schedule| {
                                    controls::queue_row(
                                        if schedule.enabled { "ON" } else { "OFF" },
                                        format!(
                                            "{} · {} · {}",
                                            schedule.name,
                                            schedule.schedule,
                                            schedule.subagent_type
                                        ),
                                    )
                                }),
                            )),
                    )
                })
                .child(goal_panel(snapshot.goal.as_ref(), goal_edit_composer, cx))
                .into_any_element()
        }
        _ => controls::empty_list_note("Waiting for Pi orchestration state.").into_any_element(),
    };

    div()
        .flex()
        .flex_col()
        .gap(px(9.0))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Work"))
                .when(orchestration.pending_actions > 0, |row| {
                    row.child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::data())
                            .child(format!("{} pending", orchestration.pending_actions)),
                    )
                }),
        )
        .child(body)
}

fn orchestration_state_note(
    message: impl Into<String>,
    color: gpui::Rgba,
    reconnect: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    div()
        .p(px(10.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .line_height(gpui::relative(1.4))
                .text_color(color)
                .child(message.into()),
        )
        .when(reconnect, |note| {
            note.child(controls::quiet_button(
                "orchestration-reconnect",
                "Reconnect",
                true,
                Box::new(cx.listener(|view, _, _, cx| {
                    view.controller
                        .update(cx, |controller, cx| controller.restart_bridge(cx));
                })),
            ))
        })
}

fn task_list(
    tasks: &[TaskSnapshot],
    selected_task_id: Option<&str>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let completed = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Completed)
        .map(|task| task.id.as_str())
        .collect::<HashSet<_>>();
    div()
        .flex()
        .flex_col()
        .gap(px(7.0))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Tasks"))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(tasks.len().to_string()),
                ),
        )
        .child(
            controls::divider_list()
                .when(tasks.is_empty(), |list| {
                    list.child(controls::empty_list_note("No tasks in this session."))
                })
                .children(tasks.iter().enumerate().map(|(index, task)| {
                    task_row(
                        index,
                        task,
                        selected_task_id == Some(task.id.as_str()),
                        task.blocked_by
                            .iter()
                            .filter(|id| !completed.contains(id.as_str()))
                            .count(),
                        cx,
                    )
                })),
        )
}

fn task_row(
    index: usize,
    task: &TaskSnapshot,
    selected: bool,
    open_blockers: usize,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let id = task.id.clone();
    let keyboard_id = task.id.clone();
    let action_id = task.id.clone();
    let status_color = task_status_color(task.status, open_blockers);
    let can_execute = task.status == TaskStatus::Pending && open_blockers == 0;
    let can_stop = task.status == TaskStatus::InProgress;
    div()
        .border_l_2()
        .border_color(if selected {
            theme::signal()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .bg(if selected {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .child(
            div()
                .id(("task-row", index))
                .tab_index(0)
                .key_context(ORCHESTRATION_ROW_CONTEXT)
                .cursor_pointer()
                .px(px(10.0))
                .py(px(9.0))
                .flex()
                .flex_col()
                .gap(px(3.0))
                .hover(|row| row.bg(theme::panel_hover()))
                .focus(|row| row.border_1().border_color(theme::focus()))
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.selected_task_id = if view.selected_task_id.as_deref() == Some(&id) {
                        None
                    } else {
                        Some(id.clone())
                    };
                    cx.notify();
                }))
                .on_action(cx.listener(move |view, _: &OrchestrationActivate, _, cx| {
                    view.selected_task_id =
                        if view.selected_task_id.as_deref() == Some(&keyboard_id) {
                            None
                        } else {
                            Some(keyboard_id.clone())
                        };
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .justify_between()
                        .gap(px(8.0))
                        .child(
                            div()
                                .min_w_0()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_UI))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::bone())
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(task.subject.clone()),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_TINY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(status_color)
                                .child(if open_blockers > 0 {
                                    "Blocked".to_owned()
                                } else {
                                    task.status.label().to_owned()
                                }),
                        ),
                )
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(format!(
                            "{} · {}",
                            task.id,
                            task.owner.as_deref().unwrap_or("unassigned")
                        )),
                ),
        )
        .when(selected, |row| {
            let task_id = action_id.clone();
            row.child(
                div()
                    .px(px(10.0))
                    .pb(px(10.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .font_family(theme::SANS)
                            .text_size(px(theme::T_UI_SM))
                            .line_height(gpui::relative(1.45))
                            .text_color(theme::bone_dim())
                            .child(task.description.clone()),
                    )
                    .when(open_blockers > 0, |detail| {
                        detail.child(controls::empty_list_note(format!(
                            "Waiting on {}",
                            task.blocked_by.join(", ")
                        )))
                    })
                    .when_some(task.output.clone(), |detail, output| {
                        detail.child(
                            div()
                                .p(px(8.0))
                                .rounded(px(theme::RADIUS_SM))
                                .bg(theme::canvas())
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::ash())
                                .child(output),
                        )
                    })
                    .when(
                        !task.metadata.is_null()
                            && task
                                .metadata
                                .as_object()
                                .is_none_or(|value| !value.is_empty()),
                        |detail| {
                            detail.child(
                                div()
                                    .font_family(theme::MONO)
                                    .text_size(px(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .child(task.metadata.to_string()),
                            )
                        },
                    )
                    .child(controls::quiet_button(
                        format!("task-action-{task_id}"),
                        if can_stop {
                            "Stop task"
                        } else {
                            "Execute task"
                        },
                        can_stop || can_execute,
                        Box::new(cx.listener(move |view, _, _, cx| {
                            let action = if can_stop {
                                OrchestrationAction::TaskStop {
                                    task_id: task_id.clone(),
                                }
                            } else {
                                OrchestrationAction::TaskExecute {
                                    task_ids: vec![task_id.clone()],
                                    additional_context: None,
                                    model: None,
                                    max_turns: None,
                                    cascade: true,
                                }
                            };
                            view.dispatch_orchestration_action(action, cx);
                        })),
                    )),
            )
        })
        .into_any_element()
}

fn subagent_list(agents: &[SubagentSnapshot], cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(7.0))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Subagents"))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(agents.len().to_string()),
                ),
        )
        .child(
            controls::divider_list()
                .when(agents.is_empty(), |list| {
                    list.child(controls::empty_list_note("No subagents in this session."))
                })
                .children(agents.iter().enumerate().map(|(index, agent)| {
                    let agent_id = agent.id.clone();
                    let keyboard_agent_id = agent.id.clone();
                    div()
                        .id(("subagent-row", index))
                        .tab_index(0)
                        .key_context(ORCHESTRATION_ROW_CONTEXT)
                        .cursor_pointer()
                        .px(px(10.0))
                        .py(px(9.0))
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .hover(|row| row.bg(theme::panel_hover()))
                        .focus(|row| row.border_1().border_color(theme::focus()))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.open_subagent(agent_id.clone(), window, cx);
                        }))
                        .on_action(cx.listener(
                            move |view, _: &OrchestrationActivate, window, cx| {
                                view.open_subagent(keyboard_agent_id.clone(), window, cx);
                            },
                        ))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .justify_between()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_UI_SM))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(agent_type_color(&agent.agent_type))
                                        .child(agent.agent_type.clone()),
                                )
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_TINY))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(subagent_status_color(agent.status))
                                        .child(agent.status.label()),
                                ),
                        )
                        .child(
                            div()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_UI_SM))
                                .line_height(gpui::relative(1.4))
                                .text_color(theme::bone_dim())
                                .child(agent.description.clone()),
                        )
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(match agent.queue_position {
                                    Some(position) => format!(
                                        "{} · queue {} · limit {}",
                                        agent.id, position, agent.max_concurrent
                                    ),
                                    None => format!(
                                        "{} · {} tool use{}",
                                        agent.id,
                                        agent.tool_uses,
                                        plural(agent.tool_uses)
                                    ),
                                }),
                        )
                })),
        )
}

fn goal_panel(
    goal: Option<&crate::orchestration::GoalSnapshot>,
    goal_edit_composer: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let Some(goal) = goal else {
        return div()
            .flex()
            .flex_col()
            .gap(px(7.0))
            .child(controls::section_label("Goal"))
            .child(controls::empty_list_note("No active goal."))
            .into_any_element();
    };
    let Some(active) = goal.active.as_ref() else {
        return div()
            .flex()
            .flex_col()
            .gap(px(7.0))
            .child(controls::section_label("Goal queue"))
            .child(controls::empty_list_note(
                "No active goal. Pi still has queued goal work.",
            ))
            .child(
                controls::divider_list().children(goal.queue.iter().enumerate().map(
                    |(index, item)| {
                        controls::queue_row(
                            format!("GOAL {:02}", index + 1),
                            item.objective.clone(),
                        )
                    },
                )),
            )
            .into_any_element();
    };
    let goal_id = active.id.clone();
    let pause_id = goal_id.clone();
    let resume_id = goal_id.clone();
    let clear_id = goal_id.clone();
    let resumable = matches!(
        active.status.as_str(),
        "paused" | "blocked" | "usage_limited" | "budget_limited"
    );
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Goal"))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(goal_status_color(&active.status))
                        .child(active.status.clone()),
                ),
        )
        .child(
            div()
                .p(px(10.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::edge_soft())
                .flex()
                .flex_col()
                .gap(px(7.0))
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(gpui::relative(1.45))
                        .text_color(theme::bone())
                        .child(active.objective.clone()),
                )
                .child(goal_metrics(active, goal.queue.len()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(6.0))
                        .child(controls::chip_button(
                            format!("goal-pause-{goal_id}"),
                            "Pause",
                            false,
                            active.status == "active",
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.dispatch_orchestration_action(
                                    OrchestrationAction::GoalPause {
                                        goal_id: pause_id.clone(),
                                    },
                                    cx,
                                );
                            })),
                        ))
                        .child(controls::chip_button(
                            format!("goal-resume-{goal_id}"),
                            "Resume",
                            false,
                            resumable,
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.dispatch_orchestration_action(
                                    OrchestrationAction::GoalResume {
                                        goal_id: resume_id.clone(),
                                    },
                                    cx,
                                );
                            })),
                        ))
                        .child(controls::chip_button(
                            format!("goal-clear-{goal_id}"),
                            "Clear",
                            false,
                            true,
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.dispatch_orchestration_action(
                                    OrchestrationAction::GoalClear {
                                        goal_id: clear_id.clone(),
                                    },
                                    cx,
                                );
                            })),
                        )),
                ),
        )
        .child(goal_edit_composer.clone())
        .when(!goal.queue.is_empty(), |panel| {
            panel.child(
                controls::divider_list().children(goal.queue.iter().enumerate().map(
                    |(index, item)| {
                        controls::queue_row(
                            format!("GOAL {:02}", index + 1),
                            item.objective.clone(),
                        )
                    },
                )),
            )
        })
        .into_any_element()
}

fn goal_metrics(goal: &GoalItemSnapshot, queued: usize) -> impl IntoElement {
    let budget = goal
        .token_budget
        .map(|budget| format!("{} / {} tokens", goal.tokens_used, budget))
        .unwrap_or_else(|| format!("{} tokens", goal.tokens_used));
    div()
        .font_family(theme::MONO)
        .text_size(px(theme::T_TINY))
        .text_color(theme::ash())
        .child(format!(
            "{} · {} elapsed · iteration {} · {} queued",
            budget,
            format_elapsed(goal.time_used_seconds),
            goal.iteration,
            queued
        ))
}

fn task_status_color(status: TaskStatus, blockers: usize) -> gpui::Rgba {
    if blockers > 0 {
        return theme::signal();
    }
    match status {
        TaskStatus::Pending => theme::ash(),
        TaskStatus::InProgress => theme::live(),
        TaskStatus::Completed => theme::smoke(),
    }
}

fn subagent_status_color(status: SubagentStatus) -> gpui::Rgba {
    match status {
        SubagentStatus::Queued => theme::data(),
        SubagentStatus::Running => theme::live(),
        SubagentStatus::Completed | SubagentStatus::Steered => theme::smoke(),
        SubagentStatus::Aborted | SubagentStatus::Stopped => theme::signal(),
        SubagentStatus::Error => theme::error(),
    }
}

fn agent_type_color(agent_type: &str) -> gpui::Rgba {
    match agent_type {
        "explore" | "research" => theme::live(),
        "plan" | "review" => theme::data(),
        _ => theme::signal(),
    }
}

fn goal_status_color(status: &str) -> gpui::Rgba {
    match status {
        "active" => theme::live(),
        "paused" | "usage_limited" | "budget_limited" => theme::signal(),
        "complete" | "completed" => theme::smoke(),
        "blocked" | "error" => theme::error(),
        _ => theme::ash(),
    }
}

fn format_elapsed(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{}s", seconds % 60)
    }
}

fn subagent_dialog(
    agent: Option<&SubagentSnapshot>,
    requested_id: &str,
    focus: &FocusHandle,
    scroll: &ScrollHandle,
    composer: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let header = agent
        .map(|agent| {
            (
                agent.agent_type.clone(),
                agent.description.clone(),
                agent.status.label().to_owned(),
                subagent_status_color(agent.status),
            )
        })
        .unwrap_or_else(|| {
            (
                "Subagent".to_owned(),
                "This agent ID is no longer present in Pi's authoritative store.".to_owned(),
                "Stale ID".to_owned(),
                theme::error(),
            )
        });
    let active = agent.is_some_and(|agent| agent.status.is_active());
    let stop_id = requested_id.to_owned();

    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .occlude()
        .bg(gpui::rgba(0x0b0a_09ed))
        .flex()
        .items_center()
        .justify_center()
        .p(px(28.0))
        .track_focus(focus)
        .tab_index(0)
        .on_key_down(cx.listener(RootView::on_subagent_dialog_key_down))
        .child(
            div()
                .id("subagent-conversation-dialog")
                .w_full()
                .max_w(px(980.0))
                .h_full()
                .max_h(px(780.0))
                .rounded(px(theme::RADIUS))
                .bg(theme::floor())
                .border_1()
                .border_color(theme::edge_hard())
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .h(px(62.0))
                        .px(px(18.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(16.0))
                        .border_b_1()
                        .border_color(theme::edge_hard())
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(3.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .gap(px(9.0))
                                        .child(
                                            div()
                                                .font_family(theme::SANS)
                                                .text_size(px(theme::T_TITLE))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(agent_type_color(&header.0))
                                                .child(header.0.clone()),
                                        )
                                        .child(
                                            div()
                                                .font_family(theme::MONO)
                                                .text_size(px(theme::T_TINY))
                                                .text_color(theme::smoke())
                                                .child(requested_id.to_owned()),
                                        ),
                                )
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_UI_SM))
                                        .text_color(theme::ash())
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(header.1.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(controls::status_pill(header.2.clone(), header.3))
                                .when(active, |row| {
                                    row.child(controls::quiet_button(
                                        "subagent-stop",
                                        "Stop",
                                        true,
                                        Box::new(cx.listener(move |view, _, _, cx| {
                                            view.dispatch_orchestration_action(
                                                OrchestrationAction::SubagentStop {
                                                    agent_id: stop_id.clone(),
                                                },
                                                cx,
                                            );
                                        })),
                                    ))
                                })
                                .child(controls::quiet_button(
                                    "subagent-close",
                                    "Close · Esc",
                                    true,
                                    Box::new(cx.listener(|view, _, window, cx| {
                                        view.close_subagent(window, cx);
                                    })),
                                )),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_row()
                        .child(
                            div()
                                .id("subagent-live-transcript")
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_y_scroll()
                                .scrollbar_width(px(theme::SCROLLBAR))
                                .track_scroll(scroll)
                                .child(
                                    div()
                                        .w_full()
                                        .max_w(px(720.0))
                                        .mx_auto()
                                        .px(px(24.0))
                                        .py(px(22.0))
                                        .flex()
                                        .flex_col()
                                        .gap(px(18.0))
                                        .when(agent.is_none(), |transcript| {
                                            transcript.child(controls::empty_list_note(
                                                "Close this view and open a current agent from the Inspector.",
                                            ))
                                        })
                                        .when_some(agent, |transcript, agent| {
                                            transcript
                                                .when(agent.transcript.is_empty(), |transcript| {
                                                    transcript.child(controls::empty_list_note(
                                                        if agent.status == SubagentStatus::Queued {
                                                            "Queued. Live output will appear when Pi starts the agent."
                                                        } else {
                                                            "Pi has not emitted conversation output for this agent yet."
                                                        },
                                                    ))
                                                })
                                                .children(agent.transcript.iter().map(
                                                    subagent_transcript_entry,
                                                ))
                                                .when(agent.transcript_truncated, |transcript| {
                                                    transcript.child(
                                                        div()
                                                            .font_family(theme::MONO)
                                                            .text_size(px(theme::T_TINY))
                                                            .text_color(theme::smoke())
                                                            .child(
                                                                "Earlier output is truncated; the live tail is shown.",
                                                            ),
                                                    )
                                                })
                                        }),
                                ),
                        )
                        .when_some(agent, |layout, agent| {
                            layout.child(subagent_metadata_panel(agent))
                        }),
                )
                .when(agent.is_some(), |dialog| {
                    dialog.child(
                        div()
                            .flex_shrink_0()
                            .px(px(18.0))
                            .py(px(12.0))
                            .border_t_1()
                            .border_color(theme::edge_hard())
                            .bg(theme::panel())
                            .child(composer.clone()),
                    )
                }),
        )
}

fn subagent_transcript_entry(
    entry: &crate::orchestration::SubagentTranscriptEntry,
) -> impl IntoElement {
    let (label, color, background) = match entry.role {
        TranscriptRole::User => ("YOU", theme::signal(), theme::panel_lift()),
        TranscriptRole::Assistant => ("AGENT", theme::live(), theme::panel()),
        TranscriptRole::ToolResult => ("TOOL", theme::data(), theme::canvas()),
        TranscriptRole::System => ("SYSTEM", theme::ash(), theme::canvas()),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(8.0))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::BOLD)
                        .text_color(if entry.is_error {
                            theme::error()
                        } else {
                            color
                        })
                        .child(label),
                )
                .when_some(entry.tool_name.clone(), |row, tool| {
                    row.child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::smoke())
                            .child(tool),
                    )
                }),
        )
        .child(
            div()
                .p(px(12.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(background)
                .border_1()
                .border_color(if entry.is_error {
                    theme::error()
                } else {
                    theme::edge_soft()
                })
                .font_family(if entry.role == TranscriptRole::ToolResult {
                    theme::MONO
                } else {
                    theme::SANS
                })
                .text_size(px(theme::T_UI))
                .line_height(gpui::relative(1.5))
                .text_color(theme::bone_dim())
                .child(entry.content.clone()),
        )
}

fn subagent_metadata_panel(agent: &SubagentSnapshot) -> impl IntoElement {
    div()
        .id("subagent-metadata")
        .w(px(230.0))
        .flex_shrink_0()
        .h_full()
        .overflow_y_scroll()
        .px(px(14.0))
        .py(px(18.0))
        .border_l_1()
        .border_color(theme::edge_hard())
        .bg(theme::panel())
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(controls::section_label("Run details"))
        .child(controls::metric_row(
            "Tool uses",
            agent.tool_uses.to_string(),
        ))
        .child(controls::metric_row(
            "Concurrency",
            agent.max_concurrent.to_string(),
        ))
        .when_some(agent.queue_position, |panel, position| {
            panel.child(controls::metric_row("Queue", position.to_string()))
        })
        .when_some(agent.output_file.clone(), |panel, output| {
            panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(controls::section_label("Output"))
                    .child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::ash())
                            .child(short_path(&output)),
                    ),
            )
        })
        .when_some(agent.worktree.as_ref(), |panel, worktree| {
            panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(controls::section_label("Worktree"))
                    .child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::ash())
                            .child(format!(
                                "{}\n{}",
                                worktree.branch,
                                short_path(&worktree.work_path)
                            )),
                    )
                    .when_some(agent.worktree_result.as_ref(), |detail, result| {
                        detail.child(
                            div()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_UI_SM))
                                .text_color(if result.has_changes {
                                    theme::live()
                                } else {
                                    theme::smoke()
                                })
                                .child(if result.has_changes {
                                    "Changes available"
                                } else {
                                    "No changes"
                                }),
                        )
                    }),
            )
        })
        .when_some(agent.memory.as_ref(), |panel, memory| {
            panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(controls::section_label("Memory"))
                    .child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::ash())
                            .child(match memory.path.as_deref() {
                                Some(path) => format!("{} · {}", memory.scope, short_path(path)),
                                None => memory.scope.clone(),
                            }),
                    ),
            )
        })
        .when_some(agent.result.clone(), |panel, result| {
            panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(controls::section_label("Result"))
                    .child(
                        div()
                            .font_family(theme::SANS)
                            .text_size(px(theme::T_UI_SM))
                            .line_height(gpui::relative(1.4))
                            .text_color(theme::bone_dim())
                            .child(result),
                    ),
            )
        })
        .when_some(agent.error.clone(), |panel, error| {
            panel.child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_UI_SM))
                    .line_height(gpui::relative(1.4))
                    .text_color(theme::error())
                    .child(error),
            )
        })
}

fn run_controls(
    conversation: &ConversationProjection,
    compaction_composer: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let locked = conversation.pending_operation.is_some();
    let can_run_controls = conversation.steering_mode.is_some();
    let compact_enabled = matches!(
        conversation.lifecycle,
        RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
    ) && !locked
        && !matches!(conversation.compaction, CompactionState::Running { .. });
    let abort_enabled = conversation.lifecycle == RuntimeLifecycle::Running;
    let abort_retry_enabled = matches!(conversation.retry, RetryState::Waiting { .. });
    let bash_running = conversation
        .bash_executions
        .iter()
        .any(|execution| execution.status == BashStatus::Running);

    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Run"))
                .when_some(conversation.pending_operation.as_ref(), |row, operation| {
                    row.child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::data())
                            .child(operation_label(operation)),
                    )
                }),
        )
        .child(
            controls::divider_list()
                .child(controls::action_row(
                    "abort-run",
                    "Abort run",
                    if abort_enabled {
                        "Current agent only"
                    } else {
                        "No active run"
                    },
                    abort_enabled,
                    controls::ControlTone::Danger,
                    Box::new(cx.listener(|view, _, window, cx| {
                        let _ = view.execute_native_action(NativeAction::Abort, "", window, cx);
                    })),
                ))
                .child(controls::action_row(
                    "abort-bash",
                    "Abort Bash",
                    if bash_running {
                        "Direct Bash only"
                    } else {
                        "No Bash running"
                    },
                    bash_running,
                    controls::ControlTone::Danger,
                    Box::new(cx.listener(|view, _, window, cx| {
                        let _ = view.execute_native_action(NativeAction::Abort, "", window, cx);
                    })),
                ))
                .child(controls::action_row(
                    "abort-retry",
                    "Abort retry",
                    if abort_retry_enabled {
                        "Retry timer only"
                    } else {
                        "No retry timer"
                    },
                    abort_retry_enabled,
                    controls::ControlTone::Danger,
                    Box::new(cx.listener(|view, _, _, cx| view.abort_retry(cx))),
                ))
                .child(controls::action_row(
                    "compact-now",
                    "Compact",
                    if compact_enabled {
                        "Manual context summary"
                    } else {
                        "Wait until idle"
                    },
                    compact_enabled,
                    controls::ControlTone::Normal,
                    Box::new(cx.listener(|view, _, window, cx| {
                        let _ = view.execute_native_action(NativeAction::Compact, "", window, cx);
                    })),
                )),
        )
        .child(compaction_composer.clone())
        .child(mode_controls(
            "steering",
            "Steering",
            conversation.steering_mode,
            locked || !can_run_controls,
            |view, mode, cx| view.set_steering_mode(mode, cx),
            cx,
        ))
        .child(mode_controls(
            "follow-up",
            "Follow-up",
            conversation.follow_up_mode,
            locked || !can_run_controls,
            |view, mode, cx| view.set_follow_up_mode(mode, cx),
            cx,
        ))
        .child(toggle_row(
            "auto-compaction",
            "Auto compaction",
            conversation.auto_compaction_enabled,
            locked || !can_run_controls,
            |view, enabled, cx| view.set_auto_compaction(enabled, cx),
            cx,
        ))
        .child(toggle_row(
            "auto-retry",
            "Auto retry",
            conversation.auto_retry_enabled,
            locked || !can_run_controls,
            |view, enabled, cx| view.set_auto_retry(enabled, cx),
            cx,
        ))
}

fn mode_controls(
    prefix: &'static str,
    title: &'static str,
    current: Option<QueueDeliveryMode>,
    locked: bool,
    apply: fn(&mut RootView, QueueDeliveryMode, &mut Context<RootView>),
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let current_label = current.map(mode_label).unwrap_or("Unknown");
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .child(controls::section_label(title))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(current_label),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(6.0))
                .child(controls::chip_button(
                    format!("{prefix}-all"),
                    "All",
                    current == Some(QueueDeliveryMode::All),
                    current.is_some() && !locked && current != Some(QueueDeliveryMode::All),
                    Box::new(
                        cx.listener(move |view, _, _, cx| apply(view, QueueDeliveryMode::All, cx)),
                    ),
                ))
                .child(controls::chip_button(
                    format!("{prefix}-one"),
                    "One at a time",
                    current == Some(QueueDeliveryMode::OneAtATime),
                    current.is_some() && !locked && current != Some(QueueDeliveryMode::OneAtATime),
                    Box::new(cx.listener(move |view, _, _, cx| {
                        apply(view, QueueDeliveryMode::OneAtATime, cx)
                    })),
                )),
        )
}

fn toggle_row(
    prefix: &'static str,
    title: &'static str,
    current: Option<bool>,
    locked: bool,
    apply: fn(&mut RootView, bool, &mut Context<RootView>),
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let current_label = current.map(on_off).unwrap_or("Unknown");
    let target = !current.unwrap_or(true);
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(controls::section_label(title))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(current_label),
                ),
        )
        .child(controls::chip_button(
            format!("{prefix}-toggle"),
            if target { "Enable" } else { "Disable" },
            false,
            current.is_some() && !locked,
            Box::new(cx.listener(move |view, _, _, cx| apply(view, target, cx))),
        ))
}

fn queue_panel(conversation: &ConversationProjection) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Queue"))
                .when(conversation.context_awaiting_fresh_usage, |row| {
                    row.child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::data())
                            .child("awaiting usage"),
                    )
                }),
        )
        .child(match &conversation.queue {
            QueueContents::Unknown { pending_count } => controls::divider_list()
                .child(controls::empty_list_note(format!(
                    "Pi reports {pending_count} queued item{}",
                    plural(*pending_count)
                )))
                .into_any_element(),
            QueueContents::Known {
                steering,
                follow_up,
            } => controls::divider_list()
                .when(steering.is_empty() && follow_up.is_empty(), |list| {
                    list.child(controls::empty_list_note("Nothing queued."))
                })
                .children(steering.iter().enumerate().map(|(index, item)| {
                    div()
                        .when(index + 1 < steering.len() || !follow_up.is_empty(), |row| {
                            row.border_b_1().border_color(theme::edge_soft())
                        })
                        .child(controls::queue_row(
                            format!("STEER {:02}", index + 1),
                            item.clone(),
                        ))
                }))
                .children(follow_up.iter().enumerate().map(|(index, item)| {
                    div()
                        .when(index + 1 < follow_up.len(), |row| {
                            row.border_b_1().border_color(theme::edge_soft())
                        })
                        .child(controls::queue_row(
                            format!("FOLLOW {:02}", index + 1),
                            item.clone(),
                        ))
                }))
                .into_any_element(),
        })
}

fn runtime_error_notice(projection: &ShellProjection) -> impl IntoElement {
    div()
        .mx(px(theme::STREAM_PAD_X))
        .mt(px(14.0))
        .p(px(12.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::error())
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::error())
                .child(projection.headline.clone()),
        )
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .line_height(gpui::relative(1.4))
                .text_color(theme::bone_dim())
                .child(projection.detail.clone()),
        )
}

fn short_path(path: &str) -> String {
    let mut parts = path.rsplit(['\\', '/']).filter(|part| !part.is_empty());
    let Some(name) = parts.next() else {
        return path.to_owned();
    };
    let Some(parent) = parts.next() else {
        return path.to_owned();
    };
    if parts.next().is_none() {
        path.to_owned()
    } else {
        format!("…\\{parent}\\{name}")
    }
}

fn context_pct(label: &str) -> Option<f32> {
    let cleaned = label
        .split('·')
        .next()
        .unwrap_or(label)
        .trim()
        .replace(',', "");
    if let Some((used, total)) = cleaned.split_once('/') {
        let used = used.trim().parse::<f32>().ok()?;
        let total = total.trim().parse::<f32>().ok()?;
        if total > 0.0 {
            return Some((used / total).clamp(0.0, 1.0));
        }
    }
    cleaned
        .trim_end_matches('%')
        .parse::<f32>()
        .ok()
        .map(|value| (value / 100.0).clamp(0.0, 1.0))
}

fn operation_label(operation: &RuntimeOperation) -> &'static str {
    match operation {
        RuntimeOperation::SetModel { .. } => "Switching model",
        RuntimeOperation::SetThinkingLevel(_) => "Changing thinking",
        RuntimeOperation::SetSteeringMode(_) => "Changing steering mode",
        RuntimeOperation::SetFollowUpMode(_) => "Changing follow-up mode",
        RuntimeOperation::Compact => "Compacting",
        RuntimeOperation::SetAutoCompaction(_) => "Changing auto compaction",
        RuntimeOperation::SetAutoRetry(_) => "Changing auto retry",
        RuntimeOperation::SetSessionName(_) => "Renaming session",
        RuntimeOperation::ExportHtml => "Exporting session",
    }
}

fn mode_label(mode: QueueDeliveryMode) -> &'static str {
    match mode {
        QueueDeliveryMode::All => "All",
        QueueDeliveryMode::OneAtATime => "One at a time",
    }
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "On" } else { "Off" }
}

fn plural(count: u64) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn action_id(action: RecoveryAction) -> &'static str {
    match action {
        RecoveryAction::Connect => "runtime-connect",
        RecoveryAction::Retry => "runtime-retry",
        RecoveryAction::Stop => "runtime-stop",
    }
}

fn lifecycle_color(projection: &ShellProjection) -> gpui::Rgba {
    match projection.lifecycle.as_str() {
        "Ready" => theme::live(),
        "Running" | "Loading" | "Connecting" | "Cancelling" | "Stopping" => theme::data(),
        "Connection error" | "No model" => theme::error(),
        "Not connected" | "Stopped" => theme::ash(),
        _ => theme::bone_dim(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExtensionDialogKey, extension_dialog_key, model_choices, short_path, thinking_choices,
    };
    use crate::controller::{ModelRuntimeProjection, UsageProjection};
    use crate::model_runtime::{CatalogPhase, ModelChangePolicy, ModelIdentity, ThinkingLevel};
    use crate::state::runtime::{ModelSummary, RuntimeThinkingLevel};

    #[test]
    fn short_path_preserves_short_values_and_truncates_deep_values() {
        assert_eq!(short_path(r"C:\workspace"), r"C:\workspace");
        assert_eq!(short_path(r"C:\workspace\pi-gui"), r"…\workspace\pi-gui");
        assert_eq!(short_path("/work/pi-gui/src"), r"…\pi-gui\src");
    }

    #[test]
    fn context_pct_parses_token_ratios_and_percentages() {
        assert_eq!(
            super::context_pct("12,345 / 200,000"),
            Some(12345.0 / 200_000.0)
        );
        assert_eq!(super::context_pct("42%"), Some(0.42));
        assert_eq!(super::context_pct("Awaiting"), None);
        assert_eq!(super::context_pct("1,000 / 2,000 · stale"), Some(0.5));
    }

    #[test]
    fn model_choices_fall_back_to_stock_rpc_models() {
        let projection = ModelRuntimeProjection {
            phase: CatalogPhase::Failed("SDK catalog unavailable".into()),
            catalog: None,
            stock_models: vec![ModelSummary {
                provider: "openai".into(),
                id: "gpt-test".into(),
                name: "GPT Test".into(),
                reasoning: true,
                supported_thinking: vec![
                    RuntimeThinkingLevel::Off,
                    RuntimeThinkingLevel::Low,
                    RuntimeThinkingLevel::High,
                ],
                context_window: 128_000,
                max_tokens: 8_192,
                supports_images: true,
            }],
            auth: None,
            feedback: None,
            active_model: Some(ModelIdentity {
                provider: "openai".into(),
                id: "gpt-test".into(),
            }),
            active_thinking: Some(ThinkingLevel::Medium),
            requested_thinking: None,
            effective_thinking: Some(ThinkingLevel::Medium),
            clamp_notice: None,
            model_change_policy: ModelChangePolicy::Allowed,
            usage: UsageProjection {
                context_tokens: None,
                context_window: None,
                context_percent: None,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                total_tokens: None,
                estimated_cost: None,
                pricing_known: false,
            },
        };

        let choices = model_choices(&projection, "gpt");
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].identity.provider, "openai");
        assert_eq!(choices[0].identity.id, "gpt-test");
        assert_eq!(choices[0].context_window, 128_000);
        assert_eq!(
            thinking_choices(&projection),
            vec![ThinkingLevel::Off, ThinkingLevel::Low, ThinkingLevel::High]
        );
    }

    #[test]
    fn extension_dialog_keyboard_routes_are_modal_and_kind_aware() {
        assert_eq!(
            extension_dialog_key(Some("select"), "tab"),
            Some(ExtensionDialogKey::ContainFocus)
        );
        assert_eq!(
            extension_dialog_key(Some("input"), "tab"),
            Some(ExtensionDialogKey::ContainFocus)
        );
        assert_eq!(
            extension_dialog_key(Some("editor"), "escape"),
            Some(ExtensionDialogKey::Cancel)
        );
        assert_eq!(
            extension_dialog_key(Some("select"), "up"),
            Some(ExtensionDialogKey::Move(-1))
        );
        assert_eq!(
            extension_dialog_key(Some("confirm"), "right"),
            Some(ExtensionDialogKey::Move(1))
        );
        assert_eq!(
            extension_dialog_key(Some("confirm"), "enter"),
            Some(ExtensionDialogKey::AcceptSelection)
        );
        assert_eq!(extension_dialog_key(Some("input"), "enter"), None);
    }
}
