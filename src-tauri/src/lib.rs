//! Agent Office desktop app (Tauri 2).

pub mod commands;
pub mod diagnostics;
pub mod git;
pub mod hooks;
pub mod host;
pub mod logging;
pub mod notify;
pub mod paths;
pub mod prefs;
pub mod providers;
pub mod reveal;
pub mod terminal;
pub mod toast;

use host::{Host, HostOptions};
use paths::AppPaths;
use std::sync::Arc;
use tauri::Manager;

/// Headless self-check used by CI and `npm run smoke`: starts the runtime,
/// runs diagnostics, prints the report as JSON and exits.
pub fn smoke_test() -> i32 {
    let paths = AppPaths::resolve();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(async {
        let options = HostOptions::for_app(&paths);
        let host = Host::start(paths, options);
        let handles = host.start_demo_office();
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        host.flush_to_disk();
        let report = host.diagnostics().await;
        let json = serde_json::to_string_pretty(&report).unwrap_or_default();
        // Also written to a file: release builds use the Windows GUI subsystem
        // and have no console attached unless output is redirected.
        let report_path = host.paths.data_dir.join("smoke-report.json");
        let _ = std::fs::write(&report_path, &json);
        println!("{json}");
        let sessions = host.snapshot().sessions.len();
        let ok = report.backend.ok
            && report.database.ok
            && report.hooks.listening
            && sessions >= handles.len();
        eprintln!(
            "smoke test: {} (database ok: {}, hook bridge listening: {}, demo sessions: {sessions}/{})",
            if ok { "PASS" } else { "FAIL" },
            report.database.ok,
            report.hooks.listening,
            handles.len()
        );
        if ok {
            0
        } else {
            1
        }
    })
}

/// Shows notices as Windows notifications; a click opens the agent.
fn notice_sink(app: tauri::AppHandle) -> notify::NoticeSink {
    let identifier = app.config().identifier.clone();
    let app_id = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| toast::app_id_for(dir, &identifier)))
        .unwrap_or(identifier);
    Arc::new(move |notice: notify::Notice, show_when_focused: bool| {
        let (app, app_id) = (app.clone(), app_id.clone());
        // Off the caller's thread: asking for the window's focus waits for
        // the UI thread.
        std::thread::spawn(move || {
            let focused = app
                .get_webview_window("main")
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false);
            if focused && !show_when_focused {
                return;
            }
            let click = app.clone();
            let agent = notice.agent_key.clone();
            if let Err(err) =
                toast::show(&app_id, &notice, move || open_agent(&click, agent.clone()))
            {
                tracing::warn!(%err, "could not show a notification");
            }
        });
    })
}

/// Brings the window to the front and asks the UI to open `agent`.
fn open_agent(app: &tauri::AppHandle, agent: Option<String>) {
    use tauri::Emitter;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Some(agent) = agent {
        let _ = app.emit("open-agent", agent);
    }
}

pub fn run() {
    let paths = AppPaths::resolve();
    let _log_guards = logging::init(&paths.log_dir);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting Agent Office");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            // Start the runtime inside Tauri's Tokio runtime so background tasks keep running.
            let options = HostOptions::for_app(&paths);
            let host: Arc<Host> =
                tauri::async_runtime::block_on(async { Host::start(paths.clone(), options) });
            host.set_notice_sink(Some(notice_sink(app.handle().clone())));
            app.manage(host);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::subscribe,
            commands::list_providers,
            commands::run_diagnostics,
            commands::list_projects,
            commands::add_project,
            commands::update_project,
            commands::remove_project,
            commands::launch_session,
            commands::start_demo_office,
            commands::stop_session,
            commands::restart_session,
            commands::open_terminal,
            commands::list_processes,
            commands::list_repositories,
            commands::open_folder,
            commands::reveal_file,
            commands::test_notification,
            commands::send_prompt,
            commands::resolve_permission,
            commands::recent_events,
            commands::get_preferences,
            commands::set_preferences,
            commands::integration_action,
            commands::list_external_sessions,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Agent Office")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(host) = app.try_state::<Arc<Host>>() {
                    host.flush_to_disk();
                }
            }
        });
}
