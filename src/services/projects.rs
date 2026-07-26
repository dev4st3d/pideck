//! Persistent project registry for the multi-workspace sidebar.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::services::session_catalog::without_windows_verbatim_prefix;

const PROJECTS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    pub path: PathBuf,
    pub expanded: bool,
    pub last_session: Option<PathBuf>,
}

impl ProjectEntry {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }
}

#[derive(Debug, Clone)]
pub struct ProjectRegistry {
    storage_path: PathBuf,
    active: PathBuf,
    projects: Vec<ProjectEntry>,
}

#[derive(Debug, Clone)]
pub struct ProjectRegistryLoad {
    pub registry: ProjectRegistry,
    pub warning: Option<String>,
    pub needs_save: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddProjectOutcome {
    Added,
    AlreadyPresent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectRegistryError {
    NotDirectory,
    LastProject,
    UnknownProject,
    InaccessibleStorage,
}

impl ProjectRegistryError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::NotDirectory => "Only existing folders can be added as projects.",
            Self::LastProject => "Keep at least one project in the sidebar.",
            Self::UnknownProject => "That project is no longer in the sidebar.",
            Self::InaccessibleStorage => "Project sidebar changes could not be saved.",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredRegistry {
    version: u32,
    active: PathBuf,
    projects: Vec<StoredProject>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredProject {
    path: PathBuf,
    #[serde(default = "expanded_by_default")]
    expanded: bool,
    #[serde(default)]
    last_session: Option<PathBuf>,
}

fn expanded_by_default() -> bool {
    true
}

impl ProjectRegistry {
    pub fn load(storage_path: PathBuf, initial_workspace: PathBuf) -> ProjectRegistryLoad {
        let initial_workspace = normalize_existing_path(&initial_workspace)
            .unwrap_or_else(|| normalize_unchecked_path(&initial_workspace));
        let fallback = || Self {
            storage_path: storage_path.clone(),
            active: initial_workspace.clone(),
            projects: vec![ProjectEntry {
                path: initial_workspace.clone(),
                expanded: true,
                last_session: None,
            }],
        };

        let bytes = match fs::read(&storage_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return ProjectRegistryLoad {
                    registry: fallback(),
                    warning: None,
                    needs_save: true,
                };
            }
            Err(_) => {
                return ProjectRegistryLoad {
                    registry: fallback(),
                    warning: Some(
                        "Saved projects could not be read. The launch folder is still available."
                            .to_owned(),
                    ),
                    needs_save: false,
                };
            }
        };

        let stored = match serde_json::from_slice::<StoredRegistry>(&bytes) {
            Ok(stored) if stored.version == PROJECTS_VERSION => stored,
            _ => {
                return ProjectRegistryLoad {
                    registry: fallback(),
                    warning: Some(
                        "Saved projects are invalid. The launch folder is still available."
                            .to_owned(),
                    ),
                    needs_save: false,
                };
            }
        };

        let stored_project_count = stored.projects.len();
        let mut projects = Vec::new();
        for stored_project in stored.projects {
            let path = normalize_existing_path(&stored_project.path)
                .unwrap_or_else(|| normalize_unchecked_path(&stored_project.path));
            if projects
                .iter()
                .any(|project: &ProjectEntry| paths_match(&project.path, &path))
            {
                continue;
            }
            projects.push(ProjectEntry {
                path,
                expanded: stored_project.expanded,
                last_session: stored_project
                    .last_session
                    .map(|path| normalize_unchecked_path(&path)),
            });
        }

        let mut needs_save = projects.len() != stored_project_count;
        if !projects
            .iter()
            .any(|project| paths_match(&project.path, &initial_workspace))
        {
            projects.push(ProjectEntry {
                path: initial_workspace.clone(),
                expanded: true,
                last_session: None,
            });
            needs_save = true;
        }

        if projects.is_empty() {
            projects.push(ProjectEntry {
                path: initial_workspace.clone(),
                expanded: true,
                last_session: None,
            });
            needs_save = true;
        }

        let stored_active = normalize_existing_path(&stored.active)
            .unwrap_or_else(|| normalize_unchecked_path(&stored.active));
        let (active, warning) = if stored_active.is_dir()
            && projects
                .iter()
                .any(|project| paths_match(&project.path, &stored_active))
        {
            (stored_active, None)
        } else {
            needs_save = true;
            (
                initial_workspace,
                Some(
                    "The last active project is unavailable. The launch folder was opened instead."
                        .to_owned(),
                ),
            )
        };

        ProjectRegistryLoad {
            registry: Self {
                storage_path,
                active,
                projects,
            },
            warning,
            needs_save,
        }
    }

    pub fn projects(&self) -> &[ProjectEntry] {
        &self.projects
    }

    pub fn active_path(&self) -> &Path {
        &self.active
    }

    pub fn active_project(&self) -> &ProjectEntry {
        self.projects
            .iter()
            .find(|project| paths_match(&project.path, &self.active))
            .expect("the project registry always retains its active project")
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.projects
            .iter()
            .any(|project| paths_match(&project.path, path))
    }

    pub fn is_active(&self, path: &Path) -> bool {
        paths_match(&self.active, path)
    }

    pub fn add(&mut self, path: PathBuf) -> Result<AddProjectOutcome, ProjectRegistryError> {
        let Some(path) = normalize_existing_path(&path) else {
            return Err(ProjectRegistryError::NotDirectory);
        };
        if !path.is_dir() {
            return Err(ProjectRegistryError::NotDirectory);
        }
        if self.contains(&path) {
            return Ok(AddProjectOutcome::AlreadyPresent);
        }
        self.projects.push(ProjectEntry {
            path,
            expanded: true,
            last_session: None,
        });
        Ok(AddProjectOutcome::Added)
    }

    pub fn set_active(&mut self, path: &Path) -> Result<bool, ProjectRegistryError> {
        let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| paths_match(&project.path, path))
        else {
            return Err(ProjectRegistryError::UnknownProject);
        };
        let changed = !paths_match(&self.active, &project.path);
        self.active.clone_from(&project.path);
        project.expanded = true;
        Ok(changed)
    }

    pub fn set_expanded(
        &mut self,
        path: &Path,
        expanded: bool,
    ) -> Result<bool, ProjectRegistryError> {
        let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| paths_match(&project.path, path))
        else {
            return Err(ProjectRegistryError::UnknownProject);
        };
        if project.expanded == expanded {
            return Ok(false);
        }
        project.expanded = expanded;
        Ok(true)
    }

