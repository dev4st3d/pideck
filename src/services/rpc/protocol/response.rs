use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use super::{
    AgentMessage, BashResult, CompactionResult, EntryId, Model, RequestId, SessionEntry,
    SessionState, SessionStats, SessionTreeNode, SlashCommand, ThinkingLevel,
};

#[derive(Debug, Clone, PartialEq)]
pub struct RpcResponse {
    pub id: Option<RequestId>,
    pub result: ResponseResult,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResponseResult {
    Prompt,
    Steer,
    FollowUp,
    Abort,
    NewSession(CancelledData),
    GetState(SessionState),
    SetModel(Model),
    CycleModel(Option<ModelCycleData>),
    GetAvailableModels(AvailableModelsData),
    SetThinkingLevel,
    CycleThinkingLevel(Option<ThinkingLevelData>),
    SetSteeringMode,
    SetFollowUpMode,
    Compact(CompactionResult),
    SetAutoCompaction,
    SetAutoRetry,
    AbortRetry,
    Bash(BashResult),
    AbortBash,
    GetSessionStats(SessionStats),
    ExportHtml(ExportHtmlData),
    SwitchSession(CancelledData),
    Fork(ForkData),
    Clone(CancelledData),
    GetForkMessages(ForkMessagesData),
    GetEntries(EntriesData),
    GetTree(TreeData),
    GetLastAssistantText(LastAssistantTextData),
    SetSessionName,
    GetMessages(MessagesData),
    GetCommands(CommandsData),
    Failure {
        command: String,
        error: String,
    },
    UnknownSuccess {
        command: String,
        data: Option<Value>,
        raw: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelledData {
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCycleData {
    pub model: Model,
    #[serde(rename = "thinkingLevel")]
    pub thinking_level: ThinkingLevel,
    #[serde(rename = "isScoped")]
    pub is_scoped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AvailableModelsData {
    pub models: Vec<Model>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingLevelData {
    pub level: ThinkingLevel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportHtmlData {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkData {
    pub text: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkMessage {
    #[serde(rename = "entryId")]
    pub entry_id: EntryId,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkMessagesData {
    pub messages: Vec<ForkMessage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntriesData {
    pub entries: Vec<SessionEntry>,
    #[serde(rename = "leafId")]
    pub leaf_id: Option<EntryId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeData {
    pub tree: Vec<SessionTreeNode>,
    #[serde(rename = "leafId")]
    pub leaf_id: Option<EntryId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastAssistantTextData {
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessagesData {
    pub messages: Vec<AgentMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandsData {
    pub commands: Vec<SlashCommand>,
}

impl ResponseResult {
    pub fn command(&self) -> &str {
        match self {
            Self::Prompt => "prompt",
            Self::Steer => "steer",
            Self::FollowUp => "follow_up",
            Self::Abort => "abort",
            Self::NewSession(_) => "new_session",
            Self::GetState(_) => "get_state",
            Self::SetModel(_) => "set_model",
            Self::CycleModel(_) => "cycle_model",
            Self::GetAvailableModels(_) => "get_available_models",
            Self::SetThinkingLevel => "set_thinking_level",
            Self::CycleThinkingLevel(_) => "cycle_thinking_level",
            Self::SetSteeringMode => "set_steering_mode",
            Self::SetFollowUpMode => "set_follow_up_mode",
            Self::Compact(_) => "compact",
            Self::SetAutoCompaction => "set_auto_compaction",
            Self::SetAutoRetry => "set_auto_retry",
            Self::AbortRetry => "abort_retry",
            Self::Bash(_) => "bash",
            Self::AbortBash => "abort_bash",
            Self::GetSessionStats(_) => "get_session_stats",
            Self::ExportHtml(_) => "export_html",
            Self::SwitchSession(_) => "switch_session",
            Self::Fork(_) => "fork",
            Self::Clone(_) => "clone",
            Self::GetForkMessages(_) => "get_fork_messages",
            Self::GetEntries(_) => "get_entries",
            Self::GetTree(_) => "get_tree",
            Self::GetLastAssistantText(_) => "get_last_assistant_text",
            Self::SetSessionName => "set_session_name",
            Self::GetMessages(_) => "get_messages",
            Self::GetCommands(_) => "get_commands",
            Self::Failure { command, .. } | Self::UnknownSuccess { command, .. } => command,
        }
    }

    fn data_value(&self) -> Result<Option<Value>, serde_json::Error> {
        macro_rules! value {
            ($data:expr) => {
                serde_json::to_value($data).map(Some)
            };
        }

        match self {
            Self::NewSession(data) => value!(data),
            Self::GetState(data) => value!(data),
            Self::SetModel(data) => value!(data),
            Self::CycleModel(data) => value!(data),
            Self::GetAvailableModels(data) => value!(data),
            Self::CycleThinkingLevel(data) => value!(data),
            Self::Compact(data) => value!(data),
            Self::Bash(data) => value!(data),
            Self::GetSessionStats(data) => value!(data),
            Self::ExportHtml(data) => value!(data),
            Self::SwitchSession(data) => value!(data),
            Self::Fork(data) => value!(data),
            Self::Clone(data) => value!(data),
            Self::GetForkMessages(data) => value!(data),
            Self::GetEntries(data) => value!(data),
            Self::GetTree(data) => value!(data),
            Self::GetLastAssistantText(data) => value!(data),
            Self::GetMessages(data) => value!(data),
            Self::GetCommands(data) => value!(data),
            Self::UnknownSuccess { data, .. } => Ok(data.clone()),
            Self::Prompt
            | Self::Steer
            | Self::FollowUp
            | Self::Abort
            | Self::SetThinkingLevel
            | Self::SetSteeringMode
            | Self::SetFollowUpMode
            | Self::SetAutoCompaction
            | Self::SetAutoRetry
            | Self::AbortRetry
            | Self::AbortBash
            | Self::SetSessionName
            | Self::Failure { .. } => Ok(None),
        }
    }
}

impl Serialize for RpcResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let ResponseResult::UnknownSuccess { raw, .. } = &self.result {
            return raw.serialize(serializer);
        }

        let mut object = Map::new();
        object.insert("type".to_owned(), Value::String("response".to_owned()));
        if let Some(id) = &self.id {
            object.insert(
                "id".to_owned(),
                serde_json::to_value(id).map_err(serde::ser::Error::custom)?,
            );
        }
        object.insert(
            "command".to_owned(),
            Value::String(self.result.command().to_owned()),
        );
        match &self.result {
            ResponseResult::Failure { error, .. } => {
                object.insert("success".to_owned(), Value::Bool(false));
                object.insert("error".to_owned(), Value::String(error.clone()));
            }
            _ => {
                object.insert("success".to_owned(), Value::Bool(true));
                if let Some(data) = self
                    .result
                    .data_value()
                    .map_err(serde::ser::Error::custom)?
                {
                    object.insert("data".to_owned(), data);
                }
            }
        }
        Value::Object(object).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RpcResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::from_value(value).map_err(D::Error::custom)
    }
}

impl RpcResponse {
    pub(crate) fn from_value(value: Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "response must be a JSON object".to_owned())?;
        if object.get("type").and_then(Value::as_str) != Some("response") {
            return Err("response requires `type: \"response\"`".to_owned());
        }
        let id = object
            .get("id")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| format!("response has invalid `id`: {error}"))?;
        let command = object
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| "response requires a string `command`".to_owned())?;
        let success = object
            .get("success")
            .and_then(Value::as_bool)
            .ok_or_else(|| "response requires a boolean `success`".to_owned())?;

        if !success {
            let error = object
                .get("error")
                .and_then(Value::as_str)
                .ok_or_else(|| "failed response requires a string `error`".to_owned())?;
            return Ok(Self {
                id,
                result: ResponseResult::Failure {
                    command: command.to_owned(),
                    error: error.to_owned(),
                },
            });
        }

        fn decode<T: for<'de> Deserialize<'de>>(
            object: &Map<String, Value>,
            command: &str,
        ) -> Result<T, String> {
            let data = object
                .get("data")
                .cloned()
                .ok_or_else(|| format!("successful `{command}` response requires `data`"))?;
            serde_json::from_value(data)
                .map_err(|error| format!("invalid `{command}` response data: {error}"))
        }

        let result = match command {
            "prompt" => ResponseResult::Prompt,
            "steer" => ResponseResult::Steer,
            "follow_up" => ResponseResult::FollowUp,
            "abort" => ResponseResult::Abort,
            "new_session" => ResponseResult::NewSession(decode(object, command)?),
            "get_state" => ResponseResult::GetState(decode(object, command)?),
            "set_model" => ResponseResult::SetModel(decode(object, command)?),
            "cycle_model" => ResponseResult::CycleModel(decode(object, command)?),
            "get_available_models" => ResponseResult::GetAvailableModels(decode(object, command)?),
            "set_thinking_level" => ResponseResult::SetThinkingLevel,
            "cycle_thinking_level" => ResponseResult::CycleThinkingLevel(decode(object, command)?),
            "set_steering_mode" => ResponseResult::SetSteeringMode,
            "set_follow_up_mode" => ResponseResult::SetFollowUpMode,
            "compact" => ResponseResult::Compact(decode(object, command)?),
            "set_auto_compaction" => ResponseResult::SetAutoCompaction,
            "set_auto_retry" => ResponseResult::SetAutoRetry,
            "abort_retry" => ResponseResult::AbortRetry,
            "bash" => ResponseResult::Bash(decode(object, command)?),
            "abort_bash" => ResponseResult::AbortBash,
            "get_session_stats" => ResponseResult::GetSessionStats(decode(object, command)?),
            "export_html" => ResponseResult::ExportHtml(decode(object, command)?),
            "switch_session" => ResponseResult::SwitchSession(decode(object, command)?),
            "fork" => ResponseResult::Fork(decode(object, command)?),
            "clone" => ResponseResult::Clone(decode(object, command)?),
            "get_fork_messages" => ResponseResult::GetForkMessages(decode(object, command)?),
            "get_entries" => ResponseResult::GetEntries(decode(object, command)?),
            "get_tree" => ResponseResult::GetTree(decode(object, command)?),
            "get_last_assistant_text" => {
                ResponseResult::GetLastAssistantText(decode(object, command)?)
            }
            "set_session_name" => ResponseResult::SetSessionName,
            "get_messages" => ResponseResult::GetMessages(decode(object, command)?),
            "get_commands" => ResponseResult::GetCommands(decode(object, command)?),
            unknown => ResponseResult::UnknownSuccess {
                command: unknown.to_owned(),
                data: object.get("data").cloned(),
                raw: value.clone(),
            },
        };
        Ok(Self { id, result })
    }
}
