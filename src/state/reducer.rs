use std::collections::HashSet;

use super::runtime::{
    CompactionState, EffectKind, ErrorKind, FacetStatus, HydrationMode, MAX_NOTIFICATIONS,
    MAX_UNKNOWN_RECORDS, MessageBlock, MessageRole, NormalizedEvent, NormalizedResponse,
    OptimisticUserInput, PromptDelivery, QueueContents, RequestFailureKind, RetryState,
    RuntimeEffect, RuntimeInput, RuntimeIntent, RuntimeLifecycle, RuntimeMessage, RuntimeRequest,
    RuntimeState, SafeError, SessionMutation, StampedInput, SubmissionKind, ToolExecution,
    ToolStatus, UnknownRecord, push_bounded,
};
use crate::services::rpc::{EntryId, RequestId};

pub fn reduce(state: &mut RuntimeState, stamped: StampedInput) -> Vec<RuntimeEffect> {
    if let RuntimeInput::Connected { recovery } = stamped.input {
        return connect(state, stamped.generation, stamped.epoch, recovery);
    }

    if stamped.generation != state.generation || stamped.epoch != state.epoch {
        state.stale_inputs_ignored = state.stale_inputs_ignored.saturating_add(1);
        return Vec::new();
    }

    match stamped.input {
        RuntimeInput::Connected { .. } => Vec::new(),
        RuntimeInput::Disconnected { error } => disconnect(state, error),
        RuntimeInput::Intent(intent) => reduce_intent(state, intent),
        RuntimeInput::Response { request, result } => reduce_response(state, request, result),
        RuntimeInput::Event(event) => {
            if state.replacement_awaiting_state {
                state.stale_inputs_ignored = state.stale_inputs_ignored.saturating_add(1);
            } else {
                reduce_event(state, event);
            }
            Vec::new()
        }
    }
}

fn connect(
    state: &mut RuntimeState,
    generation: crate::services::rpc::ConnectionGeneration,
    epoch: crate::services::rpc::SessionEpoch,
    recovery: bool,
) -> Vec<RuntimeEffect> {
    if generation < state.generation || (generation == state.generation && epoch != state.epoch) {
        state.stale_inputs_ignored = state.stale_inputs_ignored.saturating_add(1);
        return Vec::new();
    }

    let generation_changed = generation != state.generation;
    if generation_changed {
        state.generation = generation;
        state.epoch = epoch;
        state.replacement_awaiting_state = false;
        invalidate_extension_ui(state);
    }
    if matches!(state.prompt_delivery, PromptDelivery::Pending { .. }) {
        mark_prompt_uncertain(state);
    }
    state.lifecycle = RuntimeLifecycle::Loading;
    state.hydration_mode = if recovery {
        HydrationMode::Recovery
    } else {
        HydrationMode::Initial
    };
    state.incremental_fallback_used = false;
    state.live_message_keys.clear();
    state.mark_hydration_loading();
    vec![effect(state, RuntimeRequest::GetState)]
}

fn disconnect(state: &mut RuntimeState, error: SafeError) -> Vec<RuntimeEffect> {
    if matches!(state.prompt_delivery, PromptDelivery::Pending { .. }) {
        mark_prompt_uncertain(state);
    }
    state.lifecycle = RuntimeLifecycle::Disconnected;
    invalidate_extension_ui(state);
    state.bounded_error(error);
    state.bump_revision();
    Vec::new()
}