    pub fn toggle_expanded(&mut self, path: &Path) -> Result<bool, ProjectRegistryError> {
        let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| paths_match(&project.path, path))
        else {
            return Err(ProjectRegistryError::UnknownProject);
        };
        project.expanded = !project.expanded;
        Ok(project.expanded)
    }

    pub fn set_last_session(&mut self, path: &Path, session: Option<PathBuf>) -> bool {
        let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| paths_match(&project.path, path))
        else {
            return false;
        };
        let session = session.map(|path| normalize_unchecked_path(&path));
        if project.last_session == session {
            return false;
        }
        project.last_session = session;
        true
    }

    pub fn remove(&mut self, path: &Path) -> Result<PathBuf, ProjectRegistryError> {
        if self.projects.len() <= 1 {
            return Err(ProjectRegistryError::LastProject);
        }
        let Some(index) = self
            .projects
            .iter()
            .position(|project| paths_match(&project.path, path))
        else {
            return Err(ProjectRegistryError::UnknownProject);
        };
        let removed_active = self.is_active(path);
        self.projects.remove(index);
        if removed_active {
            let next = index.min(self.projects.len().saturating_sub(1));
            self.active.clone_from(&self.projects[next].path);
        }
        Ok(self.active.clone())
    }

    pub fn save(&self) -> Result<(), ProjectRegistryError> {
        let stored = StoredRegistry {
            version: PROJECTS_VERSION,
            active: self.active.clone(),
            projects: self
                .projects
                .iter()
                .map(|project| StoredProject {
                    path: project.path.clone(),
                    expanded: project.expanded,
                    last_session: project.last_session.clone(),
                })
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|_| ProjectRegistryError::InaccessibleStorage)?;
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent).map_err(|_| ProjectRegistryError::InaccessibleStorage)?;
        }
        fs::write(&self.storage_path, bytes).map_err(|_| ProjectRegistryError::InaccessibleStorage)
    }
}

pub fn project_key(path: &Path) -> String {
    let normalized = normalize_unchecked_path(path)
        .to_string_lossy()
        .replace('\\', "/");
    #[cfg(windows)]
    return normalized.to_lowercase();
    #[cfg(not(windows))]
    normalized.into_owned()
}

fn normalize_existing_path(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path)
        .ok()
        .map(|path| without_windows_verbatim_prefix(&path))
}

fn normalize_unchecked_path(path: &Path) -> PathBuf {
    without_windows_verbatim_prefix(path)
}

fn paths_match(left: &Path, right: &Path) -> bool {
    project_key(left) == project_key(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pi-gui-projects-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("temp directory");
        path
    }

    #[test]
    fn missing_store_starts_with_the_launch_project() {
        let root = temp_dir("missing");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let load = ProjectRegistry::load(root.join("projects.json"), workspace.clone());

        assert!(load.needs_save);
        assert_eq!(load.registry.projects().len(), 1);
        assert!(load.registry.is_active(&workspace));
        assert!(load.warning.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn projects_round_trip_with_expansion_and_last_session() {
        let root = temp_dir("round-trip");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let storage = root.join("config").join("projects.json");
        let mut registry = ProjectRegistry::load(storage.clone(), first.clone()).registry;
        assert_eq!(registry.add(second.clone()), Ok(AddProjectOutcome::Added));
        registry.set_active(&second).unwrap();
        registry.set_expanded(&first, false).unwrap();
        let session = root.join("second.jsonl");
        registry.set_last_session(&second, Some(session.clone()));
        registry.save().unwrap();

        let loaded = ProjectRegistry::load(storage, first.clone()).registry;
        assert_eq!(loaded.projects().len(), 2);
        assert!(loaded.is_active(&second));
        assert!(!loaded.projects()[0].expanded);
        assert_eq!(
            loaded.active_project().last_session.as_ref(),
            Some(&session)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_adds_and_last_project_removal_are_guarded() {
        let root = temp_dir("guards");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let mut registry =
            ProjectRegistry::load(root.join("projects.json"), workspace.clone()).registry;

        assert_eq!(
            registry.add(workspace.clone()),
            Ok(AddProjectOutcome::AlreadyPresent)
        );
        assert_eq!(
            registry.remove(&workspace),
            Err(ProjectRegistryError::LastProject)
        );
        assert_eq!(
            registry.add(root.join("missing")),
            Err(ProjectRegistryError::NotDirectory)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removing_the_active_project_selects_a_neighbor() {
        let root = temp_dir("remove-active");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let mut registry =
            ProjectRegistry::load(root.join("projects.json"), first.clone()).registry;
        registry.add(second.clone()).unwrap();
        registry.set_active(&second).unwrap();

        let next = registry.remove(&second).unwrap();
        assert!(paths_match(&next, &first));
        assert!(registry.is_active(&first));
        let _ = fs::remove_dir_all(root);
    }
}
