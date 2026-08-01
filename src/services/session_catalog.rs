//! Read-only discovery and metadata scanning for Pi session JSONL files.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::UNIX_EPOCH;

use async_channel::{Receiver, Sender};
use serde_json::Value;

const SESSION_DIR_ENV: &str = "PI_CODING_AGENT_SESSION_DIR";
const AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";
const SUMMARY_LIMIT: usize = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRootSource {
    Explicit,
    Environment,
    Settings,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRoot {
    pub path: PathBuf,
    pub source: SessionRootSource,
}

#[derive(Debug, Clone)]
pub struct SessionCatalogConfig {
    pub workspace: PathBuf,
    pub explicit_session_dir: Option<PathBuf>,
    pub environment_session_dir: Option<PathBuf>,
    pub agent_dir: PathBuf,
    pub settings_path: PathBuf,
}

impl SessionCatalogConfig {
    pub fn from_environment(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let agent_dir = std::env::var_os(AGENT_DIR_ENV)
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|home| home.join(".pi").join("agent")))
            .unwrap_or_else(|| workspace.join(".pi").join("agent"));
        Self {
            workspace,
            explicit_session_dir: None,
            environment_session_dir: std::env::var_os(SESSION_DIR_ENV).map(PathBuf::from),
            settings_path: agent_dir.join("settings.json"),
            agent_dir,
        }
    }

    pub fn resolve_root(&self) -> SessionRoot {
        if let Some(path) = &self.explicit_session_dir {
            return SessionRoot {
                path: resolve_configured_path(path, &self.workspace),
                source: SessionRootSource::Explicit,
            };
        }
        if let Some(path) = &self.environment_session_dir {
            return SessionRoot {
                path: resolve_configured_path(path, &self.workspace),
                source: SessionRootSource::Environment,
            };
        }
        if let Some(path) = configured_session_dir(&self.settings_path) {
            return SessionRoot {
                path: resolve_configured_path(&path, &self.workspace),
                source: SessionRootSource::Settings,
            };
        }
        SessionRoot {
            path: default_session_dir(&self.agent_dir, &self.workspace),
            source: SessionRootSource::Default,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionCounts {
    pub messages: u64,
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_results: u64,
    pub compactions: u64,
    pub branches: u64,
}

impl SessionSummary {
    /// Transient row for a live runtime whose assigned JSONL path has not been
    /// created on disk yet. Pi defers that write until the first assistant message.
    pub(crate) fn live(path: PathBuf, name: Option<String>) -> Self {
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("live-session")
            .to_owned();
        Self {
            id,
            name,
            first_user_summary: None,
            created_at: "Now".to_owned(),
            updated_at: "Now".to_owned(),
            parent_session: None,
            path,
            version: 3,
            counts: SessionCounts::default(),
            modified_sort_key: u128::MAX,
        }
    }

    /// Synthetic row for view-layer tests that must not touch the filesystem.
    #[cfg(test)]
    pub(crate) fn test_stub(id: &str, path: PathBuf) -> Self {
        let mut summary = Self::live(path, Some(id.to_owned()));
        summary.id = id.to_owned();
        summary.created_at = "2026-01-02T03:04:05.000Z".to_owned();
        summary.updated_at = "2026-01-02T03:04:05.000Z".to_owned();
        summary.modified_sort_key = 0;
        summary
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub name: Option<String>,
    pub first_user_summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub parent_session: Option<PathBuf>,
    pub path: PathBuf,
    pub version: u64,
    pub counts: SessionCounts,
    modified_sort_key: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorruptSession {
    pub path: PathBuf,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCatalogScan {
    pub root: SessionRoot,
    pub sessions: Vec<SessionSummary>,
    pub corrupt: Vec<CorruptSession>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCatalogError {
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashEligibility {
    Eligible,
    ActiveSession,
    OutsideCatalogRoot,
    ReversibleProviderUnavailable,
}

pub fn trash_eligibility(
    target: &Path,
    active_session: Option<&Path>,
    catalog_root: &Path,
    reversible_provider_available: bool,
) -> TrashEligibility {
    if active_session.is_some_and(|active| paths_match(active, target)) {
        return TrashEligibility::ActiveSession;
    }
    if !target
        .parent()
        .is_some_and(|parent| paths_match(parent, catalog_root))
    {
        return TrashEligibility::OutsideCatalogRoot;
    }
    if !reversible_provider_available {
        return TrashEligibility::ReversibleProviderUnavailable;
    }
    TrashEligibility::Eligible
}

/// Windows Recycle Bin via `SHFileOperation`. Other platforms stay unavailable
/// until a reversible provider is proven there.
pub fn reversible_trash_available() -> bool {
    cfg!(windows)
}

pub fn trash_eligibility_message(eligibility: TrashEligibility) -> &'static str {
    match eligibility {
        TrashEligibility::Eligible => "Thread can be moved to the Recycle Bin.",
        TrashEligibility::ActiveSession => "Switch to another thread before deleting this one.",
        TrashEligibility::OutsideCatalogRoot => {
            "That thread is outside this project's session catalog."
        }
        TrashEligibility::ReversibleProviderUnavailable => {
            "Recycle Bin is unavailable on this platform."
        }
    }
}

/// Move a catalog-owned session JSONL into the platform Recycle Bin.
///
/// Rejects the active session file and anything outside `catalog_root`.
pub fn trash_session_file(
    target: &Path,
    active_session: Option<&Path>,
    catalog_root: &Path,
) -> Result<(), String> {
    let eligibility = trash_eligibility(
        target,
        active_session,
        catalog_root,
        reversible_trash_available(),
    );
    if eligibility != TrashEligibility::Eligible {
        return Err(trash_eligibility_message(eligibility).to_owned());
    }
    if !target.is_file() {
        return Err("That thread file is no longer available.".to_owned());
    }
    move_path_to_trash(target)
}

fn move_path_to_trash(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        move_path_to_trash_windows(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(trash_eligibility_message(TrashEligibility::ReversibleProviderUnavailable).to_owned())
    }
}

#[cfg(windows)]
fn move_path_to_trash_windows(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };

    // SHFileOperation requires a double-null-terminated list of absolute paths.
    let mut from: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .chain(std::iter::once(0))
        .collect();

    let mut file_op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_mut_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };

    // SAFETY: `from` stays alive for the call; double-null terminator is present;
    // flags request silent undoable delete with no UI.
    let result = unsafe { SHFileOperationW(&mut file_op) };
    if result != 0 || file_op.fAnyOperationsAborted != 0 {
        return Err("Windows could not move that thread to the Recycle Bin.".to_owned());
    }
    if path.exists() {
        return Err("The thread file is still present after the Recycle Bin request.".to_owned());
    }
    Ok(())
}

pub fn scan_sessions(
    config: &SessionCatalogConfig,
) -> Result<SessionCatalogScan, SessionCatalogError> {
    let root = config.resolve_root();
    if !root.path.exists() {
        return Ok(SessionCatalogScan {
            root,
            sessions: Vec::new(),
            corrupt: Vec::new(),
        });
    }
    let entries = fs::read_dir(&root.path).map_err(|error| SessionCatalogError {
        summary: io_summary("Session directory is inaccessible", &error),
    })?;
    let workspace_key =
        canonical_path_key(&config.workspace).map_err(|error| SessionCatalogError {
            summary: io_summary("Workspace path cannot be resolved", &error),
        })?;
    let mut sessions = Vec::new();
    let mut corrupt = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                corrupt.push(CorruptSession {
                    path: root.path.clone(),
                    summary: io_summary("A directory entry could not be read", &error),
                });
                continue;
            }
        };
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
        {
            continue;
        }
        match scan_session_file(&path, &workspace_key) {
            Ok(Some(session)) => sessions.push(session),
            Ok(None) => {}
            Err(summary) => corrupt.push(CorruptSession { path, summary }),
        }
    }
    sessions.sort_by_key(|session| std::cmp::Reverse(session.modified_sort_key));
    Ok(SessionCatalogScan {
        root,
        sessions,
        corrupt,
    })
}

fn scan_session_file(path: &Path, workspace_key: &str) -> Result<Option<SessionSummary>, String> {
    let file =
        File::open(path).map_err(|error| io_summary("Session file is inaccessible", &error))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_summary("Session metadata is inaccessible", &error))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut line_number = 0_u64;
    let mut header: Option<Value> = None;
    let mut counts = SessionCounts::default();
    let mut name = None;
    let mut first_user_summary = None;
    let mut updated_at = None;

    loop {
        buffer.clear();
        let bytes = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| io_summary("Session file could not be read", &error))?;
        if bytes == 0 {
            break;
        }
        line_number = line_number.saturating_add(1);
        let complete = buffer.ends_with(b"\n");
        while matches!(buffer.last(), Some(b'\n' | b'\r')) {
            buffer.pop();
        }
        if buffer.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value: Value = match serde_json::from_slice(&buffer) {
            Ok(value) => value,
            Err(_) if !complete => break,
            Err(error) => {
                return Err(format!(
                    "Invalid JSON on complete line {line_number}: {error}"
                ));
            }
        };
        if header.is_none() {
            validate_header(&value, line_number)?;
            let cwd = value
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|cwd| !cwd.trim().is_empty())
                .ok_or_else(|| "Session header has no canonical working directory".to_owned())?;
            let header_key = canonical_path_key(Path::new(cwd)).map_err(|error| {
                io_summary("Session working directory cannot be resolved", &error)
            })?;
            if header_key != workspace_key {
                return Ok(None);
            }
            updated_at = value
                .get("timestamp")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            header = Some(value);
            continue;
        }
        let entry_type = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("Session entry on line {line_number} has no string type"))?;
        if let Some(timestamp) = activity_timestamp(&value) {
            updated_at = Some(timestamp);
        }
        match entry_type {
            "message" => {
                counts.messages = counts.messages.saturating_add(1);
                match value.pointer("/message/role").and_then(Value::as_str) {
                    Some("user") => {
                        counts.user_messages = counts.user_messages.saturating_add(1);
                        if first_user_summary.is_none() {
                            first_user_summary = message_text(value.pointer("/message/content"));
                        }
                    }
                    Some("assistant") => {
                        counts.assistant_messages = counts.assistant_messages.saturating_add(1)
                    }
                    Some("toolResult") => {
                        counts.tool_results = counts.tool_results.saturating_add(1)
                    }
                    _ => {}
                }
            }
            "session_info" => {
                name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(ToOwned::to_owned);
            }
            "compaction" => counts.compactions = counts.compactions.saturating_add(1),
            "branch_summary" => counts.branches = counts.branches.saturating_add(1),
            _ => {}
        }
    }

    let header = header.ok_or_else(|| "Session file has no header".to_owned())?;
    let version = header.get("version").and_then(Value::as_u64).unwrap_or(1);
    if !(1..=3).contains(&version) {
        return Err(format!("Unsupported session version {version}"));
    }
    let id = header
        .get("id")
        .and_then(Value::as_str)
        .expect("validated session id")
        .to_owned();
    let created_at = header
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or("Unknown")
        .to_owned();
    let modified_sort_key = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis());
    Ok(Some(SessionSummary {
        id,
        name,
        first_user_summary,
        created_at: created_at.clone(),
        updated_at: updated_at.unwrap_or(created_at),
        parent_session: header
            .get("parentSession")
            .and_then(Value::as_str)
            .map(PathBuf::from),
        path: path.to_path_buf(),
        version,
        counts,
        modified_sort_key,
    }))
}

