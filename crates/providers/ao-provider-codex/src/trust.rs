//! Asks Codex itself whether our hooks are trusted: a short-lived
//! `codex app-server` answers `hooks/list` (no account needed; verified with
//! codex-cli 0.156.1). Agent Office never writes trust state: the user
//! approves hooks in Codex with `/hooks`.

use crate::settings;
use ao_jsonrpc::{Incoming, RpcClient};
use ao_process::{ManagedProcess, ProcessEvent, SpawnSpec, Stream};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookTrust {
    pub event: String,
    /// `trusted`, `untrusted`, `modified` or `managed`.
    pub trust_status: String,
    pub enabled: bool,
}

/// Summarizes our entries into an integration verdict.
pub fn verdict(entries: &[HookTrust]) -> Option<String> {
    let untrusted: Vec<&str> = entries
        .iter()
        .filter(|e| !matches!(e.trust_status.as_str(), "trusted" | "managed"))
        .map(|e| e.event.as_str())
        .collect();
    let disabled: Vec<&str> = entries
        .iter()
        .filter(|e| !e.enabled)
        .map(|e| e.event.as_str())
        .collect();
    if !untrusted.is_empty() {
        return Some(format!(
            "Codex has not trusted the Agent Office hooks yet ({} of {}). Open Codex, type /hooks and trust them — Codex runs no hook until you do.",
            untrusted.len(),
            entries.len()
        ));
    }
    if !disabled.is_empty() {
        return Some(format!(
            "{} Agent Office hook(s) are disabled in Codex (/hooks): {}.",
            disabled.len(),
            disabled.join(", ")
        ));
    }
    None
}

/// Parses a `hooks/list` result, keeping our handlers from `hooks_file`.
pub fn parse_hooks_list(result: &Value, hooks_file: &Path) -> Vec<HookTrust> {
    let file = hooks_file
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    let mut out: Vec<HookTrust> = Vec::new();
    for entry in result
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for hook in entry
            .get("hooks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let source = hook
                .get("sourcePath")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .replace('\\', "/")
                .to_lowercase();
            let handler = json!({
                "type": hook.get("handlerType").cloned().unwrap_or(Value::Null),
                "command": hook.get("command").cloned().unwrap_or(Value::Null),
            });
            if source != file || !settings::is_ours(&handler) {
                continue;
            }
            let key = hook.get("key").and_then(Value::as_str).unwrap_or_default();
            if out.iter().any(|h: &HookTrust| h.event == key) {
                continue; // listed once per cwd
            }
            out.push(HookTrust {
                event: key.rsplit(':').nth(2).unwrap_or(key).to_owned(),
                trust_status: hook
                    .get("trustStatus")
                    .and_then(Value::as_str)
                    .unwrap_or("untrusted")
                    .to_owned(),
                enabled: hook.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            });
        }
    }
    out
}

/// Runs `codex app-server`, calls `initialize` + `hooks/list`, and stops it.
pub async fn query(
    executable: &Path,
    codex_home: Option<&PathBuf>,
    hooks_file: &Path,
) -> Result<Vec<HookTrust>, String> {
    let cwd = hooks_file.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut spec = SpawnSpec::new(executable).arg("app-server").cwd(&cwd);
    if let Some(home) = codex_home {
        spec = spec.env("CODEX_HOME", home.as_os_str());
    }
    let (process, mut lines) =
        ManagedProcess::spawn(spec).map_err(|e| format!("cannot start Codex: {e}"))?;
    let rpc = RpcClient::new(process.clone());
    let pump_rpc = rpc.clone();
    let pump = tokio::spawn(async move {
        while let Some(event) = lines.recv().await {
            match event {
                ProcessEvent::Line {
                    stream: Stream::Stdout,
                    line,
                } => {
                    // No thread exists, so no server request can arrive; any
                    // request would be answered with an error.
                    if let Some(Incoming::Request { id, .. }) = pump_rpc.handle_line(&line) {
                        let _ = pump_rpc.respond_error(&id, -32601, "not supported").await;
                    }
                }
                ProcessEvent::Exited(_) => {
                    pump_rpc.fail_all();
                    break;
                }
                _ => {}
            }
        }
    });
    let result = async {
        rpc.request(
            "initialize",
            json!({
                "clientInfo": { "name": "agent_office", "title": "Agent Office", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": null
            }),
            Duration::from_secs(20),
        )
        .await
        .map_err(|e| format!("Codex did not start: {e}"))?;
        rpc.notify("initialized", None).await.map_err(|e| e.to_string())?;
        rpc.request("hooks/list", json!({ "cwds": [cwd.display().to_string()] }), Duration::from_secs(20))
            .await
            .map_err(|e| format!("`hooks/list` failed: {e}"))
    }
    .await;
    process.stop(Duration::from_secs(3)).await;
    pump.abort();
    Ok(parse_hooks_list(&result?, hooks_file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_our_hooks_from_our_file() {
        let result = json!({"data": [{"cwd": "/home/u", "warnings": [], "errors": [], "hooks": [
            {"key": "/home/u/.codex/hooks.json:stop:0:0", "eventName": "stop", "handlerType": "command",
             "command": "notify-send done", "sourcePath": "/home/u/.codex/hooks.json", "trustStatus": "trusted", "enabled": true},
            {"key": "/home/u/.codex/hooks.json:stop:1:0", "eventName": "stop", "handlerType": "command",
             "command": "'/opt/agent-office' hook codex --origin global", "sourcePath": "/home/u/.codex/hooks.json", "trustStatus": "untrusted", "enabled": true},
            {"key": "/p/.codex/hooks.json:stop:0:0", "eventName": "stop", "handlerType": "command",
             "command": "'/opt/agent-office' hook codex --origin global", "sourcePath": "/p/.codex/hooks.json", "trustStatus": "trusted", "enabled": true}
        ]}]});
        let ours = parse_hooks_list(&result, Path::new("/home/u/.codex/hooks.json"));
        assert_eq!(
            ours,
            vec![HookTrust {
                event: "stop".into(),
                trust_status: "untrusted".into(),
                enabled: true
            }]
        );
        assert!(verdict(&ours).unwrap().contains("/hooks"));
        let trusted = vec![HookTrust {
            event: "stop".into(),
            trust_status: "trusted".into(),
            enabled: true,
        }];
        assert_eq!(verdict(&trusted), None);
    }
}
