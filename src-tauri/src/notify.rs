//! Desktop notifications: which events deserve one, and how often.
//!
//! The rules only look at unified events and the resulting state, so they
//! work the same for every provider. Delivery (Windows toasts) is done by the
//! app shell through a [`NoticeSink`]; clicking a notice opens its agent.
//!
//! Kept quiet on purpose: simulated (demo) sessions never notify, the same
//! kind of notice for the same session waits a cooldown, and bursts are
//! capped (the next notice mentions how many were held back).

use ao_core::activity::{looks_like_test_command, Activity};
use ao_core::event::{AgentEvent, EventKind, ToolCategory, WaitingReason};
use ao_core::world::{AgentState, SessionState};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use ts_rs::TS;

/// Same kind of notice for the same session: at most once per this long.
const COOLDOWN_MS: i64 = 20_000;
/// At most this many notices per burst window.
const BURST_MAX: usize = 3;
const BURST_WINDOW_MS: i64 = 10_000;
/// A finished turn is only worth a notice after this much work.
const FINISHED_MIN_MS: i64 = 30_000;
/// Events older than this (a backlog) never notify.
const STALE_MS: i64 = 120_000;
const BODY_MAX_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct NotificationPrefs {
    pub enabled: bool,
    /// Only while Agent Office is not the active window.
    pub only_in_background: bool,
    /// An agent asks for permission.
    pub permission: bool,
    /// An agent waits for your answer.
    pub input: bool,
    /// An agent stopped with an error.
    pub errors: bool,
    /// A test run finished (passed or failed).
    pub tests: bool,
    /// An agent finished a long piece of work and waits for a new prompt.
    pub finished: bool,
    /// A session ended.
    pub session_ended: bool,
    /// Include the command, file or message (clipped, secrets already
    /// redacted). Off: only who needs you and why.
    pub show_details: bool,
}