fn reduce_intent(state: &mut RuntimeState, intent: RuntimeIntent) -> Vec<RuntimeEffect> {
    match intent {
        RuntimeIntent::Submit {
            request,
            text,
            kind,
        } => {
            if matches!(state.prompt_delivery, PromptDelivery::Pending { .. }) {
                return Vec::new();
            }
            let rejection = if text.trim().is_empty() {
                Some("Write a prompt first.".to_owned())
            } else if !submission_allowed(state.lifecycle, kind) {
                Some(match kind {
                    SubmissionKind::Prompt => "Pi is not idle yet.".to_owned(),
                    SubmissionKind::Steer | SubmissionKind::FollowUp => {
                        "Pi must be running for queued input.".to_owned()
                    }
                })
            } else {
                None
            };
            if let Some(summary) = rejection {
                state.prompt_delivery = PromptDelivery::Rejected {
                    request,
                    kind,
                    summary,
                };
                return Vec::new();
            }

            let baseline = state
                .messages
                .data
                .as_ref()
                .into_iter()
                .flatten()
                .map(|message| message.key.clone())
                .collect();
            state.optimistic_user_inputs.push(OptimisticUserInput {
                request: request.clone(),
                text: text.clone(),
                kind,
                accepted: false,
                authoritative_seen: false,
                baseline,
            });
            state.pending_prompt_settled = false;
            state.prompt_delivery = PromptDelivery::Pending {
                request: request.clone(),
                kind,
            };
            vec![effect(
                state,
                RuntimeRequest::Submit {
                    request,
                    text,
                    kind,
                },
            )]
        }
        RuntimeIntent::Abort => {
            state.lifecycle = RuntimeLifecycle::Cancelling;
            vec![effect(state, RuntimeRequest::Abort)]
        }
        RuntimeIntent::AbortRetry => {
            state.lifecycle = RuntimeLifecycle::Cancelling;
            state.retry = RetryState::Cancelling;
            vec![effect(state, RuntimeRequest::AbortRetry)]
        }
        RuntimeIntent::ReplaceSession(mutation) => begin_session_replacement(state, mutation),
        RuntimeIntent::AnswerDialog { request, answer } => {
            if state.dialogs.remove(&request).is_none() {
                return Vec::new();
            }
            vec![extension_response(state, request, answer)]
        }
    }
}

fn begin_session_replacement(
    state: &mut RuntimeState,
    mutation: SessionMutation,
) -> Vec<RuntimeEffect> {
    state.epoch = state.epoch.next();
    state.lifecycle = RuntimeLifecycle::Loading;
    state.hydration_mode = HydrationMode::SessionReplacement;
    state.replacement_awaiting_state = true;
    invalidate_extension_ui(state);
    state.mark_hydration_loading();
    vec![effect(state, RuntimeRequest::SessionMutation(mutation))]
}

