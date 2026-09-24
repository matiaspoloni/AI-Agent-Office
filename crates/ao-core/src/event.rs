//! The unified event model. Every provider adapter translates its native events
//! (hooks, JSON-RPC notifications, stream-json lines) into [`AgentEvent`]s.
//! The UI and the database only ever see these types.

use crate::ids::{
    AgentId, EventId, PermissionRequestId, ProjectId, ProviderId, RepositoryId, SessionId,
    ToolCallId,
};
use crate::time::now_ms;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Envelope shared by every event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentEvent {
    pub event_id: EventId,
    /// Milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub timestamp: i64,
    pub provider: ProviderId,
    pub session_id: SessionId,
    pub agent_id: AgentId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent_agent_id: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project_id: Option<ProjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repository_id: Option<RepositoryId>,
    pub source: EventSource,
    #[serde(flatten)]
    #[ts(flatten)]
    pub kind: EventKind,
}

impl AgentEvent {
    /// Event for the main agent of a session (agent id = session id).
    pub fn for_session(
        provider: impl Into<ProviderId>,
        session_id: impl Into<SessionId>,
        source: EventSource,
        kind: EventKind,
    ) -> Self {
        let session_id = session_id.into();
        Self {
            event_id: EventId::random(),
            timestamp: now_ms(),
            provider: provider.into(),
            agent_id: AgentId(session_id.0.clone()),
            session_id,
            parent_agent_id: None,
            project_id: None,
            repository_id: None,
            source,
            kind,
        }
    }

    /// Re-targets the event at a subagent of the session.
    pub fn with_agent(mut self, agent_id: impl Into<AgentId>, parent: Option<AgentId>) -> Self {
        self.agent_id = agent_id.into();
        self.parent_agent_id = parent;
        self
    }

    pub fn at(mut self, timestamp: i64) -> Self {
        self.timestamp = timestamp;
        self
    }

    pub fn is_main_agent(&self) -> bool {
        self.agent_id.0 == self.session_id.0
    }

    pub fn type_name(&self) -> &'static str {
        self.kind.type_name()
    }

    /// Checks envelope and payload invariants. Adapters are expected to
    /// produce valid events; the pipeline rejects (and reports) the rest so a
    /// buggy adapter cannot corrupt the office state.
    pub fn validate(&self) -> Result<(), String> {
        fn id_ok(value: &str) -> bool {
            !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
        }
        if self.provider.0.is_empty()
            || self.provider.0.len() > 64
            || !self
                .provider
                .0
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(format!("invalid provider id `{}`", self.provider));
        }
        if !id_ok(&self.session_id.0) {
            return Err("invalid session id".into());
        }
        if !id_ok(&self.agent_id.0) {
            return Err("invalid agent id".into());
        }
        if self.parent_agent_id.as_ref() == Some(&self.agent_id) {
            return Err("an agent cannot be its own parent".into());
        }
        let subagent_event = matches!(
            self.kind,
            EventKind::SubagentStarted(_)
                | EventKind::SubagentUpdated(_)
                | EventKind::SubagentEnded(_)
        );
        if subagent_event && self.is_main_agent() {
            return Err(format!(
                "{} must target a subagent, not the main agent",
                self.type_name()
            ));
        }
        match &self.kind {
            EventKind::ToolStarted(t) if t.tool_name.trim().is_empty() => {
                Err("tool.started without tool name".into())
            }
            EventKind::ToolCompleted(t) | EventKind::ToolFailed(t)
                if t.tool_name.trim().is_empty() =>
            {
                Err("tool result without tool name".into())
            }
            EventKind::PermissionRequested(p) if p.request_id.0.trim().is_empty() => {
                Err("permission.requested without request id".into())
            }
            EventKind::FileRead(f)
            | EventKind::FileCreated(f)
            | EventKind::FileModified(f)
            | EventKind::FileDeleted(f)
                if f.path.trim().is_empty() =>
            {
                Err("file event without path".into())
            }
            _ => Ok(()),
        }
    }
}

