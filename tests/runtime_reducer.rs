use std::time::Instant;

use pi_gui::services::rpc::{
    Command, ConnectionGeneration, EntryId, ExtensionError, ExtensionErrorRecordType,
    IncomingRecord, RequestId, RpcClientError, RpcClientErrorKind, RpcDispatch, SessionId,
    TaggedIncomingRecord, ToolCallId, decode_record, dispatch_for_effect, normalize_call_result,
    normalize_tagged_record,
};
use pi_gui::state::reducer::reduce;
use pi_gui::state::runtime::*;
use serde_json::json;

const GENERATION: ConnectionGeneration = ConnectionGeneration::new(1);
fn stamp(state: &RuntimeState, input: RuntimeInput) -> StampedInput {
    StampedInput {
        generation: state.generation,
        epoch: state.epoch,
        observed_at: Instant::now(),
        input,
    }
}

fn apply(state: &mut RuntimeState, input: RuntimeInput) -> Vec<RuntimeEffect> {
    reduce(state, stamp(state, input))
}

fn response(
    state: &mut RuntimeState,
    request: RuntimeRequest,
    result: Result<NormalizedResponse, RequestFailure>,
) -> Vec<RuntimeEffect> {
    apply(state, RuntimeInput::Response { request, result })
}

fn failure(kind: RequestFailureKind, summary: &str) -> RequestFailure {
    RequestFailure {
        kind,
        error: SafeError::new(
            if kind == RequestFailureKind::UnknownOutcome {
                ErrorKind::UnknownOutcome
            } else {
                ErrorKind::Rejected
            },
            summary,
        ),
    }
}

fn session_state(id: &str, streaming: bool, pending: u64) -> NormalizedSessionState {
    NormalizedSessionState {
        session: SessionSnapshot {
            id: SessionId::from(id),
            file: Some(format!("{id}.jsonl")),
            name: Some(format!("Session {id}")),
            model: Some(ModelSummary {
                provider: "test".to_owned(),
                id: "model".to_owned(),
                name: "Synthetic model".to_owned(),
                reasoning: true,
                context_window: 100_000,
                max_tokens: 4_096,
                supports_images: false,
            }),
            thinking_level: RuntimeThinkingLevel::Medium,
            steering_mode: QueueDeliveryMode::OneAtATime,
            follow_up_mode: QueueDeliveryMode::All,
            auto_compaction_enabled: true,
            message_count: 2,
        },
        is_streaming: streaming,
        is_compacting: false,
        pending_message_count: pending,
    }
}

fn message(key: &str, text: &str, terminal: bool) -> RuntimeMessage {
    RuntimeMessage {
        key: MessageKey(key.to_owned()),
        role: MessageRole::Assistant,
        timestamp: 42,
        content: vec![MessageBlock::Text {
            key: BlockKey("text:0".to_owned()),
            text: text.to_owned(),
        }],
        visible: true,
        terminal,
        stop_reason: Some(MessageStopReason::Stop),
        error: None,
        assistant: None,
    }
}

fn user_message(key: &str, text: &str) -> RuntimeMessage {
    RuntimeMessage {
        key: MessageKey(key.to_owned()),
        role: MessageRole::User,
        timestamp: 43,
        content: vec![MessageBlock::Text {
            key: BlockKey("text:0".to_owned()),
            text: text.to_owned(),
        }],
        visible: true,
        terminal: true,
        stop_reason: None,
        error: None,
        assistant: None,
    }
}

fn entry(id: &str) -> RuntimeEntry {
    RuntimeEntry {
        id: EntryId::from(id),
        parent_id: None,
        timestamp: "2026-01-01T00:00:00Z".to_owned(),
        kind: EntryKind::SessionInfo { name: None },
    }
}

fn connected_state(id: &str) -> (RuntimeState, Vec<RuntimeEffect>) {
    let mut state = RuntimeState::new(GENERATION);
    let effects = apply(&mut state, RuntimeInput::Connected { recovery: false });
    assert_eq!(
        effects
            .iter()
            .map(|effect| &effect.effect)
            .collect::<Vec<_>>(),
        vec![&EffectKind::Request(RuntimeRequest::GetState)]
    );
    let hydration = response(
        &mut state,
        RuntimeRequest::GetState,
        Ok(NormalizedResponse::State(session_state(id, false, 0))),
    );
    (state, hydration)
}

#[test]
fn happy_path_hydrates_in_required_order_and_settles() {
    let mut state = RuntimeState::new(GENERATION);
    let first = apply(&mut state, RuntimeInput::Connected { recovery: false });
    assert!(matches!(
        first[0].effect,
        EffectKind::Request(RuntimeRequest::GetState)
    ));

    let hydration = response(
        &mut state,
        RuntimeRequest::GetState,
        Ok(NormalizedResponse::State(session_state("s1", true, 3))),
    );
    let requests = hydration
        .iter()
        .map(|effect| match &effect.effect {
            EffectKind::Request(request) => request,
            EffectKind::ExtensionUiResponse { .. } => panic!("unexpected extension response"),
        })
        .collect::<Vec<_>>();
    assert!(matches!(requests[0], RuntimeRequest::GetMessages { .. }));
    assert!(matches!(
        requests[1],
        RuntimeRequest::GetEntries { since: None, .. }
    ));
    assert!(matches!(requests[2], RuntimeRequest::GetStats));
    assert!(matches!(requests[3], RuntimeRequest::GetCommands));
    assert!(matches!(requests[4], RuntimeRequest::GetModels));
    assert!(matches!(requests[5], RuntimeRequest::GetTree { .. }));
    assert_eq!(state.lifecycle, RuntimeLifecycle::Running);
    assert_eq!(state.queue, QueueContents::Unknown { pending_count: 3 });

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::QueueUpdate {
            steering: Vec::new(),
            follow_up: Vec::new(),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::AgentSettled),
    );
    assert_eq!(
        state.queue,
        QueueContents::Known {
            steering: Vec::new(),
            follow_up: Vec::new()
        }
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Settled);
}

#[test]
fn agent_end_is_only_a_low_level_boundary() {
    let (mut state, _) = connected_state("s1");
    apply(&mut state, RuntimeInput::Event(NormalizedEvent::AgentStart));
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::AgentEnd {
            will_retry: false,
            messages: vec![message("a", "done", true)],
        }),
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Running);
    assert!(state.low_level_agent_end_seen);

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::AgentSettled),
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Settled);
    assert!(!state.low_level_agent_end_seen);
}

#[test]
fn retry_success_and_final_failure_wait_for_settlement() {
    let (mut state, _) = connected_state("s1");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::RetryStart {
            attempt: 1,
            max_attempts: 3,
            delay_ms: 100,
        }),
    );
    assert!(matches!(
        state.retry,
        RetryState::Waiting { attempt: 1, .. }
    ));
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::RetryEnd {
            success: true,
            attempt: 1,
            final_error: None,
        }),
    );
    assert_eq!(state.retry, RetryState::Succeeded { attempt: 1 });
    assert_eq!(state.lifecycle, RuntimeLifecycle::Running);

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::RetryStart {
            attempt: 3,
            max_attempts: 3,
            delay_ms: 400,
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::RetryEnd {
            success: false,
            attempt: 3,
            final_error: Some("Safe retry failure".to_owned()),
        }),
    );
    assert!(matches!(state.retry, RetryState::Failed { attempt: 3, .. }));
    assert_ne!(state.lifecycle, RuntimeLifecycle::Settled);
}

