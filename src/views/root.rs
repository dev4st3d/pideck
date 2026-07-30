//! Live Pi shell with an authoritative streaming conversation.

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui::{
    Bounds, ClipboardItem, Context, CursorStyle, DispatchPhase, Entity, ExternalPaths, FocusHandle,
    Focusable, FontWeight, HitboxBehavior, Image, ImageFormat, IntoElement, ListAlignment,
    ListOffset, ListState, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit,
    PathBuilder, PathPromptOptions, Pixels, Render, ScrollHandle, ScrollWheelEvent, StyledImage,
    Subscription, Task, Window, canvas, div, fill, img, list, point, prelude::*, px, size, svg,
};

use crate::actions::{
    APP_UPDATE_BUTTON_CONTEXT, APP_UPDATE_NOTICE_CONTEXT, AbortRun, ActivateAppUpdate,
    ActivateRecovery, AttachFiles, Connect, DecreaseFontSize, FocusNext, FocusPrevious,
    HistoryActivate, HistoryFirst, HistoryFold, HistoryLast, HistoryNext, HistoryPrevious,
    HistoryUnfold, ImagePreviewClose, ImagePreviewNext, ImagePreviewPrevious, IncreaseFontSize,
    ORCHESTRATION_ROW_CONTEXT, OpenAppUpdates, OpenCommandPalette, OrchestrationActivate, Retry,
    ShowHotkeys, Stop, ToggleInspector, ToggleSidebar, ToggleTerminal,
};
use crate::attachments::{self, PromptFile};
use crate::command_catalog::{
    CommandCatalog, CommandEntry, CommandTarget, InvocationResolution, NativeAction,
};
use crate::controller::{
    AcceptedSubmission, AcceptedSubmissionKind, BridgeProjection, CatalogProjection, CatalogStatus,
    CommandCatalogProjection, ComposerRuntime, ConversationProjection, ExtensionUiProjection,
    HistoryProjection, ModelRuntimeProjection, OrchestrationProjection, ResourceCenterProjection,
    RuntimeController, SubmissionPreference, ThreadRuntimeProjection,
};
use crate::file_completion::{self, AtToken, FileMatch};
use crate::fonts::{self, FontCatalog, FontRole};
use crate::model_runtime::{
    AuthFlow, AuthMethod, AuthPromptKind, AuthStage, CatalogPhase, ModelCatalogEntry,
    ModelChangePolicy, ModelIdentity, ThinkingLevel,
};
use crate::orchestration::{
    GoalItemSnapshot, OrchestrationAction, OrchestrationPhase, SubagentSnapshot, SubagentStatus,
    TaskSnapshot, TaskStatus, TranscriptRole,
};
use crate::resource_center::{
    ResourceLoadState, ResourcePhase, ResourceScopeFilter, ResourceStateFilter,
};
use crate::services::app_update::{self, CheckOutcome, InstallOutcome};
use crate::services::git_diff::{WorkspaceDiff, load_workspace_diff};
use crate::services::projects::{
    AddProjectOutcome, ProjectEntry, ProjectRegistry, ProjectRegistryError, project_key,
};
use crate::services::session_catalog::{
    SessionCatalogConfig, SessionSummary, scan_sessions, trash_session_file,
};
use crate::services::terminal::TerminalSize;
use crate::state::history::HistoryBrowser;
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
    ActivityDetail, ActivityDisclosureState, ConversationDiffSummary, ConversationListModel,
    ConversationScrollMotion, TranscriptTextCache, latest_completed_response_key,
};
use crate::views::terminal::{TerminalPanelEvent, TerminalView};

mod composer_bar;
mod inspector;
mod model_panels;
mod overlays;
mod render;
mod shared;
mod shell;

use overlays::{annotate_prompt_image, extension_dialog_key, single_line_title, wrapped_index};

