//! Git in the host: follows the working trees agents work in and turns what
//! Git reports into unified `git.*` events for their sessions.
//!
//! * A session is linked to the working tree of its folder (`cwd`).
//! * A tree is re-read shortly after an agent edits files, runs a command or
//!   finishes a turn (debounced), every 30 s while an agent works in it, and
//!   when the UI asks.
//! * Branch, file counts and ahead/behind are facts about the tree, so every
//!   session working there gets them. A **commit** is only credited to a
//!   session with evidence: that session ran a commit-creating `git` command
//!   in that tree, and the commit's time falls within that command's run.
//!   Otherwise the commit is shown in the repository, credited to nobody.

use ao_core::event::{
    AgentEvent, EventKind, EventSource, GitBranchChanged, GitCommitCreated, GitStatusChanged,
    SessionInfo,
};
use ao_core::ids::{session_key, ProviderId, RepositoryId, SessionId};
use ao_core::provider::EventSink;
use ao_core::time::now_ms;
use ao_git::{CommitInfo, Git, RepoLocation, RepoSnapshot};
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::Notify;
use ts_rs::TS;

/// Wait after the last sign of work before re-reading a tree.
const DEBOUNCE_MS: i64 = 1_500;
/// Re-read trees agents work in at least this often.
const PERIODIC_MS: i64 = 30_000;
/// Look again at a folder that was not a working tree (it may be one now).
const RECHECK_FOLDER_MS: i64 = 60_000;
/// How long a finished session's commit commands still count as evidence.
const EVIDENCE_KEEP_MS: i64 = 10 * 60_000;
/// Commit times have one-second resolution.
const CLOCK_SLACK_MS: i64 = 1_000;
const RECENT_COMMITS: usize = 10;
const NEW_COMMITS_MAX: usize = 50;
const CREDITED_KEPT: usize = 200;
const TICK: Duration = Duration::from_secs(1);

/// A `git` invocation that creates commits (`commit`, `merge`, `cherry-pick`,
/// `revert`, `am`, `rebase`, `pull`), anywhere in a shell command line
/// (`git add . && git commit -m …`, `bash -lc 'git commit …'`,
/// `git -C sub commit`). Only used as evidence, never to change anything.
static COMMIT_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:^|[\s;&|(`'"])git(?:\.exe)?(?:\s+(?:-C\s+\S+|-c\s+\S+|--[a-z][a-z-]*(?:=\S+)?|-[a-z]))*\s+(?:commit|merge|cherry-pick|revert|am|rebase|pull)(?:$|[\s;&|)`'"])"#,
    )
    .expect("valid regex")
});

pub fn is_commit_command(command: &str) -> bool {
    COMMIT_COMMAND.is_match(command)
}

/// Where the UI finds a repository's state.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepositoryView {
    pub location: RepoLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub snapshot: Option<RepoSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub refreshed_at: Option<i64>,
    /// Sessions working in this tree (keys), active ones first.
    pub sessions: Vec<String>,
    /// Projects whose folder is in this tree (ids).
    pub projects: Vec<String>,
    /// Changed files an agent's tool is known to have written.
    pub file_sessions: Vec<FileSessions>,
    /// Commits credited to a session (see the module docs).
    pub commit_sessions: Vec<CommitSession>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileSessions {
    pub path: String,
    pub sessions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommitSession {
    pub sha: String,
    pub session: String,
}

/// A project folder and the working tree it is in, if any.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectRepository {
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub worktree_root: Option<String>,
    /// Why there is none ("not a Git repository", a Git error, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepositoriesReport {
    /// False when Git is not installed (then nothing else is filled).
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unavailable_reason: Option<String>,
    pub repositories: Vec<RepositoryView>,
    pub projects: Vec<ProjectRepository>,
}

/// A session's folder as the Git service knows it (for file evidence).
pub struct LinkedSession {
    pub key: String,
    pub folder: String,
}

struct Link {
    provider: ProviderId,
    session: SessionId,
    folder: String,
    active: bool,
    ended_at: Option<i64>,
    sent: Sent,
    evidence: Vec<Evidence>,
}

/// What was last told to the session, so only changes are sent.
#[derive(Default)]
struct Sent {
    tree: Option<String>,
    branch: Option<Option<String>>,
    status: Option<GitStatusChanged>,
    worktree: Option<String>,
}

/// A commit-creating command the session ran.
struct Evidence {
    command_id: Option<String>,
    started: i64,
    finished: Option<i64>,
}

