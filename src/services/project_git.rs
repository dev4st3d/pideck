//! Bounded Git inspection and explicit, non-forced local branch switching.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_STATUS_ENTRIES: usize = 2_000;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitEntry {
    pub(crate) path: PathBuf,
    pub(crate) relative_path: PathBuf,
    pub(crate) original_path: Option<PathBuf>,
    pub(crate) index_status: char,
    pub(crate) worktree_status: char,
    pub(crate) untracked: bool,
    pub(crate) conflicted: bool,
    pub(crate) line_stats: Option<GitLineStats>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GitLineStats {
    pub(crate) additions: usize,
    pub(crate) deletions: usize,
}

impl GitEntry {
    pub(crate) fn staged(&self) -> bool {
        !self.untracked && !matches!(self.index_status, ' ' | '!')
    }

    pub(crate) fn unstaged(&self) -> bool {
        self.untracked || !matches!(self.worktree_status, ' ' | '!')
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitStatus {
    pub(crate) branch: String,
    pub(crate) entries: Vec<GitEntry>,
    pub(crate) truncated: bool,
}

impl GitStatus {
    pub(crate) fn line_stats(&self) -> Option<GitLineStats> {
        if self.truncated {
            return None;
        }
        self.entries
            .iter()
            .try_fold(GitLineStats::default(), |total, entry| {
                let lines = entry.line_stats?;
                Some(GitLineStats {
                    additions: total.additions.checked_add(lines.additions)?,
                    deletions: total.deletions.checked_add(lines.deletions)?,
                })
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DiffKind {
    Staged,
    WorkingTree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitDiff {
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitError {
    Unavailable,
    NotRepository,
    ReadFailed,
    TimedOut,
    InvalidPath,
    InvalidOutput,
    SwitchFailed,
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "Git is not available. Install Git and reopen the app.",
            Self::NotRepository => "This project is not in a Git repository.",
            Self::ReadFailed => {
                "Git could not read this project. Check its permissions and refresh."
            }
            Self::TimedOut => "Git took too long to respond. Try refreshing this project.",
            Self::InvalidPath => "Choose a file inside this project to view its changes.",
            Self::InvalidOutput => {
                "Git returned an unreadable status. Try refreshing this project."
            }
            Self::SwitchFailed => "Could not switch branches. Commit or stash conflicting changes and check whether the branch is open in another worktree.",
        })
    }
}

pub(crate) fn branches(root: &Path) -> Result<Vec<String>, GitError> {
    let output = checked_output(run_git(
        root,
        &args(&["for-each-ref", "--format=%(refname:short)", "refs/heads/"]),
    )?)?;
    if output.truncated {
        return Err(GitError::InvalidOutput);
    }
    Ok(String::from_utf8(output.bytes)
        .map_err(|_| GitError::InvalidOutput)?
        .lines()
        .map(str::to_owned)
        .collect())
}

pub(crate) fn switch_branch(root: &Path, branch: &str) -> Result<(), GitError> {
    if !branches(root)?.iter().any(|name| name == branch) {
        return Err(GitError::InvalidPath);
    }
    // Explicit local branches only. No force, auto-stash, remote guessing or hooks.
    let output = run_git(
        root,
        &args(&[
            "-c",
            "core.hooksPath=/dev/null",
            "switch",
            "--no-guess",
            "--",
            branch,
        ]),
    )?;
    if output.success {
        Ok(())
    } else {
        Err(GitError::SwitchFailed)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ChangeRow {
    pub(crate) path: PathBuf,
    pub(crate) depth: usize,
    pub(crate) kind: DiffKind,
    pub(crate) entry: Option<usize>,
}

pub(crate) fn change_rows(
    status: &GitStatus,
    collapsed: &std::collections::HashSet<(DiffKind, PathBuf)>,
) -> Vec<ChangeRow> {
    #[derive(Default)]
    struct Node {
        children: std::collections::BTreeMap<std::ffi::OsString, Node>,
        entry: Option<usize>,
    }
    fn append(
        node: &Node,
        path: &Path,
        depth: usize,
        kind: DiffKind,
        collapsed: &std::collections::HashSet<(DiffKind, PathBuf)>,
        rows: &mut Vec<ChangeRow>,
    ) {
        for directory in [true, false] {
            for (name, child) in &node.children {
                if child.entry.is_none() != directory {
                    continue;
                }
                let path = path.join(name);
                rows.push(ChangeRow {
                    path: path.clone(),
                    depth,
                    kind,
                    entry: child.entry,
                });
                if directory && !collapsed.contains(&(kind, path.clone())) {
                    append(child, &path, depth + 1, kind, collapsed, rows);
                }
            }
        }
    }
    let mut rows = Vec::new();
    for kind in [DiffKind::WorkingTree, DiffKind::Staged] {
        let mut root = Node::default();
        for (index, entry) in status.entries.iter().enumerate() {
            if !(if kind == DiffKind::Staged {
                entry.staged()
            } else {
                entry.unstaged()
            }) {
                continue;
            }
            let mut node = &mut root;
            for component in entry.relative_path.components() {
                node = node
                    .children
                    .entry(component.as_os_str().to_owned())
                    .or_default();
            }
            node.entry = Some(index);
        }
        if root.children.is_empty() {
            continue;
        }
        rows.push(ChangeRow {
            path: PathBuf::new(),
            depth: 0,
            kind,
            entry: None,
        });
        if !collapsed.contains(&(kind, PathBuf::new())) {
            append(&root, Path::new(""), 1, kind, collapsed, &mut rows);
        }
    }
    rows
}

impl std::error::Error for GitError {}

struct CommandOutput {
    bytes: Vec<u8>,
    stderr: Vec<u8>,
    success: bool,
    truncated: bool,
}

fn read_pipe(mut pipe: impl Read, limit: usize, exceeded: &AtomicBool) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8 * 1024];
    loop {
        let count = pipe.read(&mut chunk)?;
        if count == 0 {
            return Ok(bytes);
        }
        let available = limit.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..count.min(available)]);
        if count > available {
            exceeded.store(true, Ordering::Release);
            return Ok(bytes);
        }
    }
}

fn run_git(root: &Path, args: &[OsString]) -> Result<CommandOutput, GitError> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "color.ui=false",
        ])
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW keeps background reads unobtrusive.
    }
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            GitError::Unavailable
        } else {
            GitError::ReadFailed
        }
    })?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(GitError::ReadFailed);
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(GitError::ReadFailed);
    };
    let output_exceeded = Arc::new(AtomicBool::new(false));
    let error_exceeded = Arc::new(AtomicBool::new(false));
    let output_reader = thread::Builder::new()
        .name("pideck-git-output".into())
        .spawn({
            let exceeded = Arc::clone(&output_exceeded);
            move || read_pipe(stdout, MAX_OUTPUT_BYTES, &exceeded)
        });
    let output_reader = match output_reader {
        Ok(reader) => reader,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(GitError::ReadFailed);
        }
    };
    let error_reader = thread::Builder::new()
        .name("pideck-git-error".into())
        .spawn({
            let exceeded = Arc::clone(&error_exceeded);
            move || read_pipe(stderr, 16 * 1024, &exceeded)
        });
    let error_reader = match error_reader {
        Ok(reader) => reader,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(GitError::ReadFailed);
        }
    };
    let started = Instant::now();
    let completion = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status.success()),
            Ok(None) => {}
            Err(_) => break Err(GitError::ReadFailed),
        }
        if error_exceeded.load(Ordering::Acquire) {
            break Err(GitError::ReadFailed);
        }
        if output_exceeded.load(Ordering::Acquire) {
            break Ok(false);
        }
        if started.elapsed() >= COMMAND_TIMEOUT {
            break Err(GitError::TimedOut);
        }
        thread::sleep(Duration::from_millis(10));
    };
    let _ = child.kill();
    let _ = child.wait();
    let success = completion?;
    // A descendant inheriting a pipe must not extend the command deadline.
    // Dropping unfinished handles detaches their readers instead of blocking the UI worker.
    while !output_reader.is_finished() || !error_reader.is_finished() {
        if started.elapsed() >= COMMAND_TIMEOUT {
            return Err(GitError::TimedOut);
        }
        thread::sleep(Duration::from_millis(10));
    }
    let bytes = output_reader
        .join()
        .map_err(|_| GitError::ReadFailed)?
        .map_err(|_| GitError::ReadFailed)?;
    let stderr = error_reader
        .join()
        .map_err(|_| GitError::ReadFailed)?
        .map_err(|_| GitError::ReadFailed)?;
    if error_exceeded.load(Ordering::Acquire) {
        return Err(GitError::ReadFailed);
    }
    Ok(CommandOutput {
        bytes,
        stderr,
        success,
        truncated: output_exceeded.load(Ordering::Acquire),
    })
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(|value| OsString::from(*value)).collect()
}

