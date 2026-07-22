use std::time::Instant;

use serde_json::Value;

use super::{
    AgentMessage, AssistantContentBlock, Command, CompactionReason, ExtensionError,
    ExtensionUiMethod, ExtensionUiResponse, ExtensionUiResponseBody, IncomingRecord,
    KnownExtensionUiMethod, KnownSessionEntry, Model, ModelInput, NotificationType, QueueMode,
    ResponseResult, RpcClientError, RpcClientErrorKind, RpcEvent, RpcResponse, SessionEntry,
    SessionEpoch, SessionTreeNode, SlashCommand, SlashCommandSource, StopReason,
    TaggedIncomingRecord, ThinkingLevel, UserContent, UserContentBlock,
};
use crate::state::runtime::{
    AssistantMetadata, BlockKey, CommandSource, CompactionKind, DialogAnswer, DialogRequest,
    DirectBashResult, EffectKind, EntryKind, ErrorKind, ExtensionFailure, ExtensionStatus,
    ExtensionWidget, MessageBlock, MessageKey, MessageRole, MessageStopReason, MessageUsage,
    ModelSummary, NormalizedEvent, NormalizedResponse, NormalizedSessionState, NotificationKind,
    QueueDeliveryMode, RequestFailure, RequestFailureKind, RuntimeCommand, RuntimeEffect,
    RuntimeEntry, RuntimeInput, RuntimeMessage, RuntimeNotification, RuntimeRequest, RuntimeStats,
    RuntimeThinkingLevel, RuntimeTreeNode, SafeError, SessionMutation, SessionSnapshot,
    StampedInput, SubmissionKind, ToolImage, WidgetPlacement,
};

#[derive(Debug, Clone, PartialEq)]
pub enum RpcDispatch {
    Command(Command),
    ExtensionUiResponse(ExtensionUiResponse),
}

pub fn dispatch_for_effect(effect: &RuntimeEffect) -> RpcDispatch {
    match &effect.effect {
        EffectKind::Request(request) => RpcDispatch::Command(command_for_request(request)),
        EffectKind::ExtensionUiResponse { request, answer } => {
            RpcDispatch::ExtensionUiResponse(ExtensionUiResponse {
                id: request.clone(),
                response: match answer {
                    DialogAnswer::Value(value) => ExtensionUiResponseBody::Value(value.clone()),
                    DialogAnswer::Confirmed(confirmed) => {
                        ExtensionUiResponseBody::Confirmed(*confirmed)
                    }
                    DialogAnswer::Cancelled => ExtensionUiResponseBody::Cancelled,
                },
            })
        }
    }
}

pub fn normalize_call_result(
    effect: &RuntimeEffect,
    result: Result<RpcResponse, RpcClientError>,
) -> Option<StampedInput> {
    let EffectKind::Request(request) = &effect.effect else {
        return None;
    };
    let result = match result {
        Ok(response) => normalize_response(request, response),
        Err(error) => Err(normalize_client_error(request, error)),
    };
    Some(StampedInput {
        generation: effect.generation,
        epoch: effect.epoch,
        observed_at: Instant::now(),
        input: RuntimeInput::Response {
            request: request.clone(),
            result,
        },
    })
}

pub fn normalize_tagged_record(
    tagged: TaggedIncomingRecord,
    epoch: SessionEpoch,
) -> Option<StampedInput> {
    let event = match tagged.record {
        IncomingRecord::Event(event) => normalize_event(*event),
        IncomingRecord::ExtensionUiRequest(request) => normalize_extension_request(request),
        IncomingRecord::ExtensionError(error) => {
            NormalizedEvent::ExtensionError(extension_error(error))
        }
        IncomingRecord::UnknownEvent(event) => NormalizedEvent::Unknown {
            record_type: safe_identifier(&event.event_type),
        },
        IncomingRecord::Response(_) => return None,
    };
    Some(StampedInput {
        generation: tagged.generation,
        epoch,
        observed_at: Instant::now(),
        input: RuntimeInput::Event(event),
    })
}

pub fn disconnected_input(error: RpcClientError, epoch: SessionEpoch) -> StampedInput {
    StampedInput {
        generation: error.generation,
        epoch,
        observed_at: Instant::now(),
        input: RuntimeInput::Disconnected {
            error: SafeError::new(error_kind(error.kind), connection_summary(error.kind)),
        },
    }
}

