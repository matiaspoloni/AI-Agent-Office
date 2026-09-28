//! `agent-office integrations <command>`: the hook integrations without the
//! window, for the installer (and anyone who prefers a terminal).
//!
//! * `status` — per provider: installed or not (reads files only).
//! * `install <provider>` — like Diagnostics → Install.
//! * `uninstall [--remember]` — removes Agent Office's entries from every
//!   provider's configuration (only ours, with the usual backup). With
//!   `--remember` it notes which were present, so that an upgrade (the new
//!   installer first runs the old uninstaller) can put them back.
//! * `restore` — installs again what a `uninstall --remember` noted in the
//!   last hour, then forgets the note. Older notes are ignored: a later,
//!   separate installation does not silently bring hooks back.
//!
//! The result is printed as JSON; the exit code is 0 unless something failed.

use crate::host::HostOptions;
use crate::paths::AppPaths;
use crate::prefs::Preferences;
use ao_core::provider::{AdapterContext, EventSink};
use ao_core::registry::ProviderRegistry;
use ao_core::time::now_ms;
use ao_provider_demo::DemoAdapter;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MARKER: &str = "integrations-removed.json";
/// A note older than this is not restored.
const RESTORE_WINDOW_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    at: i64,
    providers: Vec<String>,
}

pub struct Outcome {
    pub code: i32,
    pub output: Value,
}

fn marker_path(paths: &AppPaths) -> PathBuf {
    paths.data_dir.join(MARKER)
}

fn read_marker(path: &Path) -> Option<Marker> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn registry(options: &HostOptions, paths: &AppPaths) -> ProviderRegistry {
    let registry = crate::providers::build_registry(
        Arc::new(DemoAdapter::new()),
        options.claude.clone(),
        options.codex.clone(),
        options.cursor.clone(),
    );
    // Hooks are generated from the preferences (e.g. whether permission
    // requests wait for Agent Office): use the saved ones.
    // (Without creating a database when there is none yet.)
    let prefs = if paths.db_path.exists() {
        ao_store::Store::open(&paths.db_path)
            .map(|store| Preferences::load(&store))
            .unwrap_or_default()
    } else {
        Preferences::default()
    };
    for adapter in registry.adapters() {
        adapter.configure(&prefs.provider_settings());
    }
    registry
}

