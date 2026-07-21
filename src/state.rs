//! UI-independent placeholder domain for the harness desk.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Idle,
    Thinking,
    Tooling,
    Waiting,
    Blocked,
}

impl RunStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Thinking => "Thinking",
            Self::Tooling => "Running tools",
            Self::Waiting => "Waiting",
            Self::Blocked => "Blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Blocked,
    Done,
}

impl TaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Running => "Running",
            Self::Blocked => "Blocked",
            Self::Done => "Done",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentKind {
    Explore,
    Plan,
    General,
}

impl SubagentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Explore => "Explore",
            Self::Plan => "Plan",
            Self::General => "General",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: &'static str,
    pub name: &'static str,
    pub project: &'static str,
    pub updated: &'static str,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceItem {
    pub name: &'static str,
    pub kind: &'static str,
    pub scope: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEntry {
    User {
        body: &'static str,
        timestamp: &'static str,
    },
    Assistant {
        body: &'static str,
        timestamp: &'static str,
    },
    Thinking {
        body: &'static str,
        level: &'static str,
    },
    Tool {
        name: &'static str,
        body: &'static str,
        summary: &'static str,
    },
    System {
        title: &'static str,
        body: &'static str,
    },
    Compaction {
        body: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskItem {
    pub id: &'static str,
    pub subject: &'static str,
    pub detail: &'static str,
    pub status: TaskStatus,
    pub owner: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentItem {
    pub id: &'static str,
    pub kind: SubagentKind,
    pub brief: &'static str,
    pub status: RunStatus,
    pub turns: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    pub mode: &'static str,
    pub preview: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelStrip {
    pub provider: &'static str,
    pub model: &'static str,
    pub thinking: &'static str,
    pub context_used_pct: f32,
    pub context_label: &'static str,
    pub tokens_in: &'static str,
    pub tokens_out: &'static str,
    pub cost: &'static str,
    pub cache: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideSection {
    Sessions,
    Skills,
    Extensions,
}

impl SideSection {
    pub const ALL: [Self; 3] = [Self::Sessions, Self::Skills, Self::Extensions];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sessions => "Sessions",
            Self::Skills => "Skills",
            Self::Extensions => "Extensions",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardState {
    pub session_name: &'static str,
    pub cwd: &'static str,
    pub run_status: RunStatus,
    pub branch_label: &'static str,
    pub sessions: Vec<SessionSummary>,
    pub skills: Vec<ResourceItem>,
    pub extensions: Vec<ResourceItem>,
    pub packages: Vec<ResourceItem>,
    pub stream: Vec<StreamEntry>,
    pub tasks: Vec<TaskItem>,
    pub subagents: Vec<SubagentItem>,
    pub queue: Vec<QueueItem>,
    pub model: ModelStrip,
    pub side_section: SideSection,
    pub selected_task_id: Option<&'static str>,
    pub composer: String,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::placeholder()
    }
}

impl DashboardState {
    pub fn placeholder() -> Self {
        Self {
            session_name: "Review auth middleware",
            cwd: r"C:\workspace\pi-gui",
            run_status: RunStatus::Tooling,
            branch_label: "main",
            sessions: vec![
                SessionSummary {
                    id: "s-8841",
                    name: "Review auth middleware",
                    project: "pi-gui",
                    updated: "Now",
                    active: true,
                },
                SessionSummary {
                    id: "s-7710",
                    name: "Fix session resume",
                    project: "pi-gui",
                    updated: "2h",
                    active: false,
                },
                SessionSummary {
                    id: "s-6602",
                    name: "Tree navigation",
                    project: "pi-mono",
                    updated: "Yesterday",
                    active: false,
                },
                SessionSummary {
                    id: "s-5509",
                    name: "Extension host",
                    project: "pi-gui",
                    updated: "Mon",
                    active: false,
                },
            ],
            skills: vec![
                ResourceItem {
                    name: "slop",
                    kind: "skill",
                    scope: "user",
                },
                ResourceItem {
                    name: "feature-suggest",
                    kind: "skill",
                    scope: "user",
                },
                ResourceItem {
                    name: "apple-design",
                    kind: "skill",
                    scope: "user",
                },
            ],
            extensions: vec![
                ResourceItem {
                    name: "git-checkpoint",
                    kind: "ext",
                    scope: "project",
                },
                ResourceItem {
                    name: "path-guard",
                    kind: "ext",
                    scope: "user",
                },
                ResourceItem {
                    name: "summarize",
                    kind: "ext",
                    scope: "user",
                },
            ],
            packages: vec![
                ResourceItem {
                    name: "@pi/devtools",
                    kind: "pkg",
                    scope: "global",
                },
                ResourceItem {
                    name: "@pi/review-kit",
                    kind: "pkg",
                    scope: "project",
                },
            ],
            stream: vec![
                StreamEntry::System {
                    title: "Session ready",
                    body: "Loaded project context, 3 skills, and 3 extensions.",
                },
                StreamEntry::User {
                    body: "Walk the auth middleware and flag anything that can drop a valid session on refresh.",
                    timestamp: "12:41",
                },
                StreamEntry::Thinking {
                    body: "Session drops on refresh usually mean restore runs before refresh settles, or the guard treats a mid-refresh token as invalid. Start at restore_session and the 401 path.",
                    level: "medium",
                },
                StreamEntry::Tool {
                    name: "read",
                    body: "src/auth/session.rs\nsrc/auth/refresh.rs\nsrc/middleware/auth.rs",
                    summary: "3 files",
                },
                StreamEntry::Tool {
                    name: "grep",
                    body: "restore_session\nrefresh_access_token\nUnauthorized",
                    summary: "12 hits",
                },
                StreamEntry::Tool {
                    name: "explore",
                    body: "Map callers of restore_session and the cookie write path before the guard.",
                    summary: "done",
                },
                StreamEntry::Thinking {
                    body: "auth.rs returns 401 when access is expired even if a refresh is already in flight. A second tab hits that branch and clears the session. Fix: await the shared refresh future, then re-check once.",
                    level: "medium",
                },
                StreamEntry::Tool {
                    name: "edit",
                    body: "src/middleware/auth.rs\n@@ fn guard_request\n- return Err(AuthError::Unauthorized);\n+ let session = refresh_lock.wait().await?;\n+ recheck_session(&session)?;",
                    summary: "+2 -1",
                },
                StreamEntry::Assistant {
                    body: "There is a race in the stale-token branch: it returns 401 before an in-flight refresh can finish.\n\nI patched the guard to await the shared refresh future, then re-check the session once. A reload during refresh should no longer drop a valid user.",
                    timestamp: "12:43",
                },
            ],
            tasks: vec![
                TaskItem {
                    id: "t-1",
                    subject: "Trace session restore",
                    detail: "cookie → restore_session → guard",
                    status: TaskStatus::Done,
                    owner: Some("explore"),
                },
                TaskItem {
                    id: "t-2",
                    subject: "Fix refresh race",
                    detail: "await in-flight refresh before 401",
                    status: TaskStatus::Running,
                    owner: Some("main"),
                },
                TaskItem {
                    id: "t-3",
                    subject: "Add regression test",
                    detail: "concurrent refresh + page reload",
                    status: TaskStatus::Pending,
                    owner: None,
                },
                TaskItem {
                    id: "t-4",
                    subject: "Verify cookie flags",
                    detail: "Secure / HttpOnly / SameSite",
                    status: TaskStatus::Blocked,
                    owner: None,
                },
            ],
            subagents: vec![
                SubagentItem {
                    id: "a-21",
                    kind: SubagentKind::Explore,
                    brief: "Map restore_session call graph",
                    status: RunStatus::Tooling,
                    turns: 4,
                },
                SubagentItem {
                    id: "a-22",
                    kind: SubagentKind::Plan,
                    brief: "Outline fix and test cases",
                    status: RunStatus::Waiting,
                    turns: 2,
                },
            ],
            queue: vec![
                QueueItem {
                    mode: "steer",
                    preview: "Prefer the smaller patch in refresh.rs",
                },
                QueueItem {
                    mode: "follow-up",
                    preview: "Summarize residual risk after the fix",
                },
            ],
            model: ModelStrip {
                provider: "Anthropic",
                model: "claude-sonnet-4-5",
                thinking: "Medium",
                context_used_pct: 0.62,
                context_label: "124k / 200k",
                tokens_in: "86.2k",
                tokens_out: "12.4k",
                cost: "$1.84",
                cache: "41k hit",
            },
            side_section: SideSection::Sessions,
            selected_task_id: Some("t-2"),
            composer: String::new(),
        }
    }

    pub fn select_side(&mut self, section: SideSection) {
        self.side_section = section;
    }

    pub fn select_session(&mut self, id: &'static str) -> bool {
        let Some(session_name) = self
            .sessions
            .iter()
            .find(|session| session.id == id)
            .map(|session| session.name)
        else {
            return false;
        };

        for session in &mut self.sessions {
            session.active = session.id == id;
        }
        self.session_name = session_name;
        true
    }

    pub fn select_task(&mut self, id: &'static str) -> bool {
        if !self.tasks.iter().any(|task| task.id == id) {
            return false;
        }

        self.selected_task_id = Some(id);
        true
    }

    pub fn cycle_run_status(&mut self) {
        self.run_status = match self.run_status {
            RunStatus::Idle => RunStatus::Thinking,
            RunStatus::Thinking => RunStatus::Tooling,
            RunStatus::Tooling => RunStatus::Waiting,
            RunStatus::Waiting => RunStatus::Blocked,
            RunStatus::Blocked => RunStatus::Idle,
        };
    }

    pub fn active_task_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| {
                matches!(
                    task.status,
                    TaskStatus::Running | TaskStatus::Pending | TaskStatus::Blocked
                )
            })
            .count()
    }

    pub fn live_subagent_count(&self) -> usize {
        self.subagents
            .iter()
            .filter(|agent| !matches!(agent.status, RunStatus::Idle))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_has_core_surfaces() {
        let state = DashboardState::placeholder();
        assert!(!state.sessions.is_empty());
        assert!(!state.stream.is_empty());
        assert!(!state.tasks.is_empty());
        assert!(!state.subagents.is_empty());
    }

    #[test]
    fn placeholder_selection_is_consistent() {
        let state = DashboardState::placeholder();
        let active_sessions: Vec<_> = state
            .sessions
            .iter()
            .filter(|session| session.active)
            .collect();

        assert_eq!(active_sessions.len(), 1);
        assert_eq!(active_sessions[0].name, state.session_name);
        assert!(
            state
                .selected_task_id
                .is_some_and(|id| state.tasks.iter().any(|task| task.id == id))
        );
    }

    #[test]
    fn selections_and_status_update() {
        let mut state = DashboardState::placeholder();
        assert!(state.select_session("s-7710"));
        assert_eq!(state.session_name, "Fix session resume");
        assert_eq!(
            state
                .sessions
                .iter()
                .filter(|session| session.active)
                .count(),
            1
        );

        assert!(state.select_task("t-3"));
        assert_eq!(state.selected_task_id, Some("t-3"));

        state.select_side(SideSection::Skills);
        assert_eq!(state.side_section, SideSection::Skills);

        let status = state.run_status;
        state.cycle_run_status();
        assert_ne!(state.run_status, status);
    }

    #[test]
    fn invalid_selection_preserves_current_state() {
        let mut state = DashboardState::placeholder();
        let session_name = state.session_name;
        let selected_task = state.selected_task_id;

        assert!(!state.select_session("missing"));
        assert!(!state.select_task("missing"));
        assert_eq!(state.session_name, session_name);
        assert_eq!(state.selected_task_id, selected_task);
        assert_eq!(
            state
                .sessions
                .iter()
                .filter(|session| session.active)
                .count(),
            1
        );
    }
}