fn command_for_request(request: &RuntimeRequest) -> Command {
    match request {
        RuntimeRequest::GetState => Command::GetState,
        RuntimeRequest::GetMessages { .. } => Command::GetMessages,
        RuntimeRequest::GetEntries { since, .. } => Command::GetEntries {
            since: since.clone(),
        },
        RuntimeRequest::GetStats => Command::GetSessionStats,
        RuntimeRequest::GetCommands => Command::GetCommands,
        RuntimeRequest::GetModels => Command::GetAvailableModels,
        RuntimeRequest::GetTree { .. } => Command::GetTree,
        RuntimeRequest::Submit { text, kind, .. } => match kind {
            SubmissionKind::Prompt => Command::Prompt {
                message: text.clone(),
                images: None,
                streaming_behavior: None,
            },
            SubmissionKind::Steer => Command::Steer {
                message: text.clone(),
                images: None,
            },
            SubmissionKind::FollowUp => Command::FollowUp {
                message: text.clone(),
                images: None,
            },
        },
        RuntimeRequest::ExecuteBash {
            command,
            exclude_from_context,
            ..
        } => Command::Bash {
            command: command.clone(),
            exclude_from_context: Some(*exclude_from_context),
        },
        RuntimeRequest::Abort => Command::Abort,
        RuntimeRequest::AbortBash => Command::AbortBash,
        RuntimeRequest::AbortRetry => Command::AbortRetry,
        RuntimeRequest::SetSteeringMode { mode } => Command::SetSteeringMode {
            mode: queue_mode_for_command(*mode),
        },
        RuntimeRequest::SetFollowUpMode { mode } => Command::SetFollowUpMode {
            mode: queue_mode_for_command(*mode),
        },
        RuntimeRequest::Compact {
            custom_instructions,
        } => Command::Compact {
            custom_instructions: custom_instructions.clone(),
        },
        RuntimeRequest::SetAutoCompaction { enabled } => {
            Command::SetAutoCompaction { enabled: *enabled }
        }
        RuntimeRequest::SetAutoRetry { enabled } => Command::SetAutoRetry { enabled: *enabled },
        RuntimeRequest::SessionMutation(mutation) => match mutation {
            SessionMutation::New { parent_session } => Command::NewSession {
                parent_session: parent_session.clone(),
            },
            SessionMutation::Switch { session_path } => Command::SwitchSession {
                session_path: session_path.clone(),
            },
            SessionMutation::Fork { entry_id } => Command::Fork {
                entry_id: entry_id.clone(),
            },
            SessionMutation::Clone => Command::Clone,
        },
    }
}

