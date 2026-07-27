//! Cursor-aware `@` file path completion for the composer.
//!
//! Workspace-relative search without `fd`: directory listing for path segments,
//! bounded fuzzy walk for free-text queries. Safe to run off the UI thread.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_RESULTS: usize = 12;
const MAX_VISITED: usize = 4_000;
const MAX_DEPTH: usize = 10;
const CANDIDATE_POOL: usize = 48;

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".cache",
    "__pycache__",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtToken {
    /// Byte range of `@…` from token start through the cursor.
    pub range: std::ops::Range<usize>,
    pub raw_query: String,
    pub is_quoted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatch {
    /// Insertion text including leading `@` and quotes when needed.
    pub value: String,
    /// Relative path shown in the menu (`src/lib.rs` or `src/`).
    pub path: String,
    pub is_directory: bool,
}

fn is_path_delimiter(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '"' | '\'' | '=')
}

/// Extract an active `@` path token from text before the cursor.
pub fn extract_at_token(text: &str, cursor: usize) -> Option<AtToken> {
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];

    if let Some(token) = extract_quoted_at_token(before) {
        return Some(token);
    }

    let token_start = before
        .char_indices()
        .rev()
        .find_map(|(index, ch)| is_path_delimiter(ch).then_some(index + ch.len_utf8()))
        .unwrap_or(0);
    if !before[token_start..].starts_with('@') {
        return None;
    }
    if token_start > 0
        && !before[..token_start]
            .chars()
            .next_back()
            .is_some_and(is_path_delimiter)
    {
        return None;
    }
    Some(AtToken {
        range: token_start..cursor,
        raw_query: before[token_start + 1..].to_owned(),
        is_quoted: false,
    })
}

fn extract_quoted_at_token(before: &str) -> Option<AtToken> {
    let mut in_quotes = false;
    let mut quote_start = None;
    for (index, ch) in before.char_indices() {
        if ch != '"' {
            continue;
        }
        if in_quotes {
            in_quotes = false;
            quote_start = None;
        } else {
            in_quotes = true;
            quote_start = Some(index);
        }
    }
    let quote_start = quote_start.filter(|_| in_quotes)?;
    let at_index = quote_start.checked_sub(1)?;
    if before.as_bytes().get(at_index) != Some(&b'@') {
        return None;
    }
    if at_index > 0
        && !before[..at_index]
            .chars()
            .next_back()
            .is_some_and(is_path_delimiter)
    {
        return None;
    }
    Some(AtToken {
        range: at_index..before.len(),
        raw_query: before[quote_start + 1..].to_owned(),
        is_quoted: true,
    })
}

/// Search workspace files for an `@` query. Safe to call off the UI thread.
pub fn search_files(workspace: &Path, query: &str, max_results: usize) -> Vec<FileMatch> {
    let max_results = max_results.clamp(1, MAX_RESULTS);
    let Ok(workspace) = workspace.canonicalize() else {
        return Vec::new();
    };
    if !workspace.is_dir() {
        return Vec::new();
    }

    let query = query.replace('\\', "/");
    if query.is_empty() || query.ends_with('/') {
        return list_dir(&workspace, &query, max_results);
    }

    if let Some((parent, name)) = query.rsplit_once('/') {
        let listed = list_dir(&workspace, &query, max_results);
        if !listed.is_empty() {
            return listed;
        }
        let root = if parent.is_empty() {
            workspace.clone()
        } else if let Some(dir) = join_under(&workspace, parent) {
            dir
        } else {
            return Vec::new();
        };
        return fuzzy_walk(&workspace, &root, name, max_results);
    }

    // Single segment: cwd prefix hits first (cheap), then a bounded deep walk.
    let mut matches = list_dir(&workspace, &query, max_results);
    if matches.len() >= max_results {
        return matches;
    }
    let deep = fuzzy_walk(&workspace, &workspace, &query, max_results);
    merge_unique(&mut matches, deep, max_results);
    matches
}

