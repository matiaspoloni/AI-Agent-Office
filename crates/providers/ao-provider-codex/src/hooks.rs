//! Codex hook payload → normalized events (external sessions). Pure functions.
//!
//! Input fields follow the hook input schemas of codex 0.156.1
//! (`codex-rs/hooks/schema/generated/*.command.input.schema.json`) and the
//! payloads recorded from a real run in `fixtures/codex/hooks/*-real.json`:
//! `session_id` is the root thread id; events inside a subagent carry
//! `agent_id` (the child thread id) and `agent_type`.

use crate::tools;
use crate::PROVIDER_ID;
use ao_core::event::*;
use ao_core::ids::{AgentId, PermissionRequestId, SessionId, ToolCallId};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct HookContext {
    pub received_at_ms: i64,
    /// For `PermissionRequest`: the id Agent Office assigned (Codex sends
    /// none) and whether the user can answer it from the app.
    pub permission: Option<(String, bool)>,
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

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

/// Event for the agent the hook ran in (main agent or subagent).
fn event(payload: &Value, ctx: &HookContext, kind: EventKind) -> AgentEvent {
    let session = SessionId::new(s(payload, "session_id").unwrap_or_default());
    let e = AgentEvent::for_session(PROVIDER_ID, session.clone(), EventSource::Hook, kind)
        .at(ctx.received_at_ms);
    match s(payload, "agent_id") {
        Some(agent) if agent != session.as_str() => e.with_agent(AgentId::new(agent), None),
        _ => e,
    }
}

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
            mode: Some(SessionMode::External),
            cwd: cwd.map(str::to_owned),
            model: owned(payload, "model"),
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
                title: Some(tools::title(tool_name, &input)),
            }))];
            if tools::is_shell(tool_name) {
                if let Some(command) = tools::command(&input) {
                    out.push(ev(EventKind::CommandStarted(CommandStarted {
                        command_id: tool_id,
                        command,
                        cwd: cwd.map(str::to_owned),
                    })));
                }
            }
            out
        }
        // Codex has no PostToolUseFailure hook; a finished call is all we know.
        "PostToolUse" if !tool_name.is_empty() => {
            let mut out = Vec::new();
            if tools::is_shell(tool_name) {
                out.push(ev(EventKind::CommandCompleted(CommandFinished {
                    command_id: tool_id.clone(),
                    command: tools::command(&input),
                    // The Bash tool_response is the output text: no exit code.
                    exit_code: None,
                    duration_ms: None,
                    error: None,
                })));
            }
            if tool_name == "apply_patch" {
                let files = tools::command(&input)
                    .map(|p| tools::patch_files(&p))
                    .unwrap_or_default();
                for (op, path) in files {
                    let touched = FileTouched {
                        path,
                        tool_call_id: tool_id.clone().map(ToolCallId),
                    };
                    out.push(ev(match op {
                        tools::PatchOp::Add => EventKind::FileCreated(touched),
                        tools::PatchOp::Update => EventKind::FileModified(touched),
                        tools::PatchOp::Delete => EventKind::FileDeleted(touched),
                    }));
                }
            }
            out.push(ev(EventKind::ToolCompleted(ToolFinished {
                tool_call_id: tool_id.map(ToolCallId),
                tool_name: tool_name.to_owned(),
                category: tools::category(tool_name),
                duration_ms: None,
                detail: None,
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
                description: tools::permission_description(tool_name, &input),
                can_resolve,
                options: Vec::new(),
            }))]
        }
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
        "Interrupt" => vec![ev(EventKind::AgentIdle(TextNote {
            text: Some("Turn interrupted".into()),
        }))],
        "SubagentStart" | "SubagentStop" => {
            if s(payload, "agent_id").is_none() {
                return Vec::new();
            }
            let info = SubagentInfo {
                agent_type: owned(payload, "agent_type"),
                description: None,
                reason: None,
            };
            if event_name(payload) == "SubagentStart" {
                return vec![ev(EventKind::SubagentStarted(info))];
            }
            let mut out = Vec::new();
            if let Some(message) = s(payload, "last_assistant_message") {
                out.push(ev(EventKind::AgentMessage(TextNote {
                    text: Some(clip(message, 2000)),
                })));
            }
            out.push(ev(EventKind::SubagentEnded(SubagentInfo {
                reason: Some("completed".into()),
                ..info
            })));
            out
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
        _ => Vec::new(),
    }
}

/// Stdout for a `PermissionRequest` hook. The output schema of codex 0.156.1
/// (`permission-request.command.output`) rejects unknown fields, so only
/// `hookEventName` and `decision.{behavior,message}` are written.
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
    fn payloads_without_session_are_ignored() {
        assert!(map_hook(&json!({"hook_event_name": "Stop"}), &HookContext::default()).is_empty());
    }

    #[test]
    fn apply_patch_post_tool_use_reports_files() {
        let payload = json!({
            "session_id": "s", "turn_id": "t", "cwd": "/p", "hook_event_name": "PostToolUse",
            "tool_name": "apply_patch", "tool_use_id": "c1",
            "tool_input": {"command": "*** Begin Patch\n*** Update File: a.rs\n@@\n-x\n+y\n*** Delete File: b.rs\n*** End Patch"},
            "tool_response": "Success"
        });
        let events = map_hook(&payload, &HookContext::default());
        let types: Vec<&str> = events.iter().map(|e| e.kind.type_name()).collect();
        assert_eq!(types, ["file.modified", "file.deleted", "tool.completed"]);
    }

    #[test]
    fn decision_output_matches_schema() {
        let allow: Value = serde_json::from_str(&permission_decision_output(true, None)).unwrap();
        assert_eq!(
            allow,
            json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}})
        );
        let deny: Value =
            serde_json::from_str(&permission_decision_output(false, Some("no"))).unwrap();
        assert_eq!(
            deny["hookSpecificOutput"]["decision"],
            json!({"behavior": "deny", "message": "no"})
        );
    }
}
