//! Explicit Git writes and snapshots shared by the sidebar and review.

use super::*;
mod history;
#[cfg(test)]
pub(crate) use history::CommitFile;
#[cfg(test)]
mod tests;
pub(crate) use history::{
    CommitDetails, CommitPrefix, CommitSummary, commit_details, commit_file_diff, day_label,
    history_page,
};
const WRITE_TIMEOUT: Duration = Duration::from_secs(120);
pub(crate) const HISTORY_PAGE_SIZE: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Upstream {
    pub(crate) label: String,
    pub(crate) remote: String,
    pub(crate) destination: String,
    pub(crate) tracking_ref: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Repository {
    pub(crate) root: PathBuf,
    pub(crate) branch: Option<String>,
    pub(crate) head: Option<String>,
    pub(crate) upstream: Option<Upstream>,
    pub(crate) remotes: Vec<String>,
    pub(crate) ahead: Option<usize>,
    pub(crate) behind: Option<usize>,
    pub(crate) index: Vec<u8>,
}

fn read(root: &Path, arguments: &[OsString]) -> Result<Vec<u8>, String> {
    let output = checked_output(run_git(root, arguments).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if output.truncated {
        return Err(
            "Git returned too much data. Select fewer files or open a smaller project.".into(),
        );
    }
    Ok(output.bytes)
}

fn text(root: &Path, arguments: &[OsString]) -> Result<String, String> {
    String::from_utf8(read(root, arguments)?)
        .map(|value| value.trim_end_matches(['\r', '\n']).to_owned())
        .map_err(|_| "Git returned unreadable text.".into())
}

fn oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn optional_head(root: &Path) -> Result<Option<String>, String> {
    let output =
        run_git(root, &args(&["rev-parse", "--verify", "HEAD"])).map_err(|e| e.to_string())?;
    if !output.success {
        return Ok(None);
    }
    let value = String::from_utf8(output.bytes).map_err(|_| "Unreadable commit ID.")?;
    let value = value.trim();
    if !oid(value) {
        return Err("Git returned an invalid commit ID.".into());
    }
    Ok(Some(value.to_owned()))
}

fn index_snapshot(root: &Path) -> Result<Vec<u8>, String> {
    read(
        root,
        &args(&[
            "diff",
            "--cached",
            "--raw",
            "-z",
            "--no-renames",
            "--abbrev=64",
            "--no-ext-diff",
            "--no-textconv",
        ]),
    )
}

pub(crate) fn repository(project: &Path) -> Result<Repository, String> {
    let root = PathBuf::from(text(project, &args(&["rev-parse", "--show-toplevel"]))?);
    let root = fs_root(&root).map_err(|e| e.to_string())?;
    let head = optional_head(&root)?;
    let current = run_git(
        &root,
        &args(&["symbolic-ref", "--quiet", "--short", "HEAD"]),
    )
    .map_err(|e| e.to_string())?;
    let branch = current
        .success
        .then(|| String::from_utf8_lossy(&current.bytes).trim().to_owned());
    let remotes = text(&root, &args(&["remote"]))?
        .lines()
        .map(str::to_owned)
        .collect();
    let mut upstream = None;
    if let Some(branch) = &branch {
        let line = text(
            &root,
            &args(&[
                "for-each-ref",
                "--format=%(upstream:short)%00%(upstream:remotename)%00%(upstream:remoteref)%00%(upstream)",
                &format!("refs/heads/{branch}"),
            ]),
        )?;
        let fields: Vec<_> = line.split('\0').collect();
        if fields.len() == 4 && !fields[0].is_empty() && !fields[1].is_empty() {
            upstream = Some(Upstream {
                label: fields[0].into(),
                remote: fields[1].into(),
                destination: fields[2].into(),
                tracking_ref: fields[3].into(),
            });
        }
    }
    let (mut ahead, mut behind) = (None, None);
    if head.is_some()
        && let Some(upstream) = &upstream
    {
        let output = run_git(
            &root,
            &args(&[
                "rev-list",
                "--left-right",
                "--count",
                &format!("HEAD...{}", upstream.tracking_ref),
                "--",
            ]),
        )
        .map_err(|e| e.to_string())?;
        if output.success {
            let counts = String::from_utf8_lossy(&output.bytes);
            let mut counts = counts.split_whitespace();
            ahead = counts.next().and_then(|value| value.parse().ok());
            behind = counts.next().and_then(|value| value.parse().ok());
        }
    }
    let index = index_snapshot(&root)?;
    Ok(Repository {
        root,
        branch,
        head,
        upstream,
        remotes,
        ahead,
        behind,
        index,
    })
}

fn write(
    root: &Path,
    arguments: &[OsString],
    input: Option<Vec<u8>>,
    recovery: &str,
) -> Result<(), String> {
    let output =
        run_git_with_input(root, arguments, input, WRITE_TIMEOUT).map_err(|e| e.to_string())?;
    if output.success && !output.truncated {
        return Ok(());
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    if diagnostic.contains("unable to auto-detect email")
        || diagnostic.contains("Author identity unknown")
    {
        return Err(
            "Set your Git user.name and user.email, then retry the commit. Your message is kept."
                .into(),
        );
    }
    if diagnostic.contains("index.lock") {
        return Err(
            "Another Git operation is using this repository. Wait for it to finish, then retry."
                .into(),
        );
    }
    Err(recovery.to_owned())
}

fn scoped_path(project: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = fs_root(project).map_err(|e| e.to_string())?;
    let path = super::super::paths::without_windows_verbatim_prefix(path);
    let relative = if path.is_absolute() {
        path.strip_prefix(&root)
            .map_err(|_| "Choose a file inside this project.")?
    } else {
        &path
    };
    relative_path(relative).map_err(|e| e.to_string())?;
    // A final symlink is a Git entry; ancestor symlinks may not escape the project.
    let mut ancestor = root
        .join(relative)
        .parent()
        .map(Path::to_path_buf)
        .ok_or("Invalid file path.")?;
    while !ancestor.exists() {
        if !ancestor.pop() {
            return Err("The file's folder is unavailable.".into());
        }
    }
    if !fs_root(&ancestor)
        .map_err(|e| e.to_string())?
        .starts_with(&root)
    {
        return Err("Choose a file inside this project.".into());
    }
    Ok(relative.to_path_buf())
}

fn literal(path: &Path) -> OsString {
    let mut value = OsString::from(":(literal)");
    value.push(path);
    value
}

pub(crate) fn stage(project: &Path, entries: &[GitEntry], staged: bool) -> Result<(), String> {
    if entries.is_empty() {
        return Err("Choose a changed file first.".into());
    }
    let mut paths = Vec::new();
    for entry in entries {
        paths.push(scoped_path(project, &entry.path)?);
        if let Some(original) = &entry.original_path {
            paths.push(scoped_path(project, original)?);
        }
    }
    paths.sort();
    paths.dedup();
    let mut arguments = if staged {
        args(&["add", "--all"])
    } else if optional_head(project)?.is_some() {
        args(&["restore", "--staged"])
    }
    // An unborn branch has no HEAD. Remove only the selected index entries.
    else {
        args(&["rm", "--cached", "--force", "--ignore-unmatch"])
    };
    arguments.push("--".into());
    arguments.extend(paths.iter().map(|path| literal(path)));
    write(
        project,
        &arguments,
        None,
        if staged {
            "Could not stage the selected files. Refresh and retry."
        } else {
            "Could not unstage the selected files. Refresh and retry."
        },
    )
}

pub(crate) fn commit(
    project: &Path,
    expected: &Repository,
    message: &str,
) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Write a commit message first.".into());
    }
    if message.len() > 8 * 1024 {
        return Err("Keep the commit message under 8 KiB.".into());
    }
    if optional_head(&expected.root)? != expected.head
        || index_snapshot(&expected.root)? != expected.index
    {
        return Err(
            "Staged changes changed since the last refresh. Review them before committing.".into(),
        );
    }
    let scope = fs_root(project).map_err(|e| e.to_string())?;
    let status = read_status(&expected.root).map_err(|e| e.to_string())?;
    if status.truncated {
        return Err("Too many changed files to verify the commit. Commit from the repository root in a terminal.".into());
    }
    if status.entries.iter().any(|entry| entry.conflicted) {
        return Err("Resolve merge conflicts before committing.".into());
    }
    if !status.entries.iter().any(GitEntry::staged) {
        return Err("Stage at least one change before committing.".into());
    }
    if status
        .entries
        .iter()
        .any(|entry| entry.staged() && !entry.path.starts_with(&scope))
    {
        return Err("Other project folders have staged changes. Open the repository root to commit them together.".into());
    }
    write(
        &expected.root,
        &args(&["commit", "--file=-"]),
        Some(message.as_bytes().to_vec()),
        "Commit failed. Check your Git hooks or signing setup, then retry. Your message is kept.",
    )?;
    optional_head(&expected.root)?.ok_or_else(|| {
        "Git did not return the saved commit. Refresh history before retrying.".into()
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PushPlan {
    pub(crate) repository: Repository,
    pub(crate) remote: String,
    pub(crate) destination: String,
    pub(crate) publish: bool,
}

impl PushPlan {
    pub(crate) fn label(&self) -> String {
        format!(
            "{}/{}",
            self.remote,
            self.destination
                .strip_prefix("refs/heads/")
                .unwrap_or(&self.destination)
        )
    }
}

pub(crate) fn push_plan(repository: &Repository, remote: Option<&str>) -> Result<PushPlan, String> {
    let branch = repository
        .branch
        .as_deref()
        .ok_or("Create or switch to a branch before pushing.")?;
    if repository.head.is_none() {
        return Err("Create a commit before publishing this branch.".into());
    }
    if let Some(upstream) = &repository.upstream {
        if !repository.remotes.contains(&upstream.remote) {
            return Err(
                "The upstream remote is unavailable. Check your Git remote configuration.".into(),
            );
        }
        return Ok(PushPlan {
            repository: repository.clone(),
            remote: upstream.remote.clone(),
            destination: upstream.destination.clone(),
            publish: false,
        });
    }
    let remote = remote.ok_or("Choose a remote to publish this branch.")?;
    if !repository.remotes.iter().any(|name| name == remote) {
        return Err("Choose an available Git remote.".into());
    }
    Ok(PushPlan {
        repository: repository.clone(),
        remote: remote.into(),
        destination: format!("refs/heads/{branch}"),
        publish: true,
    })
}

pub(crate) fn push(plan: &PushPlan) -> Result<(), String> {
    let expected = &plan.repository;
    let current = repository(&expected.root)?;
    if current.head != expected.head
        || current.branch != expected.branch
        || current.upstream != expected.upstream
    {
        return Err(
            "The branch changed. Refresh and review the outgoing commits before pushing.".into(),
        );
    }
    if !current.remotes.contains(&plan.remote) || !plan.destination.starts_with("refs/heads/") {
        return Err("The push destination is no longer available. Refresh and retry.".into());
    }
    let head = current.head.as_deref().ok_or("Create a commit first.")?;
    if !run_git(
        &current.root,
        &args(&["check-ref-format", &plan.destination]),
    )
    .map_err(|e| e.to_string())?
    .success
    {
        return Err("Choose a valid remote branch.".into());
    }
    // Pin the source object so an external commit cannot silently expand this push.
    write(
        &current.root,
        &args(&[
            "push",
            "--porcelain",
            "--no-force",
            "--no-mirror",
            "--no-follow-tags",
            "--no-prune",
            "--",
            &plan.remote,
            &format!("{head}:{}", plan.destination),
        ]),
        None,
        "Push did not finish. Your commits are saved locally. Check authentication or your connection; if the remote changed, fetch and review before retrying.",
    )?;
    if plan.publish {
        let branch = current
            .branch
            .as_deref()
            .ok_or("The branch is no longer available.")?;
        write(
            &current.root,
            &args(&[
                "branch",
                &format!("--set-upstream-to={}", plan.label()),
                "--",
                branch,
            ]),
            None,
            "The push succeeded, but upstream tracking could not be saved. Set the branch upstream before pushing again.",
        )?;
    }
    Ok(())
}

pub(crate) fn fetch(repository: &Repository, remote: &str) -> Result<(), String> {
    if !repository.remotes.iter().any(|name| name == remote) {
        return Err("Choose an available Git remote.".into());
    }
    write(
        &repository.root,
        &args(&["fetch", "--no-recurse-submodules", "--", remote]),
        None,
        "Fetch failed. Check authentication and your connection, then retry.",
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    index: Vec<u8>,
    contents: Option<String>,
    head: Option<String>,
}

fn stamp(project: &Path, path: &Path) -> Result<FileStamp, String> {
    let relative = scoped_path(project, path)?;
    let mut arguments = args(&["ls-files", "--stage", "-z", "--"]);
    arguments.push(literal(&relative));
    let index = read(project, &arguments)?;
    let contents = match std::fs::symlink_metadata(project.join(&relative)) {
        Ok(metadata) if metadata.file_type().is_file() => {
            let mut arguments = args(&["hash-object", "--no-filters", "--"]); arguments.push(relative.as_os_str().to_owned());
            Some(text(project, &arguments)?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        _ => return Err("Discard is only available for regular files. Handle symlinks and submodules in a terminal.".into()),
    };
    Ok(FileStamp {
        index,
        contents,
        head: optional_head(project)?,
    })
}

#[derive(Clone, Debug)]
pub(crate) struct DiscardPlan {
    pub(crate) project: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) untracked: bool,
    pub(crate) hunk: Option<usize>,
    snapshot: FileStamp,
    patch: Option<Vec<u8>>,
}

fn hunk_patch(diff: &str, hunk: usize) -> Result<Vec<u8>, String> {
    let lines: Vec<_> = diff.split_inclusive('\n').collect();
    let hunks: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| line.starts_with("@@ ").then_some(i))
        .collect();
    let start = *hunks
        .get(hunk)
        .ok_or("This hunk changed. Refresh the diff before discarding it.")?;
    let end = hunks.get(hunk + 1).copied().unwrap_or(lines.len());
    let header = *hunks.first().ok_or("This file has no text hunks.")?;
    if lines[..header].iter().any(|line| {
        line.starts_with("new file")
            || line.starts_with("deleted file")
            || line.starts_with("rename ")
            || line.starts_with("old mode")
    }) {
        return Err("Use Undo file for added, deleted, renamed, or mode-changed files.".into());
    }
    Ok(lines[..header]
        .iter()
        .chain(lines[start..end].iter())
        .copied()
        .collect::<String>()
        .into_bytes())
}

pub(crate) fn prepare_discard(
    project: &Path,
    entry: &GitEntry,
    hunk: Option<(usize, String)>,
) -> Result<DiscardPlan, String> {
    if entry.conflicted {
        return Err("Resolve this file's merge conflict before discarding changes.".into());
    }
    let project = fs_root(project).map_err(|e| e.to_string())?;
    let path = project.join(scoped_path(&project, &entry.path)?);
    let before = stamp(&project, &path)?;
    let current = read_status(&project).map_err(|e| e.to_string())?;
    if !current.entries.iter().any(|candidate| {
        candidate.path == path
            && candidate.untracked == entry.untracked
            && candidate.unstaged()
            && !candidate.conflicted
    }) {
        return Err("The file's Git state changed. Refresh before discarding it.".into());
    }
    let (hunk, patch) = if let Some((index, expected)) = hunk {
        if entry.untracked {
            return Err("Use Undo file for a new file.".into());
        }
        let diff = file_diff(&project, &path, DiffKind::WorkingTree).map_err(|e| e.to_string())?;
        if diff.truncated || diff.text != expected {
            return Err("The diff changed. Refresh before discarding this hunk.".into());
        }
        (Some(index), Some(hunk_patch(&diff.text, index)?))
    } else {
        (None, None)
    };
    if stamp(&project, &path)? != before {
        return Err("The file changed while preparing the discard. Refresh and retry.".into());
    }
    Ok(DiscardPlan {
        project,
        path,
        untracked: entry.untracked,
        hunk,
        snapshot: before,
        patch,
    })
}

pub(crate) fn discard(plan: &DiscardPlan) -> Result<(), String> {
    if stamp(&plan.project, &plan.path)? != plan.snapshot {
        return Err(
            "The file or staged version changed. Review the new changes before discarding.".into(),
        );
    }
    if plan.untracked {
        let result = super::super::file_operations::run(
            &plan.project,
            super::super::file_operations::Operation::Trash(vec![plan.path.clone()]),
            &AtomicBool::new(false),
        );
        return result.error.map_or(Ok(()), Err);
    }
    if let Some(patch) = &plan.patch {
        let root = PathBuf::from(text(
            &plan.project,
            &args(&["rev-parse", "--show-toplevel"]),
        )?);
        write(
            &root,
            &args(&["apply", "--reverse", "--check", "--whitespace=nowarn", "-"]),
            Some(patch.clone()),
            "This hunk no longer applies. Refresh and review it again.",
        )?;
        return write(
            &root,
            &args(&["apply", "--reverse", "--whitespace=nowarn", "-"]),
            Some(patch.clone()),
            "Could not discard the hunk. Refresh and review the file.",
        );
    }
    let mut arguments = args(&["restore", "--worktree", "--"]);
    arguments.push(literal(&scoped_path(&plan.project, &plan.path)?));
    write(
        &plan.project,
        &arguments,
        None,
        "Could not discard this file's unstaged changes. Refresh and retry.",
    )
}