#[test]
fn overflow_compaction_records_automatic_retry_without_idling() {
    let (mut state, _) = connected_state("s1");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::CompactionStart {
            reason: CompactionKind::Overflow,
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::CompactionEnd {
            reason: CompactionKind::Overflow,
            summary: Some("Synthetic summary".to_owned()),
            aborted: false,
            will_retry: true,
            error: None,
        }),
    );
    assert!(matches!(
        state.compaction,
        CompactionState::Completed {
            reason: CompactionKind::Overflow,
            will_retry: true,
            ..
        }
    ));
    assert_eq!(state.lifecycle, RuntimeLifecycle::Running);
}

#[test]
fn queued_continuation_does_not_settle_between_agent_runs() {
    let (mut state, _) = connected_state("s1");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::QueueUpdate {
            steering: Vec::new(),
            follow_up: vec!["Continue".to_owned()],
        }),
    );
    apply(&mut state, RuntimeInput::Event(NormalizedEvent::AgentStart));
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::AgentEnd {
            will_retry: false,
            messages: Vec::new(),
        }),
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Running);
    apply(&mut state, RuntimeInput::Event(NormalizedEvent::AgentStart));
    assert_eq!(state.lifecycle, RuntimeLifecycle::Running);
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::AgentSettled),
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Settled);
}

#[test]
fn abort_and_abort_retry_stay_cancelling_until_agent_settled() {
    let (mut state, _) = connected_state("s1");
    let effects = apply(&mut state, RuntimeInput::Intent(RuntimeIntent::Abort));
    assert_eq!(state.lifecycle, RuntimeLifecycle::Cancelling);
    assert!(matches!(
        effects[0].effect,
        EffectKind::Request(RuntimeRequest::Abort)
    ));
    response(
        &mut state,
        RuntimeRequest::Abort,
        Ok(NormalizedResponse::Accepted),
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Cancelling);
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::AgentSettled),
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Settled);

    apply(&mut state, RuntimeInput::Intent(RuntimeIntent::AbortRetry));
    assert_eq!(state.retry, RetryState::Cancelling);
    response(
        &mut state,
        RuntimeRequest::AbortRetry,
        Ok(NormalizedResponse::Accepted),
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Cancelling);
}

#[test]
fn parallel_tools_interleave_and_updates_replace_accumulated_results() {
    let (mut state, _) = connected_state("s1");
    let a = ToolCallId::from("a");
    let b = ToolCallId::from("b");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolUpdate {
            id: b.clone(),
            name: "bash".to_owned(),
            arguments: json!({"command": "b"}),
            accumulated: json!({"text": "first"}),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolStart {
            id: a.clone(),
            name: "read".to_owned(),
            arguments: json!({"path": "a"}),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolUpdate {
            id: b.clone(),
            name: "bash".to_owned(),
            arguments: json!({"command": "b"}),
            accumulated: json!({"text": "first second"}),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolEnd {
            id: a.clone(),
            name: "read".to_owned(),
            result: json!({"text": "done"}),
            is_error: false,
            cancelled: false,
        }),
    );
    assert_eq!(state.tools.len(), 2);
    assert_eq!(
        state.tools[&b].result,
        Some(json!({"text": "first second"}))
    );
    assert_eq!(state.tools[&a].status, ToolStatus::Succeeded);

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolEnd {
            id: b.clone(),
            name: "bash".to_owned(),
            result: json!({"cancelled": true}),
            is_error: true,
            cancelled: true,
        }),
    );
    assert_eq!(state.tools[&b].status, ToolStatus::Cancelled);

    let c = ToolCallId::from("c");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolEnd {
            id: c.clone(),
            name: "write".to_owned(),
            result: json!({"message": "failed"}),
            is_error: true,
            cancelled: false,
        }),
    );
    assert_eq!(state.tools[&c].status, ToolStatus::Failed);
}

#[test]
fn late_tool_starts_and_updates_cannot_erase_newer_or_terminal_state() {
    let (mut state, _) = connected_state("s1");
    let id = ToolCallId::from("ordered");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolUpdate {
            id: id.clone(),
            name: "custom".to_owned(),
            arguments: json!("{malformed-json"),
            accumulated: json!({"text": "accumulated"}),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolStart {
            id: id.clone(),
            name: "custom".to_owned(),
            arguments: json!("{malformed-json"),
        }),
    );
    assert_eq!(
        state.tools[&id].result,
        Some(json!({"text": "accumulated"}))
    );

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolEnd {
            id: id.clone(),
            name: "custom".to_owned(),
            result: json!({"text": "final"}),
            is_error: false,
            cancelled: false,
        }),
    );
    let terminal = state.tools[&id].clone();
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolStart {
            id: id.clone(),
            name: "late".to_owned(),
            arguments: json!({"late": true}),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolUpdate {
            id: id.clone(),
            name: "late".to_owned(),
            arguments: json!({"late": true}),
            accumulated: json!({"text": "stale"}),
        }),
    );
    assert_eq!(state.tools[&id], terminal);
}

#[test]
fn authoritative_tool_end_can_reconcile_a_settlement_cancellation() {
    let (mut state, _) = connected_state("s1");
    let id = ToolCallId::from("settled-before-end");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolStart {
            id: id.clone(),
            name: "read".to_owned(),
            arguments: json!({"path": "fixture"}),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::AgentSettled),
    );
    assert_eq!(state.tools[&id].status, ToolStatus::Cancelled);
    assert!(!state.tools[&id].authoritative_end);

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolEnd {
            id: id.clone(),
            name: "read".to_owned(),
            result: json!({"text": "late but authoritative"}),
            is_error: false,
            cancelled: false,
        }),
    );
    assert_eq!(state.tools[&id].status, ToolStatus::Succeeded);
    assert!(state.tools[&id].authoritative_end);
}

