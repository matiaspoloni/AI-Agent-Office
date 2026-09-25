//! End-to-end: the real host, IPC server and hook relay, driven by
//! `fake-codex` (a stand-in for the `codex` CLI that speaks the app-server
//! protocol recorded from codex-cli 0.156.1 and runs hooks through the shell
//! like Codex does). No real account, no network; the user's own Codex
//! folder is never touched (every test uses a temporary CODEX_HOME).

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

fn codex() -> ProviderId {
    ProviderId::new("codex")
}

fn options(data_dir: &Path, codex_home: &Path) -> HostOptions {
    HostOptions {
        relay: Some(RelayCommand {
            program: cargo_bin("ao-hook-relay", "agent-office-hook"),
            prefix_args: Vec::new(),
            data_dir: Some(data_dir.to_path_buf()),
        }),
        // Claude is not under test here: point it at nothing.
        claude: ClaudeOptions {
            executable: Some(codex_home.join("no-claude")),
            config_dir: Some(codex_home.join("no-claude-config")),
            discovery_interval: Duration::from_secs(3600),
        },
        codex: CodexOptions {
            executable: Some(cargo_bin("ao-testkit", "fake-codex")),
            config_dir: Some(codex_home.to_path_buf()),
        },
        cursor: CursorOptions {
            executable: Some(codex_home.join("no-cursor")),
        },
    }
}

