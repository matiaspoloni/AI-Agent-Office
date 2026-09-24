//! Safe installation of Agent Office hooks into Claude Code user settings.
//!
//! Rules (docs/SECURITY.md):
//! * parse first — invalid JSON is never touched;
//! * only hook handlers that are *ours* (relay executable + `claude` provider
//!   argument) are added, changed or removed; every other key, hook, plugin
//!   and setting is preserved byte-for-byte in meaning and order;
//! * idempotent: installing twice writes nothing the second time;
//! * a timestamped backup is written before any change, keeping the last 5;
//! * writes are atomic (temp file + rename).

use ao_core::provider::{IntegrationState, IntegrationStatus, RelayCommand};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub const PROVIDER_ARG: &str = "claude";

/// Events Agent Office observes. `PermissionRequest` is added separately
/// because it can be answered. `WorktreeCreate` is deliberately absent: a
/// hook there *replaces* Claude's git worktree creation.
pub const OBSERVED_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionDenied",
    "Notification",
    "Stop",
    "StopFailure",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "CwdChanged",
];
pub const PERMISSION_EVENT: &str = "PermissionRequest";

/// How the hooks should behave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPlan {
    pub relay: RelayCommand,
    /// `global` for user settings, `managed` for per-session settings.
    pub origin: &'static str,
    /// Wait for Approve/Reject in Agent Office (synchronous PermissionRequest).
    pub answer_permissions: bool,
    /// Hook timeout (seconds) for the synchronous PermissionRequest hook.
    pub permission_wait_secs: u64,
}

impl HookPlan {
    fn handler(&self, event: &str) -> Value {
        let program = self.relay.program.display().to_string();
        if event == PERMISSION_EVENT && self.answer_permissions {
            // Claude waits for this one: it may carry the user's decision.
            let wait = self.permission_wait_secs;
            return json!({
                "type": "command",
                "command": program,
                "args": self.relay.args(PROVIDER_ARG, self.origin, Some(wait)),
                "timeout": wait + 15
            });
        }
        if event == "SessionEnd" {
            // Async hooks are cancelled at teardown; this one is quick and synchronous.
            return json!({
                "type": "command",
                "command": program,
                "args": self.relay.args(PROVIDER_ARG, self.origin, Some(3)),
                "timeout": 5
            });
        }
        json!({
            "type": "command",
            "command": program,
            "args": self.relay.args(PROVIDER_ARG, self.origin, None),
            "async": true
        })
    }

    /// Our handler for every event we install.
    pub fn desired(&self) -> Vec<(&'static str, Value)> {
        let mut out: Vec<(&'static str, Value)> = OBSERVED_EVENTS
            .iter()
            .map(|e| (*e, self.handler(e)))
            .collect();
        out.push((PERMISSION_EVENT, self.handler(PERMISSION_EVENT)));
        out
    }

    /// `{"hooks": {...}}` for `claude --settings` (managed sessions).
    pub fn settings_document(&self) -> Value {
        let mut hooks = Map::new();
        for (event, handler) in self.desired() {
            hooks.insert(event.to_owned(), json!([{ "hooks": [handler] }]));
        }
        json!({ "hooks": hooks })
    }
}

fn program_is_relay(command: &str) -> bool {
    // Split on both separators: settings written on Windows use backslashes.
    let name = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    matches!(name, "agent-office" | "agent-office-hook")
}

/// Whether a hook handler was installed by Agent Office for Claude.
pub fn is_ours(handler: &Value) -> bool {
    let Some(command) = handler.get("command").and_then(Value::as_str) else {
        return false;
    };
    if !program_is_relay(command) {
        return false;
    }
    let args: Vec<&str> = handler
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let rest = if args.first() == Some(&"hook") {
        &args[1..]
    } else {
        &args[..]
    };
    rest.first() == Some(&PROVIDER_ARG)
}

fn hooks_object(settings: &mut Value) -> Option<&mut Map<String, Value>> {
    if !settings.is_object() {
        return None;
    }
    let root = settings.as_object_mut()?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        return None;
    }
    hooks.as_object_mut()
}