#[test]
fn direct_bash_records_results_reconciles_and_cancels_separately() {
    fn bash_message(key: &str) -> RuntimeMessage {
        RuntimeMessage {
            key: MessageKey(key.to_owned()),
            role: MessageRole::BashExecution,
            timestamp: 50,
            content: vec![MessageBlock::Bash {
                key: BlockKey("bash:0".to_owned()),
                command: "printf ok".to_owned(),
                output: "ok".to_owned(),
                exit_code: Some(0),
                cancelled: false,
                truncated: true,
                full_output_path: Some("C:/tmp/full.txt".to_owned()),
                exclude_from_context: true,
            }],
            visible: true,
            terminal: true,
            stop_reason: None,
            error: None,
            assistant: None,
        }
    }

    let (mut state, _) = connected_state("s1");
    state.messages.ready(vec![bash_message("bash:historical")]);
    let request = RequestId::from("bash-1");
    let effects = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::ExecuteBash {
            request: request.clone(),
            command: "printf ok".to_owned(),
            exclude_from_context: true,
        }),
    );
    assert!(matches!(
        effects[0].effect,
        EffectKind::Request(RuntimeRequest::ExecuteBash {
            exclude_from_context: true,
            ..
        })
    ));
    assert!(matches!(
        dispatch_for_effect(&effects[0]),
        RpcDispatch::Command(Command::Bash {
            command,
            exclude_from_context: Some(true),
        }) if command == "printf ok"
    ));
    assert_eq!(state.bash_executions[0].status, BashStatus::Running);

    let abort = apply(&mut state, RuntimeInput::Intent(RuntimeIntent::AbortBash));
    assert!(matches!(
        abort[0].effect,
        EffectKind::Request(RuntimeRequest::AbortBash)
    ));
    assert_eq!(state.bash_executions[0].status, BashStatus::Cancelling);
    assert_ne!(state.lifecycle, RuntimeLifecycle::Cancelling);

    let refresh = response(
        &mut state,
        RuntimeRequest::ExecuteBash {
            request,
            command: "printf ok".to_owned(),
            exclude_from_context: true,
        },
        Ok(NormalizedResponse::Bash(DirectBashResult {
            output: "ok".to_owned(),
            exit_code: Some(0),
            cancelled: false,
            truncated: true,
            full_output_path: Some("C:/tmp/full.txt".to_owned()),
        })),
    );
    assert_eq!(state.bash_executions[0].status, BashStatus::Succeeded);
    assert!(state.bash_executions[0].truncated);
    assert!(matches!(
        refresh[0].effect,
        EffectKind::Request(RuntimeRequest::GetMessages { .. })
    ));

    response(
        &mut state,
        RuntimeRequest::GetMessages { base_revision: 0 },
        Ok(NormalizedResponse::Messages(vec![bash_message(
            "bash:historical",
        )])),
    );
    assert!(!state.bash_executions[0].reconciled);

    response(
        &mut state,
        RuntimeRequest::GetMessages { base_revision: 0 },
        Ok(NormalizedResponse::Messages(vec![
            bash_message("bash:historical"),
            bash_message("bash:persisted"),
        ])),
    );
    assert!(state.bash_executions[0].reconciled);
}

#[test]
fn streaming_uses_accumulated_replacement_and_terminal_events_are_idempotent() {
    let (mut state, _) = connected_state("s1");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageStart(message("a", "", false))),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageUpdate(message("a", "Hello", false))),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageUpdate(message(
            "a",
            "Hello world",
            false,
        ))),
    );
    let terminal = message("a", "Hello world", true);
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageEnd(terminal.clone())),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageEnd(terminal)),
    );
    let messages = state.messages.data.as_ref().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].content,
        vec![MessageBlock::Text {
            key: BlockKey("text:0".to_owned()),
            text: "Hello world".to_owned(),
        }]
    );
    assert!(messages[0].terminal);
}

#[test]
fn streaming_start_cannot_erase_a_newer_partial_or_terminal_message() {
    let (mut state, _) = connected_state("s1");
    let partial = message("ordered", "partial text", false);
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageUpdate(partial.clone())),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageStart(message("ordered", "", false))),
    );
    assert_eq!(state.messages.data.as_ref().unwrap()[0], partial);

    let terminal = message("ordered", "final text", true);
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageEnd(terminal.clone())),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageUpdate(message(
            "ordered",
            "late partial",
            false,
        ))),
    );
    assert_eq!(state.messages.data.as_ref().unwrap()[0], terminal);
}

#[test]
fn partial_messages_replace_text_and_thinking_blocks_in_place() {
    let (mut state, _) = connected_state("s1");
    let mut partial = message("blocks", "Hello", false);
    partial.content.push(MessageBlock::Thinking {
        key: BlockKey("thinking:1".to_owned()),
        text: "Checking".to_owned(),
        redacted: false,
    });
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageStart(message("blocks", "", false))),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageUpdate(partial.clone())),
    );

    let mut accumulated = message("blocks", "Hello world", false);
    accumulated.content.push(MessageBlock::Thinking {
        key: BlockKey("thinking:1".to_owned()),
        text: "Checking the complete answer".to_owned(),
        redacted: false,
    });
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageUpdate(accumulated.clone())),
    );
    assert_eq!(state.messages.data.as_ref().unwrap(), &vec![accumulated]);
}

#[test]
fn adapter_preserves_safe_metadata_and_hidden_custom_visibility() {
    let assistant = json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "safe thought", "redacted": false},
                {"type": "text", "text": "safe answer"}
            ],
            "api": "synthetic-api",
            "provider": "synthetic-provider",
            "model": "synthetic-model",
            "responseModel": "effective-model",
            "responseId": "private-response-id",
            "diagnostics": [{
                "type": "provider-warning",
                "timestamp": 123,
                "details": {"secret": "never-store-this"}
            }],
            "usage": {
                "input": 12,
                "output": 7,
                "cacheRead": 3,
                "cacheWrite": 2,
                "cacheWrite1h": 1,
                "reasoning": 4,
                "totalTokens": 24,
                "cost": {
                    "input": 0.01,
                    "output": 0.02,
                    "cacheRead": 0.003,
                    "cacheWrite": 0.004,
                    "total": 0.037
                }
            },
            "stopReason": "length",
            "errorMessage": "private provider payload",
            "timestamp": 1733234567890u64
        }
    });
    let record = decode_record(assistant.to_string().as_bytes()).expect("assistant record");
    let input = normalize_tagged_record(
        TaggedIncomingRecord {
            generation: GENERATION,
            record,
        },
        pi_gui::services::rpc::SessionEpoch::default(),
    )
    .expect("normalized assistant");
    let RuntimeInput::Event(NormalizedEvent::MessageEnd(message)) = input.input else {
        panic!("expected message end");
    };
    let metadata = message.assistant.as_ref().expect("assistant metadata");
    assert_eq!(metadata.provider, "synthetic-provider");
    assert_eq!(metadata.model, "synthetic-model");
    assert_eq!(metadata.response_model.as_deref(), Some("effective-model"));
    assert_eq!(metadata.usage.total_tokens, 24);
    assert_eq!(metadata.usage.reasoning, Some(4));
    assert_eq!(message.timestamp, 1_733_234_567_890);
    assert_eq!(message.stop_reason, Some(MessageStopReason::Length));
    assert_eq!(message.error.as_deref(), Some("Assistant request failed"));
    let debug = format!("{message:?}");
    assert!(!debug.contains("never-store-this"));
    assert!(!debug.contains("private provider payload"));
    assert!(!debug.contains("private-response-id"));

    let hidden = json!({
        "type": "message_start",
        "message": {
            "role": "custom",
            "customType": "synthetic-hidden",
            "content": "hidden context",
            "display": false,
            "details": {"secret": "not-a-diagnostic"},
            "timestamp": 99
        }
    });
    let record = decode_record(hidden.to_string().as_bytes()).expect("custom record");
    let input = normalize_tagged_record(
        TaggedIncomingRecord {
            generation: GENERATION,
            record,
        },
        pi_gui::services::rpc::SessionEpoch::default(),
    )
    .expect("normalized custom");
    let RuntimeInput::Event(NormalizedEvent::MessageStart(message)) = input.input else {
        panic!("expected custom start");
    };
    assert_eq!(message.role, MessageRole::Custom);
    assert!(!message.visible);
}

