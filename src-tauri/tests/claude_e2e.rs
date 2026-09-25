//! End-to-end: the real host, IPC server and hook relay, driven by
//! `fake-claude` (a stand-in for the `claude` CLI). No real account, no
//! network, and the user's own Claude configuration is never touched.

mod common;

use agent_office_lib::host::{Host, HostOptions, IntegrationAction};
use agent_office_lib::paths::AppPaths;
use agent_office_lib::prefs::Preferences;
use ao_core::event::SessionMode;
use ao_core::ids::{ProviderId, SessionId};
use ao_core::provider::{
    IntegrationState, LaunchRequest, PermissionDecision, RelayCommand, StopMode,
};
use ao_core::world::SessionStatus;
use ao_provider_claude::ClaudeOptions;
use ao_provider_codex::CodexOptions;
use ao_provider_cursor::CursorOptions;
use ao_testkit::bins::cargo_bin;
use common::{main_agent, wait_for, wait_listening, TempDir};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn claude() -> ProviderId {
    ProviderId::new("claude")
}

fn options(data_dir: &Path, config_dir: &Path) -> HostOptions {
    HostOptions {
        relay: Some(RelayCommand {
            program: cargo_bin("ao-hook-relay", "agent-office-hook"),
            prefix_args: Vec::new(),
            data_dir: Some(data_dir.to_path_buf()),
        }),
        claude: ClaudeOptions {
            executable: Some(cargo_bin("ao-testkit", "fake-claude")),
            config_dir: Some(config_dir.to_path_buf()),
            discovery_interval: Duration::from_secs(3600),
        },
        // Codex is not under test here: point it at nothing.
        codex: CodexOptions {
            executable: Some(config_dir.join("no-codex")),
            config_dir: Some(config_dir.join("no-codex-home")),
        },
        cursor: CursorOptions {
            executable: Some(config_dir.join("no-cursor")),
        },
        git_executable: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn managed_session_runs_prompts_permissions_and_stops() {
    let tmp = TempDir::new("managed");
    let (data, config, project) = (
        tmp.sub("data"),
        tmp.sub("claude-config"),
        tmp.sub("project"),
    );
    let host: Arc<Host> = Host::start(AppPaths::at(data.clone()), options(&data, &config));
    wait_listening(&host).await;

    let handle = host
        .launch(
            claude(),
            LaunchRequest {
                project_id: None,
                cwd: project.display().to_string(),
                model: Some("claude-fake-2".into()),
                prompt: Some("please ask for permission".into()),
                name: Some("e2e".into()),
                resume_session_id: None,
                permission_mode: None,
            },
        )
        .await
        .expect("launch");
    assert_eq!(handle.mode, SessionMode::Managed);
    let sid = handle.session_id.0.clone();

    // The Bash call waits for an answer in Agent Office.
    let request = wait_for("permission request", &host, |s| {
        main_agent(s, &sid).and_then(|a| a.pending_permission.clone())
    })
    .await;
    assert!(request.can_resolve, "managed permissions are answerable");
    assert_eq!(request.tool_name.as_deref(), Some("Bash"));
    host.resolve_permission(
        claude(),
        SessionId::new(&sid),
        request.request_id.clone(),
        PermissionDecision::Approve { for_session: false },
    )
    .await
    .expect("approve");

    wait_for("approved command result", &host, |s| {
        main_agent(s, &sid)
            .filter(|a| a.last_message.as_deref() == Some("Command ran"))
            .map(|_| ())
    })
    .await;

    // Answered requests cannot be answered twice.
    assert!(host
        .resolve_permission(
            claude(),
            SessionId::new(&sid),
            request.request_id,
            PermissionDecision::Approve { for_session: false }
        )
        .await
        .is_err());

    // Second turn: rejection is delivered to Claude as a deny decision.
    host.send_prompt(
        claude(),
        SessionId::new(&sid),
        "another permission please".into(),
    )
    .await
    .expect("prompt");
    let second = wait_for("second permission request", &host, |s| {
        main_agent(s, &sid).and_then(|a| a.pending_permission.clone())
    })
    .await;
    host.resolve_permission(
        claude(),
        SessionId::new(&sid),
        second.request_id,
        PermissionDecision::Reject {
            message: Some("not now".into()),
        },
    )
    .await
    .expect("reject");
    wait_for("denied result", &host, |s| {
        main_agent(s, &sid)
            .filter(|a| a.last_message.as_deref() == Some("Permission denied: not now"))
            .map(|_| ())
    })
    .await;

    let snapshot = host.snapshot();
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id.0 == sid)
        .unwrap();
    assert_eq!(session.mode, SessionMode::Managed);
    assert_eq!(session.model.as_deref(), Some("claude-fake-2"));
    let usage = session
        .usage
        .clone()
        .expect("usage from stream-json result");
    assert_eq!(usage.input_tokens, Some(2000));
    assert!(
        usage.cost_is_estimate,
        "Claude's cost is a client-side estimate"
    );
    assert!(session.stats.tool_calls >= 2, "{:?}", session.stats);

    // Hooks arrived through the relay and were counted for Diagnostics.
    let hooks = host.hook_status();
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "Stop",
    ] {
        assert!(
            hooks
                .events
                .iter()
                .any(|e| e.provider == "claude" && e.event == event),
            "{event} missing from {:?}",
            hooks.events
        );
    }

    host.stop(claude(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_for("session end", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid && x.status == SessionStatus::Ended)
            .map(|_| ())
    })
    .await;
    // The per-session settings file is cleaned up.
    assert!(std::fs::read_dir(data.join("sessions"))
        .unwrap()
        .next()
        .is_none());
    // The user's Claude configuration was never written.
    assert!(!config.join("settings.json").exists());
}

