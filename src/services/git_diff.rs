//! Read-only Git workspace diff snapshots for the native conversation UI.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_GIT_METADATA_BYTES: usize = 2 * 1024 * 1024;
const MAX_PATCH_BYTES: usize = 768 * 1024;
const MAX_UNTRACKED_PATCH_FILE_BYTES: usize = 192 * 1024;
const MAX_UNTRACKED_COUNT_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFileKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
    Notice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_line: Option<u64>,
    pub new_line: Option<u64>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
    pub binary: bool,
    pub untracked: bool,
    pub kind: DiffFileKind,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDiff {
    pub files: Vec<DiffFile>,
    pub patch_truncated: bool,
    pub counts_partial: bool,
}

impl WorkspaceDiff {
    pub fn additions(&self) -> u64 {
        self.files
            .iter()
            .fold(0u64, |total, file| total.saturating_add(file.additions))
    }

    pub fn deletions(&self) -> u64 {
        self.files
            .iter()
            .fold(0u64, |total, file| total.saturating_add(file.deletions))
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitDiffError {
    GitUnavailable,
    NotRepository,
    InspectionFailed,
}

pub fn load_workspace_diff(workspace: &Path) -> Result<WorkspaceDiff, GitDiffError> {
    let root_output = run_git(
        workspace,
        &["rev-parse", "--show-toplevel"],
        MAX_GIT_METADATA_BYTES,
    )?;
    let root_text = String::from_utf8_lossy(&root_output.bytes);
    let root = PathBuf::from(root_text.trim());
    if root.as_os_str().is_empty() || !root.is_dir() {
        return Err(GitDiffError::NotRepository);
    }

    let has_head = run_git(
        &root,
        &["rev-parse", "--verify", "HEAD"],
        MAX_GIT_METADATA_BYTES,
    )
    .is_ok();
    let tracked_args = if has_head {
        vec!["diff", "--no-ext-diff", "--numstat", "-z", "HEAD", "--"]
    } else {
        vec!["diff", "--no-ext-diff", "--cached", "--numstat", "-z", "--"]
    };
    let tracked = run_git(&root, &tracked_args, MAX_GIT_METADATA_BYTES)?;
    let mut files = parse_numstat(&tracked.bytes);

    let patch_args = if has_head {
        vec![
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--unified=3",
            "HEAD",
            "--",
        ]
    } else {
        vec![
            "diff",
            "--no-ext-diff",
            "--cached",
            "--no-color",
            "--unified=3",
            "--",
        ]
    };
    let tracked_patch = run_git(&root, &patch_args, MAX_PATCH_BYTES)?;
    let mut patch = String::from_utf8_lossy(&tracked_patch.bytes).into_owned();
    let mut patch_truncated = tracked_patch.truncated;
    let mut counts_partial = tracked.truncated;

    let untracked = run_git(
        &root,
        &["ls-files", "--others", "--exclude-standard", "-z", "--"],
        MAX_GIT_METADATA_BYTES,
    )?;
    counts_partial |= untracked.truncated;
    for raw_path in untracked.bytes.split(|byte| *byte == 0) {
        if raw_path.is_empty() {
            continue;
        }
        let path = String::from_utf8_lossy(raw_path).into_owned();
        let display_path = sanitize_path(&path);
        let absolute = root.join(&path);
        let (additions, binary, complete) = count_untracked_file(&absolute);
        counts_partial |= !complete;
        files.push(DiffFile {
            path: display_path.clone(),
            additions,
            deletions: 0,
            binary,
            untracked: true,
            kind: DiffFileKind::Untracked,
            hunks: Vec::new(),
        });

        if patch.len() >= MAX_PATCH_BYTES {
            patch_truncated = true;
            continue;
        }
        match untracked_patch(&absolute, &display_path, MAX_UNTRACKED_PATCH_FILE_BYTES) {
            Some((file_patch, truncated)) => {
                append_patch(
                    &mut patch,
                    &file_patch,
                    MAX_PATCH_BYTES,
                    &mut patch_truncated,
                );
                patch_truncated |= truncated;
            }
            None if binary => {
                let binary_notice = format!(
                    "diff --git a/{display_path} b/{display_path}\nnew file mode 100644\nBinary file /dev/null and b/{display_path} differ\n"
                );
                append_patch(
                    &mut patch,
                    &binary_notice,
                    MAX_PATCH_BYTES,
                    &mut patch_truncated,
                );
            }
            None => patch_truncated = true,
        }
    }

    let mut merged = BTreeMap::<String, DiffFile>::new();
    for file in files {
        merged
            .entry(file.path.clone())
            .and_modify(|current| {
                current.additions = current.additions.saturating_add(file.additions);
                current.deletions = current.deletions.saturating_add(file.deletions);
                current.binary |= file.binary;
                current.untracked &= file.untracked;
                if current.untracked {
                    current.kind = DiffFileKind::Untracked;
                }
            })
            .or_insert(file);
    }

    for parsed in parse_patch(&sanitize_text(&patch)) {
        let Some(file) = merged.get_mut(&parsed.path) else {
            continue;
        };
        if !file.untracked {
            file.kind = parsed.kind;
        }
        file.binary |= parsed.binary;
        file.hunks.extend(parsed.hunks);
    }

    Ok(WorkspaceDiff {
        files: merged.into_values().collect(),
        patch_truncated,
        counts_partial,
    })
}

struct GitOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn run_git(cwd: &Path, args: &[&str], limit: usize) -> Result<GitOutput, GitDiffError> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| GitDiffError::GitUnavailable)?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(GitDiffError::InspectionFailed);
    };
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    if stdout
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(GitDiffError::InspectionFailed);
    }
    let truncated = bytes.len() > limit;
    if truncated {
        bytes.truncate(limit);
        let _ = child.kill();
    }
    let status = child.wait().map_err(|_| GitDiffError::InspectionFailed)?;
    if !status.success() && !truncated {
        return Err(if args.starts_with(&["rev-parse", "--show-toplevel"]) {
            GitDiffError::NotRepository
        } else {
            GitDiffError::InspectionFailed
        });
    }
    Ok(GitOutput { bytes, truncated })
}

