//! Git service: reads the state of the repositories agents work in by running
//! the user's `git` (Git for Windows' `git.exe`) with machine-readable output.
//!
//! Only read commands are ever run (`rev-parse`, `status`, `log`,
//! `worktree list`), and they are run so that they cannot get in the agents'
//! way or start other programs of the repository's configuration:
//!
//! * `--no-optional-locks`: `git status` does not refresh the index, so it
//!   never holds `index.lock` while an agent runs `git add` or `git commit`.
//! * `core.fsmonitor` is turned off for our calls (a repository can name a
//!   program there that `git status` would start).
//! * No pager, no colors, no credential prompts, no console window, a
//!   timeout, and none of the caller's `GIT_*` variables (which could point
//!   Git at another repository).
//! * Arguments never contain text from agents: only fixed options, folder
//!   paths (through `-C`) and commit ids checked to be hexadecimal.
//!
//! Git's own `safe.directory` check stays on: a repository owned by another
//! Windows user is reported as refused, never forced open.

mod model;
pub mod parse;

pub use model::*;

use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use ts_rs::TS;

/// Oldest Git with everything used here (`--no-optional-locks`: 2.15).
pub const MIN_VERSION: (u32, u32) = (2, 15);

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// Why Git could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// `git` could not be started.
    Unavailable(String),
    /// Git refused the repository (owned by another user: `safe.directory`).
    Refused(String),
    TimedOut,
    Failed(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Unavailable(e) => write!(f, "Git could not be started: {e}"),
            GitError::Refused(e) => write!(f, "Git refused this repository: {e}"),
            GitError::TimedOut => write!(f, "Git did not answer in time"),
            GitError::Failed(e) => write!(f, "Git failed: {e}"),
        }
    }
}

impl std::error::Error for GitError {}

/// Everything shown for one working tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepoSnapshot {
    pub location: RepoLocation,
    pub status: WorktreeStatus,
    /// Latest commits of the checked-out branch, newest first.
    pub recent_commits: Vec<CommitInfo>,
    /// All worktrees of the repository (just the main one for most).
    pub worktrees: Vec<WorktreeInfo>,
}

/// Runs read-only Git commands. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Git {
    exe: PathBuf,
    timeout: Duration,
}

impl Git {
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        Self {
            exe: exe.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn executable(&self) -> &Path {
        &self.exe
    }

    fn command(&self, dir: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.exe);
        cmd.args(["--no-pager", "--no-optional-locks"])
            .args(["-c", "core.fsmonitor="])
            .args(["-c", "core.quotepath=false"])
            .args(["-c", "color.ui=false"])
            .arg("-C")
            .arg(dir)
            .args(args);
        for (key, _) in std::env::vars_os() {
            if key
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
            {
                cmd.env_remove(key);
            }
        }
        cmd.env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .env("LANGUAGE", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    /// Runs `git -C dir args…`; `Ok((success, stdout, stderr))`.
    async fn run(&self, dir: &Path, args: &[&str]) -> Result<(bool, String, String), GitError> {
        let child = self
            .command(dir, args)
            .spawn()
            .map_err(|e| GitError::Unavailable(e.to_string()))?;
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| GitError::TimedOut)?
            .map_err(|e| GitError::Failed(e.to_string()))?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }

    /// Runs a command that must succeed; its stdout.
    async fn read(&self, dir: &Path, args: &[&str]) -> Result<String, GitError> {
        let (ok, stdout, stderr) = self.run(dir, args).await?;
        if ok {
            Ok(stdout)
        } else {
            Err(classify(&stderr))
        }
    }

    /// Which working tree and repository `dir` belongs to; `None` when it is
    /// not inside a working tree (or does not exist).
    pub async fn locate(&self, dir: &Path) -> Result<Option<RepoLocation>, GitError> {
        if !dir.is_dir() {
            return Ok(None);
        }
        let (ok, stdout, stderr) = self.run(dir, &["rev-parse", "--show-toplevel"]).await?;
        if !ok {
            return match classify(&stderr) {
                GitError::Failed(msg) if is_not_a_worktree(&msg) => Ok(None),
                err => Err(err),
            };
        }
        let worktree = clean(Path::new(stdout.trim()));
        // Asked from the top folder: relative answers are relative to it
        // (older Git printed `--git-common-dir` relative to the top even
        // when asked from a subfolder).
        let out = self
            .read(&worktree, &["rev-parse", "--git-dir", "--git-common-dir"])
            .await?;
        let mut lines = out.lines().map(str::trim);
        let (Some(git_dir), Some(common)) = (lines.next(), lines.next()) else {
            return Err(GitError::Failed("unexpected rev-parse output".into()));
        };
        let git_dir = clean(&worktree.join(git_dir));
        let common = clean(&worktree.join(common));
        let repository = match common.file_name() {
            Some(name) if name == ".git" => {
                common.parent().map_or(common.clone(), Path::to_path_buf)
            }
            _ => common.clone(),
        };
        Ok(Some(RepoLocation {
            worktree_root: display(&worktree),
            repository_root: display(&repository),
            linked_worktree: !same_path(&git_dir, &common),
        }))
    }

    /// `git status` of the working tree at `root`.
    pub async fn status(&self, root: &Path) -> Result<WorktreeStatus, GitError> {
        let out = self
            .read(
                root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=normal",
                    "--ignore-submodules=dirty",
                ],
            )
            .await?;
        Ok(parse::parse_status_v2(&out))
    }