#[test]
fn synthesized_message_keys_do_not_use_vector_position_or_collapse_equal_timestamps() {
    let mut keys = Vec::new();
    for text in ["first", "second"] {
        let event = json!({
            "type": "message_start",
            "message": {"role": "user", "content": text, "timestamp": 77}
        });
        let record = decode_record(event.to_string().as_bytes()).expect("user record");
        let input = normalize_tagged_record(
            TaggedIncomingRecord {
                generation: GENERATION,
                record,
            },
            pi_gui::services::rpc::SessionEpoch::default(),
        )
        .expect("normalized user");
        let RuntimeInput::Event(NormalizedEvent::MessageStart(message)) = input.input else {
            panic!("expected user start");
        };
        keys.push(message.key);
    }
    assert_ne!(keys[0], keys[1]);
    assert!(keys[0].0.starts_with("user:77:"));
}

#[test]
fn extension_dialogs_errors_statuses_and_widgets_have_replacement_semantics() {
    let (mut state, _) = connected_state("s1");
    let id = RequestId::from("dialog");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::Dialog {
            id: id.clone(),
            request: DialogRequest::Confirm {
                title: "Confirm".to_owned(),
                message: "Proceed?".to_owned(),
            },
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::SetStatus {
            key: "ext".to_owned(),
            value: Some(ExtensionStatus {
                text: "Running".to_owned(),
            }),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::SetWidget {
            key: "ext".to_owned(),
            value: Some(ExtensionWidget {
                lines: vec!["line".to_owned()],
                placement: WidgetPlacement::AboveEditor,
            }),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ExtensionError(ExtensionFailure {
            extension: "synthetic.ts".to_owned(),
            event: "tool_call".to_owned(),
            summary: "Extension execution failed".to_owned(),
        })),
    );
    assert!(state.dialogs.contains_key(&id));
    assert_eq!(state.statuses["ext"].text, "Running");
    assert_eq!(state.extension_errors.len(), 1);

    let effects = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::AnswerDialog {
            request: id.clone(),
            answer: DialogAnswer::Confirmed(true),
        }),
    );
    assert!(!state.dialogs.contains_key(&id));
    assert!(matches!(
        effects[0].effect,
        EffectKind::ExtensionUiResponse { .. }
    ));
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::SetStatus {
            key: "ext".to_owned(),
            value: None,
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::SetWidget {
            key: "ext".to_owned(),
            value: None,
        }),
    );
    assert!(state.statuses.is_empty());
    assert!(state.widgets.is_empty());
}

#[test]
fn prompt_delivery_tracks_acceptance_rejection_and_unknown_outcome() {
    let (mut state, _) = connected_state("s1");
    let accepted = RequestId::from("accepted");
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: accepted.clone(),
            text: "Accepted".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    assert_eq!(
        state.prompt_delivery,
        PromptDelivery::Pending {
            request: accepted.clone(),
            kind: SubmissionKind::Prompt,
        }
    );
    response(
        &mut state,
        RuntimeRequest::Submit {
            request: accepted.clone(),
            text: "Accepted".to_owned(),
            kind: SubmissionKind::Prompt,
        },
        Ok(NormalizedResponse::Accepted),
    );
    assert_eq!(
        state.prompt_delivery,
        PromptDelivery::Accepted {
            request: accepted,
            kind: SubmissionKind::Prompt,
        }
    );

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::AgentSettled),
    );
    let rejected = RequestId::from("rejected");
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: rejected.clone(),
            text: "Rejected".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    let effects = response(
        &mut state,
        RuntimeRequest::Submit {
            request: rejected.clone(),
            text: "Rejected".to_owned(),
            kind: SubmissionKind::Prompt,
        },
        Err(failure(RequestFailureKind::Rejected, "Prompt rejected")),
    );
    assert!(effects.is_empty());
    assert!(matches!(
        state.prompt_delivery,
        PromptDelivery::Rejected { request, .. } if request == rejected
    ));
}

#[test]
fn prompt_acceptance_respects_event_before_response_ordering() {
    let (mut state, _) = connected_state("s1");
    let request = RequestId::from("fast-prompt");
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: request.clone(),
            text: "Fast".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    apply(&mut state, RuntimeInput::Event(NormalizedEvent::AgentStart));
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::AgentSettled),
    );
    response(
        &mut state,
        RuntimeRequest::Submit {
            request,
            text: "Fast".to_owned(),
            kind: SubmissionKind::Prompt,
        },
        Ok(NormalizedResponse::Accepted),
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Settled);
    assert!(matches!(
        state.prompt_delivery,
        PromptDelivery::Accepted {
            kind: SubmissionKind::Prompt,
            ..
        }
    ));
}

#[test]
fn accepted_user_input_is_optimistic_only_until_authoritative_message_arrives() {
    let (mut state, _) = connected_state("s1");
    let request = RequestId::from("optimistic");
    let submit = RuntimeRequest::Submit {
        request: request.clone(),
        text: "Show this once".to_owned(),
        kind: SubmissionKind::Prompt,
    };
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: request.clone(),
            text: "Show this once".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    response(&mut state, submit, Ok(NormalizedResponse::Accepted));
    assert_eq!(state.optimistic_user_inputs.len(), 1);
    assert!(state.optimistic_user_inputs[0].accepted);

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageStart(user_message(
            "authoritative-user",
            "Show this once",
        ))),
    );
    assert!(state.optimistic_user_inputs.is_empty());
    assert_eq!(
        state
            .messages
            .data
            .as_ref()
            .unwrap()
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .count(),
        1
    );
}

#[test]
fn authoritative_user_event_before_acceptance_never_creates_a_duplicate() {
    let (mut state, _) = connected_state("s1");
    let request = RequestId::from("event-first");
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: request.clone(),
            text: "Fast authoritative input".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageStart(user_message(
            "fast-authoritative",
            "Fast authoritative input",
        ))),
    );
    assert!(state.optimistic_user_inputs[0].authoritative_seen);
    response(
        &mut state,
        RuntimeRequest::Submit {
            request,
            text: "Fast authoritative input".to_owned(),
            kind: SubmissionKind::Prompt,
        },
        Ok(NormalizedResponse::Accepted),
    );
    assert!(state.optimistic_user_inputs.is_empty());
}

#[test]
fn all_session_replacements_increment_epoch_before_emission_and_rehydrate() {
    let mutations = [
        SessionMutation::New {
            parent_session: None,
        },
        SessionMutation::Switch {
            session_path: "other.jsonl".to_owned(),
        },
        SessionMutation::Fork {
            entry_id: EntryId::from("fork-point"),
        },
        SessionMutation::Clone,
    ];

    for mutation in mutations {
        let (mut state, _) = connected_state("s1");
        let old_epoch = state.epoch;
        state.dialogs.insert(
            RequestId::from("old-dialog"),
            DialogRequest::Input {
                title: "Old".to_owned(),
                placeholder: None,
            },
        );
        let effects = apply(
            &mut state,
            RuntimeInput::Intent(RuntimeIntent::ReplaceSession(mutation.clone())),
        );
        assert_eq!(state.epoch, old_epoch.next());
        assert_eq!(effects[0].epoch, state.epoch);
        assert!(state.dialogs.is_empty());
        assert!(matches!(
            effects[0].effect,
            EffectKind::Request(RuntimeRequest::SessionMutation(_))
        ));

        let hydration = response(
            &mut state,
            RuntimeRequest::SessionMutation(mutation),
            Ok(NormalizedResponse::SessionMutation { cancelled: false }),
        );
        assert!(matches!(
            hydration[0].effect,
            EffectKind::Request(RuntimeRequest::GetState)
        ));
    }
}

