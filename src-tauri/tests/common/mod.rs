//! Helpers shared by the end-to-end tests.
#![allow(dead_code)]

use agent_office_lib::host::Host;
use ao_core::world::{AgentState, WorldSnapshot};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("ao-e2e-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn sub(&self, name: &str) -> PathBuf {
        let dir = self.0.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub async fn wait_for<T>(
    what: &str,
    host: &Host,
    mut check: impl FnMut(&WorldSnapshot) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        host.flush();
        let snapshot = host.snapshot();
        if let Some(value) = check(&snapshot) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; state: {snapshot:#?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub async fn wait_listening(host: &Host) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !host.hook_status().listening {
        assert!(
            Instant::now() < deadline,
            "hook bridge did not start: {:?}",
            host.hook_status()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub fn main_agent<'a>(snapshot: &'a WorldSnapshot, session: &str) -> Option<&'a AgentState> {
    snapshot
        .agents
        .iter()
        .find(|a| a.session_id.0 == session && a.is_main)
}