fn list_dir(workspace: &Path, query: &str, max_results: usize) -> Vec<FileMatch> {
    let (search_dir, display_base, name_prefix) = if query.is_empty() {
        (workspace.to_path_buf(), String::new(), String::new())
    } else if query.ends_with('/') {
        let rel = query.trim_end_matches('/');
        let Some(dir) = join_under(workspace, rel) else {
            return Vec::new();
        };
        if !dir.is_dir() {
            return Vec::new();
        }
        (dir, format!("{rel}/"), String::new())
    } else if let Some((parent, name)) = query.rsplit_once('/') {
        let dir = if parent.is_empty() {
            workspace.to_path_buf()
        } else if let Some(dir) = join_under(workspace, parent) {
            dir
        } else {
            return Vec::new();
        };
        if !dir.is_dir() {
            return Vec::new();
        }
        let base = if parent.is_empty() {
            String::new()
        } else {
            format!("{parent}/")
        };
        (dir, base, name.to_owned())
    } else {
        (workspace.to_path_buf(), String::new(), query.to_owned())
    };

    let Ok(entries) = fs::read_dir(&search_dir) else {
        return Vec::new();
    };
    let prefix = name_prefix.to_ascii_lowercase();
    let mut scored = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "." || name == ".." {
            continue;
        }
        if !prefix.is_empty() && !name.to_ascii_lowercase().starts_with(&prefix) {
            continue;
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if is_dir && skip_dir(&name) {
            continue;
        }
        let relative = format!("{display_base}{name}").replace('\\', "/");
        let score = if name_prefix.is_empty() {
            // Equal scores; compare_candidates orders dirs first, then path.
            1
        } else {
            score_entry(&relative, &prefix, is_dir)
        };
        if score > 0 {
            insert_candidate(&mut scored, score, make_match(&relative, is_dir));
        }
    }
    take_top(scored, max_results)
}

fn fuzzy_walk(
    workspace: &Path,
    search_root: &Path,
    query: &str,
    max_results: usize,
) -> Vec<FileMatch> {
    if query.is_empty() {
        return list_dir(workspace, "", max_results);
    }
    let query_lower = query.to_ascii_lowercase();
    let mut visited = 0usize;
    let mut scored = Vec::with_capacity(CANDIDATE_POOL);
    walk(
        workspace,
        search_root,
        0,
        &query_lower,
        &mut visited,
        &mut scored,
    );
    take_top(scored, max_results)
}

