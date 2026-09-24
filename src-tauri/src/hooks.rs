//! Hook bridge: the local IPC server that receives hook invocations from the
//! relay and hands them to provider adapters, plus counters for Diagnostics.

use ao_core::provider::RelayCommand;
use ao_ipc::Endpoint;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Mutex;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HookEventCount {
    pub provider: String,
    pub event: String,
    #[ts(type = "number")]
    pub count: u64,
    #[ts(type = "number")]
    pub last_at: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HookBridgeStatus {
    /// Named pipe (Windows) or socket path (Unix) the relay connects to.
    pub endpoint: String,
    pub listening: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
    /// Command written into hook configurations.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub relay_command: Option<String>,
    pub relay_exists: bool,
    /// Hook events actually received, per provider — the empirical evidence
    /// of what each CLI fires on this machine.
    pub events: Vec<HookEventCount>,
}

pub struct HookBridge {
    pub endpoint: Endpoint,
    pub server: Mutex<Option<ao_ipc::server::Server>>,
    pub error: Mutex<Option<String>>,
    stats: Mutex<BTreeMap<(String, String), (u64, i64)>>,
}

impl HookBridge {
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            server: Mutex::new(None),
            error: Mutex::new(None),
            stats: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn record(&self, provider: &str, event: &str, at: i64) {
        let mut stats = self.stats.lock().expect("hook stats lock");
        let entry = stats
            .entry((provider.to_owned(), event.to_owned()))
            .or_insert((0, at));
        entry.0 += 1;
        entry.1 = at;
    }

    pub fn status(&self, relay: Option<&RelayCommand>) -> HookBridgeStatus {
        let listening = self
            .server
            .lock()
            .expect("server lock")
            .as_ref()
            .is_some_and(|s| s.is_running());
        HookBridgeStatus {
            endpoint: self.endpoint.display(),
            listening,
            error: self.error.lock().expect("error lock").clone(),
            relay_command: relay.map(|r| {
                let mut parts = vec![r.program.display().to_string()];
                parts.extend(r.prefix_args.iter().cloned());
                parts.push("<provider>".into());
                parts.join(" ")
            }),
            relay_exists: relay.is_some_and(|r| r.program.is_file()),
            events: self
                .stats
                .lock()
                .expect("hook stats lock")
                .iter()
                .map(|((provider, event), (count, last_at))| HookEventCount {
                    provider: provider.clone(),
                    event: event.clone(),
                    count: *count,
                    last_at: *last_at,
                })
                .collect(),
        }
    }
}
