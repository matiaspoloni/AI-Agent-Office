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
    pub notifications: NotificationSupport,
}

/// Whether desktop notifications can be shown here.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NotificationSupport {
    /// Windows only.
    pub supported: bool,
    /// Run from a build folder: toasts appear as "Windows PowerShell".
    pub development_build: bool,
}

impl NotificationSupport {
    pub fn current() -> Self {
        let development_build = std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent().map(|dir| {
                    crate::toast::app_id_for(dir, crate::toast::IDENTIFIER)
                        == crate::toast::DEV_APP_ID
                })
            })
            .unwrap_or(false);
        Self {
            supported: cfg!(windows),
            development_build,
        }
    }
}

/// The file written by Diagnostics → "Export report".
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportedDiagnostics {
    pub format: String,
    pub note: String,
    pub report: DiagnosticsReport,
    pub processes: Vec<ManagedProcessInfo>,
    /// Warnings and errors from the newest application and provider logs.
    pub recent_warnings: Vec<crate::logs::LogLine>,
}

/// Replaces the user's home folder in every string with `replacement`
/// (case-insensitively on Windows, with either slash).
pub fn redact_home(value: &mut serde_json::Value, home: &str, replacement: &str) {
    let home = home.trim_end_matches(['/', '\\']);
    if home.len() < 3 {
        return;
    }
    let variants = [
        home.to_owned(),
        home.replace('\\', "/"),
        home.replace('/', "\\"),
    ];
    match value {
        serde_json::Value::String(text) => {
            for variant in &variants {
                *text = replace_all(text, variant, replacement);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_home(item, home, replacement);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, item) in map.iter_mut() {
                redact_home(item, home, replacement);
            }
        }
        _ => {}
    }
}

fn replace_all(text: &str, needle: &str, replacement: &str) -> String {
    if !cfg!(windows) {
        return text.replace(needle, replacement);
    }
    // Case-insensitive (ASCII) search: Windows paths ignore case.
    let (lower_text, lower_needle) = (text.to_ascii_lowercase(), needle.to_ascii_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (index, _) in lower_text.match_indices(&lower_needle) {
        out.push_str(&text[last..index]);
        out.push_str(replacement);
        last = index + needle.len();
    }
    out.push_str(&text[last..]);
    out
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
        notifications: NotificationSupport::current(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_folder_is_replaced_everywhere() {
        let mut value = serde_json::json!({
            "paths": [{"path": "/home/matia/.claude/settings.json"}],
            "note": "log at /home/matia/AppData and /home/matiax/other",
            "n": 3
        });
        redact_home(&mut value, "/home/matia", "~");
        assert_eq!(value["paths"][0]["path"], "~/.claude/settings.json");
        // A longer name that merely starts the same is still replaced as a
        // prefix; the rest of the name stays visible.
        assert_eq!(value["note"], "log at ~/AppData and ~x/other");
        assert_eq!(value["n"], 3);
    }

    #[test]
    fn windows_paths_with_either_slash() {
        let mut value = serde_json::json!(["C:\\Users\\Matia\\x", "C:/Users/Matia/y"]);
        redact_home(&mut value, "C:\\Users\\Matia", "%USERPROFILE%");
        assert_eq!(value[0], "%USERPROFILE%\\x");
        assert_eq!(value[1], "%USERPROFILE%/y");
    }
}
