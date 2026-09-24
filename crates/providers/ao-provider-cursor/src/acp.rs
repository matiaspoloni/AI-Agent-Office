//! Agent Client Protocol (ACP) messages → normalized events.
//!
//! Shapes follow the official schema published with
//! `@agentclientprotocol/sdk` 1.5.0 (protocol version 1): `session/update`
//! notifications (`SessionUpdate` variants), `session/request_permission`
//! requests and the `session/prompt` result. Nothing here is specific to a
//! single agent; Cursor-specific behaviour lives in `lib.rs` and is documented
//! in PROVIDER_CAPABILITIES §5.

use crate::PROVIDER_ID;
use ao_core::event::*;
use ao_core::ids::{PermissionRequestId, SessionId, ToolCallId};
use ao_core::provider::PermissionDecision;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const PROTOCOL_VERSION: u64 = 1;

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|x| !x.is_empty())
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

fn first_line(text: &str, max: usize) -> String {
    clip(text.lines().next().unwrap_or_default(), max)
}

/// ACP `ToolKind` → category.
pub fn category(kind: &str) -> ToolCategory {
    match kind {
        "read" => ToolCategory::Read,
        "edit" | "delete" | "move" => ToolCategory::Edit,
        "search" => ToolCategory::Search,
        "execute" => ToolCategory::Execute,
        "think" => ToolCategory::Think,
        "fetch" => ToolCategory::Fetch,
        _ => ToolCategory::Other,
    }
}

#[derive(Debug, Clone, Default)]
struct Tool {
    kind: String,
    title: String,
    name: Option<String>,
    raw_input: Value,
    locations: Vec<String>,
    /// `(path, created)` from `diff` content.
    diffs: Vec<(String, bool)>,
    text: Option<String>,
    /// Agent Office answered its permission request with reject/cancel.
    rejected: bool,
    finished: bool,
}

impl Tool {
    fn merge(&mut self, update: &Value) {
        if let Some(kind) = s(update, "kind") {
            self.kind = kind.to_owned();
        }
        if let Some(title) = s(update, "title") {
            self.title = title.to_owned();
        }
        if let Some(name) = s(update, "name") {
            self.name = Some(name.to_owned());
        }
        if let Some(input) = update.get("rawInput").filter(|v| !v.is_null()) {
            self.raw_input = input.clone();
        }
        if let Some(locations) = update.get("locations").and_then(Value::as_array) {
            self.locations = locations
                .iter()
                .filter_map(|l| s(l, "path"))
                .map(str::to_owned)
                .collect();
        }
        if let Some(content) = update.get("content").and_then(Value::as_array) {
            self.diffs = content
                .iter()
                .filter(|c| s(c, "type") == Some("diff"))
                .filter_map(|c| {
                    Some((s(c, "path")?.to_owned(), c.get("oldText").is_none_or_null()))
                })
                .collect();
            self.text = content
                .iter()
                .filter(|c| s(c, "type") == Some("content"))
                .filter_map(|c| c.pointer("/content/text").and_then(Value::as_str))
                .map(str::to_owned)
                .next();
        }
    }

    fn tool_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            if self.kind.is_empty() {
                "tool".into()
            } else {
                self.kind.clone()
            }
        })
    }

    /// Command text of an `execute` tool: `rawInput.command` when the agent
    /// sends a string there, otherwise the title.
    fn command(&self) -> String {
        s(&self.raw_input, "command")
            .map(str::to_owned)
            .unwrap_or_else(|| self.title.clone())
    }

    fn paths(&self) -> Vec<String> {
        if self.diffs.is_empty() {
            self.locations.clone()
        } else {
            self.diffs.iter().map(|(p, _)| p.clone()).collect()
        }
    }
}

trait NullOr {
    fn is_none_or_null(&self) -> bool;
}

impl NullOr for Option<&Value> {
    fn is_none_or_null(&self) -> bool {
        matches!(self, None | Some(Value::Null))
    }
}

/// Stateful mapper for one ACP session.
#[derive(Debug, Default)]
pub struct Mapper {
    session: String,
    cwd: Option<String>,
    tools: HashMap<String, Tool>,
    message: String,
    thinking: bool,
}