fn checked_output(output: CommandOutput) -> Result<CommandOutput, GitError> {
    if output.success || output.truncated {
        return Ok(output);
    }
    if String::from_utf8_lossy(&output.stderr).contains("not a git repository") {
        Err(GitError::NotRepository)
    } else {
        Err(GitError::ReadFailed)
    }
}

fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, GitError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes)
            .map(PathBuf::from)
            .map_err(|_| GitError::InvalidOutput)
    }
}

fn relative_path(path: &Path) -> Result<(), GitError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(GitError::InvalidPath);
    }
    Ok(())
}

fn parse_status(root: &Path, bytes: &[u8], mut truncated: bool) -> Result<GitStatus, GitError> {
    let mut records = bytes.split_inclusive(|byte| *byte == 0);
    let mut branch = String::from("Detached HEAD");
    let mut entries = Vec::new();
    while let Some(record) = records.next() {
        let Some(record) = record.strip_suffix(&[0]) else {
            if truncated {
                break;
            }
            return Err(GitError::InvalidOutput);
        };
        if let Some(header) = record.strip_prefix(b"## ") {
            let header = String::from_utf8_lossy(header);
            branch = header
                .strip_prefix("No commits yet on ")
                .or_else(|| header.strip_prefix("Initial commit on "))
                .unwrap_or(&header)
                .split("...")
                .next()
                .unwrap_or("Detached HEAD")
                .to_owned();
            if branch.starts_with("HEAD (") {
                branch = "Detached HEAD".into();
            }
            continue;
        }
        if record.len() < 4 || record[2] != b' ' {
            return Err(GitError::InvalidOutput);
        }
        let path = path_from_bytes(&record[3..])?;
        relative_path(&path)?;
        let index_status = char::from(record[0]);
        let worktree_status = char::from(record[1]);
        let renamed = matches!(index_status, 'R' | 'C') || matches!(worktree_status, 'R' | 'C');
        let original_path = if renamed {
            let Some(original) = records.next().and_then(|record| record.strip_suffix(&[0])) else {
                if truncated {
                    break;
                }
                return Err(GitError::InvalidOutput);
            };
            let original = path_from_bytes(original)?;
            relative_path(&original)?;
            Some(root.join(original))
        } else {
            None
        };
        if entries.len() == MAX_STATUS_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(GitEntry {
            path: root.join(&path),
            relative_path: path,
            original_path,
            index_status,
            worktree_status,
            untracked: index_status == '?' && worktree_status == '?',
            conflicted: index_status == 'U'
                || worktree_status == 'U'
                || matches!((index_status, worktree_status), ('A', 'A') | ('D', 'D')),
            line_stats: None,
        });
    }
    Ok(GitStatus {
        branch,
        entries,
        truncated,
    })
}

