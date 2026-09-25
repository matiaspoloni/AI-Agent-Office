//! The Git service in the real host, against real temporary repositories:
//! sessions get their repository's branch and status, commits are credited
//! only with evidence, and changed files are linked to the agents whose tools
//! wrote them.

mod common;

use agent_office_lib::git::RepositoriesReport;
use agent_office_lib::host::{Host, HostOptions};
use agent_office_lib::paths::AppPaths;
use ao_core::event::{
    AgentEvent, CommandFinished, CommandStarted, EventKind, EventSource, FileTouched, SessionInfo,
};
use ao_core::provider::EventSink;
use common::{wait_for, TempDir};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

fn options(dir: &Path) -> HostOptions {
    let mut options = HostOptions::default();
    // No real agent CLIs from this machine.
    options.claude.executable = Some(dir.join("no-claude"));
    options.claude.config_dir = Some(dir.join("claude-config"));
    options.codex.executable = Some(dir.join("no-codex"));
    options.codex.config_dir = Some(dir.join("codex-home"));
    options.cursor.executable = Some(dir.join("no-cursor"));
    options
}

fn start(tmp: &TempDir) -> Arc<Host> {
    Host::start(AppPaths::at(tmp.sub("data")), options(&tmp.sub("bin")))
}

/// Git for test setup, isolated from the machine's settings.
fn git(dir: &Path, args: &[&str]) -> String {
    let empty = std::env::temp_dir().join("ao-e2e-empty-gitconfig");
    let _ = std::fs::write(&empty, "");
    let out = Command::new("git")
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "user.name=Someone",
            "-c",
            "user.email=someone@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &empty)
        .output()
        .expect("git is installed");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The folder as the Git service names it.
fn toplevel(dir: &Path) -> String {
    let top = git(dir, &["rev-parse", "--show-toplevel"]);
    if cfg!(windows) {
        top.replace('/', "\\")
    } else {
        top
    }
}

fn repo_with_commit(tmp: &TempDir, name: &str) -> std::path::PathBuf {
    let root = tmp.sub(name);
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("README.md"), "hello\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    root
}

fn event(session: &str, kind: EventKind) -> AgentEvent {
    AgentEvent::for_session("demo", session, EventSource::Simulation, kind)
}

fn started(sink: &EventSink, session: &str, cwd: &Path) {
    sink.emit(event(
        session,
        EventKind::SessionStarted(SessionInfo {
            cwd: Some(cwd.display().to_string()),
            ..Default::default()
        }),
    ));
}

/// What an agent's `git commit` looks like to Agent Office: the command
/// starts, the commit happens, the command ends.
fn agent_commits(sink: &EventSink, session: &str, root: &Path, id: &str, message: &str) -> String {
    sink.emit(event(
        session,
        EventKind::CommandStarted(CommandStarted {
            command_id: Some(id.into()),
            command: format!("git commit -am '{message}'"),
            cwd: Some(root.display().to_string()),
        }),
    ));
    git(root, &["commit", "-q", "-am", message]);
    sink.emit(event(
        session,
        EventKind::CommandCompleted(CommandFinished {
            command_id: Some(id.into()),
            command: None,
            exit_code: Some(0),
            duration_ms: None,
            error: None,
        }),
    ));
    git(root, &["rev-parse", "HEAD"])
}

fn session<'a>(
    snapshot: &'a ao_core::world::WorldSnapshot,
    id: &str,
) -> Option<&'a ao_core::world::SessionState> {
    snapshot.sessions.iter().find(|s| s.session_id.0 == id)
}

