//! In-memory model of every known session and agent, folded from events by a
//! pure reducer. The backend is the single source of truth; the UI receives
//! changed [`SessionState`]/[`AgentState`] objects in batches.

use crate::activity::{activity_for_tool, looks_like_test_command, Activity};
use crate::event::{
    AgentEvent, EventKind, GitStatusChanged, PermissionRequested, SessionMode, ToolCategory,
    UsageSnapshot, WaitingReason,
};
use crate::ids::{agent_key, session_key, AgentId, ProjectId, ProviderId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use ts_rs::TS;

const MAX_TRACKED_FILES: usize = 500;
/// Finished tool ids remembered per agent to tolerate out-of-order delivery.
const FINISHED_TOOL_MEMORY: usize = 64;
const MAX_TEXT: usize = 280;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SessionStatus {
    Active,
    Ended,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionStats {
    pub prompts: u32,
    pub tool_calls: u32,
    pub failed_tools: u32,
    pub commands: u32,
    pub errors: u32,
    pub subagents: u32,
    pub commits: u32,
    /// Files the agent changed according to tool-level evidence (unique, capped).
    pub files_changed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionState {
    pub key: String,
    pub provider: ProviderId,
    pub session_id: SessionId,
    pub mode: SessionMode,
    pub status: SessionStatus,
    pub main_agent_key: String,
    pub agent_keys: Vec<String>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_status: Option<GitStatusChanged>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[ts(type = "number")]
    pub started_at: i64,
    #[ts(optional, type = "number")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
    #[ts(type = "number")]
    pub last_event_at: i64,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
    pub stats: SessionStats,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunningTool {
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub category: ToolCategory,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[ts(type = "number")]
    pub started_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentState {
    pub key: String,
    pub session_key: String,
    pub provider: ProviderId,
    pub session_id: SessionId,
    pub agent_id: AgentId,
    pub is_main: bool,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_key: Option<String>,
    pub name: String,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    pub activity: Activity,
    #[ts(type = "number")]
    pub activity_since: i64,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_action: Option<String>,
    pub running_tools: Vec<RunningTool>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_permission: Option<PermissionRequested>,
    /// When the pending permission was requested. Later work by the same agent
    /// proves the request was answered elsewhere (e.g. in the terminal).
    #[ts(optional, type = "number")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_permission_at: Option<i64>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub tool_calls: u32,
    pub ended: bool,
    #[ts(optional, type = "number")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
    /// Recently finished tool ids: hooks may be delivered out of order, so a
    /// start that arrives after its own completion must not look "running".
    #[serde(skip)]
    #[ts(skip)]
    finished_tools: VecDeque<String>,
}

impl AgentState {
    #[allow(clippy::too_many_arguments)]
    fn new(
        key: String,
        session_key: String,
        e: &AgentEvent,
        agent_id: AgentId,
        is_main: bool,
        parent_key: Option<String>,
        name: String,
        activity: Activity,
    ) -> Self {
        Self {
            key,
            session_key,
            provider: e.provider.clone(),
            session_id: e.session_id.clone(),
            agent_id,
            is_main,
            parent_key,
            name,
            agent_type: None,
            activity,
            activity_since: e.timestamp,
            current_action: None,
            running_tools: Vec::new(),
            pending_permission: None,
            pending_permission_at: None,
            last_message: None,
            last_error: None,
            tool_calls: 0,
            ended: false,
            ended_at: None,
            finished_tools: VecDeque::new(),
        }
    }

    fn remember_finished(&mut self, id: &str) {
        if self.finished_tools.len() >= FINISHED_TOOL_MEMORY {
            self.finished_tools.pop_front();
        }
        self.finished_tools.push_back(id.to_owned());
    }

    /// Clears a pending permission when later activity shows it was answered elsewhere.
    fn clear_answered_permission(&mut self, at: i64) {
        if self.pending_permission.is_some() && self.pending_permission_at.is_some_and(|t| at > t) {
            self.pending_permission = None;
            self.pending_permission_at = None;
            if matches!(self.activity, Activity::WaitingPermission) {
                self.activity = Activity::Thinking;
                self.activity_since = at;
            }
        }
    }

    fn set_activity(&mut self, activity: Activity, at: i64) {
        if self.activity != activity {
            self.activity = activity;
            self.activity_since = at;
        }
    }

    /// Activity after something finished: resume the most recent running tool,
    /// otherwise go back to thinking (unless the agent is idle/waiting/errored/done).
    fn settle(&mut self, at: i64) {
        if let Some(tool) = self.running_tools.last() {
            let activity = activity_for_tool(tool.category, tool.title.as_deref());
            self.current_action = tool.title.clone().or_else(|| Some(tool.name.clone()));
            self.set_activity(activity, at);
        } else if !matches!(
            self.activity,
            Activity::Idle
                | Activity::WaitingPermission
                | Activity::WaitingInput
                | Activity::Error
                | Activity::Done
        ) {
            self.current_action = None;
            self.set_activity(Activity::Thinking, at);
        }
    }
}

/// Keys touched by an event; used to send only changed objects to the UI.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Touched {
    pub sessions: BTreeSet<String>,
    pub agents: BTreeSet<String>,
}

impl Touched {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty() && self.agents.is_empty()
    }

    pub fn merge(&mut self, other: Touched) {
        self.sessions.extend(other.sessions);
        self.agents.extend(other.agents);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorldSnapshot {
    pub sessions: Vec<SessionState>,
    pub agents: Vec<AgentState>,
}

#[derive(Debug, Default, Clone)]
pub struct WorldState {
    sessions: BTreeMap<String, SessionState>,
    agents: BTreeMap<String, AgentState>,
}

fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT {
        text.to_owned()
    } else {
        let mut s: String = text.chars().take(MAX_TEXT - 1).collect();
        s.push('…');
        s
    }
}

fn default_name(title: Option<&str>, cwd: Option<&str>, session: &SessionId) -> String {
    if let Some(t) = title.filter(|t| !t.trim().is_empty()) {
        return clip(t);
    }
    if let Some(folder) = cwd
        .and_then(|c| c.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next())
        .filter(|f| !f.is_empty())
    {
        return folder.to_owned();
    }
    session.0.chars().take(8).collect()
}

impl WorldState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn session(&self, key: &str) -> Option<&SessionState> {
        self.sessions.get(key)
    }

    pub fn agent(&self, key: &str) -> Option<&AgentState> {
        self.agents.get(key)
    }

    pub fn sessions(&self) -> impl Iterator<Item = &SessionState> {
        self.sessions.values()
    }

    pub fn agents(&self) -> impl Iterator<Item = &AgentState> {
        self.agents.values()
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            sessions: self.sessions.values().cloned().collect(),
            agents: self.agents.values().cloned().collect(),
        }
    }

    /// Restores sessions loaded from the database (e.g. at startup).
    pub fn restore(&mut self, snapshot: WorldSnapshot) {
        for s in snapshot.sessions {
            self.sessions.insert(s.key.clone(), s);
        }
        for a in snapshot.agents {
            self.agents.insert(a.key.clone(), a);
        }
    }

    /// Removes ended sessions (and their agents) that ended before `before_ms`.
    /// Returns the removed (session keys, agent keys).
    pub fn prune_ended(&mut self, before_ms: i64) -> (Vec<String>, Vec<String>) {
        let stale: Vec<String> = self
            .sessions
            .values()
            .filter(|s| s.status == SessionStatus::Ended && s.ended_at.unwrap_or(0) < before_ms)
            .map(|s| s.key.clone())
            .collect();
        let mut removed_agents = Vec::new();
        for key in &stale {
            if let Some(session) = self.sessions.remove(key) {
                for agent_key in session.agent_keys {
                    if self.agents.remove(&agent_key).is_some() {
                        removed_agents.push(agent_key);
                    }
                }
            }
        }
        (stale, removed_agents)
    }

    fn ensure_session(&mut self, e: &AgentEvent) -> String {
        let key = session_key(&e.provider, &e.session_id);
        if !self.sessions.contains_key(&key) {
            let mode = match &e.kind {
                EventKind::SessionStarted(info) => info.mode.unwrap_or(SessionMode::External),
                _ => SessionMode::External,
            };
            let main_id = AgentId(e.session_id.0.clone());
            let main_key = agent_key(&e.provider, &e.session_id, &main_id);
            let (title, cwd) = match &e.kind {
                EventKind::SessionStarted(info) => (info.title.clone(), info.cwd.clone()),
                _ => (None, None),
            };
            self.sessions.insert(
                key.clone(),
                SessionState {
                    key: key.clone(),
                    provider: e.provider.clone(),
                    session_id: e.session_id.clone(),
                    mode,
                    status: SessionStatus::Active,
                    main_agent_key: main_key.clone(),
                    agent_keys: vec![main_key.clone()],
                    project_id: e.project_id.clone(),
                    cwd: cwd.clone(),
                    model: None,
                    title: title.clone(),
                    permission_mode: None,
                    branch: None,
                    worktree: None,
                    git_status: None,
                    pid: None,
                    started_at: e.timestamp,
                    ended_at: None,
                    last_event_at: e.timestamp,
                    end_reason: None,
                    stats: SessionStats::default(),
                    usage: None,
                },
            );
            let name = default_name(title.as_deref(), cwd.as_deref(), &e.session_id);
            self.agents.insert(
                main_key.clone(),
                AgentState::new(
                    main_key,
                    key.clone(),
                    e,
                    main_id,
                    true,
                    None,
                    name,
                    Activity::Idle,
                ),
            );
        }
        key
    }

    /// Returns the agent key, creating the subagent if needed. Returns `None`
    /// for events that must not create an unknown subagent: a stop/idle/message
    /// for an agent never seen (e.g. Claude's internal helper agents also fire
    /// SubagentStop) would otherwise spawn a ghost character.
    fn ensure_agent(&mut self, e: &AgentEvent, skey: &str) -> Option<String> {
        let key = agent_key(&e.provider, &e.session_id, &e.agent_id);
        if !self.agents.contains_key(&key) {
            if matches!(
                e.kind,
                EventKind::SubagentEnded(_) | EventKind::AgentIdle(_) | EventKind::AgentMessage(_)
            ) {
                return None;
            }
            let parent_key = Some(match &e.parent_agent_id {
                Some(parent) => agent_key(&e.provider, &e.session_id, parent),
                None => self.sessions[skey].main_agent_key.clone(),
            });
            self.agents.insert(
                key.clone(),
                AgentState::new(
                    key.clone(),
                    skey.to_owned(),
                    e,
                    e.agent_id.clone(),
                    false,
                    parent_key,
                    "Subagent".into(),
                    Activity::Thinking,
                ),
            );
            if let Some(session) = self.sessions.get_mut(skey) {
                session.agent_keys.push(key.clone());
            }
        }
        Some(key)
    }

    /// Folds one event into the state. Returns what changed.
    pub fn apply(&mut self, e: &AgentEvent) -> Touched {
        let mut touched = Touched::default();
        let skey = session_key(&e.provider, &e.session_id);

        // Provider-level errors without a known session only go to diagnostics.
        if matches!(e.kind, EventKind::ProviderError(_)) && !self.sessions.contains_key(&skey) {
            return touched;
        }

        let skey = self.ensure_session(e);
        touched.sessions.insert(skey.clone());
        let Some(akey) = self.ensure_agent(e, &skey) else {
            return touched;
        };
        touched.agents.insert(akey.clone());
        let at = e.timestamp;
        if matches!(
            e.kind,
            EventKind::ToolStarted(_)
                | EventKind::ToolCompleted(_)
                | EventKind::ToolFailed(_)
                | EventKind::CommandCompleted(_)
                | EventKind::CommandFailed(_)
                | EventKind::PromptSubmitted(_)
        ) {
            self.agents
                .get_mut(&akey)
                .expect("agent exists")
                .clear_answered_permission(at);
        }

        {
            let session = self.sessions.get_mut(&skey).expect("session exists");
            if at > session.last_event_at {
                session.last_event_at = at;
            }
            if session.project_id.is_none() {
                session.project_id = e.project_id.clone();
            }
        }

        match &e.kind {
            EventKind::SessionStarted(info) | EventKind::SessionUpdated(info) => {
                let resumed = matches!(e.kind, EventKind::SessionStarted(_));
                let session = self.sessions.get_mut(&skey).unwrap();
                if let Some(mode) = info.mode {
                    session.mode = mode;
                }
                if info.cwd.is_some() {
                    session.cwd = info.cwd.clone();
                }
                if info.model.is_some() {
                    session.model = info.model.clone();
                }
                if info.title.is_some() {
                    session.title = info.title.clone();
                }
                if info.permission_mode.is_some() {
                    session.permission_mode = info.permission_mode.clone();
                }
                if info.worktree.is_some() {
                    session.worktree = info.worktree.clone();
                }
                if info.pid.is_some() {
                    session.pid = info.pid;
                }
                let reopen = resumed && session.status == SessionStatus::Ended;
                if reopen {
                    session.status = SessionStatus::Active;
                    session.ended_at = None;
                    session.end_reason = None;
                }
                let name = default_name(
                    session.title.as_deref(),
                    session.cwd.as_deref(),
                    &session.session_id,
                );
                let main_key = session.main_agent_key.clone();
                touched.agents.insert(main_key.clone());
                let main = self.agents.get_mut(&main_key).unwrap();
                main.name = name;
                if reopen {
                    main.ended = false;
                    main.ended_at = None;
                    main.set_activity(Activity::Idle, at);
                }
            }
            EventKind::SessionEnded(info) => {
                let session = self.sessions.get_mut(&skey).unwrap();
                session.status = SessionStatus::Ended;
                session.ended_at = Some(at);
                session.end_reason = info.reason.clone();
                let keys = session.agent_keys.clone();
                for key in keys {
                    if let Some(agent) = self.agents.get_mut(&key) {
                        agent.running_tools.clear();
                        agent.pending_permission = None;
                        agent.pending_permission_at = None;
                        agent.current_action = None;
                        agent.ended = true;
                        agent.ended_at.get_or_insert(at);
                        agent.set_activity(Activity::Done, at);
                        touched.agents.insert(key);
                    }
                }
            }
            EventKind::PromptSubmitted(_) => {
                self.sessions.get_mut(&skey).unwrap().stats.prompts += 1;
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.current_action = Some("Working on a new prompt".into());
                agent.set_activity(Activity::Thinking, at);
            }
            EventKind::AgentThinking(note) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.current_action = note.text.as_deref().map(clip);
                agent.set_activity(Activity::Thinking, at);
            }
            EventKind::AgentMessage(note) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.last_message = note.text.as_deref().map(clip);
            }
            EventKind::AgentIdle(note) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.running_tools.clear();
                agent.pending_permission = None;
                agent.pending_permission_at = None;
                agent.current_action = note.text.as_deref().map(clip);
                agent.set_activity(Activity::Idle, at);
            }
            EventKind::AgentWaiting(waiting) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                match waiting.reason {
                    WaitingReason::Permission => {
                        agent.current_action = Some(
                            waiting
                                .message
                                .as_deref()
                                .map(clip)
                                .unwrap_or_else(|| "Waiting for permission".into()),
                        );
                        agent.set_activity(Activity::WaitingPermission, at);
                    }
                    WaitingReason::Input => {
                        agent.current_action = Some(
                            waiting
                                .message
                                .as_deref()
                                .map(clip)
                                .unwrap_or_else(|| "Waiting for your answer".into()),
                        );
                        agent.set_activity(Activity::WaitingInput, at);
                    }
                    WaitingReason::Other => {
                        agent.current_action = Some(
                            waiting
                                .message
                                .as_deref()
                                .map(clip)
                                .unwrap_or_else(|| "Waiting".into()),
                        );
                        agent.set_activity(Activity::Idle, at);
                    }
                }
            }
            EventKind::AgentError(err) => {
                self.sessions.get_mut(&skey).unwrap().stats.errors += 1;
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.last_error = Some(clip(&err.message));
                agent.current_action = Some(clip(&err.message));
                agent.set_activity(Activity::Error, at);
            }
            EventKind::ToolStarted(tool) => {
                self.sessions.get_mut(&skey).unwrap().stats.tool_calls += 1;
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.tool_calls += 1;
                let already_finished = tool
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| agent.finished_tools.iter().any(|f| f == &id.0));
                if already_finished {
                    // The completion overtook the start: count it, don't show it as running.
                    return touched;
                }
                agent.running_tools.push(RunningTool {
                    id: tool.tool_call_id.as_ref().map(|id| id.0.clone()),
                    name: tool.tool_name.clone(),
                    category: tool.category,
                    title: tool.title.as_deref().map(clip),
                    started_at: at,
                });
                agent.current_action = Some(clip(tool.title.as_deref().unwrap_or(&tool.tool_name)));
                agent.set_activity(activity_for_tool(tool.category, tool.title.as_deref()), at);
            }
            EventKind::ToolCompleted(tool) | EventKind::ToolFailed(tool) => {
                if matches!(e.kind, EventKind::ToolFailed(_)) {
                    self.sessions.get_mut(&skey).unwrap().stats.failed_tools += 1;
                }
                let agent = self.agents.get_mut(&akey).unwrap();
                let id = tool.tool_call_id.as_ref().map(|id| id.0.as_str());
                let position = match id {
                    Some(id) => agent
                        .running_tools
                        .iter()
                        .position(|t| t.id.as_deref() == Some(id)),
                    None => agent
                        .running_tools
                        .iter()
                        .position(|t| t.name == tool.tool_name),
                };
                if let Some(index) = position {
                    agent.running_tools.remove(index);
                } else if let Some(id) = id {
                    agent.remember_finished(id);
                }
                if let (EventKind::ToolFailed(_), Some(detail)) = (&e.kind, &tool.detail) {
                    agent.last_error = Some(clip(detail));
                }
                agent.settle(at);
            }
            EventKind::FileRead(_) => {}
            EventKind::FileCreated(file)
            | EventKind::FileModified(file)
            | EventKind::FileDeleted(file) => {
                let stats = &mut self.sessions.get_mut(&skey).unwrap().stats;
                if !stats.files_changed.contains(&file.path)
                    && stats.files_changed.len() < MAX_TRACKED_FILES
                {
                    stats.files_changed.push(file.path.clone());
                }
            }
            EventKind::CommandStarted(cmd) => {
                self.sessions.get_mut(&skey).unwrap().stats.commands += 1;
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.current_action = Some(clip(&cmd.command));
                let activity = if looks_like_test_command(&cmd.command) {
                    Activity::Testing
                } else {
                    Activity::RunningCommand
                };
                agent.set_activity(activity, at);
            }
            EventKind::CommandOutput(_) => {}
            EventKind::CommandCompleted(_) | EventKind::CommandFailed(_) => {
                self.agents.get_mut(&akey).unwrap().settle(at);
            }
            EventKind::PermissionRequested(request) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.current_action = Some(clip(&request.description));
                agent.pending_permission = Some(request.clone());
                agent.pending_permission_at = Some(at);
                agent.set_activity(Activity::WaitingPermission, at);
            }
            EventKind::PermissionExpired(resolved) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                if agent
                    .pending_permission
                    .as_ref()
                    .is_some_and(|p| p.request_id == resolved.request_id)
                {
                    agent.pending_permission = None;
                    agent.pending_permission_at = None;
                    agent.current_action =
                        Some(resolved.message.as_deref().map(clip).unwrap_or_else(|| {
                            "Waiting for permission in the agent's own prompt".into()
                        }));
                }
            }
            EventKind::PermissionApproved(resolved) | EventKind::PermissionDenied(resolved) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                if agent
                    .pending_permission
                    .as_ref()
                    .is_some_and(|p| p.request_id == resolved.request_id)
                {
                    agent.pending_permission = None;
                    agent.pending_permission_at = None;
                }
                if agent.activity == Activity::WaitingPermission
                    && agent.pending_permission.is_none()
                {
                    agent.activity = Activity::Thinking;
                    agent.activity_since = at;
                    agent.settle(at);
                }
            }
            EventKind::SubagentStarted(info) | EventKind::SubagentUpdated(info) => {
                if matches!(e.kind, EventKind::SubagentStarted(_)) {
                    self.sessions.get_mut(&skey).unwrap().stats.subagents += 1;
                }
                let agent = self.agents.get_mut(&akey).unwrap();
                if info.agent_type.is_some() {
                    agent.agent_type = info.agent_type.clone();
                    agent.name = clip(info.agent_type.as_deref().unwrap_or("Subagent"));
                }
                if let Some(description) = &info.description {
                    agent.current_action = Some(clip(description));
                }
                if !agent.ended && agent.activity == Activity::Idle {
                    agent.set_activity(Activity::Thinking, at);
                }
            }
            EventKind::SubagentEnded(_) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.running_tools.clear();
                agent.pending_permission = None;
                agent.pending_permission_at = None;
                agent.ended = true;
                agent.ended_at = Some(at);
                agent.set_activity(Activity::Done, at);
            }
            EventKind::GitBranchChanged(change) => {
                self.sessions.get_mut(&skey).unwrap().branch = change.branch.clone();
            }
            EventKind::GitCommitCreated(_) => {
                self.sessions.get_mut(&skey).unwrap().stats.commits += 1;
            }
            EventKind::GitStatusChanged(status) => {
                self.sessions.get_mut(&skey).unwrap().git_status = Some(status.clone());
            }
            EventKind::ContextCompacted(_) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.current_action = Some("Compacting context".into());
                agent.set_activity(Activity::Thinking, at);
            }
            EventKind::UsageUpdated(usage) => {
                let session = self.sessions.get_mut(&skey).unwrap();
                let merged = session.usage.get_or_insert_with(UsageSnapshot::default);
                merge_usage(merged, usage);
            }
            EventKind::ProviderError(err) => {
                let agent = self.agents.get_mut(&akey).unwrap();
                agent.last_error = Some(clip(&err.message));
            }
        }
        touched
    }
}