/// Removes our handlers; drops groups/arrays that become empty *because of us*.
/// Returns true if anything changed.
pub fn remove_ours(settings: &mut Value) -> bool {
    let Some(root) = settings.as_object_mut() else {
        return false;
    };
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    let mut empty_events = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        let before_groups = groups.len();
        groups.retain_mut(|group| {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let before = handlers.len();
            handlers.retain(|h| !is_ours(h));
            if handlers.len() != before {
                changed = true;
                // Keep a user's group even if empty only when it wasn't ours to begin with.
                return !handlers.is_empty();
            }
            true
        });
        if groups.is_empty() && before_groups > 0 {
            empty_events.push(event.clone());
        }
    }
    for event in empty_events {
        hooks.remove(&event);
    }
    if changed && hooks.is_empty() {
        root.remove("hooks");
    }
    changed
}

/// Installs (or repairs) our handlers. Returns true if anything changed.
pub fn install(settings: &mut Value, plan: &HookPlan) -> Result<bool, String> {
    if !settings.is_object() {
        return Err("settings.json does not contain a JSON object".into());
    }
    if settings.get("hooks").is_some_and(|h| !h.is_object()) {
        return Err("`hooks` in settings.json is not an object; refusing to change it".into());
    }
    let before = settings.clone();
    remove_ours(settings);
    let hooks = hooks_object(settings).ok_or("cannot edit `hooks`")?;
    for (event, handler) in plan.desired() {
        let groups = hooks.entry(event.to_owned()).or_insert_with(|| json!([]));
        let Some(groups) = groups.as_array_mut() else {
            return Err(format!(
                "`hooks.{event}` is not an array; refusing to change it"
            ));
        };
        groups.push(json!({ "hooks": [handler] }));
    }
    Ok(*settings != before)
}

