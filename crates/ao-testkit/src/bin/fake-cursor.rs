//! A stand-in for the Cursor CLI (`agent`) used by end-to-end tests. It
//! speaks the Agent Client Protocol (ACP, protocol version 1) as the official
//! schema describes it (`@agentclientprotocol/sdk` 1.5.0), plus the
//! Cursor-specific details integrators report and Agent Office tolerates.
//! None of them were observed from a real Cursor CLI (it could not be
//! installed where this was written; see docs/PROVIDER_CAPABILITIES.md §5):
//!
//! * auth method `cursor_login` only and no `agentInfo`; while "logged out"
//!   (here: while `<cwd>/.fake-cursor-logged-out` exists) `session/new`
//!   fails with -32000 until `authenticate` succeeds, and `authenticate`
//!   itself fails while `<cwd>/.fake-cursor-login-fails` exists;
//! * `session/new` returns modes agent / plan / ask, the older `models` state
//!   (answered by `session/set_model`, ids such as `composer-2.5[fast=true]`)
//!   and config options for mode and model — or only `models` while
//!   `<cwd>/.fake-cursor-legacy-models` exists;
//! * hyphenated permission option ids (`allow-once`, …);
//! * a `cursor/create_plan` request the client has to answer. Only the method
//!   name is reported; its parameters here are placeholders.
//!
//! Scripted turns are chosen by words in the prompt: `permission`, `edit`,
//! `plan`, `wait` (runs until cancelled) and `fail`; anything else gets a
//! short text answer.

use serde_json::{json, Value};
use std::io::{BufRead, Lines, StdinLock, Write};
use std::path::PathBuf;

const VERSION: &str = "2026.09.18-fake";

fn send(message: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}

fn respond(id: &Value, result: Value) {
    send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
}

fn error(id: &Value, code: i64, message: &str) {
    send(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }));
}

/// `(id, name)` of the models offered.
const MODELS: &[(&str, &str)] = &[
    ("default[]", "Auto"),
    ("gpt-5", "GPT-5"),
    ("composer-2.5[fast=true]", "Composer 2.5 Fast"),
    ("sonnet-4.5", "Claude Sonnet 4.5"),
];

const MODES: &[(&str, &str)] = &[("agent", "Agent"), ("plan", "Plan"), ("ask", "Ask")];

fn marker(name: &str) -> bool {
    std::env::current_dir().is_ok_and(|d| d.join(name).exists())
}

/// How a scripted turn ends.
enum Stop {
    Reason(&'static str),
    Error(i64, String),
}

struct Agent {
    lines: Lines<StdinLock<'static>>,
    next_request: i64,
    deferred: Vec<Value>,
    session: Option<String>,
    cwd: PathBuf,
    logged_in: bool,
    model: String,
    mode: String,
    cancelled: bool,
    used: u64,
    turns: u32,
}

impl Agent {
    fn read(&mut self) -> Option<Value> {
        loop {
            let line = self.lines.next()?.ok()?;
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                return Some(v);
            }
        }
    }

