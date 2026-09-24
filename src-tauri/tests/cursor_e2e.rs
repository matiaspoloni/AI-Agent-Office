//! End-to-end: the real host driving `fake-cursor`, a stand-in for the Cursor
//! CLI that speaks the Agent Client Protocol per the official schema plus the
//! Cursor details integrators report (see its header). No real account, no
//! network. These tests prove Agent Office follows ACP; they cannot prove the
//! real Cursor CLI behaves the same (PROVIDER_CAPABILITIES §5 lists what still
//! needs checking on a Windows 11 machine with Cursor installed).

mod common;

use agent_office_lib::host::{Host, HostOptions};
use agent_office_lib::paths::AppPaths;
use ao_core::event::{EventKind, PermissionOptionKind, SessionMode};
use ao_core::ids::{session_key, ProviderId, SessionId};
use ao_core::provider::{LaunchRequest, PermissionDecision, StopMode};
use ao_core::world::SessionStatus;
use ao_provider_claude::ClaudeOptions;
use ao_provider_codex::CodexOptions;
use ao_provider_cursor::CursorOptions;
use ao_testkit::bins::cargo_bin;
use common::{main_agent, wait_for, TempDir};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn cursor() -> ProviderId {
    ProviderId::new("cursor")
}

fn options(dir: &Path) -> HostOptions {
    HostOptions {
        relay: None,
        // Claude and Codex are not under test here: point them at nothing.
        claude: ClaudeOptions {
            executable: Some(dir.join("no-claude")),
            config_dir: Some(dir.join("no-claude-config")),
            discovery_interval: Duration::from_secs(3600),
        },
        codex: CodexOptions {
            executable: Some(dir.join("no-codex")),
            config_dir: Some(dir.join("no-codex-home")),
        },
        cursor: CursorOptions {
            executable: Some(cargo_bin("ao-testkit", "fake-cursor")),
        },
    }
}

fn launch(project: &Path, prompt: Option<&str>) -> LaunchRequest {
    LaunchRequest {
        project_id: None,
        cwd: project.display().to_string(),
        model: None,
        prompt: prompt.map(str::to_owned),
        name: Some("cursor e2e".into()),
        permission_mode: None,
    }
}

fn start(tmp: &TempDir) -> Arc<Host> {
    let data = tmp.sub("data");
    Host::start(AppPaths::at(data), options(&tmp.sub("elsewhere")))
}

async fn wait_message(host: &Host, sid: &str, text: &str) {
    wait_for(&format!("message `{text}`"), host, |s| {
        main_agent(s, sid)
            .filter(|a| a.last_message.as_deref() == Some(text))
            .map(|_| ())
    })
    .await;
}

async fn wait_ended(host: &Host, sid: &str) {
    wait_for("session end", host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid && x.status == SessionStatus::Ended)
            .map(|_| ())
    })
    .await;
}

