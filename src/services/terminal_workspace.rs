//! Saved project folders and terminal layouts, independent of agent sessions.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub(crate) const MAX_TERMINAL_TABS: usize = 8;
const WORKSPACE_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TerminalProject {
    pub(crate) path: PathBuf,
    pub(crate) tab_count: usize,
    pub(crate) active_tab: usize,
    #[serde(default)]
    pub(crate) checklist: super::checklist::Checklist,
}

impl TerminalProject {
    fn new(path: PathBuf) -> Self {
        Self {
            path: super::paths::without_windows_verbatim_prefix(&path),
            tab_count: 1,
            active_tab: 0,
            checklist: super::checklist::Checklist::default(),
        }
    }

    /// Mirror the live terminal view; that view owns tab creation and removal.
    pub(crate) fn update_layout(&mut self, count: usize, active: usize) {
        self.tab_count = count.clamp(1, MAX_TERMINAL_TABS);
        self.active_tab = active.min(self.tab_count - 1);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TerminalWorkspace {
    pub(crate) projects: Vec<TerminalProject>,
    pub(crate) active: usize,
    pub(crate) sidebar_visible: bool,
    #[serde(default)]
    pub(crate) inspector_visible: bool,
    #[serde(default)]
    pub(crate) sidebar_width: Option<u16>,
}

#[derive(Serialize, Deserialize)]
struct StoredWorkspace {
    version: u32,
    #[serde(flatten)]
    workspace: TerminalWorkspace,
}

impl TerminalWorkspace {
    pub(crate) fn new(initial_dir: PathBuf) -> Self {
        Self {
            projects: vec![TerminalProject::new(initial_dir)],
            active: 0,
            sidebar_visible: true,
            inspector_visible: false,
            sidebar_width: None,
        }
    }

    /// Read on an I/O worker. Unavailable folders remain saved for later recovery.
    pub(crate) fn load(path: &Path, initial_dir: &Path) -> (Self, Option<String>) {
        let initial_dir =
            fs::canonicalize(initial_dir).unwrap_or_else(|_| initial_dir.to_path_buf());
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return (Self::new(initial_dir), None);
            }
            Err(_) => {
                return (
                    Self::new(initial_dir),
                    Some("Saved layout could not be read. It is preserved; reopen the app to retry, or choose Replace saved layout.".into()),
                );
            }
        };
        let mut workspace = match serde_json::from_slice::<StoredWorkspace>(&bytes) {
            Ok(stored) if stored.version == WORKSPACE_VERSION => stored.workspace,
            _ => {
                return (
                    Self::new(initial_dir),
                    Some("Saved layout could not be restored. It is preserved; repair it and reopen the app, or choose Replace saved layout.".into()),
                );
            }
        };
        for project in &mut workspace.projects {
            if let Ok(path) = fs::canonicalize(&project.path) {
                project.path = path;
            }
        }
        workspace.normalize(&initial_dir);
        (workspace, None)
    }

    /// Persist layout and reminders; terminal processes and scrollback are not serialized.
    pub(crate) fn save(&self, path: &Path) -> io::Result<()> {
        let mut workspace = self.clone();
        workspace.normalize(Path::new("."));
        let stored = StoredWorkspace {
            version: WORKSPACE_VERSION,
            workspace,
        };
        let bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        super::atomic_file::write(path, &bytes)
    }

    /// Insert a folder already validated and canonicalized by an I/O worker.
    pub(crate) fn insert_project(&mut self, path: PathBuf) -> usize {
        if let Some(index) = self
            .projects
            .iter()
            .position(|project| same_path(&project.path, &path))
        {
            self.active = index;
            return index;
        }
        self.projects.push(TerminalProject::new(path));
        self.active = self.projects.len() - 1;
        self.active
    }

    pub(crate) fn select_project(&mut self, index: usize) -> bool {
        if index >= self.projects.len() {
            return false;
        }
        self.active = index;
        true
    }

    pub(crate) fn remove_project(&mut self, index: usize) -> bool {
        if self.projects.len() <= 1 || index >= self.projects.len() {
            return false;
        }
        self.projects.remove(index);
        self.active = selection_after_removal(self.active, index, self.projects.len());
        true
    }

