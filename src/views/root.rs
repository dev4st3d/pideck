//! Live Pi shell with an authoritative streaming conversation.

use std::cell::Cell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui::{
    Bounds, ClipboardItem, Context, DispatchPhase, Entity, FocusHandle, Focusable, FontWeight,
    Image, ImageFormat, IntoElement, ListAlignment, ListOffset, ListState, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, PathBuilder, Pixels, Render,
    ScrollHandle, ScrollWheelEvent, StyledImage, Subscription, Task, Window, canvas, deferred, div,
    fill, img, list, point, prelude::*, px, size,
};

use crate::actions::{
    AbortRun, ActivateRecovery, Connect, FocusNext, FocusPrevious, HistoryActivate, HistoryFirst,
    HistoryFold, HistoryLast, HistoryNext, HistoryPrevious, HistoryUnfold, ImagePreviewClose,
    ImagePreviewNext, ImagePreviewPrevious, ORCHESTRATION_ROW_CONTEXT, OpenCommandPalette,
    OrchestrationActivate, Retry, ShowHotkeys, Stop,
};
use crate::command_catalog::{
    CommandCatalog, CommandEntry, CommandTarget, InvocationResolution, NativeAction,
};
use crate::controller::{
    AcceptedSubmission, AcceptedSubmissionKind, BridgeProjection, CatalogProjection, CatalogStatus,
    CommandCatalogProjection, ComposerRuntime, ConversationProjection, ExtensionUiProjection,
    HistoryProjection, ModelRuntimeProjection, OrchestrationProjection, ResourceCenterProjection,
    RuntimeController, SubmissionPreference,
};
use crate::fonts::{self, FontCatalog, FontRole};
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
    ActivityDisclosureState, ConversationListModel, ConversationScrollMotion, TranscriptTextCache,
};

mod composer_bar;
mod inspector;
mod model_panels;
mod overlays;
mod render;
mod shared;
mod shell;

use overlays::{annotate_prompt_image, extension_dialog_key, single_line_title, wrapped_index};

struct PendingDraft {
    request: crate::services::rpc::RequestId,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ImagePoint {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone)]
struct PencilStroke {
    image_index: usize,
    points: Vec<ImagePoint>,
    color: PencilColor,
    size: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PencilColor {
    Red,
    Amber,
    Green,
    Blue,
    White,
    Black,
}

impl PencilColor {
    const ALL: [Self; 6] = [
        Self::Red,
        Self::Amber,
        Self::Green,
        Self::Blue,
        Self::White,
        Self::Black,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Amber => "Amber",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::White => "White",
            Self::Black => "Black",
        }
    }

    fn rgb(self) -> u32 {
        match self {
            Self::Red => 0xe35d5b,
            Self::Amber => 0xe0a84f,
            Self::Green => 0x69ad7c,
            Self::Blue => 0x5f8fce,
            Self::White => 0xf4f0e8,
            Self::Black => 0x171513,
        }
    }

    fn rgba8(self) -> [u8; 4] {
        let rgb = self.rgb();
        [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255]
    }
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
    Typography,
    Resources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtensionDialogKey {
    Cancel,
    ContainFocus,
    Move(isize),
    AcceptSelection,
}

/// Which queue-delivery control the inspector is editing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryFocus {
    Steering,
    FollowUp,
}

struct RenderProjections {
    shell: ShellProjection,
    catalog: CatalogProjection,
    history: HistoryProjection,
    bridge: BridgeProjection,
    models: ModelRuntimeProjection,
    resources: ResourceCenterProjection,
    orchestration: OrchestrationProjection,
}

impl RenderProjections {
    fn read(controller: &RuntimeController) -> Self {
        Self {
            shell: controller.projection(),
            catalog: controller.catalog_projection(),
            history: controller.history_projection(),
            bridge: controller.bridge_projection(),
            models: controller.model_runtime_projection(),
            resources: controller.resource_center_projection(),
            orchestration: controller.orchestration_projection(),
        }
    }
}