impl Mapper {
    pub fn new(session: impl Into<String>) -> Self {
        Self {
            session: session.into(),
            ..Default::default()
        }
    }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn event(&self, at: i64, kind: EventKind) -> AgentEvent {
        AgentEvent::for_session(
            PROVIDER_ID,
            SessionId::new(&self.session),
            EventSource::Protocol,
            kind,
        )
        .at(at)
    }

    fn relative(&self, path: &str) -> String {
        match self.cwd.as_deref() {
            Some(cwd) => path
                .strip_prefix(cwd)
                .map(|rest| rest.trim_start_matches(['/', '\\']).to_owned())
                .filter(|rest| !rest.is_empty())
                .unwrap_or_else(|| path.to_owned()),
            None => path.to_owned(),
        }
    }

    /// Streamed agent text collected so far becomes one message.
    pub fn flush_message(&mut self, at: i64) -> Option<AgentEvent> {
        let text = std::mem::take(&mut self.message);
        (!text.trim().is_empty()).then(|| {
            self.event(
                at,
                EventKind::AgentMessage(TextNote {
                    text: Some(clip(&text, 2000)),
                }),
            )
        })
    }

    fn started(&mut self, id: &str, at: i64, out: &mut Vec<AgentEvent>) {
        let tool = self.tools.get(id).cloned().unwrap_or_default();
        out.extend(self.flush_message(at));
        out.push(self.event(
            at,
            EventKind::ToolStarted(ToolStarted {
                tool_call_id: Some(ToolCallId(id.to_owned())),
                tool_name: tool.tool_name(),
                category: category(&tool.kind),
                title: Some(first_line(&tool.title, 120)).filter(|t| !t.is_empty()),
            }),
        ));
        if tool.kind == "execute" {
            out.push(self.event(
                at,
                EventKind::CommandStarted(CommandStarted {
                    command_id: Some(id.to_owned()),
                    command: tool.command(),
                    cwd: self.cwd.clone(),
                }),
            ));
        }
    }

    fn finished(&mut self, id: &str, ok: bool, at: i64, out: &mut Vec<AgentEvent>) {
        let Some(tool) = self.tools.get_mut(id) else {
            return;
        };
        if tool.finished {
            return;
        }
        tool.finished = true;
        let tool = tool.clone();
        let detail = match (ok, tool.text.as_deref()) {
            (true, _) => None,
            (false, Some(text)) => Some(first_line(text, 300)),
            (false, None) => tool.rejected.then(|| "Not allowed".to_owned()),
        };
        if tool.kind == "execute" {
            let result = CommandFinished {
                command_id: Some(id.to_owned()),
                command: Some(tool.command()),
                // ACP reports no structured exit code.
                exit_code: None,
                duration_ms: None,
                error: detail.clone(),
            };
            out.push(self.event(
                at,
                if ok {
                    EventKind::CommandCompleted(result)
                } else {
                    EventKind::CommandFailed(result)
                },
            ));
        }
        if ok {
            let touched = |path: String| FileTouched {
                path,
                tool_call_id: Some(ToolCallId(id.to_owned())),
            };
            match tool.kind.as_str() {
                "edit" if !tool.diffs.is_empty() => {
                    for (path, created) in &tool.diffs {
                        let t = touched(path.clone());
                        out.push(self.event(
                            at,
                            if *created {
                                EventKind::FileCreated(t)
                            } else {
                                EventKind::FileModified(t)
                            },
                        ));
                    }
                }
                "edit" | "move" => {
                    for path in tool.paths() {
                        out.push(self.event(at, EventKind::FileModified(touched(path))));
                    }
                }
                "delete" => {
                    for path in tool.paths() {
                        out.push(self.event(at, EventKind::FileDeleted(touched(path))));
                    }
                }
                "read" => {
                    for path in tool.paths() {
                        out.push(self.event(at, EventKind::FileRead(touched(path))));
                    }
                }
                _ => {}
            }
        }
        let finished = ToolFinished {
            tool_call_id: Some(ToolCallId(id.to_owned())),
            tool_name: tool.tool_name(),
            category: category(&tool.kind),
            duration_ms: None,
            detail,
        };
        out.push(self.event(
            at,
            if ok {
                EventKind::ToolCompleted(finished)
            } else {
                EventKind::ToolFailed(finished)
            },
        ));
    }