fn parse_numstat(bytes: &[u8]) -> Vec<DiffFile> {
    let mut fields = bytes.split(|byte| *byte == 0);
    let mut files = Vec::new();
    while let Some(record) = fields.next() {
        if record.is_empty() {
            continue;
        }
        let mut columns = record.splitn(3, |byte| *byte == b'\t');
        let Some(additions) = columns.next() else {
            continue;
        };
        let Some(deletions) = columns.next() else {
            continue;
        };
        let Some(path) = columns.next() else {
            continue;
        };
        let binary = additions == b"-" || deletions == b"-";
        let additions = parse_count(additions);
        let deletions = parse_count(deletions);
        let path = if path.is_empty() {
            let Some(old_path) = fields.next() else {
                break;
            };
            let Some(new_path) = fields.next() else {
                break;
            };
            format!(
                "{} → {}",
                sanitize_path(&String::from_utf8_lossy(old_path)),
                sanitize_path(&String::from_utf8_lossy(new_path))
            )
        } else {
            sanitize_path(&String::from_utf8_lossy(path))
        };
        files.push(DiffFile {
            path,
            additions,
            deletions,
            binary,
            untracked: false,
            kind: DiffFileKind::Modified,
            hunks: Vec::new(),
        });
    }
    files
}

struct ParsedPatchFile {
    path: String,
    kind: DiffFileKind,
    binary: bool,
    hunks: Vec<DiffHunk>,
}

fn parse_patch(patch: &str) -> Vec<ParsedPatchFile> {
    let mut sections = Vec::<Vec<&str>>::new();
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            sections.push(Vec::new());
        }
        if let Some(section) = sections.last_mut() {
            section.push(line);
        }
    }

    sections
        .into_iter()
        .filter_map(|section| parse_patch_section(&section))
        .collect()
}