impl Default for NotificationPrefs {
    fn default() -> Self {
        Self {
            enabled: true,
            only_in_background: true,
            permission: true,
            input: true,
            errors: true,
            tests: true,
            finished: true,
            session_ended: false,
            show_details: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoticeKind {
    Permission,
    Input,
    Error,
    Tests,
    Finished,
    SessionEnded,
    /// Diagnostics → "Send a test notification".
    Test,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Notice {
    pub kind: NoticeKind,
    pub title: String,
    pub body: String,
    /// The agent to open when the notice is clicked.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_key: Option<String>,
    #[ts(type = "number")]
    pub at: i64,
}

/// Delivers notices; `force` skips the "only in the background" check
/// (test notices).
pub type NoticeSink = Box<dyn Fn(Notice, bool) + Send + Sync>;

/// What the rules need to know about the event's provider.
pub struct ProviderLabel<'a> {
    pub name: &'a str,
    pub simulated: bool,
}

#[derive(Default)]
pub struct NoticeRules {
    /// When each agent's current piece of work started.
    busy_since: HashMap<String, i64>,
    last: HashMap<(String, NoticeKind), i64>,
    recent: VecDeque<i64>,
    held_back: usize,
}

fn busy(activity: Activity) -> bool {
    matches!(
        activity,
        Activity::Thinking
            | Activity::Reading
            | Activity::Coding
            | Activity::RunningCommand
            | Activity::Testing
            | Activity::WaitingPermission
    )
}

fn clip(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= BODY_MAX_CHARS {
        one_line
    } else {
        let cut: String = one_line.chars().take(BODY_MAX_CHARS - 1).collect();
        format!("{cut}…")
    }
}

fn minutes(ms: i64) -> String {
    let m = (ms / 60_000).max(1);
    if m == 1 {
        "1 minute".into()
    } else {
        format!("{m} minutes")
    }
}

impl NoticeRules {
    /// Looks at one accepted event. `before` is the agent's activity before
    /// the event was applied; `agent`/`session` are the state after it.
    #[allow(clippy::too_many_arguments)]
    pub fn observe(
        &mut self,
        event: &AgentEvent,
        before: Option<Activity>,
        agent: Option<&AgentState>,
        session: Option<&SessionState>,
        provider: &ProviderLabel<'_>,
        prefs: &NotificationPrefs,
        now: i64,
    ) -> Option<Notice> {
        let (agent, _session) = (agent?, session?);
        // Track the start of each piece of work, whatever the preferences.
        let after = agent.activity;
        if busy(after) && !before.is_some_and(busy) {
            self.busy_since
                .entry(agent.key.clone())
                .or_insert(event.timestamp);
        }
        let worked = if matches!(
            event.kind,
            EventKind::AgentIdle(_) | EventKind::SessionEnded(_)
        ) || agent.ended
        {
            self.busy_since
                .remove(&agent.key)
                .map(|start| event.timestamp - start)
        } else {
            None
        };

        if !prefs.enabled || provider.simulated || now - event.timestamp > STALE_MS {
            return None;
        }
        let who = &agent.name;
        let details = |text: &str| {
            if prefs.show_details && !text.trim().is_empty() {
                format!("{who}: {}", clip(text))
            } else {
                who.clone()
            }
        };
        let name = provider.name;
        let (kind, title, body) = match &event.kind {
            EventKind::PermissionRequested(p) if prefs.permission && !agent.ended => (
                NoticeKind::Permission,
                format!("{name} needs permission"),
                details(&p.description),
            ),
            EventKind::AgentWaiting(w) if prefs.input && w.reason == WaitingReason::Input => (
                NoticeKind::Input,
                format!("{name} is waiting for your answer"),
                details(w.message.as_deref().unwrap_or_default()),
            ),
            EventKind::AgentError(e) if prefs.errors && !e.recoverable => (
                NoticeKind::Error,
                format!("{name} encountered an error"),
                details(&e.message),
            ),
            EventKind::CommandCompleted(c) | EventKind::CommandFailed(c)
                if prefs.tests
                    && (before == Some(Activity::Testing)
                        || c.command.as_deref().is_some_and(looks_like_test_command)) =>
            {
                let failed = matches!(event.kind, EventKind::CommandFailed(_))
                    || c.exit_code.is_some_and(|code| code != 0);
                let outcome = match (failed, c.exit_code) {
                    (false, _) => "Tests passed".to_owned(),
                    (true, Some(code)) => format!("Tests failed (exit code {code})"),
                    (true, None) => "Tests failed".to_owned(),
                };
                (
                    NoticeKind::Tests,
                    format!("{name} finished tests"),
                    details(&outcome),
                )
            }
            EventKind::ToolCompleted(t) | EventKind::ToolFailed(t)
                if prefs.tests
                    && t.category == ToolCategory::Execute
                    && before == Some(Activity::Testing) =>
            {
                let outcome = if matches!(event.kind, EventKind::ToolFailed(_)) {
                    "Tests failed"
                } else {
                    "Tests finished"
                };
                (
                    NoticeKind::Tests,
                    format!("{name} finished tests"),
                    details(outcome),
                )
            }
            EventKind::AgentIdle(_)
                if prefs.finished && worked.is_some_and(|w| w >= FINISHED_MIN_MS) =>
            {
                (
                    NoticeKind::Finished,
                    format!("{name} finished"),
                    format!(
                        "{who} worked {} and is waiting for a new prompt",
                        minutes(worked.unwrap_or_default())
                    ),
                )
            }
            EventKind::SessionEnded(end) if prefs.session_ended => (
                NoticeKind::SessionEnded,
                format!("{name} session ended"),
                details(end.reason.as_deref().unwrap_or_default()),
            ),
            _ => return None,
        };
        self.admit(
            Notice {
                kind,
                title,
                body,
                agent_key: Some(agent.key.clone()),
                at: now,
            },
            &agent.session_key,
            now,
        )
    }

    /// Cooldown per session and kind, then the burst cap.
    fn admit(&mut self, mut notice: Notice, session: &str, now: i64) -> Option<Notice> {
        let key = (session.to_owned(), notice.kind);
        if self.last.get(&key).is_some_and(|t| now - t < COOLDOWN_MS) {
            return None;
        }
        self.last.insert(key, now);
        while self
            .recent
            .front()
            .is_some_and(|t| now - t >= BURST_WINDOW_MS)
        {
            self.recent.pop_front();
        }
        if self.recent.len() >= BURST_MAX {
            self.held_back += 1;
            return None;
        }
        self.recent.push_back(now);
        if self.held_back > 0 {
            let n = std::mem::take(&mut self.held_back);
            notice.body = format!(
                "{} (+{n} more alert{})",
                notice.body,
                if n == 1 { "" } else { "s" }
            );
        }
        if self.last.len() > 4096 {
            self.last.retain(|_, t| now - *t < COOLDOWN_MS);
        }
        Some(notice)
    }

    /// Forgets agents that are gone (keeps the maps small).
    pub fn forget(&mut self, agent_keys: &[String]) {
        for key in agent_keys {
            self.busy_since.remove(key);
        }
    }
}

/// The notice sent by Diagnostics → "Send a test notification".
pub fn test_notice(now: i64) -> Notice {
    Notice {
        kind: NoticeKind::Test,
        title: "Agent Office".into(),
        body: "Notifications work. Clicking one opens the agent it is about.".into(),
        agent_key: None,
        at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ao_core::event::{
        AgentError, AgentWaiting, CommandFinished, CommandStarted, PermissionRequested,
        SessionInfo, TextNote,
    };
    use ao_core::world::WorldState;

    const CLAUDE: ProviderLabel<'static> = ProviderLabel {
        name: "Claude Code",
        simulated: false,
    };

    /// Feeds events through a real world state, like the host does.
    struct Harness {
        world: WorldState,
        rules: NoticeRules,
        prefs: NotificationPrefs,
        provider: ProviderLabel<'static>,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                world: WorldState::new(),
                rules: NoticeRules::default(),
                prefs: NotificationPrefs::default(),
                provider: CLAUDE,
            }
        }

        fn feed(&mut self, session: &str, kind: EventKind, at: i64) -> Option<Notice> {
            let event =
                AgentEvent::for_session("claude", session, ao_core::event::EventSource::Hook, kind)
                    .at(at);
            let akey = ao_core::ids::agent_key(&event.provider, &event.session_id, &event.agent_id);
            let before = self.world.agent(&akey).map(|a| a.activity);
            self.world.apply(&event);
            let skey = ao_core::ids::session_key(&event.provider, &event.session_id);
            self.rules.observe(
                &event,
                before,
                self.world.agent(&akey),
                self.world.session(&skey),
                &self.provider,
                &self.prefs,
                at,
            )
        }
    }

    fn started(h: &mut Harness, s: &str, at: i64) {
        h.feed(
            s,
            EventKind::SessionStarted(SessionInfo {
                cwd: Some(format!("/work/{s}")),
                ..Default::default()
            }),
            at,
        );
    }

    fn permission(desc: &str) -> EventKind {
        EventKind::PermissionRequested(PermissionRequested {
            request_id: "p1".into(),
            tool_name: Some("Bash".into()),
            description: desc.into(),
            can_resolve: true,
            options: vec![],
        })
    }

    #[test]
    fn the_spec_examples() {
        let mut h = Harness::new();
        started(&mut h, "s1", 1_000);
        let n = h.feed("s1", permission("Run: npm install"), 2_000).unwrap();
        assert_eq!(n.kind, NoticeKind::Permission);
        assert_eq!(n.title, "Claude Code needs permission");
        assert!(n.body.ends_with("Run: npm install"), "{}", n.body);
        assert_eq!(n.agent_key.as_deref(), Some("claude:s1:s1"));

        let mut h = Harness::new();
        h.provider = ProviderLabel {
            name: "Codex CLI",
            simulated: false,
        };
        started(&mut h, "t", 1_000);
        h.feed(
            "t",
            EventKind::CommandStarted(CommandStarted {
                command_id: Some("c".into()),
                command: "cargo test".into(),
                cwd: None,
            }),
            2_000,
        );
        let n = h
            .feed(
                "t",
                EventKind::CommandFailed(CommandFinished {
                    command_id: Some("c".into()),
                    command: None,
                    exit_code: Some(101),
                    duration_ms: None,
                    error: None,
                }),
                3_000,
            )
            .unwrap();
        assert_eq!(n.title, "Codex CLI finished tests");
        assert!(
            n.body.ends_with("Tests failed (exit code 101)"),
            "{}",
            n.body
        );

        let mut h = Harness::new();
        h.provider = ProviderLabel {
            name: "Cursor CLI",
            simulated: false,
        };
        started(&mut h, "c", 1_000);
        let n = h
            .feed(
                "c",
                EventKind::AgentError(AgentError {
                    message: "Cursor exited with code 1".into(),
                    error_type: None,
                    recoverable: false,
                }),
                2_000,
            )
            .unwrap();
        assert_eq!(n.title, "Cursor CLI encountered an error");
    }

    #[test]
    fn quiet_by_default_where_it_should_be() {
        let mut h = Harness::new();
        started(&mut h, "s", 1_000);
        // Recoverable errors (retries) and ordinary commands: nothing.
        assert!(h
            .feed(
                "s",
                EventKind::AgentError(AgentError {
                    message: "retrying".into(),
                    error_type: None,
                    recoverable: true
                }),
                2_000
            )
            .is_none());
        h.feed(
            "s",
            EventKind::CommandStarted(CommandStarted {
                command_id: None,
                command: "ls".into(),
                cwd: None,
            }),
            3_000,
        );
        assert!(h
            .feed(
                "s",
                EventKind::CommandCompleted(CommandFinished {
                    command_id: None,
                    command: None,
                    exit_code: Some(0),
                    duration_ms: None,
                    error: None
                }),
                4_000
            )
            .is_none());
        // A short turn finishing: nothing. A long one: "finished".
        h.feed("s", EventKind::AgentIdle(TextNote::default()), 5_000);
        h.feed("s", EventKind::AgentThinking(TextNote::default()), 10_000);
        assert!(h
            .feed("s", EventKind::AgentIdle(TextNote::default()), 20_000)
            .is_none());
        h.feed("s", EventKind::AgentThinking(TextNote::default()), 30_000);
        let n = h
            .feed(
                "s",
                EventKind::AgentIdle(TextNote::default()),
                30_000 + 125_000,
            )
            .unwrap();
        assert_eq!(n.kind, NoticeKind::Finished);
        assert!(n.body.contains("worked 2 minutes"), "{}", n.body);
        // Session end is off by default.
        assert!(h
            .feed("s", EventKind::SessionEnded(Default::default()), 200_000)
            .is_none());
    }

    #[test]
    fn demo_sessions_old_events_and_preferences_are_respected() {
        let mut h = Harness::new();
        h.provider = ProviderLabel {
            name: "Demo",
            simulated: true,
        };
        started(&mut h, "d", 1_000);
        assert!(h.feed("d", permission("x"), 2_000).is_none());

        let mut h = Harness::new();
        started(&mut h, "s", 1_000);
        // A backlog event (two minutes old) does not notify.
        let event = AgentEvent::for_session(
            "claude",
            "s",
            ao_core::event::EventSource::Hook,
            permission("old"),
        )
        .at(1_000);
        let akey = "claude:s:s".to_string();
        h.world.apply(&event);
        assert!(h
            .rules
            .observe(
                &event,
                None,
                h.world.agent(&akey),
                h.world.session("claude:s"),
                &CLAUDE,
                &h.prefs,
                1_000 + STALE_MS + 1
            )
            .is_none());

        let mut h = Harness::new();
        h.prefs.show_details = false;
        started(&mut h, "s", 1_000);
        let n = h.feed("s", permission("Run: cat .env"), 2_000).unwrap();
        assert!(!n.body.contains(".env"), "details hidden: {}", n.body);
        h.prefs.permission = false;
        assert!(h.feed("s", permission("again"), 60_000).is_none());
        h.prefs.permission = true;
        h.prefs.enabled = false;
        assert!(h.feed("s", permission("again"), 90_000).is_none());
    }

    #[test]
    fn cooldowns_and_bursts() {
        let mut h = Harness::new();
        started(&mut h, "s", 1_000);
        assert!(h.feed("s", permission("a"), 2_000).is_some());
        // Same session, same kind, within the cooldown: held.
        assert!(h.feed("s", permission("b"), 3_000).is_none());
        assert!(h.feed("s", permission("c"), 2_000 + COOLDOWN_MS).is_some());

        // A burst from many sessions: three pass, the rest are counted.
        let mut h = Harness::new();
        for i in 0..6 {
            started(&mut h, &format!("b{i}"), 1_000);
        }
        let shown: Vec<_> = (0..6)
            .filter_map(|i| h.feed(&format!("b{i}"), permission("x"), 5_000 + i))
            .collect();
        assert_eq!(shown.len(), BURST_MAX);
        let next = h
            .feed(
                "b0",
                EventKind::AgentWaiting(AgentWaiting {
                    reason: WaitingReason::Input,
                    message: Some("Which database?".into()),
                }),
                5_000 + BURST_WINDOW_MS,
            )
            .unwrap();
        assert!(next.body.ends_with("(+3 more alerts)"), "{}", next.body);
    }

    #[test]
    fn long_text_is_clipped_to_one_line() {
        let text = format!("line one\nline two {}", "x".repeat(400));
        let out = clip(&text);
        assert!(!out.contains('\n'));
        assert_eq!(out.chars().count(), BODY_MAX_CHARS);
        assert!(out.ends_with('…'));
    }
}