    /// Maps the `update` of one `session/update` notification.
    pub fn update(&mut self, update: &Value, at: i64) -> Vec<AgentEvent> {
        let mut out = Vec::new();
        let kind = s(update, "sessionUpdate").unwrap_or_default();
        if kind != "agent_thought_chunk" {
            self.thinking = false;
        }
        match kind {
            "agent_message_chunk" => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    self.message.push_str(text);
                }
            }
            "agent_thought_chunk" if !self.thinking => {
                self.thinking = true;
                out.push(self.event(
                    at,
                    EventKind::AgentThinking(TextNote {
                        text: Some("Thinking".into()),
                    }),
                ));
            }
            "tool_call" | "tool_call_update" => {
                let Some(id) = s(update, "toolCallId").map(str::to_owned) else {
                    return out;
                };
                // A new `tool_call` reusing the id of a finished one is a new call.
                if kind == "tool_call" && self.tools.get(&id).is_some_and(|t| t.finished) {
                    self.tools.remove(&id);
                }
                let known = self.tools.contains_key(&id);
                self.tools.entry(id.clone()).or_default().merge(update);
                if !known {
                    self.started(&id, at, &mut out);
                }
                match s(update, "status") {
                    Some("completed") => self.finished(&id, true, at, &mut out),
                    Some("failed") => self.finished(&id, false, at, &mut out),
                    _ => {}
                }
            }
            "plan" => {
                let entries = update
                    .get("entries")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if !entries.is_empty() {
                    let done = entries
                        .iter()
                        .filter(|e| s(e, "status") == Some("completed"))
                        .count();
                    out.push(self.event(
                        at,
                        EventKind::AgentThinking(TextNote {
                            text: Some(format!("Plan: {done}/{} steps done", entries.len())),
                        }),
                    ));
                }
            }
            "current_mode_update" => {
                if let Some(mode) = s(update, "currentModeId") {
                    out.push(self.event(
                        at,
                        EventKind::SessionUpdated(SessionInfo {
                            permission_mode: Some(mode.to_owned()),
                            ..Default::default()
                        }),
                    ));
                }
            }
            "config_option_update" => {
                if let Some(model) = update.get("configOptions").and_then(model_name) {
                    out.push(self.event(
                        at,
                        EventKind::SessionUpdated(SessionInfo {
                            model: Some(model),
                            ..Default::default()
                        }),
                    ));
                }
            }
            "session_info_update" => {
                if let Some(title) = s(update, "title") {
                    out.push(self.event(
                        at,
                        EventKind::SessionUpdated(SessionInfo {
                            title: Some(title.to_owned()),
                            ..Default::default()
                        }),
                    ));
                }
            }
            "usage_update" => {
                let cost = update
                    .get("cost")
                    .filter(|c| s(c, "currency") == Some("USD"));
                out.push(self.event(
                    at,
                    EventKind::UsageUpdated(UsageSnapshot {
                        context_tokens: update.get("used").and_then(Value::as_u64),
                        context_window: update.get("size").and_then(Value::as_u64),
                        cost_usd: cost.and_then(|c| c.get("amount")).and_then(Value::as_f64),
                        ..Default::default()
                    }),
                ));
            }
            "notice" if s(update, "severity") == Some("error") => {
                out.push(self.event(
                    at,
                    EventKind::AgentError(AgentError {
                        message: s(update, "title").unwrap_or("Agent notice").to_owned(),
                        error_type: Some("notice".into()),
                        recoverable: true,
                    }),
                ));
            }
            "compaction_update" => match s(update, "status") {
                Some("completed") => {
                    out.push(self.event(at, EventKind::ContextCompacted(TextNote::default())))
                }
                Some("failed") => out.push(self.event(
                    at,
                    EventKind::AgentError(AgentError {
                        message: format!(
                            "Context compaction failed: {}",
                            s(update, "error").unwrap_or("unknown error")
                        ),
                        error_type: Some("compaction".into()),
                        recoverable: true,
                    }),
                )),
                _ => out.push(self.event(
                    at,
                    EventKind::AgentThinking(TextNote {
                        text: Some("Compacting context".into()),
                    }),
                )),
            },
            // user_message_chunk (our own prompt), available_commands_update,
            // plan_update/plan_removed and unknown kinds carry nothing to show.
            _ => {}
        }
        out
    }

    /// Agent Office declined the permission request of this tool call: if
    /// the agent never reports the tool again, it closes as "Not allowed".
    pub fn permission_rejected(&mut self, tool_call_id: &str) {
        if let Some(tool) = self.tools.get_mut(tool_call_id) {
            tool.rejected = true;
        }
    }

    /// Tool calls still open when the turn is over cannot run any more.
    /// Those whose outcome is known (cancelled, not allowed, turn failed)
    /// close as failed; the rest are forgotten without inventing a result
    /// (the idle event clears them from the office).
    fn close_open_tools(&mut self, reason: Option<&str>, at: i64, out: &mut Vec<AgentEvent>) {
        let mut open: Vec<String> = self
            .tools
            .iter()
            .filter(|(_, t)| !t.finished)
            .map(|(id, _)| id.clone())
            .collect();
        open.sort();
        for id in open {
            let Some(tool) = self.tools.get_mut(&id) else {
                continue;
            };
            match (reason, tool.rejected) {
                (Some(reason), _) => tool.text = Some(reason.to_owned()),
                (None, true) => tool.text = Some("Not allowed".into()),
                (None, false) => {
                    tool.finished = true;
                    continue;
                }
            }
            self.finished(&id, false, at, out);
        }
    }

    /// The `session/prompt` request finished (its result or error).
    pub fn turn_ended(&mut self, result: Result<&Value, &str>, at: i64) -> Vec<AgentEvent> {
        let mut out: Vec<AgentEvent> = self.flush_message(at).into_iter().collect();
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.close_open_tools(Some("The turn ended with an error"), at, &mut out);
                out.push(self.event(
                    at,
                    EventKind::AgentError(AgentError {
                        message: error.to_owned(),
                        error_type: Some("prompt_error".into()),
                        recoverable: true,
                    }),
                ));
                return out;
            }
        };
        if let Some(usage) = result.get("usage").filter(|u| u.is_object()) {
            out.push(self.event(
                at,
                EventKind::UsageUpdated(UsageSnapshot {
                    input_tokens: usage.get("inputTokens").and_then(Value::as_u64),
                    output_tokens: usage.get("outputTokens").and_then(Value::as_u64),
                    total_tokens: usage.get("totalTokens").and_then(Value::as_u64),
                    reasoning_tokens: usage.get("thoughtTokens").and_then(Value::as_u64),
                    cached_input_tokens: usage.get("cachedReadTokens").and_then(Value::as_u64),
                    ..Default::default()
                }),
            ));
        }
        let stop = s(result, "stopReason").unwrap_or("end_turn");
        let stopped_by = match stop {
            "max_tokens" => Some("Stopped: the model reached its maximum number of tokens"),
            "max_turn_requests" => Some("Stopped: too many model requests in one turn"),
            "refusal" => Some("The agent refused to continue"),
            _ => None,
        };
        let reason = if stop == "cancelled" {
            Some("Cancelled")
        } else {
            stopped_by
        };
        self.close_open_tools(reason, at, &mut out);
        out.push(self.event(
            at,
            match (stop, stopped_by) {
                (_, Some(message)) => EventKind::AgentError(AgentError {
                    message: message.to_owned(),
                    error_type: Some(stop.to_owned()),
                    recoverable: true,
                }),
                ("cancelled", None) => EventKind::AgentIdle(TextNote {
                    text: Some("Turn cancelled".into()),
                }),
                _ => EventKind::AgentIdle(TextNote {
                    text: Some("Turn finished — waiting for your next prompt".into()),
                }),
            },
        ));
        out
    }

    /// Maps a `session/request_permission` request. Also starts the tool if
    /// its `tool_call` was not seen yet.
    pub fn permission(&mut self, request_id: &str, params: &Value, at: i64) -> Vec<AgentEvent> {
        let mut out = Vec::new();
        let tool_call = params.get("toolCall").cloned().unwrap_or(Value::Null);
        if let Some(id) = s(&tool_call, "toolCallId").map(str::to_owned) {
            let known = self.tools.contains_key(&id);
            self.tools.entry(id.clone()).or_default().merge(&tool_call);
            if !known {
                self.started(&id, at, &mut out);
            }
        }
        let mut tool = Tool::default();
        tool.merge(&tool_call);
        if let Some(existing) = s(&tool_call, "toolCallId").and_then(|id| self.tools.get(id)) {
            tool = existing.clone();
        }
        let paths: Vec<String> = tool.paths().iter().map(|p| self.relative(p)).collect();
        let description = match tool.kind.as_str() {
            "execute" => format!("Run: {}", first_line(&tool.command(), 160)),
            "edit" | "delete" | "move" if !paths.is_empty() => {
                format!(
                    "{}: {}",
                    if tool.title.is_empty() {
                        "Change files"
                    } else {
                        &tool.title
                    },
                    paths.join(", ")
                )
            }
            _ if !tool.title.is_empty() => first_line(&tool.title, 160),
            _ => "Use a tool".to_owned(),
        };
        let options = params
            .get("options")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|o| {
                        Some(PermissionOption {
                            id: s(o, "optionId")?.to_owned(),
                            kind: option_kind(s(o, "kind")?)?,
                            label: s(o, "name").unwrap_or_default().to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(self.event(
            at,
            EventKind::PermissionRequested(PermissionRequested {
                request_id: PermissionRequestId::new(request_id),
                tool_name: Some(tool.tool_name()),
                description,
                can_resolve: true,
                options,
            }),
        ));
        out
    }
}

fn option_kind(kind: &str) -> Option<PermissionOptionKind> {
    Some(match kind {
        "allow_once" => PermissionOptionKind::AllowOnce,
        "allow_always" => PermissionOptionKind::AllowAlways,
        "reject_once" => PermissionOptionKind::RejectOnce,
        "reject_always" => PermissionOptionKind::RejectAlways,
        _ => return None,
    })
}

/// Current value (its display name) of the `model` config option, if any.
pub fn model_name(options: &Value) -> Option<String> {
    let option = options
        .as_array()?
        .iter()
        .find(|o| s(o, "category") == Some("model") && s(o, "type") == Some("select"))?;
    let current = s(option, "currentValue")?;
    Some(
        select_values(option)
            .into_iter()
            .find(|(value, _)| value == current)
            .map(|(_, name)| name)
            .unwrap_or_else(|| current.to_owned()),
    )
}

/// How a model can be chosen for a new session.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelChoice {
    /// ACP 1.x: the select config option of category `model`
    /// (`session/set_config_option`).
    ConfigOption {
        config_id: Value,
        value: String,
        name: String,
    },
    /// The older, unstable `models` state (`session/set_model`), which is not
    /// in the 1.5.0 schema but is reported for Cursor.
    Legacy { model_id: String, name: String },
}

fn model_option(session: &Value) -> Option<&Value> {
    session
        .get("configOptions")?
        .as_array()?
        .iter()
        .find(|o| s(o, "category") == Some("model") && s(o, "type") == Some("select"))
}

/// `(modelId, name)` pairs of the older `models` state.
fn legacy_models(session: &Value) -> Vec<(String, String)> {
    session
        .pointer("/models/availableModels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = s(m, "modelId")?;
            Some((id.to_owned(), s(m, "name").unwrap_or(id).to_owned()))
        })
        .collect()
}