fn reduce_response(
    state: &mut RuntimeState,
    request: RuntimeRequest,
    result: Result<NormalizedResponse, super::runtime::RequestFailure>,
) -> Vec<RuntimeEffect> {
    match (request, result) {
        (RuntimeRequest::GetState, Ok(NormalizedResponse::State(snapshot))) => {
            apply_state_hydration(state, snapshot)
        }
        (
            RuntimeRequest::GetMessages { base_revision: _ },
            Ok(NormalizedResponse::Messages(messages)),
        ) => {
            let live = state
                .messages
                .data
                .take()
                .unwrap_or_default()
                .into_iter()
                .filter(|message| state.live_message_keys.contains(&message.key))
                .collect();
            state.messages.ready(merge_messages(messages, live));
            state.live_message_keys.clear();
            reconcile_optimistic_user_inputs(state);
            state.bump_revision();
            Vec::new()
        }
        (
            RuntimeRequest::GetEntries {
                since,
                base_revision,
            },
            Ok(NormalizedResponse::Entries {
                entries,
                leaf_id: _,
            }),
        ) => {
            apply_entries(state, since, entries, base_revision);
            Vec::new()
        }
        (RuntimeRequest::GetStats, Ok(NormalizedResponse::Stats(stats))) => {
            if state.session_id().is_some_and(|id| id == &stats.session_id) {
                state.stats.ready(stats);
            } else {
                state.stale_inputs_ignored = state.stale_inputs_ignored.saturating_add(1);
            }
            Vec::new()
        }
        (RuntimeRequest::GetCommands, Ok(NormalizedResponse::Commands(commands))) => {
            state.commands.ready(commands);
            Vec::new()
        }
        (RuntimeRequest::GetModels, Ok(NormalizedResponse::Models(models))) => {
            state.models.ready(models);
            Vec::new()
        }
        (
            RuntimeRequest::GetTree { base_revision },
            Ok(NormalizedResponse::Tree { tree, leaf_id: _ }),
        ) => {
            if state.revision == base_revision || state.tree.data.is_none() {
                state.tree.ready(tree);
            } else {
                state.tree.status = FacetStatus::Ready;
            }
            Vec::new()
        }
        (RuntimeRequest::Submit { request, kind, .. }, Ok(NormalizedResponse::Accepted)) => {
            if prompt_request_matches(&state.prompt_delivery, &request) {
                state.prompt_delivery = PromptDelivery::Accepted {
                    request: request.clone(),
                    kind,
                };
                if let Some(input) = state
                    .optimistic_user_inputs
                    .iter_mut()
                    .find(|input| input.request == request)
                {
                    input.accepted = true;
                }
                reconcile_optimistic_user_inputs(state);
                state.bump_revision();
                if kind == SubmissionKind::Prompt && !state.pending_prompt_settled {
                    state.lifecycle = RuntimeLifecycle::Running;
                }
                state.pending_prompt_settled = false;
            }
            Vec::new()
        }
        (RuntimeRequest::Submit { request, kind, .. }, Err(failure)) => {
            if prompt_request_matches(&state.prompt_delivery, &request) {
                state.prompt_delivery = if matches!(
                    failure.kind,
                    RequestFailureKind::UnknownOutcome | RequestFailureKind::Disconnected
                ) {
                    PromptDelivery::Uncertain {
                        request: request.clone(),
                        kind,
                    }
                } else {
                    PromptDelivery::Rejected {
                        request: request.clone(),
                        kind,
                        summary: failure.error.summary.clone(),
                    }
                };
            }
            state
                .optimistic_user_inputs
                .retain(|input| input.request != request);
            if kind == SubmissionKind::Prompt {
                state.pending_prompt_settled = false;
            }
            state.bounded_error(failure.error);
            Vec::new()
        }
        (RuntimeRequest::Abort | RuntimeRequest::AbortRetry, Ok(NormalizedResponse::Accepted)) => {
            // Acknowledgement is not settlement. Pi may still be unwinding tools or retry work.
            state.lifecycle = RuntimeLifecycle::Cancelling;
            Vec::new()
        }
        (
            RuntimeRequest::SessionMutation(_),
            Ok(NormalizedResponse::SessionMutation { cancelled }),
        ) => {
            state.hydration_mode = if cancelled {
                state.replacement_awaiting_state = false;
                HydrationMode::Resync
            } else {
                HydrationMode::SessionReplacement
            };
            vec![effect(state, RuntimeRequest::GetState)]
        }
        (RuntimeRequest::SessionMutation(_), Err(failure)) => {
            state.replacement_awaiting_state = false;
            state.bounded_error(failure.error.clone());
            if matches!(
                failure.kind,
                RequestFailureKind::UnknownOutcome | RequestFailureKind::Disconnected
            ) {
                state.lifecycle = RuntimeLifecycle::Disconnected;
                Vec::new()
            } else {
                state.hydration_mode = HydrationMode::Resync;
                vec![effect(state, RuntimeRequest::GetState)]
            }
        }
        (RuntimeRequest::GetEntries { since: Some(_), .. }, Err(failure))
            if failure.kind == RequestFailureKind::InvalidCursor
                && !state.incremental_fallback_used =>
        {
            state.incremental_fallback_used = true;
            vec![effect(
                state,
                RuntimeRequest::GetEntries {
                    since: None,
                    base_revision: state.revision,
                },
            )]
        }
        (request, Err(failure)) if is_hydration_request(&request) => {
            fail_hydration_facet(state, &request, failure.error);
            Vec::new()
        }
        (request, Ok(_)) => {
            let operation = request_name(&request);
            let error = SafeError::new(
                ErrorKind::Protocol,
                format!("Pi returned an unexpected {operation} result"),
            );
            fail_hydration_facet(state, &request, error.clone());
            state.bounded_error(error);
            Vec::new()
        }
        (_, Err(failure)) => {
            state.bounded_error(failure.error);
            Vec::new()
        }
    }
}

