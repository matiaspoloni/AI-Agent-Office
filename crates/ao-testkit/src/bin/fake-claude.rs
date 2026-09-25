//! A stand-in for the `claude` CLI used by end-to-end tests. It implements the
//! small documented surface Agent Office relies on — `--version`,
//! `agents --json`, `stop <id>`, and headless `-p` with stream-json I/O — and
//! runs configured hooks (exec form) exactly like Claude Code: JSON on stdin,
//! async handlers not awaited, synchronous handlers' stdout read back.
//!
//! `--resume <id>` continues a session this fake started earlier with the
//! same config folder (it keeps a marker per session id); an unknown id gets
//! the reply recorded from claude 2.1.282: "No conversation found with
//! session ID: <id>" on stderr and an `error_during_execution` result.
//!
//! Extra mode for tests of the global integration:
//!   fake-claude --simulate-external <session-id>
//! runs a scripted external session using only the user settings hooks and
//! prints the PermissionRequest decision it received.

use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

fn config_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".claude")))
}

fn session_marker(id: &str) -> Option<std::path::PathBuf> {
    config_dir().map(|d| d.join("fake-sessions").join(id))
}

fn user_settings() -> Value {
    config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}))
}

/// Hook entries merge across settings levels (documented behaviour).
fn merged_hooks(sources: &[Value]) -> Value {
    let mut merged = serde_json::Map::new();
    for source in sources {
        if let Some(hooks) = source.get("hooks").and_then(Value::as_object) {
            for (event, groups) in hooks {
                let entry = merged.entry(event.clone()).or_insert_with(|| json!([]));
                if let (Some(dst), Some(src)) = (entry.as_array_mut(), groups.as_array()) {
                    dst.extend(src.iter().cloned());
                }
            }
        }
    }
    Value::Object(merged)
}

struct Session {
    id: String,
    hooks: Value,
    cwd: String,
}

impl Session {
    /// Runs every handler for `event`; returns the stdout of synchronous ones.
    fn fire(&self, event: &str, extra: Value) -> Vec<String> {
        let mut payload = json!({
            "session_id": self.id,
            "transcript_path": "/tmp/fake-claude/transcript.jsonl",
            "cwd": self.cwd,
            "permission_mode": "default",
            "hook_event_name": event,
        });
        if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                p.insert(k.clone(), v.clone());
            }
        }
        let mut outputs = Vec::new();
        let groups = self
            .hooks
            .get(event)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for handler in groups
            .iter()
            .filter_map(|g| g.get("hooks").and_then(Value::as_array))
            .flatten()
        {
            let (Some(command), Some(args)) =
                (handler["command"].as_str(), handler["args"].as_array())
            else {
                continue; // shell form is not needed by the tests
            };
            let args: Vec<&str> = args.iter().filter_map(Value::as_str).collect();
            let Ok(mut child) = Command::new(command)
                .args(&args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            else {
                continue;
            };
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(payload.to_string().as_bytes());
            }
            if handler["async"].as_bool() == Some(true) {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                continue;
            }
            let timeout = Duration::from_secs(handler["timeout"].as_u64().unwrap_or(600));
            let started = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    _ if started.elapsed() > timeout => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    _ => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            let mut out = String::new();
            if let Some(mut stdout) = child.stdout.take() {
                let _ = stdout.read_to_string(&mut out);
            }
            if !out.trim().is_empty() {
                outputs.push(out);
            }
        }
        outputs
    }
}

fn decision(outputs: &[String]) -> Option<(String, Option<String>)> {
    outputs.iter().find_map(|o| {
        let v: Value = serde_json::from_str(o).ok()?;
        let d = v.pointer("/hookSpecificOutput/decision")?;
        Some((
            d["behavior"].as_str()?.to_owned(),
            d["message"].as_str().map(str::to_owned),
        ))
    })
}