#[test]
fn cancelled_or_rejected_session_replacement_resyncs_new_epoch() {
    let (mut state, _) = connected_state("s1");
    state
        .messages
        .ready(vec![message("old", "preserved", true)]);
    let display_epoch = state.display_epoch;
    let mutation = SessionMutation::Clone;
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::ReplaceSession(mutation.clone())),
    );
    let effects = response(
        &mut state,
        RuntimeRequest::SessionMutation(mutation.clone()),
        Ok(NormalizedResponse::SessionMutation { cancelled: true }),
    );
    assert!(matches!(
        effects[0].effect,
        EffectKind::Request(RuntimeRequest::GetState)
    ));
    assert_eq!(state.display_epoch, display_epoch);
    assert_eq!(
        state.messages.data.as_ref().unwrap()[0].key,
        MessageKey("old".to_owned())
    );

    let effects = response(
        &mut state,
        RuntimeRequest::SessionMutation(mutation),
        Err(failure(RequestFailureKind::Rejected, "cancelled")),
    );
    assert!(matches!(
        effects[0].effect,
        EffectKind::Request(RuntimeRequest::GetState)
    ));
}

#[test]
fn session_rename_and_export_use_serialized_runtime_operations() {
    let (mut state, _) = connected_state("s1");
    let rename = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::SetSessionName {
            name: "  Build audit  ".to_owned(),
        }),
    );
    assert!(matches!(
        dispatch_for_effect(&rename[0]),
        RpcDispatch::Command(Command::SetSessionName { ref name }) if name == "Build audit"
    ));
    assert!(matches!(
        state.pending_operation,
        Some(RuntimeOperation::SetSessionName(ref name)) if name == "Build audit"
    ));
    response(
        &mut state,
        RuntimeRequest::SetSessionName {
            name: "Build audit".to_owned(),
        },
        Ok(NormalizedResponse::Accepted),
    );
    assert_eq!(
        state.session.data.as_ref().unwrap().name.as_deref(),
        Some("Build audit")
    );

    let export = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::ExportHtml { output_path: None }),
    );
    assert!(matches!(
        dispatch_for_effect(&export[0]),
        RpcDispatch::Command(Command::ExportHtml { output_path: None })
    ));
    response(
        &mut state,
        RuntimeRequest::ExportHtml { output_path: None },
        Ok(NormalizedResponse::Exported {
            path: "session.html".to_owned(),
        }),
    );
    assert!(state.pending_operation.is_none());
    assert_eq!(
        state.notifications.back().unwrap().message,
        "Session exported to session.html"
    );
}

#[test]
fn changed_session_performs_full_rebuild_and_advances_epoch() {
    let (mut state, _) = connected_state("s1");
    state.messages.ready(vec![message("old", "old", true)]);
    state.entries.ready(vec![entry("old")]);
    state.durable_cursor = Some(EntryId::from("old"));
    state.cursor_session_id = Some(SessionId::from("s1"));
    let epoch = state.epoch;

    let effects = response(
        &mut state,
        RuntimeRequest::GetState,
        Ok(NormalizedResponse::State(session_state("s2", false, 0))),
    );
    assert_eq!(state.epoch, epoch.next());
    assert!(state.messages.data.is_none());
    assert!(state.entries.data.is_none());
    assert!(state.durable_cursor.is_none());
    assert!(effects.iter().any(|effect| matches!(
        effect.effect,
        EffectKind::Request(RuntimeRequest::GetEntries { since: None, .. })
    )));
}

#[test]
fn session_replacement_suppresses_old_events_and_rebuilds_even_with_same_session_id() {
    let (mut state, _) = connected_state("s1");
    state
        .messages
        .ready(vec![message("old", "old transcript", true)]);
    let ignored_before = state.stale_inputs_ignored;

    let mutation = SessionMutation::Clone;
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::ReplaceSession(mutation.clone())),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageEnd(message(
            "old-late",
            "late old session content",
            true,
        ))),
    );
    assert_eq!(state.stale_inputs_ignored, ignored_before + 1);
    assert_eq!(state.messages.data.as_ref().unwrap().len(), 1);

    response(
        &mut state,
        RuntimeRequest::SessionMutation(mutation),
        Ok(NormalizedResponse::SessionMutation { cancelled: false }),
    );
    response(
        &mut state,
        RuntimeRequest::GetState,
        Ok(NormalizedResponse::State(session_state("s1", false, 0))),
    );
    assert!(!state.replacement_awaiting_state);
    assert!(state.messages.data.is_none());
    assert!(state.optimistic_user_inputs.is_empty());
}

#[test]
fn reconnect_hydration_replaces_stale_partials_but_keeps_new_generation_events() {
    let (mut state, _) = connected_state("s1");
    state
        .messages
        .ready(vec![message("stale", "old partial", false)]);
    let epoch = state.epoch;
    reduce(
        &mut state,
        StampedInput {
            generation: ConnectionGeneration::new(2),
            epoch,
            observed_at: Instant::now(),
            input: RuntimeInput::Connected { recovery: true },
        },
    );
    let hydration = response(
        &mut state,
        RuntimeRequest::GetState,
        Ok(NormalizedResponse::State(session_state("s1", true, 0))),
    );
    let messages_request = hydration
        .iter()
        .find_map(|effect| match &effect.effect {
            EffectKind::Request(request @ RuntimeRequest::GetMessages { .. }) => {
                Some(request.clone())
            }
            _ => None,
        })
        .expect("messages request");

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageUpdate(message(
            "live",
            "new generation partial",
            false,
        ))),
    );
    response(
        &mut state,
        messages_request,
        Ok(NormalizedResponse::Messages(vec![message(
            "hydrated",
            "authoritative history",
            true,
        )])),
    );

    let keys = state
        .messages
        .data
        .as_ref()
        .unwrap()
        .iter()
        .map(|message| message.key.0.as_str())
        .collect::<Vec<_>>();
    assert_eq!(keys, vec!["hydrated", "live"]);
    assert!(!keys.contains(&"stale"));
}

