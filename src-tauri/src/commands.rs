//! Tauri commands: the only surface the UI can call.

use crate::diagnostics::DiagnosticsReport;
use crate::host::Host;
use crate::prefs::Preferences;
use ao_core::batch::UiBatch;
use ao_core::event::AgentEvent;
use ao_core::ids::{PermissionRequestId, ProviderId, SessionId};
use ao_core::provider::{LaunchRequest, PermissionDecision, SessionHandle, StopMode};
use ao_core::registry::ProviderInfo;
use ao_core::world::WorldSnapshot;
use ao_store::{NewProject, Project};
use serde::Serialize;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::State;
use ts_rs::TS;

type HostState<'a> = State<'a, Arc<Host>>;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    pub platform: String,
    pub data_dir: String,
    pub log_dir: String,
    pub db_path: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InitialState {
    pub snapshot: WorldSnapshot,
    pub providers: Vec<ProviderInfo>,
}

#[tauri::command]
pub fn app_info(host: HostState<'_>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        data_dir: host.paths.data_dir.display().to_string(),
        log_dir: host.paths.log_dir.display().to_string(),
        db_path: host.paths.db_path.display().to_string(),
    }
}

/// Subscribes the UI to state batches and returns the full current state.
#[tauri::command]
pub fn subscribe(host: HostState<'_>, channel: Channel<UiBatch>) -> InitialState {
    host.set_ui_sink(Some(Box::new(move |batch| {
        if let Err(err) = channel.send(batch) {
            tracing::warn!(%err, "failed to deliver UI batch");
        }
    })));
    InitialState {
        snapshot: host.snapshot(),
        providers: host.providers(),
    }
}

#[tauri::command]
pub fn list_providers(host: HostState<'_>) -> Vec<ProviderInfo> {
    host.providers()
}

#[tauri::command]
pub async fn run_diagnostics(host: HostState<'_>) -> Result<DiagnosticsReport, String> {
    Ok(host.inner().diagnostics().await)
}

#[tauri::command]
pub fn list_projects(host: HostState<'_>) -> Result<Vec<Project>, String> {
    host.list_projects()
}

#[tauri::command]
pub fn add_project(host: HostState<'_>, project: NewProject) -> Result<Project, String> {
    host.add_project(project)
}

#[tauri::command]
pub fn update_project(host: HostState<'_>, project: Project) -> Result<(), String> {
    host.update_project(project)
}

#[tauri::command]
pub fn remove_project(host: HostState<'_>, id: String) -> Result<bool, String> {
    host.remove_project(&id)
}

#[tauri::command]
pub async fn launch_session(
    host: HostState<'_>,
    provider: String,
    request: LaunchRequest,
) -> Result<SessionHandle, String> {
    host.launch(ProviderId::new(provider), request).await
}

#[tauri::command]
pub fn start_demo_office(host: HostState<'_>) -> Vec<SessionHandle> {
    host.start_demo_office()
}

#[tauri::command]
pub async fn stop_session(
    host: HostState<'_>,
    provider: String,
    session_id: String,
    force: bool,
) -> Result<(), String> {
    let mode = if force {
        StopMode::Force
    } else {
        StopMode::Graceful
    };
    host.stop(ProviderId::new(provider), SessionId::new(session_id), mode)
        .await
}

#[tauri::command]
pub async fn send_prompt(
    host: HostState<'_>,
    provider: String,
    session_id: String,
    prompt: String,
) -> Result<(), String> {
    host.send_prompt(
        ProviderId::new(provider),
        SessionId::new(session_id),
        prompt,
    )
    .await
}

#[tauri::command]
pub async fn resolve_permission(
    host: HostState<'_>,
    provider: String,
    session_id: String,
    request_id: String,
    decision: PermissionDecision,
) -> Result<(), String> {
    host.resolve_permission(
        ProviderId::new(provider),
        SessionId::new(session_id),
        PermissionRequestId::new(request_id),
        decision,
    )
    .await
}

#[tauri::command]
pub fn recent_events(
    host: HostState<'_>,
    session_key: String,
    limit: Option<usize>,
) -> Result<Vec<AgentEvent>, String> {
    host.recent_events(&session_key, limit.unwrap_or(300))
}

#[tauri::command]
pub fn get_preferences(host: HostState<'_>) -> Preferences {
    host.preferences()
}

#[tauri::command]
pub fn set_preferences(
    host: HostState<'_>,
    preferences: Preferences,
) -> Result<Preferences, String> {
    host.set_preferences(preferences)
}
