//! Secret-free model catalog, authentication, and thinking projections.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelIdentity {
    pub provider: String,
    pub id: String,
}

impl ModelIdentity {
    pub fn display(&self) -> String {
        format!("{}/{}", self.provider, self.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

impl ThinkingLevel {
    pub const ALL: [Self; 7] = [
        Self::Off,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Xhigh,
        Self::Max,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Minimal => "Minimal",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Xhigh => "Xhigh",
            Self::Max => "Max",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingRates {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl PricingRates {
    pub fn is_zero(&self) -> bool {
        self.input == 0.0 && self.output == 0.0 && self.cache_read == 0.0 && self.cache_write == 0.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingTier {
    pub input_tokens_above: u64,
    #[serde(flatten)]
    pub rates: PricingRates,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    #[serde(flatten)]
    pub rates: PricingRates,
    #[serde(default)]
    pub tiers: Vec<PricingTier>,
}

impl ModelPricing {
    pub fn rates_for_input_tokens(&self, input_tokens: u64) -> &PricingRates {
        self.tiers
            .iter()
            .filter(|tier| input_tokens > tier.input_tokens_above)
            .max_by_key(|tier| tier.input_tokens_above)
            .map(|tier| &tier.rates)
            .unwrap_or(&self.rates)
    }

    pub fn label(&self) -> String {
        if self.rates.is_zero() && self.tiers.iter().all(|tier| tier.rates.is_zero()) {
            return "Pricing not published".to_owned();
        }
        let tier_suffix = if self.tiers.is_empty() {
            String::new()
        } else {
            format!(" · {} tier{}", self.tiers.len(), plural(self.tiers.len()))
        };
        format!(
            "${:.2} in / ${:.2} out per 1M{}",
            self.rates.input, self.rates.output, tier_suffix
        )
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogEntry {
    #[serde(flatten)]
    pub identity: ModelIdentity,
    pub name: String,
    pub api: String,
    pub reasoning: bool,
    pub supports_images: bool,
    pub context_window: u64,
    pub max_tokens: u64,
    pub pricing: ModelPricing,
    #[serde(default)]
    pub supported_thinking: Vec<ThinkingLevel>,
    #[serde(default)]
    pub thinking_level_map: BTreeMap<String, Option<String>>,
    pub available: bool,
}

impl ModelCatalogEntry {
    pub fn search_matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.name.to_lowercase().contains(&query)
            || self.identity.provider.to_lowercase().contains(&query)
            || self.identity.id.to_lowercase().contains(&query)
            || self.api.to_lowercase().contains(&query)
    }

    pub fn clamp_thinking(&self, requested: ThinkingLevel) -> ThinkingLevel {
        if self.supported_thinking.contains(&requested) {
            return requested;
        }
        let requested_index = ThinkingLevel::ALL
            .iter()
            .position(|level| *level == requested)
            .unwrap_or(0);
        ThinkingLevel::ALL[requested_index..]
            .iter()
            .chain(ThinkingLevel::ALL[..requested_index].iter().rev())
            .find(|level| self.supported_thinking.contains(level))
            .copied()
            .unwrap_or(ThinkingLevel::Off)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    ApiKey,
    Oauth,
}

impl AuthMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::ApiKey => "API key",
            Self::Oauth => "Browser or device login",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthSource {
    Stored,
    Runtime,
    Environment,
    Fallback,
    ModelsJson,
    Unknown,
}

impl AuthSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stored => "Stored by Pi",
            Self::Runtime => "Runtime only",
            Self::Environment => "Environment",
            Self::Fallback => "Provider fallback",
            Self::ModelsJson => "models.json",
            Self::Unknown => "Configured",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuthStatus {
    pub configured: bool,
    pub source: Option<AuthSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub auth_methods: Vec<AuthMethod>,
    pub auth: ProviderAuthStatus,
    pub model_count: usize,
    pub available_model_count: usize,
    pub refresh_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefaults {
    pub model: Option<ModelIdentity>,
    pub thinking: Option<ThinkingLevel>,
    #[serde(default)]
    pub scoped_models: Vec<ModelIdentity>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogSnapshot {
    #[serde(default)]
    pub providers: Vec<ProviderCatalogEntry>,
    #[serde(default)]
    pub models: Vec<ModelCatalogEntry>,
    pub defaults: ModelDefaults,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

impl ModelCatalogSnapshot {
    pub fn model(&self, identity: &ModelIdentity) -> Option<&ModelCatalogEntry> {
        self.models.iter().find(|model| model.identity == *identity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogPhase {
    Loading,
    Ready,
    Refreshing,
    Stale(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRuntimeState {
    pub phase: CatalogPhase,
    pub catalog: Option<ModelCatalogSnapshot>,
    pub active_refresh: Option<u64>,
    pub next_operation: u64,
    pub auth: Option<AuthFlow>,
    pub feedback: Option<String>,
}

impl Default for ModelRuntimeState {
    fn default() -> Self {
        Self {
            phase: CatalogPhase::Loading,
            catalog: None,
            active_refresh: None,
            next_operation: 1_000_000,
            auth: None,
            feedback: None,
        }
    }
}

impl ModelRuntimeState {
    pub fn begin_refresh(&mut self) -> (u64, bool) {
        if let Some(operation) = self.active_refresh {
            return (operation, false);
        }
        let operation = self.take_operation();
        self.active_refresh = Some(operation);
        self.phase = CatalogPhase::Refreshing;
        (operation, true)
    }

    pub fn apply_refresh(
        &mut self,
        operation: u64,
        result: Result<ModelCatalogSnapshot, String>,
    ) -> bool {
        if self.active_refresh != Some(operation) {
            return false;
        }
        self.active_refresh = None;
        match result {
            Ok(catalog) => {
                let has_errors = catalog
                    .providers
                    .iter()
                    .any(|provider| provider.refresh_error.is_some());
                self.catalog = Some(catalog);
                self.phase = if has_errors {
                    CatalogPhase::Stale(
                        "Some providers could not refresh; cached models remain available."
                            .to_owned(),
                    )
                } else {
                    CatalogPhase::Ready
                };
            }
            Err(summary) if self.catalog.is_some() => {
                self.phase = CatalogPhase::Stale(summary);
            }
            Err(summary) => self.phase = CatalogPhase::Failed(summary),
        }
        true
    }

    pub fn apply_snapshot(&mut self, catalog: ModelCatalogSnapshot) {
        self.catalog = Some(catalog);
        self.phase = CatalogPhase::Ready;
    }

    pub fn start_auth(&mut self, provider: String, method: AuthMethod) -> u64 {
        let operation = self.take_operation();
        self.auth = Some(AuthFlow {
            operation,
            provider,
            method,
            stage: AuthStage::Starting,
        });
        operation
    }

    pub fn apply_auth_event(&mut self, event: AuthEvent) -> bool {
        let Some(flow) = self.auth.as_mut() else {
            return false;
        };
        if flow.operation != event.operation() {
            return false;
        }
        flow.stage = match event {
            AuthEvent::AuthInfo { message, links, .. } => AuthStage::Info { message, links },
            AuthEvent::AuthUrl {
                url, instructions, ..
            } => AuthStage::Browser { url, instructions },
            AuthEvent::AuthDeviceCode {
                user_code,
                verification_uri,
                expires_in_seconds,
                ..
            } => AuthStage::DeviceCode {
                user_code,
                verification_uri,
                expires_in_seconds,
            },
            AuthEvent::AuthProgress { message, .. } => AuthStage::Progress { message },
            AuthEvent::AuthPrompt { prompt, .. } => AuthStage::Prompt(prompt),
        };
        true
    }

    pub fn finish_auth(&mut self, operation: u64, result: Result<(), String>) -> bool {
        if self.auth.as_ref().map(|flow| flow.operation) != Some(operation) {
            return false;
        }
        let Some(flow) = self.auth.take() else {
            return false;
        };
        self.feedback = Some(match result {
            Ok(()) => format!("{} authentication updated.", flow.provider),
            Err(summary) => format!("{} authentication failed. {summary}", flow.provider),
        });
        true
    }

    pub fn cancel_auth(&mut self, operation: u64) -> bool {
        if self.auth.as_ref().map(|flow| flow.operation) != Some(operation) {
            return false;
        }
        if let Some(flow) = self.auth.as_mut() {
            flow.stage = AuthStage::Cancelling;
        }
        true
    }

    pub fn take_operation(&mut self) -> u64 {
        let operation = self.next_operation;
        self.next_operation = self.next_operation.saturating_add(1);
        operation
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthFlow {
    pub operation: u64,
    pub provider: String,
    pub method: AuthMethod,
    pub stage: AuthStage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStage {
    Starting,
    Info {
        message: String,
        links: Vec<AuthLink>,
    },
    Browser {
        url: String,
        instructions: Option<String>,
    },
    DeviceCode {
        user_code: String,
        verification_uri: String,
        expires_in_seconds: Option<u64>,
    },
    Progress {
        message: String,
    },
    Prompt(AuthPrompt),
    Cancelling,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthLink {
    pub url: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthPromptKind {
    Text,
    Secret,
    Select,
    ManualCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthPromptOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthPrompt {
    pub prompt_id: String,
    pub kind: AuthPromptKind,
    pub message: String,
    pub placeholder: Option<String>,
    #[serde(default)]
    pub options: Vec<AuthPromptOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AuthEvent {
    AuthInfo {
        #[serde(rename = "operationId")]
        operation: u64,
        message: String,
        #[serde(default)]
        links: Vec<AuthLink>,
    },
    AuthUrl {
        #[serde(rename = "operationId")]
        operation: u64,
        url: String,
        instructions: Option<String>,
    },
    AuthDeviceCode {
        #[serde(rename = "operationId")]
        operation: u64,
        #[serde(rename = "userCode")]
        user_code: String,
        #[serde(rename = "verificationUri")]
        verification_uri: String,
        #[serde(rename = "expiresInSeconds")]
        expires_in_seconds: Option<u64>,
    },
    AuthProgress {
        #[serde(rename = "operationId")]
        operation: u64,
        message: String,
    },
    AuthPrompt {
        #[serde(rename = "operationId")]
        operation: u64,
        prompt: AuthPrompt,
    },
}

impl AuthEvent {
    pub fn operation(&self) -> u64 {
        match self {
            Self::AuthInfo { operation, .. }
            | Self::AuthUrl { operation, .. }
            | Self::AuthDeviceCode { operation, .. }
            | Self::AuthProgress { operation, .. }
            | Self::AuthPrompt { operation, .. } => *operation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelChangePolicy {
    Allowed,
    WaitUntilIdle,
    RuntimeUnavailable,
}

pub fn model_change_policy(runtime_available: bool, streaming: bool) -> ModelChangePolicy {
    if !runtime_available {
        ModelChangePolicy::RuntimeUnavailable
    } else if streaming {
        ModelChangePolicy::WaitUntilIdle
    } else {
        ModelChangePolicy::Allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(levels: Vec<ThinkingLevel>) -> ModelCatalogEntry {
        ModelCatalogEntry {
            identity: ModelIdentity {
                provider: "p".to_owned(),
                id: "m".to_owned(),
            },
            name: "Model".to_owned(),
            api: "synthetic".to_owned(),
            reasoning: true,
            supports_images: false,
            context_window: 1,
            max_tokens: 1,
            pricing: ModelPricing {
                rates: PricingRates {
                    input: 0.0,
                    output: 0.0,
                    cache_read: 0.0,
                    cache_write: 0.0,
                },
                tiers: Vec::new(),
            },
            supported_thinking: levels,
            thinking_level_map: BTreeMap::new(),
            available: true,
        }
    }

    #[test]
    fn sparse_thinking_clamps_up_then_down_without_inventing_levels() {
        let model = model(vec![
            ThinkingLevel::Off,
            ThinkingLevel::Low,
            ThinkingLevel::High,
        ]);
        assert_eq!(
            model.clamp_thinking(ThinkingLevel::Minimal),
            ThinkingLevel::Low
        );
        assert_eq!(
            model.clamp_thinking(ThinkingLevel::Max),
            ThinkingLevel::High
        );
    }

    #[test]
    fn zero_rates_are_unpriced_not_free() {
        assert_eq!(
            model(vec![ThinkingLevel::Off]).pricing.label(),
            "Pricing not published"
        );
    }

    #[test]
    fn pricing_uses_the_highest_strictly_exceeded_input_tier() {
        let pricing = ModelPricing {
            rates: PricingRates {
                input: 1.0,
                output: 2.0,
                cache_read: 0.1,
                cache_write: 1.25,
            },
            tiers: vec![
                PricingTier {
                    input_tokens_above: 100,
                    rates: PricingRates {
                        input: 3.0,
                        output: 4.0,
                        cache_read: 0.2,
                        cache_write: 2.0,
                    },
                },
                PricingTier {
                    input_tokens_above: 200,
                    rates: PricingRates {
                        input: 5.0,
                        output: 6.0,
                        cache_read: 0.3,
                        cache_write: 3.0,
                    },
                },
            ],
        };
        assert_eq!(pricing.rates_for_input_tokens(100).input, 1.0);
        assert_eq!(pricing.rates_for_input_tokens(101).input, 3.0);
        assert_eq!(pricing.rates_for_input_tokens(201).input, 5.0);
    }

    #[test]
    fn concurrent_refreshes_coalesce_and_cached_catalog_survives_failure() {
        let mut state = ModelRuntimeState::default();
        state.apply_snapshot(ModelCatalogSnapshot::default());
        let (first, started) = state.begin_refresh();
        let (second, duplicate_started) = state.begin_refresh();
        assert!(started);
        assert!(!duplicate_started);
        assert_eq!(first, second);
        assert!(state.apply_refresh(first, Err("Refresh failed.".to_owned())));
        assert!(state.catalog.is_some());
        assert!(matches!(state.phase, CatalogPhase::Stale(_)));
    }

    #[test]
    fn auth_prompt_state_machine_rejects_stale_events_and_cancels() {
        let mut state = ModelRuntimeState::default();
        let operation = state.start_auth("provider".to_owned(), AuthMethod::Oauth);
        assert!(!state.apply_auth_event(AuthEvent::AuthProgress {
            operation: operation + 1,
            message: "stale".to_owned(),
        }));
        assert!(state.apply_auth_event(AuthEvent::AuthPrompt {
            operation,
            prompt: AuthPrompt {
                prompt_id: "prompt".to_owned(),
                kind: AuthPromptKind::Select,
                message: "Choose".to_owned(),
                placeholder: None,
                options: Vec::new(),
            },
        }));
        assert!(matches!(
            state.auth.as_ref().map(|flow| &flow.stage),
            Some(AuthStage::Prompt(_))
        ));
        assert!(state.cancel_auth(operation));
        assert!(matches!(
            state.auth.as_ref().map(|flow| &flow.stage),
            Some(AuthStage::Cancelling)
        ));
    }

    #[test]
    fn streaming_model_changes_wait_for_an_idle_boundary() {
        assert_eq!(
            model_change_policy(true, true),
            ModelChangePolicy::WaitUntilIdle
        );
        assert_eq!(model_change_policy(true, false), ModelChangePolicy::Allowed);
    }
}
