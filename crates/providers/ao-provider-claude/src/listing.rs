//! External session discovery through the official `claude agents --json`
//! (active interactive and background sessions). It lets sessions appear even
//! before any hook fires, and ends them when Claude stops listing them.

use crate::PROVIDER_ID;
use ao_core::event::*;
use ao_core::ids::SessionId;
use ao_core::provider::ExternalSessionInfo;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedSession {
    pub session_id: String,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// `interactive` or `background`.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub started_at: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    /// e.g. `busy`, `idle`.
    #[serde(default)]
    pub status: Option<String>,
}

impl ListedSession {
    pub fn is_background(&self) -> bool {
        self.kind.as_deref() == Some("background")
    }

    pub fn to_info(&self) -> ExternalSessionInfo {
        ExternalSessionInfo {
            session_id: SessionId::new(&self.session_id),
            cwd: self.cwd.clone(),
            pid: self.pid,
            title: self.name.clone(),
            status: self.status.clone(),
            started_at: self.started_at,
        }
    }
}

/// Parses the JSON array printed by `claude agents --json`, skipping entries
/// without a session id (unknown future shapes are tolerated).
pub fn parse_agents_json(text: &str) -> Result<Vec<ListedSession>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text.trim()).map_err(|e| format!("not JSON: {e}"))?;
    let array = value.as_array().ok_or("expected a JSON array")?;
    Ok(array
        .iter()
        .filter_map(|item| serde_json::from_value::<ListedSession>(item.clone()).ok())
        .filter(|s| !s.session_id.is_empty())
        .collect())
}

/// Turns successive listings into events.
#[derive(Debug, Default)]
pub struct Discovery {
    listed: HashMap<String, ListedSession>,
}

impl Discovery {
    pub fn is_background(&self, session_id: &str) -> bool {
        self.listed
            .get(session_id)
            .is_some_and(ListedSession::is_background)
    }

    /// `hooked`: sessions that already sent hook events (their hooks describe
    /// activity in far more detail, so the coarse listing status is skipped).
    /// `managed`: sessions launched by Agent Office (never reported here).
    pub fn reconcile(
        &mut self,
        current: Vec<ListedSession>,
        hooked: &HashSet<String>,
        managed: &HashSet<String>,
        now_ms: i64,
    ) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        let ev = |id: &str, kind| {
            AgentEvent::for_session(PROVIDER_ID, SessionId::new(id), EventSource::Process, kind)
                .at(now_ms)
        };
        let mut next = HashMap::new();
        for session in current {
            if managed.contains(&session.session_id) {
                continue;
            }
            let previous = self.listed.get(&session.session_id);
            if previous.is_none() {
                events.push(ev(
                    &session.session_id,
                    EventKind::SessionStarted(SessionInfo {
                        mode: Some(SessionMode::External),
                        cwd: session.cwd.clone(),
                        title: session.name.clone(),
                        reason: Some("discovered by `claude agents`".into()),
                        pid: session.pid,
                        ..Default::default()
                    }),
                ));
            }
            let status_changed = previous.map(|p| &p.status) != Some(&session.status);
            if status_changed && !hooked.contains(&session.session_id) {
                match session.status.as_deref() {
                    Some("busy") => events.push(ev(
                        &session.session_id,
                        EventKind::AgentThinking(TextNote {
                            text: Some("Working (status from `claude agents`)".into()),
                        }),
                    )),
                    Some("idle") => events.push(ev(
                        &session.session_id,
                        EventKind::AgentIdle(TextNote {
                            text: Some("Idle (status from `claude agents`)".into()),
                        }),
                    )),
                    _ => {}
                }
            }
            next.insert(session.session_id.clone(), session);
        }
        for gone in self.listed.keys().filter(|id| !next.contains_key(*id)) {
            events.push(ev(
                gone,
                EventKind::SessionEnded(SessionEnded {
                    reason: Some("no longer listed by `claude agents`".into()),
                    exit_code: None,
                }),
            ));
        }
        self.listed = next;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Output recorded from Claude Code 2.1.281.
    const REAL: &str = r#"[
  {
    "pid": 158,
    "cwd": "/home/user/AI-Agent-Office",
    "kind": "interactive",
    "startedAt": 1790236673845,
    "sessionId": "57c14911-25c4-5926-a957-2fe829435439",
    "name": "ai-agent-office-14",
    "status": "busy"
  }
]"#;

    #[test]
    fn parses_real_output_and_tolerates_unknown_entries() {
        let sessions = parse_agents_json(REAL).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].pid, Some(158));
        assert_eq!(sessions[0].status.as_deref(), Some("busy"));
        assert!(!sessions[0].is_background());
        assert!(
            parse_agents_json(r#"[{"foo": 1}, {"sessionId": "x", "future": true}]"#)
                .unwrap()
                .len()
                == 1
        );
        assert!(parse_agents_json("not json").is_err());
    }

    #[test]
    fn reconcile_emits_start_status_and_end() {
        let mut d = Discovery::default();
        let none = HashSet::new();
        let first = d.reconcile(parse_agents_json(REAL).unwrap(), &none, &none, 1);
        assert_eq!(first.len(), 2);
        assert!(matches!(first[0].kind, EventKind::SessionStarted(_)));
        assert!(matches!(first[1].kind, EventKind::AgentThinking(_)));

        // Same listing again: nothing new.
        assert!(d
            .reconcile(parse_agents_json(REAL).unwrap(), &none, &none, 2)
            .is_empty());

        // Hooked sessions don't get coarse status updates.
        let hooked: HashSet<String> = ["57c14911-25c4-5926-a957-2fe829435439".to_string()].into();
        let idle = REAL.replace("busy", "idle");
        assert!(d
            .reconcile(parse_agents_json(&idle).unwrap(), &hooked, &none, 3)
            .is_empty());

        // Disappeared → ended.
        let gone = d.reconcile(Vec::new(), &none, &none, 4);
        assert!(matches!(gone[0].kind, EventKind::SessionEnded(_)));
    }

    #[test]
    fn managed_sessions_are_skipped() {
        let mut d = Discovery::default();
        let managed: HashSet<String> = ["57c14911-25c4-5926-a957-2fe829435439".to_string()].into();
        assert!(d
            .reconcile(
                parse_agents_json(REAL).unwrap(),
                &HashSet::new(),
                &managed,
                1
            )
            .is_empty());
    }
}