fn normalize_response(
    request: &RuntimeRequest,
    response: RpcResponse,
) -> Result<NormalizedResponse, RequestFailure> {
    if let ResponseResult::Failure { .. } = response.result {
        let kind = if matches!(request, RuntimeRequest::GetEntries { since: Some(_), .. }) {
            RequestFailureKind::InvalidCursor
        } else {
            RequestFailureKind::Rejected
        };
        return Err(RequestFailure {
            kind,
            error: SafeError::new(ErrorKind::Rejected, rejected_summary(request)),
        });
    }

    let normalized = match (request, response.result) {
        (RuntimeRequest::GetState, ResponseResult::GetState(state)) => {
            NormalizedResponse::State(NormalizedSessionState {
                session: SessionSnapshot {
                    id: state.session_id,
                    file: state.session_file,
                    name: state.session_name,
                    model: state.model.map(model_summary),
                    thinking_level: thinking_level(state.thinking_level),
                    steering_mode: queue_mode(state.steering_mode),
                    follow_up_mode: queue_mode(state.follow_up_mode),
                    auto_compaction_enabled: state.auto_compaction_enabled,
                    message_count: state.message_count,
                },
                is_streaming: state.is_streaming,
                is_compacting: state.is_compacting,
                pending_message_count: state.pending_message_count,
            })
        }
        (RuntimeRequest::GetMessages { .. }, ResponseResult::GetMessages(data)) => {
            NormalizedResponse::Messages(data.messages.into_iter().map(persisted_message).collect())
        }
        (RuntimeRequest::GetEntries { .. }, ResponseResult::GetEntries(data)) => {
            NormalizedResponse::Entries {
                entries: data.entries.into_iter().map(runtime_entry).collect(),
                leaf_id: data.leaf_id,
            }
        }
        (RuntimeRequest::GetStats, ResponseResult::GetSessionStats(stats)) => {
            let (context_tokens, context_window, context_percent) = stats
                .context_usage
                .map(|context| {
                    (
                        context.tokens,
                        Some(context.context_window),
                        context.percent,
                    )
                })
                .unwrap_or((None, None, None));
            NormalizedResponse::Stats(RuntimeStats {
                session_id: stats.session_id,
                user_messages: stats.user_messages,
                assistant_messages: stats.assistant_messages,
                tool_calls: stats.tool_calls,
                tool_results: stats.tool_results,
                total_messages: stats.total_messages,
                input_tokens: stats.tokens.input,
                output_tokens: stats.tokens.output,
                cache_read_tokens: stats.tokens.cache_read,
                cache_write_tokens: stats.tokens.cache_write,
                total_tokens: stats.tokens.total,
                cost: stats.cost,
                context_tokens,
                context_window,
                context_percent,
            })
        }
        (RuntimeRequest::GetCommands, ResponseResult::GetCommands(data)) => {
            NormalizedResponse::Commands(data.commands.into_iter().map(runtime_command).collect())
        }
        (RuntimeRequest::GetModels, ResponseResult::GetAvailableModels(data)) => {
            NormalizedResponse::Models(data.models.into_iter().map(model_summary).collect())
        }
        (RuntimeRequest::GetTree { .. }, ResponseResult::GetTree(data)) => {
            NormalizedResponse::Tree {
                tree: data.tree.into_iter().map(runtime_tree).collect(),
                leaf_id: data.leaf_id,
            }
        }
        (
            RuntimeRequest::Submit {
                kind: SubmissionKind::Prompt,
                ..
            },
            ResponseResult::Prompt,
        )
        | (
            RuntimeRequest::Submit {
                kind: SubmissionKind::Steer,
                ..
            },
            ResponseResult::Steer,
        )
        | (
            RuntimeRequest::Submit {
                kind: SubmissionKind::FollowUp,
                ..
            },
            ResponseResult::FollowUp,
        )
        | (RuntimeRequest::Abort, ResponseResult::Abort)
        | (RuntimeRequest::AbortBash, ResponseResult::AbortBash)
        | (RuntimeRequest::AbortRetry, ResponseResult::AbortRetry)
        | (RuntimeRequest::SetSteeringMode { .. }, ResponseResult::SetSteeringMode)
        | (RuntimeRequest::SetFollowUpMode { .. }, ResponseResult::SetFollowUpMode)
        | (RuntimeRequest::SetAutoCompaction { .. }, ResponseResult::SetAutoCompaction)
        | (RuntimeRequest::SetAutoRetry { .. }, ResponseResult::SetAutoRetry) => {
            NormalizedResponse::Accepted
        }
        (RuntimeRequest::Compact { .. }, ResponseResult::Compact(result)) => {
            NormalizedResponse::Compacted {
                summary: result.summary,
            }
        }
        (RuntimeRequest::ExecuteBash { .. }, ResponseResult::Bash(result)) => {
            NormalizedResponse::Bash(DirectBashResult {
                output: result.output,
                exit_code: result.exit_code,
                cancelled: result.cancelled,
                truncated: result.truncated,
                full_output_path: result.full_output_path,
            })
        }
        (
            RuntimeRequest::SessionMutation(SessionMutation::New { .. }),
            ResponseResult::NewSession(data),
        )
        | (
            RuntimeRequest::SessionMutation(SessionMutation::Switch { .. }),
            ResponseResult::SwitchSession(data),
        )
        | (RuntimeRequest::SessionMutation(SessionMutation::Clone), ResponseResult::Clone(data)) => {
            NormalizedResponse::SessionMutation {
                cancelled: data.cancelled,
            }
        }
        (
            RuntimeRequest::SessionMutation(SessionMutation::Fork { .. }),
            ResponseResult::Fork(data),
        ) => NormalizedResponse::SessionMutation {
            cancelled: data.cancelled,
        },
        _ => {
            return Err(RequestFailure {
                kind: RequestFailureKind::Protocol,
                error: SafeError::new(
                    ErrorKind::Protocol,
                    "Pi returned a response for a different operation",
                ),
            });
        }
    };
    Ok(normalized)
}

fn normalize_client_error(request: &RuntimeRequest, error: RpcClientError) -> RequestFailure {
    let kind = match error.kind {
        RpcClientErrorKind::UnknownOutcome => RequestFailureKind::UnknownOutcome,
        RpcClientErrorKind::ProcessExit
        | RpcClientErrorKind::StdoutFault
        | RpcClientErrorKind::WriterFailure
        | RpcClientErrorKind::ConnectionPoisoned
        | RpcClientErrorKind::Stopped => RequestFailureKind::Disconnected,
        RpcClientErrorKind::Encoding | RpcClientErrorKind::ProtocolFault => {
            RequestFailureKind::Protocol
        }
    };
    RequestFailure {
        kind,
        error: SafeError::new(error_kind(error.kind), failure_summary(request, kind)),
    }
}

