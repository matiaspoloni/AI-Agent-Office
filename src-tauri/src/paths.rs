//! Where Agent Office keeps its data. Everything is per-user and local.

use serde::Serialize;
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_path: PathBuf,
}

impl AppPaths {
    /// `%LOCALAPPDATA%\AgentOffice` on Windows (see `ao_ipc::paths`);
    /// `AGENT_OFFICE_DATA_DIR` overrides it (tests, portable installs).
    /// The hook relay resolves the same folder to find the IPC token.
    pub fn resolve() -> Self {
        Self::at(ao_ipc::paths::resolve_data_dir())
    }

    pub fn at(data_dir: PathBuf) -> Self {
        Self {
            log_dir: data_dir.join("logs"),
            db_path: data_dir.join("agent-office.db"),
            data_dir,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PathEntry {
    pub label: String,
    pub path: String,
    pub exists: bool,
}

impl PathEntry {
    pub fn new(label: &str, path: PathBuf) -> Self {
        Self {
            label: label.to_owned(),
            exists: path.exists(),
            path: path.display().to_string(),
        }
    }
}

/// Paths shown in Diagnostics (read-only; nothing here is modified in Phase 1).
pub fn known_paths(paths: &AppPaths) -> Vec<PathEntry> {
    let mut out = vec![
        PathEntry::new("Agent Office data", paths.data_dir.clone()),
        PathEntry::new("Database", paths.db_path.clone()),
        PathEntry::new("Logs", paths.log_dir.clone()),
    ];
    if let Some(home) = ao_detect::home_dir() {
        out.push(PathEntry::new(
            "Claude Code user settings",
            home.join(".claude").join("settings.json"),
        ));
        out.push(PathEntry::new(
            "Codex hooks",
            home.join(".codex").join("hooks.json"),
        ));
        out.push(PathEntry::new(
            "Codex config",
            home.join(".codex").join("config.toml"),
        ));
        out.push(PathEntry::new(
            "Cursor user hooks",
            home.join(".cursor").join("hooks.json"),
        ));
    }
    out
}