/// Where the event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum EventSource {
    /// A provider hook (via the hook relay).
    Hook,
    /// A structured provider protocol (stream-json, app-server JSON-RPC, ACP).
    Protocol,
    /// The process manager (spawn, exit, crash).
    Process,
    /// The Git service.
    Git,
    /// Agent Office itself (adapter health, housekeeping).
    Internal,
    /// The demo provider. Never used for real provider data.
    Simulation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SessionMode {
    /// Launched and owned by Agent Office.
    Managed,
    /// Started by the user elsewhere; observed only.
    External,
}

/// Provider-independent classification of a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ToolCategory {
    Read,
    Search,
    Edit,
    Execute,
    Fetch,
    Subagent,
    Mcp,
    Think,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum OutputStream {
    Stdout,
    Stderr,
    Combined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum WaitingReason {
    Permission,
    Input,
    Other,
}

/// Who resolved a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PermissionResolver {
    /// The user clicked approve/reject in Agent Office.
    App,
    /// Resolved in the provider's own UI or by its policy.
    Provider,
    /// Nobody answered before the timeout.
    Timeout,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PermissionOption {
    /// Provider's opaque option id (echoed back when answering).
    pub id: String,
    pub kind: PermissionOptionKind,
    pub label: String,
}

/// Token usage and cost. Every field is optional: only values the provider
/// actually reported are filled in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub context_window: Option<u64>,
    /// Tokens currently in the context window (ACP `usage_update.used`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub context_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cost_usd: Option<f64>,
    /// True when the cost is an estimate (e.g. Claude Code's client-side `total_cost_usd`).
    #[serde(default)]
    pub cost_is_estimate: bool,
}

impl UsageSnapshot {
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cached_input_tokens.is_none()
            && self.reasoning_tokens.is_none()
            && self.total_tokens.is_none()
            && self.context_tokens.is_none()
            && self.cost_usd.is_none()
    }
}