fn apply_state_hydration(
    state: &mut RuntimeState,
    snapshot: super::runtime::NormalizedSessionState,
) -> Vec<RuntimeEffect> {
    let previous_id = state.session_id().cloned();
    let session_changed = previous_id
        .as_ref()
        .is_some_and(|previous| previous != &snapshot.session.id);
    let replacing_session = state.hydration_mode == HydrationMode::SessionReplacement;

    if session_changed && !replacing_session {
        state.epoch = state.epoch.next();
    }
    if session_changed || replacing_session {
        clear_session_scoped_state(state);
    }
    state.replacement_awaiting_state = false;

    let same_cursor_session = state
        .cursor_session_id
        .as_ref()
        .is_some_and(|id| id == &snapshot.session.id);
    let incremental_cursor = (state.hydration_mode == HydrationMode::Recovery
        && same_cursor_session
        && !session_changed
        && !replacing_session)
        .then(|| state.durable_cursor.clone())
        .flatten();

    state.queue = QueueContents::Unknown {
        pending_count: snapshot.pending_message_count,
    };
    state.lifecycle = if snapshot.is_streaming {
        RuntimeLifecycle::Running
    } else {
        RuntimeLifecycle::Ready
    };
    state.compaction = if snapshot.is_compacting {
        CompactionState::Running {
            reason: super::runtime::CompactionKind::Manual,
        }
    } else {
        CompactionState::Idle
    };
    state.session.ready(snapshot.session);
    state.incremental_fallback_used = false;
    let revision = state.revision;

    vec![
        effect(
            state,
            RuntimeRequest::GetMessages {
                base_revision: revision,
            },
        ),
        effect(
            state,
            RuntimeRequest::GetEntries {
                since: incremental_cursor,
                base_revision: revision,
            },
        ),
        effect(state, RuntimeRequest::GetStats),
        effect(state, RuntimeRequest::GetCommands),
        effect(state, RuntimeRequest::GetModels),
        effect(
            state,
            RuntimeRequest::GetTree {
                base_revision: revision,
            },
        ),
    ]
}

fn reduce_event(state: &mut RuntimeState, event: NormalizedEvent) {
    match event {
        NormalizedEvent::AgentStart | NormalizedEvent::TurnStart => {
            state.lifecycle = RuntimeLifecycle::Running;
            state.pending_prompt_settled = false;
            state.low_level_agent_end_seen = false;
        }
        NormalizedEvent::AgentEnd { messages, .. } => {
            upsert_messages(state, messages, MessagePhase::Terminal);
            state.low_level_agent_end_seen = true;
        }
        NormalizedEvent::AgentSettled => settle(state),
        NormalizedEvent::TurnEnd {
            message,
            tool_results,
        } => {
            let mut messages = vec![message];
            messages.extend(tool_results);
            upsert_messages(state, messages, MessagePhase::Terminal);
        }
        NormalizedEvent::MessageStart(message) => {
            upsert_messages(state, vec![message], MessagePhase::Start)
        }
        NormalizedEvent::MessageUpdate(message) => {
            upsert_messages(state, vec![message], MessagePhase::Update)
        }
        NormalizedEvent::MessageEnd(message) => {
            upsert_messages(state, vec![message], MessagePhase::Terminal)
        }
        NormalizedEvent::ToolStart {
            id,
            name,
            arguments,
        } => {
            state.tools.insert(
                id.clone(),
                ToolExecution {
                    id,
                    name,
                    arguments,
                    result: None,
                    status: ToolStatus::Running,
                },
            );
            state.lifecycle = RuntimeLifecycle::Running;
            state.bump_revision();
        }
        NormalizedEvent::ToolUpdate {
            id,
            name,
            arguments,
            accumulated,
        } => {
            let tool = state
                .tools
                .entry(id.clone())
                .or_insert_with(|| ToolExecution {
                    id,
                    name: name.clone(),
                    arguments: arguments.clone(),
                    result: None,
                    status: ToolStatus::Pending,
                });
            if !matches!(
                tool.status,
                ToolStatus::Succeeded | ToolStatus::Failed | ToolStatus::Cancelled
            ) {
                tool.name = name;
                tool.arguments = arguments;
                tool.result = Some(accumulated);
                tool.status = ToolStatus::Running;
            }
            state.bump_revision();
        }
        NormalizedEvent::ToolEnd {
            id,
            name,
            result,
            is_error,
            cancelled,
        } => {
            let tool = state
                .tools
                .entry(id.clone())
                .or_insert_with(|| ToolExecution {
                    id,
                    name: name.clone(),
                    arguments: serde_json::Value::Null,
                    result: None,
                    status: ToolStatus::Pending,
                });
            if !matches!(
                tool.status,
                ToolStatus::Succeeded | ToolStatus::Failed | ToolStatus::Cancelled
            ) {
                tool.name = name;
                tool.result = Some(result);
                tool.status = if cancelled {
                    ToolStatus::Cancelled
                } else if is_error {
                    ToolStatus::Failed
                } else {
                    ToolStatus::Succeeded
                };
                state.bump_revision();
            }
        }
        NormalizedEvent::QueueUpdate {
            steering,
            follow_up,
        } => {
            state.queue = QueueContents::Known {
                steering,
                follow_up,
            };
            state.bump_revision();
        }
        NormalizedEvent::CompactionStart { reason } => {
            state.compaction = CompactionState::Running { reason };
            state.lifecycle = RuntimeLifecycle::Running;
            state.bump_revision();
        }
        NormalizedEvent::CompactionEnd {
            reason,
            summary,
            aborted,
            will_retry,
            error,
        } => {
            state.compaction = if aborted {
                CompactionState::Aborted { reason }
            } else if let Some(error) = error {
                CompactionState::Failed {
                    reason,
                    summary: error,
                }
            } else {
                CompactionState::Completed {
                    reason,
                    summary: summary.unwrap_or_default(),
                    will_retry,
                }
            };
            if will_retry {
                state.lifecycle = RuntimeLifecycle::Running;
            }
            if let Some(stats) = state.stats.data.as_mut() {
                stats.context_tokens = None;
                stats.context_percent = None;
            }
            state.bump_revision();
        }
        NormalizedEvent::RetryStart {
            attempt,
            max_attempts,
            delay_ms,
        } => {
            state.retry = RetryState::Waiting {
                attempt,
                max_attempts,
                delay_ms,
            };
            state.lifecycle = RuntimeLifecycle::Running;
            state.bump_revision();
        }
        NormalizedEvent::RetryEnd {
            success,
            attempt,
            final_error,
        } => {
            state.retry = if success {
                RetryState::Succeeded { attempt }
            } else {
                RetryState::Failed {
                    attempt,
                    summary: final_error.unwrap_or_else(|| "Automatic retry failed".to_owned()),
                }
            };
            state.bump_revision();
        }
        NormalizedEvent::EntryAppended(entry) => append_entry(state, entry),
        NormalizedEvent::SessionInfoChanged { name } => {
            if let Some(session) = state.session.data.as_mut() {
                session.name = name;
            }
        }
        NormalizedEvent::ThinkingLevelChanged { level } => {
            if let Some(session) = state.session.data.as_mut() {
                session.thinking_level = level;
            }
        }
        NormalizedEvent::Dialog { id, request } => {
            state.dialogs.insert(id, request);
        }
        NormalizedEvent::Notify(notification) => {
            push_bounded(&mut state.notifications, notification, MAX_NOTIFICATIONS);
        }
        NormalizedEvent::SetStatus { key, value } => {
            if let Some(value) = value {
                state.statuses.insert(key, value);
            } else {
                state.statuses.remove(&key);
            }
        }
        NormalizedEvent::SetWidget { key, value } => {
            if let Some(value) = value {
                state.widgets.insert(key, value);
            } else {
                state.widgets.remove(&key);
            }
        }
        NormalizedEvent::SetTitle(title) => state.title = Some(title),
        NormalizedEvent::SetEditorText(text) => state.requested_editor_text = Some(text),
        NormalizedEvent::ExtensionError(error) => {
            push_bounded(
                &mut state.extension_errors,
                error,
                super::runtime::MAX_RUNTIME_ERRORS,
            );
        }
        NormalizedEvent::Unknown { record_type } => {
            push_bounded(
                &mut state.unknown_records,
                UnknownRecord { record_type },
                MAX_UNKNOWN_RECORDS,
            );
        }
    }
}

