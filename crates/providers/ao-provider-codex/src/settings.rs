//! Agent Office hooks in the user's Codex `hooks.json` (`$CODEX_HOME/hooks.json`,
//! default `~/.codex/hooks.json`).
//!
//! Facts from the codex 0.156.1 source this module depends on:
//!
//! * The file is `{"description"?, "hooks": {"<Event>": [{"matcher"?, "hooks": [handler]}]}}`
//!   and unknown top-level keys make Codex reject it, so only `hooks` is touched.
//! * A command handler is `{"type": "command", "command", "timeout", "async"}`;
//!   `command` is a **shell command line** (`cmd.exe /C` on Windows,
//!   `$SHELL -lc` elsewhere), so the relay path is quoted here.
//! * Hooks must be trusted by the user in Codex (`/hooks`). Trust is stored per
//!   key `"<file>:<event>:<group index>:<handler index>"` with a content hash,
//!   so our groups are only ever **appended**, and a removed group that is not
//!   the last one is left as an empty placeholder: the user's other hooks keep
//!   their positions and their trust.
//! * `SessionEnd` hooks always run synchronously and, like `Interrupt`, have
//!   their timeout clamped to 3 s.

use ao_config::JsonConfigFile;
use ao_core::provider::{IntegrationState, IntegrationStatus, RelayCommand};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const PROVIDER_ARG: &str = "codex";

pub const OBSERVED_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "Interrupt",
];
pub const PERMISSION_EVENT: &str = "PermissionRequest";

/// How the shell Codex uses will parse our command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// `cmd.exe /C "<line>"` (Windows).
    Cmd,
    /// `$SHELL -lc '<line>'` (macOS, Linux).
    Posix,
}

impl ShellKind {
    pub fn native() -> Self {
        if cfg!(windows) {
            ShellKind::Cmd
        } else {
            ShellKind::Posix
        }
    }
}

fn quote(arg: &str, shell: ShellKind) -> Result<String, String> {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:\\".contains(c));
    match shell {
        ShellKind::Posix if plain => Ok(arg.to_owned()),
        ShellKind::Posix => Ok(format!("'{}'", arg.replace('\'', r"'\''"))),
        ShellKind::Cmd => {
            // Inside quotes cmd.exe still expands %VAR% and a `"` cannot be
            // escaped: such paths cannot be passed safely.
            if arg.contains('"') || arg.contains('%') {
                return Err(format!(
                    "the path `{arg}` contains `\"` or `%`, which cannot be passed safely through cmd.exe"
                ));
            }
            Ok(if plain {
                arg.to_owned()
            } else {
                format!("\"{arg}\"")
            })
        }
    }
}

/// The shell command line for one hook entry.
pub fn command_line(
    relay: &RelayCommand,
    wait_secs: Option<u64>,
    shell: ShellKind,
) -> Result<String, String> {
    let program = relay.program.display().to_string();
    // The program is always quoted so paths with spaces work.
    let mut parts = vec![match shell {
        ShellKind::Cmd => {
            quote(&program, shell)?;
            format!("\"{program}\"")
        }
        ShellKind::Posix => format!("'{}'", program.replace('\'', r"'\''")),
    }];
    for arg in relay.args(PROVIDER_ARG, "global", wait_secs) {
        parts.push(quote(&arg, shell)?);
    }
    Ok(parts.join(" "))
}

/// Splits a command line into words: `"…"` and `'…'` group words, `'\''`
/// is an escaped quote in POSIX strings. Backslashes are otherwise literal
/// (Windows paths).
pub fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_word = true;
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                    current.push(c);
                }
            }
            '\'' => {
                in_word = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    current.push(c);
                }
            }
            '\\' if chars.peek() == Some(&'\'') => {
                in_word = true;
                current.push('\'');
                chars.next();
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                current.push(c);
            }
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

