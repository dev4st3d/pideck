use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    AgentMessage, AssistantContentBlock, AssistantMessage, CompactionResult, SessionEntry,
    StopReason, ThinkingLevel, ToolCall, ToolCallId, ToolResultMessage, Usage,
};

/// Pi 0.84 streams assistant deltas instead of repeating an accumulated message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AssistantStreamEvent {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "text_start")]
    TextStart {
        #[serde(rename = "contentIndex")]
        content_index: usize,
    },
    #[serde(rename = "text_delta")]
    TextDelta {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        delta: String,
    },
    #[serde(rename = "text_end")]
    TextEnd {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        content: String,
    },
    #[serde(rename = "thinking_start")]
    ThinkingStart {
        #[serde(rename = "contentIndex")]
        content_index: usize,
    },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        delta: String,
    },
    #[serde(rename = "thinking_end")]
    ThinkingEnd {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        content: String,
    },
    #[serde(rename = "toolcall_start")]
    ToolCallStart {
        #[serde(rename = "contentIndex")]
        content_index: usize,
    },
    #[serde(rename = "toolcall_delta")]
    ToolCallDelta {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        delta: String,
    },
    #[serde(rename = "toolcall_end")]
    ToolCallEnd {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        #[serde(rename = "toolCall")]
        tool_call: ToolCall,
    },
    #[serde(rename = "done")]
    Done {
        reason: StopReason,
        message: AssistantMessage,
    },
    #[serde(rename = "error")]
    Error {
        reason: StopReason,
        error: AssistantMessage,
    },
}

#[derive(Debug, Default)]
pub(crate) struct AssistantStreamAssembler {
    current: Option<AssistantMessage>,
}

impl AssistantStreamAssembler {
    pub(crate) fn prepare(&mut self, event: &mut RpcEvent) -> bool {
        match event {
            RpcEvent::MessageStart {
                message: AgentMessage::Assistant(message),
            } => {
                self.current = Some(message.clone());
                true
            }
            RpcEvent::MessageUpdate {
                message,
                usage,
                assistant_message_event,
            } => {
                if let Some(AgentMessage::Assistant(accumulated)) = message.as_deref() {
                    self.current = Some(accumulated.clone());
                    return true;
                }
                let Some(usage) = usage else {
                    return false;
                };
                let Some(assembled) = self.apply(usage, assistant_message_event) else {
                    return false;
                };
                *message = Some(Box::new(AgentMessage::Assistant(assembled)));
                true
            }
            RpcEvent::MessageEnd {
                message: AgentMessage::Assistant(_),
            }
            | RpcEvent::AgentSettled => {
                self.current = None;
                true
            }
            _ => true,
        }
    }

    fn apply(&mut self, usage: &Usage, event: &AssistantStreamEvent) -> Option<AssistantMessage> {
        match event {
            AssistantStreamEvent::Done { message, .. }
            | AssistantStreamEvent::Error { error: message, .. } => {
                self.current = Some(message.clone());
            }
            _ => {
                let message = self.current.as_mut()?;
                message.usage = usage.clone();
                match event {
                    AssistantStreamEvent::Start
                    | AssistantStreamEvent::ToolCallStart { .. }
                    | AssistantStreamEvent::ToolCallDelta { .. } => {}
                    AssistantStreamEvent::TextStart { content_index } => {
                        start_block(
                            &mut message.content,
                            *content_index,
                            AssistantContentBlock::Text {
                                text: String::new(),
                                text_signature: None,
                            },
                        )?;
                    }
                    AssistantStreamEvent::TextDelta {
                        content_index,
                        delta,
                    } => {
                        let block = text_block(&mut message.content, *content_index)?;
                        block.push_str(delta);
                    }
                    AssistantStreamEvent::TextEnd {
                        content_index,
                        content,
                    } => {
                        *text_block(&mut message.content, *content_index)? = content.clone();
                    }
                    AssistantStreamEvent::ThinkingStart { content_index } => {
                        start_block(
                            &mut message.content,
                            *content_index,
                            AssistantContentBlock::Thinking {
                                thinking: String::new(),
                                thinking_signature: None,
                                redacted: None,
                            },
                        )?;
                    }
                    AssistantStreamEvent::ThinkingDelta {
                        content_index,
                        delta,
                    } => {
                        let block = thinking_block(&mut message.content, *content_index)?;
                        block.push_str(delta);
                    }
                    AssistantStreamEvent::ThinkingEnd {
                        content_index,
                        content,
                    } => {
                        *thinking_block(&mut message.content, *content_index)? = content.clone();
                    }
                    AssistantStreamEvent::ToolCallEnd {
                        content_index,
                        tool_call,
                    } => {
                        start_block(
                            &mut message.content,
                            *content_index,
                            AssistantContentBlock::ToolCall {
                                id: tool_call.id.clone(),
                                name: tool_call.name.clone(),
                                arguments: tool_call.arguments.clone(),
                                thought_signature: tool_call.thought_signature.clone(),
                            },
                        )?;
                    }
                    AssistantStreamEvent::Done { .. } | AssistantStreamEvent::Error { .. } => {}
                }
            }
        }
        self.current.clone()
    }
}