enum Folder {
    Unknown,
    Tree { root: String },
    NotRepo { checked: i64 },
    Error { message: String, checked: i64 },
}

struct Tree {
    location: RepoLocation,
    snapshot: Option<RepoSnapshot>,
    error: Option<String>,
    refreshed_at: Option<i64>,
    due: Option<i64>,
    credited: VecDeque<CommitSession>,
}

#[derive(Default)]
struct State {
    sessions: HashMap<String, Link>,
    folders: HashMap<String, Folder>,
    trees: HashMap<String, Tree>,
    projects: Vec<(String, String)>,
}

impl State {
    fn tree_of(&self, folder: &str) -> Option<&str> {
        match self.folders.get(folder) {
            Some(Folder::Tree { root }) => Some(root),
            _ => None,
        }
    }

    fn schedule(&mut self, folder: &str, at: i64) {
        let root = self.tree_of(folder).map(str::to_owned);
        match root.and_then(|r| self.trees.get_mut(&r)) {
            Some(tree) => tree.due = Some(tree.due.map_or(at, |d| d.min(at))),
            None => {
                self.folders
                    .entry(folder.to_owned())
                    .or_insert(Folder::Unknown);
            }
        }
    }
}

pub struct GitService {
    git: Option<Git>,
    unavailable: Option<String>,
    sink: EventSink,
    state: Mutex<State>,
    wake: Notify,
    /// One reading pass at a time (worker or UI request).
    pass: tokio::sync::Mutex<()>,
}

impl GitService {
    /// `executable`: the `git` to use; `None` finds it like Diagnostics does.
    pub fn start(sink: EventSink, executable: Option<PathBuf>) -> Arc<Self> {
        let (exe, unavailable) = match executable {
            Some(path) if path.is_file() => (Some(path), None),
            Some(path) => (
                None,
                Some(format!(
                    "The configured Git ({}) does not exist.",
                    path.display()
                )),
            ),
            None => {
                match ao_detect::find_executable(&["git".to_string()], crate::diagnostics::GIT_DIRS)
                {
                    Some(path) => (Some(path), None),
                    None => (
                        None,
                        Some("Git was not found on PATH or in its usual folders.".to_string()),
                    ),
                }
            }
        };
        let service = Arc::new(Self {
            unavailable,
            git: exe.map(Git::new),
            sink,
            state: Mutex::new(State::default()),
            wake: Notify::new(),
            pass: tokio::sync::Mutex::new(()),
        });
        if service.git.is_some() {
            let weak = Arc::downgrade(&service);
            tokio::spawn(worker(weak));
        }
        service
    }

    pub fn available(&self) -> bool {
        self.git.is_some()
    }

    /// Called for every accepted event with the session's folder. Cheap: only
    /// bookkeeping; Git runs in the background worker.
    pub fn observe(&self, event: &AgentEvent, folder: Option<&str>) {
        if event.source == EventSource::Git || self.git.is_none() {
            return;
        }
        let key = session_key(&event.provider, &event.session_id);
        let folder = folder.map(str::trim).filter(|f| !f.is_empty());
        let now = now_ms();
        let mut st = self.state.lock().expect("git state");
        let link = match st.sessions.get_mut(&key) {
            Some(link) => link,
            None => {
                let Some(folder) = folder else {
                    return;
                };
                st.sessions.insert(
                    key.clone(),
                    Link {
                        provider: event.provider.clone(),
                        session: event.session_id.clone(),
                        folder: folder.to_owned(),
                        active: true,
                        ended_at: None,
                        sent: Sent::default(),
                        evidence: Vec::new(),
                    },
                );
                st.sessions.get_mut(&key).expect("just inserted")
            }
        };
        if let Some(folder) = folder {
            if link.folder != folder {
                link.folder = folder.to_owned();
                link.sent = Sent::default();
            }
        }
        let mut when = None;
        match &event.kind {
            EventKind::SessionStarted(_) | EventKind::SessionUpdated(_) => {
                if matches!(event.kind, EventKind::SessionStarted(_)) {
                    link.active = true;
                    link.ended_at = None;
                }
                when = Some(now);
            }
            EventKind::SessionEnded(_) => {
                link.active = false;
                link.ended_at = Some(now);
                when = Some(now + DEBOUNCE_MS);
            }
            EventKind::CommandStarted(cmd) => {
                if is_commit_command(&cmd.command) {
                    link.evidence.push(Evidence {
                        command_id: cmd.command_id.clone(),
                        started: event.timestamp,
                        finished: None,
                    });
                }
            }
            EventKind::CommandCompleted(cmd) | EventKind::CommandFailed(cmd) => {
                let open = link.evidence.iter_mut().rev().find(|e| {
                    e.finished.is_none()
                        && (cmd.command_id.is_none() || e.command_id == cmd.command_id)
                });
                if let Some(evidence) = open {
                    evidence.finished = Some(event.timestamp.max(evidence.started));
                }
                when = Some(now + DEBOUNCE_MS);
            }
            EventKind::ToolCompleted(_)
            | EventKind::ToolFailed(_)
            | EventKind::FileCreated(_)
            | EventKind::FileModified(_)
            | EventKind::FileDeleted(_)
            | EventKind::AgentIdle(_) => when = Some(now + DEBOUNCE_MS),
            _ => {}
        }
        if let Some(at) = when {
            let folder = link.folder.clone();
            st.schedule(&folder, at);
            drop(st);
            self.wake.notify_one();
        }
    }