fn emit(line: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn headless(args: &[String]) {
    let resumed = arg_value(args, "--resume");
    let id = resumed
        .clone()
        .or_else(|| arg_value(args, "--session-id"))
        .unwrap_or_else(|| "fake-session".into());
    let known = session_marker(&id).is_some_and(|m| m.exists());
    if resumed.is_some() && !known {
        let message = format!("No conversation found with session ID: {id}");
        eprintln!("{message}");
        emit(json!({
            "type": "result", "subtype": "error_during_execution", "duration_ms": 0, "duration_api_ms": 0,
            "is_error": true, "num_turns": 0, "stop_reason": null, "session_id": id, "total_cost_usd": 0,
            "usage": { "input_tokens": 0, "output_tokens": 0 }, "modelUsage": {}, "permission_denials": [],
            "errors": [message]
        }));
        std::process::exit(1);
    }
    if let Some(marker) = session_marker(&id) {
        let _ = std::fs::create_dir_all(marker.parent().unwrap());
        let _ = std::fs::write(marker, "");
    }
    let flag_settings = arg_value(args, "--settings")
        .map(|s| std::fs::read_to_string(&s).unwrap_or(s))
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or_else(|| json!({}));
    let model = arg_value(args, "--model").unwrap_or_else(|| "claude-fake-1".into());
    let cwd = std::env::current_dir().unwrap().display().to_string();
    let session = Session {
        id: id.clone(),
        hooks: merged_hooks(&[user_settings(), flag_settings]),
        cwd: cwd.clone(),
    };

    session.fire(
        "SessionStart",
        json!({ "source": if resumed.is_some() { "resume" } else { "startup" } }),
    );
    emit(
        json!({ "type": "system", "subtype": "init", "session_id": id, "cwd": cwd, "model": model, "permissionMode": "default", "claude_code_version": "2.1.281" }),
    );

    let mut turn = 0;
    let mut tokens_in = 0u64;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if msg["type"] != "user" {
            continue;
        }
        turn += 1;
        let prompt = msg
            .pointer("/message/content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        session.fire("UserPromptSubmit", json!({ "prompt": prompt }));
        let text = if prompt.contains("permission") {
            let tool_id = format!("toolu_fake_{turn}");
            let input = json!({ "command": "rm -rf build", "description": "Clean" });
            session.fire(
                "PreToolUse",
                json!({ "tool_name": "Bash", "tool_input": input, "tool_use_id": tool_id }),
            );
            let answer = decision(&session.fire(
                "PermissionRequest",
                json!({ "tool_name": "Bash", "tool_input": input }),
            ));
            match answer {
                Some((behavior, _)) if behavior == "allow" => {
                    session.fire("PostToolUse", json!({ "tool_name": "Bash", "tool_input": input, "tool_response": { "stdout": "" }, "tool_use_id": tool_id, "duration_ms": 5 }));
                    "Command ran".to_owned()
                }
                Some((_, message)) => format!("Permission denied: {}", message.unwrap_or_default()),
                None => "Permission denied: no decision".to_owned(),
            }
        } else {
            let tool_id = format!("toolu_fake_{turn}");
            let input = json!({ "file_path": format!("{cwd}/README.md") });
            session.fire(
                "PreToolUse",
                json!({ "tool_name": "Read", "tool_input": input, "tool_use_id": tool_id }),
            );
            session.fire("PostToolUse", json!({ "tool_name": "Read", "tool_input": input, "tool_response": {}, "tool_use_id": tool_id, "duration_ms": 1 }));
            "Read README.md".to_owned()
        };
        emit(
            json!({ "type": "assistant", "session_id": id, "parent_tool_use_id": null, "message": { "role": "assistant", "content": [{ "type": "text", "text": text }] } }),
        );
        tokens_in += 1000;
        emit(json!({
            "type": "result", "subtype": "success", "session_id": id, "is_error": false, "num_turns": turn,
            "duration_ms": 10, "duration_api_ms": 5, "result": text, "stop_reason": "end_turn",
            "total_cost_usd": 0.001 * turn as f64,
            "usage": { "input_tokens": 1000, "output_tokens": 100 },
            "modelUsage": { model.clone(): { "inputTokens": tokens_in, "outputTokens": 100 * turn, "cacheReadInputTokens": 0, "cacheCreationInputTokens": 0, "webSearchRequests": 0, "costUSD": 0.001 * turn as f64, "contextWindow": 200000, "maxOutputTokens": 8000 } },
            "permission_denials": []
        }));
        session.fire(
            "Stop",
            json!({ "stop_hook_active": false, "last_assistant_message": text }),
        );
    }
    session.fire("SessionEnd", json!({ "reason": "other" }));
}

fn simulate_external(id: &str) {
    let cwd = std::env::current_dir().unwrap().display().to_string();
    let session = Session {
        id: id.to_owned(),
        hooks: merged_hooks(&[user_settings()]),
        cwd: cwd.clone(),
    };
    let pause = || std::thread::sleep(Duration::from_millis(50));
    session.fire(
        "SessionStart",
        json!({ "source": "startup", "model": "claude-fake-1" }),
    );
    pause();
    session.fire("UserPromptSubmit", json!({ "prompt": "external prompt" }));
    pause();
    let input =
        json!({ "file_path": format!("{cwd}/src/main.rs"), "old_string": "a", "new_string": "b" });
    session.fire(
        "PreToolUse",
        json!({ "tool_name": "Edit", "tool_input": input, "tool_use_id": "toolu_ext_1" }),
    );
    let answer = decision(&session.fire(
        "PermissionRequest",
        json!({ "tool_name": "Edit", "tool_input": input }),
    ));
    emit(json!({ "fakeDecision": answer.map(|(b, _)| b) }));
    session.fire("PostToolUse", json!({ "tool_name": "Edit", "tool_input": input, "tool_response": {}, "tool_use_id": "toolu_ext_1" }));
    pause();
    session.fire(
        "Stop",
        json!({ "stop_hook_active": false, "last_assistant_message": "Edited main.rs" }),
    );
    pause();
    session.fire("SessionEnd", json!({ "reason": "prompt_input_exit" }));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") | Some("-v") => println!("2.1.281 (Claude Code)"),
        Some("agents") => println!(
            "{}",
            std::env::var("FAKE_CLAUDE_AGENTS").unwrap_or_else(|_| "[]".into())
        ),
        Some("stop") => {}
        Some("--simulate-external") => simulate_external(
            args.get(1)
                .map(String::as_str)
                .unwrap_or("external-session"),
        ),
        _ if args.iter().any(|a| a == "-p" || a == "--print") => headless(&args),
        _ => {
            eprintln!("fake-claude: unsupported invocation {args:?}");
            std::process::exit(2);
        }
    }
}