fn parse_patch_section(lines: &[&str]) -> Option<ParsedPatchFile> {
    let mut old_path = None;
    let mut new_path = None;
    let mut saw_old_header = false;
    let mut saw_new_header = false;
    let mut rename_from = None;
    let mut rename_to = None;
    let mut kind = DiffFileKind::Modified;
    let mut binary = false;

    for line in lines {
        if let Some(path) = line.strip_prefix("--- ") {
            saw_old_header = true;
            old_path = parse_prefixed_patch_path(path);
        } else if let Some(path) = line.strip_prefix("+++ ") {
            saw_new_header = true;
            new_path = parse_prefixed_patch_path(path);
        } else if let Some(path) = line.strip_prefix("rename from ") {
            rename_from = Some(decode_git_path(path));
            kind = DiffFileKind::Renamed;
        } else if let Some(path) = line.strip_prefix("rename to ") {
            rename_to = Some(decode_git_path(path));
            kind = DiffFileKind::Renamed;
        } else if line.starts_with("new file mode ") {
            kind = DiffFileKind::Added;
        } else if line.starts_with("deleted file mode ") {
            kind = DiffFileKind::Deleted;
        } else if line.starts_with("Binary files ") || line == &"GIT binary patch" {
            binary = true;
        }
    }

    let fallback_paths = lines
        .first()
        .and_then(|line| line.strip_prefix("diff --git "))
        .and_then(parse_diff_header_paths);
    let old_path = rename_from.or(old_path).or_else(|| {
        (!saw_old_header)
            .then(|| fallback_paths.as_ref().map(|paths| paths.0.clone()))
            .flatten()
    });
    let new_path = rename_to.or(new_path).or_else(|| {
        (!saw_new_header)
            .then(|| fallback_paths.as_ref().map(|paths| paths.1.clone()))
            .flatten()
    });
    let path = match (old_path.as_deref(), new_path.as_deref()) {
        (Some(old), Some(new)) if old != new => format!("{old} → {new}"),
        (_, Some(new)) => new.to_owned(),
        (Some(old), None) => old.to_owned(),
        (None, None) => return None,
    };

    Some(ParsedPatchFile {
        path: sanitize_path(&path),
        kind,
        binary,
        hunks: parse_hunks(lines),
    })
}

fn parse_hunks(lines: &[&str]) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;
    let mut old_line = 0u64;
    let mut new_line = 0u64;

    for line in lines {
        if line.starts_with("@@") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            let (old_start, new_start) = hunk_starts(line).unwrap_or((0, 0));
            old_line = old_start;
            new_line = new_start;
            current = Some(DiffHunk {
                header: (*line).to_owned(),
                lines: Vec::new(),
            });
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            continue;
        };
        let (kind, previous, current_line, text) = if let Some(text) = line.strip_prefix('+') {
            let line_number = new_line;
            new_line = new_line.saturating_add(1);
            (DiffLineKind::Addition, None, Some(line_number), text)
        } else if let Some(text) = line.strip_prefix('-') {
            let line_number = old_line;
            old_line = old_line.saturating_add(1);
            (DiffLineKind::Deletion, Some(line_number), None, text)
        } else if let Some(text) = line.strip_prefix(' ') {
            let previous = old_line;
            let current_line = new_line;
            old_line = old_line.saturating_add(1);
            new_line = new_line.saturating_add(1);
            (
                DiffLineKind::Context,
                Some(previous),
                Some(current_line),
                text,
            )
        } else {
            (DiffLineKind::Notice, None, None, *line)
        };
        hunk.lines.push(DiffLine {
            kind,
            old_line: previous,
            new_line: current_line,
            text: text.to_owned(),
        });
    }

    if let Some(hunk) = current {
        hunks.push(hunk);
    }
    hunks
}

fn hunk_starts(header: &str) -> Option<(u64, u64)> {
    let mut fields = header.split_whitespace();
    (fields.next()? == "@@").then_some(())?;
    let old = fields
        .next()?
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    let new = fields
        .next()?
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    Some((old, new))
}

fn parse_prefixed_patch_path(value: &str) -> Option<String> {
    if value == "/dev/null" {
        return None;
    }
    let decoded = decode_git_path(value);
    Some(
        decoded
            .strip_prefix("a/")
            .or_else(|| decoded.strip_prefix("b/"))
            .unwrap_or(&decoded)
            .to_owned(),
    )
}

fn parse_diff_header_paths(value: &str) -> Option<(String, String)> {
    if value.starts_with('"') {
        let (old, rest) = take_quoted_git_path(value)?;
        let (new, _) = take_quoted_git_path(rest.trim_start())?;
        return Some((strip_diff_prefix(old), strip_diff_prefix(new)));
    }

    let split = value.rfind(" b/")?;
    let old = value[..split].trim();
    let new = value[split + 1..].trim();
    Some((
        strip_diff_prefix(decode_git_path(old)),
        strip_diff_prefix(decode_git_path(new)),
    ))
}