    /// Project folders to show in the Projects view.
    pub fn set_projects(&self, projects: Vec<(String, String)>) {
        let mut st = self.state.lock().expect("git state");
        for (_, folder) in &projects {
            st.folders.entry(folder.clone()).or_insert(Folder::Unknown);
        }
        st.projects = projects;
    }

    /// Re-reads, now, every tree (projects' too) not read in the last `max_age`.
    pub async fn refresh_now(&self, max_age: Duration) {
        let Some(git) = &self.git else {
            return;
        };
        let _pass = self.pass.lock().await;
        let folders: Vec<String> = {
            let st = self.state.lock().expect("git state");
            let mut list: Vec<String> = st.projects.iter().map(|(_, f)| f.clone()).collect();
            list.extend(st.sessions.values().map(|l| l.folder.clone()));
            list
        };
        self.resolve(git, folders, true).await;
        let cutoff = now_ms() - max_age.as_millis() as i64;
        let roots: Vec<String> = {
            let st = self.state.lock().expect("git state");
            st.trees
                .iter()
                .filter(|(_, t)| t.refreshed_at.map_or(true, |r| r < cutoff))
                .map(|(r, _)| r.clone())
                .collect()
        };
        for root in roots {
            self.refresh_tree(git, &root).await;
        }
    }

    /// One background pass: look up new folders, re-read due trees.
    async fn pass(&self) {
        let Some(git) = &self.git else {
            return;
        };
        let _pass = self.pass.lock().await;
        let now = now_ms();
        let folders: Vec<String> = {
            let mut st = self.state.lock().expect("git state");
            prune(&mut st, now);
            st.sessions
                .values()
                .filter(|l| l.active)
                .map(|l| l.folder.clone())
                .collect()
        };
        self.resolve(git, folders, false).await;
        let due: Vec<String> = {
            let st = self.state.lock().expect("git state");
            let busy: Vec<&str> = st
                .sessions
                .values()
                .filter(|l| l.active)
                .filter_map(|l| st.tree_of(&l.folder))
                .collect();
            st.trees
                .iter()
                .filter(|(root, t)| {
                    t.due.is_some_and(|d| d <= now)
                        || (busy.contains(&root.as_str())
                            && t.refreshed_at.map_or(true, |r| now - r >= PERIODIC_MS))
                })
                .map(|(r, _)| r.clone())
                .collect()
        };
        for root in due {
            self.refresh_tree(git, &root).await;
        }
    }

    /// Finds the working tree of folders not looked up yet (or, when `all`,
    /// looked up without success a while ago).
    async fn resolve(&self, git: &Git, folders: Vec<String>, all: bool) {
        let now = now_ms();
        for folder in folders {
            let wanted = {
                let st = self.state.lock().expect("git state");
                match st.folders.get(&folder) {
                    None | Some(Folder::Unknown) => true,
                    Some(Folder::Tree { root }) => !st.trees.contains_key(root),
                    Some(Folder::NotRepo { checked } | Folder::Error { checked, .. }) => {
                        all || now - checked >= RECHECK_FOLDER_MS
                    }
                }
            };
            if !wanted {
                continue;
            }
            let found = git.locate(Path::new(&folder)).await;
            let mut st = self.state.lock().expect("git state");
            let entry = match found {
                Ok(Some(location)) => {
                    let root = location.worktree_root.clone();
                    st.trees.entry(root.clone()).or_insert_with(|| Tree {
                        location,
                        snapshot: None,
                        error: None,
                        refreshed_at: None,
                        due: Some(now),
                        credited: VecDeque::new(),
                    });
                    Folder::Tree { root }
                }
                Ok(None) => Folder::NotRepo { checked: now },
                Err(err) => {
                    tracing::debug!(%folder, %err, "git locate failed");
                    Folder::Error {
                        message: err.to_string(),
                        checked: now,
                    }
                }
            };
            st.folders.insert(folder, entry);
        }
    }

