//! Diagnostics report: providers, integrations, backend, database, Git, paths.

use crate::hooks::HookBridgeStatus;
use crate::host::Host;
use crate::paths::{known_paths, PathEntry};
use ao_core::provider::{InstallationInfo, IntegrationStatus};
use ao_core::registry::ProviderInfo;
use ao_core::time::now_ms;
use ao_store::StoreStats;
use serde::Serialize;
use std::sync::Arc;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderDiagnostics {
    pub provider: ProviderInfo,
    pub installation: InstallationInfo,
    pub integration: IntegrationStatus,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackendStatus {
    pub ok: bool,
    #[ts(type = "number")]
    pub uptime_ms: i64,
    #[ts(type = "number")]
    pub events_ingested: u64,
    #[ts(type = "number")]
    pub events_dropped: u64,
    #[ts(type = "number")]
    pub duplicates_dropped: u64,
    #[ts(type = "number")]
    pub events_rejected: u64,
    pub active_sessions: u32,
    pub ui_subscribed: bool,
    pub max_output_chars: u32,
    pub store_prompts: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DatabaseStatus {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stats: Option<StoreStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderErrorRecord {
    #[ts(type = "number")]
    pub at: i64,
    pub provider: String,
    pub component: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagnosticsReport {
    #[ts(type = "number")]
    pub generated_at: i64,
    pub app_version: String,
    pub platform: String,
    pub backend: BackendStatus,
    pub database: DatabaseStatus,
    pub git: InstallationInfo,
    pub providers: Vec<ProviderDiagnostics>,
    pub paths: Vec<PathEntry>,
    pub provider_errors: Vec<ProviderErrorRecord>,
    pub hooks: HookBridgeStatus,
}

/// A process Agent Office started (an agent CLI) and still tracks.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ManagedProcessInfo {
    pub pid: u32,
    pub label: String,
    /// The executable's file name.
    pub program: String,
    #[ts(type = "number")]
    pub started_at: i64,
    pub running: bool,
    /// "running", or how it ended ("finished", "exited with code 2", …).
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub last_output_at: Option<i64>,
    /// Processes alive in its tree (the agent plus what it started).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tree_processes: Option<u32>,
}

impl From<ao_process::ProcessSnapshot> for ManagedProcessInfo {
    fn from(p: ao_process::ProcessSnapshot) -> Self {
        let program = std::path::Path::new(&p.program)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(p.program);
        Self {
            pid: p.pid,
            label: p.label,
            program,
            started_at: p.started_at_ms,
            running: p.exit.is_none(),
            status: p
                .exit
                .as_ref()
                .map_or_else(|| "running".to_owned(), |e| e.describe()),
            exit_code: p.exit.as_ref().and_then(|e| e.code),
            last_output_at: p.last_output_ms,
            tree_processes: p.tree_processes,
        }
    }
}

/// Every agent process Agent Office started and still tracks, newest first.
pub fn managed_processes() -> Vec<ManagedProcessInfo> {
    let mut list: Vec<ManagedProcessInfo> = ao_process::managed_processes()
        .into_iter()
        .map(ManagedProcessInfo::from)
        .collect();
    list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    list
}

/// Where Git for Windows installs besides PATH.
pub const GIT_DIRS: &[&str] = if cfg!(windows) {
    &[
        "%ProgramFiles%\\Git\\cmd",
        "%LOCALAPPDATA%\\Programs\\Git\\cmd",
    ]
} else {
    &["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"]
};

pub async fn run(host: Arc<Host>) -> DiagnosticsReport {
    // Probe every provider concurrently; one slow or failing CLI cannot block the others.
    let mut probes = tokio::task::JoinSet::new();
    for (index, adapter) in host.registry.adapters().into_iter().enumerate() {
        let ctx = host.adapter_context();
        probes.spawn(async move {
            let installation = adapter.detect_installation().await;
            let integration = adapter.integration_status(&ctx).await;
            (
                index,
                ProviderDiagnostics {
                    provider: ProviderInfo {
                        descriptor: adapter.descriptor(),
                        capabilities: adapter.capabilities(),
                    },
                    installation,
                    integration,
                },
            )
        });
    }
    let git = ao_detect::detect(&["git".to_string()], GIT_DIRS, &["--version"]).await;

    let mut providers = Vec::new();
    while let Some(result) = probes.join_next().await {
        match result {
            Ok(entry) => providers.push(entry),
            Err(err) => tracing::error!(%err, "provider probe panicked"),
        }
    }
    providers.sort_by_key(|(index, _)| *index);

    let database = host.with_store(|store| match store.stats() {
        Ok(stats) => DatabaseStatus {
            ok: host.store_error().is_none() && stats.integrity_ok,
            stats: Some(stats),
            error: host.store_error(),
        },
        Err(err) => DatabaseStatus {
            ok: false,
            stats: None,
            error: Some(err.to_string()),
        },
    });

    let prefs = host.preferences();
    DiagnosticsReport {
        generated_at: now_ms(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        backend: BackendStatus {
            ok: true,
            uptime_ms: now_ms() - host.started_at,
            events_ingested: host.ingested(),
            events_dropped: host.dropped(),
            duplicates_dropped: host.duplicates(),
            events_rejected: host.rejected(),
            active_sessions: host.active_sessions() as u32,
            ui_subscribed: host.ui_subscribed(),
            max_output_chars: prefs.max_output_chars,
            store_prompts: prefs.store_prompts,
        },
        database,
        git,
        providers: providers.into_iter().map(|(_, p)| p).collect(),
        paths: known_paths(&host.paths),
        provider_errors: host.provider_errors(),
        hooks: host.hook_status(),
    }
}
