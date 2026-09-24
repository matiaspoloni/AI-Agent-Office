//! Claude Code hook payload → normalized events. Pure functions; field names
//! follow the documented hook inputs (code.claude.com/docs/en/hooks) and the
//! payloads recorded from Claude Code 2.1.281 in `fixtures/claude/hooks/`.

use crate::tools;
use crate::PROVIDER_ID;
use ao_core::event::*;
use ao_core::ids::{AgentId, PermissionRequestId, SessionId, ToolCallId};
use serde_json::Value;

/// Per-invocation context supplied by the adapter.
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    pub received_at_ms: i64,
    /// The session was launched by Agent Office.
    pub managed: bool,
    /// For `PermissionRequest`: the id Agent Office assigned (Claude sends none)
    /// and whether the user can answer it from the app.
    pub permission: Option<(String, bool)>,
    /// Agent type the session itself runs as (`claude --agent X`), used to
    /// recognise Claude's internal helper agents in SubagentStart/Stop.
    pub session_agent_type: Option<String>,
}

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|x| !x.is_empty())
}

fn owned(v: &Value, key: &str) -> Option<String> {
    s(v, key).map(str::to_owned)
}

pub fn event_name(payload: &Value) -> &str {
    s(payload, "hook_event_name").unwrap_or_default()
}

pub fn session_id(payload: &Value) -> Option<&str> {
    s(payload, "session_id")
}

/// Builds an event for the agent the hook ran in (main agent or subagent).
fn event(payload: &Value, ctx: &HookContext, kind: EventKind) -> AgentEvent {
    let session = SessionId::new(s(payload, "session_id").unwrap_or_default());
    let e = AgentEvent::for_session(PROVIDER_ID, session.clone(), EventSource::Hook, kind)
        .at(ctx.received_at_ms);
    match s(payload, "agent_id") {
        Some(agent) if agent != session.as_str() => e.with_agent(AgentId::new(agent), None),
        _ => e,
    }
}