fn settle(state: &mut RuntimeState) {
    state.pending_prompt_settled = matches!(
        state.prompt_delivery,
        PromptDelivery::Pending {
            kind: SubmissionKind::Prompt,
            ..
        }
    );
    state.lifecycle = RuntimeLifecycle::Settled;
    state.low_level_agent_end_seen = false;
    if matches!(state.retry, RetryState::Cancelling) {
        state.retry = RetryState::Idle;
    }
    for tool in state.tools.values_mut() {
        if matches!(tool.status, ToolStatus::Pending | ToolStatus::Running) {
            tool.status = ToolStatus::Cancelled;
        }
    }
    state.bump_revision();
}

fn clear_session_scoped_state(state: &mut RuntimeState) {
    state.messages.data = None;
    state.entries.data = None;
    state.stats.data = None;
    state.tree.data = None;
    state.tools.clear();
    state.queue = QueueContents::default();
    state.retry = RetryState::Idle;
    state.compaction = CompactionState::Idle;
    state.durable_cursor = None;
    state.cursor_session_id = None;
    state.live_message_keys.clear();
    state.optimistic_user_inputs.clear();
    state.prompt_delivery = PromptDelivery::None;
    state.pending_prompt_settled = false;
    invalidate_extension_ui(state);
}

fn invalidate_extension_ui(state: &mut RuntimeState) {
    state.dialogs.clear();
    state.statuses.clear();
    state.widgets.clear();
    state.title = None;
    state.requested_editor_text = None;
}