/// Display name of the session's current model, from a `session/new` (or
/// `session/load`) result.
pub fn current_model(session: &Value) -> Option<String> {
    if let Some(options) = session.get("configOptions") {
        if let Some(name) = model_name(options) {
            return Some(name);
        }
    }
    let current = session
        .pointer("/models/currentModelId")
        .and_then(Value::as_str)?;
    Some(
        legacy_models(session)
            .into_iter()
            .find(|(id, _)| id == current)
            .map(|(_, name)| name)
            .unwrap_or_else(|| current.to_owned()),
    )
}

/// Finds the model the user asked for: an exact id or name (ignoring case),
/// else an id whose base matches (`composer-2.5` → `composer-2.5[fast=true]`).
pub fn find_model(session: &Value, wanted: &str) -> Option<ModelChoice> {
    fn pick(choices: &[(String, String)], wanted: &str) -> Option<(String, String)> {
        let base = |id: &str| id.split('[').next().unwrap_or(id).to_owned();
        choices
            .iter()
            .find(|(id, name)| id.eq_ignore_ascii_case(wanted) || name.eq_ignore_ascii_case(wanted))
            .or_else(|| {
                choices
                    .iter()
                    .find(|(id, _)| base(id).eq_ignore_ascii_case(wanted))
            })
            .cloned()
    }
    if let Some(option) = model_option(session) {
        let (value, name) = pick(&select_values(option), wanted)?;
        return Some(ModelChoice::ConfigOption {
            config_id: option.get("id").cloned().unwrap_or(Value::Null),
            value,
            name,
        });
    }
    let (model_id, name) = pick(&legacy_models(session), wanted)?;
    Some(ModelChoice::Legacy { model_id, name })
}

