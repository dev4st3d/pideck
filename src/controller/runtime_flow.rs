//! Runtime scheduling, connection transitions, and frame-aware event batching.
//!
//! Streaming message and tool updates are replaceable within one display frame.
//! Lifecycle and control records remain strict ordering barriers so coalescing can
//! never move visible state across a semantic transition. Reconnect policy lives
//! here as well so the controller owns intent while this module owns timing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{Context, Task};

use super::RuntimeController;
use crate::services::rpc::ConnectionGeneration;
use crate::services::runtime_worker::{
    AttemptGeneration, RuntimeStartFailureKind, WorkerResult,
};
use crate::state::runtime::{NormalizedEvent, RuntimeInput};

const FRAME_BUDGET: Duration = Duration::from_millis(16);
const MAX_BATCH: usize = 512;
const RECONNECT_BASE_DELAY: Duration = Duration::from_millis(300);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(8);

pub(super) fn reconnect_delay(failure_count: u32) -> Duration {
    let shift = failure_count.min(5);
    RECONNECT_BASE_DELAY
        .checked_mul(1_u32 << shift)
        .unwrap_or(RECONNECT_MAX_DELAY)
        .min(RECONNECT_MAX_DELAY)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectionTransition {
    None,
    Connected,
    Disconnected,
    RetryableFailure,
    TerminalFailure,
}

pub(super) fn connection_transition(
    result: &WorkerResult,
    current_attempt: AttemptGeneration,
    current_generation: ConnectionGeneration,
) -> ConnectionTransition {
    match result {
        WorkerResult::Connected {
            attempt,
            generation,
        } if *attempt == current_attempt && *generation == current_generation => {
            ConnectionTransition::Connected
        }
        WorkerResult::Input { attempt, input }
            if *attempt == current_attempt
                && input.generation == current_generation
                && matches!(&input.input, RuntimeInput::Disconnected { .. }) =>
        {
            ConnectionTransition::Disconnected
        }
        WorkerResult::ConnectionFailed {
            attempt,
            generation,
            failure,
        } if *attempt == current_attempt && *generation == current_generation => match failure.kind
        {
            RuntimeStartFailureKind::Readiness | RuntimeStartFailureKind::Launch => {
                ConnectionTransition::RetryableFailure
            }
            RuntimeStartFailureKind::MissingPi | RuntimeStartFailureKind::IncompatiblePi => {
                ConnectionTransition::TerminalFailure
            }
        },
        _ => ConnectionTransition::None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ReplaceableUpdate {
    Message(String),
    Tool(String),
}

fn replaceable_update(result: &WorkerResult) -> Option<ReplaceableUpdate> {
    let WorkerResult::Input { input, .. } = result else {
        return None;
    };
    match &input.input {
        RuntimeInput::Event(NormalizedEvent::MessageUpdate(message)) => {
            Some(ReplaceableUpdate::Message(message.key.0.clone()))
        }
        RuntimeInput::Event(NormalizedEvent::ToolUpdate { id, .. }) => {
            Some(ReplaceableUpdate::Tool(id.as_str().to_owned()))
        }
        _ => None,
    }
}

fn flush_replaceable(
    pending: &mut HashMap<ReplaceableUpdate, (usize, WorkerResult)>,
    coalesced: &mut Vec<WorkerResult>,
) {
    let mut latest = std::mem::take(pending).into_values().collect::<Vec<_>>();
    latest.sort_unstable_by_key(|(sequence, _)| *sequence);
    coalesced.extend(latest.into_iter().map(|(_, result)| result));
}

fn coalesce(results: Vec<WorkerResult>) -> Vec<WorkerResult> {
    let mut coalesced = Vec::with_capacity(results.len());
    let mut pending = HashMap::new();
    for (sequence, result) in results.into_iter().enumerate() {
        if let Some(key) = replaceable_update(&result) {
            pending.insert(key, (sequence, result));
        } else {
            // Control and lifecycle records are ordering barriers.
            flush_replaceable(&mut pending, &mut coalesced);
            coalesced.push(result);
        }
    }
    flush_replaceable(&mut pending, &mut coalesced);
    coalesced
}

fn batch_delay(elapsed: Duration, replaceable: bool) -> Option<Duration> {
    if !replaceable || elapsed >= FRAME_BUDGET {
        None
    } else {
        Some(FRAME_BUDGET - elapsed)
    }
}

pub(super) fn spawn(
    results: async_channel::Receiver<WorkerResult>,
    cx: &mut Context<RuntimeController>,
) -> Task<()> {
    cx.spawn(async move |controller, cx| {
        let mut last_dispatch = Instant::now()
            .checked_sub(FRAME_BUDGET)
            .unwrap_or_else(Instant::now);
        while let Ok(first) = results.recv().await {
            if let Some(delay) = batch_delay(
                last_dispatch.elapsed(),
                replaceable_update(&first).is_some(),
            ) {
                cx.background_executor().timer(delay).await;
            }

            let mut batch = vec![first];
            while batch.len() < MAX_BATCH {
                let Ok(result) = results.try_recv() else {
                    break;
                };
                batch.push(result);
            }

            let batch = coalesce(batch);
            let updated = controller.update(cx, |controller, cx| {
                for result in batch {
                    controller.receive(result, cx);
                }
                cx.notify();
            });
            if updated.is_err() {
                break;
            }
            last_dispatch = Instant::now();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::rpc::{ConnectionGeneration, SessionEpoch};
    use crate::services::runtime_worker::AttemptGeneration;
    use crate::state::runtime::{
        MessageKey, MessageRole, RuntimeMessage, StampedInput,
    };

    fn runtime_event(event: NormalizedEvent) -> WorkerResult {
        WorkerResult::Input {
            attempt: AttemptGeneration::new(1),
            input: Box::new(StampedInput {
                generation: ConnectionGeneration::new(1),
                epoch: SessionEpoch::new(1),
                observed_at: Instant::now(),
                input: RuntimeInput::Event(event),
            }),
        }
    }

    fn message_update(key: &str, timestamp: u64) -> WorkerResult {
        runtime_event(NormalizedEvent::MessageUpdate(RuntimeMessage {
            key: MessageKey(key.to_owned()),
            role: MessageRole::Assistant,
            timestamp,
            content: Vec::new(),
            visible: true,
            terminal: false,
            stop_reason: None,
            error: None,
            assistant: None,
        }))
    }

    #[test]
    fn first_and_control_events_dispatch_without_delay() {
        assert_eq!(batch_delay(FRAME_BUDGET, true), None);
        assert_eq!(batch_delay(Duration::ZERO, false), None);
    }

    #[test]
    fn replaceable_updates_wait_only_for_the_remaining_frame_budget() {
        assert_eq!(
            batch_delay(Duration::from_millis(3), true),
            Some(Duration::from_millis(13))
        );
    }

    #[test]
    fn keeps_only_the_latest_stream_update_per_key() {
        let coalesced = coalesce(vec![
            message_update("a", 1),
            message_update("b", 2),
            message_update("a", 3),
        ]);
        assert_eq!(coalesced.len(), 2);
        let messages = coalesced
            .iter()
            .map(|result| {
                let WorkerResult::Input { input, .. } = result else {
                    panic!("expected a runtime input");
                };
                let RuntimeInput::Event(NormalizedEvent::MessageUpdate(message)) = &input.input
                else {
                    panic!("expected a message update");
                };
                (message.key.0.as_str(), message.timestamp)
            })
            .collect::<Vec<_>>();
        assert_eq!(messages, vec![("b", 2), ("a", 3)]);
    }

    #[test]
    fn never_moves_stream_updates_across_control_events() {
        let coalesced = coalesce(vec![
            message_update("a", 1),
            runtime_event(NormalizedEvent::AgentStart),
            message_update("a", 2),
        ]);
        assert_eq!(coalesced.len(), 3);
    }
}