    async fn refresh_tree(&self, git: &Git, root: &str) {
        let prev = {
            let st = self.state.lock().expect("git state");
            let Some(tree) = st.trees.get(root) else {
                return;
            };
            tree.snapshot
                .as_ref()
                .map(|s| (s.status.head.clone(), s.recent_commits.len()))
        };
        let result = git.snapshot(Path::new(root), RECENT_COMMITS).await;
        let now = now_ms();
        let snapshot = match result {
            Ok(Some(snapshot)) => snapshot,
            other => {
                let mut st = self.state.lock().expect("git state");
                let folders_to_recheck: Vec<String> = st
                    .folders
                    .iter()
                    .filter(|(_, f)| matches!(f, Folder::Tree { root: r } if r == root))
                    .map(|(k, _)| k.clone())
                    .collect();
                if let Some(tree) = st.trees.get_mut(root) {
                    tree.error = Some(match &other {
                        Err(err) => err.to_string(),
                        _ => "This folder is no longer a Git working tree.".into(),
                    });
                    tree.refreshed_at = Some(now);
                    tree.due = None;
                }
                if matches!(other, Ok(None)) {
                    for folder in folders_to_recheck {
                        st.folders.insert(folder, Folder::Unknown);
                    }
                    st.trees.remove(root);
                }
                return;
            }
        };
        let new_commits: Vec<CommitInfo> = match (&prev, &snapshot.status.head) {
            (Some((Some(before), _)), Some(after)) if before != after => git
                .commits_between(Path::new(root), before, after, NEW_COMMITS_MAX)
                .await
                .unwrap_or_default(),
            // The first commits of a repository that had none.
            (Some((None, _)), Some(_)) => snapshot.recent_commits.clone(),
            _ => Vec::new(),
        };
        let events = {
            let mut st = self.state.lock().expect("git state");
            let State {
                sessions,
                folders,
                trees,
                ..
            } = &mut *st;
            let Some(tree) = trees.get_mut(root) else {
                return;
            };
            tree.location = snapshot.location.clone();
            tree.error = None;
            tree.refreshed_at = Some(now);
            tree.due = None;
            let in_tree = |link: &Link| matches!(folders.get(&link.folder), Some(Folder::Tree { root: r }) if r == root);
            let mut events = Vec::new();
            let repository = RepositoryId(snapshot.location.repository_root.clone());
            let status = &snapshot.status;
            let counts = GitStatusChanged {
                dirty: status.counts.dirty(),
                staged: status.counts.staged,
                untracked: Some(status.counts.untracked),
                conflicted: Some(status.counts.conflicted),
                ahead: status.ahead,
                behind: status.behind,
            };
            let worktree = snapshot
                .location
                .linked_worktree
                .then(|| snapshot.location.worktree_root.clone());
            let mut keys: Vec<&String> = sessions.keys().collect();
            keys.sort();
            let keys: Vec<String> = keys.into_iter().cloned().collect();
            for key in &keys {
                let link = sessions.get_mut(key).expect("key from map");
                if !link.active || !in_tree(link) {
                    continue;
                }
                let (provider, session) = (link.provider.clone(), link.session.clone());
                let make = |kind: EventKind| {
                    let mut e = AgentEvent::for_session(
                        provider.clone(),
                        session.clone(),
                        EventSource::Git,
                        kind,
                    );
                    e.repository_id = Some(repository.clone());
                    e
                };
                if link.sent.tree.as_deref() != Some(root) {
                    link.sent = Sent {
                        tree: Some(root.to_owned()),
                        ..Default::default()
                    };
                }
                if let Some(path) = &worktree {
                    if link.sent.worktree.as_ref() != Some(path) {
                        events.push(make(EventKind::SessionUpdated(SessionInfo {
                            worktree: Some(path.clone()),
                            ..Default::default()
                        })));
                    }
                }
                if link.sent.branch.as_ref() != Some(&status.branch) {
                    events.push(make(EventKind::GitBranchChanged(GitBranchChanged {
                        branch: status.branch.clone(),
                        previous: link.sent.branch.clone().flatten(),
                    })));
                }
                if link.sent.status.as_ref() != Some(&counts) {
                    events.push(make(EventKind::GitStatusChanged(counts.clone())));
                }
                link.sent.worktree = worktree.clone();
                link.sent.branch = Some(status.branch.clone());
                link.sent.status = Some(counts.clone());
            }
            // Oldest first, so the log reads in order.
            for commit in new_commits.iter().rev() {
                let candidates: Vec<&String> = keys
                    .iter()
                    .filter(|key| {
                        let link = &sessions[*key];
                        in_tree(link)
                            && link.evidence.iter().any(|e| {
                                commit.time >= e.started - CLOCK_SLACK_MS
                                    && commit.time <= e.finished.unwrap_or(now) + CLOCK_SLACK_MS
                            })
                    })
                    .collect();
                if let [key] = candidates[..] {
                    let link = &sessions[key];
                    let mut e = AgentEvent::for_session(
                        link.provider.clone(),
                        link.session.clone(),
                        EventSource::Git,
                        EventKind::GitCommitCreated(GitCommitCreated {
                            sha: commit.sha.clone(),
                            summary: commit.summary.clone(),
                        }),
                    );
                    e.repository_id = Some(repository.clone());
                    events.push(e);
                    tree.credited.push_front(CommitSession {
                        sha: commit.sha.clone(),
                        session: key.clone(),
                    });
                    tree.credited.truncate(CREDITED_KEPT);
                } else if candidates.len() > 1 {
                    tracing::info!(sha = %commit.sha, "commit not credited: several sessions ran git commit");
                }
            }
            tree.snapshot = Some(snapshot);
            events
        };
        for event in events {
            self.sink.emit(event);
        }
    }