pub(crate) fn read_status(root: &Path) -> Result<GitStatus, GitError> {
    let project_root = fs_root(root)?;
    let repository = checked_output(run_git(
        &project_root,
        &args(&["rev-parse", "--show-toplevel"]),
    )?)?;
    let repository_path = path_from_bytes(
        repository
            .bytes
            .strip_suffix(b"\n")
            .unwrap_or(&repository.bytes),
    )?;
    let repository_root = fs_root(&repository_path)?;
    let output = checked_output(run_git(
        &project_root,
        &args(&[
            "status",
            "--porcelain=v1",
            "-z",
            "--branch",
            "--untracked-files=all",
            "--",
            ".",
        ]),
    )?)?;
    let mut status = parse_status(&repository_root, &output.bytes, output.truncated)?;
    status
        .entries
        .retain(|entry| entry.path.starts_with(&project_root));
    for entry in &mut status.entries {
        entry.relative_path = entry
            .path
            .strip_prefix(&project_root)
            .map_err(|_| GitError::InvalidPath)?
            .to_path_buf();
    }
    // Two bounded reads cover the entire project, including staged-only changes.
    // Statistics are optional so binary files or unavailable counts stay honest.
    let working_stats = read_numstat(&project_root, false);
    let staged_stats = read_numstat(&project_root, true);
    for entry in &mut status.entries {
        if entry.untracked || entry.conflicted {
            continue;
        }
        let mut total = GitLineStats::default();
        let mut complete = true;
        for (needed, stats) in [
            (entry.unstaged(), &working_stats),
            (entry.staged(), &staged_stats),
        ] {
            if !needed {
                continue;
            }
            match stats {
                Ok(stats) => match stats.get(&entry.relative_path) {
                    Some(Some(lines)) => {
                        total.additions += lines.additions;
                        total.deletions += lines.deletions;
                    }
                    Some(None) | None => complete = false,
                },
                Err(_) => complete = false,
            }
        }
        entry.line_stats = complete.then_some(total);
    }
    Ok(status)
}