/// `(value, name)` pairs of a select option (flat or grouped).
pub fn select_values(option: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in option
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(group) = entry.get("options").and_then(Value::as_array) {
            for o in group {
                if let Some(value) = s(o, "value") {
                    out.push((value.to_owned(), s(o, "name").unwrap_or(value).to_owned()));
                }
            }
        } else if let Some(value) = s(entry, "value") {
            out.push((
                value.to_owned(),
                s(entry, "name").unwrap_or(value).to_owned(),
            ));
        }
    }
    out
}

/// The option to send for a decision, chosen by **kind** (agents use their
/// own ids, e.g. `allow-once`): approve → `allow_once` (or `allow_always`
/// for the session), reject → `reject_once`, falling back to the other
/// variant of the same decision.
pub fn select_option(options: &Value, decision: &PermissionDecision) -> Option<String> {
    let wanted: &[&str] = match decision {
        PermissionDecision::Approve { for_session: true } => &["allow_always", "allow_once"],
        PermissionDecision::Approve { .. } => &["allow_once", "allow_always"],
        PermissionDecision::Reject { .. } => &["reject_once", "reject_always"],
    };
    let list = options.as_array()?;
    wanted.iter().find_map(|kind| {
        list.iter()
            .find(|o| s(o, "kind") == Some(kind))
            .and_then(|o| s(o, "optionId"))
            .map(str::to_owned)
    })
}