#[test]
fn reconnect_hydration_reconciles_accepted_input_against_authoritative_history() {
    let (mut state, _) = connected_state("s1");
    let request = RequestId::from("accepted-before-reconnect");
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: request.clone(),
            text: "Persisted while reconnecting".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    response(
        &mut state,
        RuntimeRequest::Submit {
            request,
            text: "Persisted while reconnecting".to_owned(),
            kind: SubmissionKind::Prompt,
        },
        Ok(NormalizedResponse::Accepted),
    );
    assert_eq!(state.optimistic_user_inputs.len(), 1);

    let epoch = state.epoch;
    reduce(
        &mut state,
        StampedInput {
            generation: ConnectionGeneration::new(2),
            epoch,
            observed_at: Instant::now(),
            input: RuntimeInput::Connected { recovery: true },
        },
    );
    let hydration = response(
        &mut state,
        RuntimeRequest::GetState,
        Ok(NormalizedResponse::State(session_state("s1", false, 0))),
    );
    let messages_request = hydration
        .into_iter()
        .find_map(|effect| match effect.effect {
            EffectKind::Request(request @ RuntimeRequest::GetMessages { .. }) => Some(request),
            _ => None,
        })
        .expect("messages request");
    response(
        &mut state,
        messages_request,
        Ok(NormalizedResponse::Messages(vec![user_message(
            "persisted-user",
            "Persisted while reconnecting",
        )])),
    );
    assert!(state.optimistic_user_inputs.is_empty());
}

#[test]
fn invalid_incremental_cursor_falls_back_to_full_once() {
    let (mut state, _) = connected_state("s1");
    state.durable_cursor = Some(EntryId::from("cursor"));
    state.cursor_session_id = Some(SessionId::from("s1"));

    let reconnect = StampedInput {
        generation: ConnectionGeneration::new(2),
        epoch: state.epoch,
        observed_at: Instant::now(),
        input: RuntimeInput::Connected { recovery: true },
    };
    reduce(&mut state, reconnect);
    let hydration = response(
        &mut state,
        RuntimeRequest::GetState,
        Ok(NormalizedResponse::State(session_state("s1", false, 0))),
    );
    let incremental = hydration
        .iter()
        .find_map(|effect| match &effect.effect {
            EffectKind::Request(request @ RuntimeRequest::GetEntries { .. }) => {
                Some(request.clone())
            }
            _ => None,
        })
        .expect("entries request");
    assert!(matches!(
        incremental,
        RuntimeRequest::GetEntries { since: Some(_), .. }
    ));

    let fallback = response(
        &mut state,
        incremental,
        Err(failure(RequestFailureKind::InvalidCursor, "invalid cursor")),
    );
    let full = match &fallback[0].effect {
        EffectKind::Request(request @ RuntimeRequest::GetEntries { since: None, .. }) => {
            request.clone()
        }
        effect => panic!("unexpected fallback: {effect:?}"),
    };
    let repeated = response(
        &mut state,
        full,
        Err(failure(RequestFailureKind::InvalidCursor, "still invalid")),
    );
    assert!(repeated.is_empty());
    assert!(matches!(state.entries.status, FacetStatus::Failed(_)));
}

#[test]
fn unknown_session_outcome_is_not_retried_or_resynced_automatically() {
    let (mut state, _) = connected_state("s1");
    let mutation = SessionMutation::Switch {
        session_path: "unknown.jsonl".to_owned(),
    };
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::ReplaceSession(mutation.clone())),
    );
    let effects = response(
        &mut state,
        RuntimeRequest::SessionMutation(mutation),
        Err(failure(
            RequestFailureKind::UnknownOutcome,
            "Session outcome unknown",
        )),
    );
    assert!(effects.is_empty());
    assert_eq!(state.lifecycle, RuntimeLifecycle::Disconnected);
}

#[test]
fn stale_generation_and_epoch_inputs_have_no_side_effects() {
    let (mut state, _) = connected_state("s1");
    let before = state.lifecycle;
    let stale_generation = StampedInput {
        generation: ConnectionGeneration::new(0),
        epoch: state.epoch,
        observed_at: Instant::now(),
        input: RuntimeInput::Event(NormalizedEvent::AgentStart),
    };
    assert!(reduce(&mut state, stale_generation).is_empty());
    assert_eq!(state.lifecycle, before);

    let stale_epoch = StampedInput {
        generation: state.generation,
        epoch: state.epoch.next(),
        observed_at: Instant::now(),
        input: RuntimeInput::Event(NormalizedEvent::AgentStart),
    };
    assert!(reduce(&mut state, stale_epoch).is_empty());
    assert_eq!(state.lifecycle, before);
    assert_eq!(state.stale_inputs_ignored, 2);
}

#[test]
fn unknown_records_and_extension_failures_are_bounded_without_raw_payloads() {
    let (mut state, _) = connected_state("s1");
    for index in 0..(MAX_UNKNOWN_RECORDS + 5) {
        apply(
            &mut state,
            RuntimeInput::Event(NormalizedEvent::Unknown {
                record_type: format!("future-{index}"),
            }),
        );
    }
    for index in 0..(MAX_RUNTIME_ERRORS + 5) {
        apply(
            &mut state,
            RuntimeInput::Event(NormalizedEvent::ExtensionError(ExtensionFailure {
                extension: format!("extension-{index}"),
                event: "event".to_owned(),
                summary: "Extension execution failed".to_owned(),
            })),
        );
    }
    assert_eq!(state.unknown_records.len(), MAX_UNKNOWN_RECORDS);
    assert_eq!(state.extension_errors.len(), MAX_RUNTIME_ERRORS);
}

#[test]
fn long_streamed_content_is_not_truncated_or_split_into_duplicate_messages() {
    let (mut state, _) = connected_state("s1");
    let long = "0123456789abcdef\n".repeat(12_000);
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageUpdate(message(
            "long", &long, false,
        ))),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::MessageEnd(message("long", &long, true))),
    );
    let messages = state.messages.data.as_ref().unwrap();
    assert_eq!(messages.len(), 1);
    let MessageBlock::Text { text, .. } = &messages[0].content[0] else {
        panic!("expected text");
    };
    assert_eq!(text.len(), long.len());
    assert_eq!(text, &long);
}

#[test]
fn process_crash_preserves_last_valid_data_and_invalidates_dialogs() {
    let (mut state, _) = connected_state("s1");
    state.messages.ready(vec![message("a", "saved", true)]);
    state.dialogs.insert(
        RequestId::from("dialog"),
        DialogRequest::Input {
            title: "Input".to_owned(),
            placeholder: None,
        },
    );
    apply(
        &mut state,
        RuntimeInput::Disconnected {
            error: SafeError::new(ErrorKind::Process, "Pi exited unexpectedly"),
        },
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Disconnected);
    assert_eq!(state.messages.data.as_ref().map(Vec::len), Some(1));
    assert!(state.dialogs.is_empty());
}

#[test]
fn reconnect_uses_fresh_generation_and_only_hydrates() {
    let (mut state, _) = connected_state("s1");
    let epoch = state.epoch;
    let effects = reduce(
        &mut state,
        StampedInput {
            generation: ConnectionGeneration::new(2),
            epoch,
            observed_at: Instant::now(),
            input: RuntimeInput::Connected { recovery: true },
        },
    );
    assert_eq!(state.generation, ConnectionGeneration::new(2));
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        effects[0].effect,
        EffectKind::Request(RuntimeRequest::GetState)
    ));
}

