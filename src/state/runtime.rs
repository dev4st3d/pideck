use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde_json::Value;

use crate::services::rpc::{
    ConnectionGeneration, EntryId, RequestId, SessionEpoch, SessionId, ToolCallId,
};

pub const MAX_RUNTIME_ERRORS: usize = 32;
pub const MAX_UNKNOWN_RECORDS: usize = 32;
pub const MAX_NOTIFICATIONS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLifecycle {
    Loading,
    Ready,
    Running,
    Cancelling,
    Settled,
    Disconnected,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeError {
    pub kind: ErrorKind,
    pub summary: String,
}

impl SafeError {
    pub fn new(kind: ErrorKind, summary: impl Into<String>) -> Self {
        Self {
            kind,
            summary: summary.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Rejected,
    UnknownOutcome,
    Disconnected,
    Protocol,
    Process,
    OptionalFacet,
    Extension,
    UnknownRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacetStatus {
    Loading,
    Ready,
    Failed(SafeError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Facet<T> {
    pub status: FacetStatus,
    pub data: Option<T>,
}

impl<T> Default for Facet<T> {
    fn default() -> Self {
        Self {
            status: FacetStatus::Loading,
            data: None,
        }
    }
}

impl<T> Facet<T> {
    pub fn loading(&mut self) {
        self.status = FacetStatus::Loading;
    }

    pub fn ready(&mut self, data: T) {
        self.data = Some(data);
        self.status = FacetStatus::Ready;
    }

    pub fn failed(&mut self, error: SafeError) {
        self.status = FacetStatus::Failed(error);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSnapshot {
    pub id: SessionId,
    pub file: Option<String>,
    pub name: Option<String>,
    pub model: Option<ModelSummary>,
    pub thinking_level: RuntimeThinkingLevel,
    pub steering_mode: QueueDeliveryMode,
    pub follow_up_mode: QueueDeliveryMode,
    pub auto_compaction_enabled: bool,
    pub message_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueDeliveryMode {
    All,
    OneAtATime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelSummary {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub reasoning: bool,
    pub context_window: u64,
    pub max_tokens: u64,
    pub supports_images: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeStats {
    pub session_id: SessionId,
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub total_messages: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub cost: f64,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub context_percent: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCommand {
    pub name: String,
    pub description: Option<String>,
    pub source: CommandSource,
    pub scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    Extension,
    Prompt,
    Skill,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageKey(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockKey(pub String);

#[derive(Debug, Clone, PartialEq)]
pub struct MessageUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cache_write_1h: Option<u64>,
    pub reasoning: Option<u64>,
    pub total_tokens: u64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: f64,
    pub cache_write_cost: f64,
    pub total_cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantMetadata {
    pub api: String,
    pub provider: String,
    pub model: String,
    pub response_model: Option<String>,
    pub usage: MessageUsage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeMessage {
    pub key: MessageKey,
    pub role: MessageRole,
    pub timestamp: u64,
    pub content: Vec<MessageBlock>,
    pub visible: bool,
    pub terminal: bool,
    pub stop_reason: Option<MessageStopReason>,
    pub error: Option<String>,
    pub assistant: Option<AssistantMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    ToolResult,
    BashExecution,
    Custom,
    BranchSummary,
    CompactionSummary,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageBlock {
    Text {
        key: BlockKey,
        text: String,
    },
    Thinking {
        key: BlockKey,
        text: String,
        redacted: bool,
    },
    Image {
        key: BlockKey,
        mime_type: String,
    },
    ToolCall {
        key: BlockKey,
        id: ToolCallId,
        name: String,
        arguments: Value,
    },
    ToolResult {
        key: BlockKey,
        id: ToolCallId,
        name: String,
        content: String,
        is_error: bool,
    },
    Bash {
        key: BlockKey,
        command: String,
        output: String,
        cancelled: bool,
    },
    Summary {
        key: BlockKey,
        text: String,
    },
    Custom {
        key: BlockKey,
        kind: String,
        text: String,
    },
    Unsupported {
        key: BlockKey,
        kind: String,
    },
}

impl MessageBlock {
    pub fn key(&self) -> &BlockKey {
        match self {
            Self::Text { key, .. }
            | Self::Thinking { key, .. }
            | Self::Image { key, .. }
            | Self::ToolCall { key, .. }
            | Self::ToolResult { key, .. }
            | Self::Bash { key, .. }
            | Self::Summary { key, .. }
            | Self::Custom { key, .. }
            | Self::Unsupported { key, .. } => key,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStopReason {
    Stop,
    Length,
    ToolUse,
    Error,
    Aborted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeEntry {
    pub id: EntryId,
    pub parent_id: Option<EntryId>,
    pub timestamp: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntryKind {
    Message(Box<RuntimeMessage>),
    ThinkingLevel(String),
    Model {
        provider: String,
        model_id: String,
    },
    Compaction {
        summary: String,
    },
    BranchSummary {
        summary: String,
    },
    Custom {
        kind: String,
    },
    CustomMessage {
        kind: String,
        content: Vec<MessageBlock>,
        display: bool,
    },
    Label {
        target: EntryId,
        label: Option<String>,
    },
    SessionInfo {
        name: Option<String>,
    },
    Unknown {
        entry_type: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeTreeNode {
    pub entry: RuntimeEntry,
    pub children: Vec<RuntimeTreeNode>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueueContents {
    Unknown {
        pending_count: u64,
    },
    Known {
        steering: Vec<String>,
        follow_up: Vec<String>,
    },
}

impl Default for QueueContents {
    fn default() -> Self {
        Self::Unknown { pending_count: 0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecution {
    pub id: ToolCallId,
    pub name: String,
    pub arguments: Value,
    pub result: Option<Value>,
    pub status: ToolStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryState {
    Idle,
    Waiting {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
    },
    Succeeded {
        attempt: u32,
    },
    Failed {
        attempt: u32,
        summary: String,
    },
    Cancelling,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompactionState {
    Idle,
    Running {
        reason: CompactionKind,
    },
    Completed {
        reason: CompactionKind,
        summary: String,
        will_retry: bool,
    },
    Failed {
        reason: CompactionKind,
        summary: String,
    },
    Aborted {
        reason: CompactionKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionKind {
    Manual,
    Threshold,
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionKind {
    Prompt,
    Steer,
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptimisticUserInput {
    pub request: RequestId,
    pub text: String,
    pub kind: SubmissionKind,
    pub accepted: bool,
    pub authoritative_seen: bool,
    pub baseline: HashSet<MessageKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptDelivery {
    None,
    Pending {
        request: RequestId,
        kind: SubmissionKind,
    },
    Accepted {
        request: RequestId,
        kind: SubmissionKind,
    },
    Rejected {
        request: RequestId,
        kind: SubmissionKind,
        summary: String,
    },
    Uncertain {
        request: RequestId,
        kind: SubmissionKind,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DialogRequest {
    Select {
        title: String,
        options: Vec<String>,
    },
    Confirm {
        title: String,
        message: String,
    },
    Input {
        title: String,
        placeholder: Option<String>,
    },
    Editor {
        title: String,
        prefill: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAnswer {
    Value(String),
    Confirmed(bool),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionStatus {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionWidget {
    pub lines: Vec<String>,
    pub placement: WidgetPlacement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetPlacement {
    AboveEditor,
    BelowEditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeNotification {
    pub message: String,
    pub kind: NotificationKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionFailure {
    pub extension: String,
    pub event: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRecord {
    pub record_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HydrationMode {
    Initial,
    Recovery,
    SessionReplacement,
    Resync,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeState {
    pub generation: ConnectionGeneration,
    pub epoch: SessionEpoch,
    pub lifecycle: RuntimeLifecycle,
    pub session: Facet<SessionSnapshot>,
    pub messages: Facet<Vec<RuntimeMessage>>,
    pub entries: Facet<Vec<RuntimeEntry>>,
    pub stats: Facet<RuntimeStats>,
    pub commands: Facet<Vec<RuntimeCommand>>,
    pub models: Facet<Vec<ModelSummary>>,
    pub tree: Facet<Vec<RuntimeTreeNode>>,
    pub tools: HashMap<ToolCallId, ToolExecution>,
    pub queue: QueueContents,
    pub retry: RetryState,
    pub compaction: CompactionState,
    pub dialogs: HashMap<RequestId, DialogRequest>,
    pub statuses: BTreeMap<String, ExtensionStatus>,
    pub widgets: BTreeMap<String, ExtensionWidget>,
    pub notifications: VecDeque<RuntimeNotification>,
    pub title: Option<String>,
    pub requested_editor_text: Option<String>,
    pub extension_errors: VecDeque<ExtensionFailure>,
    pub errors: VecDeque<SafeError>,
    pub unknown_records: VecDeque<UnknownRecord>,
    pub durable_cursor: Option<EntryId>,
    pub cursor_session_id: Option<SessionId>,
    pub live_message_keys: HashSet<MessageKey>,
    pub optimistic_user_inputs: Vec<OptimisticUserInput>,
    pub prompt_delivery: PromptDelivery,
    pub pending_prompt_settled: bool,
    pub replacement_awaiting_state: bool,
    pub low_level_agent_end_seen: bool,
    pub stale_inputs_ignored: u64,
    pub hydration_mode: HydrationMode,
    pub incremental_fallback_used: bool,
    pub revision: u64,
    pub next_request: u64,
}

impl RuntimeState {
    pub fn new(generation: ConnectionGeneration) -> Self {
        Self {
            generation,
            epoch: SessionEpoch::default(),
            lifecycle: RuntimeLifecycle::Loading,
            session: Facet::default(),
            messages: Facet::default(),
            entries: Facet::default(),
            stats: Facet::default(),
            commands: Facet::default(),
            models: Facet::default(),
            tree: Facet::default(),
            tools: HashMap::new(),
            queue: QueueContents::default(),
            retry: RetryState::Idle,
            compaction: CompactionState::Idle,
            dialogs: HashMap::new(),
            statuses: BTreeMap::new(),
            widgets: BTreeMap::new(),
            notifications: VecDeque::new(),
            title: None,
            requested_editor_text: None,
            extension_errors: VecDeque::new(),
            errors: VecDeque::new(),
            unknown_records: VecDeque::new(),
            durable_cursor: None,
            cursor_session_id: None,
            live_message_keys: HashSet::new(),
            optimistic_user_inputs: Vec::new(),
            prompt_delivery: PromptDelivery::None,
            pending_prompt_settled: false,
            replacement_awaiting_state: false,
            low_level_agent_end_seen: false,
            stale_inputs_ignored: 0,
            hydration_mode: HydrationMode::Initial,
            incremental_fallback_used: false,
            revision: 0,
            next_request: 1,
        }
    }

    pub fn session_id(&self) -> Option<&SessionId> {
        self.session.data.as_ref().map(|session| &session.id)
    }

    pub(crate) fn mark_hydration_loading(&mut self) {
        self.session.loading();
        self.messages.loading();
        self.entries.loading();
        self.stats.loading();
        self.commands.loading();
        self.models.loading();
        self.tree.loading();
    }

    pub(crate) fn bounded_error(&mut self, error: SafeError) {
        push_bounded(&mut self.errors, error, MAX_RUNTIME_ERRORS);
    }

    pub(crate) fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new(ConnectionGeneration::default())
    }
}

pub(crate) fn push_bounded<T>(values: &mut VecDeque<T>, value: T, capacity: usize) {
    if values.len() == capacity {
        values.pop_front();
    }
    values.push_back(value);
}

#[derive(Debug, Clone, PartialEq)]
pub struct StampedInput {
    pub generation: ConnectionGeneration,
    pub epoch: SessionEpoch,
    pub input: RuntimeInput,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeInput {
    Connected {
        recovery: bool,
    },
    Disconnected {
        error: SafeError,
    },
    Intent(RuntimeIntent),
    Response {
        request: RuntimeRequest,
        result: Result<NormalizedResponse, RequestFailure>,
    },
    Event(NormalizedEvent),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeIntent {
    Submit {
        request: RequestId,
        text: String,
        kind: SubmissionKind,
    },
    Abort,
    AbortRetry,
    ReplaceSession(SessionMutation),
    AnswerDialog {
        request: RequestId,
        answer: DialogAnswer,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionMutation {
    New { parent_session: Option<String> },
    Switch { session_path: String },
    Fork { entry_id: EntryId },
    Clone,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeRequest {
    GetState,
    GetMessages {
        base_revision: u64,
    },
    GetEntries {
        since: Option<EntryId>,
        base_revision: u64,
    },
    GetStats,
    GetCommands,
    GetModels,
    GetTree {
        base_revision: u64,
    },
    Submit {
        request: RequestId,
        text: String,
        kind: SubmissionKind,
    },
    Abort,
    AbortRetry,
    SessionMutation(SessionMutation),
}

#[derive(Debug, Clone, PartialEq)]
pub enum NormalizedResponse {
    State(NormalizedSessionState),
    Messages(Vec<RuntimeMessage>),
    Entries {
        entries: Vec<RuntimeEntry>,
        leaf_id: Option<EntryId>,
    },
    Stats(RuntimeStats),
    Commands(Vec<RuntimeCommand>),
    Models(Vec<ModelSummary>),
    Tree {
        tree: Vec<RuntimeTreeNode>,
        leaf_id: Option<EntryId>,
    },
    Accepted,
    SessionMutation {
        cancelled: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedSessionState {
    pub session: SessionSnapshot,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub pending_message_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFailure {
    pub kind: RequestFailureKind,
    pub error: SafeError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestFailureKind {
    Rejected,
    InvalidCursor,
    UnknownOutcome,
    Disconnected,
    Protocol,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NormalizedEvent {
    AgentStart,
    AgentEnd {
        will_retry: bool,
        messages: Vec<RuntimeMessage>,
    },
    AgentSettled,
    TurnStart,
    TurnEnd {
        message: RuntimeMessage,
        tool_results: Vec<RuntimeMessage>,
    },
    MessageStart(RuntimeMessage),
    MessageUpdate(RuntimeMessage),
    MessageEnd(RuntimeMessage),
    ToolStart {
        id: ToolCallId,
        name: String,
        arguments: Value,
    },
    ToolUpdate {
        id: ToolCallId,
        name: String,
        arguments: Value,
        accumulated: Value,
    },
    ToolEnd {
        id: ToolCallId,
        name: String,
        result: Value,
        is_error: bool,
        cancelled: bool,
    },
    QueueUpdate {
        steering: Vec<String>,
        follow_up: Vec<String>,
    },
    CompactionStart {
        reason: CompactionKind,
    },
    CompactionEnd {
        reason: CompactionKind,
        summary: Option<String>,
        aborted: bool,
        will_retry: bool,
        error: Option<String>,
    },
    RetryStart {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
    },
    RetryEnd {
        success: bool,
        attempt: u32,
        final_error: Option<String>,
    },
    EntryAppended(RuntimeEntry),
    SessionInfoChanged {
        name: Option<String>,
    },
    ThinkingLevelChanged {
        level: RuntimeThinkingLevel,
    },
    Dialog {
        id: RequestId,
        request: DialogRequest,
    },
    Notify(RuntimeNotification),
    SetStatus {
        key: String,
        value: Option<ExtensionStatus>,
    },
    SetWidget {
        key: String,
        value: Option<ExtensionWidget>,
    },
    SetTitle(String),
    SetEditorText(String),
    ExtensionError(ExtensionFailure),
    Unknown {
        record_type: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeEffect {
    pub generation: ConnectionGeneration,
    pub epoch: SessionEpoch,
    pub sequence: u64,
    pub effect: EffectKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectKind {
    Request(RuntimeRequest),
    ExtensionUiResponse {
        request: RequestId,
        answer: DialogAnswer,
    },
}
