use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use super::ToolCallId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueueMode {
    All,
    OneAtATime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageContent {
    #[serde(rename = "type")]
    pub content_type: ImageContentType,
    pub data: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageContentType {
    #[serde(rename = "image")]
    Image,
}

impl ImageContent {
    pub fn new(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            content_type: ImageContentType::Image,
            data: data.into(),
            mime_type: mime_type.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextContent {
    pub text: String,
    #[serde(rename = "textSignature", skip_serializing_if = "Option::is_none")]
    pub text_signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingContent {
    pub thinking: String,
    #[serde(rename = "thinkingSignature", skip_serializing_if = "Option::is_none")]
    pub thinking_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redacted: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(rename = "type")]
    pub content_type: ToolCallContentType,
    pub id: ToolCallId,
    pub name: String,
    pub arguments: Value,
    #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallContentType {
    #[serde(rename = "toolCall")]
    ToolCall,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UserContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(rename = "textSignature", skip_serializing_if = "Option::is_none")]
        text_signature: Option<String>,
    },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Blocks(Vec<UserContentBlock>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AssistantContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(rename = "textSignature", skip_serializing_if = "Option::is_none")]
        text_signature: Option<String>,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(rename = "thinkingSignature", skip_serializing_if = "Option::is_none")]
        thinking_signature: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        redacted: Option<bool>,
    },
    #[serde(rename = "toolCall")]
    ToolCall {
        id: ToolCallId,
        name: String,
        arguments: Value,
        #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageCost {
    pub input: f64,
    pub output: f64,
    #[serde(rename = "cacheRead")]
    pub cache_read: f64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: f64,
    pub total: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    #[serde(rename = "cacheRead")]
    pub cache_read: u64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: u64,
    #[serde(rename = "cacheWrite1h", skip_serializing_if = "Option::is_none")]
    pub cache_write_1h: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
    #[serde(rename = "totalTokens")]
    pub total_tokens: u64,
    pub cost: UsageCost,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticError {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessageDiagnostic {
    #[serde(rename = "type")]
    pub diagnostic_type: String,
    pub timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DiagnosticError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "length")]
    Length,
    #[serde(rename = "toolUse")]
    ToolUse,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "aborted")]
    Aborted,
    #[serde(rename = "deferred")]
    Deferred,
}

