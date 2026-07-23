//! Typed snapshots and guarded actions for Pi-owned task, subagent, and goal state.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestrationPhase {
    Loading,
    Ready,
    Empty,
    Stale,
    Error,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationSnapshot {
    pub session_id: String,
    pub generation: u64,
    pub captured_at: u64,
    #[serde(default)]
    pub tasks: Vec<TaskSnapshot>,
    #[serde(default)]
    pub subagents: Vec<SubagentSnapshot>,
    #[serde(default)]
    pub schedules: Vec<SubagentScheduleSnapshot>,
    pub goal: Option<GoalSnapshot>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

impl OrchestrationSnapshot {
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
            && self.subagents.is_empty()
            && self.schedules.is_empty()
            && self.goal.is_none()
    }

    pub fn task(&self, id: &str) -> Option<&TaskSnapshot> {
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn subagent(&self, id: &str) -> Option<&SubagentSnapshot> {
        self.subagents.iter().find(|agent| agent.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl TaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::InProgress => "Running",
            Self::Completed => "Done",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub status: TaskStatus,
    pub active_form: Option<String>,
    pub owner: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub output: Option<String>,
}

impl TaskSnapshot {
    pub fn open_blockers<'a>(&'a self, tasks: &'a HashMap<&str, &'a TaskSnapshot>) -> Vec<&'a str> {
        self.blocked_by
            .iter()
            .filter_map(|id| {
                tasks
                    .get(id.as_str())
                    .is_none_or(|task| task.status != TaskStatus::Completed)
                    .then_some(id.as_str())
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Queued,
    Running,
    Completed,
    Steered,
    Aborted,
    Stopped,
    Error,
}

impl SubagentStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Completed => "Done",
            Self::Steered => "Wrapped up",
            Self::Aborted => "Aborted",
            Self::Stopped => "Stopped",
            Self::Error => "Failed",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSnapshot {
    pub id: String,
    #[serde(rename = "type")]
    pub agent_type: String,
    pub description: String,
    pub status: SubagentStatus,
    pub result: Option<String>,
    pub error: Option<String>,
    pub tool_uses: u64,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub queue_position: Option<u64>,
    pub max_concurrent: u64,
    pub output_file: Option<String>,
    #[serde(default)]
    pub pending_steers: Vec<String>,
    pub worktree: Option<WorktreeSnapshot>,
    pub worktree_result: Option<WorktreeResultSnapshot>,
    pub memory: Option<MemorySnapshot>,
    #[serde(default)]
    pub transcript: Vec<SubagentTranscriptEntry>,
    pub transcript_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSnapshot {
    pub path: String,
    pub branch: String,
    pub base_sha: String,
    pub work_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeResultSnapshot {
    pub has_changes: bool,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySnapshot {
    pub scope: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    User,
    Assistant,
    ToolResult,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentTranscriptEntry {
    pub role: TranscriptRole,
    pub content: String,
    pub timestamp: Option<String>,
    pub tool_name: Option<String>,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentScheduleSnapshot {
    pub id: String,
    pub name: String,
    pub description: String,
    pub schedule: String,
    pub schedule_type: String,
    pub subagent_type: String,
    pub enabled: bool,
    pub created_at: String,
    pub last_run: Option<String>,
    pub last_status: Option<String>,
    pub next_run: Option<String>,
    pub run_count: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSnapshot {
    pub active: Option<GoalItemSnapshot>,
    #[serde(default)]
    pub queue: Vec<GoalItemSnapshot>,
    pub pending_action: Option<Value>,
    pub queue_frozen: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalItemSnapshot {
    pub id: String,
    pub objective: String,
    pub status: String,
    pub started_at: u64,
    pub updated_at: u64,
    pub iteration: u64,
    pub token_budget: Option<u64>,
    pub tokens_used: u64,
    pub time_used_seconds: f64,
    pub active_started_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrchestrationState {
    pub phase: OrchestrationPhase,
    pub expected_session_id: Option<String>,
    pub snapshot: Option<OrchestrationSnapshot>,
    pub feedback: Option<String>,
}

impl Default for OrchestrationState {
    fn default() -> Self {
        Self {
            phase: OrchestrationPhase::Loading,
            expected_session_id: None,
            snapshot: None,
            feedback: None,
        }
    }
}

impl OrchestrationState {
    pub fn begin_session(&mut self, session_id: Option<String>) {
        if self.expected_session_id == session_id {
            return;
        }
        self.expected_session_id = session_id;
        self.phase = OrchestrationPhase::Loading;
        self.feedback = None;
        self.snapshot = None;
    }

    pub fn apply_snapshot(&mut self, snapshot: OrchestrationSnapshot) -> bool {
        if self
            .expected_session_id
            .as_deref()
            .is_some_and(|expected| expected != snapshot.session_id)
        {
            self.phase = if self.snapshot.is_some() {
                OrchestrationPhase::Stale
            } else {
                OrchestrationPhase::Loading
            };
            return false;
        }
        if self
            .snapshot
            .as_ref()
            .is_some_and(|current| current.generation > snapshot.generation)
        {
            self.phase = OrchestrationPhase::Stale;
            return false;
        }
        self.phase = if snapshot.is_empty() {
            OrchestrationPhase::Empty
        } else {
            OrchestrationPhase::Ready
        };
        self.feedback = None;
        self.snapshot = Some(snapshot);
        true
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.phase = if self.snapshot.is_some() {
            OrchestrationPhase::Stale
        } else {
            OrchestrationPhase::Error
        };
        self.feedback = Some(message.into());
    }

    pub fn disconnect(&mut self) {
        self.phase = OrchestrationPhase::Disconnected;
        self.feedback = Some(
            "The orchestration adapter disconnected. Last known work remains visible.".to_owned(),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationActionRequest {
    pub session_id: String,
    #[serde(flatten)]
    pub action: OrchestrationAction,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum OrchestrationAction {
    TaskExecute {
        task_ids: Vec<String>,
        additional_context: Option<String>,
        model: Option<String>,
        max_turns: Option<u64>,
        cascade: bool,
    },
    TaskStop {
        task_id: String,
    },
    SubagentSteer {
        agent_id: String,
        message: String,
    },
    SubagentStop {
        agent_id: String,
    },
    SubagentResume {
        agent_id: String,
        prompt: String,
    },
    GoalPause {
        goal_id: String,
    },
    GoalResume {
        goal_id: String,
    },
    GoalEdit {
        goal_id: String,
        objective: String,
        token_budget: Option<u64>,
    },
    GoalClear {
        goal_id: String,
    },
}

pub fn task_cycle_members(tasks: &[TaskSnapshot]) -> HashSet<String> {
    fn visit(
        id: &str,
        edges: &HashMap<&str, Vec<&str>>,
        visiting: &mut Vec<String>,
        visited: &mut HashSet<String>,
        cycles: &mut HashSet<String>,
    ) {
        if let Some(index) = visiting.iter().position(|candidate| candidate == id) {
            cycles.extend(visiting[index..].iter().cloned());
            return;
        }
        if !visited.insert(id.to_owned()) {
            return;
        }
        visiting.push(id.to_owned());
        for next in edges.get(id).into_iter().flatten() {
            visit(next, edges, visiting, visited, cycles);
        }
        visiting.pop();
    }

    let edges = tasks
        .iter()
        .map(|task| {
            (
                task.id.as_str(),
                task.blocked_by.iter().map(String::as_str).collect(),
            )
        })
        .collect::<HashMap<_, Vec<_>>>();
    let mut visiting = Vec::new();
    let mut visited = HashSet::new();
    let mut cycles = HashSet::new();
    for task in tasks {
        visit(&task.id, &edges, &mut visiting, &mut visited, &mut cycles);
    }
    cycles
}

pub fn cascade_ready_tasks<'a>(
    tasks: &'a [TaskSnapshot],
    completed_id: &str,
) -> Vec<&'a TaskSnapshot> {
    let by_id = tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect::<HashMap<_, _>>();
    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::Pending
                && task.blocked_by.iter().any(|id| id == completed_id)
                && task.open_blockers(&by_id).is_empty()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, status: TaskStatus, blocked_by: &[&str]) -> TaskSnapshot {
        TaskSnapshot {
            id: id.to_owned(),
            subject: format!("Task {id}"),
            description: String::new(),
            status,
            active_form: None,
            owner: None,
            metadata: Value::Object(Default::default()),
            blocks: Vec::new(),
            blocked_by: blocked_by.iter().map(|id| (*id).to_owned()).collect(),
            created_at: 1,
            updated_at: 1,
            output: None,
        }
    }

    #[test]
    fn task_graph_detects_cycles_and_cascade_only_after_all_dependencies() {
        let mut tasks = vec![
            task("1", TaskStatus::Completed, &[]),
            task("2", TaskStatus::Pending, &["1", "3"]),
            task("3", TaskStatus::Pending, &[]),
            task("4", TaskStatus::Pending, &["5"]),
            task("5", TaskStatus::Pending, &["4"]),
        ];
        assert_eq!(
            task_cycle_members(&tasks),
            HashSet::from(["4".to_owned(), "5".to_owned()])
        );
        assert!(cascade_ready_tasks(&tasks, "1").is_empty());
        tasks[2].status = TaskStatus::Completed;
        assert_eq!(cascade_ready_tasks(&tasks, "3")[0].id, "2");
    }

    #[test]
    fn stale_session_and_older_generation_cannot_replace_current_snapshot() {
        let mut state = OrchestrationState::default();
        state.begin_session(Some("current".to_owned()));
        let snapshot = |session: &str, generation| OrchestrationSnapshot {
            session_id: session.to_owned(),
            generation,
            captured_at: generation,
            tasks: Vec::new(),
            subagents: Vec::new(),
            schedules: Vec::new(),
            goal: None,
            diagnostics: Vec::new(),
        };
        assert!(!state.apply_snapshot(snapshot("old", 1)));
        assert!(state.apply_snapshot(snapshot("current", 2)));
        assert!(!state.apply_snapshot(snapshot("current", 1)));
        assert_eq!(state.snapshot.as_ref().unwrap().generation, 2);
        assert_eq!(state.phase, OrchestrationPhase::Stale);
    }

    #[test]
    fn bridge_restart_keeps_last_snapshot_stale_and_session_switch_clears_it() {
        let mut state = OrchestrationState::default();
        state.begin_session(Some("first".to_owned()));
        state.apply_snapshot(OrchestrationSnapshot {
            session_id: "first".to_owned(),
            generation: 1,
            captured_at: 1,
            tasks: vec![task("1", TaskStatus::InProgress, &[])],
            subagents: Vec::new(),
            schedules: Vec::new(),
            goal: None,
            diagnostics: Vec::new(),
        });

        state.disconnect();
        assert_eq!(state.phase, OrchestrationPhase::Disconnected);
        assert_eq!(state.snapshot.as_ref().unwrap().tasks[0].id, "1");

        state.begin_session(Some("second".to_owned()));
        assert_eq!(state.phase, OrchestrationPhase::Loading);
        assert!(state.snapshot.is_none());
        assert_eq!(state.expected_session_id.as_deref(), Some("second"));
    }
}
