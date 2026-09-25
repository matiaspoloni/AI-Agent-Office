//! What Agent Office knows about a repository. Everything here comes from
//! Git's own machine-readable output; nothing is guessed.

use serde::Serialize;
use ts_rs::TS;

/// Where a folder sits in Git: its working tree and the repository it
/// belongs to (a linked worktree shares the repository of its main one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepoLocation {
    /// Top folder of the working tree the folder is in.
    pub worktree_root: String,
    /// The repository: the main working tree's folder (shared by all its
    /// worktrees). Used as the repository id.
    pub repository_root: String,
    /// True for a worktree added with `git worktree add`.
    pub linked_worktree: bool,
}

/// How a file differs, as Git reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ChangeKind {
    Modified,
    TypeChanged,
    Added,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Conflicted,
}

/// One changed file (paths relative to the working tree, `/`-separated).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileChange {
    pub path: String,
    /// The previous path of a renamed or copied file.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub orig_path: Option<String>,
    /// Change recorded in the index (will be in the next commit).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub staged: Option<ChangeKind>,
    /// Change in the working tree that is not staged yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unstaged: Option<ChangeKind>,
}

/// File counts of a working tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StatusCounts {
    /// Files with staged changes.
    pub staged: u32,
    /// Tracked files changed but not staged.
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicted: u32,
}

impl StatusCounts {
    /// Files with changes that are not staged yet (what "dirty" means in
    /// Agent Office): unstaged edits, new files and conflicts.
    pub fn dirty(&self) -> u32 {
        self.unstaged + self.untracked + self.conflicted
    }
}

/// `git status` of one working tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorktreeStatus {
    /// Checked-out branch (`None` when detached or unknown).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branch: Option<String>,
    pub detached: bool,
    /// Commit checked out (`None` in a repository without commits).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub upstream: Option<String>,
    /// Commits not on the upstream yet / on the upstream but not here.
    /// Only known when the branch has an upstream.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ahead: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub behind: Option<u32>,
    pub counts: StatusCounts,
    /// Changed files (the first `MAX_FILES`; `counts` are always complete).
    pub files: Vec<FileChange>,
    pub files_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub author: String,
    /// Commit time (committer date), milliseconds since the epoch.
    #[ts(type = "number")]
    pub time: i64,
    pub summary: String,
}

/// An entry of `git worktree list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorktreeInfo {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}
