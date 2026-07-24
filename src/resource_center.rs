//! UI-independent resource inventory and Resource Center filtering.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Extension,
    Tool,
    Skill,
    Prompt,
    Theme,
    Package,
    Context,
    Provider,
}

impl ResourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Extension => "Extension",
            Self::Tool => "Tool",
            Self::Skill => "Skill",
            Self::Prompt => "Prompt",
            Self::Theme => "Theme",
            Self::Package => "Package",
            Self::Context => "Context",
            Self::Provider => "Provider",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceScope {
    Global,
    Project,
    Package,
    Temporary,
}

impl ResourceScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Project => "Project",
            Self::Package => "Package",
            Self::Temporary => "Temporary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceLoadState {
    Loaded,
    Disabled,
    Error,
}

impl ResourceLoadState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Loaded => "Loaded",
            Self::Disabled => "Disabled",
            Self::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceTrust {
    Trusted,
    Rejected,
    NotApplicable,
}

impl ResourceTrust {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trusted => "Trusted",
            Self::Rejected => "Trust rejected",
            Self::NotApplicable => "Not applicable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceItem {
    pub id: String,
    pub kind: ResourceKind,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub state: ResourceLoadState,
    pub scope: ResourceScope,
    #[serde(default)]
    pub owner_scope: Option<String>,
    pub trust: ResourceTrust,
    #[serde(default)]
    pub path: Option<String>,
    pub source: String,
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub filtered: Option<bool>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSettingsSnapshot {
    pub enable_skill_commands: bool,
    #[serde(default)]
    pub theme: Option<String>,
    pub default_project_trust: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageMutationPolicy {
    pub install: bool,
    pub remove: bool,
    pub update: bool,
    pub configure: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInventorySnapshot {
    pub generation: u64,
    pub project_trusted: bool,
    pub project_trust_reason: String,
    pub items: Vec<ResourceItem>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
    pub settings: ResourceSettingsSnapshot,
    pub package_mutations: PackageMutationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourcePhase {
    Loading,
    Ready,
    Refreshing,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceCenterState {
    pub phase: ResourcePhase,
    pub snapshot: Option<Arc<ResourceInventorySnapshot>>,
    pub feedback: Option<String>,
    next_operation: u64,
}

impl Default for ResourceCenterState {
    fn default() -> Self {
        Self {
            phase: ResourcePhase::Loading,
            snapshot: None,
            feedback: None,
            next_operation: 1_000_000,
        }
    }
}

impl ResourceCenterState {
    pub fn take_operation(&mut self) -> u64 {
        let operation = self.next_operation;
        self.next_operation = self.next_operation.saturating_add(1);
        operation
    }

    pub fn begin_refresh(&mut self) -> Option<u64> {
        if matches!(self.phase, ResourcePhase::Refreshing) {
            return None;
        }
        let operation = self.take_operation();
        self.phase = ResourcePhase::Refreshing;
        self.feedback = Some("Reloading installed resources…".to_owned());
        Some(operation)
    }

    pub fn apply_snapshot(&mut self, snapshot: ResourceInventorySnapshot) {
        self.phase = ResourcePhase::Ready;
        self.snapshot = Some(Arc::new(snapshot));
        self.feedback = None;
    }

    pub fn apply_failure(&mut self, summary: String) {
        self.phase = ResourcePhase::Failed(summary.clone());
        self.feedback = Some(summary);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceScopeFilter {
    All,
    Global,
    Project,
    Package,
}

impl ResourceScopeFilter {
    pub fn matches(self, item: &ResourceItem) -> bool {
        match self {
            Self::All => true,
            Self::Global => item.scope == ResourceScope::Global,
            Self::Project => item.scope == ResourceScope::Project,
            Self::Package => item.scope == ResourceScope::Package,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceStateFilter {
    All,
    Loaded,
    Disabled,
    Error,
}

impl ResourceStateFilter {
    pub fn matches(self, item: &ResourceItem) -> bool {
        match self {
            Self::All => true,
            Self::Loaded => item.state == ResourceLoadState::Loaded,
            Self::Disabled => item.state == ResourceLoadState::Disabled,
            Self::Error => item.state == ResourceLoadState::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ResourceItem, ResourceKind, ResourceLoadState, ResourceScope, ResourceScopeFilter,
        ResourceStateFilter, ResourceTrust,
    };

    fn item(scope: ResourceScope, state: ResourceLoadState) -> ResourceItem {
        ResourceItem {
            id: "item".to_owned(),
            kind: ResourceKind::Skill,
            name: "Example".to_owned(),
            description: None,
            state,
            scope,
            owner_scope: None,
            trust: ResourceTrust::Trusted,
            path: None,
            source: "test".to_owned(),
            origin: None,
            active: None,
            pinned: None,
            filtered: None,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn scope_and_state_filters_are_independent() {
        let project_error = item(ResourceScope::Project, ResourceLoadState::Error);
        assert!(ResourceScopeFilter::Project.matches(&project_error));
        assert!(!ResourceScopeFilter::Global.matches(&project_error));
        assert!(ResourceStateFilter::Error.matches(&project_error));
        assert!(!ResourceStateFilter::Loaded.matches(&project_error));
    }
}