fn walk(
    workspace: &Path,
    dir: &Path,
    depth: usize,
    query_lower: &str,
    visited: &mut usize,
    out: &mut Vec<(i32, FileMatch)>,
) {
    if depth > MAX_DEPTH || *visited >= MAX_VISITED {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        if *visited >= MAX_VISITED {
            break;
        }
        *visited += 1;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "." || name == ".." {
            continue;
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if is_dir && skip_dir(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(workspace) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let score = score_entry(&relative, query_lower, is_dir);
        if score > 0 {
            insert_candidate(out, score, make_match(&relative, is_dir));
        }
        if is_dir {
            subdirs.push(path);
        }
    }

    // Pool already full of strong basename hits — further descent rarely helps ranking.
    if out.len() >= CANDIDATE_POOL && out.iter().all(|(score, _)| *score >= 80) {
        return;
    }

    for subdir in subdirs {
        if *visited >= MAX_VISITED {
            break;
        }
        walk(workspace, &subdir, depth + 1, query_lower, visited, out);
    }
}

fn join_under(workspace: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() {
        return Some(workspace.to_path_buf());
    }
    if relative.split('/').any(|part| part == "..") {
        return None;
    }
    let candidate = workspace.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    let canonical = candidate.canonicalize().ok()?;
    canonical.starts_with(workspace).then_some(canonical)
}

fn skip_dir(name: &str) -> bool {
    SKIP_DIRS.iter().any(|skip| name.eq_ignore_ascii_case(skip))
}

fn make_match(relative: &str, is_directory: bool) -> FileMatch {
    let path = if is_directory {
        format!("{}/", relative.trim_end_matches('/'))
    } else {
        relative.to_owned()
    };
    FileMatch {
        value: build_completion_value(&path),
        path,
        is_directory,
    }
}

fn insert_candidate(out: &mut Vec<(i32, FileMatch)>, score: i32, item: FileMatch) {
    if let Some((existing_score, existing)) = out.iter_mut().find(|(_, m)| m.path == item.path) {
        if score > *existing_score {
            *existing_score = score;
            *existing = item;
        }
        return;
    }
    if out.len() < CANDIDATE_POOL {
        out.push((score, item));
        return;
    }
    let Some(weak_index) = out
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| compare_candidates(a, b))
        .map(|(index, _)| index)
    else {
        return;
    };
    let candidate = (score, item);
    if compare_candidates(&candidate, &out[weak_index]) == Ordering::Greater {
        out[weak_index] = candidate;
    }
}

/// Higher score is better; directories beat files; shorter then alphabetical paths win ties.
fn compare_candidates(left: &(i32, FileMatch), right: &(i32, FileMatch)) -> Ordering {
    left.0
        .cmp(&right.0)
        .then_with(|| left.1.is_directory.cmp(&right.1.is_directory))
        .then_with(|| right.1.path.len().cmp(&left.1.path.len()))
        .then_with(|| right.1.path.cmp(&left.1.path))
}

fn take_top(mut scored: Vec<(i32, FileMatch)>, max_results: usize) -> Vec<FileMatch> {
    scored.sort_by(|left, right| compare_candidates(right, left));
    scored.truncate(max_results);
    scored.into_iter().map(|(_, item)| item).collect()
}

fn merge_unique(into: &mut Vec<FileMatch>, from: Vec<FileMatch>, max_results: usize) {
    for item in from {
        if into.iter().any(|existing| existing.path == item.path) {
            continue;
        }
        into.push(item);
        if into.len() >= max_results {
            break;
        }
    }
}

/// `query_lower` must already be lowercased for the free-text path.
fn score_entry(path: &str, query_lower: &str, is_directory: bool) -> i32 {
    if query_lower.is_empty() {
        return 1;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let name_lower = file_name.to_ascii_lowercase();
    let path_lower = path.to_ascii_lowercase();
    let mut score: i32 = if name_lower == query_lower {
        100
    } else if name_lower.starts_with(query_lower) {
        80
    } else if name_lower.contains(query_lower) {
        50
    } else if path_lower.contains(query_lower) {
        30
    } else if fuzzy_subsequence(&path_lower, query_lower) {
        15
    } else {
        0
    };
    if is_directory && score > 0 {
        score += 10;
    }
    // Prefer shorter paths among equal scores without a second sort key allocation.
    if score > 0 {
        score -= (path.len() / 40).min(20) as i32;
    }
    score
}

fn fuzzy_subsequence(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    for wanted in needle.chars() {
        if chars.find(|candidate| *candidate == wanted).is_none() {
            return false;
        }
    }
    true
}

pub fn build_completion_value(path: &str) -> String {
    if path.contains(' ') {
        format!("@\"{path}\"")
    } else {
        format!("@{path}")
    }
}

/// Apply a match over the extracted token range and return the new draft + cursor.
pub fn apply_file_match(text: &str, token: &AtToken, item: &FileMatch) -> (String, usize) {
    let mut value = item.value.clone();
    if token.is_quoted && !value.starts_with("@\"") {
        let inner = value.strip_prefix('@').unwrap_or(&value);
        value = format!("@\"{inner}");
        if !value.ends_with('"') {
            value.push('"');
        }
    }
    let suffix = if item.is_directory { "" } else { " " };
    let mut out = String::with_capacity(text.len() + value.len() + 1);
    out.push_str(&text[..token.range.start]);
    out.push_str(&value);
    out.push_str(suffix);
    out.push_str(&text[token.range.end..]);

    let mut cursor = token.range.start + value.len() + suffix.len();
    if item.is_directory && value.ends_with('"') {
        cursor = cursor.saturating_sub(1);
    }
    (out, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("pi-gui-file-completion-{name}-{stamp}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp workspace");
        root
    }

    #[test]
    fn extracts_simple_at_token_at_cursor() {
        let text = "see @src/vi";
        let token = extract_at_token(text, text.len()).expect("token");
        assert_eq!(token.raw_query, "src/vi");
        assert_eq!(&text[token.range.clone()], "@src/vi");
        assert!(!token.is_quoted);
    }

    #[test]
    fn extracts_mid_line_token_only_before_cursor() {
        let text = "look at @readme and more";
        let cursor = text.find(" and").expect("marker");
        let token = extract_at_token(text, cursor).expect("token");
        assert_eq!(token.raw_query, "readme");
        assert_eq!(&text[token.range.clone()], "@readme");
    }

    #[test]
    fn ignores_email_like_at_signs() {
        assert!(extract_at_token("user@host", 9).is_none());
    }

    #[test]
    fn extracts_quoted_at_token() {
        let text = r#"open @"src/My File"#;
        let token = extract_at_token(text, text.len()).expect("token");
        assert!(token.is_quoted);
        assert_eq!(token.raw_query, "src/My File");
    }

    #[test]
    fn lists_top_level_entries_for_empty_query() {
        let root = temp_workspace("empty");
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("README.md"), "hi").unwrap();
        let matches = search_files(&root, "", 20);
        let paths: Vec<_> = matches.iter().map(|item| item.path.as_str()).collect();
        assert!(paths.iter().any(|path| *path == "src/"));
        assert!(paths.iter().any(|path| *path == "README.md"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn filters_by_name_prefix_and_builds_at_value() {
        let root = temp_workspace("prefix");
        fs::write(root.join("alpha.rs"), "").unwrap();
        fs::write(root.join("beta.rs"), "").unwrap();
        let matches = search_files(&root, "al", 20);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].value, "@alpha.rs");
        assert_eq!(matches[0].path, "alpha.rs");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn finds_nested_files_by_substring() {
        let root = temp_workspace("nested");
        fs::create_dir_all(root.join("src").join("views")).unwrap();
        fs::write(root.join("src").join("views").join("root.rs"), "").unwrap();
        let matches = search_files(&root, "root", 20);
        assert!(
            matches.iter().any(|item| item.path.contains("root.rs")),
            "{matches:?}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn skips_git_and_target_directories() {
        let root = temp_workspace("skip");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("config"), "").unwrap();
        fs::create_dir_all(root.join("target").join("debug")).unwrap();
        fs::write(root.join("target").join("debug").join("secret.rs"), "").unwrap();
        fs::write(root.join("keep.rs"), "").unwrap();
        let matches = search_files(&root, "secret", 20);
        assert!(matches.is_empty(), "{matches:?}");
        let keep = search_files(&root, "keep", 20);
        assert_eq!(keep.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn apply_inserts_path_and_space_for_files() {
        let text = "see @al";
        let token = extract_at_token(text, text.len()).unwrap();
        let item = FileMatch {
            value: "@alpha.rs".into(),
            path: "alpha.rs".into(),
            is_directory: false,
        };
        let (next, cursor) = apply_file_match(text, &token, &item);
        assert_eq!(next, "see @alpha.rs ");
        assert_eq!(cursor, next.len());
    }

    #[test]
    fn apply_keeps_directory_open_for_further_typing() {
        let text = "see @s";
        let token = extract_at_token(text, text.len()).unwrap();
        let item = FileMatch {
            value: "@src/".into(),
            path: "src/".into(),
            is_directory: true,
        };
        let (next, cursor) = apply_file_match(text, &token, &item);
        assert_eq!(next, "see @src/");
        assert_eq!(cursor, next.len());
    }

    #[test]
    fn quotes_paths_with_spaces() {
        assert_eq!(build_completion_value("My File.rs"), "@\"My File.rs\"");
    }
}