fn run_external(config: &Path, cwd: &Path, session: &str) -> std::thread::JoinHandle<Value> {
    let (exe, config, cwd, session) = (
        cargo_bin("ao-testkit", "fake-claude"),
        config.to_path_buf(),
        cwd.to_path_buf(),
        session.to_owned(),
    );
    std::thread::spawn(move || {
        let output = std::process::Command::new(exe)
            .args(["--simulate-external", &session])
            .env("CLAUDE_CONFIG_DIR", &config)
            .current_dir(&cwd)
            .output()
            .expect("run fake-claude");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout
            .lines()
            .find(|l| l.contains("fakeDecision"))
            .expect("decision line");
        serde_json::from_str(line).unwrap()
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn global_hooks_observe_and_answer_external_sessions() {
    let tmp = TempDir::new("global");
    let (data, config, project) = (
        tmp.sub("data"),
        tmp.sub("claude-config"),
        tmp.sub("project"),
    );
    let user_settings = json!({
        "model": "opus",
        "enabledPlugins": {"formatter@market": true},
        "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "notify-send done"}]}]}
    });
    let settings_path = config.join("settings.json");
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&user_settings).unwrap(),
    )
    .unwrap();

    let host: Arc<Host> = Host::start(AppPaths::at(data.clone()), options(&data, &config));
    wait_listening(&host).await;
    host.set_preferences(Preferences {
        answer_permissions_from_app: true,
        permission_timeout_secs: 60,
        ..Preferences::default()
    })
    .await
    .unwrap();

    let status = host
        .integration_action(claude(), IntegrationAction::Install)
        .await
        .unwrap();
    assert_eq!(status.state, IntegrationState::Installed, "{status:?}");
    let installed: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert_eq!(installed["model"], "opus");
    assert_eq!(installed["enabledPlugins"], user_settings["enabledPlugins"]);
    assert_eq!(
        installed["hooks"]["Stop"].as_array().unwrap().len(),
        2,
        "user hook kept, ours added"
    );

    // An external session asks for permission; Agent Office answers it.
    let run = run_external(&config, &project, "ext-1");
    let request = wait_for("external permission request", &host, |s| {
        main_agent(s, "ext-1").and_then(|a| a.pending_permission.clone())
    })
    .await;
    assert!(request.can_resolve);
    let snapshot = host.snapshot();
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id.0 == "ext-1")
        .unwrap();
    assert_eq!(session.mode, SessionMode::External);
    // External sessions cannot receive prompts (no such interface in Claude Code).
    assert!(host
        .send_prompt(claude(), SessionId::new("ext-1"), "hi".into())
        .await
        .is_err());
    host.resolve_permission(
        claude(),
        SessionId::new("ext-1"),
        request.request_id,
        PermissionDecision::Approve { for_session: false },
    )
    .await
    .unwrap();
    let decision = tokio::task::spawn_blocking(move || run.join().unwrap())
        .await
        .unwrap();
    assert_eq!(decision["fakeDecision"], "allow");
    wait_for("external session end", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == "ext-1" && x.status == SessionStatus::Ended)
            .map(|_| ())
    })
    .await;

    // Observe-only mode: the preference change re-writes the hook, and
    // Claude keeps its own permission prompt.
    host.set_preferences(Preferences::default()).await.unwrap();
    let status = host
        .integration_action(claude(), IntegrationAction::Status)
        .await
        .unwrap();
    assert_eq!(
        status.state,
        IntegrationState::Installed,
        "auto-repaired: {status:?}"
    );
    let run = run_external(&config, &project, "ext-2");
    let decision = tokio::task::spawn_blocking(move || run.join().unwrap())
        .await
        .unwrap();
    assert_eq!(decision["fakeDecision"], Value::Null);
    let request = wait_for("observed permission request", &host, |s| {
        main_agent(s, "ext-2").and_then(|a| {
            // Either still shown as pending (not answerable) or already moved on.
            a.pending_permission.clone().map(Some).or(Some(None))
        })
    })
    .await;
    if let Some(request) = request {
        assert!(
            !request.can_resolve,
            "observe-only requests are not answerable"
        );
    }

    // Uninstall leaves exactly the user's configuration.
    let status = host
        .integration_action(claude(), IntegrationAction::Uninstall)
        .await
        .unwrap();
    assert_eq!(status.state, IntegrationState::NotInstalled);
    let restored: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert_eq!(restored, user_settings);
    let backups = std::fs::read_dir(&config)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("settings.json.agent-office-backup-")
        })
        .count();
    assert!(backups >= 1, "a backup is taken before every change");
}