fn start_block(
    blocks: &mut Vec<AssistantContentBlock>,
    index: usize,
    block: AssistantContentBlock,
) -> Option<()> {
    match index.cmp(&blocks.len()) {
        std::cmp::Ordering::Less => blocks[index] = block,
        std::cmp::Ordering::Equal => blocks.push(block),
        std::cmp::Ordering::Greater => return None,
    }
    Some(())
}

fn text_block(blocks: &mut Vec<AssistantContentBlock>, index: usize) -> Option<&mut String> {
    if index == blocks.len() {
        blocks.push(AssistantContentBlock::Text {
            text: String::new(),
            text_signature: None,
        });
    }
    match blocks.get_mut(index)? {
        AssistantContentBlock::Text { text, .. } => Some(text),
        _ => None,
    }
}

fn thinking_block(blocks: &mut Vec<AssistantContentBlock>, index: usize) -> Option<&mut String> {
    if index == blocks.len() {
        blocks.push(AssistantContentBlock::Thinking {
            thinking: String::new(),
            thinking_signature: None,
            redacted: None,
        });
    }
    match blocks.get_mut(index)? {
        AssistantContentBlock::Thinking { thinking, .. } => Some(thinking),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompactionReason {
    Manual,
    Threshold,
    Overflow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RpcEvent {
    #[serde(rename = "agent_start")]
    AgentStart,
    #[serde(rename = "agent_end")]
    AgentEnd {
        messages: Vec<AgentMessage>,
        #[serde(rename = "willRetry")]
        will_retry: bool,
    },
    #[serde(rename = "agent_settled")]
    AgentSettled,
    #[serde(rename = "turn_start")]
    TurnStart,
    #[serde(rename = "turn_end")]
    TurnEnd {
        message: AgentMessage,
        #[serde(rename = "toolResults")]
        tool_results: Vec<ToolResultMessage>,
    },
    #[serde(rename = "message_start")]
    MessageStart { message: AgentMessage },
    #[serde(rename = "message_update")]
    MessageUpdate {
        /// Pi <=0.83 supplied this field; the assembler fills it for Pi 0.84 deltas.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<Box<AgentMessage>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(rename = "assistantMessageEvent")]
        assistant_message_event: AssistantStreamEvent,
    },
    #[serde(rename = "message_end")]
    MessageEnd { message: AgentMessage },
    #[serde(rename = "bash_execution_update")]
    BashExecutionUpdate {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<super::RequestId>,
        delta: String,
    },
    #[serde(rename = "tool_execution_start")]
    ToolExecutionStart {
        #[serde(rename = "toolCallId")]
        tool_call_id: ToolCallId,
        #[serde(rename = "toolName")]
        tool_name: String,
        args: Value,
    },
    #[serde(rename = "tool_execution_update")]
    ToolExecutionUpdate {
        #[serde(rename = "toolCallId")]
        tool_call_id: ToolCallId,
        #[serde(rename = "toolName")]
        tool_name: String,
        args: Value,
        #[serde(rename = "partialResult")]
        partial_result: Value,
    },
    #[serde(rename = "tool_execution_end")]
    ToolExecutionEnd {
        #[serde(rename = "toolCallId")]
        tool_call_id: ToolCallId,
        #[serde(rename = "toolName")]
        tool_name: String,
        result: Value,
        #[serde(rename = "isError")]
        is_error: bool,
    },
    #[serde(rename = "queue_update")]
    QueueUpdate {
        steering: Vec<String>,
        #[serde(rename = "followUp")]
        follow_up: Vec<String>,
    },
    #[serde(rename = "compaction_start")]
    CompactionStart { reason: CompactionReason },
    #[serde(rename = "compaction_end")]
    CompactionEnd {
        reason: CompactionReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<CompactionResult>,
        aborted: bool,
        #[serde(rename = "willRetry")]
        will_retry: bool,
        #[serde(rename = "errorMessage", skip_serializing_if = "Option::is_none")]
        error_message: Option<String>,
    },
    #[serde(rename = "auto_retry_start")]
    AutoRetryStart {
        attempt: u32,
        #[serde(rename = "maxAttempts")]
        max_attempts: u32,
        #[serde(rename = "delayMs")]
        delay_ms: u64,
        #[serde(rename = "errorMessage")]
        error_message: String,
    },
    #[serde(rename = "auto_retry_end")]
    AutoRetryEnd {
        success: bool,
        attempt: u32,
        #[serde(rename = "finalError", skip_serializing_if = "Option::is_none")]
        final_error: Option<String>,
    },
    #[serde(rename = "entry_appended")]
    EntryAppended { entry: SessionEntry },
    #[serde(rename = "session_info_changed")]
    SessionInfoChanged {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    #[serde(rename = "thinking_level_changed")]
    ThinkingLevelChanged { level: ThinkingLevel },
}

pub(crate) fn is_known_event(record_type: &str) -> bool {
    matches!(
        record_type,
        "agent_start"
            | "agent_end"
            | "agent_settled"
            | "turn_start"
            | "turn_end"
            | "message_start"
            | "message_update"
            | "message_end"
            | "bash_execution_update"
            | "tool_execution_start"
            | "tool_execution_update"
            | "tool_execution_end"
            | "queue_update"
            | "compaction_start"
            | "compaction_end"
            | "auto_retry_start"
            | "auto_retry_end"
            | "entry_appended"
            | "session_info_changed"
            | "thinking_level_changed"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn assistant(content: Value, output: u64) -> Value {
        json!({
            "role": "assistant",
            "content": content,
            "api": "synthetic-api",
            "provider": "synthetic-provider",
            "model": "synthetic-model",
            "usage": usage(output),
            "stopReason": "pending",
            "timestamp": 42
        })
    }

    fn usage(output: u64) -> Value {
        json!({
            "input": 10,
            "output": output,
            "cacheRead": 2,
            "cacheWrite": 1,
            "totalTokens": 13 + output,
            "cost": {
                "input": 0.1,
                "output": 0.2,
                "cacheRead": 0.01,
                "cacheWrite": 0.02,
                "total": 0.33
            }
        })
    }

    fn decode(value: Value) -> RpcEvent {
        serde_json::from_value(value).expect("stream event should decode")
    }

    #[test]
    fn assistant_deltas_rebuild_accumulated_text_thinking_and_usage() {
        let mut assembler = AssistantStreamAssembler::default();
        let mut start = decode(json!({
            "type": "message_start",
            "message": assistant(json!([]), 0)
        }));
        assert!(assembler.prepare(&mut start));

        let updates = [
            json!({"type":"text_start","contentIndex":0}),
            json!({"type":"text_delta","contentIndex":0,"delta":"Hello "}),
            json!({"type":"text_delta","contentIndex":0,"delta":"world"}),
            json!({"type":"text_end","contentIndex":0,"content":"Hello world"}),
            json!({"type":"thinking_start","contentIndex":1}),
            json!({"type":"thinking_delta","contentIndex":1,"delta":"Checked"}),
            json!({"type":"thinking_end","contentIndex":1,"content":"Checked carefully"}),
        ];

        let mut latest = None;
        for (index, assistant_message_event) in updates.into_iter().enumerate() {
            let mut update = decode(json!({
                "type": "message_update",
                "usage": usage(index as u64 + 1),
                "assistantMessageEvent": assistant_message_event
            }));
            assert!(assembler.prepare(&mut update));
            let RpcEvent::MessageUpdate { message, .. } = update else {
                panic!("expected message update");
            };
            latest = message;
        }

        let Some(message) = latest else {
            panic!("expected accumulated message");
        };
        let AgentMessage::Assistant(message) = *message else {
            panic!("expected assistant message");
        };
        assert_eq!(message.usage.output, 7);
        assert!(matches!(
            &message.content[0],
            AssistantContentBlock::Text { text, .. } if text == "Hello world"
        ));
        assert!(matches!(
            &message.content[1],
            AssistantContentBlock::Thinking { thinking, .. } if thinking == "Checked carefully"
        ));
    }

    #[test]
    fn tool_call_end_and_terminal_events_remain_authoritative() {
        let mut assembler = AssistantStreamAssembler::default();
        let mut start = decode(json!({
            "type": "message_start",
            "message": assistant(json!([]), 0)
        }));
        assert!(assembler.prepare(&mut start));

        let mut tool = decode(json!({
            "type": "message_update",
            "usage": usage(1),
            "assistantMessageEvent": {
                "type": "toolcall_end",
                "contentIndex": 0,
                "toolCall": {
                    "type": "toolCall",
                    "id": "tool-1",
                    "name": "read",
                    "arguments": {"path":"README.md"}
                }
            }
        }));
        assert!(assembler.prepare(&mut tool));
        assert!(matches!(
            tool,
            RpcEvent::MessageUpdate {
                message: Some(message),
                ..
            } if matches!(
                message.as_ref(),
                AgentMessage::Assistant(AssistantMessage { content, .. })
                    if matches!(&content[0], AssistantContentBlock::ToolCall { name, .. } if name == "read")
            )
        ));

        let terminal = assistant(json!([{"type":"text","text":"final"}]), 3);
        let mut done = decode(json!({
            "type": "message_update",
            "usage": usage(3),
            "assistantMessageEvent": {
                "type": "done",
                "reason": "stop",
                "message": terminal
            }
        }));
        assert!(assembler.prepare(&mut done));
        assert!(matches!(
            done,
            RpcEvent::MessageUpdate {
                message: Some(message),
                ..
            } if matches!(
                message.as_ref(),
                AgentMessage::Assistant(AssistantMessage { content, .. })
                    if matches!(&content[0], AssistantContentBlock::Text { text, .. } if text == "final")
            )
        ));
    }
}