/// Whether a handler was installed by Agent Office for Codex.
pub fn is_ours(handler: &Value) -> bool {
    if handler.get("type").and_then(Value::as_str) != Some("command") {
        return false;
    }
    let Some(command) = handler.get("command").and_then(Value::as_str) else {
        return false;
    };
    let words = split_words(command);
    let Some(program) = words.first() else {
        return false;
    };
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    if !matches!(name, "agent-office" | "agent-office-hook") {
        return false;
    }
    let rest = &words[1..];
    let rest = if rest.first().map(String::as_str) == Some("hook") {
        &rest[1..]
    } else {
        rest
    };
    rest.first().map(String::as_str) == Some(PROVIDER_ARG)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPlan {
    pub relay: RelayCommand,
    /// Wait for Approve/Reject in Agent Office (synchronous PermissionRequest).
    pub answer_permissions: bool,
    pub permission_wait_secs: u64,
    pub shell: ShellKind,
}

impl HookPlan {
    fn handler(&self, event: &str) -> Result<Value, String> {
        let line = |wait| command_line(&self.relay, wait, self.shell);
        Ok(match event {
            PERMISSION_EVENT if self.answer_permissions => {
                let wait = self.permission_wait_secs;
                json!({ "type": "command", "command": line(Some(wait))?, "timeout": wait + 15 })
            }
            "SessionEnd" => json!({ "type": "command", "command": line(Some(2))?, "timeout": 3 }),
            "Interrupt" => {
                json!({ "type": "command", "command": line(None)?, "timeout": 3, "async": true })
            }
            _ => json!({ "type": "command", "command": line(None)?, "timeout": 10, "async": true }),
        })
    }

    pub fn desired(&self) -> Result<Vec<(&'static str, Value)>, String> {
        let mut out = Vec::new();
        for event in OBSERVED_EVENTS.iter().copied().chain([PERMISSION_EVENT]) {
            out.push((event, self.handler(event)?));
        }
        Ok(out)
    }
}

fn is_empty_group(group: &Value) -> bool {
    matches!(group.get("matcher"), None | Some(Value::Null))
        && group
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
}

/// Removes our handlers from one event's groups without moving anyone
/// else's: an emptied group that is followed by other groups stays as
/// `{"hooks": []}`; trailing empty groups are dropped.
fn remove_ours_from(groups: &mut Vec<Value>) -> bool {
    let mut changed = false;
    for group in groups.iter_mut() {
        if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            let before = handlers.len();
            handlers.retain(|h| !is_ours(h));
            changed |= handlers.len() != before;
        }
    }
    while groups.last().is_some_and(is_empty_group) {
        groups.pop();
        changed = true;
    }
    changed
}

fn hooks_object(file: &mut Value) -> Result<&mut Map<String, Value>, String> {
    let root = file
        .as_object_mut()
        .ok_or("hooks.json is not a JSON object; Agent Office will not modify it")?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    hooks.as_object_mut().ok_or_else(|| {
        "`hooks` in hooks.json is not an object; Agent Office will not modify it".to_owned()
    })
}

/// Removes every Agent Office handler. Returns whether anything changed.
pub fn remove_ours(file: &mut Value) -> bool {
    let Ok(hooks) = hooks_object(file) else {
        return false;
    };
    let mut changed = false;
    let mut emptied = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        if let Some(groups) = groups.as_array_mut() {
            changed |= remove_ours_from(groups);
            if groups.is_empty() {
                emptied.push(event.clone());
            }
        }
    }
    for event in emptied {
        hooks.remove(&event);
    }
    if hooks.is_empty() {
        if let Some(root) = file.as_object_mut() {
            root.remove("hooks");
        }
    }
    changed
}