macro_rules! message_role {
    ($name:ident, $variant:ident, $wire:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

message_role!(UserMessageRole, User, "user");
message_role!(AssistantMessageRole, Assistant, "assistant");
message_role!(ToolResultMessageRole, ToolResult, "toolResult");
message_role!(BashExecutionMessageRole, BashExecution, "bashExecution");
message_role!(CustomMessageRole, Custom, "custom");
message_role!(BranchSummaryMessageRole, BranchSummary, "branchSummary");
message_role!(
    CompactionSummaryMessageRole,
    CompactionSummary,
    "compactionSummary"
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    pub role: UserMessageRole,
    pub content: UserContent,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub role: AssistantMessageRole,
    pub content: Vec<AssistantContentBlock>,
    pub api: String,
    pub provider: String,
    pub model: String,
    #[serde(rename = "responseModel", skip_serializing_if = "Option::is_none")]
    pub response_model: Option<String>,
    #[serde(rename = "responseId", skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<AssistantMessageDiagnostic>>,
    pub usage: Usage,
    #[serde(rename = "stopReason")]
    pub stop_reason: StopReason,
    #[serde(rename = "errorMessage", skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub role: ToolResultMessageRole,
    #[serde(rename = "toolCallId")]
    pub tool_call_id: ToolCallId,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    pub content: Vec<UserContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(rename = "addedToolNames", skip_serializing_if = "Option::is_none")]
    pub added_tool_names: Option<Vec<String>>,
    #[serde(rename = "isError")]
    pub is_error: bool,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BashExecutionMessage {
    pub role: BashExecutionMessageRole,
    pub command: String,
    pub output: String,
    #[serde(rename = "exitCode", skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    pub truncated: bool,
    #[serde(rename = "fullOutputPath", skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
    pub timestamp: u64,
    #[serde(rename = "excludeFromContext", skip_serializing_if = "Option::is_none")]
    pub exclude_from_context: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomMessage {
    pub role: CustomMessageRole,
    #[serde(rename = "customType")]
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSummaryMessage {
    pub role: BranchSummaryMessageRole,
    pub summary: String,
    #[serde(rename = "fromId")]
    pub from_id: super::EntryId,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionSummaryMessage {
    pub role: CompactionSummaryMessageRole,
    pub summary: String,
    #[serde(rename = "tokensBefore")]
    pub tokens_before: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentMessage {
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
    BashExecution(BashExecutionMessage),
    Custom(CustomMessage),
    BranchSummary(BranchSummaryMessage),
    CompactionSummary(CompactionSummaryMessage),
    Unknown { role: String, raw: Value },
}

impl Serialize for AgentMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (role, value) = match self {
            Self::User(message) => ("user", serde_json::to_value(message)),
            Self::Assistant(message) => ("assistant", serde_json::to_value(message)),
            Self::ToolResult(message) => ("toolResult", serde_json::to_value(message)),
            Self::BashExecution(message) => ("bashExecution", serde_json::to_value(message)),
            Self::Custom(message) => ("custom", serde_json::to_value(message)),
            Self::BranchSummary(message) => ("branchSummary", serde_json::to_value(message)),
            Self::CompactionSummary(message) => {
                ("compactionSummary", serde_json::to_value(message))
            }
            Self::Unknown { raw, .. } => return raw.serialize(serializer),
        };
        let mut value = value.map_err(serde::ser::Error::custom)?;
        value
            .as_object_mut()
            .ok_or_else(|| serde::ser::Error::custom("message did not serialize as an object"))?
            .insert("role".to_owned(), Value::String(role.to_owned()));
        value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AgentMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let role = value
            .as_object()
            .and_then(|object| object.get("role"))
            .and_then(Value::as_str)
            .ok_or_else(|| D::Error::custom("agent message requires a string `role`"))?;

        macro_rules! decode {
            ($type:ty, $variant:ident) => {
                serde_json::from_value::<$type>(value)
                    .map(Self::$variant)
                    .map_err(D::Error::custom)
            };
        }

        match role {
            "user" => decode!(UserMessage, User),
            "assistant" => decode!(AssistantMessage, Assistant),
            "toolResult" => decode!(ToolResultMessage, ToolResult),
            "bashExecution" => decode!(BashExecutionMessage, BashExecution),
            "custom" => decode!(CustomMessage, Custom),
            "branchSummary" => decode!(BranchSummaryMessage, BranchSummary),
            "compactionSummary" => decode!(CompactionSummaryMessage, CompactionSummary),
            unknown => Ok(Self::Unknown {
                role: unknown.to_owned(),
                raw: value,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCostRates {
    pub input: f64,
    pub output: f64,
    #[serde(rename = "cacheRead")]
    pub cache_read: f64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCostTier {
    pub input: f64,
    pub output: f64,
    #[serde(rename = "cacheRead")]
    pub cache_read: f64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: f64,
    #[serde(rename = "inputTokensAbove")]
    pub input_tokens_above: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCost {
    pub input: f64,
    pub output: f64,
    #[serde(rename = "cacheRead")]
    pub cache_read: f64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<ModelCostTier>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelInput {
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    pub reasoning: bool,
    #[serde(rename = "thinkingLevelMap", skip_serializing_if = "Option::is_none")]
    pub thinking_level_map: Option<BTreeMap<String, Option<String>>>,
    pub input: Vec<ModelInput>,
    pub cost: ModelCost,
    #[serde(rename = "contextWindow")]
    pub context_window: u64,
    #[serde(rename = "maxTokens")]
    pub max_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compat: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub path: String,
    pub source: String,
    pub scope: SourceScope,
    pub origin: SourceOrigin,
    #[serde(rename = "baseDir", skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceScope {
    User,
    Project,
    Temporary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceOrigin {
    #[serde(rename = "package")]
    Package,
    #[serde(rename = "top-level")]
    TopLevel,
}