#[derive(Clone)]
struct PendingDraft {
    request: crate::services::rpc::RequestId,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadActivity {
    Idle,
    Opening,
    Working,
    Cancelling,
    Attention,
}

#[derive(Debug, Clone)]
struct ThreadRuntimeStatus {
    project: String,
    active: bool,
    activity: ThreadActivity,
}

#[derive(Default)]
struct ThreadUiState {
    draft: String,
    images: Vec<PromptImage>,
    files: Vec<PromptFile>,
    pending_draft: Option<PendingDraft>,
    pending_bash: Option<crate::services::rpc::RequestId>,
    pending_compaction_focus: Option<String>,
    pending_session_name: Option<String>,
}

impl ThreadUiState {
    fn can_evict(&self) -> bool {
        self.draft.is_empty()
            && self.images.is_empty()
            && self.files.is_empty()
            && self.pending_draft.is_none()
            && self.pending_bash.is_none()
            && self.pending_compaction_focus.is_none()
            && self.pending_session_name.is_none()
    }
}

fn can_reuse_runtime_for_navigation(
    projection: &ThreadRuntimeProjection,
    ui: &ThreadUiState,
) -> bool {
    projection.status == crate::state::ControllerStatus::Active
        && matches!(
            projection.lifecycle,
            RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
        )
        && !projection.pending_operation
        && ui.can_evict()
}

struct ThreadRuntimeSlot {
    id: u64,
    project_path: PathBuf,
    requested_session: Option<PathBuf>,
    controller: Entity<RuntimeController>,
    projection: ThreadRuntimeProjection,
    last_activated: u64,
    ui: ThreadUiState,
}

const MAX_LIVE_THREAD_RUNTIMES: usize = 8;

#[derive(Debug, Clone)]
struct ProjectCatalogCache {
    status: CatalogStatus,
    sessions: Arc<Vec<SessionSummary>>,
    corrupt_count: usize,
    error: Option<String>,
}

impl ProjectCatalogCache {
    fn loading(previous: Option<&Self>) -> Self {
        Self {
            status: CatalogStatus::Loading,
            sessions: previous
                .map(|catalog| Arc::clone(&catalog.sessions))
                .unwrap_or_default(),
            corrupt_count: previous.map_or(0, |catalog| catalog.corrupt_count),
            error: None,
        }
    }
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
    Pi,
    Usage,
    Typography,
    Resources,
    App,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PiDeckUpdateState {
    Idle,
    Checking,
    Current,
    Available { version: String },
    Downloading { version: String },
    Restarting,
    NotInstalled,
    Error(String),
}

impl PiDeckUpdateState {
    fn available_version(&self) -> Option<&str> {
        match self {
            Self::Available { version } => Some(version),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtensionDialogKey {
    Cancel,
    ContainFocus,
    Move(isize),
    AcceptSelection,
    /// 1-based option shortcut (`1`–`9`) for select dialogs.
    SelectIndex(usize),
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
    active_theme: theme::ThemeId,
    theme_menu_open: bool,
    font_scale: theme::FontScale,
    composer: Entity<Composer>,
    terminal: Entity<TerminalView>,
    compaction_composer: Entity<Composer>,
    session_name_composer: Entity<Composer>,
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
    model_provider_filter: Option<String>,
    resource_scope_filter: ResourceScopeFilter,
    resource_state_filter: ResourceStateFilter,
    font_catalog: FontCatalog,
    font_role: FontRole,
    font_feedback: Option<String>,
    font_save_generation: u64,
    app_update: PiDeckUpdateState,
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
    file_completion_matches: Vec<FileMatch>,
    file_completion_token: Option<AtToken>,
    file_completion_selection: usize,
    file_completion_scroll: ScrollHandle,
    file_completion_generation: u64,
    file_completion_query: Option<String>,
    dismissed_file_token: Option<String>,
    command_palette_scroll: ScrollHandle,
    slash_command_scroll: ScrollHandle,
    model_switcher_scroll: ScrollHandle,
    model_provider_scroll: ScrollHandle,
    thinking_select_scroll: ScrollHandle,
    pi_settings_scroll: ScrollHandle,
    runtime_notifications: VecDeque<RuntimeNotification>,
    dismissed_slash_draft: Option<String>,
    last_slash_draft: String,
    active_auth_prompt_id: Option<String>,
    provider_auth_focus: FocusHandle,
    last_auth_browser_launch: Option<(u64, String)>,
    auth_browser_feedback: Option<(String, controls::ControlTone)>,
    model_catalog_auto_refresh_pending: bool,
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
    projects: ProjectRegistry,
    runtime_slots: Vec<ThreadRuntimeSlot>,
    runtime_observations: HashMap<u64, Subscription>,
    active_runtime_id: u64,
    next_runtime_id: u64,
    runtime_clock: u64,
    project_catalogs: HashMap<String, ProjectCatalogCache>,
    /// Sidebar thread row under the pointer; drives hover-only delete chrome.
    hovered_thread_key: Option<String>,
    project_feedback: Option<String>,
    project_picker_pending: bool,
    attachment_picker_pending: bool,
    attachment_task: Option<Task<()>>,
    project_scan_generation: u64,
    project_scan_task: Option<Task<()>>,
    project_save_generation: u64,
    project_save_task: Option<Task<()>>,
    sessions_scroll: ScrollHandle,
    sessions_scroll_motion: ConversationScrollMotion,
    subagent_dialog_focus: FocusHandle,
    subagent_dialog_scroll: ScrollHandle,
    window_title: String,
    history: HistoryBrowser,
    history_focus: FocusHandle,
    history_open: bool,
    /// Workspace project sidebar visibility (animated open/close).
    sidebar_open: bool,
    /// Bumps on each toggle so width animation only runs after user action.
    sidebar_motion_key: u64,
    /// Bottom workspace terminal visibility.
    terminal_open: bool,
    /// User-adjusted terminal panel height in logical pixels.
    terminal_height: f32,
    /// Pointer Y and height captured when a splitter drag begins.
    terminal_drag_origin: Option<(Pixels, f32)>,
    /// Right-hand Inspector panel visibility (animated open/close).
    inspector_open: bool,
    /// Bumps on each toggle so width animation only runs after user action.
    inspector_motion_key: u64,
    session_rename_open: bool,
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
    activity_detail: Option<ActivityDetail>,
    activity_detail_focus: FocusHandle,
    activity_detail_restore_focus: Option<FocusHandle>,
    activity_detail_scroll: ScrollHandle,
    workspace_diff: Option<Arc<WorkspaceDiff>>,
    workspace_diff_identity: Option<(u64, String)>,
    workspace_diff_generation: u64,
    workspace_diff_files_expanded: bool,
    workspace_diff_open: bool,
    workspace_diff_selected: usize,
    workspace_diff_collapsed_folders: HashSet<String>,
    workspace_diff_files_scroll: ScrollHandle,
    workspace_diff_scroll: ScrollHandle,
    workspace_diff_focus: FocusHandle,
    pending_draft: Option<PendingDraft>,
    pending_bash: Option<crate::services::rpc::RequestId>,
    pending_compaction_focus: Option<String>,
    pending_session_name: Option<String>,
    focus_handle: FocusHandle,
    _activity_disclosure_observation: Subscription,
    _composer_subscription: Subscription,
    _terminal_subscription: Subscription,
    _compaction_subscription: Subscription,
    _session_name_subscription: Subscription,
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
        projects: ProjectRegistry,
        project_feedback: Option<String>,
        projects_need_save: bool,
        mut font_catalog: FontCatalog,
        cx: &mut Context<Self>,
    ) -> Self {
        let active_theme = font_catalog
            .theme_key
            .as_deref()
            .and_then(theme::ThemeId::from_key)
            .unwrap_or_else(|| {
                font_catalog.theme_key = None;
                theme::ThemeId::PiDeckDark
            });
        theme::set_active(active_theme);
        let font_scale = theme::FontScale::default();
        window.set_rem_size(font_scale.rem_size());
        let focus_handle = cx.focus_handle();
        let composer = cx.new(Composer::new);
        let terminal_workspace = projects.active_path().to_path_buf();
        let terminal = cx.new(|cx| TerminalView::new(terminal_workspace, cx));
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
        let history_label_composer =
            cx.new(|cx| Composer::field("history-label", "Label active tip…", "Set", cx));
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
        // Empty submit is valid: ask_user_question multi-select treats "" as no
        // selection (same as TUI Next with nothing toggled).
        let extension_input_composer = cx.new(|cx| {
            Composer::field("extension-dialog-input", "Enter a value...", "Submit", cx)
                .allowing_empty_submit()
        });
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
        let provider_auth_focus = cx.focus_handle();
        let extension_dialog_focus = cx.focus_handle();
        let subagent_dialog_focus = cx.focus_handle();
        let activity_detail_focus = cx.focus_handle();
        let workspace_diff_focus = cx.focus_handle();
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
        let initial_runtime_id = 1;
        let controller_observation =
            cx.observe_in(&controller, window, move |view, _, window, cx| {
                view.on_thread_runtime_changed(initial_runtime_id, window, cx)
            });
        let activity_disclosure_observation =
            cx.observe(&activity_disclosures, |_, _, cx| cx.notify());
        let composer_subscription =
            cx.subscribe_in(&composer, window, |view, _, event, window, cx| {
                view.on_composer_event(event, window, cx)
            });
        let terminal_subscription =
            cx.subscribe_in(&terminal, window, |view, _, event, window, cx| {
                view.on_terminal_panel_event(event, window, cx)
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
        let history_label_subscription =
            cx.subscribe_in(&history_label_composer, window, |view, _, event, _, cx| {
                view.on_history_label_event(event, cx)
            });
        let import_path_subscription =
            cx.subscribe_in(&import_path_composer, window, |view, _, event, _, cx| {
                view.on_import_path_event(event, cx)
            });
        let model_search_observation =
            cx.observe_in(&model_search_composer, window, |view, _, _, cx| {
                view.model_switcher_scroll.scroll_to_item(0);
                cx.notify();
            });
        let font_search_observation =
            cx.observe_in(&font_search_composer, window, |_, _, _, cx| cx.notify());
        let composer_observation = cx.observe_in(&composer, window, |view, _, _, cx| {
            view.sync_composer_completions(cx)
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
        let auth_input_subscription = cx.subscribe_in(
            &auth_input_composer,
            window,
            |view, _, event, window, cx| view.on_auth_input_event(event, false, window, cx),
        );
        let auth_secret_subscription = cx.subscribe_in(
            &auth_secret_composer,
            window,
            |view, _, event, window, cx| view.on_auth_input_event(event, true, window, cx),
        );
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
        window.set_window_title("πdeck");
        let initial_project_path = projects.active_path().to_path_buf();
        let initial_projection = controller.read(cx).thread_runtime_projection();
        let mut runtime_observations = HashMap::new();
        runtime_observations.insert(initial_runtime_id, controller_observation);
        let mut view = Self {
            controller: controller.clone(),
            render_projections,
            active_theme,
            theme_menu_open: false,
            font_scale,
            composer,
            terminal,
            compaction_composer,
            session_name_composer,
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
            model_provider_filter: None,
            resource_scope_filter: ResourceScopeFilter::All,
            resource_state_filter: ResourceStateFilter::All,
            font_feedback: font_catalog.load_warning.clone(),
            font_catalog,
            font_role: FontRole::Sans,
            font_save_generation: 0,
            app_update: PiDeckUpdateState::Idle,
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
            file_completion_matches: Vec::new(),
            file_completion_token: None,
            file_completion_selection: 0,
            file_completion_scroll: ScrollHandle::new(),
            file_completion_generation: 0,
            file_completion_query: None,
            dismissed_file_token: None,
            command_palette_scroll: ScrollHandle::new(),
            slash_command_scroll: ScrollHandle::new(),
            model_switcher_scroll: ScrollHandle::new(),
            model_provider_scroll: ScrollHandle::new(),
            thinking_select_scroll: ScrollHandle::new(),
            pi_settings_scroll: ScrollHandle::new(),
            runtime_notifications: VecDeque::new(),
            dismissed_slash_draft: None,
            last_slash_draft: String::new(),
            active_auth_prompt_id: None,
            provider_auth_focus,
            last_auth_browser_launch: None,
            auth_browser_feedback: None,
            model_catalog_auto_refresh_pending: false,
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
            projects,
            runtime_slots: vec![ThreadRuntimeSlot {
                id: initial_runtime_id,
                project_path: initial_project_path,
                requested_session: None,
                controller: controller.clone(),
                projection: initial_projection,
                last_activated: 1,
                ui: ThreadUiState::default(),
            }],
            runtime_observations,
            active_runtime_id: initial_runtime_id,
            next_runtime_id: initial_runtime_id + 1,
            runtime_clock: 1,
            project_catalogs: HashMap::new(),
            hovered_thread_key: None,
            project_feedback,
            project_picker_pending: false,
            attachment_picker_pending: false,
            attachment_task: None,
            project_scan_generation: 0,
            project_scan_task: None,
            project_save_generation: 0,
            project_save_task: None,
            sessions_scroll: ScrollHandle::new(),
            sessions_scroll_motion: ConversationScrollMotion::default(),
            subagent_dialog_focus,
            subagent_dialog_scroll: ScrollHandle::new(),
            window_title: "πdeck".to_owned(),
            history: HistoryBrowser::default(),
            history_focus,
            history_open: false,
            sidebar_open: true,
            sidebar_motion_key: 0,
            terminal_open: false,
            terminal_height: 260.0,
            terminal_drag_origin: None,
            inspector_open: true,
            inspector_motion_key: 0,
            session_rename_open: false,
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
            activity_detail: None,
            activity_detail_focus,
            activity_detail_restore_focus: None,
            activity_detail_scroll: ScrollHandle::new(),
            workspace_diff: None,
            workspace_diff_identity: None,
            workspace_diff_generation: 0,
            workspace_diff_files_expanded: false,
            workspace_diff_open: false,
            workspace_diff_selected: 0,
            workspace_diff_collapsed_folders: HashSet::new(),
            workspace_diff_files_scroll: ScrollHandle::new(),
            workspace_diff_scroll: ScrollHandle::new(),
            workspace_diff_focus,
            pending_draft: None,
            pending_bash: None,
            pending_compaction_focus: None,
            pending_session_name: None,
            focus_handle,
            _activity_disclosure_observation: activity_disclosure_observation,
            _composer_subscription: composer_subscription,
            _terminal_subscription: terminal_subscription,
            _compaction_subscription: compaction_subscription,
            _session_name_subscription: session_name_subscription,
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
        };
        view.refresh_project_catalogs(cx);
        if projects_need_save {
            view.persist_projects(cx);
        }
        view.check_for_app_update(cx);
        view
    }

    fn toggle_theme_menu(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.theme_menu_open = !self.theme_menu_open;
        cx.notify();
    }

    fn toggle_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        self.sidebar_motion_key = self.sidebar_motion_key.wrapping_add(1);
        if !self.sidebar_open {
            // History sits beside the workspace list; hide it when the rail closes.
            self.history_open = false;
            self.history_confirmation = None;
        }
        cx.notify();
    }

    fn on_toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_sidebar(window, cx);
    }

    fn ensure_sidebar_open(&mut self) {
        if self.sidebar_open {
            return;
        }
        self.sidebar_open = true;
        self.sidebar_motion_key = self.sidebar_motion_key.wrapping_add(1);
    }

    fn set_terminal_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_open == open {
            return;
        }
        self.terminal_open = open;
        self.terminal_drag_origin = None;
        if open {
            self.terminal
                .update(cx, |terminal, cx| terminal.activate(cx));
            window.focus(&self.terminal.read(cx).focus_handle(cx));
        } else {
            window.focus(&self.composer.read(cx).focus_handle(cx));
        }
        cx.notify();
    }

    fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_terminal_open(!self.terminal_open, window, cx);
    }

    fn on_toggle_terminal(
        &mut self,
        _: &ToggleTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_terminal(window, cx);
    }

    fn on_terminal_panel_event(
        &mut self,
        event: &TerminalPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalPanelEvent::CloseRequested => self.set_terminal_open(false, window, cx),
        }
    }

    fn begin_terminal_resize(&mut self, pointer_y: Pixels, cx: &mut Context<Self>) {
        self.terminal_drag_origin = Some((pointer_y, self.terminal_height));
        cx.notify();
    }

    fn update_terminal_resize(
        &mut self,
        pointer_y: Pixels,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((start_y, start_height)) = self.terminal_drag_origin else {
            return false;
        };
        let delta = f32::from(start_y - pointer_y);
        let max_height = (f32::from(viewport_height) - theme::TITLE_H - 210.0).max(180.0);
        let next = (start_height + delta).clamp(180.0, max_height);
        if (next - self.terminal_height).abs() >= 0.5 {
            self.terminal_height = next;
            cx.notify();
        }
        true
    }

    fn end_terminal_resize(&mut self, cx: &mut Context<Self>) -> bool {
        if self.terminal_drag_origin.take().is_none() {
            return false;
        }
        cx.notify();
        true
    }

    fn terminal_size(&self, window: &Window) -> TerminalSize {
        let mut width = f32::from(window.viewport_size().width);
        if self.sidebar_open {
            width -= theme::SIDE_W;
        }
        if self.history_open {
            width -= theme::HISTORY_W;
        }
        if self.inspector_open {
            width -= theme::INSPECT_W;
        }
        let rows = ((self.terminal_height - 50.0) / 18.0).floor().max(4.0) as u16;
        let cols = ((width - 24.0) / 7.4).floor().max(24.0) as u16;
        TerminalSize::new(rows, cols)
    }

    fn toggle_inspector(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.inspector_open = !self.inspector_open;
        self.inspector_motion_key = self.inspector_motion_key.wrapping_add(1);
        cx.notify();
    }

    fn on_toggle_inspector(
        &mut self,
        _: &ToggleInspector,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inspector(window, cx);
    }

    fn close_theme_menu(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.theme_menu_open {
            return;
        }
        self.theme_menu_open = false;
        cx.notify();
    }

    fn set_theme(&mut self, theme_id: theme::ThemeId, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_menu_open = false;
        if self.active_theme == theme_id {
            cx.notify();
            return;
        }
        self.active_theme = theme_id;
        theme::set_active(self.active_theme);
        self.font_catalog.theme_key = Some(self.active_theme.key().to_owned());
        self.persist_settings(None, None, cx);
        window.refresh();
        cx.notify();
    }

    fn adjust_font_scale(&mut self, increase: bool, window: &mut Window, cx: &mut Context<Self>) {
        let changed = if increase {
            self.font_scale.increase()
        } else {
            self.font_scale.decrease()
        };
        if !changed {
            return;
        }
        window.set_rem_size(self.font_scale.rem_size());
        window.refresh();
        cx.notify();
    }

    fn on_increase_font_size(
        &mut self,
        _: &IncreaseFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.adjust_font_scale(true, window, cx);
    }

    fn on_decrease_font_size(
        &mut self,
        _: &DecreaseFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.adjust_font_scale(false, window, cx);
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

    fn on_attach_files(&mut self, _: &AttachFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.choose_attachments(cx);
    }

    fn on_retry(&mut self, _: &Retry, _: &mut Window, cx: &mut Context<Self>) {
        self.connect(cx);
    }

    fn on_stop(&mut self, _: &Stop, _: &mut Window, cx: &mut Context<Self>) {
        self.stop(cx);
    }

    fn on_abort_run(&mut self, _: &AbortRun, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity_detail.is_some() {
            self.close_activity_detail(window, cx);
            return;
        }
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
            ComposerEvent::Accept {
                text,
                images,
                files,
            } => {
                self.pasted_image_preview = None;
                self.execute_composer_text(
                    text.clone(),
                    images.clone(),
                    files.clone(),
                    SubmissionPreference::Default,
                    window,
                    cx,
                )
            }
            ComposerEvent::FollowUp {
                text,
                images,
                files,
            } => {
                self.pasted_image_preview = None;
                self.execute_composer_text(
                    text.clone(),
                    images.clone(),
                    files.clone(),
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
            ComposerEvent::CommandNext => {
                if self.file_completion_active() {
                    self.move_file_completion_selection(1, cx);
                } else {
                    self.move_command_selection(1, false, cx);
                }
            }
            ComposerEvent::CommandPrevious => {
                if self.file_completion_active() {
                    self.move_file_completion_selection(-1, cx);
                } else {
                    self.move_command_selection(-1, false, cx);
                }
            }
            ComposerEvent::CommandAccept => {
                if self.file_completion_active() {
                    self.accept_file_completion(window, cx);
                } else {
                    self.accept_slash_completion(window, cx);
                }
            }
            ComposerEvent::CommandDismiss => {
                if self.file_completion_active() {
                    self.dismiss_file_completion(cx);
                } else {
                    self.dismissed_slash_draft = Some(self.composer.read(cx).draft().to_owned());
                    self.composer.update(cx, |composer, cx| {
                        composer.set_command_completion_active(false, cx)
                    });
                    cx.notify();
                }
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

    fn sync_composer_completions(&mut self, cx: &mut Context<Self>) {
        self.sync_slash_completion(cx);
        let slash_owns = {
            let composer = self.composer.read(cx);
            let draft = composer.draft();
            !composer.has_attachments()
                && self.slash_intercepts_enter
                && self.dismissed_slash_draft.as_deref() != Some(draft)
        };
        if slash_owns {
            self.clear_file_completion();
            self.apply_completion_keyboard_routing(cx);
            return;
        }
        self.sync_file_completion(cx);
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

        let slash_active = !self.composer.read(cx).has_attachments()
            && self.dismissed_slash_draft.as_deref() != Some(draft.as_str())
            && self.slash_intercepts_enter;
        if slash_active {
            self.command_selection = self
                .command_selection
                .min(self.slash_command_matches.len().saturating_sub(1));
        } else if !self.slash_intercepts_enter {
            self.command_selection = 0;
        }
        self.apply_completion_keyboard_routing(cx);
    }

    fn file_completion_active(&self) -> bool {
        self.file_completion_token.is_some() && !self.file_completion_matches.is_empty()
    }

    fn slash_completion_active(&self, cx: &Context<Self>) -> bool {
        let composer = self.composer.read(cx);
        !composer.has_attachments()
            && self.slash_intercepts_enter
            && self.dismissed_slash_draft.as_deref() != Some(composer.draft())
    }

    fn apply_completion_keyboard_routing(&mut self, cx: &mut Context<Self>) {
        let active = self.slash_completion_active(cx) || self.file_completion_active();
        self.composer.update(cx, |composer, cx| {
            composer.set_command_completion_active(active, cx)
        });
        cx.notify();
    }

    fn clear_file_completion(&mut self) {
        self.file_completion_generation = self.file_completion_generation.wrapping_add(1);
        self.file_completion_matches.clear();
        self.file_completion_token = None;
        self.file_completion_selection = 0;
        self.file_completion_query = None;
    }

    fn sync_file_completion(&mut self, cx: &mut Context<Self>) {
        if self.composer.read(cx).has_attachments() {
            self.clear_file_completion();
            self.apply_completion_keyboard_routing(cx);
            return;
        }

        let (draft, cursor) = {
            let composer = self.composer.read(cx);
            (composer.draft().to_owned(), composer.cursor())
        };
        let Some(token) = file_completion::extract_at_token(&draft, cursor) else {
            self.dismissed_file_token = None;
            self.clear_file_completion();
            self.apply_completion_keyboard_routing(cx);
            return;
        };

        let token_key = format!("{}:{}", token.range.start, token.raw_query);
        if self.dismissed_file_token.as_deref() == Some(token_key.as_str()) {
            self.clear_file_completion();
            self.apply_completion_keyboard_routing(cx);
            return;
        }
        if self
            .dismissed_file_token
            .as_ref()
            .is_some_and(|dismissed| dismissed != &token_key)
        {
            self.dismissed_file_token = None;
        }

        let query = token.raw_query.clone();
        self.file_completion_token = Some(token);

        // Same query: keep current results / in-flight search.
        if self.file_completion_query.as_deref() == Some(query.as_str()) {
            if !self.file_completion_matches.is_empty() {
                self.file_completion_selection = self
                    .file_completion_selection
                    .min(self.file_completion_matches.len().saturating_sub(1));
            }
            self.apply_completion_keyboard_routing(cx);
            return;
        }

        self.file_completion_selection = 0;
        self.file_completion_scroll.scroll_to_item(0);
        // Keep previous rows until the new search lands so the menu does not flash empty.
        self.file_completion_query = Some(query.clone());
        let generation = self.file_completion_generation.wrapping_add(1);
        self.file_completion_generation = generation;

        let workspace = PathBuf::from(&self.render_projections.shell.workspace);
        // Debounce keystrokes; generation cancels superseded work.
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(45))
                .await;
            let still_current = view
                .update(cx, |view, _| view.file_completion_generation == generation)
                .unwrap_or(false);
            if !still_current {
                return;
            }
            let search_query = query.clone();
            let matches = cx
                .background_executor()
                .spawn(async move { file_completion::search_files(&workspace, &search_query, 12) })
                .await;
            let _ = view.update(cx, |view, cx| {
                if view.file_completion_generation != generation {
                    return;
                }
                if view.file_completion_query.as_deref() != Some(query.as_str()) {
                    return;
                }
                view.file_completion_matches = matches;
                view.file_completion_selection = 0;
                view.apply_completion_keyboard_routing(cx);
            });
        })
        .detach();
        self.apply_completion_keyboard_routing(cx);
    }

    fn move_file_completion_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.file_completion_matches.len();
        if count == 0 {
            self.file_completion_selection = 0;
            return;
        }
        self.file_completion_selection =
            (self.file_completion_selection as isize + delta).rem_euclid(count as isize) as usize;
        self.file_completion_scroll
            .scroll_to_item(self.file_completion_selection);
        cx.notify();
    }

    fn accept_file_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(token) = self.file_completion_token.clone() else {
            return;
        };
        let Some(item) = self
            .file_completion_matches
            .get(self.file_completion_selection)
            .cloned()
        else {
            return;
        };
        let draft = self.composer.read(cx).draft().to_owned();
        let (next, cursor) = file_completion::apply_file_match(&draft, &token, &item);
        self.dismissed_file_token = None;
        self.clear_file_completion();
        self.composer.update(cx, |composer, cx| {
            composer.set_draft_with_cursor(&next, cursor, cx);
        });
        // Directory accept re-opens listing for the extended `@path/`.
        self.sync_composer_completions(cx);
        window.focus(&self.composer.read(cx).focus_handle(cx));
    }

    fn dismiss_file_completion(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.file_completion_token.as_ref() {
            self.dismissed_file_token = Some(format!("{}:{}", token.range.start, token.raw_query));
        }
        self.clear_file_completion();
        self.apply_completion_keyboard_routing(cx);
    }

    fn choose_file_match(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.file_completion_matches.len() {
            return;
        }
        self.file_completion_selection = index;
        self.accept_file_completion(window, cx);
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
        files: Vec<PromptFile>,
        preference: SubmissionPreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !images.is_empty() || !files.is_empty() {
            self.submit(text, images, files, preference, cx);
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
            Ok(None) => self.submit(text, images, files, preference, cx),
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
        files: Vec<PromptFile>,
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
            controller.submit_with_attachments(
                text.clone(),
                images.clone(),
                files.clone(),
                preference,
                cx,
            )
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
        self.accept_extension_dialog_at(self.extension_dialog_selection, window, cx);
    }

    fn accept_extension_dialog_at(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = match self
            .extension_ui
            .active_dialog
            .as_ref()
            .map(|dialog| &dialog.request)
        {
            Some(DialogRequest::Select { options, .. }) => {
                options.get(index).cloned().map(DialogAnswer::Value)
            }
            Some(DialogRequest::Confirm { .. }) if index < 2 => {
                Some(DialogAnswer::Confirmed(index == 1))
            }
            _ => None,
        };
        if let Some(answer) = answer {
            self.extension_dialog_selection = index;
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
            Some(ExtensionDialogKey::SelectIndex(index)) => {
                cx.stop_propagation();
                self.accept_extension_dialog_at(index, window, cx);
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
            .unwrap_or_else(|| "πdeck".to_owned());
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

        self.activity_detail = None;
        self.activity_detail_restore_focus = None;
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
                self.open_thread(self.projects.active_path().to_path_buf(), None, window, cx)
                    .then_some(())
                    .ok_or_else(|| "A new session could not be opened.".to_owned())
            }
            NativeAction::Sessions => {
                self.open_session_rename(window, cx);
                Ok(())
            }
            NativeAction::Tree => {
                self.history_open = !self.history_open;
                self.history_confirmation = None;
                if self.history_open {
                    self.ensure_sidebar_open();
                    self.sync_history_selection();
                    window.focus(&self.history_focus);
                } else {
                    window.focus(&self.composer.read(cx).focus_handle(cx));
                }
                cx.notify();
                Ok(())
            }
            NativeAction::Fork => {
                self.ensure_sidebar_open();
                self.history_open = true;
                self.sync_history_selection();
                self.history_confirmation = None;
                window.focus(&self.history_focus);
                cx.notify();
                Ok(())
            }
            NativeAction::Clone => {
                self.ensure_sidebar_open();
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

    fn export_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn on_auth_input_event(
        &mut self,
        event: &ComposerEvent,
        secret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ComposerEvent::Accept { text, .. } = event else {
            if matches!(event, ComposerEvent::Abort | ComposerEvent::AbortBash) {
                self.cancel_provider_auth(cx);
                window.focus(&self.provider_auth_focus);
            }
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

    fn toggle_composer_enlarged(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |composer, cx| composer.toggle_input_enlarged(cx));
        // Keep the draft field focused so enlarge is height-only, not a focus detour.
        window.focus(&self.composer.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn show_model_panel(&mut self, panel: ModelPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.model_panel = Some(panel);
        if matches!(panel, ModelPanel::Switcher)
            || matches!(panel, ModelPanel::Settings(tab) if tab != ModelSettingsTab::App)
        {
            self.model_catalog_auto_refresh_pending = true;
            self.try_auto_refresh_models(cx);
        }
        match panel {
            ModelPanel::Switcher => {
                self.model_provider_filter = self
                    .render_projections
                    .models
                    .active_model
                    .as_ref()
                    .map(|identity| identity.provider.clone());
                self.model_switcher_scroll.scroll_to_item(0);
                self.model_search_composer
                    .update(cx, |search, cx| search.set_draft("", cx));
                window.focus(&self.model_search_composer.read(cx).focus_handle(cx));
            }
            ModelPanel::Settings(ModelSettingsTab::Models) => {
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

    fn open_app_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_model_panel(ModelPanel::Settings(ModelSettingsTab::App), window, cx);
    }

    fn on_open_app_updates(
        &mut self,
        _: &OpenAppUpdates,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_app_updates(window, cx);
    }

    fn check_for_app_update(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.app_update,
            PiDeckUpdateState::Checking
                | PiDeckUpdateState::Downloading { .. }
                | PiDeckUpdateState::Restarting
        ) {
            return;
        }

        self.app_update = PiDeckUpdateState::Checking;
        cx.notify();
        let check = cx
            .background_executor()
            .spawn(async { app_update::check_for_update() });
        cx.spawn(async move |view, cx| {
            let result = check.await;
            let _ = view.update(cx, |view, cx| {
                view.app_update = match result {
                    Ok(CheckOutcome::Current) => PiDeckUpdateState::Current,
                    Ok(CheckOutcome::UpdateAvailable { version }) => {
                        PiDeckUpdateState::Available { version }
                    }
                    Ok(CheckOutcome::NotInstalled) => PiDeckUpdateState::NotInstalled,
                    Err(error) => PiDeckUpdateState::Error(error.message().to_owned()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn activate_app_update(&mut self, cx: &mut Context<Self>) {
        let Some(version) = self.app_update.available_version().map(str::to_owned) else {
            if !matches!(
                self.app_update,
                PiDeckUpdateState::Checking
                    | PiDeckUpdateState::Downloading { .. }
                    | PiDeckUpdateState::Restarting
            ) {
                self.check_for_app_update(cx);
            }
            return;
        };

        self.app_update = PiDeckUpdateState::Downloading { version };
        cx.notify();
        let install = cx
            .background_executor()
            .spawn(async { app_update::download_and_schedule_update() });
        cx.spawn(async move |view, cx| {
            let result = install.await;
            let _ = view.update(cx, |view, cx| {
                view.app_update = match result {
                    Ok(InstallOutcome::RestartScheduled) => PiDeckUpdateState::Restarting,
                    Ok(InstallOutcome::AlreadyCurrent) => PiDeckUpdateState::Current,
                    Ok(InstallOutcome::NotInstalled) => PiDeckUpdateState::NotInstalled,
                    Err(error) => PiDeckUpdateState::Error(error.message().to_owned()),
                };
                let restart = matches!(view.app_update, PiDeckUpdateState::Restarting);
                cx.notify();
                if restart {
                    cx.quit();
                }
            });
        })
        .detach();
    }

    fn on_activate_app_update(
        &mut self,
        _: &ActivateAppUpdate,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_app_update(cx);
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
        self.persist_settings(Some("Saving typography…"), Some("Typography saved"), cx);
        cx.notify();
    }

    fn persist_settings(
        &mut self,
        progress: Option<&str>,
        success: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if let Some(message) = progress {
            self.font_feedback = Some(message.to_owned());
        }
        self.font_save_generation = self.font_save_generation.wrapping_add(1);
        let generation = self.font_save_generation;
        let path = self.font_catalog.settings_path.clone();
        let preferences = self.font_catalog.preferences.clone();
        let theme = self.font_catalog.theme_key.clone();
        let success = success.map(str::to_owned);
        let save = cx
            .background_executor()
            .spawn(async move { fonts::save(&path, &preferences, theme.as_deref()) });
        cx.spawn(async move |view, cx| {
            let result = save.await;
            let _ = view.update(cx, |view, cx| {
                if view.font_save_generation != generation {
                    return;
                }
                match result {
                    Ok(()) => {
                        if let Some(message) = success {
                            view.font_feedback = Some(message);
                            cx.notify();
                        }
                    }
                    Err(error) => {
                        view.font_feedback = Some(format!("Settings could not be saved: {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn set_model_provider_filter(&mut self, provider: Option<String>, cx: &mut Context<Self>) {
        if self.model_provider_filter == provider {
            return;
        }
        self.model_provider_filter = provider;
        self.model_switcher_scroll.scroll_to_item(0);
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

    fn try_auto_refresh_models(&mut self, cx: &mut Context<Self>) {
        if !self.model_catalog_auto_refresh_pending {
            return;
        }
        let satisfied = self.controller.update(cx, |controller, cx| {
            if matches!(
                controller.model_runtime_projection().phase,
                CatalogPhase::Refreshing
            ) {
                true
            } else {
                controller.refresh_model_catalog(cx)
            }
        });
        if satisfied {
            self.model_catalog_auto_refresh_pending = false;
        }
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

    fn login_provider(
        &mut self,
        provider: String,
        method: AuthMethod,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_auth_browser_launch = None;
        self.auth_browser_feedback = None;
        let accepted = self.controller.update(cx, |controller, cx| {
            controller.login_provider(provider, method, cx)
        });
        if accepted {
            window.focus(&self.provider_auth_focus);
        }
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

    fn open_provider_auth_url(&mut self, url: String, cx: &mut Context<Self>) {
        self.auth_browser_feedback = Some(
            match crate::services::path_actions::open_provider_auth_url(&url) {
                Ok(()) => (
                    "Opened your browser. Finish authentication there, then return here."
                        .to_owned(),
                    controls::ControlTone::Normal,
                ),
                Err(summary) => (
                    format!("Could not open the browser. {summary}"),
                    controls::ControlTone::Danger,
                ),
            },
        );
        cx.notify();
    }

    fn copy_provider_auth_code(&mut self, code: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(code));
        self.auth_browser_feedback = Some((
            "Device code copied. Paste it on the provider verification page.".to_owned(),
            controls::ControlTone::Normal,
        ));
        cx.notify();
    }

    pub(in crate::views) fn on_provider_auth_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            cx.stop_propagation();
            self.cancel_provider_auth(cx);
            return;
        }
        if key == "enter" {
            let auth = self.render_projections.models.auth.as_ref();
            let url = auth
                .and_then(AuthFlow::browser_target)
                .map(str::to_owned)
                .or_else(|| {
                    let operation = auth.map(|auth| auth.operation)?;
                    self.last_auth_browser_launch
                        .as_ref()
                        .filter(|(launched, _)| *launched == operation)
                        .map(|(_, url)| url.clone())
                });
            if let Some(url) = url {
                cx.stop_propagation();
                self.open_provider_auth_url(url, cx);
            }
            return;
        }
        if key.eq_ignore_ascii_case("c") {
            let code = self
                .render_projections
                .models
                .auth
                .as_ref()
                .and_then(|auth| match &auth.stage {
                    AuthStage::DeviceCode { user_code, .. } => Some(user_code.clone()),
                    _ => None,
                });
            if let Some(code) = code {
                cx.stop_propagation();
                self.copy_provider_auth_code(code, cx);
            }
            return;
        }
        let Some(index) = key
            .parse::<usize>()
            .ok()
            .filter(|index| (1..=9).contains(index))
            .map(|index| index - 1)
        else {
            return;
        };
        let selection = self
            .render_projections
            .models
            .auth
            .as_ref()
            .and_then(|auth| match &auth.stage {
                AuthStage::Prompt(prompt) if prompt.kind == AuthPromptKind::Select => prompt
                    .options
                    .get(index)
                    .map(|option| (prompt.clone(), option.id.clone())),
                _ => None,
            });
        if let Some((prompt, value)) = selection {
            cx.stop_propagation();
            self.answer_auth_select(prompt, value, cx);
        }
    }

    fn sync_provider_auth(
        &mut self,
        auth: Option<&AuthFlow>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(auth) = auth else {
            self.active_auth_prompt_id = None;
            self.last_auth_browser_launch = None;
            self.auth_browser_feedback = None;
            return;
        };

        let prompt = match &auth.stage {
            AuthStage::Prompt(prompt) => Some(prompt),
            _ => None,
        };
        let prompt_id = prompt.map(|prompt| prompt.prompt_id.clone());
        if prompt_id != self.active_auth_prompt_id {
            self.active_auth_prompt_id = prompt_id;
            self.auth_input_composer
                .update(cx, |composer, cx| composer.set_draft("", cx));
            self.auth_secret_composer
                .update(cx, |composer, cx| composer.set_draft("", cx));
            if let Some(prompt) = prompt
                && prompt.kind != AuthPromptKind::Select
            {
                let composer = if prompt.kind == AuthPromptKind::Secret {
                    &self.auth_secret_composer
                } else {
                    &self.auth_input_composer
                };
                window.focus(&composer.read(cx).focus_handle(cx));
            } else {
                window.focus(&self.provider_auth_focus);
            }
        }

        let Some(url) = auth.browser_target() else {
            return;
        };
        let launch = (auth.operation, url.to_owned());
        if self.last_auth_browser_launch.as_ref() == Some(&launch) {
            return;
        }
        self.last_auth_browser_launch = Some(launch);
        self.open_provider_auth_url(url.to_owned(), cx);
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

    fn set_pi_setting(
        &mut self,
        key: &'static str,
        value: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        self.controller.update(cx, |controller, cx| {
            controller.set_pi_setting(key.to_owned(), value, cx);
        });
    }

    fn sync_history_selection(&mut self) {
        let tree = Arc::clone(&self.render_projections.history.tree);
        let leaf_id = self.render_projections.history.leaf_id.clone();
        self.history.synchronize(&tree, leaf_id.as_ref());
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

    fn project_switch_enabled(&self) -> bool {
        true
    }

    fn persist_projects(&mut self, cx: &mut Context<Self>) {
        self.project_save_generation = self.project_save_generation.wrapping_add(1);
        let generation = self.project_save_generation;
        let projects = self.projects.clone();
        let previous = self.project_save_task.take();
        self.project_save_task = Some(cx.spawn(async move |view, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let save = cx
                .background_executor()
                .spawn(async move { projects.save() });
            let result = save.await;
            let _ = view.update(cx, |view, cx| {
                if view.project_save_generation != generation {
                    return;
                }
                match result {
                    Ok(()) => {
                        if view.project_feedback.as_deref()
                            == Some(ProjectRegistryError::InaccessibleStorage.message())
                        {
                            view.project_feedback = None;
                        }
                    }
                    Err(error) => view.project_feedback = Some(error.message().to_owned()),
                }
                cx.notify();
            });
        }));
    }

    fn refresh_project_catalogs(&mut self, cx: &mut Context<Self>) {
        self.project_scan_generation = self.project_scan_generation.wrapping_add(1);
        let generation = self.project_scan_generation;
        let paths = self
            .projects
            .projects()
            .iter()
            .filter(|project| !self.projects.is_active(&project.path))
            .map(|project| project.path.clone())
            .collect::<Vec<_>>();

        for path in &paths {
            let key = project_key(path);
            let loading = ProjectCatalogCache::loading(self.project_catalogs.get(&key));
            self.project_catalogs.insert(key, loading);
        }
        if paths.is_empty() {
            self.project_scan_task = None;
            cx.notify();
            return;
        }

        let scan = cx.background_executor().spawn(async move {
            paths
                .into_iter()
                .map(|path| {
                    let result = if path.is_dir() {
                        scan_sessions(&SessionCatalogConfig::from_environment(path.clone()))
                            .map_err(|error| error.summary)
                    } else {
                        Err("Project folder is unavailable.".to_owned())
                    };
                    (path, result)
                })
                .collect::<Vec<_>>()
        });
        self.project_scan_task = Some(cx.spawn(async move |view, cx| {
            let results = scan.await;
            let _ = view.update(cx, |view, cx| {
                if view.project_scan_generation != generation {
                    return;
                }
                for (path, result) in results {
                    let key = project_key(&path);
                    match result {
                        Ok(scan) => {
                            let status = if scan.sessions.is_empty() {
                                CatalogStatus::Empty
                            } else {
                                CatalogStatus::Ready
                            };
                            view.project_catalogs.insert(
                                key,
                                ProjectCatalogCache {
                                    status,
                                    sessions: Arc::new(scan.sessions),
                                    corrupt_count: scan.corrupt.len(),
                                    error: None,
                                },
                            );
                        }
                        Err(error) => {
                            let previous = view.project_catalogs.get(&key);
                            let sessions = previous
                                .map(|catalog| Arc::clone(&catalog.sessions))
                                .unwrap_or_default();
                            let status = if sessions.is_empty() {
                                CatalogStatus::Inaccessible
                            } else {
                                CatalogStatus::Stale
                            };
                            view.project_catalogs.insert(
                                key,
                                ProjectCatalogCache {
                                    status,
                                    sessions,
                                    corrupt_count: previous
                                        .map_or(0, |catalog| catalog.corrupt_count),
                                    error: Some(error),
                                },
                            );
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn cache_active_project_catalog(&mut self) {
        let catalog = &self.render_projections.catalog;
        self.project_catalogs.insert(
            project_key(self.projects.active_path()),
            ProjectCatalogCache {
                status: catalog.status,
                sessions: Arc::clone(&catalog.sessions),
                corrupt_count: catalog.corrupt.len(),
                error: catalog.error.clone(),
            },
        );
    }

    fn set_project_expanded(&mut self, path: PathBuf, expanded: bool, cx: &mut Context<Self>) {
        match self.projects.set_expanded(&path, expanded) {
            Ok(true) => self.persist_projects(cx),
            Ok(false) => {}
            Err(error) => self.project_feedback = Some(error.message().to_owned()),
        }
        cx.notify();
    }

    fn toggle_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match self.projects.toggle_expanded(&path) {
            Ok(_) => self.persist_projects(cx),
            Err(error) => self.project_feedback = Some(error.message().to_owned()),
        }
        cx.notify();
    }

    fn choose_attachments(&mut self, cx: &mut Context<Self>) {
        if self.attachment_picker_pending || !self.composer.read(cx).can_add_attachments() {
            return;
        }
        self.attachment_picker_pending = true;
        self.composer
            .update(cx, |composer, cx| composer.set_attachment_loading(true, cx));
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach files".into()),
        });
        self.attachment_task = Some(cx.spawn(async move |view, cx| {
            let selection = receiver.await;
            let paths = match selection {
                Ok(Ok(Some(paths))) if !paths.is_empty() => paths,
                Ok(Ok(Some(_))) | Ok(Ok(None)) => {
                    let _ = view.update(cx, |view, cx| {
                        view.attachment_picker_pending = false;
                        view.attachment_task = None;
                        view.composer.update(cx, |composer, cx| {
                            composer.set_attachment_loading(false, cx)
                        });
                    });
                    return;
                }
                Ok(Err(_)) | Err(_) => {
                    let _ = view.update(cx, |view, cx| {
                        view.attachment_picker_pending = false;
                        view.attachment_task = None;
                        view.composer.update(cx, |composer, cx| {
                            composer.set_feedback(
                                ComposerFeedback::Rejected(
                                    "The file picker could not be opened.".to_owned(),
                                ),
                                cx,
                            )
                        });
                    });
                    return;
                }
            };
            let Some(limits) = view
                .update(cx, |view, cx| {
                    view.composer.read(cx).attachment_load_limits()
                })
                .ok()
            else {
                return;
            };
            let batch = cx
                .background_executor()
                .spawn(async move { attachments::load_attachments(paths, limits) })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.attachment_picker_pending = false;
                view.attachment_task = None;
                view.composer.update(cx, |composer, cx| {
                    composer.add_loaded_attachments(batch, cx)
                });
            });
        }));
        cx.notify();
    }

    fn attach_dropped_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        if paths.is_empty()
            || self.attachment_picker_pending
            || !self.composer.read(cx).can_add_attachments()
        {
            return;
        }
        self.attachment_picker_pending = true;
        self.composer
            .update(cx, |composer, cx| composer.set_attachment_loading(true, cx));
        let paths = paths.to_vec();
        let limits = self.composer.read(cx).attachment_load_limits();
        self.attachment_task = Some(cx.spawn(async move |view, cx| {
            let batch = cx
                .background_executor()
                .spawn(async move { attachments::load_attachments(paths, limits) })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.attachment_picker_pending = false;
                view.attachment_task = None;
                view.composer.update(cx, |composer, cx| {
                    composer.add_loaded_attachments(batch, cx)
                });
            });
        }));
        cx.notify();
    }

    fn choose_projects(&mut self, cx: &mut Context<Self>) {
        if self.project_picker_pending {
            return;
        }
        self.project_picker_pending = true;
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add project folders".into()),
        });
        cx.spawn(async move |view, cx| {
            let selection = receiver.await;
            let _ = view.update(cx, |view, cx| {
                view.project_picker_pending = false;
                match selection {
                    Ok(Ok(Some(paths))) => {
                        let mut added = 0usize;
                        let mut duplicates = 0usize;
                        for path in paths {
                            match view.projects.add(path) {
                                Ok(AddProjectOutcome::Added) => added += 1,
                                Ok(AddProjectOutcome::AlreadyPresent) => duplicates += 1,
                                Err(error) => {
                                    view.project_feedback = Some(error.message().to_owned())
                                }
                            }
                        }
                        if added > 0 {
                            view.project_feedback = Some(format!(
                                "Added {added} project{}.",
                                if added == 1 { "" } else { "s" }
                            ));
                            view.persist_projects(cx);
                            view.refresh_project_catalogs(cx);
                        } else if duplicates > 0 {
                            view.project_feedback =
                                Some("That project is already in the sidebar.".to_owned());
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(_)) | Err(_) => {
                        view.project_feedback =
                            Some("The folder picker could not be opened.".to_owned())
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn reset_for_project_switch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.attachment_task.take();
        self.attachment_picker_pending = false;
        self.model_panel = None;
        self.command_palette_open = false;
        self.hotkey_help_open = false;
        self.compaction_modal_open = false;
        self.pasted_image_preview = None;
        self.pencil_stroke = None;
        self.pencil_undo.clear();
        self.pencil_error = None;
        self.session_rename_open = false;
        self.history_open = false;
        self.history = HistoryBrowser::default();
        self.history_confirmation = None;
        self.active_auth_prompt_id = None;
        self.last_auth_browser_launch = None;
        self.auth_browser_feedback = None;
        self.model_catalog_auto_refresh_pending = false;
        self.active_extension_dialog_id = None;
        self.extension_dialog_timeout_task = None;
        self.runtime_notifications.clear();
        self.selected_task_id = None;
        self.selected_subagent_id = None;
        self.workspace_diff = None;
        self.workspace_diff_identity = None;
        self.workspace_diff_generation = self.workspace_diff_generation.wrapping_add(1);
        self.workspace_diff_files_expanded = false;
        self.workspace_diff_open = false;
        self.workspace_diff_selected = 0;
        self.workspace_diff_collapsed_folders.clear();
        self.pending_draft = None;
        self.pending_bash = None;
        self.pending_compaction_focus = None;
        self.pending_session_name = None;
        self.sessions_scroll_motion.cancel();
        self.conversation_scroll_motion.cancel();
        self.conversation_follow.set(true);
        self.composer.update(cx, |composer, cx| {
            composer.set_feedback(ComposerFeedback::Ready, cx)
        });
        window.focus(&self.focus_handle);
    }

    fn save_active_thread_ui(&mut self, cx: &mut Context<Self>) {
        let ui = ThreadUiState {
            draft: self.composer.read(cx).draft().to_owned(),
            images: self.composer.read(cx).images().to_vec(),
            files: self.composer.read(cx).files().to_vec(),
            pending_draft: self.pending_draft.clone(),
            pending_bash: self.pending_bash.clone(),
            pending_compaction_focus: self.pending_compaction_focus.clone(),
            pending_session_name: self.pending_session_name.clone(),
        };
        if let Some(slot) = self
            .runtime_slots
            .iter_mut()
            .find(|slot| slot.id == self.active_runtime_id)
        {
            slot.ui = ui;
        }
    }

    fn restore_active_thread_ui(&mut self, cx: &mut Context<Self>) {
        let Some(slot) = self
            .runtime_slots
            .iter()
            .find(|slot| slot.id == self.active_runtime_id)
        else {
            return;
        };
        self.pending_draft = slot.ui.pending_draft.clone();
        self.pending_bash = slot.ui.pending_bash.clone();
        self.pending_compaction_focus = slot.ui.pending_compaction_focus.clone();
        self.pending_session_name = slot.ui.pending_session_name.clone();
        let draft = slot.ui.draft.clone();
        let images = slot.ui.images.clone();
        let files = slot.ui.files.clone();
        self.composer.update(cx, |composer, cx| {
            composer.restore_draft(&draft, images, files, cx)
        });
    }

    fn thread_statuses(&self) -> HashMap<String, ThreadRuntimeStatus> {
        self.runtime_slots
            .iter()
            .filter_map(|slot| {
                let session = slot
                    .projection
                    .pending_session_file
                    .as_ref()
                    .or(slot.requested_session.as_ref())
                    .or(slot.projection.session_file.as_ref())?;
                let activity = if matches!(
                    slot.projection.status,
                    crate::state::ControllerStatus::Connecting
                ) || slot.projection.lifecycle == RuntimeLifecycle::Loading
                {
                    ThreadActivity::Opening
                } else if slot.projection.lifecycle == RuntimeLifecycle::Running {
                    ThreadActivity::Working
                } else if slot.projection.lifecycle == RuntimeLifecycle::Cancelling
                    || slot.projection.status == crate::state::ControllerStatus::Stopping
                {
                    ThreadActivity::Cancelling
                } else if slot.projection.has_error
                    || matches!(
                        slot.projection.status,
                        crate::state::ControllerStatus::Failed
                    )
                {
                    ThreadActivity::Attention
                } else {
                    ThreadActivity::Idle
                };
                Some((
                    project_key(session),
                    ThreadRuntimeStatus {
                        project: project_key(&slot.project_path),
                        active: slot.id == self.active_runtime_id,
                        activity,
                    },
                ))
            })
            .collect()
    }

    fn on_thread_runtime_changed(
        &mut self,
        runtime_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .runtime_slots
            .iter()
            .position(|slot| slot.id == runtime_id)
        else {
            return;
        };
        let projection = self.runtime_slots[index]
            .controller
            .read(cx)
            .thread_runtime_projection();
        if let Some(session) = projection.session_file.clone() {
            self.runtime_slots[index].requested_session = Some(session);
        }
        self.runtime_slots[index].projection = projection;
        if runtime_id == self.active_runtime_id {
            self.sync_runtime_state(false, window, cx);
        } else {
            cx.notify();
        }
    }

    fn create_thread_runtime(
        &mut self,
        project_path: PathBuf,
        session: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u64 {
        let controller = cx.new(|cx| RuntimeController::for_workspace(project_path.clone(), cx));
        let runtime_id = self.next_runtime_id;
        self.next_runtime_id = self.next_runtime_id.saturating_add(1);
        let observation = cx.observe_in(&controller, window, move |view, _, window, cx| {
            view.on_thread_runtime_changed(runtime_id, window, cx)
        });
        let projection = controller.read(cx).thread_runtime_projection();
        self.runtime_observations.insert(runtime_id, observation);
        self.runtime_slots.push(ThreadRuntimeSlot {
            id: runtime_id,
            project_path,
            requested_session: session.clone(),
            controller: controller.clone(),
            projection,
            last_activated: 0,
            ui: ThreadUiState::default(),
        });
        controller.update(cx, |controller, cx| {
            controller.connect_to_session(session, cx)
        });
        runtime_id
    }

    fn activate_runtime(
        &mut self,
        runtime_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if runtime_id == self.active_runtime_id {
            return false;
        }
        let Some(index) = self
            .runtime_slots
            .iter()
            .position(|slot| slot.id == runtime_id)
        else {
            return false;
        };
        let project_path = self.runtime_slots[index].project_path.clone();
        let controller = self.runtime_slots[index].controller.clone();

        if !self.projects.is_active(&project_path) {
            self.cache_active_project_catalog();
            if let Err(error) = self.projects.set_active(&project_path) {
                self.project_feedback = Some(error.message().to_owned());
                return false;
            }
            self.persist_projects(cx);
        }
        self.save_active_thread_ui(cx);

        self.runtime_clock = self.runtime_clock.saturating_add(1);
        self.runtime_slots[index].last_activated = self.runtime_clock;
        self.active_runtime_id = runtime_id;
        self.controller = controller;
        self.reset_for_project_switch(window, cx);
        self.restore_active_thread_ui(cx);
        self.sync_runtime_state(true, window, cx);
        self.refresh_project_catalogs(cx);
        true
    }

    fn open_thread(
        &mut self,
        project_path: PathBuf,
        session: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !project_path.is_dir() {
            self.project_feedback = Some("Project folder is unavailable.".to_owned());
            cx.notify();
            return false;
        }
        let existing = session.as_ref().and_then(|session| {
            let session_key = project_key(session);
            self.runtime_slots
                .iter()
                .find(|slot| {
                    project_key(&slot.project_path) == project_key(&project_path)
                        && [
                            slot.projection.session_file.as_ref(),
                            slot.projection.pending_session_file.as_ref(),
                            slot.requested_session.as_ref(),
                        ]
                        .into_iter()
                        .flatten()
                        .any(|path| project_key(path) == session_key)
                })
                .map(|slot| slot.id)
        });
        let runtime_id = if let Some(runtime_id) = existing {
            runtime_id
        } else if self.reuse_idle_runtime_for_thread(&project_path, session.clone(), window, cx) {
            self.project_feedback = None;
            return true;
        } else {
            if self.runtime_slots.len() >= MAX_LIVE_THREAD_RUNTIMES
                && !self.evict_oldest_inactive_runtime(cx)
            {
                self.project_feedback = Some(format!(
                    "The {MAX_LIVE_THREAD_RUNTIMES}-thread runtime limit is active. Let background work finish or clear an inactive draft first."
                ));
                cx.notify();
                return false;
            }
            self.create_thread_runtime(project_path, session, window, cx)
        };
        self.project_feedback = None;
        self.activate_runtime(runtime_id, window, cx)
    }

    fn reuse_idle_runtime_for_thread(
        &mut self,
        project_path: &std::path::Path,
        session: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.save_active_thread_ui(cx);
        let project = project_key(project_path);
        let Some(runtime_id) = self
            .runtime_slots
            .iter()
            .filter(|slot| {
                project_key(&slot.project_path) == project
                    && can_reuse_runtime_for_navigation(&slot.projection, &slot.ui)
            })
            .min_by_key(|slot| (slot.id != self.active_runtime_id, slot.last_activated))
            .map(|slot| slot.id)
        else {
            return false;
        };
        let Some(index) = self
            .runtime_slots
            .iter()
            .position(|slot| slot.id == runtime_id)
        else {
            return false;
        };
        let controller = self.runtime_slots[index].controller.clone();
        let accepted = controller.update(cx, |controller, cx| {
            if let Some(path) = session.clone() {
                controller.switch_session(path, cx)
            } else {
                controller.new_session(cx)
            }
        });
        if !accepted {
            return false;
        }
        self.runtime_slots[index].requested_session = session;
        self.runtime_slots[index].ui = ThreadUiState::default();
        runtime_id == self.active_runtime_id || self.activate_runtime(runtime_id, window, cx)
    }

    fn evict_oldest_inactive_runtime(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(runtime_id) = self
            .runtime_slots
            .iter()
            .filter(|slot| {
                slot.id != self.active_runtime_id
                    && !slot.projection.keeps_background_process()
                    && slot.ui.can_evict()
            })
            .min_by_key(|slot| slot.last_activated)
            .map(|slot| slot.id)
        else {
            return false;
        };
        let Some(index) = self
            .runtime_slots
            .iter()
            .position(|slot| slot.id == runtime_id)
        else {
            return false;
        };
        let slot = self.runtime_slots.remove(index);
        slot.controller
            .update(cx, |controller, _| controller.shutdown());
        self.runtime_observations.remove(&runtime_id);
        true
    }

    fn activate_project(
        &mut self,
        path: PathBuf,
        session: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.projects.is_active(&path) && session.is_none() {
            self.toggle_project(path, cx);
            return;
        }
        let preferred_session = session.or_else(|| {
            self.projects
                .projects()
                .iter()
                .find(|project| project_key(&project.path) == project_key(&path))
                .and_then(|project| project.last_session.clone())
                .filter(|path| path.is_file())
        });
        let _ = self.open_thread(path, preferred_session, window, cx);
    }

    fn remove_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.save_active_thread_ui(cx);
        let path_key = project_key(&path);
        if self.runtime_slots.iter().any(|slot| {
            project_key(&slot.project_path) == path_key
                && (slot.projection.keeps_background_process() || !slot.ui.can_evict())
        }) {
            self.project_feedback = Some(
                "Finish background work and clear saved drafts before removing this project."
                    .to_owned(),
            );
            cx.notify();
            return;
        }
        let was_active = self.projects.is_active(&path);
        let next_available = self
            .projects
            .projects()
            .iter()
            .find(|project| project_key(&project.path) != path_key && project.path.is_dir())
            .map(|project| project.path.clone());
        if was_active && next_available.is_none() {
            self.project_feedback =
                Some("Add or restore another project folder before removing this one.".to_owned());
            cx.notify();
            return;
        }
        match self.projects.remove(&path) {
            Ok(_) => {}
            Err(error) => {
                self.project_feedback = Some(error.message().to_owned());
                cx.notify();
                return;
            }
        }
        self.project_catalogs.remove(&path_key);
        let removed_ids = self
            .runtime_slots
            .iter()
            .filter(|slot| project_key(&slot.project_path) == path_key)
            .map(|slot| slot.id)
            .collect::<Vec<_>>();
        for runtime_id in removed_ids {
            if let Some(index) = self
                .runtime_slots
                .iter()
                .position(|slot| slot.id == runtime_id)
            {
                let slot = self.runtime_slots.remove(index);
                slot.controller
                    .update(cx, |controller, _| controller.shutdown());
                self.runtime_observations.remove(&runtime_id);
            }
        }
        if let Some(next) = next_available
            && was_active
        {
            let preferred = self
                .projects
                .projects()
                .iter()
                .find(|project| project_key(&project.path) == project_key(&next))
                .and_then(|project| project.last_session.clone())
                .filter(|path| path.is_file());
            let _ = self.open_thread(next, preferred, window, cx);
        } else {
            self.refresh_project_catalogs(cx);
        }
        self.project_feedback = Some("Project removed from the sidebar.".to_owned());
        self.persist_projects(cx);
        cx.notify();
    }

    fn switch_session(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = self.open_thread(
            self.projects.active_path().to_path_buf(),
            Some(path),
            window,
            cx,
        );
    }

    fn set_hovered_thread(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if self.hovered_thread_key == key {
            return;
        }
        self.hovered_thread_key = key;
        cx.notify();
    }

    fn trash_thread(
        &mut self,
        project_path: PathBuf,
        session_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let session_key = project_key(&session_path);
        let live_runtime_id = self
            .runtime_slots
            .iter()
            .find(|slot| {
                [
                    slot.projection.session_file.as_ref(),
                    slot.projection.pending_session_file.as_ref(),
                    slot.requested_session.as_ref(),
                ]
                .into_iter()
                .flatten()
                .any(|path| project_key(path) == session_key)
            })
            .map(|slot| slot.id);

        if let Some(runtime_id) = live_runtime_id {
            let Some(slot) = self.runtime_slots.iter().find(|slot| slot.id == runtime_id) else {
                return;
            };
            let is_active_session = runtime_id == self.active_runtime_id
                || self
                    .render_projections
                    .catalog
                    .pending_session_file
                    .as_ref()
                    .or(self
                        .render_projections
                        .catalog
                        .current_session_file
                        .as_ref())
                    .is_some_and(|path| project_key(path) == session_key);
            if is_active_session {
                self.project_feedback =
                    Some("Switch to another thread before deleting this one.".to_owned());
                cx.notify();
                return;
            }
            if slot.projection.keeps_background_process() || !slot.ui.can_evict() {
                self.project_feedback = Some(
                    "Finish background work and clear drafts before deleting this thread."
                        .to_owned(),
                );
                cx.notify();
                return;
            }
        }

        let catalog_root = self
            .render_projections
            .catalog
            .root
            .as_ref()
            .filter(|_| self.projects.is_active(&project_path))
            .map(|root| root.path.clone())
            .unwrap_or_else(|| {
                SessionCatalogConfig::from_environment(project_path.clone())
                    .resolve_root()
                    .path
            });
        let active_session = self
            .render_projections
            .catalog
            .pending_session_file
            .as_ref()
            .or(self
                .render_projections
                .catalog
                .current_session_file
                .as_ref())
            .cloned();

        if let Err(error) =
            trash_session_file(&session_path, active_session.as_deref(), &catalog_root)
        {
            self.project_feedback = Some(error);
            cx.notify();
            return;
        }

        if let Some(runtime_id) = live_runtime_id
            && let Some(index) = self
                .runtime_slots
                .iter()
                .position(|slot| slot.id == runtime_id)
        {
            let slot = self.runtime_slots.remove(index);
            slot.controller
                .update(cx, |controller, _| controller.shutdown());
            self.runtime_observations.remove(&runtime_id);
        }

        let project_cache_key = project_key(&project_path);
        if let Some(cache) = self.project_catalogs.get_mut(&project_cache_key) {
            let sessions = Arc::make_mut(&mut cache.sessions);
            sessions.retain(|session| project_key(&session.path) != session_key);
            cache.status = if sessions.is_empty() {
                CatalogStatus::Empty
            } else {
                CatalogStatus::Ready
            };
        }

        if self.projects.is_active(&project_path) {
            self.controller.update(cx, |controller, cx| {
                controller.refresh_sessions(cx);
            });
        } else {
            self.refresh_project_catalogs(cx);
        }

        if self
            .projects
            .projects()
            .iter()
            .find(|project| project_key(&project.path) == project_cache_key)
            .and_then(|project| project.last_session.as_ref())
            .is_some_and(|last| project_key(last) == session_key)
            && self.projects.set_last_session(&project_path, None)
        {
            self.persist_projects(cx);
        }

        self.hovered_thread_key = None;
        self.project_feedback = Some("Thread moved to the Recycle Bin.".to_owned());
        cx.notify();
    }

    fn refresh_sessions(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.refresh_sessions(cx);
        });
        self.refresh_project_catalogs(cx);
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

    fn sync_workspace_diff(
        &mut self,
        conversation: &ConversationProjection,
        workspace: &str,
        cx: &mut Context<Self>,
    ) {
        let identity = latest_completed_response_key(conversation)
            .map(|key| (conversation.epoch.value(), key));
        if identity == self.workspace_diff_identity {
            return;
        }

        self.workspace_diff_identity = identity.clone();
        self.workspace_diff = None;
        self.workspace_diff_files_expanded = false;
        self.workspace_diff_open = false;
        self.workspace_diff_selected = 0;
        self.workspace_diff_collapsed_folders.clear();
        self.workspace_diff_files_scroll = ScrollHandle::new();
        self.workspace_diff_scroll = ScrollHandle::new();
        self.workspace_diff_generation = self.workspace_diff_generation.wrapping_add(1);
        self.conversation_list
            .refresh_trailing(&self.conversation_list_state);
        let generation = self.workspace_diff_generation;
        let Some(_) = identity else {
            cx.notify();
            return;
        };

        let workspace = PathBuf::from(workspace);
        let scan = cx
            .background_executor()
            .spawn(async move { load_workspace_diff(&workspace) });
        cx.spawn(async move |view, cx| {
            let result = scan.await;
            let _ = view.update(cx, |view, cx| {
                if view.workspace_diff_generation != generation {
                    return;
                }
                view.workspace_diff = result
                    .ok()
                    .filter(|snapshot| !snapshot.is_empty())
                    .map(Arc::new);
                view.conversation_list
                    .refresh_trailing(&view.conversation_list_state);
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::views) fn toggle_workspace_diff_files(&mut self, cx: &mut Context<Self>) {
        if self.workspace_diff.is_none() {
            return;
        }
        self.workspace_diff_files_expanded = !self.workspace_diff_files_expanded;
        self.conversation_list
            .refresh_trailing(&self.conversation_list_state);
        cx.notify();
    }

    pub(in crate::views) fn open_workspace_diff(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_diff.is_none() {
            return;
        }
        self.workspace_diff_selected = self.workspace_diff_selected.min(
            self.workspace_diff
                .as_ref()
                .map_or(0, |diff| diff.files.len().saturating_sub(1)),
        );
        self.workspace_diff_scroll = ScrollHandle::new();
        self.workspace_diff_open = true;
        window.focus(&self.workspace_diff_focus);
        cx.notify();
    }

    pub(in crate::views) fn select_workspace_diff_file(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = self.workspace_diff.clone() else {
            return;
        };
        let Some(file) = snapshot.files.get(index) else {
            return;
        };

        let selection_changed = index != self.workspace_diff_selected;
        let mut folder_path = String::new();
        let mut expanded = false;
        let target_path = file.path.rsplit(" → ").next().unwrap_or(&file.path);
        let mut parts = target_path.split('/').collect::<Vec<_>>();
        parts.pop();
        for folder in parts {
            if !folder_path.is_empty() {
                folder_path.push('/');
            }
            folder_path.push_str(folder);
            expanded |= self.workspace_diff_collapsed_folders.remove(&folder_path);
        }

        if !selection_changed && !expanded {
            return;
        }
        self.workspace_diff_selected = index;
        if let Some(row) = crate::views::diff_summary::file_tree_row_index(
            &snapshot,
            &self.workspace_diff_collapsed_folders,
            index,
        ) {
            self.workspace_diff_files_scroll.scroll_to_item(row);
        }
        if selection_changed {
            self.workspace_diff_scroll = ScrollHandle::new();
        }
        cx.notify();
    }

    pub(in crate::views) fn toggle_workspace_diff_folder(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_diff_collapsed_folders.remove(path) {
            self.workspace_diff_collapsed_folders
                .insert(path.to_owned());
        }
        cx.notify();
    }

    fn set_selected_workspace_diff_folder_collapsed(
        &mut self,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self
            .workspace_diff
            .as_ref()
            .and_then(|snapshot| snapshot.files.get(self.workspace_diff_selected))
        else {
            return;
        };
        let target_path = file.path.rsplit(" → ").next().unwrap_or(&file.path);
        let mut parts = target_path.split('/').collect::<Vec<_>>();
        parts.pop();
        let mut folder_path = String::new();
        let mut folders = Vec::new();
        for folder in parts {
            if !folder_path.is_empty() {
                folder_path.push('/');
            }
            folder_path.push_str(folder);
            folders.push(folder_path.clone());
        }

        let changed = if collapsed {
            folders
                .last()
                .is_some_and(|folder| self.workspace_diff_collapsed_folders.insert(folder.clone()))
        } else {
            folders
                .iter()
                .find(|folder| self.workspace_diff_collapsed_folders.contains(*folder))
                .cloned()
                .is_some_and(|folder| self.workspace_diff_collapsed_folders.remove(&folder))
        };
        if changed {
            cx.notify();
        }
    }

    fn move_workspace_diff_file(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(snapshot) = self.workspace_diff.as_ref() else {
            return;
        };
        let Some(next) = crate::views::diff_summary::adjacent_file_tree_index(
            snapshot,
            &self.workspace_diff_collapsed_folders,
            self.workspace_diff_selected,
            delta,
        ) else {
            return;
        };
        self.select_workspace_diff_file(next, cx);
    }

    pub(in crate::views) fn open_activity_detail(
        &mut self,
        detail: ActivityDetail,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activity_detail_restore_focus = window.focused(cx);
        self.activity_detail = Some(detail);
        self.activity_detail_scroll
            .set_offset(point(px(0.0), px(0.0)));
        window.focus(&self.activity_detail_focus);
        cx.notify();
    }

    fn close_activity_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity_detail.take().is_none() {
            return;
        }
        if let Some(focus) = self.activity_detail_restore_focus.take() {
            window.focus(&focus);
        } else {
            window.focus(&self.composer.read(cx).focus_handle(cx));
        }
        cx.notify();
    }

    pub(in crate::views) fn on_activity_detail_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                self.close_activity_detail(window, cx);
            }
            "tab" => {
                cx.stop_propagation();
                window.focus(&self.activity_detail_focus);
            }
            _ => {}
        }
    }

    pub(in crate::views) fn on_workspace_diff_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                self.close_workspace_diff(window, cx);
            }
            "up" | "k" => {
                cx.stop_propagation();
                self.move_workspace_diff_file(-1, cx);
            }
            "down" | "j" => {
                cx.stop_propagation();
                self.move_workspace_diff_file(1, cx);
            }
            "left" => {
                cx.stop_propagation();
                self.set_selected_workspace_diff_folder_collapsed(true, cx);
            }
            "right" => {
                cx.stop_propagation();
                self.set_selected_workspace_diff_folder_collapsed(false, cx);
            }
            "home" | "end" => {
                cx.stop_propagation();
                let last = event.keystroke.key == "end";
                let index = self.workspace_diff.as_ref().and_then(|snapshot| {
                    crate::views::diff_summary::edge_file_tree_index(
                        snapshot,
                        &self.workspace_diff_collapsed_folders,
                        last,
                    )
                });
                if let Some(index) = index {
                    self.select_workspace_diff_file(index, cx);
                }
            }
            "tab" => cx.stop_propagation(),
            _ => {}
        }
    }

    pub(in crate::views) fn close_workspace_diff(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_diff_open {
            return;
        }
        self.workspace_diff_open = false;
        window.focus(&self.composer.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn sync_runtime_state(
        &mut self,
        force_epoch_reset: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.try_auto_refresh_models(cx);
        self.sync_extension_ui(extension_ui, window, cx);
        self.sync_workspace_diff(&conversation, &render_projections.shell.workspace, cx);
        if !matches!(
            conversation.lifecycle,
            RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
        ) && self.workspace_diff_open
        {
            self.close_workspace_diff(window, cx);
        }
        let epoch_changed = force_epoch_reset || conversation.epoch != self.conversation.epoch;
        if epoch_changed {
            self.conversation_scroll_motion.cancel();
            self.conversation_follow.set(true);
            if self.activity_detail.take().is_some() {
                self.activity_detail_restore_focus = None;
                window.focus(&self.composer.read(cx).focus_handle(cx));
            }
        }
        self.transcript_cache.update(cx, |cache, _| {
            if force_epoch_reset {
                cache.reset(conversation.epoch);
            } else {
                cache.prepare_epoch(conversation.epoch);
            }
        });
        self.activity_disclosures.update(cx, |disclosures, _| {
            if force_epoch_reset {
                disclosures.reset(conversation.epoch);
            } else {
                disclosures.prepare_epoch(conversation.epoch);
            }
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
        let active_project_path = self.projects.active_path().to_path_buf();
        if let Some(session_file) = render_projections.catalog.current_session_file.clone()
            && self
                .projects
                .set_last_session(&active_project_path, Some(session_file))
        {
            self.persist_projects(cx);
        }
        let catalog = &render_projections.catalog;
        let bridge = &render_projections.bridge;
        let rename_available =
            compact_available && catalog.current_session_file.is_some() && !catalog.switching;
        if !rename_available {
            self.session_rename_open = false;
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
        self.sync_provider_auth(render_projections.models.auth.as_ref(), window, cx);
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
            self.sync_composer_completions(cx);
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
        if self.render_projections.models.auth.is_some() {
            window.focus(&self.provider_auth_focus);
            return;
        }
        if self.activity_detail.is_some() {
            window.focus(&self.activity_detail_focus);
            return;
        }
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
        if self.render_projections.models.auth.is_some() {
            window.focus(&self.provider_auth_focus);
            return;
        }
        if self.activity_detail.is_some() {
            window.focus(&self.activity_detail_focus);
            return;
        }
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
        if self.render_projections.models.auth.is_some()
            || self.activity_detail.is_some()
            || self.pasted_image_preview.is_some()
            || self.extension_ui.active_dialog.is_some()
        {
            return;
        }
        if self.command_palette_open {
            self.close_command_palette(window, cx);
        } else {
            self.open_command_palette(window, cx);
        }
    }

    fn on_show_hotkeys(&mut self, _: &ShowHotkeys, window: &mut Window, cx: &mut Context<Self>) {
        if self.render_projections.models.auth.is_some()
            || self.activity_detail.is_some()
            || self.pasted_image_preview.is_some()
            || self.extension_ui.active_dialog.is_some()
        {
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
        ExtensionDialogKey, ThreadUiState, can_reuse_runtime_for_navigation,
        model_panels::{model_choices, model_provider_choices, thinking_choices},
        overlays::{extension_dialog_key, wrapped_index},
        shared::short_path,
    };
    use crate::controller::{ModelRuntimeProjection, ThreadRuntimeProjection, UsageProjection};
    use crate::model_runtime::{CatalogPhase, ModelChangePolicy, ModelIdentity, ThinkingLevel};
    use crate::state::ControllerStatus;
    use crate::state::runtime::{ModelSummary, RuntimeLifecycle, RuntimeThinkingLevel};

    #[test]
    fn idle_empty_runtime_is_reused_but_live_or_stateful_threads_are_not() {
        let mut projection = ThreadRuntimeProjection {
            workspace: "workspace".to_owned(),
            status: ControllerStatus::Active,
            lifecycle: RuntimeLifecycle::Ready,
            session_file: None,
            session_name: None,
            pending_session_file: None,
            pending_operation: false,
            has_error: false,
        };
        let mut ui = ThreadUiState::default();
        assert!(can_reuse_runtime_for_navigation(&projection, &ui));

        projection.lifecycle = RuntimeLifecycle::Running;
        assert!(!can_reuse_runtime_for_navigation(&projection, &ui));
        projection.lifecycle = RuntimeLifecycle::Ready;
        projection.pending_operation = true;
        assert!(!can_reuse_runtime_for_navigation(&projection, &ui));
        projection.pending_operation = false;
        ui.draft = "keep this thread".to_owned();
        assert!(!can_reuse_runtime_for_navigation(&projection, &ui));
    }

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
    fn model_choices_group_and_filter_stock_rpc_models() {
        let projection = ModelRuntimeProjection {
            phase: CatalogPhase::Failed("SDK catalog unavailable".into()),
            catalog: None,
            stock_models: Arc::new(vec![
                ModelSummary {
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
                },
                ModelSummary {
                    provider: "anthropic".into(),
                    id: "claude-test".into(),
                    name: "Claude Test".into(),
                    reasoning: true,
                    supported_thinking: vec![RuntimeThinkingLevel::Off],
                    context_window: 200_000,
                    max_tokens: 8_192,
                    supports_images: true,
                },
            ]),
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

        let choices = model_choices(&projection, Some("openai"), "gpt");
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].identity.provider, "openai");
        assert_eq!(choices[0].identity.id, "gpt-test");
        assert_eq!(choices[0].context_window, 128_000);
        assert!(model_choices(&projection, Some("anthropic"), "gpt").is_empty());
        assert_eq!(
            model_choices(&projection, None, "test")
                .into_iter()
                .map(|choice| choice.identity.provider)
                .collect::<Vec<_>>(),
            vec!["openai", "anthropic"]
        );
        let providers = model_provider_choices(&projection);
        assert_eq!(
            providers
                .iter()
                .map(|provider| (provider.id.as_str(), provider.model_count))
                .collect::<Vec<_>>(),
            vec![("openai", 1), ("anthropic", 1)]
        );
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
        // Digit shortcuts only apply to select so multi-select free-text input can type numbers.
        assert_eq!(
            extension_dialog_key(Some("select"), "1"),
            Some(ExtensionDialogKey::SelectIndex(0))
        );
        assert_eq!(
            extension_dialog_key(Some("select"), "9"),
            Some(ExtensionDialogKey::SelectIndex(8))
        );
        assert_eq!(extension_dialog_key(Some("select"), "0"), None);
        assert_eq!(extension_dialog_key(Some("input"), "1"), None);
        assert_eq!(extension_dialog_key(Some("confirm"), "2"), None);
    }
}