/// Compares the file's handlers with what the plan wants.
pub fn evaluate(settings: &Value, plan: &HookPlan) -> (IntegrationState, Vec<String>) {
    let mut details = Vec::new();
    let hooks = settings.get("hooks").and_then(Value::as_object);
    let ours = |event: &str| -> Vec<Value> {
        hooks
            .and_then(|h| h.get(event))
            .and_then(Value::as_array)
            .map(|groups| {
                groups
                    .iter()
                    .filter_map(|g| g.get("hooks").and_then(Value::as_array))
                    .flatten()
                    .filter(|h| is_ours(h))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let desired = plan.desired();
    let mut present = 0;
    let mut problems = 0;
    for (event, handler) in &desired {
        let found = ours(event);
        match found.len() {
            0 => {}
            1 if &found[0] == handler => present += 1,
            1 => {
                problems += 1;
                details.push(format!("{event}: outdated entry (path or options changed)"));
            }
            n => {
                problems += 1;
                details.push(format!("{event}: {n} duplicate entries"));
            }
        }
    }
    let state = if present == 0 && problems == 0 {
        IntegrationState::NotInstalled
    } else if present == desired.len() {
        IntegrationState::Installed
    } else {
        let missing = desired.len() - present - problems;
        if missing > 0 {
            details.push(format!("{missing} hook event(s) missing"));
        }
        IntegrationState::NeedsRepair
    };
    if state == IntegrationState::Installed
        && settings.get("disableAllHooks").and_then(Value::as_bool) == Some(true)
    {
        details.push("`disableAllHooks` is true in your Claude settings: no hook runs until you turn it off.".into());
        return (IntegrationState::NeedsUserAction, details);
    }
    (state, details)
}

/// The Claude Code user settings file on disk (safe edits via `ao-config`).
pub type SettingsFile = ao_config::JsonConfigFile;

/// `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json`.
pub fn default_location() -> Option<SettingsFile> {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| ao_detect::home_dir().map(|h| h.join(".claude")))?;
    Some(SettingsFile::new(dir.join("settings.json")))
}

/// Reads, evaluates and reports the integration state of a settings file.
pub fn status(file: &SettingsFile, plan: Option<&HookPlan>) -> IntegrationStatus {
    let config_path = Some(file.path.display().to_string());
    let Some(plan) = plan else {
        return IntegrationStatus {
            state: IntegrationState::Error,
            config_path,
            details: vec![
                "The hook relay is unavailable in this build; integration cannot be managed."
                    .into(),
            ],
        };
    };
    match file.read() {
        Err(e) => IntegrationStatus {
            state: IntegrationState::Error,
            config_path,
            details: vec![e],
        },
        Ok(None) => IntegrationStatus {
            state: IntegrationState::NotInstalled,
            config_path,
            details: vec!["No Claude Code user settings file yet.".into()],
        },
        Ok(Some(settings)) => {
            let (state, mut details) = evaluate(&settings, plan);
            let backups = file.backup_count();
            if backups > 0 {
                details.push(format!("{backups} backup(s) next to the settings file"));
            }
            IntegrationStatus {
                state,
                config_path,
                details,
            }
        }
    }
}

/// Installs/repairs (`install = true`) or uninstalls, writing only on change.
pub fn apply(
    file: &SettingsFile,
    plan: &HookPlan,
    install_hooks: bool,
) -> Result<IntegrationStatus, String> {
    let current = file.read()?;
    let exists = current.is_some();
    let mut settings = current.unwrap_or_else(|| json!({}));
    let changed = if install_hooks {
        install(&mut settings, plan)?
    } else {
        remove_ours(&mut settings)
    };
    if changed && (exists || install_hooks) {
        file.write(&settings)?;
    }
    Ok(status(file, Some(plan)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(answer: bool) -> HookPlan {
        HookPlan {
            relay: RelayCommand {
                program: PathBuf::from("C:\\Program Files\\Agent Office\\agent-office.exe"),
                prefix_args: vec!["hook".into()],
                data_dir: None,
            },
            origin: "global",
            answer_permissions: answer,
            permission_wait_secs: 120,
        }
    }

    fn user_settings() -> Value {
        json!({
            "model": "opus",
            "enabledPlugins": {"agents-md@builtin": true},
            "permissions": {"allow": ["Bash(git status)"]},
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "~/.claude/hooks/guard.sh"}]}
                ],
                "Notification": [
                    {"hooks": [{"type": "command", "command": "notify-send Claude"}]}
                ]
            }
        })
    }

    #[test]
    fn install_preserves_user_config_and_is_idempotent() {
        let mut s = user_settings();
        assert!(install(&mut s, &plan(false)).unwrap());
        assert_eq!(s["model"], "opus");
        assert_eq!(s["enabledPlugins"]["agents-md@builtin"], true);
        assert_eq!(s["permissions"]["allow"][0], "Bash(git status)");
        let pre = s["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2, "user's group kept, ours appended");
        assert_eq!(pre[0]["hooks"][0]["command"], "~/.claude/hooks/guard.sh");
        assert!(is_ours(&pre[1]["hooks"][0]));
        assert_eq!(pre[1]["hooks"][0]["async"], true);
        assert_eq!(evaluate(&s, &plan(false)).0, IntegrationState::Installed);

        let snapshot = s.clone();
        assert!(
            !install(&mut s, &plan(false)).unwrap(),
            "second install changes nothing"
        );
        assert_eq!(s, snapshot);
    }

    #[test]
    fn uninstall_restores_the_user_config() {
        let original = user_settings();
        let mut s = original.clone();
        install(&mut s, &plan(true)).unwrap();
        assert!(remove_ours(&mut s));
        assert_eq!(s, original);
        assert!(!remove_ours(&mut s));

        let mut empty = json!({});
        install(&mut empty, &plan(false)).unwrap();
        remove_ours(&mut empty);
        assert_eq!(empty, json!({}));
    }

    #[test]
    fn detects_outdated_and_duplicated_entries() {
        let mut s = json!({});
        install(&mut s, &plan(false)).unwrap();
        // Answer mode changes the PermissionRequest handler → repair needed.
        let (state, details) = evaluate(&s, &plan(true));
        assert_eq!(state, IntegrationState::NeedsRepair);
        assert!(details.iter().any(|d| d.contains("PermissionRequest")));

        // App moved to another folder → every entry outdated.
        let mut moved = plan(false);
        moved.relay.program = PathBuf::from("D:\\Apps\\agent-office.exe");
        assert_eq!(evaluate(&s, &moved).0, IntegrationState::NeedsRepair);
        assert!(install(&mut s, &moved).unwrap());
        assert_eq!(evaluate(&s, &moved).0, IntegrationState::Installed);

        // A duplicate handler (e.g. copied by hand).
        let dup = s["hooks"]["Stop"][0].clone();
        s["hooks"]["Stop"].as_array_mut().unwrap().push(dup);
        assert_eq!(evaluate(&s, &moved).0, IntegrationState::NeedsRepair);
        install(&mut s, &moved).unwrap();
        assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn refuses_unexpected_shapes() {
        assert!(install(&mut json!({"hooks": []}), &plan(false)).is_err());
        assert!(install(&mut json!({"hooks": {"Stop": {"x": 1}}}), &plan(false)).is_err());
        assert!(install(&mut json!([1, 2]), &plan(false)).is_err());
    }

    #[test]
    fn disable_all_hooks_needs_user_action() {
        let mut s = json!({"disableAllHooks": true});
        install(&mut s, &plan(false)).unwrap();
        assert_eq!(
            evaluate(&s, &plan(false)).0,
            IntegrationState::NeedsUserAction
        );
    }

    #[test]
    fn recognises_only_our_handlers() {
        assert!(is_ours(
            &json!({"type": "command", "command": "C:\\x\\agent-office.exe", "args": ["hook", "claude"]})
        ));
        assert!(is_ours(
            &json!({"type": "command", "command": "/usr/bin/agent-office-hook", "args": ["claude", "--origin", "global"]})
        ));
        assert!(!is_ours(
            &json!({"type": "command", "command": "C:\\x\\agent-office.exe", "args": ["hook", "codex"]})
        ));
        assert!(!is_ours(
            &json!({"type": "command", "command": "my-agent-office-helper", "args": ["claude"]})
        ));
        assert!(!is_ours(
            &json!({"type": "http", "url": "http://localhost"})
        ));
    }

    #[test]
    fn file_roundtrip_with_backup_and_invalid_json_protection() {
        let dir = std::env::temp_dir().join(format!(
            "ao-claude-settings-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let file = SettingsFile::new(dir.join("settings.json"));

        // Missing file: install creates it, no backup needed.
        let status = apply(&file, &plan(false), true).unwrap();
        assert_eq!(status.state, IntegrationState::Installed);
        assert_eq!(file.backup_count(), 0);

        // Re-install: nothing written, still no backup.
        apply(&file, &plan(false), true).unwrap();
        assert_eq!(file.backup_count(), 0);

        // Uninstall: backup taken first.
        let status = apply(&file, &plan(false), false).unwrap();
        assert_eq!(status.state, IntegrationState::NotInstalled);
        assert_eq!(file.backup_count(), 1);

        // Backups are pruned.
        for i in 0..8 {
            apply(&file, &plan(i % 2 == 0), true).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(file.backup_count() <= ao_config::KEEP_BACKUPS);

        // Invalid JSON is never modified.
        std::fs::write(&file.path, "{ \"model\": \"opus\", // comment\n}").unwrap();
        assert!(apply(&file, &plan(false), true).is_err());
        assert_eq!(
            std::fs::read_to_string(&file.path).unwrap(),
            "{ \"model\": \"opus\", // comment\n}"
        );
        assert_eq!(status_state(&file), IntegrationState::Error);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn status_state(file: &SettingsFile) -> IntegrationState {
        status(file, Some(&plan(false))).state
    }

    #[test]
    fn managed_settings_document_has_every_event() {
        let mut p = plan(true);
        p.origin = "managed";
        let doc = p.settings_document();
        for event in OBSERVED_EVENTS.iter().chain([&PERMISSION_EVENT]) {
            let handler = &doc["hooks"][*event][0]["hooks"][0];
            assert!(is_ours(handler), "{event}");
            assert!(handler["args"]
                .as_array()
                .unwrap()
                .contains(&json!("managed")));
        }
        assert!(doc["hooks"]["PermissionRequest"][0]["hooks"][0]
            .get("async")
            .is_none());
        assert!(doc["hooks"].get("WorktreeCreate").is_none());
    }
}