fn mark_prompt_uncertain(state: &mut RuntimeState) {
    state.pending_prompt_settled = false;
    if let PromptDelivery::Pending { request, kind } = &state.prompt_delivery {
        let request = request.clone();
        state.prompt_delivery = PromptDelivery::Uncertain {
            request: request.clone(),
            kind: *kind,
        };
        state
            .optimistic_user_inputs
            .retain(|input| input.request != request);
    }
}

fn prompt_request_matches(delivery: &PromptDelivery, request: &RequestId) -> bool {
    matches!(delivery, PromptDelivery::Pending { request: pending, .. } if pending == request)
}

fn submission_allowed(lifecycle: RuntimeLifecycle, kind: SubmissionKind) -> bool {
    match kind {
        SubmissionKind::Prompt => {
            matches!(
                lifecycle,
                RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
            )
        }
        SubmissionKind::Steer | SubmissionKind::FollowUp => lifecycle == RuntimeLifecycle::Running,
    }
}

fn apply_entries(
    state: &mut RuntimeState,
    since: Option<EntryId>,
    entries: Vec<super::runtime::RuntimeEntry>,
    base_revision: u64,
) {
    let entries = dedup_entries(entries);
    if since.is_some() {
        let existing = state.entries.data.take().unwrap_or_default();
        state.entries.ready(merge_entries(existing, entries));
    } else if state.revision != base_revision {
        let live = state.entries.data.take().unwrap_or_default();
        state.entries.ready(merge_entries(entries, live));
    } else {
        state.entries.ready(entries);
    }
    if let Some(last) = state
        .entries
        .data
        .as_ref()
        .and_then(|entries| entries.last())
    {
        state.durable_cursor = Some(last.id.clone());
        state.cursor_session_id = state.session_id().cloned();
    } else if since.is_none() {
        state.durable_cursor = None;
        state.cursor_session_id = state.session_id().cloned();
    }
}

fn append_entry(state: &mut RuntimeState, entry: super::runtime::RuntimeEntry) {
    let id = entry.id.clone();
    let entries = state.entries.data.get_or_insert_with(Vec::new);
    if !entries.iter().any(|existing| existing.id == id) {
        entries.push(entry);
        state.durable_cursor = Some(id);
        state.cursor_session_id = state.session_id().cloned();
        state.entries.status = FacetStatus::Ready;
        state.bump_revision();
    }
}

fn dedup_entries(entries: Vec<super::runtime::RuntimeEntry>) -> Vec<super::runtime::RuntimeEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter(|entry| seen.insert(entry.id.clone()))
        .collect()
}

fn merge_entries(
    mut existing: Vec<super::runtime::RuntimeEntry>,
    incoming: Vec<super::runtime::RuntimeEntry>,
) -> Vec<super::runtime::RuntimeEntry> {
    let mut seen = existing
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<HashSet<_>>();
    existing.extend(
        incoming
            .into_iter()
            .filter(|entry| seen.insert(entry.id.clone())),
    );
    existing
}

#[derive(Clone, Copy)]
enum MessagePhase {
    Start,
    Update,
    Terminal,
}

fn upsert_messages(state: &mut RuntimeState, messages: Vec<RuntimeMessage>, phase: MessagePhase) {
    let transcript = state.messages.data.get_or_insert_with(Vec::new);
    let mut changed = false;
    for message in messages {
        state.live_message_keys.insert(message.key.clone());
        changed |= upsert_message(transcript, message, phase);
    }
    state.messages.status = FacetStatus::Ready;
    reconcile_optimistic_user_inputs(state);
    if changed {
        state.bump_revision();
    }
}

fn merge_messages(
    mut first: Vec<RuntimeMessage>,
    second: Vec<RuntimeMessage>,
) -> Vec<RuntimeMessage> {
    let original = std::mem::take(&mut first);
    for message in original.into_iter().chain(second) {
        let phase = if message.terminal {
            MessagePhase::Terminal
        } else {
            MessagePhase::Update
        };
        upsert_message(&mut first, message, phase);
    }
    first
}