fn strip_diff_prefix(path: String) -> String {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(&path)
        .to_owned()
}

fn take_quoted_git_path(value: &str) -> Option<(String, &str)> {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return Some((decode_git_path(&value[..=index]), &value[index + 1..]));
        }
    }
    None
}

fn decode_git_path(value: &str) -> String {
    let value = value.trim();
    let Some(quoted) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return value.to_owned();
    };

    let bytes = quoted.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        index += 1;
        let Some(escaped) = bytes.get(index).copied() else {
            break;
        };
        match escaped {
            b'n' => decoded.push(b'\n'),
            b'r' => decoded.push(b'\r'),
            b't' => decoded.push(b'\t'),
            b'"' | b'\\' => decoded.push(escaped),
            b'0'..=b'7' => {
                let mut value = 0u16;
                let mut digits = 0;
                while digits < 3 && index < bytes.len() && matches!(bytes[index], b'0'..=b'7') {
                    value = value * 8 + u16::from(bytes[index] - b'0');
                    index += 1;
                    digits += 1;
                }
                decoded.push(value.min(255) as u8);
                continue;
            }
            other => decoded.push(other),
        }
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn parse_count(value: &[u8]) -> u64 {
    std::str::from_utf8(value)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn count_untracked_file(path: &Path) -> (u64, bool, bool) {
    let Ok(file) = File::open(path) else {
        return (0, false, false);
    };
    let Ok(metadata) = file.metadata() else {
        return (0, false, false);
    };
    if !metadata.is_file() {
        return (0, false, true);
    }
    if metadata.len() > MAX_UNTRACKED_COUNT_BYTES {
        return (0, false, false);
    }

    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut lines = 0u64;
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => return (lines, false, true),
            Ok(_) if line.contains(&0) => return (0, true, true),
            Ok(_) => lines = lines.saturating_add(1),
            Err(_) => return (0, false, false),
        }
    }
}