#[test]
fn uncertain_prompt_is_never_resent_on_reconnect() {
    let (mut state, _) = connected_state("s1");
    let request = RequestId::from("prompt-1");
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: request.clone(),
            text: "Synthetic prompt".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Disconnected {
            error: SafeError::new(ErrorKind::Disconnected, "Connection closed"),
        },
    );
    assert_eq!(
        state.prompt_delivery,
        PromptDelivery::Uncertain {
            request,
            kind: SubmissionKind::Prompt,
        }
    );

    let epoch = state.epoch;
    let effects = reduce(
        &mut state,
        StampedInput {
            generation: ConnectionGeneration::new(2),
            epoch,
            observed_at: Instant::now(),
            input: RuntimeInput::Connected { recovery: true },
        },
    );
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        effects[0].effect,
        EffectKind::Request(RuntimeRequest::GetState)
    ));
}

#[test]
fn optional_stats_models_and_unknown_context_fail_honestly_without_data_loss() {
    let (mut state, _) = connected_state("s1");
    let stats = RuntimeStats {
        session_id: SessionId::from("s1"),
        user_messages: 1,
        assistant_messages: 1,
        tool_calls: 0,
        tool_results: 0,
        total_messages: 2,
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        total_tokens: 15,
        cost: 0.01,
        context_tokens: None,
        context_window: Some(100_000),
        context_percent: None,
    };
    state.stats.ready(stats.clone());
    state.models.ready(vec![ModelSummary {
        provider: "test".to_owned(),
        id: "m".to_owned(),
        name: "M".to_owned(),
        reasoning: false,
        context_window: 100_000,
        max_tokens: 4_096,
        supports_images: false,
    }]);

    response(
        &mut state,
        RuntimeRequest::GetStats,
        Err(failure(RequestFailureKind::Rejected, "stats unavailable")),
    );
    response(
        &mut state,
        RuntimeRequest::GetModels,
        Err(failure(RequestFailureKind::Rejected, "models unavailable")),
    );
    assert!(matches!(state.stats.status, FacetStatus::Failed(_)));
    assert_eq!(state.stats.data, Some(stats));
    assert!(matches!(state.models.status, FacetStatus::Failed(_)));
    assert_eq!(state.models.data.as_ref().map(Vec::len), Some(1));
}

#[test]
fn service_adapter_maps_effects_errors_and_safe_extension_records() {
    let (mut state, _) = connected_state("s1");
    let request = RequestId::from("prompt-adapter");
    let effects = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: request.clone(),
            text: "Synthetic prompt".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    assert!(matches!(
        dispatch_for_effect(&effects[0]),
        RpcDispatch::Command(Command::Prompt { .. })
    ));

    let normalized = normalize_call_result(
        &effects[0],
        Err(RpcClientError {
            kind: RpcClientErrorKind::UnknownOutcome,
            generation: effects[0].generation,
            operation: Some("prompt".to_owned()),
        }),
    )
    .expect("request result");
    reduce(&mut state, normalized);
    assert_eq!(
        state.prompt_delivery,
        PromptDelivery::Uncertain {
            request,
            kind: SubmissionKind::Prompt,
        }
    );

    let record = TaggedIncomingRecord {
        generation: state.generation,
        record: IncomingRecord::ExtensionError(ExtensionError {
            record_type: ExtensionErrorRecordType::ExtensionError,
            extension_path: r"C:\private\secret-extension.ts".to_owned(),
            event: "tool_call".to_owned(),
            error: "token=never-expose-this".to_owned(),
        }),
    };
    let normalized = normalize_tagged_record(record, state.epoch).expect("extension event");
    reduce(&mut state, normalized);
    let error = state.extension_errors.back().expect("safe extension error");
    assert_eq!(error.extension, "secret-extension.ts");
    assert!(!error.summary.contains("never-expose-this"));
    assert!(!error.summary.contains("private"));
}

#[test]
fn entries_deduplicate_hydration_incrementals_and_live_appends() {
    let (mut state, _) = connected_state("s1");
    response(
        &mut state,
        RuntimeRequest::GetEntries {
            since: None,
            base_revision: 0,
        },
        Ok(NormalizedResponse::Entries {
            entries: vec![entry("a"), entry("a"), entry("b")],
            leaf_id: Some(EntryId::from("b")),
        }),
    );
    assert_eq!(state.entries.data.as_ref().map(Vec::len), Some(2));
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::EntryAppended(entry("b"))),
    );
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::EntryAppended(entry("c"))),
    );
    assert_eq!(state.entries.data.as_ref().map(Vec::len), Some(3));
    assert_eq!(state.durable_cursor, Some(EntryId::from("c")));
}

#[test]
fn empty_and_rapid_duplicate_submissions_never_emit_side_effects() {
    let (mut state, _) = connected_state("s1");
    let empty = RequestId::from("empty");
    let effects = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: empty.clone(),
            text: " \n ".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    assert!(effects.is_empty());
    assert!(matches!(
        state.prompt_delivery,
        PromptDelivery::Rejected { ref request, .. } if request == &empty
    ));

    let first = RequestId::from("first");
    assert_eq!(
        apply(
            &mut state,
            RuntimeInput::Intent(RuntimeIntent::Submit {
                request: first.clone(),
                text: "Run once".to_owned(),
                kind: SubmissionKind::Prompt,
            }),
        )
        .len(),
        1
    );
    let duplicate = RequestId::from("duplicate");
    assert!(
        apply(
            &mut state,
            RuntimeInput::Intent(RuntimeIntent::Submit {
                request: duplicate,
                text: "Run twice".to_owned(),
                kind: SubmissionKind::Prompt,
            }),
        )
        .is_empty()
    );
    assert_eq!(
        state.prompt_delivery,
        PromptDelivery::Pending {
            request: first,
            kind: SubmissionKind::Prompt,
        }
    );
}

#[test]
fn steer_and_follow_up_use_distinct_rpc_commands_and_disconnect_safely() {
    let (mut state, _) = connected_state("s1");
    state.lifecycle = RuntimeLifecycle::Running;
    let steer = RequestId::from("steer");
    let effects = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: steer.clone(),
            text: "Change course".to_owned(),
            kind: SubmissionKind::Steer,
        }),
    );
    assert!(matches!(
        dispatch_for_effect(&effects[0]),
        RpcDispatch::Command(Command::Steer { .. })
    ));
    response(
        &mut state,
        RuntimeRequest::Submit {
            request: steer,
            text: "Change course".to_owned(),
            kind: SubmissionKind::Steer,
        },
        Ok(NormalizedResponse::Accepted),
    );

    let follow_up = RequestId::from("follow-up");
    let effects = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: follow_up.clone(),
            text: "Then summarize".to_owned(),
            kind: SubmissionKind::FollowUp,
        }),
    );
    assert!(matches!(
        dispatch_for_effect(&effects[0]),
        RpcDispatch::Command(Command::FollowUp { .. })
    ));
    apply(
        &mut state,
        RuntimeInput::Disconnected {
            error: SafeError::new(ErrorKind::Disconnected, "Connection closed"),
        },
    );
    assert_eq!(
        state.prompt_delivery,
        PromptDelivery::Uncertain {
            request: follow_up,
            kind: SubmissionKind::FollowUp,
        }
    );
}

