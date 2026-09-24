//! Shared data-directory resolution (used by the app and by the relay, which
//! must agree on where the IPC token lives).

use std::path::PathBuf;

pub const DATA_DIR_ENV: &str = "AGENT_OFFICE_DATA_DIR";

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Platform default: `%LOCALAPPDATA%\AgentOffice` on Windows,
/// `~/Library/Application Support/AgentOffice` on macOS,
/// `$XDG_DATA_HOME/AgentOffice` or `~/.local/share/AgentOffice` elsewhere.
pub fn default_data_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        None
    }
    .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
    .or_else(|| {
        home_dir().map(|h| {
            if cfg!(target_os = "macos") {
                h.join("Library").join("Application Support")
            } else {
                h.join(".local").join("share")
            }
        })
    })
    .unwrap_or_else(std::env::temp_dir);
    base.join("AgentOffice")
}

/// `AGENT_OFFICE_DATA_DIR` when set, otherwise [`default_data_dir`].
pub fn resolve_data_dir() -> PathBuf {
    std::env::var_os(DATA_DIR_ENV)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(default_data_dir)
}