// ---------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mode: Option<SessionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionEnded {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PromptSubmitted {
    /// Prompt text (may be omitted when the user disabled prompt storage).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TextNote {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentWaiting {
    pub reason: WaitingReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentError {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error_type: Option<String>,
    #[serde(default)]
    pub recoverable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ToolStarted {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tool_call_id: Option<ToolCallId>,
    pub tool_name: String,
    pub category: ToolCategory,
    /// Short human-readable description ("Edit src/main.rs", "npm test").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ToolFinished {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tool_call_id: Option<ToolCallId>,
    pub tool_name: String,
    pub category: ToolCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub duration_ms: Option<u64>,
    /// Output summary or error message (truncated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileTouched {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tool_call_id: Option<ToolCallId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandStarted {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command_id: Option<String>,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command_id: Option<String>,
    pub stream: OutputStream,
    pub chunk: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandFinished {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PermissionRequested {
    pub request_id: PermissionRequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tool_name: Option<String>,
    /// What the agent wants to do ("Run: npm install").
    pub description: String,
    /// True when Agent Office can answer this request (the UI shows Approve/Reject).
    pub can_resolve: bool,
    #[serde(default)]
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PermissionResolved {
    pub request_id: PermissionRequestId,
    pub resolved_by: PermissionResolver,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SubagentInfo {
    /// Subagent type/role ("Explore", "code-reviewer", …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GitBranchChanged {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub previous: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GitCommitCreated {
    pub sha: String,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GitStatusChanged {
    pub dirty: u32,
    pub staged: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ahead: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub behind: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderErrorInfo {
    pub component: String,
    pub message: String,
}

/// Event type + payload. Serialized adjacently: `{"type": "tool.started", "payload": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload")]
#[ts(export)]
pub enum EventKind {
    #[serde(rename = "session.started")]
    SessionStarted(SessionInfo),
    #[serde(rename = "session.updated")]
    SessionUpdated(SessionInfo),
    #[serde(rename = "session.ended")]
    SessionEnded(SessionEnded),
    #[serde(rename = "prompt.submitted")]
    PromptSubmitted(PromptSubmitted),
    #[serde(rename = "agent.thinking")]
    AgentThinking(TextNote),
    #[serde(rename = "agent.message")]
    AgentMessage(TextNote),
    #[serde(rename = "agent.idle")]
    AgentIdle(TextNote),
    #[serde(rename = "agent.waiting")]
    AgentWaiting(AgentWaiting),
    #[serde(rename = "agent.error")]
    AgentError(AgentError),
    #[serde(rename = "tool.started")]
    ToolStarted(ToolStarted),
    #[serde(rename = "tool.completed")]
    ToolCompleted(ToolFinished),
    #[serde(rename = "tool.failed")]
    ToolFailed(ToolFinished),
    #[serde(rename = "file.read")]
    FileRead(FileTouched),
    #[serde(rename = "file.created")]
    FileCreated(FileTouched),
    #[serde(rename = "file.modified")]
    FileModified(FileTouched),
    #[serde(rename = "file.deleted")]
    FileDeleted(FileTouched),
    #[serde(rename = "command.started")]
    CommandStarted(CommandStarted),
    #[serde(rename = "command.output")]
    CommandOutput(CommandOutput),
    #[serde(rename = "command.completed")]
    CommandCompleted(CommandFinished),
    #[serde(rename = "command.failed")]
    CommandFailed(CommandFinished),
    #[serde(rename = "permission.requested")]
    PermissionRequested(PermissionRequested),
    #[serde(rename = "permission.approved")]
    PermissionApproved(PermissionResolved),
    #[serde(rename = "permission.denied")]
    PermissionDenied(PermissionResolved),
    /// Agent Office stopped offering an answer (timeout); the provider's own
    /// prompt (e.g. in the terminal) is now the only way to answer.
    #[serde(rename = "permission.expired")]
    PermissionExpired(PermissionResolved),
    #[serde(rename = "subagent.started")]
    SubagentStarted(SubagentInfo),
    #[serde(rename = "subagent.updated")]
    SubagentUpdated(SubagentInfo),
    #[serde(rename = "subagent.ended")]
    SubagentEnded(SubagentInfo),
    #[serde(rename = "git.branch_changed")]
    GitBranchChanged(GitBranchChanged),
    #[serde(rename = "git.commit_created")]
    GitCommitCreated(GitCommitCreated),
    #[serde(rename = "git.status_changed")]
    GitStatusChanged(GitStatusChanged),
    #[serde(rename = "context.compacted")]
    ContextCompacted(TextNote),
    #[serde(rename = "usage.updated")]
    UsageUpdated(UsageSnapshot),
    #[serde(rename = "provider.error")]
    ProviderError(ProviderErrorInfo),
}

impl EventKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            EventKind::SessionStarted(_) => "session.started",
            EventKind::SessionUpdated(_) => "session.updated",
            EventKind::SessionEnded(_) => "session.ended",
            EventKind::PromptSubmitted(_) => "prompt.submitted",
            EventKind::AgentThinking(_) => "agent.thinking",
            EventKind::AgentMessage(_) => "agent.message",
            EventKind::AgentIdle(_) => "agent.idle",
            EventKind::AgentWaiting(_) => "agent.waiting",
            EventKind::AgentError(_) => "agent.error",
            EventKind::ToolStarted(_) => "tool.started",
            EventKind::ToolCompleted(_) => "tool.completed",
            EventKind::ToolFailed(_) => "tool.failed",
            EventKind::FileRead(_) => "file.read",
            EventKind::FileCreated(_) => "file.created",
            EventKind::FileModified(_) => "file.modified",
            EventKind::FileDeleted(_) => "file.deleted",
            EventKind::CommandStarted(_) => "command.started",
            EventKind::CommandOutput(_) => "command.output",
            EventKind::CommandCompleted(_) => "command.completed",
            EventKind::CommandFailed(_) => "command.failed",
            EventKind::PermissionRequested(_) => "permission.requested",
            EventKind::PermissionApproved(_) => "permission.approved",
            EventKind::PermissionDenied(_) => "permission.denied",
            EventKind::PermissionExpired(_) => "permission.expired",
            EventKind::SubagentStarted(_) => "subagent.started",
            EventKind::SubagentUpdated(_) => "subagent.updated",
            EventKind::SubagentEnded(_) => "subagent.ended",
            EventKind::GitBranchChanged(_) => "git.branch_changed",
            EventKind::GitCommitCreated(_) => "git.commit_created",
            EventKind::GitStatusChanged(_) => "git.status_changed",
            EventKind::ContextCompacted(_) => "context.compacted",
            EventKind::UsageUpdated(_) => "usage.updated",
            EventKind::ProviderError(_) => "provider.error",
        }
    }

    /// Identifier used to de-duplicate the same logical event arriving twice
    /// (e.g. from a global hook and a per-session hook).
    pub fn correlation_id(&self) -> Option<&str> {
        match self {
            EventKind::ToolStarted(p) => p.tool_call_id.as_ref().map(|id| id.as_str()),
            EventKind::ToolCompleted(p) | EventKind::ToolFailed(p) => {
                p.tool_call_id.as_ref().map(|id| id.as_str())
            }
            EventKind::PermissionRequested(p) => Some(p.request_id.as_str()),
            EventKind::PermissionApproved(p)
            | EventKind::PermissionDenied(p)
            | EventKind::PermissionExpired(p) => Some(p.request_id.as_str()),
            EventKind::CommandStarted(p) => p.command_id.as_deref(),
            EventKind::CommandCompleted(p) | EventKind::CommandFailed(p) => p.command_id.as_deref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_flat_envelope_with_dotted_type() {
        let event = AgentEvent::for_session(
            "claude",
            "s1",
            EventSource::Hook,
            EventKind::ToolStarted(ToolStarted {
                tool_call_id: Some("toolu_1".into()),
                tool_name: "Edit".into(),
                category: ToolCategory::Edit,
                title: Some("Edit src/main.rs".into()),
            }),
        )
        .at(1_700_000_000_000);

        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "tool.started");
        assert_eq!(json["provider"], "claude");
        assert_eq!(json["sessionId"], "s1");
        assert_eq!(json["agentId"], "s1");
        assert_eq!(json["source"], "hook");
        assert_eq!(json["timestamp"], 1_700_000_000_000i64);
        assert_eq!(json["payload"]["toolName"], "Edit");
        assert_eq!(json["payload"]["category"], "edit");
        assert!(json.get("parentAgentId").is_none());

        let back: AgentEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn validation_rejects_broken_envelopes() {
        let ok = AgentEvent::for_session(
            "claude",
            "s1",
            EventSource::Hook,
            EventKind::AgentIdle(TextNote::default()),
        );
        assert!(ok.validate().is_ok());

        let mut bad = ok.clone();
        bad.provider = ProviderId::new("Claude Code");
        assert!(bad.validate().is_err());

        let mut bad = ok.clone();
        bad.session_id = SessionId::new(" ");
        assert!(bad.validate().is_err());

        let sub_on_main = AgentEvent::for_session(
            "claude",
            "s1",
            EventSource::Hook,
            EventKind::SubagentStarted(SubagentInfo::default()),
        );
        assert!(sub_on_main.validate().is_err());
        assert!(sub_on_main
            .clone()
            .with_agent("a1", None)
            .validate()
            .is_ok());
        assert!(sub_on_main
            .with_agent("a1", Some(AgentId::new("a1")))
            .validate()
            .is_err());
    }

    #[test]
    fn type_name_matches_serde_tag_for_every_variant() {
        let kinds = vec![
            EventKind::SessionStarted(SessionInfo::default()),
            EventKind::SessionEnded(SessionEnded::default()),
            EventKind::AgentIdle(TextNote::default()),
            EventKind::UsageUpdated(UsageSnapshot::default()),
            EventKind::GitStatusChanged(GitStatusChanged::default()),
            EventKind::SubagentStarted(SubagentInfo::default()),
            EventKind::ContextCompacted(TextNote::default()),
        ];
        for kind in kinds {
            let json = serde_json::to_value(&kind).unwrap();
            assert_eq!(json["type"], kind.type_name());
        }
    }
}
