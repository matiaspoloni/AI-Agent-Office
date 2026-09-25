//! The Git service against real temporary repositories (needs `git` on PATH,
//! like the app itself).

use ao_git::{ChangeKind, Git, GitError};
use std::path::{Path, PathBuf};
use std::process::Command;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ao-git-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Git reports real paths (long Windows names, /private/tmp on
        // macOS). Windows' real path starts with `\\?\`, which Git cannot
        // use for every command: drop it.
        let real = dir.canonicalize().unwrap();
        let text = real.to_string_lossy().into_owned();
        Self(match text.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest),
            None => real,
        })
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs git for test setup, isolated from the machine's global settings.
fn git(dir: &Path, args: &[&str]) -> String {
    let empty = std::env::temp_dir().join("ao-git-empty-gitconfig");
    let _ = std::fs::write(&empty, "");
    let mut cmd = Command::new("git");
    cmd.args([
        "-c",
        "init.defaultBranch=main",
        "-c",
        "user.name=Test Agent",
        "-c",
        "user.email=agent@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.autocrlf=false",
    ])
    .arg("-C")
    .arg(dir)
    .args(args)
    .env("GIT_CONFIG_NOSYSTEM", "1")
    .env("GIT_CONFIG_GLOBAL", &empty);
    let out = cmd.output().expect("git is installed");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn repo(tmp: &TempDir, name: &str) -> PathBuf {
    let dir = tmp.path().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    dir
}

fn service() -> Git {
    Git::new("git")
}

fn shown(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text
    }
}

#[tokio::test]
async fn folders_outside_git_are_not_repositories() {
    let tmp = TempDir::new("plain");
    let git = service();
    assert_eq!(git.locate(tmp.path()).await.unwrap(), None);
    assert_eq!(git.locate(&tmp.path().join("missing")).await.unwrap(), None);
    assert_eq!(git.snapshot(tmp.path(), 5).await.unwrap(), None);
}

#[tokio::test]
async fn snapshot_reports_branch_changes_and_commits() {
    let tmp = TempDir::new("snapshot");
    let root = repo(&tmp, "app");
    write(&root, "a.txt", "one\n");
    write(&root, "b.txt", "two\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "First commit"]);
    write(&root, "a.txt", "one changed\n"); // unstaged
    write(&root, "src/c.rs", "fn main() {}\n");
    git(&root, &["add", "src/c.rs"]); // staged new file
    std::fs::create_dir_all(root.join("docs")).unwrap();
    git(&root, &["mv", "b.txt", "docs/b renamed.txt"]); // staged rename
    write(&root, "notes/ñandú.txt", "hola\n"); // untracked

    let git = service();
    // Asked from a subfolder: same working tree.
    std::fs::create_dir_all(root.join("src")).unwrap();
    let snap = git.snapshot(&root.join("src"), 5).await.unwrap().unwrap();
    assert_eq!(snap.location.worktree_root, shown(&root));
    assert_eq!(snap.location.repository_root, shown(&root));
    assert!(!snap.location.linked_worktree);
    let status = &snap.status;
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(status.head.is_some());
    assert_eq!(status.upstream, None);
    assert_eq!((status.ahead, status.behind), (None, None));
    assert_eq!(status.counts.staged, 2, "{status:#?}");
    assert_eq!(status.counts.unstaged, 1);
    assert_eq!(status.counts.untracked, 1);
    assert_eq!(status.counts.dirty(), 2);
    let file = |p: &str| status.files.iter().find(|f| f.path == p).cloned();
    assert_eq!(file("a.txt").unwrap().unstaged, Some(ChangeKind::Modified));
    assert_eq!(file("src/c.rs").unwrap().staged, Some(ChangeKind::Added));
    let renamed = file("docs/b renamed.txt").unwrap();
    assert_eq!(renamed.staged, Some(ChangeKind::Renamed));
    assert_eq!(renamed.orig_path.as_deref(), Some("b.txt"));
    // Untracked folders are listed as the folder (like `git status`).
    assert!(file("notes/").is_some(), "{:?}", status.files);

    assert_eq!(snap.recent_commits.len(), 1);
    let commit = &snap.recent_commits[0];
    assert_eq!(commit.summary, "First commit");
    assert_eq!(commit.author, "Test Agent");
    assert_eq!(Some(commit.sha.as_str()), status.head.as_deref());
    assert!(commit.time > 1_600_000_000_000);
    assert_eq!(snap.worktrees.len(), 1);
    assert_eq!(snap.worktrees[0].path, shown(&root));
}

#[tokio::test]
async fn a_repository_without_commits() {
    let tmp = TempDir::new("empty");
    let root = repo(&tmp, "new");
    write(&root, "README.md", "x");
    let snap = service().snapshot(&root, 5).await.unwrap().unwrap();
    assert_eq!(snap.status.head, None);
    assert_eq!(snap.status.branch.as_deref(), Some("main"));
    assert_eq!(snap.status.counts.untracked, 1);
    assert!(snap.recent_commits.is_empty());
    assert!(service().recent_commits(&root, 5).await.unwrap().is_empty());
}

#[tokio::test]
async fn ahead_and_behind_the_upstream() {
    let tmp = TempDir::new("upstream");
    let origin = tmp.path().join("origin.git");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    let seed = repo(&tmp, "seed");
    write(&seed, "f.txt", "1");
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "base"]);
    git(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);

    let mine = tmp.path().join("mine");
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            mine.to_str().unwrap(),
        ],
    );
    for i in 0..2 {
        write(&mine, &format!("m{i}.txt"), "x");
        git(&mine, &["add", "."]);
        git(&mine, &["commit", "-q", "-m", &format!("mine {i}")]);
    }
    write(&seed, "g.txt", "2");
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "theirs"]);
    git(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
    git(&mine, &["fetch", "-q"]);

    let status = service().status(&mine).await.unwrap();
    assert_eq!(status.upstream.as_deref(), Some("origin/main"));
    assert_eq!((status.ahead, status.behind), (Some(2), Some(1)));
}