fn validate_header(value: &Value, line_number: u64) -> Result<(), String> {
    if value.get("type").and_then(Value::as_str) != Some("session") {
        return Err(format!("Line {line_number} is not a session header"));
    }
    if value.get("id").and_then(Value::as_str).is_none() {
        return Err("Session header has no string id".to_owned());
    }
    Ok(())
}

fn activity_timestamp(entry: &Value) -> Option<String> {
    if !matches!(
        entry.pointer("/message/role").and_then(Value::as_str),
        Some("user" | "assistant")
    ) {
        return None;
    }
    entry
        .get("timestamp")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn message_text(content: Option<&Value>) -> Option<String> {
    let text = match content? {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(truncate_chars(&normalized, SUMMARY_LIMIT))
    }
}

fn truncate_chars(value: &str, limit: usize) -> String {
    let mut characters = value.chars();
    let prefix = characters.by_ref().take(limit).collect::<String>();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn configured_session_dir(settings_path: &Path) -> Option<PathBuf> {
    let value: Value = serde_json::from_slice(&fs::read(settings_path).ok()?).ok()?;
    value
        .get("sessionDir")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

fn resolve_configured_path(path: &Path, workspace: &Path) -> PathBuf {
    let expanded = expand_tilde(path);
    if expanded.is_absolute() {
        expanded
    } else {
        workspace.join(expanded)
    }
}

fn expand_tilde(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if value == "~" {
        return home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
        && let Some(home) = home_dir()
    {
        return home.join(rest);
    }
    path.to_path_buf()
}

fn default_session_dir(agent_dir: &Path, workspace: &Path) -> PathBuf {
    let resolved = fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let normalized = without_windows_verbatim_prefix(&resolved);
    let text = normalized.to_string_lossy();
    let without_root = text.trim_start_matches(['/', '\\']);
    let encoded = without_root
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' => '-',
            other => other,
        })
        .collect::<String>();
    agent_dir.join("sessions").join(format!("--{encoded}--"))
}

fn canonical_path_key(path: &Path) -> io::Result<String> {
    let canonical = fs::canonicalize(path)?;
    let normalized = without_windows_verbatim_prefix(&canonical)
        .to_string_lossy()
        .replace('\\', "/");
    #[cfg(windows)]
    return Ok(normalized.to_lowercase());
    #[cfg(not(windows))]
    Ok(normalized)
}

pub(crate) fn without_windows_verbatim_prefix(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

fn paths_match(left: &Path, right: &Path) -> bool {
    match (canonical_path_key(left), canonical_path_key(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => {
            if cfg!(windows) {
                left.to_string_lossy()
                    .eq_ignore_ascii_case(&right.to_string_lossy())
            } else {
                left == right
            }
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn io_summary(context: &str, error: &io::Error) -> String {
    format!("{context}: {}", error.kind())
}

#[derive(Debug)]
enum CatalogCommand {
    Refresh { generation: u64 },
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct CatalogWorkerResult {
    pub generation: u64,
    pub result: Result<SessionCatalogScan, SessionCatalogError>,
}

pub struct SessionCatalogWorker {
    commands: mpsc::Sender<CatalogCommand>,
    results: Receiver<CatalogWorkerResult>,
}

impl SessionCatalogWorker {
    pub fn spawn(config: SessionCatalogConfig) -> Self {
        let (commands, command_receiver) = mpsc::channel();
        let (result_sender, results) = async_channel::unbounded();
        thread::spawn(move || catalog_worker(config, command_receiver, result_sender));
        Self { commands, results }
    }

    pub fn refresh(&self, generation: u64) -> bool {
        self.commands
            .send(CatalogCommand::Refresh { generation })
            .is_ok()
    }

    pub fn results(&self) -> Receiver<CatalogWorkerResult> {
        self.results.clone()
    }
}

impl Drop for SessionCatalogWorker {
    fn drop(&mut self) {
        let _ = self.commands.send(CatalogCommand::Shutdown);
    }
}

fn catalog_worker(
    config: SessionCatalogConfig,
    commands: mpsc::Receiver<CatalogCommand>,
    results: Sender<CatalogWorkerResult>,
) {
    while let Ok(command) = commands.recv() {
        match command {
            CatalogCommand::Refresh { generation } => {
                let _ = results.send_blocking(CatalogWorkerResult {
                    generation,
                    result: scan_sessions(&config),
                });
            }
            CatalogCommand::Shutdown => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pi-gui-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("temp directory");
        path
    }

    fn config(workspace: &Path, root: &Path) -> SessionCatalogConfig {
        SessionCatalogConfig {
            workspace: workspace.to_path_buf(),
            explicit_session_dir: Some(root.to_path_buf()),
            environment_session_dir: None,
            agent_dir: root.join("agent"),
            settings_path: root.join("missing-settings.json"),
        }
    }

    #[test]
    fn directory_precedence_is_explicit_environment_settings_default() {
        let root = temp_dir("catalog-precedence");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let settings = root.join("settings.json");
        fs::write(&settings, r#"{"sessionDir":"settings"}"#).unwrap();
        let mut config = SessionCatalogConfig {
            workspace: workspace.clone(),
            explicit_session_dir: Some(PathBuf::from("explicit")),
            environment_session_dir: Some(PathBuf::from("environment")),
            agent_dir: root.join("agent"),
            settings_path: settings,
        };
        assert_eq!(config.resolve_root().source, SessionRootSource::Explicit);
        config.explicit_session_dir = None;
        assert_eq!(config.resolve_root().source, SessionRootSource::Environment);
        config.environment_session_dir = None;
        assert_eq!(config.resolve_root().source, SessionRootSource::Settings);
        fs::remove_file(&config.settings_path).unwrap();
        assert_eq!(config.resolve_root().source, SessionRootSource::Default);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_verbatim_workspace_uses_the_same_valid_default_root_as_pi() {
        let agent_dir = Path::new(r"C:\synthetic-agent");
        let workspace = Path::new(r"\\?\C:\synthetic\workspace");

        assert_eq!(
            without_windows_verbatim_prefix(workspace),
            PathBuf::from(r"C:\synthetic\workspace")
        );
        assert_eq!(
            default_session_dir(agent_dir, workspace),
            PathBuf::from(r"C:\synthetic-agent\sessions\--C--synthetic-workspace--")
        );
        assert_eq!(
            without_windows_verbatim_prefix(Path::new(r"\\?\UNC\server\share\workspace")),
            PathBuf::from(r"\\server\share\workspace")
        );
    }

    #[test]
    fn canonical_cwd_filter_and_metadata_work_for_v1_to_v3() {
        let root = temp_dir("catalog-versions");
        let workspace = root.join("workspace");
        let other = root.join("other");
        let sessions = root.join("sessions");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::create_dir_all(&sessions).unwrap();
        for version in 1..=3 {
            let file = sessions.join(format!("v{version}.jsonl"));
            let header = if version == 1 {
                format!(
                    "{{\"type\":\"session\",\"id\":\"v1\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":{}}}\n",
                    serde_json::to_string(&workspace.to_string_lossy()).unwrap()
                )
            } else {
                format!(
                    "{{\"type\":\"session\",\"version\":{version},\"id\":\"v{version}\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":{}}}\n",
                    serde_json::to_string(&workspace.to_string_lossy()).unwrap()
                )
            };
            fs::write(
                file,
                format!(
                    "{header}{{\"type\":\"message\",\"id\":\"entry\",\"parentId\":null,\"timestamp\":\"2026-01-01T00:00:01Z\",\"message\":{{\"role\":\"user\",\"content\":\"hello v{version}\"}}}}\n"
                ),
            )
            .unwrap();
        }
        fs::write(
            sessions.join("other.jsonl"),
            format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"other\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":{}}}\n",
                serde_json::to_string(&other.to_string_lossy()).unwrap()
            ),
        )
        .unwrap();
        let scan = scan_sessions(&config(&workspace, &sessions)).unwrap();
        assert_eq!(scan.sessions.len(), 3);
        assert!(
            scan.sessions
                .iter()
                .all(|session| session.counts.user_messages == 1)
        );
        assert!(
            scan.sessions
                .iter()
                .all(|session| session.first_user_summary.is_some())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn incomplete_trailing_record_is_tolerated_but_earlier_corruption_is_rejected() {
        let root = temp_dir("catalog-corruption");
        let workspace = root.join("workspace");
        let sessions = root.join("sessions");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&sessions).unwrap();
        let header = format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"ok\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":{}}}\n",
            serde_json::to_string(&workspace.to_string_lossy()).unwrap()
        );
        fs::write(
            sessions.join("trailing.jsonl"),
            format!("{header}{{\"type\":"),
        )
        .unwrap();
        let mut corrupt = File::create(sessions.join("corrupt.jsonl")).unwrap();
        corrupt.write_all(header.as_bytes()).unwrap();
        corrupt.write_all(b"{bad}\n").unwrap();
        corrupt.flush().unwrap();
        let scan = scan_sessions(&config(&workspace, &sessions)).unwrap();
        assert_eq!(scan.sessions.len(), 1);
        assert_eq!(scan.corrupt.len(), 1);
        assert!(scan.corrupt[0].summary.contains("complete line 2"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn trash_guard_never_allows_the_active_file_or_unproven_provider() {
        let root = temp_dir("catalog-trash-guard");
        let active = root.join("active.jsonl");
        let inactive = root.join("inactive.jsonl");
        fs::write(&active, "").unwrap();
        fs::write(&inactive, "").unwrap();
        assert_eq!(
            trash_eligibility(&active, Some(&active), &root, true),
            TrashEligibility::ActiveSession
        );
        assert_eq!(
            trash_eligibility(&inactive, Some(&active), &root, false),
            TrashEligibility::ReversibleProviderUnavailable
        );
        assert_eq!(
            trash_eligibility(&inactive, Some(&active), &root, true),
            TrashEligibility::Eligible
        );
        assert!(
            trash_session_file(&active, Some(&active), &root)
                .unwrap_err()
                .contains("Switch to another thread")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(windows)]
    fn trash_session_moves_inactive_file_out_of_the_catalog() {
        let root = temp_dir("catalog-trash-session");
        let active = root.join("active.jsonl");
        let inactive = root.join("inactive.jsonl");
        fs::write(&active, "active").unwrap();
        fs::write(&inactive, "inactive").unwrap();
        trash_session_file(&inactive, Some(&active), &root).unwrap();
        assert!(!inactive.exists());
        assert!(active.exists());
        let _ = fs::remove_dir_all(root);
    }
}