#[test]
fn run_control_operations_dispatch_and_update_authoritative_settings() {
    let (mut state, _) = connected_state("s1");
    let steer = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::SetSteeringMode(QueueDeliveryMode::All)),
    );
    assert!(matches!(
        dispatch_for_effect(&steer[0]),
        RpcDispatch::Command(Command::SetSteeringMode {
            mode: pi_gui::services::rpc::QueueMode::All,
        })
    ));
    assert_eq!(
        state.pending_operation,
        Some(RuntimeOperation::SetSteeringMode(QueueDeliveryMode::All))
    );
    response(
        &mut state,
        RuntimeRequest::SetSteeringMode {
            mode: QueueDeliveryMode::All,
        },
        Ok(NormalizedResponse::Accepted),
    );
    assert_eq!(
        state.session.data.as_ref().unwrap().steering_mode,
        QueueDeliveryMode::All
    );
    assert!(state.pending_operation.is_none());

    let compact = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Compact {
            custom_instructions: Some("Keep build errors visible".to_owned()),
        }),
    );
    assert!(matches!(
        dispatch_for_effect(&compact[0]),
        RpcDispatch::Command(Command::Compact {
            custom_instructions: Some(ref text),
        }) if text == "Keep build errors visible"
    ));
    let refresh = response(
        &mut state,
        RuntimeRequest::Compact {
            custom_instructions: Some("Keep build errors visible".to_owned()),
        },
        Ok(NormalizedResponse::Compacted {
            summary: "summary".to_owned(),
        }),
    );
    assert!(matches!(
        state.compaction,
        CompactionState::Completed {
            reason: CompactionKind::Manual,
            ..
        }
    ));
    assert!(state.context_awaiting_fresh_usage);
    assert!(matches!(
        refresh[0].effect,
        EffectKind::Request(RuntimeRequest::GetEntries { .. })
    ));

    let auto = apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::SetAutoRetry { enabled: false }),
    );
    assert!(matches!(
        dispatch_for_effect(&auto[0]),
        RpcDispatch::Command(Command::SetAutoRetry { enabled: false })
    ));
    response(
        &mut state,
        RuntimeRequest::SetAutoRetry { enabled: false },
        Ok(NormalizedResponse::Accepted),
    );
    assert_eq!(state.auto_retry_enabled, Some(false));
}

#[test]
fn cancellation_scopes_do_not_cross_target_operations() {
    let (mut state, _) = connected_state("s1");
    state.lifecycle = RuntimeLifecycle::Running;
    state.retry = RetryState::Waiting {
        attempt: 2,
        max_attempts: 3,
        delay_ms: 1_000,
        started_at: Instant::now(),
    };
    state.bash_executions.push(BashExecution {
        request: RequestId::from("bash-scope"),
        command: "sleep 30".to_owned(),
        exclude_from_context: false,
        output: String::new(),
        exit_code: None,
        cancelled: false,
        truncated: false,
        full_output_path: None,
        status: BashStatus::Running,
        started_at: Instant::now(),
        finished_at: None,
        reconciled: false,
        baseline: Default::default(),
        error: None,
    });

    let retry_abort = apply(&mut state, RuntimeInput::Intent(RuntimeIntent::AbortRetry));
    assert!(matches!(
        retry_abort[0].effect,
        EffectKind::Request(RuntimeRequest::AbortRetry)
    ));
    assert_eq!(state.retry, RetryState::Cancelling);
    assert_eq!(state.bash_executions[0].status, BashStatus::Running);

    let bash_abort = apply(&mut state, RuntimeInput::Intent(RuntimeIntent::AbortBash));
    assert!(matches!(
        bash_abort[0].effect,
        EffectKind::Request(RuntimeRequest::AbortBash)
    ));
    assert_eq!(state.bash_executions[0].status, BashStatus::Cancelling);
}

#[test]
fn close_during_tool_marks_live_tool_uncertain_without_erasing_transcript() {
    let (mut state, _) = connected_state("s1");
    state.messages.ready(vec![message("durable", "kept", true)]);
    let tool_id = ToolCallId::from("live-tool");
    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolStart {
            id: tool_id.clone(),
            name: "read".to_owned(),
            arguments: json!({"path": "src/lib.rs"}),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Disconnected {
            error: SafeError::new(ErrorKind::Process, "Pi exited unexpectedly"),
        },
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Disconnected);
    assert_eq!(state.messages.data.as_ref().map(Vec::len), Some(1));
    assert_eq!(state.tools[&tool_id].status, ToolStatus::Uncertain);

    apply(
        &mut state,
        RuntimeInput::Event(NormalizedEvent::ToolEnd {
            id: tool_id.clone(),
            name: "read".to_owned(),
            result: json!({"text": "late authoritative"}),
            is_error: false,
            cancelled: false,
        }),
    );
    assert_eq!(state.tools[&tool_id].status, ToolStatus::Succeeded);
}

#[test]
fn crash_after_acceptance_preserves_last_durable_transcript_read_only() {
    let (mut state, _) = connected_state("s1");
    state.messages.ready(vec![message("durable", "kept", true)]);
    let request = RequestId::from("accepted-before-crash");
    let submit = RuntimeRequest::Submit {
        request: request.clone(),
        text: "May not be durable yet".to_owned(),
        kind: SubmissionKind::Prompt,
    };
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Submit {
            request: request.clone(),
            text: "May not be durable yet".to_owned(),
            kind: SubmissionKind::Prompt,
        }),
    );
    response(&mut state, submit, Ok(NormalizedResponse::Accepted));
    apply(
        &mut state,
        RuntimeInput::Disconnected {
            error: SafeError::new(ErrorKind::Process, "Pi exited unexpectedly"),
        },
    );
    assert_eq!(state.lifecycle, RuntimeLifecycle::Disconnected);
    assert_eq!(state.messages.data.as_ref().map(Vec::len), Some(1));
    assert!(
        state
            .optimistic_user_inputs
            .iter()
            .any(|input| input.request == request)
    );
}

#[test]
fn crash_during_switch_invalidates_dialogs_and_requires_explicit_reconnect() {
    let (mut state, _) = connected_state("s1");
    state.dialogs.insert(
        RequestId::from("switch-dialog"),
        DialogRequest::Confirm {
            title: "Continue".to_owned(),
            message: "Switch?".to_owned(),
        },
    );
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::ReplaceSession(SessionMutation::Switch {
            session_path: "other.jsonl".to_owned(),
        })),
    );
    assert!(state.dialogs.is_empty());
    assert!(state.replacement_awaiting_state);

    let effects = apply(
        &mut state,
        RuntimeInput::Disconnected {
            error: SafeError::new(ErrorKind::Process, "Pi exited during session switch"),
        },
    );
    assert!(effects.is_empty());
    assert_eq!(state.lifecycle, RuntimeLifecycle::Disconnected);
    assert!(!state.replacement_awaiting_state);
}

#[test]
fn stale_generation_cannot_complete_run_control_operation() {
    let (mut state, _) = connected_state("s1");
    let ignored_before = state.stale_inputs_ignored;
    let epoch = state.epoch;
    let effects = reduce(
        &mut state,
        StampedInput {
            generation: ConnectionGeneration::default(),
            epoch,
            observed_at: Instant::now(),
            input: RuntimeInput::Intent(RuntimeIntent::SetAutoCompaction { enabled: false }),
        },
    );
    assert!(effects.is_empty());
    assert_eq!(state.stale_inputs_ignored, ignored_before + 1);
    assert!(state.pending_operation.is_none());
}