fn normalize_event(event: RpcEvent) -> NormalizedEvent {
    match event {
        RpcEvent::AgentStart => NormalizedEvent::AgentStart,
        RpcEvent::AgentEnd {
            messages,
            will_retry,
        } => NormalizedEvent::AgentEnd {
            will_retry,
            messages: messages.into_iter().map(runtime_message).collect(),
        },
        RpcEvent::AgentSettled => NormalizedEvent::AgentSettled,
        RpcEvent::TurnStart => NormalizedEvent::TurnStart,
        RpcEvent::TurnEnd {
            message,
            tool_results,
        } => NormalizedEvent::TurnEnd {
            message: runtime_message(message),
            tool_results: tool_results
                .into_iter()
                .map(|message| runtime_message(AgentMessage::ToolResult(message)))
                .collect(),
        },
        RpcEvent::MessageStart { message } => {
            NormalizedEvent::MessageStart(runtime_message(message))
        }
        RpcEvent::MessageUpdate { message, .. } => {
            // Pi's message field is the accumulated partial assistant message.
            NormalizedEvent::MessageUpdate(runtime_message(*message))
        }
        RpcEvent::MessageEnd { message } => NormalizedEvent::MessageEnd(runtime_message(message)),
        RpcEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
        } => NormalizedEvent::ToolStart {
            id: tool_call_id,
            name: tool_name,
            arguments: args,
        },
        RpcEvent::ToolExecutionUpdate {
            tool_call_id,
            tool_name,
            args,
            partial_result,
        } => NormalizedEvent::ToolUpdate {
            id: tool_call_id,
            name: tool_name,
            arguments: args,
            accumulated: partial_result,
        },
        RpcEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            result,
            is_error,
        } => {
            let cancelled = result
                .get("cancelled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            NormalizedEvent::ToolEnd {
                id: tool_call_id,
                name: tool_name,
                result,
                is_error,
                cancelled,
            }
        }
        RpcEvent::QueueUpdate {
            steering,
            follow_up,
        } => NormalizedEvent::QueueUpdate {
            steering,
            follow_up,
        },
        RpcEvent::CompactionStart { reason } => NormalizedEvent::CompactionStart {
            reason: compaction_kind(reason),
        },
        RpcEvent::CompactionEnd {
            reason,
            result,
            aborted,
            will_retry,
            error_message,
        } => NormalizedEvent::CompactionEnd {
            reason: compaction_kind(reason),
            summary: result.map(|result| result.summary),
            aborted,
            will_retry,
            error: error_message.map(|_| "Compaction failed".to_owned()),
        },
        RpcEvent::AutoRetryStart {
            attempt,
            max_attempts,
            delay_ms,
            ..
        } => NormalizedEvent::RetryStart {
            attempt,
            max_attempts,
            delay_ms,
        },
        RpcEvent::AutoRetryEnd {
            success,
            attempt,
            final_error,
        } => NormalizedEvent::RetryEnd {
            success,
            attempt,
            final_error: final_error.map(|_| "Automatic retry failed".to_owned()),
        },
        RpcEvent::EntryAppended { entry } => NormalizedEvent::EntryAppended(runtime_entry(entry)),
        RpcEvent::SessionInfoChanged { name } => NormalizedEvent::SessionInfoChanged { name },
        RpcEvent::ThinkingLevelChanged { level } => NormalizedEvent::ThinkingLevelChanged {
            level: thinking_level(level),
        },
    }
}

fn normalize_extension_request(request: super::ExtensionUiRequest) -> NormalizedEvent {
    let id = request.id;
    match request.request {
        ExtensionUiMethod::Known(KnownExtensionUiMethod::Select { title, options, .. }) => {
            NormalizedEvent::Dialog {
                id,
                request: DialogRequest::Select { title, options },
            }
        }
        ExtensionUiMethod::Known(KnownExtensionUiMethod::Confirm { title, message, .. }) => {
            NormalizedEvent::Dialog {
                id,
                request: DialogRequest::Confirm { title, message },
            }
        }
        ExtensionUiMethod::Known(KnownExtensionUiMethod::Input {
            title, placeholder, ..
        }) => NormalizedEvent::Dialog {
            id,
            request: DialogRequest::Input { title, placeholder },
        },
        ExtensionUiMethod::Known(KnownExtensionUiMethod::Editor { title, prefill }) => {
            NormalizedEvent::Dialog {
                id,
                request: DialogRequest::Editor { title, prefill },
            }
        }
        ExtensionUiMethod::Known(KnownExtensionUiMethod::Notify {
            message,
            notify_type,
        }) => NormalizedEvent::Notify(RuntimeNotification {
            message,
            kind: match notify_type.unwrap_or(NotificationType::Info) {
                NotificationType::Info => NotificationKind::Info,
                NotificationType::Warning => NotificationKind::Warning,
                NotificationType::Error => NotificationKind::Error,
            },
        }),
        ExtensionUiMethod::Known(KnownExtensionUiMethod::SetStatus {
            status_key,
            status_text,
        }) => NormalizedEvent::SetStatus {
            key: status_key,
            value: status_text.map(|text| ExtensionStatus { text }),
        },
        ExtensionUiMethod::Known(KnownExtensionUiMethod::SetWidget {
            widget_key,
            widget_lines,
            widget_placement,
        }) => NormalizedEvent::SetWidget {
            key: widget_key,
            value: widget_lines.map(|lines| ExtensionWidget {
                lines,
                placement: match widget_placement {
                    Some(super::WidgetPlacement::BelowEditor) => WidgetPlacement::BelowEditor,
                    Some(super::WidgetPlacement::AboveEditor) | None => {
                        WidgetPlacement::AboveEditor
                    }
                },
            }),
        },
        ExtensionUiMethod::Known(KnownExtensionUiMethod::SetTitle { title }) => {
            NormalizedEvent::SetTitle(title)
        }
        ExtensionUiMethod::Known(KnownExtensionUiMethod::SetEditorText { text }) => {
            NormalizedEvent::SetEditorText(text)
        }
        ExtensionUiMethod::Unknown { method, .. } => NormalizedEvent::Unknown {
            record_type: format!("extension_ui_request:{}", safe_identifier(&method)),
        },
    }
}