fn read_numstat(
    root: &Path,
    staged: bool,
) -> Result<HashMap<PathBuf, Option<GitLineStats>>, GitError> {
    let mut arguments = args(&[
        "diff",
        "--numstat",
        "-z",
        "--relative",
        "--no-ext-diff",
        "--no-textconv",
    ]);
    if staged {
        arguments.push("--cached".into());
    }
    arguments.extend(args(&["--", "."]));
    let output = checked_output(run_git(root, &arguments)?)?;
    if output.truncated {
        return Err(GitError::InvalidOutput);
    }
    parse_numstat(&output.bytes)
}

fn parse_numstat(bytes: &[u8]) -> Result<HashMap<PathBuf, Option<GitLineStats>>, GitError> {
    let mut records = bytes.split_inclusive(|byte| *byte == 0);
    let mut result = HashMap::new();
    while let Some(record) = records.next() {
        let record = record.strip_suffix(&[0]).ok_or(GitError::InvalidOutput)?;
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let additions = fields.next().ok_or(GitError::InvalidOutput)?;
        let deletions = fields.next().ok_or(GitError::InvalidOutput)?;
        let path = fields.next().ok_or(GitError::InvalidOutput)?;
        let path = if path.is_empty() {
            // In -z mode Git emits renamed paths as a separate old/new pair.
            let old = records
                .next()
                .and_then(|record| record.strip_suffix(&[0]))
                .ok_or(GitError::InvalidOutput)?;
            relative_path(&path_from_bytes(old)?)?;
            records
                .next()
                .and_then(|record| record.strip_suffix(&[0]))
                .ok_or(GitError::InvalidOutput)?
        } else {
            path
        };
        let path = path_from_bytes(path)?;
        relative_path(&path)?;
        let stats = if additions == b"-" && deletions == b"-" {
            None
        } else {
            let number = |value| {
                std::str::from_utf8(value)
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or(GitError::InvalidOutput)
            };
            Some(GitLineStats {
                additions: number(additions)?,
                deletions: number(deletions)?,
            })
        };
        result.insert(path, stats);
    }
    Ok(result)
}

