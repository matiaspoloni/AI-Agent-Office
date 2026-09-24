//! A stand-in for the `codex` CLI used by end-to-end tests. It implements the
//! small surface Agent Office relies on, with message shapes copied from
//! codex-cli 0.156.1 (see fixtures/codex/*-real.json):
//!
//! * `--version`
//! * `app-server`: JSON-RPC over stdio — `initialize`, `hooks/list`,
//!   `thread/start`, `turn/start`, `turn/steer`, `turn/interrupt`; scripted
//!   turns with command/file approvals and a subagent;
//! * user hooks from `$CODEX_HOME/hooks.json`, run through the shell exactly
//!   like Codex (`cmd.exe /C "<line>"` on Windows, `$SHELL -lc` elsewhere),
//!   and only when trusted — here: when `$CODEX_HOME/fake-trust-all` exists
//!   (standing in for the user trusting them with `/hooks`).
//!
//! `fake-codex --simulate-external <session-id>` runs a scripted external
//! session through the hooks and prints `{"fakeDecision": …}`.

use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const VERSION: &str = "0.156.1";

fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".codex"))
}

fn hooks_file() -> PathBuf {
    codex_home().join("hooks.json")
}

fn trusted() -> bool {
    codex_home().join("fake-trust-all").exists()
}