    /// The latest `count` commits of HEAD (empty for a repository without commits).
    pub async fn recent_commits(
        &self,
        root: &Path,
        count: usize,
    ) -> Result<Vec<CommitInfo>, GitError> {
        let n = format!("-n{}", count.max(1));
        self.log(root, &["log", &n, "--no-color", parse::LOG_FORMAT])
            .await
    }

    /// Commits reachable from `to` but not from `from` (newest first, at most
    /// `count`). Both must be commit ids.
    pub async fn commits_between(
        &self,
        root: &Path,
        from: &str,
        to: &str,
        count: usize,
    ) -> Result<Vec<CommitInfo>, GitError> {
        if !is_commit_id(from) || !is_commit_id(to) {
            return Err(GitError::Failed("not a commit id".into()));
        }
        let n = format!("-n{}", count.max(1));
        let range = format!("{from}..{to}");
        self.log(
            root,
            &["log", &n, "--no-color", parse::LOG_FORMAT, &range, "--"],
        )
        .await
    }

    async fn log(&self, root: &Path, args: &[&str]) -> Result<Vec<CommitInfo>, GitError> {
        let (ok, stdout, stderr) = self.run(root, args).await?;
        if ok {
            return Ok(parse::parse_log(&stdout));
        }
        if stderr.contains("does not have any commits") || stderr.contains("bad default revision") {
            return Ok(Vec::new());
        }
        Err(classify(&stderr))
    }

    /// The repository's worktrees.
    pub async fn worktrees(&self, root: &Path) -> Result<Vec<WorktreeInfo>, GitError> {
        let out = self
            .read(root, &["worktree", "list", "--porcelain"])
            .await?;
        let mut list = parse::parse_worktrees(&out);
        for tree in &mut list {
            tree.path = display(&clean(Path::new(&tree.path)));
        }
        Ok(list)
    }

    /// Location, status, recent commits and worktrees of the working tree
    /// containing `dir`; `None` when it is not in one.
    pub async fn snapshot(
        &self,
        dir: &Path,
        commits: usize,
    ) -> Result<Option<RepoSnapshot>, GitError> {
        let Some(location) = self.locate(dir).await? else {
            return Ok(None);
        };
        let root = PathBuf::from(&location.worktree_root);
        let status = self.status(&root).await?;
        let recent_commits = if status.head.is_some() {
            self.recent_commits(&root, commits).await?
        } else {
            Vec::new()
        };
        // Worktrees are extra information: a failure there is not fatal.
        let worktrees = self.worktrees(&root).await.unwrap_or_default();
        Ok(Some(RepoSnapshot {
            location,
            status,
            recent_commits,
            worktrees,
        }))
    }
}

fn classify(stderr: &str) -> GitError {
    let message = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no error message")
        .trim_start_matches("fatal: ")
        .to_owned();
    if stderr.contains("dubious ownership") || stderr.contains("safe.directory") {
        GitError::Refused(
            "the folder belongs to another Windows user (Git's safe.directory check)".into(),
        )
    } else {
        GitError::Failed(message)
    }
}

fn is_not_a_worktree(message: &str) -> bool {
    message.contains("not a git repository")
        || message.contains("must be run in a work tree")
        || message.contains("cannot change to")
}

/// A (possibly abbreviated) hexadecimal commit id; nothing else is ever
/// passed to Git as a revision.
pub fn is_commit_id(text: &str) -> bool {
    (4..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `true` when `version` (e.g. "2.43.0" or "2.45.1.windows.1") is at least
/// [`MIN_VERSION`].
pub fn version_supported(version: &str) -> bool {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    (major, minor) >= MIN_VERSION
}

/// Removes `.` and `..` components without touching the disk.
fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(part);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// How paths are shown and compared: the platform's separators.
fn display(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        display(a).to_lowercase() == display(b).to_lowercase()
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_hex_ids_are_revisions() {
        assert!(is_commit_id("2d0f3c8a"));
        assert!(!is_commit_id("--output=x"));
        assert!(!is_commit_id("HEAD"));
        assert!(!is_commit_id("abc"));
        assert!(!is_commit_id("main..evil"));
    }

    #[test]
    fn versions() {
        assert!(version_supported("2.43.0"));
        assert!(version_supported("2.45.1.windows.1"));
        assert!(version_supported("2.15.0"));
        assert!(!version_supported("2.14.9"));
        assert!(!version_supported("1.9"));
        assert!(!version_supported(""));
    }

    #[test]
    fn errors_are_classified() {
        let refused = classify("fatal: detected dubious ownership in repository at 'C:/x'\nTo add an exception…safe.directory");
        assert!(matches!(refused, GitError::Refused(_)));
        assert_eq!(
            classify("fatal: not a git repository (or any of the parent directories): .git\n"),
            GitError::Failed(
                "not a git repository (or any of the parent directories): .git".into()
            )
        );
    }

    #[test]
    fn paths_are_cleaned_lexically() {
        assert_eq!(
            clean(Path::new("/a/b/../c/./.git")),
            PathBuf::from("/a/c/.git")
        );
    }
}