async fn events(host: &Host, sid: &str) -> Vec<EventKind> {
    host.flush();
    host.recent_events(&session_key(&cursor(), &SessionId::new(sid)), 2_000)
        .expect("events")
        .into_iter()
        .map(|e| e.kind)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn managed_session_streams_asks_permission_and_stops() {
    let tmp = TempDir::new("cursor-managed");
    let project = tmp.sub("project");
    let host = start(&tmp);

    let mut request = launch(&project, Some("hello"));
    request.model = Some("GPT-5".into());
    request.permission_mode = Some("plan".into());
    let handle = host.launch(cursor(), request).await.expect("launch");
    assert_eq!(handle.mode, SessionMode::Managed);
    let sid = handle.session_id.0.clone();

    // 1. Streamed chunks become one message; usage and model are reported.
    wait_message(&host, &sid, "Hello from fake Cursor.").await;
    let session = wait_for("turn usage", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid)
            .filter(|x| x.usage.as_ref().is_some_and(|u| u.input_tokens.is_some()))
            .cloned()
    })
    .await;
    assert_eq!(session.model.as_deref(), Some("GPT-5"));
    assert_eq!(session.permission_mode.as_deref(), Some("plan"));
    let usage = session.usage.clone().expect("usage");
    assert_eq!(usage.context_tokens, Some(1200));
    assert_eq!(usage.context_window, Some(200000));
    assert_eq!(usage.input_tokens, Some(1200));
    assert!(usage.cost_usd.is_some(), "the agent reported a USD cost");

    // 2. A command waits for permission; the options carry Cursor-style ids.
    host.send_prompt(
        cursor(),
        SessionId::new(&sid),
        "run tests, ask permission".into(),
    )
    .await
    .expect("prompt");
    let ask = wait_for("permission", &host, |s| {
        main_agent(s, &sid).and_then(|a| a.pending_permission.clone())
    })
    .await;
    assert_eq!(ask.description, "Run: cargo test");
    assert!(ask.can_resolve);
    let kinds: Vec<_> = ask
        .options
        .iter()
        .map(|o| (o.id.as_str(), o.kind))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("allow-once", PermissionOptionKind::AllowOnce),
            ("allow-always", PermissionOptionKind::AllowAlways),
            ("reject-once", PermissionOptionKind::RejectOnce),
        ]
    );
    assert!(
        host.send_prompt(cursor(), SessionId::new(&sid), "again".into())
            .await
            .is_err(),
        "one turn at a time"
    );
    host.resolve_permission(
        cursor(),
        SessionId::new(&sid),
        ask.request_id.clone(),
        PermissionDecision::Approve { for_session: false },
    )
    .await
    .expect("approve");
    wait_message(&host, &sid, "Tests pass.").await;
    assert!(
        host.resolve_permission(
            cursor(),
            SessionId::new(&sid),
            ask.request_id,
            PermissionDecision::Approve { for_session: false },
        )
        .await
        .is_err(),
        "an answered request cannot be answered again"
    );

    // 3. Rejected this time: the tool fails, the turn goes on.
    host.send_prompt(cursor(), SessionId::new(&sid), "permission again".into())
        .await
        .expect("prompt");
    let ask = wait_for("second permission", &host, |s| {
        main_agent(s, &sid).and_then(|a| a.pending_permission.clone())
    })
    .await;
    host.resolve_permission(
        cursor(),
        SessionId::new(&sid),
        ask.request_id,
        PermissionDecision::Reject { message: None },
    )
    .await
    .expect("reject");
    wait_message(&host, &sid, "Okay, I did not run it.").await;

    // 4. File tools: read, approved edit, created file.
    host.send_prompt(cursor(), SessionId::new(&sid), "edit the code".into())
        .await
        .expect("prompt");
    let ask = wait_for("edit permission", &host, |s| {
        main_agent(s, &sid).and_then(|a| a.pending_permission.clone())
    })
    .await;
    assert_eq!(ask.description, "Edit: src/lib.rs");
    host.resolve_permission(
        cursor(),
        SessionId::new(&sid),
        ask.request_id,
        PermissionDecision::Approve { for_session: true },
    )
    .await
    .expect("approve edit");
    wait_message(&host, &sid, "Edited the code.").await;
    let snapshot = host.snapshot();
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id.0 == sid)
        .unwrap();
    assert_eq!(session.stats.commands, 2);
    assert_eq!(session.stats.failed_tools, 1, "the rejected command");
    assert_eq!(
        session.stats.files_changed.len(),
        2,
        "{:?}",
        session.stats.files_changed
    );

    let kinds = events(&host, &sid).await;
    assert!(kinds
        .iter()
        .any(|k| matches!(k, EventKind::FileRead(f) if f.path.ends_with("README.md"))));
    assert!(kinds
        .iter()
        .any(|k| matches!(k, EventKind::FileModified(f) if f.path.ends_with("lib.rs"))));
    assert!(kinds
        .iter()
        .any(|k| matches!(k, EventKind::FileCreated(f) if f.path.ends_with("new.rs"))));
    assert!(kinds.iter().any(|k| matches!(k, EventKind::CommandCompleted(c) if c.command.as_deref() == Some("cargo test"))));

    // 5. Graceful stop ends the process and the session.
    host.stop(cursor(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_ended(&host, &sid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_mid_turn_cancels_pending_permission() {
    let tmp = TempDir::new("cursor-cancel");
    let project = tmp.sub("project");
    let host = start(&tmp);

    let handle = host
        .launch(cursor(), launch(&project, Some("needs permission")))
        .await
        .expect("launch");
    let sid = handle.session_id.0.clone();
    wait_for("permission", &host, |s| {
        main_agent(s, &sid).and_then(|a| a.pending_permission.clone())
    })
    .await;

    host.stop(cursor(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_ended(&host, &sid).await;
    let kinds = events(&host, &sid).await;
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, EventKind::PermissionDenied(_))),
        "the pending request was answered `cancelled`"
    );
    assert!(
        kinds.iter().any(
            |k| matches!(k, EventKind::AgentIdle(n) if n.text.as_deref() == Some("Turn cancelled"))
        ),
        "the agent ended the turn as cancelled: {kinds:#?}"
    );
    let snapshot = host.snapshot();
    let agent = snapshot
        .agents
        .iter()
        .find(|a| a.session_id.0 == sid && a.is_main)
        .unwrap();
    assert!(agent.pending_permission.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn long_turn_is_cancelled_and_open_tools_closed() {
    let tmp = TempDir::new("cursor-wait");
    let project = tmp.sub("project");
    let host = start(&tmp);

    let handle = host
        .launch(cursor(), launch(&project, Some("wait for it")))
        .await
        .expect("launch");
    let sid = handle.session_id.0.clone();
    wait_for("running tool", &host, |s| {
        main_agent(s, &sid)
            .filter(|a| !a.running_tools.is_empty())
            .map(|_| ())
    })
    .await;
    host.stop(cursor(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_ended(&host, &sid).await;
    let kinds = events(&host, &sid).await;
    assert!(
        kinds.iter().any(|k| matches!(k, EventKind::ToolFailed(t)
            if t.detail.as_deref() == Some("Cancelled"))),
        "the open search was closed as cancelled: {kinds:#?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logs_in_when_the_agent_requires_it() {
    let tmp = TempDir::new("cursor-auth");
    let project = tmp.sub("project");
    std::fs::write(project.join(".fake-cursor-logged-out"), "").unwrap();
    let host = start(&tmp);

    let handle = host
        .launch(cursor(), launch(&project, Some("hello")))
        .await
        .expect("launch after authenticate");
    wait_message(&host, &handle.session_id.0, "Hello from fake Cursor.").await;
    host.stop(cursor(), handle.session_id.clone(), StopMode::Force)
        .await
        .expect("stop");
    wait_ended(&host, &handle.session_id.0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unsupported_requests_errors_and_models_are_reported() {
    let tmp = TempDir::new("cursor-errors");
    let project = tmp.sub("project");
    let host = start(&tmp);

    // An unknown model: the session starts with the default and says so.
    let mut request = launch(&project, Some("make a plan"));
    request.model = Some("no-such-model".into());
    request.permission_mode = Some("yolo".into());
    let handle = host.launch(cursor(), request).await.expect("launch");
    let sid = handle.session_id.0.clone();

    // `cursor/create_plan` is declined at once, so the turn does not stall.
    wait_message(&host, &sid, "The plan could not be shown; continuing.").await;
    let kinds = events(&host, &sid).await;
    let errors: Vec<&str> = kinds
        .iter()
        .filter_map(|k| match k {
            EventKind::AgentError(e) => Some(e.message.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        errors.iter().any(|m| m.contains("no-such-model")),
        "{errors:?}"
    );
    assert!(errors.iter().any(|m| m.contains("`yolo`")), "{errors:?}");
    assert!(
        errors.iter().any(|m| m.contains("cursor/create_plan")),
        "{errors:?}"
    );
    let snapshot = host.snapshot();
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id.0 == sid)
        .unwrap();
    assert_eq!(session.model.as_deref(), Some("Auto"));

    // A failed prompt is an error on the agent; the session stays usable.
    host.send_prompt(cursor(), SessionId::new(&sid), "fail please".into())
        .await
        .expect("prompt");
    wait_for("prompt error", &host, |s| {
        main_agent(s, &sid)
            .and_then(|a| a.last_error.clone())
            .filter(|e| e.contains("model is unavailable"))
    })
    .await;
    host.send_prompt(cursor(), SessionId::new(&sid), "hello".into())
        .await
        .expect("usable after an error");
    wait_message(&host, &sid, "Hello from fake Cursor.").await;

    host.stop(cursor(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_ended(&host, &sid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_folder_and_missing_cli_are_clear_errors() {
    let tmp = TempDir::new("cursor-missing");
    let host = start(&tmp);
    let err = host
        .launch(cursor(), launch(&tmp.sub("x").join("nope"), None))
        .await
        .unwrap_err();
    assert!(err.contains("does not exist"), "{err}");

    let mut options = options(&tmp.sub("elsewhere"));
    options.cursor.executable = Some(tmp.sub("bin").join("no-cursor-agent"));
    let host = Host::start(AppPaths::at(tmp.sub("data2")), options);
    let err = host
        .launch(cursor(), launch(&tmp.sub("project"), None))
        .await
        .unwrap_err();
    assert!(!err.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn older_model_selection_and_failed_login() {
    let tmp = TempDir::new("cursor-legacy");
    let project = tmp.sub("project");
    std::fs::write(project.join(".fake-cursor-legacy-models"), "").unwrap();
    let host = start(&tmp);

    // Only the older `models` state is offered: `session/set_model`, and a
    // base id finds the variant with options (`composer-2.5[fast=true]`).
    let mut request = launch(&project, None);
    request.model = Some("composer-2.5".into());
    let handle = host.launch(cursor(), request).await.expect("launch");
    let sid = handle.session_id.0.clone();
    let model = wait_for("session", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid)
            .map(|x| x.model.clone())
    })
    .await;
    assert_eq!(model.as_deref(), Some("Composer 2.5 Fast"));
    host.stop(cursor(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_ended(&host, &sid).await;

    // Logged out and the login fails: a clear error naming `agent login`.
    let locked = tmp.sub("locked");
    std::fs::write(locked.join(".fake-cursor-logged-out"), "").unwrap();
    std::fs::write(locked.join(".fake-cursor-login-fails"), "").unwrap();
    let err = host
        .launch(cursor(), launch(&locked, Some("hello")))
        .await
        .unwrap_err();
    assert!(err.contains("agent login"), "{err}");
}