    /// Working trees the service follows.
    pub fn known_roots(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("git state")
            .trees
            .keys()
            .cloned()
            .collect()
    }

    /// Repositories and project folders, for the UI. File evidence is added
    /// by the host (it needs the sessions' changed files).
    pub fn report(&self) -> (RepositoriesReport, HashMap<String, Vec<LinkedSession>>) {
        let st = self.state.lock().expect("git state");
        let mut links: HashMap<String, Vec<LinkedSession>> = HashMap::new();
        let mut order: Vec<(&String, &Link)> = st.sessions.iter().collect();
        order.sort_by_key(|(key, link)| (!link.active, (*key).clone()));
        for (key, link) in order {
            if let Some(root) = st.tree_of(&link.folder) {
                links
                    .entry(root.to_owned())
                    .or_default()
                    .push(LinkedSession {
                        key: key.clone(),
                        folder: link.folder.clone(),
                    });
            }
        }
        let projects: Vec<ProjectRepository> = st
            .projects
            .iter()
            .map(|(id, folder)| {
                let (worktree_root, note) = match st.folders.get(folder) {
                    Some(Folder::Tree { root }) => (Some(root.clone()), None),
                    Some(Folder::NotRepo { .. }) => {
                        (None, Some("Not in a Git repository".to_string()))
                    }
                    Some(Folder::Error { message, .. }) => (None, Some(message.clone())),
                    _ => (None, None),
                };
                ProjectRepository {
                    project_id: id.clone(),
                    worktree_root,
                    note,
                }
            })
            .collect();
        let mut repositories: Vec<RepositoryView> = st
            .trees
            .iter()
            .map(|(root, tree)| RepositoryView {
                location: tree.location.clone(),
                snapshot: tree.snapshot.clone(),
                error: tree.error.clone(),
                refreshed_at: tree.refreshed_at,
                sessions: links
                    .get(root)
                    .map(|l| l.iter().map(|s| s.key.clone()).collect())
                    .unwrap_or_default(),
                projects: projects
                    .iter()
                    .filter(|p| p.worktree_root.as_deref() == Some(root))
                    .map(|p| p.project_id.clone())
                    .collect(),
                file_sessions: Vec::new(),
                commit_sessions: tree.credited.iter().cloned().collect(),
            })
            .collect();
        repositories.sort_by(|a, b| a.location.worktree_root.cmp(&b.location.worktree_root));
        (
            RepositoriesReport {
                available: self.git.is_some(),
                unavailable_reason: self.unavailable.clone(),
                repositories,
                projects,
            },
            links,
        )
    }
}

/// Drops sessions that ended long ago, and old evidence.
fn prune(st: &mut State, now: i64) {
    st.sessions
        .retain(|_, l| l.ended_at.map_or(true, |t| now - t < EVIDENCE_KEEP_MS));
    for link in st.sessions.values_mut() {
        link.evidence.retain(|e| {
            let last = e.finished.unwrap_or(e.started + 6 * EVIDENCE_KEEP_MS);
            now - last < EVIDENCE_KEEP_MS
        });
    }
}