/// `RequestPermissionResponse` for the chosen option (`None` = cancelled).
pub fn permission_response(option_id: Option<&str>) -> Value {
    match option_id {
        Some(id) => json!({ "outcome": { "outcome": "selected", "optionId": id } }),
        None => json!({ "outcome": { "outcome": "cancelled" } }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_chosen_by_kind_and_ids_echoed() {
        // Hyphenated ids as reported for Cursor; kinds per the ACP schema.
        let options = json!([
            {"optionId": "allow-once", "name": "Allow once", "kind": "allow_once"},
            {"optionId": "allow-always", "name": "Always allow", "kind": "allow_always"},
            {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"}
        ]);
        assert_eq!(
            select_option(
                &options,
                &PermissionDecision::Approve { for_session: false }
            )
            .as_deref(),
            Some("allow-once")
        );
        assert_eq!(
            select_option(&options, &PermissionDecision::Approve { for_session: true }).as_deref(),
            Some("allow-always")
        );
        assert_eq!(
            select_option(&options, &PermissionDecision::Reject { message: None }).as_deref(),
            Some("reject-once")
        );
        let only_always = json!([{"optionId": "x", "name": "Always", "kind": "allow_always"}]);
        assert_eq!(
            select_option(
                &only_always,
                &PermissionDecision::Approve { for_session: false }
            )
            .as_deref(),
            Some("x")
        );
        assert_eq!(
            select_option(&only_always, &PermissionDecision::Reject { message: None }),
            None
        );
        assert_eq!(
            permission_response(None),
            json!({"outcome": {"outcome": "cancelled"}})
        );
    }

    #[test]
    fn model_names_come_from_flat_or_grouped_selects() {
        let options = json!([
            {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "agent", "options": [{"value": "agent", "name": "Agent"}]},
            {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "m2",
             "options": [{"group": "g", "name": "Fast", "options": [{"value": "m1", "name": "Model One"}, {"value": "m2", "name": "Model Two"}]}]}
        ]);
        assert_eq!(model_name(&options).as_deref(), Some("Model Two"));
        assert_eq!(select_values(&options[1]).len(), 2);
    }

    #[test]
    fn models_are_found_in_config_options_or_the_older_state() {
        let both = json!({
            "configOptions": [{"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "default[]",
                "options": [{"value": "default[]", "name": "Auto"}, {"value": "composer-2.5[fast=true]", "name": "Composer 2.5 Fast"}]}],
            "models": {"currentModelId": "default[]", "availableModels": [{"modelId": "default[]", "name": "Auto"}]}
        });
        assert_eq!(current_model(&both).as_deref(), Some("Auto"));
        assert_eq!(
            find_model(&both, "composer-2.5"),
            Some(ModelChoice::ConfigOption {
                config_id: json!("model"),
                value: "composer-2.5[fast=true]".into(),
                name: "Composer 2.5 Fast".into()
            })
        );
        let legacy = json!({"models": {"currentModelId": "b", "availableModels": [{"modelId": "a", "name": "Model A"}, {"modelId": "b", "name": "Model B"}]}});
        assert_eq!(current_model(&legacy).as_deref(), Some("Model B"));
        assert_eq!(
            find_model(&legacy, "model a"),
            Some(ModelChoice::Legacy {
                model_id: "a".into(),
                name: "Model A".into()
            })
        );
        assert_eq!(find_model(&legacy, "c"), None);
        assert_eq!(current_model(&json!({})), None);
    }

    #[test]
    fn reused_tool_call_ids_start_a_new_call() {
        let mut m = Mapper::new("s");
        let call = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "kind": "execute", "title": "ls", "status": "pending"});
        let done =
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"});
        for _ in 0..2 {
            let started = m.update(&call, 1);
            assert!(
                matches!(started[0].kind, EventKind::ToolStarted(_)),
                "{started:?}"
            );
            let finished = m.update(&done, 2);
            assert!(
                matches!(finished.last().unwrap().kind, EventKind::ToolCompleted(_)),
                "{finished:?}"
            );
        }
        assert!(
            m.update(&done, 3).is_empty(),
            "a duplicate completion is ignored"
        );
    }

    #[test]
    fn open_tools_are_closed_only_when_their_outcome_is_known() {
        let mut m = Mapper::new("s");
        for id in ["rejected", "unknown"] {
            m.update(&json!({"sessionUpdate": "tool_call", "toolCallId": id, "kind": "edit", "title": id, "status": "pending"}), 1);
        }
        m.permission_rejected("rejected");
        let events = m.turn_ended(Ok(&json!({"stopReason": "end_turn"})), 2);
        let failed: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::ToolFailed(t) => {
                    Some((t.tool_call_id.clone().unwrap().0, t.detail.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            failed,
            vec![("rejected".to_owned(), Some("Not allowed".to_owned()))]
        );
        // Nothing leaks into the next turn.
        assert!(m
            .turn_ended(Ok(&json!({"stopReason": "cancelled"})), 3)
            .iter()
            .all(|e| !matches!(e.kind, EventKind::ToolFailed(_))));
    }

    #[test]
    fn message_chunks_become_one_message() {
        let mut m = Mapper::new("s");
        for text in ["Hello ", "world"] {
            assert!(m.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}), 1).is_empty());
        }
        let events = m.turn_ended(Ok(&json!({"stopReason": "end_turn"})), 2);
        assert!(
            matches!(&events[0].kind, EventKind::AgentMessage(n) if n.text.as_deref() == Some("Hello world"))
        );
        assert!(matches!(events[1].kind, EventKind::AgentIdle(_)));
    }
}