    fn update(&self, update: Value) {
        send(json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "sessionId": self.session, "update": update }
        }));
    }

    fn text(&self, text: &str) {
        self.update(json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } }));
    }

    fn path(&self, name: &str) -> String {
        self.cwd.join(name).display().to_string()
    }

    fn config_options(&self) -> Value {
        let select = |values: &[(&str, &str)]| {
            values
                .iter()
                .map(|(value, name)| json!({ "value": value, "name": name }))
                .collect::<Vec<_>>()
        };
        json!([
            { "id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": self.mode, "options": select(MODES) },
            { "id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": self.model, "options": select(MODELS) }
        ])
    }

    fn set_model(&mut self, id: &Value, model: &str) -> bool {
        if MODELS.iter().any(|(m, _)| *m == model) {
            self.model = model.to_owned();
            true
        } else {
            error(id, -32602, "unknown model");
            false
        }
    }

    /// Sends an agent → client request and waits for its answer (`Err` holds
    /// a JSON-RPC error). A `session/cancel` meanwhile is remembered: ACP
    /// requires the client to still answer, with `cancelled`.
    fn ask(&mut self, method: &str, params: Value) -> Option<Result<Value, Value>> {
        let id = self.next_request;
        self.next_request += 1;
        send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let message = self.read()?;
            if message.get("method").is_none() && message["id"] == json!(id) {
                return Some(match message.get("error") {
                    Some(e) => Err(e.clone()),
                    None => Ok(message["result"].clone()),
                });
            }
            if message["method"] == "session/cancel" {
                self.cancelled = true;
                continue;
            }
            self.deferred.push(message);
        }
    }

    /// A long-running step: waits until the client cancels the turn.
    fn wait_for_cancel(&mut self) -> Option<()> {
        while !self.cancelled {
            let message = self.read()?;
            if message["method"] == "session/cancel" {
                self.cancelled = true;
            } else {
                self.deferred.push(message);
            }
        }
        Some(())
    }

    fn usage(&mut self, add: u64) {
        self.used += add;
        self.update(json!({
            "sessionUpdate": "usage_update",
            "used": self.used,
            "size": 200000,
            "cost": { "amount": self.used as f64 / 1_000_000.0, "currency": "USD" }
        }));
    }

    /// Asks permission for a tool call. `Ok(true)` allowed, `Ok(false)`
    /// rejected, `Err(stop)` the turn has to end.
    fn permission(&mut self, tool_call: Value) -> Option<Result<bool, Stop>> {
        let answer = self.ask(
            "session/request_permission",
            json!({
                "sessionId": self.session,
                "toolCall": tool_call,
                "options": [
                    { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                    { "optionId": "allow-always", "name": "Always allow", "kind": "allow_always" },
                    { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" }
                ]
            }),
        )?;
        let Ok(answer) = answer else {
            return Some(Err(Stop::Error(-32603, "permission request failed".into())));
        };
        Some(
            match answer.pointer("/outcome/outcome").and_then(Value::as_str) {
                Some("cancelled") => Err(Stop::Reason("cancelled")),
                Some("selected") => {
                    match answer.pointer("/outcome/optionId").and_then(Value::as_str) {
                        Some("allow-once" | "allow-always") => Ok(true),
                        Some("reject-once") => Ok(false),
                        other => Err(Stop::Error(
                            -32602,
                            format!("fake-cursor: unknown optionId {other:?}"),
                        )),
                    }
                }
                _ => Err(Stop::Error(
                    -32602,
                    format!("fake-cursor: malformed permission answer {answer}"),
                )),
            },
        )
    }

    fn turn(&mut self, prompt: &str) -> Option<Stop> {
        self.cancelled = false;
        self.turns += 1;
        let turn = self.turns;
        let id = move |name: &str| format!("call-{name}-{turn}");
        self.update(json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "Reading the request" } }));
        if prompt.contains("fail") {
            return Some(Stop::Error(
                -32603,
                "fake-cursor: the model is unavailable".into(),
            ));
        }
        if prompt.contains("permission") {
            let call = json!({ "toolCallId": id("run"), "title": "`cargo test`", "kind": "execute", "status": "pending", "rawInput": { "command": "cargo test" } });
            self.update(json!({ "sessionUpdate": "tool_call" }).merged(&call));
            match self.permission(call)? {
                Err(stop) => return Some(stop),
                Ok(true) => {
                    self.update(json!({ "sessionUpdate": "tool_call_update", "toolCallId": id("run"), "status": "in_progress" }));
                    self.update(json!({ "sessionUpdate": "tool_call_update", "toolCallId": id("run"), "status": "completed",
                        "content": [{ "type": "content", "content": { "type": "text", "text": "test result: ok. 3 passed" } }] }));
                    self.text("Tests pass.");
                }
                Ok(false) => {
                    self.update(json!({ "sessionUpdate": "tool_call_update", "toolCallId": id("run"), "status": "failed",
                        "content": [{ "type": "content", "content": { "type": "text", "text": "Rejected by the user" } }] }));
                    self.text("Okay, I did not run it.");
                }
            }
            self.usage(900);
            return Some(Stop::Reason("end_turn"));
        }
        if prompt.contains("edit") {
            let readme = self.path("README.md");
            self.update(json!({ "sessionUpdate": "tool_call", "toolCallId": id("read"), "title": "Read README.md", "kind": "read",
                "status": "completed", "locations": [{ "path": readme }] }));
            let lib = self.path("src/lib.rs");
            let call = json!({ "toolCallId": id("edit"), "title": "Edit", "kind": "edit", "status": "pending",
                "locations": [{ "path": lib }],
                "content": [{ "type": "diff", "path": lib, "oldText": "fn a() {}", "newText": "fn a() { b() }" }] });
            self.update(json!({ "sessionUpdate": "tool_call" }).merged(&call));
            match self.permission(call)? {
                Err(stop) => return Some(stop),
                Ok(allowed) => {
                    let status = if allowed { "completed" } else { "failed" };
                    self.update(json!({ "sessionUpdate": "tool_call_update", "toolCallId": id("edit"), "status": status }));
                }
            }
            let new_file = self.path("src/new.rs");
            self.update(json!({ "sessionUpdate": "tool_call", "toolCallId": id("new"), "title": "Create src/new.rs", "kind": "edit",
                "status": "completed", "content": [{ "type": "diff", "path": new_file, "oldText": null, "newText": "pub fn b() {}" }] }));
            self.text("Edited the code.");
            self.usage(2500);
            return Some(Stop::Reason("end_turn"));
        }
        if prompt.contains("plan") {
            self.update(json!({ "sessionUpdate": "plan", "entries": [
                { "content": "Read the code", "priority": "high", "status": "completed" },
                { "content": "Write the tests", "priority": "medium", "status": "pending" }
            ] }));
            // Cursor extension request (placeholder parameters).
            let answer = self.ask(
                "cursor/create_plan",
                json!({ "sessionId": self.session, "plan": "1. Read 2. Test" }),
            )?;
            self.text(if answer.is_ok() {
                "Plan accepted."
            } else {
                "The plan could not be shown; continuing."
            });
            return Some(Stop::Reason("end_turn"));
        }
        if prompt.contains("wait") {
            self.update(json!({ "sessionUpdate": "tool_call", "toolCallId": id("search"), "title": "Search the codebase", "kind": "search", "status": "in_progress" }));
            self.text("Working on a long task");
            self.wait_for_cancel()?;
            return Some(Stop::Reason("cancelled"));
        }
        self.update(
            json!({ "sessionUpdate": "available_commands_update", "availableCommands": [] }),
        );
        self.text("Hello ");
        self.text("from fake Cursor.");
        self.usage(1200);
        Some(Stop::Reason("end_turn"))
    }

    fn handle(&mut self, message: Value) -> Option<()> {
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let id = message.get("id").cloned().filter(|v| !v.is_null());
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            // Notifications: `session/cancel` outside a turn has nothing to cancel.
            return Some(());
        };
        match method.as_str() {
            "initialize" => respond(
                &id,
                json!({
                    "protocolVersion": 1,
                    "agentCapabilities": {
                        "loadSession": true,
                        "promptCapabilities": { "image": true, "audio": false, "embeddedContext": false },
                        "mcpCapabilities": { "http": true, "sse": false }
                    },
                    "authMethods": [{ "id": "cursor_login", "name": "Cursor Login", "description": "Log in with your Cursor account" }]
                }),
            ),
            "authenticate" => {
                if marker(".fake-cursor-login-fails") {
                    error(&id, -32000, "Not logged in. Run `agent login`.");
                } else if params["methodId"] == "cursor_login" {
                    self.logged_in = true;
                    respond(&id, json!({}));
                } else {
                    error(&id, -32602, "unknown auth method");
                }
            }
            "session/new" => {
                let cwd = PathBuf::from(params["cwd"].as_str().unwrap_or("."));
                if cwd.join(".fake-cursor-logged-out").exists() && !self.logged_in {
                    error(&id, -32000, "Authentication required");
                    return Some(());
                }
                let legacy_only = cwd.join(".fake-cursor-legacy-models").exists();
                self.cwd = cwd;
                let session = format!("fake-cursor-{}", std::process::id());
                self.session = Some(session.clone());
                let mut result = json!({
                    "sessionId": session,
                    "modes": {
                        "currentModeId": self.mode,
                        "availableModes": MODES.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>()
                    },
                    "models": {
                        "currentModelId": self.model,
                        "availableModels": MODELS.iter().map(|(id, name)| json!({ "modelId": id, "name": name })).collect::<Vec<_>>()
                    }
                });
                if !legacy_only {
                    result["configOptions"] = self.config_options();
                }
                respond(&id, result);
            }
            "session/set_config_option" if params["configId"] == "model" => {
                let value = params["value"].as_str().unwrap_or_default();
                if self.set_model(&id, value) {
                    respond(&id, json!({ "configOptions": self.config_options() }));
                }
            }
            "session/set_config_option" => error(&id, -32602, "invalid config option"),
            "session/set_model" => {
                let model = params["modelId"].as_str().unwrap_or_default();
                if self.set_model(&id, model) {
                    respond(&id, json!({}));
                }
            }
            "session/set_mode" => {
                let mode = params["modeId"].as_str().unwrap_or_default();
                if MODES.iter().any(|(m, _)| *m == mode) {
                    self.mode = mode.to_owned();
                    respond(&id, json!({}));
                    self.update(
                        json!({ "sessionUpdate": "current_mode_update", "currentModeId": mode }),
                    );
                } else {
                    error(&id, -32602, "unknown mode");
                }
            }
            "session/prompt"
                if self.session.is_some() && params["sessionId"] == json!(self.session) =>
            {
                let prompt = params["prompt"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|block| block["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                match self.turn(&prompt)? {
                    Stop::Reason(reason) => respond(
                        &id,
                        json!({ "stopReason": reason, "usage": { "inputTokens": self.used, "outputTokens": 40, "totalTokens": self.used + 40 } }),
                    ),
                    Stop::Error(code, message) => error(&id, code, &message),
                }
            }
            "session/prompt" => error(&id, -32602, "unknown session"),
            _ => error(&id, -32601, &format!("Method not found: {method}")),
        }
        Some(())
    }
}

trait Merged {
    fn merged(self, other: &Value) -> Value;
}

impl Merged for Value {
    fn merged(mut self, other: &Value) -> Value {
        if let (Some(map), Some(extra)) = (self.as_object_mut(), other.as_object()) {
            for (k, v) in extra {
                map.insert(k.clone(), v.clone());
            }
        }
        self
    }
}

fn acp() {
    eprintln!("fake-cursor {VERSION}: ACP on stdio");
    let mut agent = Agent {
        lines: std::io::stdin().lock().lines(),
        next_request: 0,
        deferred: Vec::new(),
        session: None,
        cwd: PathBuf::from("."),
        logged_in: false,
        model: "default[]".into(),
        mode: "agent".into(),
        cancelled: false,
        used: 0,
        turns: 0,
    };
    loop {
        let message = if agent.deferred.is_empty() {
            agent.read()
        } else {
            Some(agent.deferred.remove(0))
        };
        let Some(message) = message else { break };
        if agent.handle(message).is_none() {
            break;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("{VERSION}"),
        Some("acp") => acp(),
        _ => {
            eprintln!("fake-cursor: unsupported invocation {args:?}");
            std::process::exit(2);
        }
    }
}