async fn worker(service: Weak<GitService>) {
    loop {
        let Some(this) = service.upgrade() else {
            return;
        };
        this.pass().await;
        // Until there is new work, or the next tick (periodic re-reads).
        let _ = tokio::time::timeout(TICK, this.wake.notified()).await;
    }
}

/// `path` (as an agent's tool reported it) relative to `root`, `/`-separated;
/// `None` when it is outside. Relative paths are taken from `cwd`.
pub fn relative_to_tree(root: &str, cwd: &str, path: &str) -> Option<String> {
    let full = if Path::new(path).is_absolute() || looks_absolute(path) {
        PathBuf::from(path)
    } else {
        Path::new(cwd).join(path)
    };
    lexically_within(root, &full.to_string_lossy()).or_else(|| {
        // The same folder can be written two ways (Windows short names such
        // as `RUNNER~1`, symbolic links): compare the real paths.
        lexically_within(&canonical(Path::new(root))?, &canonical(&full)?)
    })
}

fn lexically_within(root: &str, full: &str) -> Option<String> {
    let full = normalize(full);
    let root = normalize(root);
    let rest = strip_prefix_ci(&full, &root)?;
    let rest = rest.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_owned())
}

/// The real path of `path` (of its folder, for a deleted file), without
/// Windows' `\\?\` prefix.
fn canonical(path: &Path) -> Option<String> {
    let real = std::fs::canonicalize(path).ok().or_else(|| {
        let parent = std::fs::canonicalize(path.parent()?).ok()?;
        Some(parent.join(path.file_name()?))
    })?;
    let text = real.to_string_lossy().into_owned();
    Some(match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned(),
    })
}

fn looks_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || path.starts_with("\\\\")
}

/// `/` separators, `.` and `..` resolved, no trailing slash.
fn normalize(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let absolute = unified.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    if cfg!(windows) {
        let head = text.get(..prefix.len())?;
        head.eq_ignore_ascii_case(prefix)
            .then(|| &text[prefix.len()..])
    } else {
        text.strip_prefix(prefix)
    }
}

/// Whether a changed-file entry of `git status` (a file, or a folder ending
/// in `/` for new folders) covers `touched` (both relative to the tree).
pub fn covers(entry: &str, touched: &str) -> bool {
    let same = |a: &str, b: &str| {
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    if entry.ends_with('/') {
        touched.len() > entry.len() && same(&touched[..entry.len()], entry)
    } else {
        same(entry, touched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_commands_are_recognised_conservatively() {
        for yes in [
            "git commit -m 'Add login'",
            "git add . && git commit -am \"x\"",
            "/bin/bash -lc 'git commit -m fix'",
            "powershell.exe -Command \"git commit -m fix\"",
            "git -C sub/dir commit",
            "git -c user.name=Bot commit --amend --no-edit",
            "cd app; git merge feature",
            "git cherry-pick abc123",
            "git pull --rebase",
            "git.exe commit -m x",
            "git --no-pager rebase main",
        ] {
            assert!(is_commit_command(yes), "{yes}");
        }
        for no in [
            "git status",
            "git log --grep commit",
            "git diff HEAD~1",
            "npm run commit",
            "gitk --all",
            "git stash",
            "echo commit",
            "git commitment",
            "legit commit",
        ] {
            assert!(!is_commit_command(no), "{no}");
        }
    }

    #[test]
    fn tool_paths_are_placed_in_the_tree() {
        assert_eq!(
            relative_to_tree("/work/app", "/work/app/src", "main.rs").as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(
            relative_to_tree("/work/app", "/work/app", "/work/app/docs/../README.md").as_deref(),
            Some("README.md")
        );
        assert_eq!(
            relative_to_tree("/work/app", "/work/app", "/work/other/x"),
            None
        );
        assert_eq!(
            relative_to_tree("/work/app", "/work/app", "/work/application/x"),
            None
        );
        assert_eq!(
            relative_to_tree(r"C:\Work\App", r"C:\Work\App", r"C:\Work\App\src\a.ts").as_deref(),
            Some("src/a.ts")
        );
        assert!(covers("notes/", "notes/a.txt"));
        assert!(!covers("notes/", "notes/"));
        assert!(covers("src/a.ts", "src/a.ts"));
        assert!(!covers("src/a.ts", "src/a.tsx"));
    }
}