fn fs_root(root: &Path) -> Result<PathBuf, GitError> {
    let path = std::fs::canonicalize(root).map_err(|_| GitError::ReadFailed)?;
    Ok(super::paths::without_windows_verbatim_prefix(&path))
}

pub(crate) fn file_diff(root: &Path, path: &Path, kind: DiffKind) -> Result<GitDiff, GitError> {
    let root = fs_root(root)?;
    let path = super::paths::without_windows_verbatim_prefix(path);
    let path = if path.is_absolute() {
        path.strip_prefix(&root)
            .map_err(|_| GitError::InvalidPath)?
    } else {
        path.as_path()
    };
    relative_path(path)?;
    // The side-by-side parser consumes standard unified-diff prefixes, even
    // when the repository normally suppresses blank context indicators.
    let mut arguments = args(&[
        "-c",
        "diff.suppressBlankEmpty=false",
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--output-indicator-new=+",
        "--output-indicator-old=-",
        "--output-indicator-context= ",
    ]);
    if kind == DiffKind::Staged {
        arguments.push("--cached".into());
    }
    arguments.push("--".into());
    arguments.push(OsStr::new(":(literal)").to_os_string());
    // Literal pathspecs prevent filenames containing '*' or ':' from selecting
    // unrelated files. Prefix and path must be one argument.
    if let Some(argument) = arguments.last_mut() {
        argument.push(path);
    }
    let output = checked_output(run_git(&root, &arguments)?)?;
    Ok(GitDiff {
        text: String::from_utf8_lossy(&output.bytes).into_owned(),
        truncated: output.truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn tree_groups_staging_and_preserves_deleted_paths() {
        let status = parse_status(
            Path::new("project"),
            b"## main\0MM src/a.rs\0 D src/gone.rs\0?? new.txt\0",
            false,
        )
        .unwrap();
        let mut collapsed = std::collections::HashSet::new();
        let rows = change_rows(&status, &collapsed);
        assert_eq!(rows.iter().filter(|row| row.entry == Some(0)).count(), 2);
        assert!(rows.iter().any(|row| row.path == Path::new("src/gone.rs")));
        collapsed.insert((DiffKind::WorkingTree, PathBuf::from("src")));
        let rows = change_rows(&status, &collapsed);
        assert!(!rows.iter().any(|row| row.path == Path::new("src/gone.rs")));
        assert!(
            rows.iter()
                .any(|row| row.kind == DiffKind::Staged && row.path == Path::new("src/a.rs"))
        );
    }

    #[test]
    fn project_totals_require_counts_for_every_changed_file() {
        let mut status =
            parse_status(Path::new("project"), b"## main\0 M a.rs\0 M b.rs\0", false).unwrap();
        status.entries[0].line_stats = Some(GitLineStats {
            additions: 12,
            deletions: 4,
        });
        assert_eq!(status.line_stats(), None);
        status.entries[1].line_stats = Some(GitLineStats {
            additions: 6,
            deletions: 2,
        });
        assert_eq!(
            status.line_stats(),
            Some(GitLineStats {
                additions: 18,
                deletions: 6
            })
        );
        status.truncated = true;
        assert_eq!(status.line_stats(), None);
        status.truncated = false;
        status.entries[1].line_stats = Some(GitLineStats {
            additions: usize::MAX,
            deletions: 2,
        });
        assert_eq!(status.line_stats(), None);
    }

    #[test]
    fn numstat_preserves_special_paths_and_distinguishes_binary_files() {
        let stats = parse_numstat(
            b"12\t4\tsrc/a\tfile.rs\0-\t-\timage.png\00\t0\t\0old.rs\0new\nname.rs\0",
        )
        .unwrap();
        assert_eq!(
            stats[Path::new("src/a\tfile.rs")],
            Some(GitLineStats {
                additions: 12,
                deletions: 4
            })
        );
        assert_eq!(stats[Path::new("image.png")], None);
        assert_eq!(
            stats[Path::new("new\nname.rs")],
            Some(GitLineStats::default())
        );
        assert!(parse_numstat(b"2\t1\tpartial").is_err());
        assert!(parse_numstat(b"2\t1\t../outside\0").is_err());
        assert!(parse_numstat(b"2\t1\t\0old\0").is_err());
    }

    struct TestRepository(PathBuf);

    #[test]
    fn branch_switch_preserves_conflicting_local_changes() {
        let root = std::env::temp_dir().join(format!(
            "pideck-branches-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = TestRepository(root);
        let init = run_git(
            &root.0,
            &args(&["init", "--quiet", "--initial-branch=main", "--template="]),
        );
        if matches!(init, Err(GitError::Unavailable)) {
            return;
        }
        assert!(init.unwrap().success);
        std::fs::write(root.0.join("file.txt"), "main\n").unwrap();
        assert!(
            run_git(&root.0, &args(&["add", "file.txt"]))
                .unwrap()
                .success
        );
        let commit = [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ];
        assert!(run_git(&root.0, &args(&commit)).unwrap().success);
        assert!(
            run_git(&root.0, &args(&["branch", "feature/nested"]))
                .unwrap()
                .success
        );
        assert!(
            branches(&root.0)
                .unwrap()
                .contains(&"feature/nested".to_owned())
        );
        switch_branch(&root.0, "feature/nested").unwrap();
        std::fs::write(root.0.join("file.txt"), "feature\n").unwrap();
        assert!(
            run_git(&root.0, &args(&["add", "file.txt"]))
                .unwrap()
                .success
        );
        assert!(run_git(&root.0, &args(&commit)).unwrap().success);
        switch_branch(&root.0, "main").unwrap();
        std::fs::write(root.0.join("file.txt"), "unsaved on disk\n").unwrap();
        assert_eq!(
            switch_branch(&root.0, "feature/nested"),
            Err(GitError::SwitchFailed)
        );
        assert_eq!(
            std::fs::read_to_string(root.0.join("file.txt")).unwrap(),
            "unsaved on disk\n"
        );
        assert_eq!(read_status(&root.0).unwrap().branch, "main");
        assert_eq!(
            switch_branch(&root.0, "--discard-changes"),
            Err(GitError::InvalidPath)
        );
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn porcelain_paths_are_nul_delimited_and_renames_keep_both_names() {
        let root = Path::new("project");
        let status = parse_status(root, b"## main...origin/main [ahead 1]\0 M a file.txt\0R  new\nname.txt\0old\tname.txt\0?? --new.txt\0UU conflict.txt\0", false).unwrap();
        assert_eq!(status.branch, "main");
        assert_eq!(status.entries.len(), 4);
        assert!(status.entries[0].unstaged());
        assert!(!status.entries[0].staged());
        assert_eq!(status.entries[1].relative_path, Path::new("new\nname.txt"));
        assert_eq!(
            status.entries[1].original_path,
            Some(root.join("old\tname.txt"))
        );
        assert!(status.entries[1].staged());
        assert!(status.entries[2].untracked);
        assert!(status.entries[3].conflicted);
    }

    #[test]
    fn unborn_branches_and_truncated_records_are_handled() {
        let status = parse_status(
            Path::new("project"),
            b"## No commits yet on trunk\0?? complete\0R  incomplete\0partial",
            true,
        )
        .unwrap();
        assert_eq!(status.branch, "trunk");
        assert_eq!(status.entries.len(), 1);
        assert!(status.truncated);
        assert_eq!(
            parse_status(Path::new("project"), b" M partial", false),
            Err(GitError::InvalidOutput)
        );
        assert_eq!(
            parse_status(Path::new("project"), b" M ../outside\0", false),
            Err(GitError::InvalidPath)
        );
    }

    #[test]
    fn bounded_pipe_reads_stop_at_the_limit() {
        let exceeded = AtomicBool::new(false);
        let bytes = read_pipe(&b"123456789"[..], 5, &exceeded).unwrap();
        assert_eq!(bytes, b"12345");
        assert!(exceeded.load(Ordering::Acquire));
    }

    #[test]
    fn status_entry_count_is_bounded_without_corrupting_paths() {
        let mut bytes = b"## fixture\0".to_vec();
        for index in 0..=MAX_STATUS_ENTRIES {
            bytes.extend_from_slice(format!("?? file-{index}\0").as_bytes());
        }
        let status = parse_status(Path::new("project"), &bytes, false).unwrap();
        assert_eq!(status.entries.len(), MAX_STATUS_ENTRIES);
        assert!(status.truncated);
    }

    #[test]
    fn reads_staged_and_worktree_diffs_in_a_synthetic_nested_project_without_changing_index() {
        let root = std::env::temp_dir().join(format!(
            "pideck-git-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = TestRepository(root);
        let initialized = run_git(
            &root.0,
            &args(&["init", "--quiet", "--initial-branch=fixture", "--template="]),
        );
        if matches!(initialized, Err(GitError::Unavailable)) {
            eprintln!("Git is unavailable; skipping the synthetic repository integration fixture.");
            return;
        }
        assert!(initialized.unwrap().success);
        assert!(
            run_git(
                &root.0,
                &args(&["config", "--local", "diff.suppressBlankEmpty", "true"]),
            )
            .unwrap()
            .success
        );
        let configuration = std::fs::read(root.0.join(".git/config")).unwrap();
        let project = root.0.join("nested");
        std::fs::create_dir(&project).unwrap();
        let file = project.join("a[1].txt");
        std::fs::write(&file, b"baseline\n\ncontext\n").unwrap();
        assert!(
            run_git(&root.0, &args(&["add", "--", "nested/a[1].txt"]))
                .unwrap()
                .success
        );
        std::fs::write(&file, b"edited\n\ncontext\n").unwrap();
        std::fs::write(project.join("a1.txt"), b"unrelated\n").unwrap();
        std::fs::write(root.0.join("outside.txt"), b"outside project\n").unwrap();
        let index = std::fs::read(root.0.join(".git/index")).unwrap();
        let status = read_status(&project).unwrap();
        assert_eq!(status.branch, "fixture");
        assert_eq!(status.entries.len(), 2);
        assert!(
            status
                .entries
                .iter()
                .all(|entry| entry.relative_path != Path::new("outside.txt"))
        );
        let tracked = status
            .entries
            .iter()
            .find(|entry| entry.relative_path == Path::new("a[1].txt"))
            .unwrap();
        assert!(tracked.staged() && tracked.unstaged());
        assert_eq!(
            tracked.line_stats,
            Some(GitLineStats {
                additions: 4,
                deletions: 1
            })
        );
        assert!(
            status
                .entries
                .iter()
                .filter(|entry| entry.untracked)
                .all(|entry| entry.line_stats.is_none())
        );
        let staged = file_diff(&project, &tracked.path, DiffKind::Staged).unwrap();
        assert!(staged.text.contains("+baseline"));
        let worktree = file_diff(&project, &tracked.path, DiffKind::WorkingTree).unwrap();
        assert!(worktree.text.contains("-baseline") && worktree.text.contains("+edited"));
        assert!(worktree.text.contains("\n \n context\n"));
        assert!(!worktree.text.contains("unrelated"));
        assert_eq!(std::fs::read(root.0.join(".git/index")).unwrap(), index);
        assert_eq!(
            std::fs::read(root.0.join(".git/config")).unwrap(),
            configuration
        );
    }
}