async fn wait_status(host: &Host, sid: &str, status: SessionStatus, restarts: u32) {
    wait_for("session status", host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid && x.status == status && x.restarts == restarts)
            .map(|_| ())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restart_continues_the_same_conversation() {
    let tmp = TempDir::new("restart");
    let (data, config, project) = (
        tmp.sub("data"),
        tmp.sub("claude-config"),
        tmp.sub("project"),
    );
    let host: Arc<Host> = Host::start(AppPaths::at(data.clone()), options(&data, &config));
    wait_listening(&host).await;
    let handle = host
        .launch(
            claude(),
            LaunchRequest {
                cwd: project.display().to_string(),
                model: Some("claude-fake-2".into()),
                prompt: Some("hello".into()),
                ..Default::default()
            },
        )
        .await
        .expect("launch");
    let sid = handle.session_id.0.clone();
    wait_for("first answer", &host, |s| {
        main_agent(s, &sid)
            .filter(|a| a.last_message.as_deref() == Some("Read README.md"))
            .map(|_| ())
    })
    .await;

    // Stopped, then restarted: same session id, counted as a restart.
    host.stop(claude(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_status(&host, &sid, SessionStatus::Ended, 0).await;
    let again = host
        .restart(claude(), SessionId::new(&sid))
        .await
        .expect("restart");
    assert_eq!(again.session_id.0, sid);
    wait_status(&host, &sid, SessionStatus::Active, 1).await;
    let session = host
        .snapshot()
        .sessions
        .into_iter()
        .find(|s| s.session_id.0 == sid)
        .unwrap();
    assert_eq!(
        session.model.as_deref(),
        Some("claude-fake-2"),
        "the model is kept"
    );
    host.send_prompt(claude(), SessionId::new(&sid), "hello again".into())
        .await
        .expect("prompt after restart");

    // Restart while running: stopped first, then continued.
    host.restart(claude(), SessionId::new(&sid))
        .await
        .expect("restart a running session");
    wait_status(&host, &sid, SessionStatus::Active, 2).await;

    // The conversation is gone from Claude's store: Claude's own error shows.
    host.stop(claude(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_status(&host, &sid, SessionStatus::Ended, 2).await;
    std::fs::remove_file(config.join("fake-sessions").join(&sid)).unwrap();
    host.restart(claude(), SessionId::new(&sid))
        .await
        .expect("the process starts");
    wait_for("resume error", &host, |s| {
        main_agent(s, &sid)
            .and_then(|a| a.last_error.clone())
            .filter(|e| e.contains("No conversation found with session ID"))
    })
    .await;
    wait_status(&host, &sid, SessionStatus::Ended, 3).await;
}
