use super::*;
use std::fs;
use std::sync::atomic::AtomicU64;

static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct Repo(PathBuf);
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Repo {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pideck-workflow-{}-{stamp}-{}",
            std::process::id(),
            FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let repo = Self(path);
        repo.git(&["init", "-b", "main"]);
        repo.configure();
        repo
    }
    fn configure(&self) {
        self.git(&["config", "user.name", "Example Developer"]);
        self.git(&["config", "user.email", "example@example.invalid"]);
        self.git(&["config", "commit.gpgsign", "false"]);
        self.git(&["config", "core.autocrlf", "false"]);
        self.git(&["config", "core.hooksPath", ".git/empty-hooks"]);
    }
    fn git(&self, values: &[&str]) -> String {
        let output = run_git_with_input(&self.0, &args(values), None, WRITE_TIMEOUT).unwrap();
        assert!(
            output.success,
            "Synthetic Git command failed: {:?}: {}",
            values,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.bytes)
            .unwrap()
            .trim_end_matches(['\r', '\n'])
            .into()
    }
    fn put(&self, name: &str, contents: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    fn entry(&self, name: &str) -> GitEntry {
        read_status(&self.0)
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.relative_path == Path::new(name))
            .unwrap()
    }
    fn save(&self, message: &str) -> String {
        self.git(&["add", "--all", "--", "."]);
        commit(&self.0, &repository(&self.0).unwrap(), message).unwrap()
    }
}

#[test]
fn literal_staging_and_unborn_unstage_preserve_working_files_and_new_line_counts() {
    let repo = Repo::new();
    repo.put("a[1].txt", "first\nsecond\n");
    repo.put("a1.txt", "other\n");
    let entry = repo.entry("a[1].txt");
    assert_eq!(
        entry.working_stats,
        Some(GitLineStats {
            additions: 2,
            deletions: 0
        })
    );
    stage(&repo.0, &[entry], true).unwrap();
    assert_eq!(repo.git(&["diff", "--cached", "--name-only"]), "a[1].txt");
    stage(&repo.0, &[repo.entry("a[1].txt")], false).unwrap();
    assert!(repo.git(&["diff", "--cached", "--name-only"]).is_empty());
    assert_eq!(
        fs::read_to_string(repo.0.join("a[1].txt")).unwrap(),
        "first\nsecond\n"
    );
    assert_eq!(
        fs::read_to_string(repo.0.join("a1.txt")).unwrap(),
        "other\n"
    );
    repo.put("empty.txt", "");
    assert_eq!(
        repo.entry("empty.txt").working_stats,
        Some(GitLineStats::default())
    );
}

#[test]
fn discard_checks_the_preview_snapshot_and_preserves_the_staged_version() {
    let repo = Repo::new();
    repo.put("file.txt", "base\nsecond\n");
    repo.save("Initial");
    repo.put("file.txt", "staged\nsecond\n");
    stage(&repo.0, &[repo.entry("file.txt")], true).unwrap();
    repo.put("file.txt", "staged\nworking\n");
    let old = prepare_discard(&repo.0, &repo.entry("file.txt"), None).unwrap();
    repo.put("file.txt", "newer\nworking\n");
    assert!(discard(&old).is_err());
    assert_eq!(
        fs::read_to_string(repo.0.join("file.txt")).unwrap(),
        "newer\nworking\n"
    );
    let plan = prepare_discard(&repo.0, &repo.entry("file.txt"), None).unwrap();
    discard(&plan).unwrap();
    assert_eq!(
        fs::read_to_string(repo.0.join("file.txt")).unwrap(),
        "staged\nsecond\n"
    );
    let status = repo.entry("file.txt");
    assert!(status.staged());
    assert!(!status.unstaged());
    commit(&repo.0, &repository(&repo.0).unwrap(), "Only staged").unwrap();
    assert_eq!(repo.git(&["show", "HEAD:file.txt"]), "staged\nsecond");
}