fn extension_error(error: ExtensionError) -> ExtensionFailure {
    let extension = error
        .extension_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .map(safe_identifier)
        .unwrap_or_else(|| "extension".to_owned());
    ExtensionFailure {
        extension,
        event: safe_identifier(&error.event),
        summary: "Extension execution failed".to_owned(),
    }
}

fn runtime_message(message: AgentMessage) -> RuntimeMessage {
    match message {
        AgentMessage::User(message) => {
            let fingerprint = user_content_fingerprint(&message.content);
            RuntimeMessage {
                key: message_key("user", message.timestamp, fingerprint),
                role: MessageRole::User,
                timestamp: message.timestamp,
                content: user_content(message.content),
                visible: true,
                terminal: true,
                stop_reason: None,
                error: None,
                assistant: None,
            }
        }
        AgentMessage::Assistant(message) => {
            let identity = stable_hash(
                format!("{}\0{}\0{}", message.api, message.provider, message.model).as_bytes(),
            );
            let content = message
                .content
                .into_iter()
                .enumerate()
                .map(|(index, block)| assistant_block(index, block))
                .collect();
            let usage = MessageUsage {
                input: message.usage.input,
                output: message.usage.output,
                cache_read: message.usage.cache_read,
                cache_write: message.usage.cache_write,
                cache_write_1h: message.usage.cache_write_1h,
                reasoning: message.usage.reasoning,
                total_tokens: message.usage.total_tokens,
                input_cost: message.usage.cost.input,
                output_cost: message.usage.cost.output,
                cache_read_cost: message.usage.cost.cache_read,
                cache_write_cost: message.usage.cost.cache_write,
                total_cost: message.usage.cost.total,
            };
            let stop_reason = match message.stop_reason {
                StopReason::Stop => MessageStopReason::Stop,
                StopReason::Length => MessageStopReason::Length,
                StopReason::ToolUse => MessageStopReason::ToolUse,
                StopReason::Error => MessageStopReason::Error,
                StopReason::Aborted => MessageStopReason::Aborted,
            };
            RuntimeMessage {
                key: message_key("assistant", message.timestamp, identity),
                role: MessageRole::Assistant,
                timestamp: message.timestamp,
                content,
                visible: true,
                terminal: false,
                stop_reason: Some(stop_reason),
                error: message
                    .error_message
                    .map(|_| "Assistant request failed".to_owned()),
                assistant: Some(AssistantMetadata {
                    api: message.api,
                    provider: message.provider,
                    model: message.model,
                    response_model: message.response_model,
                    usage,
                }),
            }
        }
        AgentMessage::ToolResult(message) => {
            let mut text = Vec::new();
            let mut images = Vec::new();
            for block in message.content {
                match block {
                    UserContentBlock::Text { text: value, .. } => text.push(value),
                    UserContentBlock::Image { data, mime_type } => {
                        images.push(ToolImage { data, mime_type });
                    }
                }
            }
            let text = text.join("\n");
            let tool_id = message.tool_call_id;
            RuntimeMessage {
                key: message_key(
                    "tool-result",
                    message.timestamp,
                    stable_hash(tool_id.as_str().as_bytes()),
                ),
                role: MessageRole::ToolResult,
                timestamp: message.timestamp,
                content: vec![MessageBlock::ToolResult {
                    key: block_key("tool-result", 0, Some(tool_id.as_str())),
                    id: tool_id,
                    name: message.tool_name,
                    content: text,
                    images,
                    details: message.details,
                    is_error: message.is_error,
                }],
                visible: true,
                terminal: true,
                stop_reason: None,
                error: None,
                assistant: None,
            }
        }
        AgentMessage::BashExecution(message) => {
            let identity = stable_hash(message.command.as_bytes());
            RuntimeMessage {
                key: message_key("bash", message.timestamp, identity),
                role: MessageRole::BashExecution,
                timestamp: message.timestamp,
                content: vec![MessageBlock::Bash {
                    key: block_key("bash", 0, None),
                    command: message.command,
                    output: message.output,
                    exit_code: message.exit_code,
                    cancelled: message.cancelled,
                    truncated: message.truncated,
                    full_output_path: message.full_output_path,
                    exclude_from_context: message.exclude_from_context.unwrap_or(false),
                }],
                visible: true,
                terminal: true,
                stop_reason: None,
                error: None,
                assistant: None,
            }
        }
        AgentMessage::Custom(message) => {
            let fingerprint = user_content_fingerprint(&message.content)
                ^ stable_hash(message.custom_type.as_bytes());
            RuntimeMessage {
                key: message_key("custom", message.timestamp, fingerprint),
                role: MessageRole::Custom,
                timestamp: message.timestamp,
                content: custom_content(message.custom_type, message.content),
                visible: message.display,
                terminal: true,
                stop_reason: None,
                error: None,
                assistant: None,
            }
        }
        AgentMessage::BranchSummary(message) => RuntimeMessage {
            key: message_key(
                "branch-summary",
                message.timestamp,
                stable_hash(message.from_id.as_str().as_bytes()),
            ),
            role: MessageRole::BranchSummary,
            timestamp: message.timestamp,
            content: vec![MessageBlock::Summary {
                key: block_key("branch-summary", 0, Some(message.from_id.as_str())),
                text: message.summary,
            }],
            visible: true,
            terminal: true,
            stop_reason: None,
            error: None,
            assistant: None,
        },
        AgentMessage::CompactionSummary(message) => RuntimeMessage {
            key: message_key(
                "compaction-summary",
                message.timestamp,
                message.tokens_before,
            ),
            role: MessageRole::CompactionSummary,
            timestamp: message.timestamp,
            content: vec![MessageBlock::Summary {
                key: block_key("compaction-summary", 0, None),
                text: message.summary,
            }],
            visible: true,
            terminal: true,
            stop_reason: None,
            error: None,
            assistant: None,
        },
        AgentMessage::Unknown { role, raw } => {
            let timestamp = raw.get("timestamp").and_then(Value::as_u64).unwrap_or(0);
            let identity = serde_json::to_vec(&raw)
                .map(|bytes| stable_hash(&bytes))
                .unwrap_or_else(|_| stable_hash(role.as_bytes()));
            let kind = safe_identifier(&role);
            RuntimeMessage {
                key: message_key("unknown", timestamp, identity),
                role: MessageRole::Unknown,
                timestamp,
                content: vec![MessageBlock::Unsupported {
                    key: block_key("unsupported", 0, Some(&kind)),
                    kind,
                }],
                visible: true,
                terminal: true,
                stop_reason: None,
                error: None,
                assistant: None,
            }
        }
    }
}

