use pi_gui::services::rpc::{
    Command, ConnectionGeneration, EntryId, ExtensionError, ExtensionErrorRecordType,
    IncomingRecord, RequestId, RpcClientError, RpcClientErrorKind, RpcDispatch, SessionId,
    TaggedIncomingRecord, ToolCallId, dispatch_for_effect, normalize_call_result,
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
        content: vec![MessageBlock::Text(text.to_owned())],
        terminal,
        stop_reason: Some(MessageStopReason::Stop),
        error: None,
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
        vec![MessageBlock::Text("Hello world".to_owned())]
    );
    assert!(messages[0].terminal);
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
        RuntimeInput::Intent(RuntimeIntent::Prompt {
            request: accepted.clone(),
            text: "Accepted".to_owned(),
        }),
    );
    assert_eq!(
        state.prompt_delivery,
        PromptDelivery::Pending {
            request: accepted.clone()
        }
    );
    response(
        &mut state,
        RuntimeRequest::Prompt {
            request: accepted.clone(),
            text: "Accepted".to_owned(),
        },
        Ok(NormalizedResponse::Accepted),
    );
    assert_eq!(
        state.prompt_delivery,
        PromptDelivery::Accepted { request: accepted }
    );

    let rejected = RequestId::from("rejected");
    apply(
        &mut state,
        RuntimeInput::Intent(RuntimeIntent::Prompt {
            request: rejected.clone(),
            text: "Rejected".to_owned(),
        }),
    );
    let effects = response(
        &mut state,
        RuntimeRequest::Prompt {
            request: rejected.clone(),
            text: "Rejected".to_owned(),
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
fn invalid_incremental_cursor_falls_back_to_full_once() {
    let (mut state, _) = connected_state("s1");
    state.durable_cursor = Some(EntryId::from("cursor"));
    state.cursor_session_id = Some(SessionId::from("s1"));

    let reconnect = StampedInput {
        generation: ConnectionGeneration::new(2),
        epoch: state.epoch,
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
        input: RuntimeInput::Event(NormalizedEvent::AgentStart),
    };
    assert!(reduce(&mut state, stale_generation).is_empty());
    assert_eq!(state.lifecycle, before);

    let stale_epoch = StampedInput {
        generation: state.generation,
        epoch: state.epoch.next(),
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
        RuntimeInput::Intent(RuntimeIntent::Prompt {
            request: request.clone(),
            text: "Synthetic prompt".to_owned(),
        }),
    );
    apply(
        &mut state,
        RuntimeInput::Disconnected {
            error: SafeError::new(ErrorKind::Disconnected, "Connection closed"),
        },
    );
    assert_eq!(state.prompt_delivery, PromptDelivery::Uncertain { request });

    let epoch = state.epoch;
    let effects = reduce(
        &mut state,
        StampedInput {
            generation: ConnectionGeneration::new(2),
            epoch,
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
        RuntimeInput::Intent(RuntimeIntent::Prompt {
            request: request.clone(),
            text: "Synthetic prompt".to_owned(),
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
    assert_eq!(state.prompt_delivery, PromptDelivery::Uncertain { request });

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
