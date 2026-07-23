use std::collections::{HashMap, HashSet};
use std::time::Instant;

use super::runtime::{
    BashExecution, BashStatus, CommandSource, CompactionState, EffectKind, ErrorKind, FacetStatus,
    HydrationMode, MAX_NOTIFICATIONS, MAX_UNKNOWN_RECORDS, MessageBlock, MessageRole,
    NormalizedEvent, NormalizedResponse, OptimisticUserInput, PromptDelivery, QueueContents,
    RequestFailureKind, RetryState, RuntimeEffect, RuntimeInput, RuntimeIntent, RuntimeLifecycle,
    RuntimeMessage, RuntimeOperation, RuntimeRequest, RuntimeState, SafeError, SessionMutation,
    StampedInput, SubmissionKind, ToolExecution, ToolStatus, UnknownRecord, push_bounded,
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
        RuntimeInput::Disconnected { error } => disconnect(state, error, stamped.observed_at),
        RuntimeInput::Intent(intent) => reduce_intent(state, intent, stamped.observed_at),
        RuntimeInput::Response { request, result } => {
            reduce_response(state, request, result, stamped.observed_at)
        }
        RuntimeInput::Event(event) => {
            if state.replacement_awaiting_state {
                state.stale_inputs_ignored = state.stale_inputs_ignored.saturating_add(1);
            } else {
                reduce_event(state, event, stamped.observed_at);
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
        state.pending_operation = None;
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

fn disconnect(
    state: &mut RuntimeState,
    error: SafeError,
    observed_at: Instant,
) -> Vec<RuntimeEffect> {
    state.pending_operation = None;
    state.replacement_awaiting_state = false;
    if matches!(state.prompt_delivery, PromptDelivery::Pending { .. }) {
        mark_prompt_uncertain(state);
    }
    for execution in &mut state.bash_executions {
        if execution.status.is_active() {
            execution.status = BashStatus::Uncertain;
            execution.finished_at = Some(observed_at);
            execution.error = Some("Pi disconnected during Bash execution".to_owned());
        }
    }
    for tool in state.tools.values_mut() {
        if matches!(tool.status, ToolStatus::Pending | ToolStatus::Running) {
            tool.status = ToolStatus::Uncertain;
            tool.finished_at = Some(observed_at);
        }
    }
    state.lifecycle = RuntimeLifecycle::Disconnected;
    invalidate_extension_ui(state);
    state.bounded_error(error);
    state.bump_revision();
    Vec::new()
}

fn reduce_intent(
    state: &mut RuntimeState,
    intent: RuntimeIntent,
    observed_at: Instant,
) -> Vec<RuntimeEffect> {
    match intent {
        RuntimeIntent::Submit {
            request,
            text,
            kind,
        } => submit(state, request, text, kind, None),
        RuntimeIntent::InvokeCommand {
            request,
            text,
            kind,
            source,
        } => submit(state, request, text, kind, Some(source)),
        RuntimeIntent::RefreshCommands => {
            state.commands.loading();
            vec![effect(state, RuntimeRequest::GetCommands)]
        }
        RuntimeIntent::ExecuteBash {
            request,
            command,
            exclude_from_context,
        } => {
            if command.trim().is_empty()
                || state
                    .bash_executions
                    .iter()
                    .any(|execution| execution.status.is_active())
            {
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
            state.bash_executions.push(BashExecution {
                request: request.clone(),
                command: command.clone(),
                exclude_from_context,
                output: String::new(),
                exit_code: None,
                cancelled: false,
                truncated: false,
                full_output_path: None,
                status: BashStatus::Running,
                started_at: observed_at,
                finished_at: None,
                reconciled: false,
                baseline,
                error: None,
            });
            state.bump_revision();
            vec![effect(
                state,
                RuntimeRequest::ExecuteBash {
                    request,
                    command,
                    exclude_from_context,
                },
            )]
        }
        RuntimeIntent::Abort => {
            state.lifecycle = RuntimeLifecycle::Cancelling;
            vec![effect(state, RuntimeRequest::Abort)]
        }
        RuntimeIntent::AbortBash => {
            let Some(execution) = state
                .bash_executions
                .iter_mut()
                .rev()
                .find(|execution| execution.status == BashStatus::Running)
            else {
                return Vec::new();
            };
            execution.status = BashStatus::Cancelling;
            state.bump_revision();
            vec![effect(state, RuntimeRequest::AbortBash)]
        }
        RuntimeIntent::AbortRetry => {
            state.lifecycle = RuntimeLifecycle::Cancelling;
            state.retry = RetryState::Cancelling;
            vec![effect(state, RuntimeRequest::AbortRetry)]
        }
        RuntimeIntent::SetModel { provider, id } => {
            if !idle_settings_allowed(state) || state.pending_operation.is_some() {
                return Vec::new();
            }
            state.pending_operation = Some(RuntimeOperation::SetModel {
                provider: provider.clone(),
                id: id.clone(),
            });
            state.bump_revision();
            vec![effect(state, RuntimeRequest::SetModel { provider, id })]
        }
        RuntimeIntent::SetThinkingLevel(level) => {
            if !idle_settings_allowed(state) || state.pending_operation.is_some() {
                return Vec::new();
            }
            state.pending_operation = Some(RuntimeOperation::SetThinkingLevel(level));
            state.bump_revision();
            vec![effect(state, RuntimeRequest::SetThinkingLevel { level })]
        }
        RuntimeIntent::SetSteeringMode(mode) => {
            if !settings_allowed(state) || state.pending_operation.is_some() {
                return Vec::new();
            }
            state.pending_operation = Some(RuntimeOperation::SetSteeringMode(mode));
            state.bump_revision();
            vec![effect(state, RuntimeRequest::SetSteeringMode { mode })]
        }
        RuntimeIntent::SetFollowUpMode(mode) => {
            if !settings_allowed(state) || state.pending_operation.is_some() {
                return Vec::new();
            }
            state.pending_operation = Some(RuntimeOperation::SetFollowUpMode(mode));
            state.bump_revision();
            vec![effect(state, RuntimeRequest::SetFollowUpMode { mode })]
        }
        RuntimeIntent::Compact {
            custom_instructions,
        } => {
            if !matches!(
                state.lifecycle,
                RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
            ) || state.pending_operation.is_some()
                || !matches!(
                    state.compaction,
                    CompactionState::Idle | CompactionState::Completed { .. }
                )
            {
                return Vec::new();
            }
            state.pending_operation = Some(RuntimeOperation::Compact);
            state.compaction = CompactionState::Running {
                reason: super::runtime::CompactionKind::Manual,
            };
            state.bump_revision();
            vec![effect(
                state,
                RuntimeRequest::Compact {
                    custom_instructions,
                },
            )]
        }
        RuntimeIntent::SetAutoCompaction { enabled } => {
            if !settings_allowed(state) || state.pending_operation.is_some() {
                return Vec::new();
            }
            state.pending_operation = Some(RuntimeOperation::SetAutoCompaction(enabled));
            state.bump_revision();
            vec![effect(state, RuntimeRequest::SetAutoCompaction { enabled })]
        }
        RuntimeIntent::SetAutoRetry { enabled } => {
            if !settings_allowed(state) || state.pending_operation.is_some() {
                return Vec::new();
            }
            state.pending_operation = Some(RuntimeOperation::SetAutoRetry(enabled));
            state.bump_revision();
            vec![effect(state, RuntimeRequest::SetAutoRetry { enabled })]
        }
        RuntimeIntent::SetSessionName { name } => {
            let name = name.trim().to_owned();
            if name.is_empty() || !settings_allowed(state) || state.pending_operation.is_some() {
                return Vec::new();
            }
            state.pending_operation = Some(RuntimeOperation::SetSessionName(name.clone()));
            state.bump_revision();
            vec![effect(state, RuntimeRequest::SetSessionName { name })]
        }
        RuntimeIntent::ExportHtml { output_path } => {
            if !settings_allowed(state) || state.pending_operation.is_some() {
                return Vec::new();
            }
            state.pending_operation = Some(RuntimeOperation::ExportHtml);
            state.bump_revision();
            vec![effect(state, RuntimeRequest::ExportHtml { output_path })]
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

fn submit(
    state: &mut RuntimeState,
    request: RequestId,
    text: String,
    kind: SubmissionKind,
    dynamic_source: Option<CommandSource>,
) -> Vec<RuntimeEffect> {
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
    let request = match dynamic_source {
        Some(source) => RuntimeRequest::InvokeCommand {
            request,
            text,
            kind,
            source,
        },
        None => RuntimeRequest::Submit {
            request,
            text,
            kind,
        },
    };
    vec![effect(state, request)]
}

fn begin_session_replacement(
    state: &mut RuntimeState,
    mutation: SessionMutation,
) -> Vec<RuntimeEffect> {
    state.pending_operation = None;
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
    observed_at: Instant,
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
            reconcile_bash_executions(state);
            state.bump_revision();
            Vec::new()
        }
        (
            RuntimeRequest::GetEntries {
                since,
                base_revision,
            },
            Ok(NormalizedResponse::Entries { entries, leaf_id }),
        ) => {
            state.entries_leaf_id = leaf_id;
            apply_entries(state, since, entries, base_revision);
            rebuild_tree_from_entries(state);
            Vec::new()
        }
        (RuntimeRequest::GetStats, Ok(NormalizedResponse::Stats(stats))) => {
            if state.session_id().is_some_and(|id| id == &stats.session_id) {
                state.context_awaiting_fresh_usage = stats.context_tokens.is_none();
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
            Ok(NormalizedResponse::Tree { tree, leaf_id }),
        ) => {
            state.tree_leaf_id = leaf_id;
            if state.revision == base_revision || state.tree.data.is_none() {
                state.tree.ready(tree);
            } else {
                state.tree.status = FacetStatus::Ready;
            }
            Vec::new()
        }
        (RuntimeRequest::GetForkMessages, Ok(NormalizedResponse::ForkMessages(messages))) => {
            state.fork_messages.ready(messages);
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
        (
            RuntimeRequest::InvokeCommand {
                request,
                kind,
                source,
                ..
            },
            Ok(NormalizedResponse::Accepted),
        ) => {
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
                if source != CommandSource::Extension
                    && kind == SubmissionKind::Prompt
                    && !state.pending_prompt_settled
                {
                    state.lifecycle = RuntimeLifecycle::Running;
                }
                state.pending_prompt_settled = false;
            }
            if source == CommandSource::Extension {
                state.commands.loading();
                vec![effect(state, RuntimeRequest::GetCommands)]
            } else {
                Vec::new()
            }
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
        (RuntimeRequest::InvokeCommand { request, kind, .. }, Err(failure)) => {
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
        (RuntimeRequest::ExecuteBash { request, .. }, Ok(NormalizedResponse::Bash(result))) => {
            if let Some(execution) = state
                .bash_executions
                .iter_mut()
                .find(|execution| execution.request == request)
            {
                execution.output = result.output;
                execution.exit_code = result.exit_code;
                execution.cancelled = result.cancelled;
                execution.truncated = result.truncated;
                execution.full_output_path = result.full_output_path;
                execution.status = if result.cancelled {
                    BashStatus::Cancelled
                } else if result.exit_code.is_some_and(|code| code != 0) {
                    BashStatus::Failed
                } else {
                    BashStatus::Succeeded
                };
                execution.finished_at = Some(observed_at);
                state.bump_revision();
            }
            vec![effect(
                state,
                RuntimeRequest::GetMessages {
                    base_revision: state.revision,
                },
            )]
        }
        (RuntimeRequest::ExecuteBash { request, .. }, Err(failure)) => {
            if let Some(execution) = state
                .bash_executions
                .iter_mut()
                .find(|execution| execution.request == request)
            {
                execution.status = if matches!(
                    failure.kind,
                    RequestFailureKind::UnknownOutcome | RequestFailureKind::Disconnected
                ) {
                    BashStatus::Uncertain
                } else {
                    BashStatus::Failed
                };
                execution.error = Some(failure.error.summary.clone());
                execution.finished_at = Some(observed_at);
                state.bump_revision();
            }
            state.bounded_error(failure.error);
            Vec::new()
        }
        (RuntimeRequest::Abort | RuntimeRequest::AbortRetry, Ok(NormalizedResponse::Accepted)) => {
            // Acknowledgement is not settlement. Pi may still be unwinding tools or retry work.
            state.lifecycle = RuntimeLifecycle::Cancelling;
            Vec::new()
        }
        (RuntimeRequest::SetSteeringMode { mode }, Ok(NormalizedResponse::Accepted)) => {
            if matches!(state.pending_operation, Some(RuntimeOperation::SetSteeringMode(pending)) if pending == mode)
            {
                if let Some(session) = state.session.data.as_mut() {
                    session.steering_mode = mode;
                }
                state.pending_operation = None;
                state.bump_revision();
            }
            Vec::new()
        }
        (RuntimeRequest::SetFollowUpMode { mode }, Ok(NormalizedResponse::Accepted)) => {
            if matches!(state.pending_operation, Some(RuntimeOperation::SetFollowUpMode(pending)) if pending == mode)
            {
                if let Some(session) = state.session.data.as_mut() {
                    session.follow_up_mode = mode;
                }
                state.pending_operation = None;
                state.bump_revision();
            }
            Vec::new()
        }
        (
            RuntimeRequest::SetModel { provider, id },
            Ok(NormalizedResponse::ModelChanged { model }),
        ) => {
            if matches!(state.pending_operation.as_ref(), Some(RuntimeOperation::SetModel { provider: pending_provider, id: pending_id }) if pending_provider == &provider && pending_id == &id)
            {
                if let Some(session) = state.session.data.as_mut() {
                    session.model = Some(model);
                }
                state.pending_operation = None;
                state.stats.loading();
                state.context_awaiting_fresh_usage = true;
                state.bump_revision();
            }
            vec![effect(state, RuntimeRequest::GetStats)]
        }
        (RuntimeRequest::SetThinkingLevel { level }, Ok(NormalizedResponse::Accepted)) => {
            if matches!(state.pending_operation, Some(RuntimeOperation::SetThinkingLevel(pending)) if pending == level)
            {
                if let Some(session) = state.session.data.as_mut() {
                    session.thinking_level = level;
                }
                state.pending_operation = None;
                state.bump_revision();
            }
            Vec::new()
        }
        (RuntimeRequest::Compact { .. }, Ok(NormalizedResponse::Compacted { summary })) => {
            state.pending_operation = None;
            state.compaction = CompactionState::Completed {
                reason: super::runtime::CompactionKind::Manual,
                summary,
                will_retry: false,
            };
            state.context_awaiting_fresh_usage = true;
            if let Some(stats) = state.stats.data.as_mut() {
                stats.context_tokens = None;
                stats.context_percent = None;
            }
            state.bump_revision();
            vec![effect(
                state,
                RuntimeRequest::GetEntries {
                    since: state.durable_cursor.clone(),
                    base_revision: state.revision,
                },
            )]
        }
        (RuntimeRequest::SetAutoCompaction { enabled }, Ok(NormalizedResponse::Accepted)) => {
            if matches!(state.pending_operation, Some(RuntimeOperation::SetAutoCompaction(pending)) if pending == enabled)
            {
                if let Some(session) = state.session.data.as_mut() {
                    session.auto_compaction_enabled = enabled;
                }
                state.pending_operation = None;
                state.bump_revision();
            }
            Vec::new()
        }
        (RuntimeRequest::SetAutoRetry { enabled }, Ok(NormalizedResponse::Accepted)) => {
            if matches!(state.pending_operation, Some(RuntimeOperation::SetAutoRetry(pending)) if pending == enabled)
            {
                state.auto_retry_enabled = Some(enabled);
                state.pending_operation = None;
                state.bump_revision();
            }
            Vec::new()
        }
        (RuntimeRequest::SetSessionName { name }, Ok(NormalizedResponse::Accepted)) => {
            if matches!(state.pending_operation.as_ref(), Some(RuntimeOperation::SetSessionName(pending)) if pending == &name)
            {
                if let Some(session) = state.session.data.as_mut() {
                    session.name = Some(name);
                }
                state.pending_operation = None;
                state.bump_revision();
            }
            Vec::new()
        }
        (RuntimeRequest::ExportHtml { .. }, Ok(NormalizedResponse::Exported { path })) => {
            if matches!(state.pending_operation, Some(RuntimeOperation::ExportHtml)) {
                state.pending_operation = None;
                state
                    .notifications
                    .push_back(super::runtime::RuntimeNotification {
                        message: format!("Session exported to {path}"),
                        kind: super::runtime::NotificationKind::Info,
                    });
                while state.notifications.len() > MAX_NOTIFICATIONS {
                    state.notifications.pop_front();
                }
                state.bump_revision();
            }
            Vec::new()
        }
        (RuntimeRequest::AbortBash, Ok(NormalizedResponse::Accepted)) => Vec::new(),
        (RuntimeRequest::AbortBash, Err(failure)) => {
            if let Some(execution) = state
                .bash_executions
                .iter_mut()
                .rev()
                .find(|execution| execution.status == BashStatus::Cancelling)
            {
                execution.status = BashStatus::Running;
                execution.error = Some(failure.error.summary.clone());
                state.bump_revision();
            }
            state.bounded_error(failure.error);
            Vec::new()
        }
        (
            RuntimeRequest::SessionMutation(_),
            Ok(NormalizedResponse::SessionMutation {
                cancelled,
                editor_text,
            }),
        ) => {
            if !cancelled && editor_text.is_some() {
                state.requested_editor_text = editor_text;
            }
            state.hydration_mode = if cancelled {
                state.replacement_awaiting_state = false;
                HydrationMode::Resync
            } else {
                HydrationMode::SessionReplacement
            };
            vec![effect(state, RuntimeRequest::GetState)]
        }
        (
            request @ (RuntimeRequest::SetSteeringMode { .. }
            | RuntimeRequest::SetFollowUpMode { .. }
            | RuntimeRequest::SetModel { .. }
            | RuntimeRequest::SetThinkingLevel { .. }
            | RuntimeRequest::Compact { .. }
            | RuntimeRequest::SetAutoCompaction { .. }
            | RuntimeRequest::SetAutoRetry { .. }
            | RuntimeRequest::SetSessionName { .. }
            | RuntimeRequest::ExportHtml { .. }),
            Err(failure),
        ) => {
            if matches!(request, RuntimeRequest::Compact { .. }) {
                state.compaction = CompactionState::Failed {
                    reason: super::runtime::CompactionKind::Manual,
                    summary: failure.error.summary.clone(),
                };
            }
            state.pending_operation = None;
            state.bounded_error(failure.error);
            state.bump_revision();
            Vec::new()
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
    let replacement_editor_text = state.requested_editor_text.clone();
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
        state.requested_editor_text = replacement_editor_text;
        state.display_epoch = state.epoch;
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
        effect(state, RuntimeRequest::GetForkMessages),
    ]
}

fn reduce_event(state: &mut RuntimeState, event: NormalizedEvent, observed_at: Instant) {
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
            let sequence = state.next_tool_sequence;
            let tool = state
                .tools
                .entry(id.clone())
                .or_insert_with(|| ToolExecution {
                    id,
                    name: name.clone(),
                    arguments: arguments.clone(),
                    result: None,
                    status: ToolStatus::Pending,
                    authoritative_end: false,
                    sequence,
                    started_at: observed_at,
                    finished_at: None,
                });
            if !tool.status.is_terminal() {
                tool.name = name;
                tool.arguments = arguments;
                tool.status = ToolStatus::Running;
                state.next_tool_sequence = state.next_tool_sequence.saturating_add(1);
                state.lifecycle = RuntimeLifecycle::Running;
                state.bump_revision();
            }
        }
        NormalizedEvent::ToolUpdate {
            id,
            name,
            arguments,
            accumulated,
        } => {
            let sequence = state.next_tool_sequence;
            let tool = state
                .tools
                .entry(id.clone())
                .or_insert_with(|| ToolExecution {
                    id,
                    name: name.clone(),
                    arguments: arguments.clone(),
                    result: None,
                    status: ToolStatus::Pending,
                    authoritative_end: false,
                    sequence,
                    started_at: observed_at,
                    finished_at: None,
                });
            if !tool.status.is_terminal() {
                tool.name = name;
                tool.arguments = arguments;
                // Pi sends the full accumulated partial result, not an append-only delta.
                tool.result = Some(accumulated);
                tool.status = ToolStatus::Running;
                state.next_tool_sequence = state.next_tool_sequence.saturating_add(1);
                state.bump_revision();
            }
        }
        NormalizedEvent::ToolEnd {
            id,
            name,
            result,
            is_error,
            cancelled,
        } => {
            let sequence = state.next_tool_sequence;
            let tool = state
                .tools
                .entry(id.clone())
                .or_insert_with(|| ToolExecution {
                    id,
                    name: name.clone(),
                    arguments: serde_json::Value::Null,
                    result: None,
                    status: ToolStatus::Pending,
                    authoritative_end: false,
                    sequence,
                    started_at: observed_at,
                    finished_at: None,
                });
            if !tool.status.is_terminal()
                || (matches!(tool.status, ToolStatus::Cancelled | ToolStatus::Uncertain)
                    && !tool.authoritative_end)
            {
                tool.name = name;
                tool.result = Some(result);
                tool.status = if cancelled {
                    ToolStatus::Cancelled
                } else if is_error {
                    ToolStatus::Failed
                } else {
                    ToolStatus::Succeeded
                };
                tool.authoritative_end = true;
                tool.finished_at = Some(observed_at);
                state.next_tool_sequence = state.next_tool_sequence.saturating_add(1);
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
            let compaction_succeeded = !aborted && error.is_none();
            state.pending_operation = None;
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
            state.context_awaiting_fresh_usage = compaction_succeeded;
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
                started_at: observed_at,
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
    state.tree_leaf_id = None;
    state.entries_leaf_id = None;
    state.fork_messages.data = None;
    state.tools.clear();
    state.bash_executions.clear();
    state.queue = QueueContents::default();
    state.retry = RetryState::Idle;
    state.compaction = CompactionState::Idle;
    state.pending_operation = None;
    state.context_awaiting_fresh_usage = false;
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

fn settings_allowed(state: &RuntimeState) -> bool {
    state.session.data.is_some()
        && matches!(
            state.lifecycle,
            RuntimeLifecycle::Ready | RuntimeLifecycle::Running | RuntimeLifecycle::Settled
        )
}

fn idle_settings_allowed(state: &RuntimeState) -> bool {
    state.session.data.is_some()
        && matches!(
            state.lifecycle,
            RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
        )
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
        rebuild_tree_from_entries(state);
        state.bump_revision();
    }
}

fn rebuild_tree_from_entries(state: &mut RuntimeState) {
    let Some(entries) = state.entries.data.as_ref() else {
        return;
    };
    let indices = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut labels = HashMap::new();
    for entry in entries {
        if let super::runtime::EntryKind::Label { target, label } = &entry.kind {
            labels.insert(target.clone(), label.clone());
        }
    }

    let mut children = vec![Vec::new(); entries.len()];
    let mut roots = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let parent = entry
            .parent_id
            .as_ref()
            .filter(|parent| *parent != &entry.id)
            .and_then(|parent| indices.get(parent).copied());
        if let Some(parent) = parent {
            children[parent].push(index);
        } else {
            roots.push(index);
        }
    }
    for siblings in &mut children {
        siblings.sort_by(|left, right| entries[*left].timestamp.cmp(&entries[*right].timestamp));
    }

    fn build_node(
        index: usize,
        entries: &[super::runtime::RuntimeEntry],
        children: &[Vec<usize>],
        labels: &HashMap<crate::services::rpc::EntryId, Option<String>>,
    ) -> super::runtime::RuntimeTreeNode {
        let entry = entries[index].clone();
        let label = labels.get(&entry.id).cloned().flatten();
        super::runtime::RuntimeTreeNode {
            entry,
            children: children[index]
                .iter()
                .map(|child| build_node(*child, entries, children, labels))
                .collect(),
            label,
        }
    }

    state.tree_leaf_id = state.entries_leaf_id.clone();
    state.tree.ready(
        roots
            .into_iter()
            .map(|root| build_node(root, entries, &children, &labels))
            .collect(),
    );
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
    reconcile_bash_executions(state);
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

fn reconcile_bash_executions(state: &mut RuntimeState) {
    let messages = state.messages.data.as_deref().unwrap_or_default();
    for execution in &mut state.bash_executions {
        if execution.reconciled || execution.status.is_active() {
            continue;
        }
        execution.reconciled = messages.iter().any(|message| {
            !execution.baseline.contains(&message.key)
                && message.content.iter().any(|block| match block {
                    MessageBlock::Bash {
                        command,
                        output,
                        cancelled,
                        exclude_from_context,
                        ..
                    } => {
                        command == &execution.command
                            && output == &execution.output
                            && cancelled == &execution.cancelled
                            && exclude_from_context == &execution.exclude_from_context
                    }
                    _ => false,
                })
        });
    }
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
        RuntimeRequest::GetForkMessages => state.fork_messages.failed(error),
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
            | RuntimeRequest::GetForkMessages
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
        RuntimeRequest::GetForkMessages => "get_fork_messages",
        RuntimeRequest::Submit { kind, .. } => match kind {
            SubmissionKind::Prompt => "prompt",
            SubmissionKind::Steer => "steer",
            SubmissionKind::FollowUp => "follow_up",
        },
        RuntimeRequest::InvokeCommand { .. } => "prompt",
        RuntimeRequest::ExecuteBash { .. } => "bash",
        RuntimeRequest::Abort => "abort",
        RuntimeRequest::AbortBash => "abort_bash",
        RuntimeRequest::AbortRetry => "abort_retry",
        RuntimeRequest::SetModel { .. } => "set_model",
        RuntimeRequest::SetThinkingLevel { .. } => "set_thinking_level",
        RuntimeRequest::SetSteeringMode { .. } => "set_steering_mode",
        RuntimeRequest::SetFollowUpMode { .. } => "set_follow_up_mode",
        RuntimeRequest::Compact { .. } => "compact",
        RuntimeRequest::SetAutoCompaction { .. } => "set_auto_compaction",
        RuntimeRequest::SetAutoRetry { .. } => "set_auto_retry",
        RuntimeRequest::SetSessionName { .. } => "set_session_name",
        RuntimeRequest::ExportHtml { .. } => "export_html",
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