    fn normalize(&mut self, initial_dir: &Path) {
        self.sidebar_width = self.sidebar_width.map(|width| width.clamp(256, 420));
        let selected_path = self
            .projects
            .get(self.active)
            .map(|project| project.path.clone());
        let mut unique: Vec<TerminalProject> = Vec::with_capacity(self.projects.len());
        for mut project in self.projects.drain(..) {
            project.path = super::paths::without_windows_verbatim_prefix(&project.path);
            if project.path.as_os_str().is_empty()
                || unique
                    .iter()
                    .any(|existing| same_path(&existing.path, &project.path))
            {
                continue;
            }
            project.update_layout(project.tab_count, project.active_tab);
            for section in &mut project.checklist.sections {
                section.normalize();
            }
            unique.push(project);
        }
        if unique.is_empty() {
            unique.push(TerminalProject::new(initial_dir.to_path_buf()));
        }
        self.active = selected_path
            .and_then(|path| {
                unique
                    .iter()
                    .position(|project| same_path(&project.path, &path))
            })
            .unwrap_or_else(|| self.active.min(unique.len() - 1));
        self.projects = unique;
    }
}

fn selection_after_removal(active: usize, removed: usize, remaining: usize) -> usize {
    if active > removed {
        (active - 1).min(remaining - 1)
    } else {
        active.min(remaining - 1)
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        super::paths::without_windows_verbatim_prefix(left)
            .as_os_str()
            .as_encoded_bytes()
            .eq_ignore_ascii_case(
                super::paths::without_windows_verbatim_prefix(right)
                    .as_os_str()
                    .as_encoded_bytes(),
            )
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::checklist;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pideck-terminal-workspace-{}-{}",
                std::process::id(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn legacy_layouts_default_the_width_and_invalid_widths_are_bounded() {
        let root = TestDirectory::new();
        let path = root.0.join("layout.json");
        fs::write(&path, br#"{"version":1,"projects":[{"path":"synthetic","tab_count":1,"active_tab":0}],"active":0,"sidebar_visible":true}"#).unwrap();
        let (mut workspace, warning) = TerminalWorkspace::load(&path, &root.0);
        assert!(warning.is_none());
        assert_eq!(workspace.sidebar_width, None);
        assert!(!workspace.inspector_visible);
        assert_eq!(
            workspace.projects[0].checklist,
            checklist::Checklist::default()
        );
        workspace.sidebar_width = Some(900);
        workspace.normalize(&root.0);
        assert_eq!(workspace.sidebar_width, Some(420));
    }

    #[test]
    fn saved_layout_roundtrips_and_keeps_unavailable_projects() {
        let root = TestDirectory::new();
        let mut workspace = TerminalWorkspace::new(root.0.clone());
        workspace.projects.push(TerminalProject {
            path: super::super::paths::without_windows_verbatim_prefix(
                &root.0.join("temporarily-offline"),
            ),
            tab_count: 3,
            active_tab: 2,
            checklist: checklist::Checklist::default(),
        });
        workspace.active = 1;
        workspace.sidebar_visible = false;
        workspace.sidebar_width = Some(376);
        workspace.inspector_visible = true;
        let outline = &mut workspace.projects[0].checklist.sections[0];
        outline.add("Synthetic parent".into(), None);
        outline.add("Synthetic child".into(), Some(0));
        outline.toggle(1);
        outline.tasks[0].collapsed = true;
        workspace.projects[1].checklist.sections[0].add("Other project".into(), None);
        let path = root.0.join("layout.json");
        workspace.save(&path).unwrap();
        let (loaded, warning) = TerminalWorkspace::load(&path, &root.0);
        assert_eq!(loaded, workspace);
        assert_eq!(warning, None);
    }

    #[test]
    fn missing_or_malformed_storage_keeps_launch_folder() {
        let root = TestDirectory::new();
        let path = root.0.join("layout.json");
        let (missing, warning) = TerminalWorkspace::load(&path, &root.0);
        assert_eq!(missing, TerminalWorkspace::new(root.0.clone()));
        assert_eq!(warning, None);
        fs::write(&path, b"invalid json").unwrap();
        let (malformed, warning) = TerminalWorkspace::load(&path, &root.0);
        assert_eq!(malformed, missing);
        assert!(warning.is_some());
        assert_eq!(fs::read(path).unwrap(), b"invalid json");
    }

    #[test]
    fn normalization_bounds_tabs_deduplicates_and_retains_selection() {
        let mut workspace = TerminalWorkspace {
            projects: vec![
                TerminalProject {
                    path: "one".into(),
                    tab_count: 0,
                    active_tab: 90,
                    checklist: checklist::Checklist::default(),
                },
                TerminalProject {
                    path: "one".into(),
                    tab_count: 2,
                    active_tab: 1,
                    checklist: checklist::Checklist::default(),
                },
                TerminalProject {
                    path: "two".into(),
                    tab_count: 90,
                    active_tab: 90,
                    checklist: checklist::Checklist::default(),
                },
            ],
            active: 2,
            sidebar_visible: true,
            inspector_visible: false,
            sidebar_width: None,
        };
        workspace.normalize(Path::new("fallback"));
        assert_eq!(workspace.projects.len(), 2);
        assert_eq!(workspace.active, 1);
        assert_eq!(workspace.projects[0].tab_count, 1);
        assert_eq!(workspace.projects[0].active_tab, 0);
        assert_eq!(workspace.projects[1].tab_count, MAX_TERMINAL_TABS);
        assert_eq!(workspace.projects[1].active_tab, MAX_TERMINAL_TABS - 1);
        workspace.projects.clear();
        workspace.active = usize::MAX;
        workspace.normalize(Path::new("fallback"));
        assert_eq!(workspace, TerminalWorkspace::new("fallback".into()));
    }

    #[test]
    fn inserting_an_existing_folder_selects_its_original_layout() {
        let mut workspace = TerminalWorkspace::new("one".into());
        workspace.projects[0].update_layout(4, 2);
        assert_eq!(workspace.insert_project("two".into()), 1);
        assert_eq!(workspace.insert_project("one".into()), 0);
        assert_eq!(workspace.active, 0);
        assert_eq!(workspace.projects.len(), 2);
        assert_eq!(workspace.projects[0].tab_count, 4);
        assert_eq!(workspace.projects[0].active_tab, 2);
    }

    #[cfg(windows)]
    #[test]
    fn insertion_deduplicates_canonical_windows_drive_and_unc_paths() {
        let mut workspace = TerminalWorkspace::new(r"C:\Projects\one".into());
        assert_eq!(workspace.insert_project(r"\\?\C:\Projects\one".into()), 0);
        assert_eq!(workspace.insert_project(r"c:\projects\ONE".into()), 0);
        assert_eq!(
            workspace.insert_project(r"\\?\UNC\server\share\two".into()),
            1
        );
        assert_eq!(workspace.insert_project(r"\\server\share\two".into()), 1);
        assert_eq!(workspace.projects.len(), 2);
    }

    #[test]
    fn project_removal_preserves_active_identity_and_last_project() {
        let mut workspace = TerminalWorkspace::new("one".into());
        workspace.projects.push(TerminalProject::new("two".into()));
        workspace
            .projects
            .push(TerminalProject::new("three".into()));
        assert!(workspace.select_project(2));
        assert!(!workspace.select_project(3));
        assert!(workspace.remove_project(0));
        assert_eq!(workspace.active, 1);
        assert_eq!(
            workspace.projects[workspace.active].path,
            Path::new("three")
        );
        assert!(workspace.remove_project(1));
        assert_eq!(workspace.active, 0);
        assert!(!workspace.remove_project(0));
        assert!(!workspace.remove_project(2));
    }

    #[test]
    fn live_terminal_snapshot_replaces_metadata_and_bounds_restored_values() {
        let mut project = TerminalProject::new("one".into());
        project.update_layout(5, 3);
        assert_eq!((project.tab_count, project.active_tab), (5, 3));
        project.update_layout(2, 0);
        assert_eq!((project.tab_count, project.active_tab), (2, 0));
        project.update_layout(usize::MAX, usize::MAX);
        assert_eq!(
            (project.tab_count, project.active_tab),
            (MAX_TERMINAL_TABS, MAX_TERMINAL_TABS - 1)
        );
        project.update_layout(0, usize::MAX);
        assert_eq!((project.tab_count, project.active_tab), (1, 0));
    }
}
