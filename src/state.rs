//! UI-independent application projections and Pi runtime state.

pub mod history;
pub mod reducer;
pub mod runtime;

use runtime::{
    Facet, FacetStatus, RuntimeLifecycle, RuntimeState, RuntimeStats, RuntimeThinkingLevel,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerStatus {
    Idle,
    Connecting,
    Active,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    Connect,
    Retry,
    Stop,
}

impl RecoveryAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connect => "Connect",
            Self::Retry => "Retry",
            Self::Stop => "Stop",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Self::Connect => "Ctrl+Alt+C",
            Self::Retry => "Ctrl+Alt+R",
            Self::Stop => "Ctrl+Alt+S",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayValue {
    Known(String),
    Loading,
    Awaiting,
    Unknown,
    Stale(String),
}

impl DisplayValue {
    pub fn label(&self) -> String {
        match self {
            Self::Known(value) => value.clone(),
            Self::Loading => "Loading".to_owned(),
            Self::Awaiting => "Awaiting".to_owned(),
            Self::Unknown => "Unknown".to_owned(),
            Self::Stale(value) => format!("{value} · stale"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellProjection {
    pub workspace: String,
    pub session: DisplayValue,
    pub model: DisplayValue,
    pub thinking: DisplayValue,
    pub cost: DisplayValue,
    pub context: DisplayValue,
    pub input_tokens: DisplayValue,
    pub output_tokens: DisplayValue,
    pub cache_read: DisplayValue,
    pub cache_write: DisplayValue,
    pub lifecycle: String,
    pub headline: String,
    pub detail: String,
    pub action: Option<RecoveryAction>,
    pub has_stale_values: bool,
    pub no_model: bool,
}

impl ShellProjection {
    pub fn from_runtime(
        status: ControllerStatus,
        workspace: impl Into<String>,
        runtime: &RuntimeState,
        controller_error: Option<&str>,
    ) -> Self {
        let workspace = workspace.into();
        let session = project_session(runtime);
        let model = project_model(runtime);
        let thinking = project_thinking(runtime);
        let cost = project_stats(&runtime.stats, |stats| format_cost(stats.cost));
        let context = project_context(runtime);
        let input_tokens = project_stats(&runtime.stats, |stats| format_count(stats.input_tokens));
        let output_tokens =
            project_stats(&runtime.stats, |stats| format_count(stats.output_tokens));
        let cache_read = project_stats(&runtime.stats, |stats| {
            format_count(stats.cache_read_tokens)
        });
        let cache_write = project_stats(&runtime.stats, |stats| {
            format_count(stats.cache_write_tokens)
        });
        let no_model = status == ControllerStatus::Active && model_is_unavailable(runtime);
        let reducer_error = runtime
            .errors
            .back()
            .map(|error| error.summary.as_str())
            .or(match &runtime.session.status {
                FacetStatus::Failed(error) => Some(error.summary.as_str()),
                FacetStatus::Loading | FacetStatus::Ready => None,
            });
        let error = controller_error.or(reducer_error);

        let (lifecycle, headline, detail, action) = match status {
            ControllerStatus::Idle => (
                "Not connected",
                "Pi is ready to connect",
                "The runtime will use this workspace with tools and project resources disabled.",
                Some(RecoveryAction::Connect),
            ),
            ControllerStatus::Connecting => (
                "Connecting",
                "Starting Pi",
                "Discovering Pi and waiting for correlated RPC readiness.",
                Some(RecoveryAction::Stop),
            ),
            ControllerStatus::Stopping => (
                "Stopping",
                "Stopping Pi",
                "The supervised runtime is shutting down.",
                None,
            ),
            ControllerStatus::Stopped => (
                "Stopped",
                "Pi is stopped",
                "Connect to start a fresh ephemeral runtime.",
                Some(RecoveryAction::Connect),
            ),
            ControllerStatus::Failed => (
                "Connection error",
                "Pi could not connect",
                error.unwrap_or("The Pi runtime is unavailable."),
                Some(RecoveryAction::Retry),
            ),
            ControllerStatus::Active if no_model => (
                "No model",
                "No model is available",
                "Configure credentials in Pi, then retry. Credentials remain managed by Pi.",
                Some(RecoveryAction::Retry),
            ),
            ControllerStatus::Active => match runtime.lifecycle {
                RuntimeLifecycle::Loading => (
                    "Loading",
                    "Reading runtime state",
                    "Pi is ready. Session and model details are loading.",
                    Some(RecoveryAction::Stop),
                ),
                RuntimeLifecycle::Ready | RuntimeLifecycle::Settled => (
                    "Ready",
                    "Pi is ready",
                    "Live runtime, model, and usage values are shown here.",
                    Some(RecoveryAction::Stop),
                ),
                RuntimeLifecycle::Running => (
                    "Running",
                    "Pi is running",
                    "The active runtime reports work in progress.",
                    Some(RecoveryAction::Stop),
                ),
                RuntimeLifecycle::Cancelling => (
                    "Cancelling",
                    "Pi is cancelling",
                    "The current runtime operation is being cancelled.",
                    Some(RecoveryAction::Stop),
                ),
                RuntimeLifecycle::Disconnected | RuntimeLifecycle::Failed => (
                    "Connection error",
                    "The Pi connection closed",
                    error.unwrap_or("The last valid values remain visible."),
                    Some(RecoveryAction::Retry),
                ),
            },
        };

        let has_stale_values = [
            &session,
            &model,
            &thinking,
            &cost,
            &context,
            &input_tokens,
            &output_tokens,
            &cache_read,
            &cache_write,
        ]
        .into_iter()
        .any(|value| matches!(value, DisplayValue::Stale(_)));

        Self {
            workspace,
            session,
            model,
            thinking,
            cost,
            context,
            input_tokens,
            output_tokens,
            cache_read,
            cache_write,
            lifecycle: lifecycle.to_owned(),
            headline: headline.to_owned(),
            detail: detail.to_owned(),
            action,
            has_stale_values,
            no_model,
        }
    }
}

fn project_session(runtime: &RuntimeState) -> DisplayValue {
    project_facet(&runtime.session, |session| {
        session
            .name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Unknown".to_owned())
    })
}

fn project_model(runtime: &RuntimeState) -> DisplayValue {
    project_facet(&runtime.session, |session| {
        session
            .model
            .as_ref()
            .map(|model| {
                if model.name.trim().is_empty() {
                    format!("{}/{}", model.provider, model.id)
                } else {
                    model.name.clone()
                }
            })
            .unwrap_or_else(|| "Unknown".to_owned())
    })
}

fn project_thinking(runtime: &RuntimeState) -> DisplayValue {
    project_facet(&runtime.session, |session| {
        match session.thinking_level {
            RuntimeThinkingLevel::Off => "Off",
            RuntimeThinkingLevel::Minimal => "Minimal",
            RuntimeThinkingLevel::Low => "Low",
            RuntimeThinkingLevel::Medium => "Medium",
            RuntimeThinkingLevel::High => "High",
            RuntimeThinkingLevel::Xhigh => "Xhigh",
            RuntimeThinkingLevel::Max => "Max",
        }
        .to_owned()
    })
}

fn project_context(runtime: &RuntimeState) -> DisplayValue {
    if runtime.context_awaiting_fresh_usage {
        return DisplayValue::Awaiting;
    }
    let stats = &runtime.stats;
    match (&stats.status, stats.data.as_ref()) {
        (FacetStatus::Loading, None) => DisplayValue::Awaiting,
        (FacetStatus::Loading, Some(stats)) => context_value(stats, true),
        (FacetStatus::Ready, Some(stats)) => context_value(stats, false),
        (FacetStatus::Failed(_), Some(stats)) => context_value(stats, true),
        (FacetStatus::Ready | FacetStatus::Failed(_), None) => DisplayValue::Unknown,
    }
}

fn context_value(stats: &RuntimeStats, stale: bool) -> DisplayValue {
    let Some(tokens) = stats.context_tokens else {
        return DisplayValue::Unknown;
    };
    let value = match stats.context_window {
        Some(window) => format!("{} / {}", format_count(tokens), format_count(window)),
        None => format_count(tokens),
    };
    if stale {
        DisplayValue::Stale(value)
    } else {
        DisplayValue::Known(value)
    }
}

fn project_stats(
    stats: &Facet<RuntimeStats>,
    value: impl Fn(&RuntimeStats) -> String,
) -> DisplayValue {
    match (&stats.status, stats.data.as_ref()) {
        (FacetStatus::Loading, None) => DisplayValue::Awaiting,
        (FacetStatus::Loading, Some(stats)) | (FacetStatus::Failed(_), Some(stats)) => {
            DisplayValue::Stale(value(stats))
        }
        (FacetStatus::Ready, Some(stats)) => DisplayValue::Known(value(stats)),
        (FacetStatus::Ready | FacetStatus::Failed(_), None) => DisplayValue::Unknown,
    }
}

fn project_facet<T>(facet: &Facet<T>, value: impl Fn(&T) -> String) -> DisplayValue {
    match (&facet.status, facet.data.as_ref()) {
        (FacetStatus::Loading, None) => DisplayValue::Loading,
        (FacetStatus::Loading, Some(data)) | (FacetStatus::Failed(_), Some(data)) => {
            let value = value(data);
            if value == "Unknown" {
                DisplayValue::Unknown
            } else {
                DisplayValue::Stale(value)
            }
        }
        (FacetStatus::Ready, Some(data)) => {
            let value = value(data);
            if value == "Unknown" {
                DisplayValue::Unknown
            } else {
                DisplayValue::Known(value)
            }
        }
        (FacetStatus::Ready | FacetStatus::Failed(_), None) => DisplayValue::Unknown,
    }
}

fn model_is_unavailable(runtime: &RuntimeState) -> bool {
    let session_has_model = runtime
        .session
        .data
        .as_ref()
        .and_then(|session| session.model.as_ref())
        .is_some();
    if session_has_model {
        return false;
    }
    !matches!(runtime.models.status, FacetStatus::Loading)
}

fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted
}

fn format_cost(cost: f64) -> String {
    if cost < 0.01 {
        format!("${cost:.4}")
    } else {
        format!("${cost:.2}")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::services::rpc::{ConnectionGeneration, SessionId};

    #[test]
    fn recovery_actions_have_distinct_keyboard_paths() {
        assert_eq!(RecoveryAction::Connect.shortcut(), "Ctrl+Alt+C");
        assert_eq!(RecoveryAction::Retry.shortcut(), "Ctrl+Alt+R");
        assert_eq!(RecoveryAction::Stop.shortcut(), "Ctrl+Alt+S");
    }

    #[test]
    fn unavailable_context_is_unknown_not_zero() {
        let mut runtime = RuntimeState::default();
        runtime.stats.ready(RuntimeStats {
            session_id: SessionId::from("session"),
            user_messages: 0,
            assistant_messages: 0,
            tool_calls: 0,
            tool_results: 0,
            total_messages: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            total_tokens: 0,
            cost: 0.0,
            context_tokens: None,
            context_window: Some(100_000),
            context_percent: None,
        });

        let projection = ShellProjection::from_runtime(
            ControllerStatus::Active,
            "C:\\workspace",
            &runtime,
            None,
        );
        assert_eq!(projection.context, DisplayValue::Unknown);
        assert_ne!(projection.context.label(), "0");
    }

    #[test]
    fn checked_missing_model_exposes_only_retry_recovery() {
        let mut runtime = RuntimeState::new(ConnectionGeneration::new(1));
        runtime.session.ready(runtime::SessionSnapshot {
            id: SessionId::from("session"),
            file: None,
            name: None,
            model: None,
            thinking_level: RuntimeThinkingLevel::Off,
            steering_mode: runtime::QueueDeliveryMode::All,
            follow_up_mode: runtime::QueueDeliveryMode::All,
            auto_compaction_enabled: true,
            message_count: 0,
        });
        runtime.models.ready(Arc::new(Vec::new()));
        runtime.lifecycle = RuntimeLifecycle::Ready;

        let projection =
            ShellProjection::from_runtime(ControllerStatus::Active, "workspace", &runtime, None);
        assert!(projection.no_model);
        assert_eq!(projection.action, Some(RecoveryAction::Retry));
        assert!(projection.detail.contains("Configure credentials in Pi"));
    }

    #[test]
    fn failed_optional_stats_keep_prior_values_and_ready_connection() {
        let mut runtime = RuntimeState {
            lifecycle: RuntimeLifecycle::Ready,
            ..RuntimeState::default()
        };
        runtime.stats.ready(RuntimeStats {
            session_id: SessionId::from("session"),
            user_messages: 1,
            assistant_messages: 1,
            tool_calls: 0,
            tool_results: 0,
            total_messages: 2,
            input_tokens: 120,
            output_tokens: 40,
            cache_read_tokens: 20,
            cache_write_tokens: 5,
            total_tokens: 185,
            cost: 0.12,
            context_tokens: Some(160),
            context_window: Some(1_000),
            context_percent: Some(16.0),
        });
        runtime.stats.failed(runtime::SafeError::new(
            runtime::ErrorKind::OptionalFacet,
            "Statistics are unavailable",
        ));

        let projection = ShellProjection::from_runtime(
            ControllerStatus::Active,
            "C:\\workspace",
            &runtime,
            None,
        );
        assert_eq!(projection.lifecycle, "Ready");
        assert_eq!(
            projection.input_tokens,
            DisplayValue::Stale("120".to_owned())
        );
        assert_eq!(projection.cost, DisplayValue::Stale("$0.12".to_owned()));
        assert!(projection.has_stale_values);
    }
}