#[tokio::test]
async fn linked_worktrees_share_their_repository() {
    let tmp = TempDir::new("worktree");
    let root = repo(&tmp, "main");
    write(&root, "f.txt", "1");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    let linked = tmp.path().join("feature tree");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            linked.to_str().unwrap(),
        ],
    );

    let git = service();
    let location = git.locate(&linked).await.unwrap().unwrap();
    assert!(location.linked_worktree);
    assert_eq!(location.worktree_root, shown(&linked));
    assert_eq!(location.repository_root, shown(&root));
    let main = git.locate(&root).await.unwrap().unwrap();
    assert!(!main.linked_worktree);

    let snap = git.snapshot(&linked, 5).await.unwrap().unwrap();
    assert_eq!(snap.status.branch.as_deref(), Some("feature"));
    assert_eq!(snap.worktrees.len(), 2);
    let feature = snap
        .worktrees
        .iter()
        .find(|w| w.path == shown(&linked))
        .unwrap();
    assert_eq!(feature.branch.as_deref(), Some("feature"));
}

#[tokio::test]
async fn detached_head_and_conflicts() {
    let tmp = TempDir::new("conflict");
    let root = repo(&tmp, "app");
    write(&root, "f.txt", "base\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    git(&root, &["checkout", "-q", "-b", "other"]);
    write(&root, "f.txt", "other\n");
    git(&root, &["commit", "-q", "-am", "other"]);
    git(&root, &["checkout", "-q", "main"]);
    write(&root, "f.txt", "main\n");
    git(&root, &["commit", "-q", "-am", "main"]);
    let merge = Command::new("git")
        .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
        .arg("-C")
        .arg(&root)
        .args(["merge", "-q", "other"])
        .output()
        .unwrap();
    assert!(!merge.status.success(), "the merge must conflict");

    let status = service().status(&root).await.unwrap();
    assert_eq!(status.counts.conflicted, 1);
    assert_eq!(status.files[0].unstaged, Some(ChangeKind::Conflicted));

    git(&root, &["merge", "--abort"]);
    git(&root, &["checkout", "-q", "--detach"]);
    let status = service().status(&root).await.unwrap();
    assert!(status.detached);
    assert_eq!(status.branch, None);
}

#[tokio::test]
async fn new_commits_between_two_heads() {
    let tmp = TempDir::new("between");
    let root = repo(&tmp, "app");
    write(&root, "f.txt", "1");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    let before = git(&root, &["rev-parse", "HEAD"]);
    for i in 0..3 {
        write(&root, "f.txt", &i.to_string());
        git(&root, &["commit", "-q", "-am", &format!("change {i}")]);
    }
    let after = git(&root, &["rev-parse", "HEAD"]);
    let git = service();
    let commits = git
        .commits_between(&root, &before, &after, 10)
        .await
        .unwrap();
    let summaries: Vec<&str> = commits.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(summaries, ["change 2", "change 1", "change 0"]);
    assert!(matches!(
        git.commits_between(&root, "--all", &after, 10).await,
        Err(GitError::Failed(_))
    ));
}

/// A repository can name a program in `core.fsmonitor` that `git status`
/// starts. Agent Office's reads turn it off.
#[tokio::test]
async fn repository_fsmonitor_programs_are_not_started() {
    let tmp = TempDir::new("fsmonitor");
    let root = repo(&tmp, "app");
    write(&root, "f.txt", "1");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    let marker = root.join("fsmonitor-ran");
    git(
        &root,
        &[
            "config",
            "core.fsmonitor",
            "echo ran > fsmonitor-ran; exit 1",
        ],
    );

    service().status(&root).await.unwrap();
    service().snapshot(&root, 5).await.unwrap();
    assert!(
        !marker.exists(),
        "Agent Office started the repository's fsmonitor program"
    );

    // Control: a plain `git status` does start it (where Git runs it through sh).
    if cfg!(unix) {
        git(&root, &["status", "--porcelain"]);
        assert!(
            marker.exists(),
            "control: plain git status should run the fsmonitor program"
        );
    }
}

/// `git status` normally rewrites the index to refresh file times, which can
/// collide with an agent's own `git add` / `git commit` (index.lock). Agent
/// Office's status leaves the index alone.
#[tokio::test]
async fn status_does_not_rewrite_the_index() {
    let tmp = TempDir::new("locks");
    let root = repo(&tmp, "app");
    write(&root, "f.txt", "same\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    // Same content, newer file time: the index entry is now stale.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    write(&root, "f.txt", "same\n");
    let index = root.join(".git").join("index");
    let before = std::fs::read(&index).unwrap();

    let status = service().status(&root).await.unwrap();
    assert_eq!(status.counts.dirty(), 0);
    assert_eq!(
        std::fs::read(&index).unwrap(),
        before,
        "the index was rewritten"
    );

    // Control: a plain `git status` refreshes (rewrites) it.
    git(&root, &["status", "--porcelain"]);
    assert_ne!(std::fs::read(&index).unwrap(), before);
}

#[tokio::test]
async fn a_missing_git_is_reported() {
    let tmp = TempDir::new("nogit");
    let git = Git::new(tmp.path().join("no-such-git"));
    assert!(matches!(
        git.locate(tmp.path()).await,
        Err(GitError::Unavailable(_))
    ));
}