fn snake(event: &str) -> String {
    let mut out = String::new();
    for (i, c) in event.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn handlers(event: &str) -> Vec<(String, Value)> {
    let file: Value = std::fs::read_to_string(hooks_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    let mut out = Vec::new();
    let groups = file
        .pointer(&format!("/hooks/{event}"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (g, group) in groups.iter().enumerate() {
        for (h, handler) in group["hooks"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            let key = format!("{}:{}:{g}:{h}", hooks_file().display(), snake(event));
            out.push((key, handler));
        }
    }
    out
}

fn shell(line: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));
        c.arg("/C");
        c.raw_arg(format!("\"{line}\""));
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
        c.arg("-lc").arg(line);
        c
    }
}

/// Runs trusted handlers for `event`; returns stdout of synchronous ones.
fn fire(event: &str, payload: Value, cwd: &str) -> Vec<String> {
    if !trusted() {
        return Vec::new();
    }
    let mut outputs = Vec::new();
    for (_, handler) in handlers(event) {
        if handler["type"] != "command" {
            continue;
        }
        let Some(line) = handler["command"].as_str() else {
            continue;
        };
        let Ok(mut child) = shell(line)
            .current_dir(cwd)
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
        // SessionEnd always runs synchronously (as in Codex).
        if handler["async"].as_bool() == Some(true) && event != "SessionEnd" {
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

fn hook_payload(session: &str, event: &str, cwd: &str, extra: Value) -> Value {
    let mut p = json!({
        "session_id": session,
        "transcript_path": null,
        "cwd": cwd,
        "hook_event_name": event,
        "model": "gpt-fake",
        "permission_mode": "default",
    });
    if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            p.insert(k.clone(), v.clone());
        }
    }
    p
}

fn decision(outputs: &[String]) -> Option<String> {
    outputs.iter().find_map(|o| {
        let v: Value = serde_json::from_str(o).ok()?;
        v.pointer("/hookSpecificOutput/decision/behavior")?
            .as_str()
            .map(str::to_owned)
    })
}

// ------------------------------------------------------------------ app-server

struct Server {
    lines: std::io::Lines<std::io::StdinLock<'static>>,
    next_request: i64,
    thread: Option<String>,
    cwd: String,
    model: String,
    turn_seq: u32,
    usage: u64,
    session_started: bool,
    /// Requests that arrived while an approval was outstanding.
    deferred: Vec<Value>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn send(message: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}

fn notify(method: &str, params: Value) {
    send(json!({ "method": method, "params": params, "emittedAtMs": now_ms() }));
}

fn respond(id: &Value, result: Value) {
    send(json!({ "id": id, "result": result }));
}

impl Server {
    fn read(&mut self) -> Option<Value> {
        loop {
            let line = self.lines.next()?.ok()?;
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                return Some(v);
            }
        }
    }

    /// Sends a server request and waits for its response. Returns `None`
    /// when the turn was interrupted meanwhile.
    fn ask(&mut self, method: &str, params: Value) -> Option<Value> {
        let id = self.next_request;
        self.next_request += 1;
        send(json!({ "method": method, "id": id, "params": params }));
        loop {
            let message = self.read()?;
            if message.get("method").is_none() && message["id"] == json!(id) {
                return Some(message["result"].clone());
            }
            if message["method"] == "turn/interrupt" {
                respond(&message["id"], json!({}));
                notify(
                    "serverRequest/resolved",
                    json!({ "threadId": self.thread, "requestId": id }),
                );
                return None;
            }
            self.deferred.push(message);
        }
    }

    fn item(&self, thread: &str, turn: &str, item: Value) {
        notify(
            "item/started",
            json!({ "item": item, "threadId": thread, "turnId": turn, "startedAtMs": now_ms() }),
        );
    }

    fn done(&self, thread: &str, turn: &str, item: Value) {
        notify(
            "item/completed",
            json!({ "item": item, "threadId": thread, "turnId": turn, "completedAtMs": now_ms() }),
        );
    }

    fn usage_update(&mut self, thread: &str, turn: &str, add: u64) -> u64 {
        self.usage += add;
        let total = json!({ "totalTokens": self.usage + 10, "inputTokens": self.usage, "cachedInputTokens": 0, "cacheWriteInputTokens": 0, "outputTokens": 10, "reasoningOutputTokens": 0 });
        notify(
            "thread/tokenUsage/updated",
            json!({ "threadId": thread, "turnId": turn, "tokenUsage": { "total": total, "last": total, "modelContextWindow": 200000 } }),
        );
        self.usage
    }

    fn turn(&mut self, prompt: &str) -> Option<()> {
        let thread = self.thread.clone()?;
        self.turn_seq += 1;
        let turn = format!("turn-{}", self.turn_seq);
        let cwd = self.cwd.clone();
        notify(
            "turn/started",
            json!({ "threadId": thread, "turn": { "id": turn, "items": [], "itemsView": "notLoaded", "status": "inProgress", "error": null, "startedAt": 1, "completedAt": null, "durationMs": null } }),
        );
        if !self.session_started {
            self.session_started = true;
            fire(
                "SessionStart",
                hook_payload(
                    &thread,
                    "SessionStart",
                    &cwd,
                    json!({ "source": "startup" }),
                ),
                &cwd,
            );
        }
        fire(
            "UserPromptSubmit",
            hook_payload(
                &thread,
                "UserPromptSubmit",
                &cwd,
                json!({ "turn_id": turn, "prompt": prompt }),
            ),
            &cwd,
        );
        let user = json!({ "type": "userMessage", "id": format!("u-{turn}"), "clientId": null, "content": [{ "type": "text", "text": prompt, "text_elements": [] }] });
        self.item(&thread, &turn, user.clone());
        self.done(&thread, &turn, user);

        let mut reply = "Hello from fake codex".to_owned();
        if prompt.contains("permission") {
            let cmd_id = format!("call-cmd-{turn}");
            let item = |status: &str, exit: Value| json!({ "type": "commandExecution", "id": cmd_id, "command": "/bin/bash -lc 'rm -rf build'", "cwd": cwd, "processId": null, "source": "agent", "status": status, "commandActions": [{ "type": "unknown", "command": "rm -rf build" }], "aggregatedOutput": null, "exitCode": exit, "durationMs": 3 });
            self.item(&thread, &turn, item("inProgress", Value::Null));
            let answer = self.ask("item/commandExecution/requestApproval", json!({ "kind": "command", "threadId": thread, "turnId": turn, "itemId": cmd_id, "startedAtMs": now_ms(), "environmentId": "local", "command": "/bin/bash -lc 'rm -rf build'", "cwd": cwd, "commandActions": [{ "type": "unknown", "command": "rm -rf build" }] }));
            let Some(answer) = answer else {
                return self.finish(&thread, &turn, "interrupted");
            };
            if answer["decision"] == "accept" || answer["decision"] == "acceptForSession" {
                self.done(&thread, &turn, item("completed", json!(0)));
                reply = "Command ran".into();
            } else {
                self.done(&thread, &turn, item("declined", Value::Null));
                reply = "Command declined".into();
            }
        }
        if prompt.contains("edit") {
            let patch_id = format!("call-patch-{turn}");
            let path = format!("{}/hello.txt", cwd.replace('\\', "/"));
            let item = |status: &str| json!({ "type": "fileChange", "id": patch_id, "changes": [{ "path": path, "kind": { "type": "add" }, "diff": "hi\n" }], "status": status });
            self.item(&thread, &turn, item("inProgress"));
            let answer = self.ask("item/fileChange/requestApproval", json!({ "threadId": thread, "turnId": turn, "itemId": patch_id, "startedAtMs": now_ms(), "reason": null, "grantRoot": null }));
            let Some(answer) = answer else {
                return self.finish(&thread, &turn, "interrupted");
            };
            let ok = answer["decision"] == "accept";
            self.done(
                &thread,
                &turn,
                item(if ok { "completed" } else { "declined" }),
            );
            reply = if ok {
                "File written".into()
            } else {
                "Edit declined".into()
            };
        }
        if prompt.contains("subagent") {
            let child = format!("{thread}-child");
            let spawn = |status: &str, receivers: Value| json!({ "type": "collabAgentToolCall", "id": "call-spawn", "tool": "spawnAgent", "status": status, "senderThreadId": thread, "receiverThreadIds": receivers, "prompt": "Count the files", "model": "gpt-fake", "reasoningEffort": "medium", "agentsStates": {} });
            self.item(&thread, &turn, spawn("inProgress", json!([])));
            self.done(&thread, &turn, spawn("completed", json!([child])));
            let child_turn = format!("{turn}-child");
            notify(
                "turn/started",
                json!({ "threadId": child, "turn": { "id": child_turn, "items": [], "itemsView": "notLoaded", "status": "inProgress", "error": null, "startedAt": 1, "completedAt": null, "durationMs": null } }),
            );
            let msg = json!({ "type": "agentMessage", "id": "m-child", "text": "3 files", "phase": null, "memoryCitation": null, "delivery": null, "questions": null });
            self.done(&child, &child_turn, msg);
            self.usage_update(&child, &child_turn, 0);
            notify(
                "turn/completed",
                json!({ "threadId": child, "turn": { "id": child_turn, "items": [], "itemsView": "summary", "status": "completed", "error": null, "startedAt": 1, "completedAt": 2, "durationMs": 1 } }),
            );
        }
        let msg = json!({ "type": "agentMessage", "id": format!("m-{turn}"), "text": reply, "phase": null, "memoryCitation": null, "delivery": null, "questions": null });
        self.item(&thread, &turn, msg.clone());
        self.done(&thread, &turn, msg);
        self.usage_update(&thread, &turn, 1000);
        fire(
            "Stop",
            hook_payload(
                &thread,
                "Stop",
                &cwd,
                json!({ "turn_id": turn, "stop_hook_active": false, "last_assistant_message": reply }),
            ),
            &cwd,
        );
        self.finish(&thread, &turn, "completed")
    }

    fn finish(&self, thread: &str, turn: &str, status: &str) -> Option<()> {
        notify(
            "thread/status/changed",
            json!({ "threadId": thread, "status": { "type": "idle" } }),
        );
        notify(
            "turn/completed",
            json!({ "threadId": thread, "turn": { "id": turn, "items": [], "itemsView": "summary", "status": status, "error": null, "startedAt": 1, "completedAt": 2, "durationMs": 1 } }),
        );
        Some(())
    }

    fn handle(&mut self, message: Value) -> Option<()> {
        let id = message.get("id").cloned();
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (method.as_str(), id) {
            ("initialize", Some(id)) => {
                let name = params
                    .pointer("/clientInfo/name")
                    .and_then(Value::as_str)
                    .unwrap_or("client");
                respond(
                    &id,
                    json!({ "userAgent": format!("{name}/{VERSION} (fake) test"), "codexHome": codex_home(), "platformFamily": std::env::consts::FAMILY, "platformOs": std::env::consts::OS }),
                );
            }
            ("hooks/list", Some(id)) => {
                let mut hooks = Vec::new();
                for event in [
                    "PreToolUse",
                    "PermissionRequest",
                    "PostToolUse",
                    "PreCompact",
                    "PostCompact",
                    "SessionStart",
                    "SessionEnd",
                    "UserPromptSubmit",
                    "SubagentStart",
                    "SubagentStop",
                    "Stop",
                    "Interrupt",
                ] {
                    for (key, handler) in handlers(event) {
                        let mut name: Vec<char> = event.chars().collect();
                        name[0] = name[0].to_ascii_lowercase();
                        hooks.push(json!({ "key": key, "eventName": name.into_iter().collect::<String>(), "handlerType": handler["type"], "command": handler["command"], "async": handler["async"].as_bool().unwrap_or(false), "sourcePath": hooks_file(), "source": "user", "enabled": true, "isManaged": false, "currentHash": "sha256:fake", "trustStatus": if trusted() { "trusted" } else { "untrusted" } }));
                    }
                }
                let cwd = params.pointer("/cwds/0").cloned().unwrap_or(json!("."));
                respond(
                    &id,
                    json!({ "data": [{ "cwd": cwd, "hooks": hooks, "warnings": [], "errors": [] }] }),
                );
            }
            ("thread/start", Some(id)) => {
                let thread = format!("fake-thread-{}", std::process::id());
                self.cwd = params["cwd"].as_str().unwrap_or(".").to_owned();
                if let Some(model) = params["model"].as_str() {
                    self.model = model.to_owned();
                }
                self.thread = Some(thread.clone());
                let t = json!({ "id": thread, "sessionId": thread, "parentThreadId": null, "preview": "", "ephemeral": false, "modelProvider": "openai", "model": self.model, "createdAt": 1, "updatedAt": 1, "status": { "type": "idle" }, "cwd": self.cwd, "cliVersion": VERSION, "source": "vscode", "turns": [] });
                respond(
                    &id,
                    json!({ "thread": t, "model": self.model, "modelProvider": "openai", "cwd": self.cwd, "approvalPolicy": params.get("approvalPolicy").cloned().unwrap_or(json!("on-request")), "sandbox": { "type": "workspaceWrite" }, "reasoningEffort": null }),
                );
                notify("thread/started", json!({ "thread": t }));
            }
            ("turn/start", Some(id)) => {
                let prompt = params
                    .pointer("/input/0/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                respond(
                    &id,
                    json!({ "turn": { "id": format!("turn-{}", self.turn_seq + 1), "items": [], "itemsView": "notLoaded", "status": "inProgress", "error": null, "startedAt": null, "completedAt": null, "durationMs": null } }),
                );
                self.turn(&prompt)?;
            }
            ("turn/steer", Some(id)) => {
                send(json!({ "id": id, "error": { "code": -32600, "message": "no active turn" } }))
            }
            ("turn/interrupt", Some(id)) => respond(&id, json!({})),
            (_, Some(id)) => send(
                json!({ "id": id, "error": { "code": -32601, "message": format!("fake-codex: {method} not implemented") } }),
            ),
            (_, None) => {}
        }
        Some(())
    }
}

fn app_server() {
    let mut server = Server {
        lines: std::io::stdin().lock().lines(),
        next_request: 0,
        thread: None,
        cwd: ".".into(),
        model: "gpt-fake".into(),
        turn_seq: 0,
        usage: 0,
        session_started: false,
        deferred: Vec::new(),
    };
    loop {
        let message = if server.deferred.is_empty() {
            server.read()
        } else {
            Some(server.deferred.remove(0))
        };
        let Some(message) = message else { break };
        if server.handle(message).is_none() {
            break;
        }
    }
    if let Some(thread) = server.thread.clone() {
        let cwd = server.cwd.clone();
        fire(
            "SessionEnd",
            hook_payload(&thread, "SessionEnd", &cwd, json!({ "reason": "other" })),
            &cwd,
        );
    }
}

fn simulate_external(session: &str) {
    let cwd = std::env::current_dir().unwrap().display().to_string();
    let pause = || std::thread::sleep(Duration::from_millis(50));
    let p = |event: &str, extra: Value| hook_payload(session, event, &cwd, extra);
    fire(
        "SessionStart",
        p("SessionStart", json!({ "source": "startup" })),
        &cwd,
    );
    pause();
    fire(
        "UserPromptSubmit",
        p(
            "UserPromptSubmit",
            json!({ "turn_id": "t1", "prompt": "external prompt" }),
        ),
        &cwd,
    );
    pause();
    let input = json!({ "command": "cargo test" });
    fire(
        "PreToolUse",
        p(
            "PreToolUse",
            json!({ "turn_id": "t1", "tool_name": "Bash", "tool_input": input, "tool_use_id": "call_ext_1" }),
        ),
        &cwd,
    );
    let answer = decision(&fire(
        "PermissionRequest",
        p(
            "PermissionRequest",
            json!({ "turn_id": "t1", "tool_name": "Bash", "tool_input": input }),
        ),
        &cwd,
    ));
    println!("{}", json!({ "fakeDecision": answer }));
    fire(
        "PostToolUse",
        p(
            "PostToolUse",
            json!({ "turn_id": "t1", "tool_name": "Bash", "tool_input": input, "tool_response": "ok\n", "tool_use_id": "call_ext_1" }),
        ),
        &cwd,
    );
    pause();
    fire(
        "Stop",
        p(
            "Stop",
            json!({ "turn_id": "t1", "stop_hook_active": false, "last_assistant_message": "Tests pass" }),
        ),
        &cwd,
    );
    pause();
    fire(
        "SessionEnd",
        p("SessionEnd", json!({ "reason": "other" })),
        &cwd,
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("codex-cli {VERSION}"),
        Some("app-server") => app_server(),
        Some("--simulate-external") => simulate_external(
            args.get(1)
                .map(String::as_str)
                .unwrap_or("external-session"),
        ),
        _ => {
            eprintln!("fake-codex: unsupported invocation {args:?}");
            std::process::exit(2);
        }
    }
}
