//! Local IPC between `agent-office hook …` (the hook relay, spawned by agent
//! CLIs) and the running Agent Office app.
//!
//! * Transport: a Windows named pipe (`\\.\pipe\agent-office-v1-<hash>`,
//!   remote clients rejected) or a Unix domain socket (mode 0600).
//!   No TCP port is ever opened.
//! * Framing: 4-byte little-endian length + JSON.
//! * Authentication: every request carries the per-user token stored in
//!   `<data dir>/ipc.token`. Requests with a wrong token are dropped.
//! * The relay is **fail-open**: any IPC failure means "no opinion", so the
//!   agent behaves exactly as if Agent Office were not installed.

pub mod client;
pub mod frame;
pub mod paths;
#[cfg(feature = "server")]
pub mod server;
pub mod token;

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
/// Upper bound for one frame (hook payloads can include file contents).
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// Where a hook invocation came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookOrigin {
    /// A user-level hook installed by Agent Office (external sessions).
    Global,
    /// A per-session hook injected into a session Agent Office launched.
    Managed,
}

impl HookOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            HookOrigin::Global => "global",
            HookOrigin::Managed => "managed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRequest {
    pub v: u32,
    pub token: String,
    /// Provider id, e.g. `claude`.
    pub provider: String,
    pub origin: HookOrigin,
    /// When the relay started (ms since epoch) — the event's timestamp.
    pub received_at_ms: i64,
    pub relay_pid: u32,
    /// The hook's stdin JSON (or `{"raw": "..."}` when it was not JSON).
    pub payload: serde_json::Value,
    #[serde(default)]
    pub payload_truncated: bool,
}

/// What the relay prints and returns to the agent CLI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(default)]
    pub exit_code: i32,
}

/// Local endpoint name derived from the data directory, so a test instance
/// with its own data folder never collides with the real app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    #[cfg(windows)]
    pub pipe_name: String,
    #[cfg(unix)]
    pub socket_path: std::path::PathBuf,
}

impl Endpoint {
    pub fn for_data_dir(data_dir: &std::path::Path) -> Self {
        let hash = fnv1a64(data_dir.to_string_lossy().to_lowercase().as_bytes());
        #[cfg(windows)]
        {
            Self {
                pipe_name: format!(r"\\.\pipe\agent-office-v{PROTOCOL_VERSION}-{hash:016x}"),
            }
        }
        #[cfg(unix)]
        {
            let base = std::env::var_os("XDG_RUNTIME_DIR")
                .map(std::path::PathBuf::from)
                .filter(|p| p.is_dir())
                .unwrap_or_else(std::env::temp_dir);
            Self {
                socket_path: base
                    .join(format!("agent-office-v{PROTOCOL_VERSION}-{hash:016x}.sock")),
            }
        }
    }

    pub fn display(&self) -> String {
        #[cfg(windows)]
        {
            self.pipe_name.clone()
        }
        #[cfg(unix)]
        {
            self.socket_path.display().to_string()
        }
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Constant-time comparison for the token check.
pub fn tokens_match(a: &str, b: &str) -> bool {
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_depends_on_data_dir() {
        let a = Endpoint::for_data_dir(std::path::Path::new("/data/a"));
        let b = Endpoint::for_data_dir(std::path::Path::new("/data/b"));
        assert_ne!(a, b);
        assert_eq!(a, Endpoint::for_data_dir(std::path::Path::new("/DATA/A")));
    }

    #[test]
    fn token_comparison() {
        assert!(tokens_match("abc", "abc"));
        assert!(!tokens_match("abc", "abd"));
        assert!(!tokens_match("", ""));
        assert!(!tokens_match("abc", "abcd"));
    }

    #[test]
    fn request_json_shape() {
        let req = HookRequest {
            v: PROTOCOL_VERSION,
            token: "t".into(),
            provider: "claude".into(),
            origin: HookOrigin::Managed,
            received_at_ms: 1,
            relay_pid: 2,
            payload: serde_json::json!({"hook_event_name": "Stop"}),
            payload_truncated: false,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["origin"], "managed");
        assert_eq!(json["receivedAtMs"], 1);
    }
}