fn persisted_message(message: AgentMessage) -> RuntimeMessage {
    let mut message = runtime_message(message);
    message.terminal = true;
    message
}

fn assistant_block(index: usize, block: AssistantContentBlock) -> MessageBlock {
    match block {
        AssistantContentBlock::Text { text, .. } => MessageBlock::Text {
            key: block_key("text", index, None),
            text,
        },
        AssistantContentBlock::Thinking {
            thinking, redacted, ..
        } => MessageBlock::Thinking {
            key: block_key("thinking", index, None),
            text: thinking,
            redacted: redacted.unwrap_or(false),
        },
        AssistantContentBlock::ToolCall {
            id,
            name,
            arguments,
            ..
        } => MessageBlock::ToolCall {
            key: block_key("tool-call", index, Some(id.as_str())),
            id,
            name,
            arguments,
        },
    }
}

fn user_content(content: UserContent) -> Vec<MessageBlock> {
    match content {
        UserContent::Text(text) => vec![MessageBlock::Text {
            key: block_key("text", 0, None),
            text,
        }],
        UserContent::Blocks(blocks) => blocks
            .into_iter()
            .enumerate()
            .map(|(index, block)| match block {
                UserContentBlock::Text { text, .. } => MessageBlock::Text {
                    key: block_key("text", index, None),
                    text,
                },
                UserContentBlock::Image { data, mime_type } => MessageBlock::Image {
                    key: block_key("image", index, Some(&mime_type)),
                    mime_type,
                    data: Some(data),
                },
            })
            .collect(),
    }
}

fn custom_content(kind: String, content: UserContent) -> Vec<MessageBlock> {
    match content {
        UserContent::Text(text) => vec![MessageBlock::Custom {
            key: block_key("custom", 0, Some(&kind)),
            kind,
            text,
        }],
        UserContent::Blocks(blocks) => blocks
            .into_iter()
            .enumerate()
            .map(|(index, block)| match block {
                UserContentBlock::Text { text, .. } => MessageBlock::Custom {
                    key: block_key("custom", index, Some(&kind)),
                    kind: kind.clone(),
                    text,
                },
                UserContentBlock::Image { data, mime_type } => MessageBlock::Image {
                    key: block_key("image", index, Some(&mime_type)),
                    mime_type,
                    data: Some(data),
                },
            })
            .collect(),
    }
}