fn upsert_message(
    messages: &mut Vec<RuntimeMessage>,
    mut message: RuntimeMessage,
    phase: MessagePhase,
) -> bool {
    message.terminal |= matches!(phase, MessagePhase::Terminal);
    let Some(existing) = messages
        .iter_mut()
        .find(|existing| existing.key == message.key)
    else {
        messages.push(message);
        return true;
    };

    match phase {
        MessagePhase::Start => false,
        MessagePhase::Update if existing.terminal => false,
        MessagePhase::Update | MessagePhase::Terminal => {
            if existing == &message {
                false
            } else {
                *existing = message;
                true
            }
        }
    }
}

fn reconcile_optimistic_user_inputs(state: &mut RuntimeState) {
    let messages = state.messages.data.as_deref().unwrap_or_default();
    for input in &mut state.optimistic_user_inputs {
        input.authoritative_seen |= messages.iter().any(|message| {
            message.role == MessageRole::User
                && !input.baseline.contains(&message.key)
                && message_text(message) == input.text
        });
    }
    state
        .optimistic_user_inputs
        .retain(|input| !(input.accepted && input.authoritative_seen));
}

fn message_text(message: &RuntimeMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            MessageBlock::Text { text, .. } => Some(text.as_str()),
            MessageBlock::Thinking { .. }
            | MessageBlock::Image { .. }
            | MessageBlock::ToolCall { .. }
            | MessageBlock::ToolResult { .. }
            | MessageBlock::Bash { .. }
            | MessageBlock::Summary { .. }
            | MessageBlock::Custom { .. }
            | MessageBlock::Unsupported { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn fail_hydration_facet(state: &mut RuntimeState, request: &RuntimeRequest, error: SafeError) {
    match request {
        RuntimeRequest::GetState => {
            state.session.failed(error.clone());
            state.lifecycle = RuntimeLifecycle::Failed;
        }
        RuntimeRequest::GetMessages { .. } => state.messages.failed(error),
        RuntimeRequest::GetEntries { .. } => state.entries.failed(error),
        RuntimeRequest::GetStats => state.stats.failed(error),
        RuntimeRequest::GetCommands => state.commands.failed(error),
        RuntimeRequest::GetModels => state.models.failed(error),
        RuntimeRequest::GetTree { .. } => state.tree.failed(error),
        _ => state.bounded_error(error),
    }
}

fn is_hydration_request(request: &RuntimeRequest) -> bool {
    matches!(
        request,
        RuntimeRequest::GetState
            | RuntimeRequest::GetMessages { .. }
            | RuntimeRequest::GetEntries { .. }
            | RuntimeRequest::GetStats
            | RuntimeRequest::GetCommands
            | RuntimeRequest::GetModels
            | RuntimeRequest::GetTree { .. }
    )
}

fn request_name(request: &RuntimeRequest) -> &'static str {
    match request {
        RuntimeRequest::GetState => "get_state",
        RuntimeRequest::GetMessages { .. } => "get_messages",
        RuntimeRequest::GetEntries { .. } => "get_entries",
        RuntimeRequest::GetStats => "get_session_stats",
        RuntimeRequest::GetCommands => "get_commands",
        RuntimeRequest::GetModels => "get_available_models",
        RuntimeRequest::GetTree { .. } => "get_tree",
        RuntimeRequest::Submit { kind, .. } => match kind {
            SubmissionKind::Prompt => "prompt",
            SubmissionKind::Steer => "steer",
            SubmissionKind::FollowUp => "follow_up",
        },
        RuntimeRequest::Abort => "abort",
        RuntimeRequest::AbortRetry => "abort_retry",
        RuntimeRequest::SessionMutation(_) => "session mutation",
    }
}

fn effect(state: &mut RuntimeState, request: RuntimeRequest) -> RuntimeEffect {
    emit_effect(state, EffectKind::Request(request))
}

fn extension_response(
    state: &mut RuntimeState,
    request: RequestId,
    answer: super::runtime::DialogAnswer,
) -> RuntimeEffect {
    emit_effect(state, EffectKind::ExtensionUiResponse { request, answer })
}

fn emit_effect(state: &mut RuntimeState, effect: EffectKind) -> RuntimeEffect {
    let sequence = state.next_request;
    state.next_request = state.next_request.saturating_add(1);
    RuntimeEffect {
        generation: state.generation,
        epoch: state.epoch,
        sequence,
        effect,
    }
}