pub async fn run(args: &[String], paths: &AppPaths, options: HostOptions) -> Outcome {
    let (sink, _events) = EventSink::new(16);
    let ctx = AdapterContext {
        sink,
        relay: options.relay.clone(),
        data_dir: paths.data_dir.clone(),
    };
    let registry = registry(&options, paths);
    let command = args.first().map(String::as_str).unwrap_or("status");
    let remember = args.iter().any(|a| a == "--remember");
    let mut failed = false;
    let mut results = serde_json::Map::new();

    match command {
        "status" => {
            for adapter in registry.adapters() {
                let present = adapter.integration_present(&ctx).await;
                results.insert(adapter.id().0.clone(), json!({ "installed": present }));
            }
        }
        "install" => {
            let Some(wanted) = args.get(1) else {
                return Outcome {
                    code: 2,
                    output: json!({ "error": "usage: integrations install <provider>" }),
                };
            };
            match registry
                .adapters()
                .into_iter()
                .find(|a| &a.id().0 == wanted)
            {
                Some(adapter) => match adapter.install_integration(&ctx).await {
                    Ok(status) => {
                        results.insert(wanted.clone(), json!({ "state": status.state }));
                    }
                    Err(err) => {
                        failed = true;
                        results.insert(wanted.clone(), json!({ "error": err.to_string() }));
                    }
                },
                None => {
                    return Outcome {
                        code: 2,
                        output: json!({ "error": format!("unknown provider `{wanted}`") }),
                    }
                }
            }
        }
        "uninstall" => {
            let mut removed = Vec::new();
            for adapter in registry.adapters() {
                if !adapter.integration_present(&ctx).await {
                    continue;
                }
                let id = adapter.id().0.clone();
                match adapter.uninstall_integration(&ctx).await {
                    Ok(_) => {
                        results.insert(id.clone(), json!({ "removed": true }));
                        removed.push(id);
                    }
                    Err(err) => {
                        failed = true;
                        results.insert(id, json!({ "error": err.to_string() }));
                    }
                }
            }
            if remember && !removed.is_empty() {
                let marker = Marker {
                    at: now_ms(),
                    providers: removed,
                };
                let _ = std::fs::create_dir_all(&paths.data_dir);
                if let Ok(text) = serde_json::to_string(&marker) {
                    let _ = std::fs::write(marker_path(paths), text);
                }
            }
        }
        "restore" => {
            let path = marker_path(paths);
            let marker = read_marker(&path);
            let _ = std::fs::remove_file(&path);
            if let Some(marker) = marker.filter(|m| now_ms() - m.at < RESTORE_WINDOW_MS) {
                for adapter in registry.adapters() {
                    let id = adapter.id().0.clone();
                    if !marker.providers.contains(&id) {
                        continue;
                    }
                    match adapter.install_integration(&ctx).await {
                        Ok(status) => {
                            results.insert(id, json!({ "state": status.state }));
                        }
                        Err(err) => {
                            failed = true;
                            results.insert(id, json!({ "error": err.to_string() }));
                        }
                    }
                }
            }
        }
        other => {
            return Outcome {
                code: 2,
                output: json!({
                    "error": format!("unknown command `{other}`"),
                    "usage": "integrations status | install <provider> | uninstall [--remember] | restore"
                }),
            }
        }
    }
    tracing::info!(command, ?results, "integrations command");
    Outcome {
        code: i32::from(failed),
        output: json!({ "command": command, "providers": results }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ao_core::provider::RelayCommand;
    use ao_provider_claude::ClaudeOptions;
    use ao_provider_codex::CodexOptions;
    use ao_provider_cursor::CursorOptions;

    struct Setup {
        root: PathBuf,
        paths: AppPaths,
        options: HostOptions,
    }

    impl Setup {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "ao-integrations-{name}-{}-{}",
                std::process::id(),
                now_ms()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("claude")).unwrap();
            std::fs::create_dir_all(root.join("codex")).unwrap();
            let options = HostOptions {
                relay: Some(RelayCommand {
                    program: root.join("Agent Office").join("agent-office.exe"),
                    prefix_args: vec!["hook".into()],
                    data_dir: None,
                }),
                claude: ClaudeOptions {
                    executable: Some(root.join("no-claude")),
                    config_dir: Some(root.join("claude")),
                    ..Default::default()
                },
                codex: CodexOptions {
                    executable: Some(root.join("no-codex")),
                    config_dir: Some(root.join("codex")),
                },
                cursor: CursorOptions {
                    executable: Some(root.join("no-cursor")),
                },
                git_executable: None,
            };
            Self {
                paths: AppPaths::at(root.join("data")),
                options,
                root,
            }
        }

        async fn run(&self, args: &[&str]) -> Outcome {
            let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
            run(&args, &self.paths, self.options.clone()).await
        }

        fn claude_settings(&self) -> Value {
            serde_json::from_str(
                &std::fs::read_to_string(self.root.join("claude").join("settings.json")).unwrap(),
            )
            .unwrap()
        }
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn has_our_hook(settings: &Value) -> bool {
        settings.to_string().contains("agent-office.exe")
    }

    #[tokio::test]
    async fn an_upgrade_removes_and_restores_the_hooks() {
        let s = Setup::new("upgrade");
        // The user's own settings, which must survive everything.
        std::fs::write(
            s.root.join("claude").join("settings.json"),
            r#"{"model":"opus","hooks":{"Stop":[{"hooks":[{"type":"command","command":"notify-me"}]}]}}"#,
        )
        .unwrap();
        let out = s.run(&["install", "claude"]).await;
        assert_eq!(out.code, 0, "{}", out.output);
        assert!(has_our_hook(&s.claude_settings()));
        let status = s.run(&["status"]).await.output;
        assert_eq!(status["providers"]["claude"]["installed"], true);
        assert_eq!(status["providers"]["codex"]["installed"], false);

        // The old version's uninstaller.
        let out = s.run(&["uninstall", "--remember"]).await;
        assert_eq!(out.code, 0, "{}", out.output);
        assert_eq!(out.output["providers"]["claude"]["removed"], true);
        assert!(
            out.output["providers"].get("codex").is_none(),
            "nothing to remove there"
        );
        let settings = s.claude_settings();
        assert!(!has_our_hook(&settings));
        assert_eq!(settings["model"], "opus");
        assert!(settings.to_string().contains("notify-me"));

        // The new version's installer.
        let out = s.run(&["restore"]).await;
        assert_eq!(out.code, 0, "{}", out.output);
        assert!(has_our_hook(&s.claude_settings()));
        assert!(
            !s.paths.data_dir.join(MARKER).exists(),
            "the note is used once"
        );
        // Nothing noted any more: a second restore does nothing.
        let out = s.run(&["restore"]).await;
        assert_eq!(out.output["providers"], json!({}));
    }

    #[tokio::test]
    async fn a_plain_uninstall_is_not_undone_later() {
        let s = Setup::new("plain");
        s.run(&["install", "claude"]).await;
        // Uninstalled without --remember: nothing to restore.
        s.run(&["uninstall"]).await;
        assert!(!has_our_hook(&s.claude_settings()));
        s.run(&["restore"]).await;
        assert!(!has_our_hook(&s.claude_settings()));

        // A note older than an hour is ignored (and removed).
        s.run(&["install", "claude"]).await;
        s.run(&["uninstall", "--remember"]).await;
        let old = Marker {
            at: now_ms() - RESTORE_WINDOW_MS - 1,
            providers: vec!["claude".into()],
        };
        std::fs::write(
            s.paths.data_dir.join(MARKER),
            serde_json::to_string(&old).unwrap(),
        )
        .unwrap();
        s.run(&["restore"]).await;
        assert!(!has_our_hook(&s.claude_settings()));
        assert!(!s.paths.data_dir.join(MARKER).exists());
    }

    #[tokio::test]
    async fn usage_errors() {
        let s = Setup::new("usage");
        assert_eq!(s.run(&["install"]).await.code, 2);
        assert_eq!(s.run(&["install", "nobody"]).await.code, 2);
        assert_eq!(s.run(&["frobnicate"]).await.code, 2);
    }
}