fn user_content_fingerprint(content: &UserContent) -> u64 {
    serde_json::to_vec(content)
        .map(|bytes| stable_hash(&bytes))
        .unwrap_or_default()
}

fn message_key(role: &str, timestamp: u64, identity: u64) -> MessageKey {
    MessageKey(format!("{role}:{timestamp}:{identity:016x}"))
}

fn block_key(kind: &str, index: usize, identity: Option<&str>) -> BlockKey {
    let identity = identity.map_or(0, |value| stable_hash(value.as_bytes()));
    BlockKey(format!("{kind}:{index}:{identity:016x}"))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn runtime_entry(entry: SessionEntry) -> RuntimeEntry {
    match entry {
        SessionEntry::Known(entry) => match *entry {
            KnownSessionEntry::Message { base, message } => RuntimeEntry {
                id: base.id,
                parent_id: base.parent_id,
                timestamp: base.timestamp,
                kind: EntryKind::Message(Box::new(persisted_message(message))),
            },
            KnownSessionEntry::ThinkingLevelChange {
                base,
                thinking_level,
            } => RuntimeEntry {
                id: base.id,
                parent_id: base.parent_id,
                timestamp: base.timestamp,
                kind: EntryKind::ThinkingLevel(thinking_level),
            },
            KnownSessionEntry::ModelChange {
                base,
                provider,
                model_id,
            } => RuntimeEntry {
                id: base.id,
                parent_id: base.parent_id,
                timestamp: base.timestamp,
                kind: EntryKind::Model { provider, model_id },
            },
            KnownSessionEntry::Compaction { base, summary, .. } => RuntimeEntry {
                id: base.id,
                parent_id: base.parent_id,
                timestamp: base.timestamp,
                kind: EntryKind::Compaction { summary },
            },
            KnownSessionEntry::BranchSummary { base, summary, .. } => RuntimeEntry {
                id: base.id,
                parent_id: base.parent_id,
                timestamp: base.timestamp,
                kind: EntryKind::BranchSummary { summary },
            },
            KnownSessionEntry::Custom {
                base, custom_type, ..
            } => RuntimeEntry {
                id: base.id,
                parent_id: base.parent_id,
                timestamp: base.timestamp,
                kind: EntryKind::Custom { kind: custom_type },
            },
            KnownSessionEntry::CustomMessage {
                base,
                custom_type,
                content,
                display,
                ..
            } => RuntimeEntry {
                id: base.id,
                parent_id: base.parent_id,
                timestamp: base.timestamp,
                kind: EntryKind::CustomMessage {
                    kind: custom_type,
                    content: user_content(content),
                    display,
                },
            },
            KnownSessionEntry::Label {
                base,
                target_id,
                label,
            } => RuntimeEntry {
                id: base.id,
                parent_id: base.parent_id,
                timestamp: base.timestamp,
                kind: EntryKind::Label {
                    target: target_id,
                    label,
                },
            },
            KnownSessionEntry::SessionInfo { base, name } => RuntimeEntry {
                id: base.id,
                parent_id: base.parent_id,
                timestamp: base.timestamp,
                kind: EntryKind::SessionInfo { name },
            },
        },
        SessionEntry::Unknown { entry_type, raw } => RuntimeEntry {
            id: raw
                .get("id")
                .and_then(Value::as_str)
                .map(super::EntryId::from)
                .unwrap_or_else(|| {
                    super::EntryId::new(format!("unknown:{}", safe_identifier(&entry_type)))
                }),
            parent_id: raw
                .get("parentId")
                .and_then(Value::as_str)
                .map(super::EntryId::from),
            timestamp: raw
                .get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            kind: EntryKind::Unknown {
                entry_type: safe_identifier(&entry_type),
            },
        },
    }
}

fn runtime_tree(node: SessionTreeNode) -> RuntimeTreeNode {
    RuntimeTreeNode {
        entry: runtime_entry(node.entry),
        children: node.children.into_iter().map(runtime_tree).collect(),
        label: node.label,
    }
}

fn model_summary(model: Model) -> ModelSummary {
    ModelSummary {
        provider: model.provider,
        id: model.id,
        name: model.name,
        reasoning: model.reasoning,
        context_window: model.context_window,
        max_tokens: model.max_tokens,
        supports_images: model.input.contains(&ModelInput::Image),
    }
}

fn runtime_command(command: SlashCommand) -> RuntimeCommand {
    RuntimeCommand {
        name: command.name,
        description: command.description,
        source: match command.source {
            SlashCommandSource::Extension => CommandSource::Extension,
            SlashCommandSource::Prompt => CommandSource::Prompt,
            SlashCommandSource::Skill => CommandSource::Skill,
        },
        scope: format!("{:?}", command.source_info.scope).to_lowercase(),
    }
}

fn thinking_level(level: ThinkingLevel) -> RuntimeThinkingLevel {
    match level {
        ThinkingLevel::Off => RuntimeThinkingLevel::Off,
        ThinkingLevel::Minimal => RuntimeThinkingLevel::Minimal,
        ThinkingLevel::Low => RuntimeThinkingLevel::Low,
        ThinkingLevel::Medium => RuntimeThinkingLevel::Medium,
        ThinkingLevel::High => RuntimeThinkingLevel::High,
        ThinkingLevel::Xhigh => RuntimeThinkingLevel::Xhigh,
        ThinkingLevel::Max => RuntimeThinkingLevel::Max,
    }
}

fn queue_mode(mode: QueueMode) -> QueueDeliveryMode {
    match mode {
        QueueMode::All => QueueDeliveryMode::All,
        QueueMode::OneAtATime => QueueDeliveryMode::OneAtATime,
    }
}

fn queue_mode_for_command(mode: QueueDeliveryMode) -> QueueMode {
    match mode {
        QueueDeliveryMode::All => QueueMode::All,
        QueueDeliveryMode::OneAtATime => QueueMode::OneAtATime,
    }
}

fn compaction_kind(reason: CompactionReason) -> CompactionKind {
    match reason {
        CompactionReason::Manual => CompactionKind::Manual,
        CompactionReason::Threshold => CompactionKind::Threshold,
        CompactionReason::Overflow => CompactionKind::Overflow,
    }
}

fn rejected_summary(request: &RuntimeRequest) -> String {
    format!("Pi rejected {}", operation_name(request))
}

fn failure_summary(request: &RuntimeRequest, kind: RequestFailureKind) -> String {
    match kind {
        RequestFailureKind::UnknownOutcome => {
            format!("The outcome of {} is unknown", operation_name(request))
        }
        RequestFailureKind::Disconnected => {
            format!("Pi disconnected during {}", operation_name(request))
        }
        RequestFailureKind::Protocol => {
            format!("Pi returned an invalid {} result", operation_name(request))
        }
        RequestFailureKind::Rejected | RequestFailureKind::InvalidCursor => {
            rejected_summary(request)
        }
    }
}

fn operation_name(request: &RuntimeRequest) -> &'static str {
    match request {
        RuntimeRequest::GetState => "state hydration",
        RuntimeRequest::GetMessages { .. } => "message hydration",
        RuntimeRequest::GetEntries { .. } => "entry hydration",
        RuntimeRequest::GetStats => "statistics hydration",
        RuntimeRequest::GetCommands => "command hydration",
        RuntimeRequest::GetModels => "model hydration",
        RuntimeRequest::GetTree { .. } => "tree hydration",
        RuntimeRequest::Submit { kind, .. } => match kind {
            SubmissionKind::Prompt => "prompt delivery",
            SubmissionKind::Steer => "steering delivery",
            SubmissionKind::FollowUp => "follow-up delivery",
        },
        RuntimeRequest::ExecuteBash { .. } => "Bash execution",
        RuntimeRequest::Abort => "abort",
        RuntimeRequest::AbortBash => "Bash cancellation",
        RuntimeRequest::AbortRetry => "retry cancellation",
        RuntimeRequest::SetSteeringMode { .. } => "steering mode update",
        RuntimeRequest::SetFollowUpMode { .. } => "follow-up mode update",
        RuntimeRequest::Compact { .. } => "manual compaction",
        RuntimeRequest::SetAutoCompaction { .. } => "auto-compaction update",
        RuntimeRequest::SetAutoRetry { .. } => "auto-retry update",
        RuntimeRequest::SessionMutation(_) => "session replacement",
    }
}

