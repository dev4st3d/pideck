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
    pub(crate) fn prefix(&self) -> Option<CommitPrefix<'_>> {
        conventional_prefix(&self.subject)
    }
}

/// A Conventional Commits header such as `fix(ui)!: keep focus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CommitPrefix<'a> {
    pub(crate) kind: &'a str,
    pub(crate) scope: Option<&'a str>,
    pub(crate) breaking: bool,
    pub(crate) summary: &'a str,
}

fn conventional_prefix(subject: &str) -> Option<CommitPrefix<'_>> {
    let (head, summary) = subject.split_once(':')?;
    // Require whitespace after the colon so text such as `std::io` is not a prefix.
    if !summary.starts_with(char::is_whitespace) {
        return None;
    }
    let summary = summary.trim_start();
    if summary.is_empty() {
        return None;
    }
    let (head, breaking) = match head.strip_suffix('!') {
        Some(head) => (head, true),
        None => (head, false),
    };
    let (kind, scope) = match head.split_once('(') {
        Some((kind, scope)) => {
            let scope = scope.strip_suffix(')')?;
            if scope.is_empty() || scope.contains(['(', ')']) {
                return None;
            }
            (kind, Some(scope))
        }
        None => (head, None),
    };
    if kind.is_empty() || kind.len() > 16 || !kind.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    Some(CommitPrefix {
        kind,
        scope,
        breaking,
        summary,
    })
}

/// Days since 1970-01-01 for a proleptic Gregorian `YYYY-MM-DD` (Hinnant's days_from_civil).
fn civil_day(day: &str) -> Option<i64> {
    let mut parts = day.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

/// Inverse of [`civil_day`]: `(year, month, day)`.
fn civil_date(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

/// The local day number at `now`. Git formats commit dates in local time but std has no
/// time zone access, so the UTC offset is borrowed from the newest commit; a DST change
/// since that commit can shift the boundary by an hour.
fn local_today(commit: &CommitSummary, now: i64) -> Option<i64> {
    let hours: i64 = commit.date.get(11..13)?.parse().ok()?;
    let minutes: i64 = commit.date.get(14..16)?.parse().ok()?;
    let local = civil_day(commit.day())? * 86_400 + hours * 3_600 + minutes * 60;
    let offset = local - (commit.timestamp - commit.timestamp.rem_euclid(60));
    Some((now + offset).div_euclid(86_400))
}

/// Formats a `YYYY-MM-DD` day as `Today`, `Yesterday`, `Tue, Sep 23`, or `Sep 23, 2025`.
pub(crate) fn day_label(day: &str, today: Option<i64>) -> String {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let Some(days) = civil_day(day) else {
        return day.into();
    };
    let (year, month, date) = civil_date(days);
    let weekday = WEEKDAYS[(days + 4).rem_euclid(7) as usize];
    let month = MONTHS[(month - 1) as usize];
    let Some(today) = today else {
        return format!("{weekday}, {month} {date}, {year}");
    };
    match today - days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ if civil_date(today).0 == year => format!("{weekday}, {month} {date}"),
        _ => format!("{month} {date}, {year}"),
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
    /// Local day number (days since 1970-01-01) for relative day labels.
    pub(crate) today: Option<i64>,
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
            today: None,
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
    let today = commits.first().and_then(|commit| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        local_today(commit, i64::try_from(now.as_secs()).ok()?)
    });
    Ok(HistoryPage {
        anchor: Some(anchor.into()),
        commits,
        more,
        today,
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

    #[test]
    fn conventional_prefixes_parse_kind_scope_and_breaking_marker() {
        let prefix = conventional_prefix("fix(ui)!: keep focus").unwrap();
        assert_eq!(prefix.kind, "fix");
        assert_eq!(prefix.scope, Some("ui"));
        assert!(prefix.breaking);
        assert_eq!(prefix.summary, "keep focus");
        let prefix = conventional_prefix("chore: bump version to 0.2.6").unwrap();
        assert_eq!((prefix.kind, prefix.scope), ("chore", None));
        assert!(!prefix.breaking);
        for subject in [
            "Merge branch 'main': sync",
            "Use std::io for reads",
            "fix:",
            "fix: ",
            "fix(): empty scope",
            "fix(ui: unclosed",
            "Revert \"fix: x\"",
            "plain subject",
        ] {
            assert_eq!(conventional_prefix(subject), None, "{subject}");
        }
    }

    #[test]
    fn day_labels_are_relative_near_today_and_include_year_when_needed() {
        let today = civil_day("2026-09-25");
        assert_eq!(day_label("2026-09-25", today), "Today");
        assert_eq!(day_label("2026-09-24", today), "Yesterday");
        assert_eq!(day_label("2026-09-23", today), "Wed, Sep 23");
        assert_eq!(day_label("2025-12-31", today), "Dec 31, 2025");
        assert_eq!(day_label("2024-02-29", None), "Thu, Feb 29, 2024");
        assert_eq!(day_label("not a day", today), "not a day");
        assert_eq!(civil_date(civil_day("2000-03-01").unwrap()), (2000, 3, 1));
    }

    #[test]
    fn local_today_borrows_the_newest_commit_offset() {
        let commit = CommitSummary {
            id: "a".repeat(40),
            parents: Vec::new(),
            author: "Example Developer".into(),
            // 2026-09-25 17:28 UTC, recorded locally as UTC+02:00.
            timestamp: 1_790_357_280,
            date: "2026-09-25 19:28".into(),
            subject: "Example commit".into(),
            body: String::new(),
            local: None,
            head: false,
            remote_tip: false,
        };
        assert_eq!(
            local_today(&commit, commit.timestamp),
            civil_day("2026-09-25")
        );
        assert_eq!(
            local_today(&commit, commit.timestamp + 5 * 3_600),
            civil_day("2026-09-26")
        );
    }
}