#[test]
fn discard_one_hunk_leaves_other_hunks_and_the_index_untouched() {
    let repo = Repo::new();
    let mut lines: Vec<_> = (0..30).map(|i| format!("line {i}")).collect();
    repo.put("file.txt", &(lines.join("\n") + "\n"));
    repo.save("Initial");
    lines[2] = "first edit".into();
    lines[25] = "second edit".into();
    repo.put("file.txt", &(lines.join("\n") + "\n"));
    let entry = repo.entry("file.txt");
    let diff = file_diff(&repo.0, &entry.path, DiffKind::WorkingTree).unwrap();
    assert_eq!(
        diff.text
            .lines()
            .filter(|line| line.starts_with("@@ "))
            .count(),
        2
    );
    let before = index_snapshot(&repo.0).unwrap();
    let plan = prepare_discard(&repo.0, &entry, Some((0, diff.text))).unwrap();
    discard(&plan).unwrap();
    lines[2] = "line 2".into();
    assert_eq!(
        fs::read_to_string(repo.0.join("file.txt")).unwrap(),
        lines.join("\n") + "\n"
    );
    assert_eq!(index_snapshot(&repo.0).unwrap(), before);
    assert!(discard(&plan).is_err());
}

#[test]
fn committing_rejects_stale_staging_and_staged_files_outside_a_nested_project() {
    let repo = Repo::new();
    repo.put("nested/file.txt", "base\n");
    repo.put("outside.txt", "base\n");
    repo.save("Initial");
    repo.put("nested/file.txt", "change\n");
    stage(&repo.0, &[repo.entry("nested/file.txt")], true).unwrap();
    let expected = repository(&repo.0).unwrap();
    repo.put("outside.txt", "outside\n");
    stage(&repo.0, &[repo.entry("outside.txt")], true).unwrap();
    assert!(commit(&repo.0, &expected, "Stale").is_err());
    let current = repository(&repo.0).unwrap();
    assert!(commit(&repo.0.join("nested"), &current, "Outside scope").is_err());
    assert_eq!(repository(&repo.0).unwrap().head, current.head);
    assert_eq!(index_snapshot(&repo.0).unwrap(), current.index);
    assert!(stage(&repo.0.join("nested"), &[repo.entry("outside.txt")], true).is_err());
}

#[test]
fn history_covers_root_commits_multiline_messages_renames_binary_files_and_pagination() {
    let repo = Repo::new();
    repo.put("a.txt", "first\n");
    let first = repo.save("Initial\n\nA body\nwith two lines.");
    repo.git(&["mv", "a.txt", "renamed.txt"]);
    fs::write(repo.0.join("binary.dat"), [0, 1, 2, 3]).unwrap();
    let renamed = repo.save("Rename and add binary");
    let snapshot = repository(&repo.0).unwrap();
    let page = history_page(&repo.0, &snapshot, None, 0).unwrap();
    assert_eq!(page.commits[0].id, renamed);
    assert_eq!(page.commits[1].id, first);
    assert!(page.commits[1].body.contains("with two lines."));
    let root = commit_details(&repo.0, &page.commits[1], 0).unwrap();
    assert!(root.parent.is_none());
    assert_eq!(root.files.len(), 1);
    assert!(
        commit_file_diff(&repo.0, &root, 0)
            .unwrap()
            .text
            .contains("+first")
    );
    let details = commit_details(&repo.0, &page.commits[0], 0).unwrap();
    assert!(
        details
            .files
            .iter()
            .any(|file| file.relative_path == Path::new("binary.dat") && file.stats.is_none())
    );
    assert!(
        details
            .files
            .iter()
            .any(|file| file.relative_path == Path::new("a.txt"))
    );
    assert!(
        details
            .files
            .iter()
            .any(|file| file.relative_path == Path::new("renamed.txt"))
    );
    for i in 0..HISTORY_PAGE_SIZE {
        repo.git(&["commit", "--allow-empty", "-m", &format!("Commit {i}")]);
    }
    let snapshot = repository(&repo.0).unwrap();
    let first_page = history_page(&repo.0, &snapshot, None, 0).unwrap();
    let second = history_page(
        &repo.0,
        &snapshot,
        first_page.anchor.as_deref(),
        first_page.commits.len(),
    )
    .unwrap();
    assert!(first_page.more);
    assert!(!second.more);
    assert!(
        first_page
            .commits
            .iter()
            .all(|a| second.commits.iter().all(|b| a.id != b.id))
    );
    assert_eq!(
        first_page.commits.len() + second.commits.len(),
        HISTORY_PAGE_SIZE + 2
    );
}