fn is_internal_agent(payload: &Value, ctx: &HookContext) -> bool {
    match s(payload, "agent_type") {
        None => true,
        Some(t) => ctx.session_agent_type.as_deref() == Some(t),
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

/// Maps one hook payload to zero or more events.
pub fn map_hook(payload: &Value, ctx: &HookContext) -> Vec<AgentEvent> {
    if session_id(payload).is_none() {
        return Vec::new();
    }
    let cwd = s(payload, "cwd");
    let tool_name = s(payload, "tool_name").unwrap_or_default();
    let input = payload.get("tool_input").cloned().unwrap_or(Value::Null);
    let tool_id = owned(payload, "tool_use_id");
    let ev = |kind| event(payload, ctx, kind);

    match event_name(payload) {
        "SessionStart" => vec![ev(EventKind::SessionStarted(SessionInfo {
            mode: Some(if ctx.managed {
                SessionMode::Managed
            } else {
                SessionMode::External
            }),
            cwd: cwd.map(str::to_owned),
            model: owned(payload, "model"),
            title: owned(payload, "session_title"),
            reason: owned(payload, "source"),
            permission_mode: owned(payload, "permission_mode"),
            ..Default::default()
        }))],
        "SessionEnd" => vec![ev(EventKind::SessionEnded(SessionEnded {
            reason: owned(payload, "reason"),
            exit_code: None,
        }))],
        "UserPromptSubmit" => vec![ev(EventKind::PromptSubmitted(PromptSubmitted {
            text: owned(payload, "prompt"),
        }))],
        "PreToolUse" if !tool_name.is_empty() => {
            let mut out = vec![ev(EventKind::ToolStarted(ToolStarted {
                tool_call_id: tool_id.clone().map(ToolCallId),
                tool_name: tool_name.to_owned(),
                category: tools::category(tool_name),
                title: Some(tools::title(tool_name, &input, cwd)),
            }))];
            if tools::is_shell(tool_name) {
                if let Some(command) = tools::command(&input) {
                    out.push(ev(EventKind::CommandStarted(CommandStarted {
                        command_id: tool_id.clone(),
                        command,
                        cwd: cwd.map(str::to_owned),
                    })));
                }
            }
            if tool_name == "AskUserQuestion" {
                out.push(ev(EventKind::AgentWaiting(AgentWaiting {
                    reason: WaitingReason::Input,
                    message: Some("Claude asked you a question".into()),
                })));
            }
            out
        }
        "PostToolUse" if !tool_name.is_empty() => {
            let duration = payload.get("duration_ms").and_then(Value::as_u64);
            let response = payload.get("tool_response").cloned().unwrap_or(Value::Null);
            let mut out = Vec::new();
            if tools::is_shell(tool_name) {
                out.push(ev(EventKind::CommandCompleted(CommandFinished {
                    command_id: tool_id.clone(),
                    command: tools::command(&input),
                    // Claude's Bash result carries no exit code on success.
                    exit_code: None,
                    duration_ms: duration,
                    error: None,
                })));
            }
            if let Some(path) = tools::file_path(&input) {
                let touched = FileTouched {
                    path,
                    tool_call_id: tool_id.clone().map(ToolCallId),
                };
                match tool_name {
                    "Read" | "NotebookRead" => out.push(ev(EventKind::FileRead(touched))),
                    "Write" if response.get("type").and_then(Value::as_str) == Some("create") => {
                        out.push(ev(EventKind::FileCreated(touched)))
                    }
                    "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
                        out.push(ev(EventKind::FileModified(touched)))
                    }
                    _ => {}
                }
            }
            out.push(ev(EventKind::ToolCompleted(ToolFinished {
                tool_call_id: tool_id.map(ToolCallId),
                tool_name: tool_name.to_owned(),
                category: tools::category(tool_name),
                duration_ms: duration,
                detail: None,
            })));
            out
        }
        "PostToolUseFailure" if !tool_name.is_empty() => {
            let error = owned(payload, "error");
            let duration = payload.get("duration_ms").and_then(Value::as_u64);
            let mut out = Vec::new();
            if tools::is_shell(tool_name) {
                out.push(ev(EventKind::CommandFailed(CommandFinished {
                    command_id: tool_id.clone(),
                    command: tools::command(&input),
                    exit_code: error.as_deref().and_then(tools::exit_code_from_error),
                    duration_ms: duration,
                    error: error
                        .as_deref()
                        .map(|e| clip(e.lines().next().unwrap_or_default(), 200)),
                })));
            }
            out.push(ev(EventKind::ToolFailed(ToolFinished {
                tool_call_id: tool_id.map(ToolCallId),
                tool_name: tool_name.to_owned(),
                category: tools::category(tool_name),
                duration_ms: duration,
                detail: error.map(|e| clip(&e, 500)),
            })));
            out
        }
        "PermissionRequest" => {
            let Some((request_id, can_resolve)) = ctx.permission.clone() else {
                return Vec::new();
            };
            vec![ev(EventKind::PermissionRequested(PermissionRequested {
                request_id: PermissionRequestId(request_id),
                tool_name: Some(tool_name.to_owned()).filter(|t| !t.is_empty()),
                description: tools::permission_description(tool_name, &input, cwd),
                can_resolve,
                options: Vec::new(),
            }))]
        }
        "PermissionDenied" => {
            let Some(id) = tool_id else { return Vec::new() };
            vec![ev(EventKind::PermissionDenied(PermissionResolved {
                request_id: PermissionRequestId(id),
                resolved_by: PermissionResolver::Provider,
                message: owned(payload, "reason"),
            }))]
        }
        "Notification" => match s(payload, "notification_type") {
            Some("permission_prompt") => vec![ev(EventKind::AgentWaiting(AgentWaiting {
                reason: WaitingReason::Permission,
                message: owned(payload, "message"),
            }))],
            Some("idle_prompt") => vec![ev(EventKind::AgentIdle(TextNote {
                text: owned(payload, "message"),
            }))],
            Some("agent_needs_input" | "elicitation_dialog" | "elicitation_url_dialog") => {
                vec![ev(EventKind::AgentWaiting(AgentWaiting {
                    reason: WaitingReason::Input,
                    message: owned(payload, "message"),
                }))]
            }
            _ => Vec::new(),
        },
        "Stop" => {
            let mut out = Vec::new();
            if let Some(message) = s(payload, "last_assistant_message") {
                out.push(ev(EventKind::AgentMessage(TextNote {
                    text: Some(clip(message, 2000)),
                })));
            }
            out.push(ev(EventKind::AgentIdle(TextNote {
                text: Some("Turn finished — waiting for your next prompt".into()),
            })));
            out
        }
        "StopFailure" => {
            let kind = owned(payload, "error");
            let message = owned(payload, "last_assistant_message")
                .or_else(|| owned(payload, "error_details"))
                .unwrap_or_else(|| format!("API error: {}", kind.as_deref().unwrap_or("unknown")));
            let recoverable = matches!(
                kind.as_deref(),
                Some("rate_limit" | "overloaded" | "server_error" | "max_output_tokens")
            );
            vec![ev(EventKind::AgentError(AgentError {
                message,
                error_type: kind,
                recoverable,
            }))]
        }
        "SubagentStart" | "SubagentStop" => {
            // Internal helper agents (prompt suggestions, /btw) must not become characters.
            let Some(agent) = s(payload, "agent_id") else {
                return Vec::new();
            };
            if is_internal_agent(payload, ctx) {
                return Vec::new();
            }
            let info = SubagentInfo {
                agent_type: owned(payload, "agent_type"),
                description: None,
                reason: None,
            };
            let kind = if event_name(payload) == "SubagentStart" {
                EventKind::SubagentStarted(info)
            } else {
                EventKind::SubagentEnded(SubagentInfo {
                    reason: Some("completed".into()),
                    ..info
                })
            };
            vec![AgentEvent::for_session(
                PROVIDER_ID,
                SessionId::new(session_id(payload).unwrap_or_default()),
                EventSource::Hook,
                kind,
            )
            .at(ctx.received_at_ms)
            .with_agent(AgentId::new(agent), None)]
        }
        "PreCompact" => vec![ev(EventKind::AgentThinking(TextNote {
            text: Some(format!(
                "Compacting context ({})",
                s(payload, "trigger").unwrap_or("auto")
            )),
        }))],
        "PostCompact" => vec![ev(EventKind::ContextCompacted(TextNote {
            text: owned(payload, "trigger"),
        }))],
        "CwdChanged" => vec![ev(EventKind::SessionUpdated(SessionInfo {
            cwd: owned(payload, "new_cwd").or_else(|| cwd.map(str::to_owned)),
            ..Default::default()
        }))],
        _ => Vec::new(),
    }
}

/// Stdout for a `PermissionRequest` hook (documented decision format).
pub fn permission_decision_output(allow: bool, message: Option<&str>) -> String {
    let decision = if allow {
        serde_json::json!({ "behavior": "allow" })
    } else {
        serde_json::json!({ "behavior": "deny", "message": message.unwrap_or("Rejected in Agent Office") })
    };
    serde_json::json!({
        "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ignores_payloads_without_session() {
        assert!(map_hook(&json!({"hook_event_name": "Stop"}), &HookContext::default()).is_empty());
    }

    #[test]
    fn decision_output_matches_documented_shape() {
        let allow: Value = serde_json::from_str(&permission_decision_output(true, None)).unwrap();
        assert_eq!(
            allow["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
        assert_eq!(allow["hookSpecificOutput"]["decision"]["behavior"], "allow");
        let deny: Value =
            serde_json::from_str(&permission_decision_output(false, Some("no"))).unwrap();
        assert_eq!(deny["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(deny["hookSpecificOutput"]["decision"]["message"], "no");
    }
}