fn untracked_patch(path: &Path, display_path: &str, limit: usize) -> Option<(String, bool)> {
    let mut file = File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(limit.min(32 * 1024));
    file.by_ref()
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let truncated = bytes.len() > limit;
    if truncated {
        bytes.truncate(limit);
        while std::str::from_utf8(&bytes).is_err() && !bytes.is_empty() {
            bytes.pop();
        }
    }
    let content = std::str::from_utf8(&bytes).ok()?;
    let line_count = content.lines().count();
    let mut patch = format!(
        "diff --git a/{display_path} b/{display_path}\nnew file mode 100644\n--- /dev/null\n+++ b/{display_path}\n@@ -0,0 +1,{line_count} @@\n"
    );
    for line in content.lines() {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    Some((patch, truncated))
}

fn append_patch(target: &mut String, addition: &str, limit: usize, truncated: &mut bool) {
    if !target.is_empty() && !target.ends_with('\n') {
        target.push('\n');
    }
    let available = limit.saturating_sub(target.len());
    if addition.len() <= available {
        target.push_str(addition);
        return;
    }
    let mut end = available.min(addition.len());
    while !addition.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    target.push_str(&addition[..end]);
    *truncated = true;
}

fn sanitize_path(path: &str) -> String {
    path.chars()
        .map(|character| {
            if character.is_control() {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

fn sanitize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .map(|character| match character {
            '\n' | '\t' => character,
            character if character.is_control() => '\u{fffd}',
            character => character,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_parses_text_binary_and_renames() {
        let files = parse_numstat(
            b"12\t3\tsrc/main.rs\x00-\t-\tassets/logo.png\x000\t1\t\x00old name.rs\x00new name.rs\x00",
        );
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!((files[0].additions, files[0].deletions), (12, 3));
        assert!(files[1].binary);
        assert_eq!(files[2].path, "old name.rs → new name.rs");
    }

    #[test]
    fn patch_append_respects_utf8_boundary_and_limit() {
        let mut patch = "base\n".to_owned();
        let mut truncated = false;
        append_patch(&mut patch, "😀😀", 10, &mut truncated);
        assert!(patch.is_char_boundary(patch.len()));
        assert!(truncated);
        assert_eq!(patch, "base\n😀");
    }

    #[test]
    fn patch_sanitization_normalizes_line_endings_and_controls() {
        assert_eq!(sanitize_text("one\r\ntwo\rthree\u{1b}"), "one\ntwo\nthree�");
    }

    #[test]
    fn untracked_text_is_counted_and_rendered_without_mutating_git() {
        let path = std::env::temp_dir().join(format!(
            "pi-gui-diff-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "first\nsecond\n").unwrap();

        assert_eq!(count_untracked_file(&path), (2, false, true));
        let (patch, truncated) = untracked_patch(&path, "notes.txt", 1_024).unwrap();
        assert!(!truncated);
        assert!(patch.contains("+++ b/notes.txt"));
        assert!(patch.contains("+first\n+second"));

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn patch_is_split_into_file_hunks_with_line_numbers() {
        let files = parse_patch(concat!(
            "diff --git a/src/main.rs b/src/main.rs\n",
            "index 123..456 100644\n",
            "--- a/src/main.rs\n",
            "+++ b/src/main.rs\n",
            "@@ -10,3 +10,4 @@ fn main() {\n",
            " before();\n",
            "-old();\n",
            "+new();\n",
            "+after();\n",
            " done();\n",
        ));

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].hunks.len(), 1);
        let lines = &files[0].hunks[0].lines;
        assert_eq!((lines[0].old_line, lines[0].new_line), (Some(10), Some(10)));
        assert_eq!((lines[1].old_line, lines[1].new_line), (Some(11), None));
        assert_eq!((lines[2].old_line, lines[2].new_line), (None, Some(11)));
        assert_eq!((lines[4].old_line, lines[4].new_line), (Some(12), Some(13)));
    }

    #[test]
    fn patch_detects_added_deleted_renamed_and_binary_files() {
        let files = parse_patch(
            "diff --git a/new.txt b/new.txt\n\
             new file mode 100644\n\
             --- /dev/null\n\
             +++ b/new.txt\n\
             @@ -0,0 +1 @@\n\
             +new\n\
             diff --git a/old.txt b/gone.txt\n\
             deleted file mode 100644\n\
             --- a/old.txt\n\
             +++ /dev/null\n\
             @@ -1 +0,0 @@\n\
             -old\n\
             diff --git a/before.rs b/after.rs\n\
             similarity index 100%\n\
             rename from before.rs\n\
             rename to after.rs\n\
             diff --git a/image.png b/image.png\n\
             Binary files a/image.png and b/image.png differ\n",
        );

        assert_eq!(files[0].kind, DiffFileKind::Added);
        assert_eq!(files[1].path, "old.txt");
        assert_eq!(files[1].kind, DiffFileKind::Deleted);
        assert_eq!(files[2].path, "before.rs → after.rs");
        assert_eq!(files[2].kind, DiffFileKind::Renamed);
        assert!(files[3].binary);
    }

    #[test]
    fn quoted_git_paths_are_decoded() {
        let files = parse_patch(
            "diff --git \"a/docs/caf\\303\\251 note.txt\" \"b/docs/caf\\303\\251 note.txt\"\n\
             --- \"a/docs/caf\\303\\251 note.txt\"\n\
             +++ \"b/docs/caf\\303\\251 note.txt\"\n\
             @@ -1 +1 @@\n\
             -old\n\
             +new\n",
        );
        assert_eq!(files[0].path, "docs/café note.txt");
    }

    #[test]
    fn workspace_totals_ignore_binary_line_counts() {
        let snapshot = WorkspaceDiff {
            files: vec![
                DiffFile {
                    path: "a.rs".into(),
                    additions: 8,
                    deletions: 2,
                    binary: false,
                    untracked: false,
                    kind: DiffFileKind::Modified,
                    hunks: Vec::new(),
                },
                DiffFile {
                    path: "image.png".into(),
                    additions: 0,
                    deletions: 0,
                    binary: true,
                    untracked: true,
                    kind: DiffFileKind::Untracked,
                    hunks: Vec::new(),
                },
            ],
            patch_truncated: false,
            counts_partial: false,
        };
        assert_eq!(snapshot.additions(), 8);
        assert_eq!(snapshot.deletions(), 2);
        assert!(!snapshot.is_empty());
    }
}