async fn report(host: &Host) -> RepositoriesReport {
    host.repositories(true).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sessions_get_their_repository_branch_and_status() {
    let tmp = TempDir::new("git-status");
    let root = repo_with_commit(&tmp, "app");
    std::fs::write(root.join("README.md"), "changed by someone\n").unwrap();
    let sub = root.join("src");
    std::fs::create_dir_all(&sub).unwrap();
    let host = start(&tmp);
    let sink = host.adapter_context().sink;
    started(&sink, "s1", &sub);

    let expected_root = toplevel(&root);
    let state = wait_for("branch and status", &host, |s| {
        session(s, "s1")
            .filter(|x| x.branch.as_deref() == Some("main") && x.git_status.is_some())
            .cloned()
    })
    .await;
    assert_eq!(
        state.repository_id.as_ref().map(|r| r.0.clone()),
        Some(expected_root.clone())
    );
    assert_eq!(
        state.worktree, None,
        "a main working tree is not a linked worktree"
    );
    let status = state.git_status.unwrap();
    assert_eq!((status.dirty, status.staged), (1, 0));
    assert_eq!(state.stats.commits, 0);

    // The agent writes a file: the status follows shortly after.
    let written = sub.join("main.rs");
    std::fs::write(&written, "fn main() {}\n").unwrap();
    sink.emit(event(
        "s1",
        EventKind::FileCreated(FileTouched {
            path: written.display().to_string(),
            tool_call_id: None,
        }),
    ));
    wait_for("the new file", &host, |s| {
        session(s, "s1")
            .and_then(|x| x.git_status.as_ref())
            .filter(|g| g.dirty == 2 && g.untracked == Some(1))
            .map(|_| ())
    })
    .await;

    // Only the file the agent's tool wrote is linked to it.
    let report = report(&host).await;
    assert!(report.available);
    let repo = report
        .repositories
        .iter()
        .find(|r| r.location.worktree_root == expected_root)
        .expect("the repository is listed");
    assert_eq!(repo.sessions, vec!["demo:s1".to_string()]);
    let linked: Vec<&str> = repo.file_sessions.iter().map(|f| f.path.as_str()).collect();
    // A new folder is listed by Git as the folder.
    assert_eq!(linked, ["src/"], "{:#?}", repo.file_sessions);
    assert_eq!(repo.file_sessions[0].sessions, vec!["demo:s1".to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commits_are_credited_only_with_evidence() {
    let tmp = TempDir::new("git-commits");
    let root = repo_with_commit(&tmp, "app");
    let host = start(&tmp);
    let sink = host.adapter_context().sink;
    started(&sink, "a", &root);
    started(&sink, "b", &root);
    for id in ["a", "b"] {
        wait_for("first status", &host, |s| {
            session(s, id).and_then(|x| x.git_status.clone())
        })
        .await;
    }

    // 1. Agent A runs `git commit`: credited to A only.
    std::fs::write(root.join("README.md"), "by agent a\n").unwrap();
    let sha = agent_commits(&sink, "a", &root, "cmd-1", "Change by agent A");
    wait_for("A's commit", &host, |s| {
        session(s, "a").filter(|x| x.stats.commits == 1).map(|_| ())
    })
    .await;
    let r = report(&host).await;
    let credited = &r.repositories[0].commit_sessions;
    assert_eq!(credited.len(), 1);
    assert_eq!(
        (credited[0].sha.as_str(), credited[0].session.as_str()),
        (sha.as_str(), "demo:a")
    );

    // 2. Someone commits outside any agent: shown, credited to nobody.
    std::fs::write(root.join("README.md"), "by a person\n").unwrap();
    git(&root, &["commit", "-q", "-am", "Change by a person"]);
    let person = git(&root, &["rev-parse", "HEAD"]);
    let r = wait_until_head(&host, &person).await;
    assert_eq!(r.repositories[0].commit_sessions.len(), 1);
    assert_eq!(
        r.repositories[0].snapshot.as_ref().unwrap().recent_commits[0].summary,
        "Change by a person"
    );

    // 3. Both agents run `git commit` at the same time: nobody is credited.
    sink.emit(event(
        "b",
        EventKind::CommandStarted(CommandStarted {
            command_id: Some("cmd-b".into()),
            command: "git add -A && git commit -m wip".into(),
            cwd: None,
        }),
    ));
    std::fs::write(root.join("README.md"), "who knows\n").unwrap();
    let both = agent_commits(&sink, "a", &root, "cmd-2", "Ambiguous change");
    let r = wait_until_head(&host, &both).await;
    assert_eq!(r.repositories[0].commit_sessions.len(), 1);

    let snapshot = host.snapshot();
    assert_eq!(session(&snapshot, "a").unwrap().stats.commits, 1);
    assert_eq!(session(&snapshot, "b").unwrap().stats.commits, 0);
}

async fn wait_until_head(host: &Host, sha: &str) -> RepositoriesReport {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let r = report(host).await;
        // Let the attribution events reach the state too.
        host.flush();
        let head = r
            .repositories
            .first()
            .and_then(|repo| repo.snapshot.as_ref())
            .and_then(|s| s.status.head.clone());
        if head.as_deref() == Some(sha) {
            tokio::time::sleep(Duration::from_millis(300)).await;
            host.flush();
            return report(host).await;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "HEAD never became {sha}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn linked_worktrees_and_project_folders() {
    let tmp = TempDir::new("git-worktree");
    let root = repo_with_commit(&tmp, "main");
    let linked = tmp.sub("feature-tree"); // empty: git accepts it
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
    let plain = tmp.sub("not-a-repo");
    let host = start(&tmp);
    let project = host
        .add_project(ao_store::NewProject {
            name: "App".into(),
            path: root.display().to_string(),
            ..Default::default()
        })
        .unwrap();
    let other = host
        .add_project(ao_store::NewProject {
            name: "Notes".into(),
            path: plain.display().to_string(),
            ..Default::default()
        })
        .unwrap();
    let sink = host.adapter_context().sink;
    started(&sink, "w", &linked);

    let state = wait_for("worktree session", &host, |s| {
        session(s, "w")
            .filter(|x| x.worktree.is_some() && x.branch.is_some())
            .cloned()
    })
    .await;
    assert_eq!(state.branch.as_deref(), Some("feature"));
    assert_eq!(state.worktree.as_deref(), Some(toplevel(&linked).as_str()));
    assert_eq!(
        state.repository_id.map(|r| r.0),
        Some(toplevel(&root)),
        "a worktree belongs to its main repository"
    );

    let r = report(&host).await;
    let app = r
        .projects
        .iter()
        .find(|p| p.project_id == project.id)
        .unwrap();
    assert_eq!(app.worktree_root.as_deref(), Some(toplevel(&root).as_str()));
    let notes = r
        .projects
        .iter()
        .find(|p| p.project_id == other.id)
        .unwrap();
    assert_eq!(notes.worktree_root, None);
    assert_eq!(notes.note.as_deref(), Some("Not in a Git repository"));
    let main = r
        .repositories
        .iter()
        .find(|x| x.location.worktree_root == toplevel(&root))
        .expect("the project's repository is read on request");
    assert_eq!(main.projects, vec![project.id.clone()]);
    assert_eq!(main.snapshot.as_ref().unwrap().worktrees.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_git_nothing_is_run() {
    let tmp = TempDir::new("git-missing");
    let root = repo_with_commit(&tmp, "app");
    let mut options = options(&tmp.sub("bin"));
    options.git_executable = Some(tmp.sub("bin").join("no-git"));
    let host = Host::start(AppPaths::at(tmp.sub("data")), options);
    let r = report(&host).await;
    assert!(!r.available);
    assert!(r.unavailable_reason.is_some());
    let sink = host.adapter_context().sink;
    started(&sink, "s", &root);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    host.flush();
    let snapshot = host.snapshot();
    let s = session(&snapshot, "s").unwrap();
    assert_eq!(s.branch, None);
    assert_eq!(s.git_status, None);
    assert!(report(&host).await.repositories.is_empty());
}