/// Installs or repairs our handlers. Events that already hold exactly the
/// desired handler are left untouched (keeping their trust).
pub fn install(file: &mut Value, plan: &HookPlan) -> Result<bool, String> {
    let desired = plan.desired()?;
    let hooks = hooks_object(file)?;
    for (event, groups) in hooks.iter() {
        if !groups.is_array() {
            return Err(format!(
                "`hooks.{event}` in hooks.json is not a list; Agent Office will not modify it"
            ));
        }
    }
    let mut changed = false;
    for (event, handler) in desired {
        let groups = hooks
            .entry(event.to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("checked above");
        let ours: Vec<&Value> = groups
            .iter()
            .filter_map(|g| g.get("hooks").and_then(Value::as_array))
            .flatten()
            .filter(|h| is_ours(h))
            .collect();
        if ours.len() == 1 && *ours[0] == handler {
            continue;
        }
        remove_ours_from(groups);
        groups.push(json!({ "hooks": [handler] }));
        changed = true;
    }
    Ok(changed)
}

pub fn evaluate(file: &Value, plan: &HookPlan) -> (IntegrationState, Vec<String>) {
    let desired = match plan.desired() {
        Ok(d) => d,
        Err(e) => return (IntegrationState::Error, vec![e]),
    };
    let hooks = file.get("hooks").and_then(Value::as_object);
    let mut present = 0;
    let mut problems = 0;
    let mut details = Vec::new();
    for (event, handler) in &desired {
        let ours: Vec<&Value> = hooks
            .and_then(|h| h.get(*event))
            .and_then(Value::as_array)
            .map(|groups| {
                groups
                    .iter()
                    .filter_map(|g| g.get("hooks").and_then(Value::as_array))
                    .flatten()
                    .filter(|h| is_ours(h))
                    .collect()
            })
            .unwrap_or_default();
        match ours.as_slice() {
            [] => {}
            [one] if *one == handler => present += 1,
            [_] => {
                problems += 1;
                details.push(format!("{event}: outdated entry"));
            }
            _ => {
                problems += 1;
                details.push(format!("{event}: {} duplicate entries", ours.len()));
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
    (state, details)
}

/// `$CODEX_HOME/hooks.json`, else `~/.codex/hooks.json`.
pub fn default_location() -> Option<JsonConfigFile> {
    let dir = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| ao_detect::home_dir().map(|h| h.join(".codex")))?;
    Some(JsonConfigFile::new(dir.join("hooks.json")))
}

/// State of the file only (trust is checked separately through Codex).
pub fn status(file: &JsonConfigFile, plan: Option<&HookPlan>) -> IntegrationStatus {
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
            details: vec!["No Codex hooks.json yet.".into()],
        },
        Ok(Some(content)) => {
            let (state, mut details) = evaluate(&content, plan);
            let backups = file.backup_count();
            if backups > 0 {
                details.push(format!("{backups} backup(s) next to hooks.json"));
            }
            IntegrationStatus {
                state,
                config_path,
                details,
            }
        }
    }
}

/// Installs/repairs (`install_hooks = true`) or uninstalls, writing only on change.
pub fn apply(
    file: &JsonConfigFile,
    plan: &HookPlan,
    install_hooks: bool,
) -> Result<IntegrationStatus, String> {
    let current = file.read()?;
    let exists = current.is_some();
    let mut content = current.unwrap_or_else(|| json!({}));
    let changed = if install_hooks {
        install(&mut content, plan)?
    } else {
        remove_ours(&mut content)
    };
    if changed && (exists || install_hooks) {
        file.write(&content)?;
    }
    Ok(status(file, Some(plan)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(answer: bool, shell: ShellKind) -> HookPlan {
        HookPlan {
            relay: RelayCommand {
                program: PathBuf::from(match shell {
                    ShellKind::Cmd => "C:\\Program Files\\Agent Office\\agent-office.exe",
                    ShellKind::Posix => "/opt/Agent Office/agent-office",
                }),
                prefix_args: vec!["hook".into()],
                data_dir: None,
            },
            answer_permissions: answer,
            permission_wait_secs: 150,
            shell,
        }
    }

    fn user_file() -> Value {
        json!({
            "description": "my hooks",
            "hooks": {
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "python3 ~/.codex/check.py", "timeout": 30}]}],
                "Stop": [{"hooks": [{"type": "command", "command": "notify-send done"}]}]
            }
        })
    }

    #[test]
    fn command_lines_are_quoted_for_each_shell() {
        let cmd = command_line(&plan(false, ShellKind::Cmd).relay, None, ShellKind::Cmd).unwrap();
        assert_eq!(
            cmd,
            r#""C:\Program Files\Agent Office\agent-office.exe" hook codex --origin global"#
        );
        let sh = command_line(
            &plan(false, ShellKind::Posix).relay,
            Some(5),
            ShellKind::Posix,
        )
        .unwrap();
        assert_eq!(
            sh,
            "'/opt/Agent Office/agent-office' hook codex --origin global --wait 5"
        );
        assert_eq!(
            split_words(&sh),
            [
                "/opt/Agent Office/agent-office",
                "hook",
                "codex",
                "--origin",
                "global",
                "--wait",
                "5"
            ]
        );

        let mut odd = plan(false, ShellKind::Posix).relay;
        odd.program = PathBuf::from("/home/o'brien/agent-office");
        odd.data_dir = Some(PathBuf::from("/data dir"));
        let line = command_line(&odd, None, ShellKind::Posix).unwrap();
        assert_eq!(split_words(&line)[0], "/home/o'brien/agent-office");
        assert_eq!(split_words(&line).last().unwrap(), "/data dir");

        let mut bad = plan(false, ShellKind::Cmd).relay;
        bad.program = PathBuf::from("C:\\100%\\agent-office.exe");
        assert!(command_line(&bad, None, ShellKind::Cmd).is_err());
    }

    #[test]
    fn install_appends_after_user_hooks_and_uninstall_restores() {
        for shell in [ShellKind::Cmd, ShellKind::Posix] {
            let original = user_file();
            let mut f = original.clone();
            assert!(install(&mut f, &plan(false, shell)).unwrap());
            // The user's groups keep index 0; ours come after.
            assert_eq!(
                f["hooks"]["PreToolUse"][0],
                original["hooks"]["PreToolUse"][0]
            );
            assert!(is_ours(&f["hooks"]["PreToolUse"][1]["hooks"][0]));
            assert_eq!(f["description"], "my hooks");
            assert_eq!(
                evaluate(&f, &plan(false, shell)).0,
                IntegrationState::Installed
            );
            assert!(!install(&mut f, &plan(false, shell)).unwrap(), "idempotent");
            assert!(remove_ours(&mut f));
            assert_eq!(f, original);
        }
    }

    #[test]
    fn removing_a_middle_group_keeps_later_indices() {
        let mut f = user_file();
        install(&mut f, &plan(false, ShellKind::Posix)).unwrap();
        // The user adds a hook after ours.
        let later = json!({"hooks": [{"type": "command", "command": "later.sh"}]});
        f["hooks"]["Stop"]
            .as_array_mut()
            .unwrap()
            .push(later.clone());
        remove_ours(&mut f);
        let stop = f["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 3);
        assert_eq!(
            stop[1],
            json!({"hooks": []}),
            "placeholder keeps `later.sh` at index 2"
        );
        assert_eq!(stop[2], later);
        // Re-installing appends again instead of reusing the placeholder.
        install(&mut f, &plan(false, ShellKind::Posix)).unwrap();
        assert!(is_ours(&f["hooks"]["Stop"][3]["hooks"][0]));
    }

    #[test]
    fn detects_changes_that_need_repair() {
        let mut f = json!({});
        install(&mut f, &plan(false, ShellKind::Posix)).unwrap();
        let (state, details) = evaluate(&f, &plan(true, ShellKind::Posix));
        assert_eq!(state, IntegrationState::NeedsRepair);
        assert!(details.iter().any(|d| d.contains("PermissionRequest")));
        let session_end = &f["hooks"]["SessionEnd"][0]["hooks"][0];
        assert_eq!(session_end["timeout"], 3);
        assert!(
            session_end.get("async").is_none(),
            "SessionEnd always runs synchronously"
        );
    }

    #[test]
    fn recognises_only_our_handlers_and_refuses_odd_shapes() {
        assert!(is_ours(
            &json!({"type": "command", "command": "\"C:\\x\\agent-office.exe\" hook codex --origin global"})
        ));
        assert!(is_ours(
            &json!({"type": "command", "command": "/usr/bin/agent-office-hook codex --origin global"})
        ));
        assert!(!is_ours(
            &json!({"type": "command", "command": "/usr/bin/agent-office hook claude"})
        ));
        assert!(!is_ours(
            &json!({"type": "command", "command": "echo agent-office hook codex"})
        ));
        assert!(!is_ours(&json!({"type": "prompt"})));
        assert!(install(&mut json!({"hooks": []}), &plan(false, ShellKind::Posix)).is_err());
        assert!(install(
            &mut json!({"hooks": {"Stop": {}}}),
            &plan(false, ShellKind::Posix)
        )
        .is_err());
    }
}
