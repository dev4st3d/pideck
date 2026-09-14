use super::*;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommitSummary {
    pub(crate) id: String,
    pub(crate) parents: Vec<String>,
    pub(crate) author: String,
    pub(crate) timestamp: i64,
    pub(crate) date: String,
    pub(crate) subject: String,
    pub(crate) body: String,
    pub(crate) local: Option<bool>,
    pub(crate) head: bool,
    pub(crate) remote_tip: bool,
}

impl CommitSummary {
    pub(crate) fn short_id(&self) -> &str {
        &self.id[..7]
    }
    pub(crate) fn day(&self) -> &str {
        self.date.get(..10).unwrap_or(&self.date)
    }
    pub(crate) fn time(&self) -> &str {
        self.date.get(11..16).unwrap_or("")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommitFile {
    pub(crate) path: PathBuf,
    pub(crate) relative_path: PathBuf,
    pub(crate) stats: Option<GitLineStats>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommitDetails {
    pub(crate) summary: CommitSummary,
    pub(crate) parent: Option<String>,
    pub(crate) parent_index: usize,
    pub(crate) files: Vec<CommitFile>,
}

impl CommitDetails {
    pub(crate) fn stats(&self) -> Option<GitLineStats> {
        self.files
            .iter()
            .try_fold(GitLineStats::default(), |total, file| {
                let stats = file.stats?;
                Some(GitLineStats {
                    additions: total.additions.checked_add(stats.additions)?,
                    deletions: total.deletions.checked_add(stats.deletions)?,
                })
            })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryPage {
    pub(crate) anchor: Option<String>,
    pub(crate) commits: Vec<CommitSummary>,
    pub(crate) more: bool,
}

fn readable(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

fn parse_log(bytes: &[u8]) -> Result<Vec<CommitSummary>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut fields: Vec<_> = bytes.split(|byte| *byte == 0).collect();
    if fields.last() == Some(&b"".as_slice()) {
        fields.pop();
    }
    if fields.len() % 7 != 0 {
        return Err("Git returned an incomplete history page. Refresh and retry.".into());
    }
    fields
        .chunks_exact(7)
        .map(|fields| {
            let field = |i| String::from_utf8_lossy(fields[i]).into_owned();
            let id = field(0);
            let parents: Vec<_> = field(1).split_whitespace().map(str::to_owned).collect();
            if !oid(&id) || parents.iter().any(|parent| !oid(parent)) {
                return Err("Git returned invalid history references.".into());
            }
            Ok(CommitSummary {
                id,
                parents,
                author: readable(&field(2)),
                timestamp: field(3)
                    .parse()
                    .map_err(|_| "Git returned an invalid commit date.")?,
                date: field(4),
                subject: readable(&field(5)),
                body: readable(&field(6)),
                local: None,
                head: false,
                remote_tip: false,
            })
        })
        .collect()
}

pub(crate) fn history_page(
    project: &Path,
    repository: &Repository,
    anchor: Option<&str>,
    skip: usize,
) -> Result<HistoryPage, String> {
    let anchor = anchor.or(repository.head.as_deref());
    let Some(anchor) = anchor else {
        return Ok(HistoryPage {
            anchor: None,
            commits: Vec::new(),
            more: false,
        });
    };
    if !oid(anchor) || skip > 20_000 {
        return Err("Refresh history before loading more commits.".into());
    }
    let mut arguments = args(&[
        "log",
        "--first-parent",
        "-z",
        "--date=format-local:%Y-%m-%d %H:%M",
        "--format=%H%x00%P%x00%an%x00%ct%x00%cd%x00%s%x00%b",
        &format!("--max-count={}", HISTORY_PAGE_SIZE + 1),
        &format!("--skip={skip}"),
        anchor,
        "--",
    ]);
    let project_root = fs_root(project).map_err(|e| e.to_string())?;
    if project_root != repository.root {
        arguments.push(".".into());
    }
    let mut commits = parse_log(&read(&project_root, &arguments)?)?;
    let more = commits.len() > HISTORY_PAGE_SIZE;
    commits.truncate(HISTORY_PAGE_SIZE);
    let mut outgoing = None;
    let mut remote_tip = None;
    if let Some(upstream) = &repository.upstream {
        let tip = run_git(
            &repository.root,
            &args(&["rev-parse", "--verify", &upstream.tracking_ref]),
        )
        .map_err(|e| e.to_string())?;
        if tip.success {
            remote_tip = Some(String::from_utf8_lossy(&tip.bytes).trim().to_owned());
            outgoing = read(
                &repository.root,
                &args(&["rev-list", anchor, "--not", &upstream.tracking_ref, "--"]),
            )
            .ok()
            .map(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .lines()
                    .map(str::to_owned)
                    .collect::<HashSet<_>>()
            });
        }
    }
    for commit in &mut commits {
        commit.local = outgoing.as_ref().map(|ids| ids.contains(&commit.id));
        commit.head = repository.head.as_ref() == Some(&commit.id);
        commit.remote_tip = remote_tip.as_ref() == Some(&commit.id);
    }
    Ok(HistoryPage {
        anchor: Some(anchor.into()),
        commits,
        more,
    })
}

pub(crate) fn commit_details(
    project: &Path,
    summary: &CommitSummary,
    parent_index: usize,
) -> Result<CommitDetails, String> {
    if !oid(&summary.id) || (parent_index > 0 && parent_index >= summary.parents.len()) {
        return Err("Choose an available comparison parent.".into());
    }
    let parent = summary.parents.get(parent_index).cloned();
    let mut arguments = if let Some(parent) = &parent {
        if !oid(parent) {
            return Err("Invalid comparison parent.".into());
        }
        args(&["diff", parent, &summary.id])
    } else {
        args(&["show", "--format=", "--root", &summary.id])
    };
    arguments.extend(args(&[
        "--numstat",
        "-z",
        "--relative",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--",
        ".",
    ]));
    let numbers = parse_numstat(&read(project, &arguments)?).map_err(|e| e.to_string())?;
    let root = fs_root(project).map_err(|e| e.to_string())?;
    let mut files: Vec<_> = numbers
        .into_iter()
        .map(|(relative_path, stats)| CommitFile {
            path: root.join(&relative_path),
            relative_path,
            stats,
        })
        .collect();
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(CommitDetails {
        summary: summary.clone(),
        parent,
        parent_index,
        files,
    })
}

pub(crate) fn commit_file_diff(
    project: &Path,
    details: &CommitDetails,
    index: usize,
) -> Result<GitDiff, String> {
    let file = details
        .files
        .get(index)
        .ok_or("Choose a changed file in this commit.")?;
    if !oid(&details.summary.id) {
        return Err("Invalid commit ID.".into());
    }
    let mut arguments = args(&["-c", "diff.suppressBlankEmpty=false"]);
    if let Some(parent) = &details.parent {
        if !oid(parent) {
            return Err("Invalid comparison parent.".into());
        }
        arguments.extend(args(&["diff", parent, &details.summary.id]));
    } else {
        arguments.extend(args(&["show", "--format=", "--root", &details.summary.id]));
    }
    arguments.extend(args(&[
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--output-indicator-new=+",
        "--output-indicator-old=-",
        "--output-indicator-context= ",
        "--",
    ]));
    arguments.push(literal(&file.relative_path));
    let output = checked_output(run_git(project, &arguments).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(GitDiff {
        text: String::from_utf8_lossy(&output.bytes).into_owned(),
        truncated: output.truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_parser_preserves_multiline_messages_and_rejects_incomplete_records() {
        let record = format!(
            "{}\0\0Example Author\01700000000\02026-09-14 14:32\0Subject\0A body\nwith lines\0",
            "a".repeat(40)
        );
        let parsed = parse_log(record.as_bytes()).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].body, "A body\nwith lines");
        assert!(parse_log(b"bad\0data\0").is_err());
    }
}