#[test]
fn merge_history_can_compare_each_parent() {
    let repo = Repo::new();
    repo.put("base.txt", "base\n");
    repo.save("Initial");
    repo.git(&["switch", "-c", "topic"]);
    repo.put("topic.txt", "topic\n");
    repo.save("Topic");
    repo.git(&["switch", "main"]);
    repo.put("main.txt", "main\n");
    repo.save("Main");
    repo.git(&["merge", "--no-ff", "topic", "-m", "Merge topic"]);
    let snapshot = repository(&repo.0).unwrap();
    let summary = history_page(&repo.0, &snapshot, None, 0)
        .unwrap()
        .commits
        .remove(0);
    assert_eq!(summary.parents.len(), 2);
    let first = commit_details(&repo.0, &summary, 0).unwrap();
    let second = commit_details(&repo.0, &summary, 1).unwrap();
    assert!(
        first
            .files
            .iter()
            .any(|file| file.relative_path == Path::new("topic.txt"))
    );
    assert!(
        second
            .files
            .iter()
            .any(|file| file.relative_path == Path::new("main.txt"))
    );
}

#[test]
fn publish_push_fetch_and_rejected_push_use_only_a_local_bare_remote() {
    let repo = Repo::new();
    repo.put("file.txt", "base\n");
    let initial = repo.save("Initial");
    let bare = repo.0.join("remote.git");
    repo.git(&["init", "--bare", bare.to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    fs::write(repo.0.join(".gitignore"), "remote.git/\nother/\n").unwrap();
    let plan = push_plan(&repository(&repo.0).unwrap(), Some("origin")).unwrap();
    push(&plan).unwrap();
    let published = repository(&repo.0).unwrap();
    assert_eq!(published.upstream.as_ref().unwrap().label, "origin/main");
    assert_eq!(published.ahead, Some(0));
    let output = read(&bare, &args(&["rev-parse", "refs/heads/main"])).unwrap();
    assert_eq!(String::from_utf8(output).unwrap().trim(), initial);
    let page = history_page(&repo.0, &published, None, 0).unwrap();
    assert!(page.commits[0].remote_tip);
    assert_eq!(page.commits[0].local, Some(false));
    repo.put("file.txt", "local\n");
    repo.save("Local");
    let stale = push_plan(&repository(&repo.0).unwrap(), None).unwrap();
    repo.put("another.txt", "later\n");
    repo.save("Later");
    assert!(push(&stale).is_err());
    let other_path = repo.0.join("other");
    repo.git(&[
        "clone",
        "-b",
        "main",
        bare.to_str().unwrap(),
        other_path.to_str().unwrap(),
    ]);
    let other = Repo(other_path);
    other.configure();
    other.put("remote.txt", "remote\n");
    let remote_head = other.save("Remote");
    other.git(&["push", "origin", "main"]);
    fetch(&repository(&repo.0).unwrap(), "origin").unwrap();
    let diverged = repository(&repo.0).unwrap();
    assert_eq!(diverged.behind, Some(1));
    assert!(diverged.ahead.unwrap() > 0);
    assert!(push(&push_plan(&diverged, None).unwrap()).is_err());
    assert_eq!(repository(&repo.0).unwrap().head, diverged.head);
    assert_eq!(
        text(&bare, &args(&["rev-parse", "refs/heads/main"])).unwrap(),
        remote_head
    );
}