fn launch(project: &Path, prompt: &str) -> LaunchRequest {
    LaunchRequest {
        project_id: None,
        cwd: project.display().to_string(),
        model: Some("gpt-fake-2".into()),
        prompt: Some(prompt.into()),
        name: Some("codex e2e".into()),
        resume_session_id: None,
        permission_mode: Some("untrusted".into()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn managed_session_approvals_subagent_usage_and_stop() {
    let tmp = TempDir::new("codex-managed");
    let (data, home, project) = (tmp.sub("data"), tmp.sub("codex-home"), tmp.sub("project"));
    let host: Arc<Host> = Host::start(AppPaths::at(data.clone()), options(&data, &home));
    wait_listening(&host).await;

    let handle = host
        .launch(
            codex(),
            launch(&project, "need permission to edit, then a subagent"),
        )
        .await
        .expect("launch");
    assert_eq!(handle.mode, SessionMode::Managed);
    let sid = handle.session_id.0.clone();

    // 1. The command waits for approval → approve it.
    let command = wait_for("command approval", &host, |s| {
        main_agent(s, &sid).and_then(|a| a.pending_permission.clone())
    })
    .await;
    assert_eq!(command.tool_name.as_deref(), Some("commandExecution"));
    assert_eq!(command.description, "Run: rm -rf build");
    assert!(command.can_resolve);
    host.resolve_permission(
        codex(),
        SessionId::new(&sid),
        command.request_id.clone(),
        PermissionDecision::Approve { for_session: false },
    )
    .await
    .expect("approve");

    // 2. The file change waits for its own approval → reject it.
    let edit = wait_for("file change approval", &host, |s| {
        main_agent(s, &sid)
            .and_then(|a| a.pending_permission.clone())
            .filter(|p| p.request_id != command.request_id)
    })
    .await;
    assert_eq!(edit.description, "Apply changes: add hello.txt");
    host.resolve_permission(
        codex(),
        SessionId::new(&sid),
        edit.request_id.clone(),
        PermissionDecision::Reject { message: None },
    )
    .await
    .expect("reject");
    assert!(
        host.resolve_permission(
            codex(),
            SessionId::new(&sid),
            edit.request_id,
            PermissionDecision::Approve { for_session: false }
        )
        .await
        .is_err(),
        "an answered approval cannot be answered again"
    );

    // 3. The turn finishes; the subagent appeared as its own character.
    wait_for("turn result", &host, |s| {
        main_agent(s, &sid)
            .filter(|a| a.last_message.as_deref() == Some("Edit declined"))
            .map(|_| ())
    })
    .await;
    let snapshot = host.snapshot();
    let child = snapshot
        .agents
        .iter()
        .find(|a| a.session_id.0 == sid && !a.is_main)
        .expect("subagent");
    assert_eq!(child.last_message.as_deref(), Some("3 files"));
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id.0 == sid)
        .unwrap();
    assert_eq!(session.model.as_deref(), Some("gpt-fake-2"));
    assert_eq!(session.permission_mode.as_deref(), Some("untrusted"));
    let usage = session.usage.clone().expect("token usage");
    assert!(usage.input_tokens.unwrap_or(0) >= 1000);
    assert_eq!(usage.cost_usd, None, "Codex reports no cost");

    // 4. A follow-up prompt.
    host.send_prompt(codex(), SessionId::new(&sid), "hello again".into())
        .await
        .expect("prompt");
    wait_for("second turn", &host, |s| {
        main_agent(s, &sid)
            .filter(|a| a.last_message.as_deref() == Some("Hello from fake codex"))
            .map(|_| ())
    })
    .await;

    // 5. Graceful stop ends the session.
    host.stop(codex(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_for("session end", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid && x.status == SessionStatus::Ended)
            .map(|_| ())
    })
    .await;
    assert!(
        !home.join("hooks.json").exists(),
        "the Codex folder was never written"
    );
}

fn run_external(home: &Path, cwd: &Path, session: &str) -> std::thread::JoinHandle<Value> {
    let (exe, home, cwd, session) = (
        cargo_bin("ao-testkit", "fake-codex"),
        home.to_path_buf(),
        cwd.to_path_buf(),
        session.to_owned(),
    );
    std::thread::spawn(move || {
        let output = std::process::Command::new(exe)
            .args(["--simulate-external", &session])
            .env("CODEX_HOME", &home)
            .current_dir(&cwd)
            .output()
            .expect("run fake-codex");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout
            .lines()
            .find(|l| l.contains("fakeDecision"))
            .expect("decision line");
        serde_json::from_str(line).unwrap()
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hooks_need_trust_then_observe_and_answer_external_sessions() {
    let tmp = TempDir::new("codex-global");
    let (data, home, project) = (tmp.sub("data"), tmp.sub("codex-home"), tmp.sub("project"));
    let user_hooks = json!({
        "description": "my hooks",
        "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "exit 0", "timeout": 5}]}]}
    });
    let hooks_path = home.join("hooks.json");
    std::fs::write(
        &hooks_path,
        serde_json::to_string_pretty(&user_hooks).unwrap(),
    )
    .unwrap();

    let host: Arc<Host> = Host::start(AppPaths::at(data.clone()), options(&data, &home));
    wait_listening(&host).await;
    host.set_preferences(Preferences {
        answer_permissions_from_app: true,
        permission_timeout_secs: 60,
        ..Preferences::default()
    })
    .await
    .unwrap();

    // Installed, but Codex does not run untrusted hooks: the user must act.
    let status = host
        .integration_action(codex(), IntegrationAction::Install)
        .await
        .unwrap();
    assert_eq!(
        status.state,
        IntegrationState::NeedsUserAction,
        "{status:?}"
    );
    assert!(status.details[0].contains("/hooks"), "{status:?}");
    let installed: Value =
        serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
    assert_eq!(installed["description"], "my hooks");
    assert_eq!(
        installed["hooks"]["Stop"][0], user_hooks["hooks"]["Stop"][0],
        "user hook keeps its position"
    );

    // Untrusted: an external session is invisible.
    let run = run_external(&home, &project, "ext-cx-0");
    let decision = tokio::task::spawn_blocking(move || run.join().unwrap())
        .await
        .unwrap();
    assert_eq!(decision["fakeDecision"], Value::Null);

    // The user trusts the hooks in Codex (/hooks).
    std::fs::write(home.join("fake-trust-all"), "").unwrap();
    let status = host
        .integration_action(codex(), IntegrationAction::Status)
        .await
        .unwrap();
    assert_eq!(status.state, IntegrationState::Installed, "{status:?}");

    // An external session asks for permission; Agent Office answers it.
    let run = run_external(&home, &project, "ext-cx-1");
    let request = wait_for("external permission request", &host, |s| {
        main_agent(s, "ext-cx-1").and_then(|a| a.pending_permission.clone())
    })
    .await;
    assert_eq!(request.description, "Run: cargo test");
    assert!(host
        .send_prompt(codex(), SessionId::new("ext-cx-1"), "hi".into())
        .await
        .is_err());
    host.resolve_permission(
        codex(),
        SessionId::new("ext-cx-1"),
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
            .find(|x| x.session_id.0 == "ext-cx-1" && x.status == SessionStatus::Ended)
            .map(|_| ())
    })
    .await;
    let snapshot = host.snapshot();
    let external = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id.0 == "ext-cx-1")
        .unwrap();
    assert_eq!(external.mode, SessionMode::External);
    assert!(snapshot
        .sessions
        .iter()
        .all(|s| s.session_id.0 != "ext-cx-0"));

    // A managed session also runs the (trusted) user hooks; they must not
    // turn it into a second, external session.
    let handle = host
        .launch(codex(), launch(&project, "hello"))
        .await
        .expect("launch");
    let sid = handle.session_id.0.clone();
    wait_for("managed reply", &host, |s| {
        main_agent(s, &sid)
            .filter(|a| a.last_message.as_deref() == Some("Hello from fake codex"))
            .map(|_| ())
    })
    .await;
    host.stop(codex(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .unwrap();
    wait_for("managed end", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid && x.status == SessionStatus::Ended)
            .map(|_| ())
    })
    .await;
    let hooks = host.hook_status();
    assert!(hooks
        .events
        .iter()
        .any(|e| e.provider == "codex" && e.event == "UserPromptSubmit"));
    let snapshot = host.snapshot();
    let managed = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id.0 == sid)
        .unwrap();
    assert_eq!(managed.mode, SessionMode::Managed);
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .filter(|s| s.session_id.0 == sid)
            .count(),
        1
    );

    // Uninstall restores the user's file exactly.
    let status = host
        .integration_action(codex(), IntegrationAction::Uninstall)
        .await
        .unwrap();
    assert_eq!(status.state, IntegrationState::NotInstalled);
    let restored: Value =
        serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
    assert_eq!(restored, user_hooks);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restart_resumes_the_thread() {
    let tmp = TempDir::new("codex-restart");
    let (data, home, project) = (tmp.sub("data"), tmp.sub("codex-home"), tmp.sub("project"));
    let host: Arc<Host> = Host::start(AppPaths::at(data.clone()), options(&data, &home));
    wait_listening(&host).await;
    let handle = host
        .launch(codex(), launch(&project, "hello"))
        .await
        .expect("launch");
    let sid = handle.session_id.0.clone();
    let session = |host: &Host| {
        host.snapshot()
            .sessions
            .into_iter()
            .find(|s| s.session_id.0 == sid)
            .unwrap()
    };
    wait_for("first answer", &host, |s| {
        main_agent(s, &sid)
            .filter(|a| a.last_message.as_deref() == Some("Hello from fake codex"))
            .map(|_| ())
    })
    .await;
    host.stop(codex(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_for("ended", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid && x.status == SessionStatus::Ended)
            .map(|_| ())
    })
    .await;

    let again = host
        .restart(codex(), SessionId::new(&sid))
        .await
        .expect("restart");
    assert_eq!(again.session_id.0, sid, "thread/resume keeps the thread id");
    let restarted = wait_for("restarted", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid && x.status == SessionStatus::Active)
            .cloned()
    })
    .await;
    assert_eq!(restarted.restarts, 1);
    assert_eq!(restarted.model.as_deref(), Some("gpt-fake-2"));
    assert_eq!(restarted.permission_mode.as_deref(), Some("untrusted"));
    let prompts = restarted.stats.prompts;
    host.send_prompt(codex(), SessionId::new(&sid), "hello again".into())
        .await
        .expect("prompt after restart");
    wait_for("second answer", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid && x.stats.prompts > prompts)
            .map(|_| ())
    })
    .await;

    // A thread Codex no longer has: its error, and the session stays ended.
    host.stop(codex(), SessionId::new(&sid), StopMode::Graceful)
        .await
        .expect("stop");
    wait_for("ended again", &host, |s| {
        s.sessions
            .iter()
            .find(|x| x.session_id.0 == sid && x.status == SessionStatus::Ended)
            .map(|_| ())
    })
    .await;
    std::fs::remove_file(home.join("fake-threads").join(&sid)).unwrap();
    let err = host
        .restart(codex(), SessionId::new(&sid))
        .await
        .unwrap_err();
    assert!(err.contains("no rollout found"), "{err}");
    assert_eq!(session(&host).status, SessionStatus::Ended);
}