fn error_kind(kind: RpcClientErrorKind) -> ErrorKind {
    match kind {
        RpcClientErrorKind::UnknownOutcome => ErrorKind::UnknownOutcome,
        RpcClientErrorKind::Encoding | RpcClientErrorKind::ProtocolFault => ErrorKind::Protocol,
        RpcClientErrorKind::ProcessExit => ErrorKind::Process,
        RpcClientErrorKind::StdoutFault
        | RpcClientErrorKind::WriterFailure
        | RpcClientErrorKind::ConnectionPoisoned
        | RpcClientErrorKind::Stopped => ErrorKind::Disconnected,
    }
}

fn connection_summary(kind: RpcClientErrorKind) -> &'static str {
    match kind {
        RpcClientErrorKind::UnknownOutcome => "A Pi operation has an unknown outcome",
        RpcClientErrorKind::Encoding | RpcClientErrorKind::ProtocolFault => {
            "The Pi protocol connection failed"
        }
        RpcClientErrorKind::ProcessExit => "Pi exited unexpectedly",
        RpcClientErrorKind::StdoutFault
        | RpcClientErrorKind::WriterFailure
        | RpcClientErrorKind::ConnectionPoisoned
        | RpcClientErrorKind::Stopped => "The Pi connection closed",
    }
}

fn safe_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .take(80)
        .collect::<String>()
}