/// Usage snapshots are cumulative: fields present in `update` replace old values.
fn merge_usage(target: &mut UsageSnapshot, update: &UsageSnapshot) {
    macro_rules! take {
        ($field:ident) => {
            if update.$field.is_some() {
                target.$field = update.$field;
            }
        };
    }
    take!(input_tokens);
    take!(output_tokens);
    take!(cached_input_tokens);
    take!(reasoning_tokens);
    take!(total_tokens);
    take!(context_window);
    if update.cost_usd.is_some() {
        target.cost_usd = update.cost_usd;
        target.cost_is_estimate = update.cost_is_estimate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::*;

    fn ev(kind: EventKind, at: i64) -> AgentEvent {
        AgentEvent::for_session("claude", "s1", EventSource::Hook, kind).at(at)
    }

    fn main_agent(world: &WorldState) -> &AgentState {
        world.agent("claude:s1:s1").unwrap()
    }

    fn tool(id: &str, name: &str, category: ToolCategory, title: &str) -> ToolStarted {
        ToolStarted {
            tool_call_id: Some(id.into()),
            tool_name: name.into(),
            category,
            title: Some(title.into()),
        }
    }

    fn done(id: &str, name: &str, category: ToolCategory) -> ToolFinished {
        ToolFinished {
            tool_call_id: Some(id.into()),
            tool_name: name.into(),
            category,
            duration_ms: Some(10),
            detail: None,
        }
    }

    #[test]
    fn full_turn_lifecycle() {
        let mut w = WorldState::new();
        w.apply(&ev(
            EventKind::SessionStarted(SessionInfo {
                mode: Some(SessionMode::External),
                cwd: Some("C:\\Projects\\Nalu".into()),
                model: Some("opus".into()),
                ..Default::default()
            }),
            1,
        ));
        assert_eq!(main_agent(&w).name, "Nalu");
        assert_eq!(main_agent(&w).activity, Activity::Idle);

        w.apply(&ev(
            EventKind::PromptSubmitted(PromptSubmitted {
                text: Some("fix".into()),
            }),
            2,
        ));
        assert_eq!(main_agent(&w).activity, Activity::Thinking);

        w.apply(&ev(
            EventKind::ToolStarted(tool("t1", "Edit", ToolCategory::Edit, "Edit a.rs")),
            3,
        ));
        assert_eq!(main_agent(&w).activity, Activity::Coding);
        assert_eq!(main_agent(&w).activity_since, 3);

        w.apply(&ev(
            EventKind::FileModified(FileTouched {
                path: "a.rs".into(),
                tool_call_id: None,
            }),
            4,
        ));
        w.apply(&ev(
            EventKind::ToolCompleted(done("t1", "Edit", ToolCategory::Edit)),
            5,
        ));
        assert_eq!(main_agent(&w).activity, Activity::Thinking);
        assert!(main_agent(&w).running_tools.is_empty());

        w.apply(&ev(
            EventKind::ToolStarted(tool("t2", "Bash", ToolCategory::Execute, "cargo test")),
            6,
        ));
        assert_eq!(main_agent(&w).activity, Activity::Testing);
        w.apply(&ev(
            EventKind::ToolFailed(done("t2", "Bash", ToolCategory::Execute)),
            7,
        ));

        w.apply(&ev(EventKind::AgentIdle(TextNote::default()), 8));
        assert_eq!(main_agent(&w).activity, Activity::Idle);

        let s = w.session("claude:s1").unwrap();
        assert_eq!(s.stats.tool_calls, 2);
        assert_eq!(s.stats.failed_tools, 1);
        assert_eq!(s.stats.prompts, 1);
        assert_eq!(s.stats.files_changed, vec!["a.rs".to_string()]);
        assert_eq!(s.model.as_deref(), Some("opus"));

        w.apply(&ev(EventKind::SessionEnded(SessionEnded::default()), 9));
        assert_eq!(main_agent(&w).activity, Activity::Done);
        assert_eq!(w.session("claude:s1").unwrap().status, SessionStatus::Ended);
    }

    #[test]
    fn nested_tools_resume_outer_activity() {
        let mut w = WorldState::new();
        w.apply(&ev(
            EventKind::ToolStarted(tool("a", "Read", ToolCategory::Read, "Read x")),
            1,
        ));
        w.apply(&ev(
            EventKind::ToolStarted(tool("b", "Bash", ToolCategory::Execute, "ls")),
            2,
        ));
        assert_eq!(main_agent(&w).activity, Activity::RunningCommand);
        w.apply(&ev(
            EventKind::ToolCompleted(done("b", "Bash", ToolCategory::Execute)),
            3,
        ));
        assert_eq!(main_agent(&w).activity, Activity::Reading);
    }

    #[test]
    fn permission_flow() {
        let mut w = WorldState::new();
        let request = PermissionRequested {
            request_id: "p1".into(),
            tool_name: Some("Bash".into()),
            description: "Run: rm -rf build".into(),
            can_resolve: true,
            options: vec![],
        };
        w.apply(&ev(EventKind::PermissionRequested(request), 1));
        assert_eq!(main_agent(&w).activity, Activity::WaitingPermission);
        assert!(main_agent(&w).pending_permission.is_some());
        w.apply(&ev(
            EventKind::PermissionApproved(PermissionResolved {
                request_id: "p1".into(),
                resolved_by: PermissionResolver::App,
                message: None,
            }),
            2,
        ));
        assert!(main_agent(&w).pending_permission.is_none());
        assert_eq!(main_agent(&w).activity, Activity::Thinking);
    }

    #[test]
    fn subagents_link_to_parent_and_end() {
        let mut w = WorldState::new();
        w.apply(&ev(EventKind::SessionStarted(SessionInfo::default()), 1));
        let sub = ev(
            EventKind::SubagentStarted(SubagentInfo {
                agent_type: Some("Explore".into()),
                description: Some("Find the router".into()),
                reason: None,
            }),
            2,
        )
        .with_agent("sub-1", None);
        let touched = w.apply(&sub);
        assert!(touched.agents.contains("claude:s1:sub-1"));
        let a = w.agent("claude:s1:sub-1").unwrap();
        assert_eq!(a.parent_key.as_deref(), Some("claude:s1:s1"));
        assert_eq!(a.name, "Explore");
        assert!(!a.is_main);
        assert_eq!(w.session("claude:s1").unwrap().agent_keys.len(), 2);

        w.apply(
            &ev(EventKind::SubagentEnded(SubagentInfo::default()), 3).with_agent("sub-1", None),
        );
        let a = w.agent("claude:s1:sub-1").unwrap();
        assert!(a.ended);
        assert_eq!(a.activity, Activity::Done);
    }

    #[test]
    fn completion_overtaking_start_does_not_leave_a_running_tool() {
        let mut w = WorldState::new();
        w.apply(&ev(
            EventKind::ToolCompleted(done("t1", "Read", ToolCategory::Read)),
            5,
        ));
        w.apply(&ev(
            EventKind::ToolStarted(tool("t1", "Read", ToolCategory::Read, "Read a")),
            4,
        ));
        assert!(main_agent(&w).running_tools.is_empty());
        assert_ne!(main_agent(&w).activity, Activity::Reading);
        assert_eq!(w.session("claude:s1").unwrap().stats.tool_calls, 1);
    }

    #[test]
    fn permission_answered_elsewhere_is_cleared_by_later_work() {
        let mut w = WorldState::new();
        w.apply(&ev(
            EventKind::ToolStarted(tool("t1", "Bash", ToolCategory::Execute, "rm -rf build")),
            1,
        ));
        let request = PermissionRequested {
            request_id: "perm-1".into(),
            tool_name: Some("Bash".into()),
            description: "Run: rm -rf build".into(),
            can_resolve: false,
            options: vec![],
        };
        w.apply(&ev(EventKind::PermissionRequested(request), 2));
        // A PreToolUse delivered late (older timestamp) must not clear it.
        w.apply(&ev(
            EventKind::ToolStarted(tool("t0", "Read", ToolCategory::Read, "Read x")),
            1,
        ));
        assert!(main_agent(&w).pending_permission.is_some());
        // The tool finishing afterwards proves the user answered in the terminal.
        w.apply(&ev(
            EventKind::ToolCompleted(done("t1", "Bash", ToolCategory::Execute)),
            3,
        ));
        assert!(main_agent(&w).pending_permission.is_none());
        assert_ne!(main_agent(&w).activity, Activity::WaitingPermission);
    }

    #[test]
    fn expired_permission_keeps_waiting_without_app_answer() {
        let mut w = WorldState::new();
        let request = PermissionRequested {
            request_id: "perm-1".into(),
            tool_name: None,
            description: "Edit a.rs".into(),
            can_resolve: true,
            options: vec![],
        };
        w.apply(&ev(EventKind::PermissionRequested(request), 1));
        w.apply(&ev(
            EventKind::PermissionExpired(PermissionResolved {
                request_id: "perm-1".into(),
                resolved_by: PermissionResolver::Timeout,
                message: None,
            }),
            2,
        ));
        assert!(main_agent(&w).pending_permission.is_none());
        assert_eq!(main_agent(&w).activity, Activity::WaitingPermission);
    }

    #[test]
    fn stop_of_unknown_subagent_does_not_create_a_ghost() {
        let mut w = WorldState::new();
        w.apply(&ev(EventKind::SessionStarted(SessionInfo::default()), 1));
        w.apply(
            &ev(EventKind::SubagentEnded(SubagentInfo::default()), 2)
                .with_agent("internal-1", None),
        );
        assert_eq!(w.agents().count(), 1);
        assert_eq!(w.session("claude:s1").unwrap().agent_keys.len(), 1);
    }

    #[test]
    fn waiting_for_input_has_its_own_activity() {
        let mut w = WorldState::new();
        w.apply(&ev(
            EventKind::AgentWaiting(AgentWaiting {
                reason: WaitingReason::Input,
                message: None,
            }),
            1,
        ));
        assert_eq!(main_agent(&w).activity, Activity::WaitingInput);
        w.apply(&ev(
            EventKind::ToolStarted(tool("t1", "Read", ToolCategory::Read, "Read a")),
            2,
        ));
        assert_eq!(main_agent(&w).activity, Activity::Reading);
    }

    #[test]
    fn usage_merges_cumulative_fields() {
        let mut w = WorldState::new();
        w.apply(&ev(
            EventKind::UsageUpdated(UsageSnapshot {
                input_tokens: Some(10),
                output_tokens: Some(5),
                ..Default::default()
            }),
            1,
        ));
        w.apply(&ev(
            EventKind::UsageUpdated(UsageSnapshot {
                output_tokens: Some(7),
                cost_usd: Some(0.01),
                cost_is_estimate: true,
                ..Default::default()
            }),
            2,
        ));
        let usage = w.session("claude:s1").unwrap().usage.clone().unwrap();
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.cost_usd, Some(0.01));
        assert!(usage.cost_is_estimate);
    }

    #[test]
    fn provider_error_without_session_is_ignored() {
        let mut w = WorldState::new();
        let touched = w.apply(&ev(
            EventKind::ProviderError(ProviderErrorInfo {
                component: "adapter".into(),
                message: "boom".into(),
            }),
            1,
        ));
        assert!(touched.is_empty());
        assert_eq!(w.sessions().count(), 0);
    }

    #[test]
    fn resume_reopens_ended_session_and_prune_removes_old_ones() {
        let mut w = WorldState::new();
        w.apply(&ev(EventKind::SessionStarted(SessionInfo::default()), 1));
        w.apply(&ev(EventKind::SessionEnded(SessionEnded::default()), 2));
        w.apply(&ev(EventKind::SessionStarted(SessionInfo::default()), 3));
        assert_eq!(
            w.session("claude:s1").unwrap().status,
            SessionStatus::Active
        );
        assert!(!main_agent(&w).ended);

        w.apply(&ev(EventKind::SessionEnded(SessionEnded::default()), 4));
        let (sessions, agents) = w.prune_ended(10);
        assert_eq!(sessions, vec!["claude:s1".to_string()]);
        assert_eq!(agents, vec!["claude:s1:s1".to_string()]);
        assert_eq!(w.agents().count(), 0);
    }
}