pub struct RootView {
    controller: Entity<RuntimeController>,
    render_projections: RenderProjections,
    composer: Entity<Composer>,
    compaction_composer: Entity<Composer>,
    session_name_composer: Entity<Composer>,
    history_search_composer: Entity<Composer>,
    history_label_composer: Entity<Composer>,
    import_path_composer: Entity<Composer>,
    model_search_composer: Entity<Composer>,
    font_search_composer: Entity<Composer>,
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
    font_catalog: FontCatalog,
    font_role: FontRole,
    font_feedback: Option<String>,
    font_save_generation: u64,
    command_palette_open: bool,
    hotkey_help_open: bool,
    compaction_modal_open: bool,
    pasted_image_preview: Option<usize>,
    pencil_enabled: bool,
    pencil_color: PencilColor,
    pencil_size: u16,
    pencil_stroke: Option<PencilStroke>,
    pencil_undo: Vec<PromptImage>,
    pencil_error: Option<String>,
    command_selection: usize,
    command_catalog_source: CommandCatalogProjection,
    command_catalog: CommandCatalog,
    command_palette_matches: Vec<CommandEntry>,
    slash_command_matches: Vec<CommandEntry>,
    slash_intercepts_enter: bool,
    command_palette_scroll: ScrollHandle,
    slash_command_scroll: ScrollHandle,
    model_switcher_scroll: ScrollHandle,
    thinking_select_scroll: ScrollHandle,
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
    /// Inspector edits one queue delivery mode at a time (steering or follow-up).
    delivery_focus: DeliveryFocus,
    usage_tooltip_hovered: bool,
    usage_tooltip_visible: bool,
    usage_tooltip_epoch: u64,
    sessions_scroll: ScrollHandle,
    sessions_scroll_motion: ConversationScrollMotion,
    subagent_dialog_focus: FocusHandle,
    subagent_dialog_scroll: ScrollHandle,
    window_title: String,
    history: HistoryBrowser,
    history_focus: FocusHandle,
    history_open: bool,
    session_rename_open: bool,
    session_menu_open: bool,
    history_confirmation: Option<HistoryConfirmation>,
    summarize_navigation: bool,
    conversation: Arc<ConversationProjection>,
    conversation_list: Arc<ConversationListModel>,
    conversation_list_state: ListState,
    conversation_follow: Rc<Cell<bool>>,
    conversation_scroll_motion: ConversationScrollMotion,
    conversation_scrollbar_drag_offset: Option<Pixels>,
    transcript_cache: Entity<TranscriptTextCache>,
    activity_disclosures: Entity<ActivityDisclosureState>,
    pending_draft: Option<PendingDraft>,
    pending_bash: Option<crate::services::rpc::RequestId>,
    pending_compaction_focus: Option<String>,
    pending_session_name: Option<String>,
    focus_handle: FocusHandle,
    _controller_observation: Subscription,
    _activity_disclosure_observation: Subscription,
    _composer_subscription: Subscription,
    _compaction_subscription: Subscription,
    _session_name_subscription: Subscription,
    _history_search_observation: Subscription,
    _history_label_subscription: Subscription,
    _import_path_subscription: Subscription,
    _model_search_observation: Subscription,
    _font_search_observation: Subscription,
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
        font_catalog: FontCatalog,
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
            .allowing_empty_submit()
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
            Composer::field("model-search", "Search models…", "", cx).with_field_height(26.0)
        });
        let font_search_composer = cx.new(|cx| {
            Composer::field("font-search", "Search system fonts…", "", cx).with_field_height(30.0)
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
        let (conversation, extension_ui, render_projections, command_catalog_source) = {
            let controller = controller.read(cx);
            (
                controller.conversation_projection(),
                controller.extension_ui_projection(),
                RenderProjections::read(controller),
                controller.command_catalog_projection(),
            )
        };
        let command_catalog = CommandCatalog::build(
            &command_catalog_source.status,
            &command_catalog_source.commands,
        );
        let command_palette_matches = command_catalog.filtered("").into_iter().cloned().collect();
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
        let activity_disclosures = cx.new(|_| ActivityDisclosureState::new(conversation.epoch));
        window.focus(&composer.read(cx).focus_handle(cx));
        let controller_observation = cx.observe_in(&controller, window, |view, _, window, cx| {
            view.sync_runtime(window, cx)
        });
        let activity_disclosure_observation =
            cx.observe(&activity_disclosures, |_, _, cx| cx.notify());
        let composer_subscription =
            cx.subscribe_in(&composer, window, |view, _, event, window, cx| {
                view.on_composer_event(event, window, cx)
            });
        let compaction_subscription = cx.subscribe_in(
            &compaction_composer,
            window,
            |view, _, event, window, cx| view.on_compaction_event(event, window, cx),
        );
        let session_name_subscription = cx.subscribe_in(
            &session_name_composer,
            window,
            |view, _, event, window, cx| view.on_session_name_event(event, window, cx),
        );
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
        let font_search_observation =
            cx.observe_in(&font_search_composer, window, |_, _, _, cx| cx.notify());
        let composer_observation = cx.observe_in(&composer, window, |view, _, _, cx| {
            view.sync_slash_completion(cx)
        });
        let command_search_observation =
            cx.observe_in(&command_search_composer, window, |view, _, _, cx| {
                view.refresh_command_palette_matches(cx);
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
        window.set_window_title("Pideck");
        Self {
            controller,
            render_projections,
            composer,
            compaction_composer,
            session_name_composer,
            history_search_composer,
            history_label_composer,
            import_path_composer,
            model_search_composer,
            font_search_composer,
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
            font_feedback: font_catalog.load_warning.clone(),
            font_catalog,
            font_role: FontRole::Sans,
            font_save_generation: 0,
            command_palette_open: false,
            hotkey_help_open: false,
            compaction_modal_open: false,
            pasted_image_preview: None,
            pencil_enabled: false,
            pencil_color: PencilColor::Red,
            pencil_size: 6,
            pencil_stroke: None,
            pencil_undo: Vec::new(),
            pencil_error: None,
            command_selection: 0,
            command_catalog_source,
            command_catalog,
            command_palette_matches,
            slash_command_matches: Vec::new(),
            slash_intercepts_enter: false,
            command_palette_scroll: ScrollHandle::new(),
            slash_command_scroll: ScrollHandle::new(),
            model_switcher_scroll: ScrollHandle::new(),
            thinking_select_scroll: ScrollHandle::new(),
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
            delivery_focus: DeliveryFocus::Steering,
            usage_tooltip_hovered: false,
            usage_tooltip_visible: false,
            usage_tooltip_epoch: 0,
            sessions_scroll: ScrollHandle::new(),
            sessions_scroll_motion: ConversationScrollMotion::default(),
            subagent_dialog_focus,
            subagent_dialog_scroll: ScrollHandle::new(),
            window_title: "Pideck".to_owned(),
            history: HistoryBrowser::default(),
            history_focus,
            history_open: false,
            session_rename_open: false,
            session_menu_open: false,
            history_confirmation: None,
            summarize_navigation: false,
            conversation: Arc::new(conversation),
            conversation_list: Arc::new(conversation_list),
            conversation_list_state,
            conversation_follow,
            conversation_scroll_motion: ConversationScrollMotion::default(),
            conversation_scrollbar_drag_offset: None,
            transcript_cache,
            activity_disclosures,
            pending_draft: None,
            pending_bash: None,
            pending_compaction_focus: None,
            pending_session_name: None,
            focus_handle,
            _controller_observation: controller_observation,
            _activity_disclosure_observation: activity_disclosure_observation,
            _composer_subscription: composer_subscription,
            _compaction_subscription: compaction_subscription,
            _session_name_subscription: session_name_subscription,
            _history_search_observation: history_search_observation,
            _history_label_subscription: history_label_subscription,
            _import_path_subscription: import_path_subscription,
            _model_search_observation: model_search_observation,
            _font_search_observation: font_search_observation,
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

    fn set_usage_tooltip_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.usage_tooltip_hovered == hovered {
            return;
        }

        self.usage_tooltip_hovered = hovered;
        self.usage_tooltip_epoch = self.usage_tooltip_epoch.wrapping_add(1);
        let epoch = self.usage_tooltip_epoch;
        if hovered {
            self.usage_tooltip_visible = true;
            cx.notify();
            return;
        }

        cx.notify();
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(90))
                .await;
            let _ = view.update(cx, |view, cx| {
                if !view.usage_tooltip_hovered && view.usage_tooltip_epoch == epoch {
                    view.usage_tooltip_visible = false;
                    cx.notify();
                }
            });
        })
        .detach();
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
        if self.pasted_image_preview.is_some() {
            self.close_pasted_image(window, cx);
            return;
        }
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
            ComposerEvent::Accept { text, images } => {
                self.pasted_image_preview = None;
                self.execute_composer_text(
                    text.clone(),
                    images.clone(),
                    SubmissionPreference::Default,
                    window,
                    cx,
                )
            }
            ComposerEvent::FollowUp { text, images } => {
                self.pasted_image_preview = None;
                self.execute_composer_text(
                    text.clone(),
                    images.clone(),
                    SubmissionPreference::FollowUp,
                    window,
                    cx,
                )
            }
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
                cx.notify();
            }
            ComposerEvent::PreviewImage(index) => self.open_pasted_image(*index, window, cx),
        }
    }

    fn open_pasted_image(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.composer.read(cx).images().len() {
            return;
        }
        self.command_palette_open = false;
        self.hotkey_help_open = false;
        self.pasted_image_preview = Some(index);
        self.pencil_stroke = None;
        self.pencil_undo.clear();
        self.pencil_error = None;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn close_pasted_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pasted_image_preview.take().is_some() {
            self.pencil_stroke = None;
            self.pencil_undo.clear();
            self.pencil_error = None;
            window.focus(&self.composer.read(cx).focus_handle(cx));
            cx.notify();
        }
    }

    fn move_pasted_image(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(current) = self.pasted_image_preview else {
            return;
        };
        let count = self.composer.read(cx).images().len();
        if count > 1 {
            self.pasted_image_preview = Some(wrapped_index(current, count, delta));
            self.pencil_stroke = None;
            self.pencil_undo.clear();
            self.pencil_error = None;
            cx.notify();
        }
    }

    fn toggle_pencil(&mut self, cx: &mut Context<Self>) {
        self.pencil_enabled = !self.pencil_enabled;
        self.pencil_stroke = None;
        self.pencil_error = None;
        cx.notify();
    }

    fn set_pencil_color(&mut self, color: PencilColor, cx: &mut Context<Self>) {
        self.pencil_color = color;
        self.pencil_error = None;
        cx.notify();
    }

    fn adjust_pencil_size(&mut self, delta: i16, cx: &mut Context<Self>) {
        self.pencil_size = (self.pencil_size as i16 + delta).clamp(1, 64) as u16;
        self.pencil_error = None;
        cx.notify();
    }

    fn start_pencil_stroke(
        &mut self,
        image_index: usize,
        point: ImagePoint,
        cx: &mut Context<Self>,
    ) {
        if !self.pencil_enabled || self.pasted_image_preview != Some(image_index) {
            return;
        }
        self.pencil_stroke = Some(PencilStroke {
            image_index,
            points: vec![point],
            color: self.pencil_color,
            size: self.pencil_size,
        });
        self.pencil_error = None;
        cx.notify();
    }

    fn continue_pencil_stroke(
        &mut self,
        image_index: usize,
        point: ImagePoint,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(stroke) = self.pencil_stroke.as_mut() else {
            return false;
        };
        if stroke.image_index != image_index {
            return false;
        }
        let should_append = stroke.points.last().is_none_or(|last| {
            let dx = point.x - last.x;
            let dy = point.y - last.y;
            dx * dx + dy * dy >= 0.25
        });
        if should_append {
            stroke.points.push(point);
            cx.notify();
        }
        true
    }

    fn finish_pencil_stroke(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(stroke) = self.pencil_stroke.take() else {
            return false;
        };
        let Some(original) = self
            .composer
            .read(cx)
            .images()
            .get(stroke.image_index)
            .cloned()
        else {
            return false;
        };

        match annotate_prompt_image(&original, &stroke) {
            Ok(edited) => {
                let replacement = self.composer.update(cx, |composer, cx| {
                    composer.replace_image(stroke.image_index, edited, cx)
                });
                match replacement {
                    Ok(()) => {
                        self.pencil_undo.push(original);
                        if self.pencil_undo.len() > 20 {
                            self.pencil_undo.remove(0);
                        }
                        self.pencil_error = None;
                    }
                    Err(message) => self.pencil_error = Some(message.to_owned()),
                }
            }
            Err(message) => self.pencil_error = Some(message),
        }
        cx.notify();
        true
    }

    fn undo_pencil_stroke(&mut self, cx: &mut Context<Self>) {
        let (Some(index), Some(previous)) = (self.pasted_image_preview, self.pencil_undo.pop())
        else {
            return;
        };
        if let Err(message) = self.composer.update(cx, |composer, cx| {
            composer.replace_image(index, previous.clone(), cx)
        }) {
            self.pencil_undo.push(previous);
            self.pencil_error = Some(message.to_owned());
        } else {
            self.pencil_error = None;
        }
        cx.notify();
    }

    fn on_image_preview_previous(
        &mut self,
        _: &ImagePreviewPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_pasted_image(-1, cx);
    }

    fn on_image_preview_next(
        &mut self,
        _: &ImagePreviewNext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_pasted_image(1, cx);
    }

    fn on_image_preview_close(
        &mut self,
        _: &ImagePreviewClose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_pasted_image(window, cx);
    }

    fn command_catalog(&self) -> &CommandCatalog {
        &self.command_catalog
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

        let (matches, intercept_enter) =
            self.command_catalog().slash_completion(&draft).map_or_else(
                || (Vec::new(), false),
                |completion| {
                    (
                        completion.matches.into_iter().cloned().collect(),
                        completion.intercept_enter,
                    )
                },
            );
        self.slash_command_matches = matches;
        self.slash_intercepts_enter = intercept_enter;

        let active = !self.composer.read(cx).has_images()
            && self.dismissed_slash_draft.as_deref() != Some(draft.as_str())
            && self.slash_intercepts_enter;
        if active {
            self.command_selection = self
                .command_selection
                .min(self.slash_command_matches.len().saturating_sub(1));
        } else {
            self.command_selection = 0;
        }
        self.composer.update(cx, |composer, cx| {
            composer.set_command_completion_active(active, cx)
        });
        cx.notify();
    }

    fn refresh_command_palette_matches(&mut self, cx: &Context<Self>) {
        let query = self.command_search_composer.read(cx).draft().to_owned();
        let matches = self
            .command_catalog()
            .filtered(&query)
            .into_iter()
            .cloned()
            .collect();
        self.command_palette_matches = matches;
        self.command_selection = self
            .command_selection
            .min(self.command_palette_matches.len().saturating_sub(1));
    }

    fn move_command_selection(&mut self, delta: isize, palette: bool, cx: &mut Context<Self>) {
        let count = if palette {
            self.command_palette_matches.len()
        } else {
            self.slash_command_matches.len()
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
        let Some(entry) = self
            .slash_command_matches
            .get(self.command_selection)
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
        let resolution = match self.command_catalog().resolve(&text) {
            InvocationResolution::Command { entry, invocation } => {
                Ok(Some((entry.clone(), invocation.arguments)))
            }
            InvocationResolution::UnsupportedBuiltin(name) => Err(name),
            InvocationResolution::NotACommand => Ok(None),
        };
        match resolution {
            Ok(Some((entry, arguments))) => {
                self.execute_entry_with_preference(entry, arguments, preference, window, cx)
            }
            Err(name) => self.command_error(
                format!("/{name} is a TUI-only command and cannot run in the native RPC client."),
                window,
                cx,
            ),
            Ok(None) => self.submit(text, images, preference, cx),
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
            | ComposerEvent::CommandDismiss
            | ComposerEvent::PreviewImage(_) => {}
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

    fn sync_extension_ui(
        &mut self,
        projection: ExtensionUiProjection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            .unwrap_or_else(|| "Pideck".to_owned());
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
            let request = dialog.id.clone();
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
            NativeAction::NewSession => {
                self.session_rename_open = false;
                self.session_menu_open = false;
                self.controller
                    .update(cx, |controller, cx| controller.new_session(cx))
                    .then_some(())
                    .ok_or_else(|| "A new session cannot start in the current state.".to_owned())
            }
            NativeAction::Sessions => {
                self.open_session_rename(window, cx);
                Ok(())
            }
            NativeAction::Tree => {
                self.session_menu_open = false;
                self.history_open = !self.history_open;
                self.history_confirmation = None;
                if self.history_open {
                    self.sync_history_selection();
                    window.focus(&self.history_focus);
                } else {
                    window.focus(&self.composer.read(cx).focus_handle(cx));
                }
                cx.notify();
                Ok(())
            }
            NativeAction::Fork => {
                self.history_open = true;
                self.sync_history_selection();
                self.history_confirmation = None;
                window.focus(&self.history_focus);
                cx.notify();
                Ok(())
            }
            NativeAction::Clone => {
                self.history_open = true;
                self.sync_history_selection();
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

    fn open_compaction_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_compaction_focus.is_some() {
            return;
        }
        self.command_palette_open = false;
        self.hotkey_help_open = false;
        self.compaction_modal_open = true;
        self.compaction_composer.update(cx, |composer, cx| {
            composer.set_feedback(ComposerFeedback::Ready, cx)
        });
        window.focus(&self.compaction_composer.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn close_compaction_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_compaction_focus.is_some() {
            return;
        }
        self.compaction_modal_open = false;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn on_compaction_event(
        &mut self,
        event: &ComposerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComposerEvent::Accept { text, .. } => {
                if self.pending_compaction_focus.is_some() {
                    return;
                }
                let draft = text.trim().to_owned();
                let focus = (!draft.is_empty()).then(|| draft.clone());
                let accepted = self
                    .controller
                    .update(cx, |controller, cx| controller.compact(focus, cx));
                if accepted {
                    self.pending_compaction_focus = Some(draft);
                    self.compaction_composer.update(cx, |composer, cx| {
                        composer.set_feedback(ComposerFeedback::Pending(SubmissionKind::Prompt), cx)
                    });
                }
            }
            ComposerEvent::Abort | ComposerEvent::AbortBash => {
                self.close_compaction_modal(window, cx)
            }
            ComposerEvent::FollowUp { .. }
            | ComposerEvent::CommandNext
            | ComposerEvent::CommandPrevious
            | ComposerEvent::CommandAccept
            | ComposerEvent::CommandDismiss
            | ComposerEvent::PreviewImage(_) => {}
        }
    }

    fn open_session_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .render_projections
            .catalog
            .current_session_file
            .is_none()
        {
            return;
        }
        self.session_menu_open = false;
        if !self.session_rename_open {
            let current_name = self
                .render_projections
                .catalog
                .current_session_name
                .clone()
                .unwrap_or_default();
            self.session_name_composer
                .update(cx, |composer, cx| composer.set_draft(&current_name, cx));
            self.session_rename_open = true;
        }
        window.focus(&self.session_name_composer.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn toggle_session_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session_rename_open {
            self.session_rename_open = false;
            window.focus(&self.focus_handle);
            cx.notify();
        } else {
            self.open_session_rename(window, cx);
        }
    }

    fn toggle_session_menu(&mut self, cx: &mut Context<Self>) {
        self.session_rename_open = false;
        self.session_menu_open = !self.session_menu_open;
        cx.notify();
    }

    fn export_session_from_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session_menu_open = false;
        let _ = self.execute_native_action(NativeAction::ExportHtml, "", window, cx);
        cx.notify();
    }

    fn on_session_name_event(
        &mut self,
        event: &ComposerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComposerEvent::Accept { text, .. } => {
                let name = text.trim().to_owned();
                if name.is_empty() || self.pending_session_name.is_some() {
                    return;
                }
                let accepted = self.controller.update(cx, |controller, cx| {
                    controller.set_session_name(name.clone(), cx)
                });
                if accepted {
                    self.pending_session_name = Some(name);
                    self.session_rename_open = false;
                    self.session_name_composer.update(cx, |composer, cx| {
                        composer.set_feedback(ComposerFeedback::Pending(SubmissionKind::Prompt), cx)
                    });
                    window.focus(&self.focus_handle);
                    cx.notify();
                }
            }
            ComposerEvent::Abort | ComposerEvent::AbortBash => {
                self.session_rename_open = false;
                window.focus(&self.focus_handle);
                cx.notify();
            }
            ComposerEvent::FollowUp { .. }
            | ComposerEvent::CommandNext
            | ComposerEvent::CommandPrevious
            | ComposerEvent::CommandAccept
            | ComposerEvent::CommandDismiss
            | ComposerEvent::PreviewImage(_) => {}
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
            .render_projections
            .models
            .auth
            .as_ref()
            .map(|flow| &flow.stage)
        else {
            return;
        };
        let prompt = prompt.clone();
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
            ModelPanel::Settings(ModelSettingsTab::Typography) => {
                window.focus(&self.font_search_composer.read(cx).focus_handle(cx));
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

    fn set_model_settings_tab(
        &mut self,
        tab: ModelSettingsTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model_panel = Some(ModelPanel::Settings(tab));
        if tab == ModelSettingsTab::Models {
            window.focus(&self.model_search_composer.read(cx).focus_handle(cx));
        } else if tab == ModelSettingsTab::Typography {
            window.focus(&self.font_search_composer.read(cx).focus_handle(cx));
        }
        cx.notify();
    }

    fn set_font_role(&mut self, role: FontRole, cx: &mut Context<Self>) {
        self.font_role = role;
        cx.notify();
    }

    fn select_font(&mut self, role: FontRole, family: String, cx: &mut Context<Self>) {
        if !self
            .font_catalog
            .families
            .iter()
            .any(|available| available == &family)
            || self.font_catalog.preferences.family(role) == family
        {
            return;
        }

        self.font_catalog.preferences.set(role, family);
        fonts::install(self.font_catalog.preferences.clone());
        self.font_feedback = Some("Saving typography…".to_owned());
        self.font_save_generation = self.font_save_generation.wrapping_add(1);
        let generation = self.font_save_generation;
        let path = self.font_catalog.settings_path.clone();
        let preferences = self.font_catalog.preferences.clone();
        let save = cx
            .background_executor()
            .spawn(async move { fonts::save(&path, &preferences) });
        cx.spawn(async move |view, cx| {
            let result = save.await;
            let _ = view.update(cx, |view, cx| {
                if view.font_save_generation != generation {
                    return;
                }
                view.font_feedback = Some(match result {
                    Ok(()) => "Typography saved".to_owned(),
                    Err(error) => format!("Typography could not be saved: {error}"),
                });
                cx.notify();
            });
        })
        .detach();
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
            .render_projections
            .models
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.defaults.thinking);
        self.controller.update(cx, |controller, cx| {
            controller.set_model_defaults(Some(identity), thinking, cx);
        });
    }

    fn set_default_thinking(&mut self, thinking: ThinkingLevel, cx: &mut Context<Self>) {
        let model = self
            .render_projections
            .models
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.defaults.model.clone());
        self.controller.update(cx, |controller, cx| {
            controller.set_model_defaults(model, Some(thinking), cx);
        });
    }

    fn toggle_model_scope(&mut self, identity: ModelIdentity, cx: &mut Context<Self>) {
        let Some(catalog) = self.render_projections.models.catalog.as_ref() else {
            return;
        };
        let mut scope = catalog.defaults.scoped_models.clone();
        if let Some(index) = scope.iter().position(|model| model == &identity) {
            scope.remove(index);
        } else {
            scope.push(identity);
        }
        self.controller.update(cx, |controller, cx| {
            controller.set_model_scope(scope, cx);
        });
    }

    fn sync_history_selection(&mut self) {
        let tree = Arc::clone(&self.render_projections.history.tree);
        let leaf_id = self.render_projections.history.leaf_id.clone();
        self.history.synchronize(&tree, leaf_id.as_ref());
    }

    fn select_history(
        &mut self,
        entry: crate::services::rpc::EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tree = Arc::clone(&self.render_projections.history.tree);
        if self.history.select(entry, &tree) {
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
        let tree = Arc::clone(&self.render_projections.history.tree);
        let leaf_id = self.render_projections.history.leaf_id.clone();
        let rows = self.history.rows(&tree, leaf_id.as_ref());
        if self.history.move_next(&rows) {
            cx.notify();
        }
    }

    fn on_history_previous(&mut self, _: &HistoryPrevious, _: &mut Window, cx: &mut Context<Self>) {
        let tree = Arc::clone(&self.render_projections.history.tree);
        let leaf_id = self.render_projections.history.leaf_id.clone();
        let rows = self.history.rows(&tree, leaf_id.as_ref());
        if self.history.move_previous(&rows) {
            cx.notify();
        }
    }

    fn on_history_first(&mut self, _: &HistoryFirst, _: &mut Window, cx: &mut Context<Self>) {
        let tree = Arc::clone(&self.render_projections.history.tree);
        let leaf_id = self.render_projections.history.leaf_id.clone();
        let rows = self.history.rows(&tree, leaf_id.as_ref());
        if self.history.move_first(&rows) {
            cx.notify();
        }
    }

    fn on_history_last(&mut self, _: &HistoryLast, _: &mut Window, cx: &mut Context<Self>) {
        let tree = Arc::clone(&self.render_projections.history.tree);
        let leaf_id = self.render_projections.history.leaf_id.clone();
        let rows = self.history.rows(&tree, leaf_id.as_ref());
        if self.history.move_last(&rows) {
            cx.notify();
        }
    }

    fn on_history_fold(&mut self, _: &HistoryFold, _: &mut Window, cx: &mut Context<Self>) {
        let tree = Arc::clone(&self.render_projections.history.tree);
        if self.history.fold_or_parent(&tree) {
            cx.notify();
        }
    }

    fn on_history_unfold(&mut self, _: &HistoryUnfold, _: &mut Window, cx: &mut Context<Self>) {
        let tree = Arc::clone(&self.render_projections.history.tree);
        if self.history.unfold_or_child(&tree) {
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

    fn on_sessions_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.delta.precise() {
            self.sessions_scroll_motion.cancel();
            return false;
        }

        let distance = event.delta.pixel_delta(px(20.0)).y;
        if distance == px(0.0) {
            return false;
        }

        let now = Instant::now();
        if self.sessions_scroll_motion.push(distance, now) {
            self.advance_sessions_scroll(now, cx);
            self.schedule_sessions_scroll_frame(window, cx);
        }
        true
    }

    fn advance_sessions_scroll(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(step) = self.sessions_scroll_motion.advance(now) else {
            return;
        };

        let before = self.sessions_scroll.offset();
        let max_offset = self.sessions_scroll.max_offset().height;
        let next_y = (before.y + step).clamp(-max_offset, Pixels::ZERO);
        self.sessions_scroll.set_offset(point(before.x, next_y));
        if (f32::from(next_y) - f32::from(before.y)).abs() < 0.01 {
            self.sessions_scroll_motion.cancel();
        }
        cx.notify();
    }

    fn schedule_sessions_scroll_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sessions_scroll_motion.schedule_frame() {
            return;
        }
        cx.on_next_frame(window, |view, window, cx| {
            view.sessions_scroll_motion.begin_frame();
            view.advance_sessions_scroll(Instant::now(), cx);
            view.schedule_sessions_scroll_frame(window, cx);
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
        let (
            conversation,
            extension_ui,
            render_projections,
            command_catalog_source,
            composer_projection,
        ) = {
            let controller = self.controller.read(cx);
            (
                controller.conversation_projection(),
                controller.extension_ui_projection(),
                RenderProjections::read(controller),
                controller.command_catalog_projection(),
                controller.composer_projection(),
            )
        };
        self.sync_extension_ui(extension_ui, window, cx);
        let epoch_changed = conversation.epoch != self.conversation.epoch;
        if epoch_changed {
            self.conversation_scroll_motion.cancel();
            self.conversation_follow.set(true);
        }
        self.transcript_cache
            .update(cx, |cache, _| cache.prepare_epoch(conversation.epoch));
        self.activity_disclosures.update(cx, |disclosures, _| {
            disclosures.prepare_epoch(conversation.epoch)
        });
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
        self.enforce_all_delivery_modes(cx);

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
        let catalog = &render_projections.catalog;
        let bridge = &render_projections.bridge;
        let rename_available =
            compact_available && catalog.current_session_file.is_some() && !catalog.switching;
        if !rename_available {
            self.session_rename_open = false;
        }
        if catalog.switching {
            self.session_menu_open = false;
        }
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
        let auth_prompt =
            render_projections
                .models
                .auth
                .as_ref()
                .and_then(|flow| match &flow.stage {
                    AuthStage::Prompt(prompt) => Some(prompt.clone()),
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
        self.reconcile_scoped_operations(catalog, window, cx);

        let projection = composer_projection;
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

        if self.history_open {
            let tree = Arc::clone(&render_projections.history.tree);
            let leaf_id = render_projections.history.leaf_id.clone();
            self.history.synchronize(&tree, leaf_id.as_ref());
        }
        let command_catalog_changed = command_catalog_source.status
            != self.command_catalog_source.status
            || !Arc::ptr_eq(
                &command_catalog_source.commands,
                &self.command_catalog_source.commands,
            );
        if command_catalog_changed {
            self.command_catalog = CommandCatalog::build(
                &command_catalog_source.status,
                &command_catalog_source.commands,
            );
            self.command_catalog_source = command_catalog_source;
        }
        self.render_projections = render_projections;
        if command_catalog_changed {
            self.refresh_command_palette_matches(cx);
            self.sync_slash_completion(cx);
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

    fn enforce_all_delivery_modes(&mut self, cx: &mut Context<Self>) {
        if self.conversation.pending_operation.is_some() {
            return;
        }
        if self.conversation.steering_mode == Some(QueueDeliveryMode::OneAtATime) {
            self.set_steering_mode(QueueDeliveryMode::All, cx);
        } else if self.conversation.follow_up_mode == Some(QueueDeliveryMode::OneAtATime) {
            self.set_follow_up_mode(QueueDeliveryMode::All, cx);
        }
    }

    fn reconcile_scoped_operations(
        &mut self,
        catalog: &CatalogProjection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            if completed {
                self.compaction_modal_open = false;
                window.focus(&self.focus_handle);
            } else {
                window.focus(&self.compaction_composer.read(cx).focus_handle(cx));
            }
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
        let action = self.render_projections.shell.action;
        if let Some(action) = action {
            self.activate_recovery(action, cx);
        }
    }

    fn on_focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        if self.pasted_image_preview.is_some() {
            window.focus(&self.focus_handle);
            return;
        }
        if self.extension_ui.active_dialog.is_some() {
            // Extension requests are modal: global focus traversal must not escape behind them.
            self.focus_active_extension_dialog(window, cx);
            return;
        }
        if self.compaction_modal_open {
            window.focus(&self.compaction_composer.read(cx).focus_handle(cx));
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
        if self.pasted_image_preview.is_some() {
            window.focus(&self.focus_handle);
            return;
        }
        if self.extension_ui.active_dialog.is_some() {
            self.focus_active_extension_dialog(window, cx);
            return;
        }
        if self.compaction_modal_open {
            window.focus(&self.compaction_composer.read(cx).focus_handle(cx));
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
        if self.pasted_image_preview.is_some() || self.extension_ui.active_dialog.is_some() {
            return;
        }
        if self.command_palette_open {
            self.close_command_palette(window, cx);
        } else {
            self.open_command_palette(window, cx);
        }
    }

    fn on_show_hotkeys(&mut self, _: &ShowHotkeys, window: &mut Window, cx: &mut Context<Self>) {
        if self.pasted_image_preview.is_some() || self.extension_ui.active_dialog.is_some() {
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
        self.refresh_command_palette_matches(cx);
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
                let Some(entry) = self
                    .command_palette_matches
                    .get(self.command_selection)
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
            | ComposerEvent::AbortBash
            | ComposerEvent::PreviewImage(_) => {}
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
                    .render_projections
                    .orchestration
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
            | ComposerEvent::CommandDismiss
            | ComposerEvent::PreviewImage(_) => {}
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
            .render_projections
            .orchestration
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        ExtensionDialogKey,
        model_panels::{model_choices, thinking_choices},
        overlays::{extension_dialog_key, wrapped_index},
        shared::short_path,
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
            super::inspector::context_pct("12,345 / 200,000"),
            Some(12345.0 / 200_000.0)
        );
        assert_eq!(super::inspector::context_pct("42%"), Some(0.42));
        assert_eq!(super::inspector::context_pct("Awaiting"), None);
        assert_eq!(
            super::inspector::context_pct("1,000 / 2,000 · stale"),
            Some(0.5)
        );
    }

    #[test]
    fn model_choices_fall_back_to_stock_rpc_models() {
        let projection = ModelRuntimeProjection {
            phase: CatalogPhase::Failed("SDK catalog unavailable".into()),
            catalog: None,
            stock_models: Arc::new(vec![ModelSummary {
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
            }]),
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
    fn pasted_image_navigation_wraps_in_both_directions() {
        assert_eq!(wrapped_index(0, 4, -1), 3);
        assert_eq!(wrapped_index(3, 4, 1), 0);
        assert_eq!(wrapped_index(1, 4, 1), 2);
        assert_eq!(wrapped_index(0, 0, 1), 0);
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
