//! Coalesces state changes into periodic UI batches, so a burst of events
//! becomes one UI update instead of hundreds.

use crate::event::AgentEvent;
use crate::world::{AgentState, SessionState, Touched, WorldState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;

/// Maximum number of raw events forwarded to the UI per batch (the log view).
/// State objects are always complete; only the event log is capped.
pub const MAX_EVENTS_PER_BATCH: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UiBatch {
    #[ts(type = "number")]
    pub seq: u64,
    pub sessions: Vec<SessionState>,
    pub agents: Vec<AgentState>,
    pub removed_sessions: Vec<String>,
    pub removed_agents: Vec<String>,
    pub events: Vec<AgentEvent>,
    /// Events that happened but were not forwarded because of the cap.
    pub skipped_events: u32,
}

impl UiBatch {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
            && self.agents.is_empty()
            && self.removed_sessions.is_empty()
            && self.removed_agents.is_empty()
            && self.events.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct Batcher {
    seq: u64,
    dirty: Touched,
    removed_sessions: BTreeSet<String>,
    removed_agents: BTreeSet<String>,
    events: Vec<AgentEvent>,
    skipped: u32,
}

impl Batcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn note(&mut self, event: &AgentEvent, touched: Touched) {
        self.dirty.merge(touched);
        if self.events.len() < MAX_EVENTS_PER_BATCH {
            self.events.push(event.clone());
        } else {
            self.skipped += 1;
        }
    }

    /// Changes that did not come from an event (e.g. a session going silent).
    pub fn note_touched(&mut self, touched: Touched) {
        self.dirty.merge(touched);
    }

    pub fn note_removed(&mut self, sessions: Vec<String>, agents: Vec<String>) {
        for s in sessions {
            self.dirty.sessions.remove(&s);
            self.removed_sessions.insert(s);
        }
        for a in agents {
            self.dirty.agents.remove(&a);
            self.removed_agents.insert(a);
        }
    }

    /// Builds the batch from the current world state and resets. Returns
    /// `None` when nothing changed.
    pub fn flush(&mut self, world: &WorldState) -> Option<UiBatch> {
        if self.dirty.is_empty()
            && self.events.is_empty()
            && self.removed_sessions.is_empty()
            && self.removed_agents.is_empty()
        {
            return None;
        }
        self.seq += 1;
        let dirty = std::mem::take(&mut self.dirty);
        let batch = UiBatch {
            seq: self.seq,
            sessions: dirty
                .sessions
                .iter()
                .filter_map(|k| world.session(k).cloned())
                .collect(),
            agents: dirty
                .agents
                .iter()
                .filter_map(|k| world.agent(k).cloned())
                .collect(),
            removed_sessions: std::mem::take(&mut self.removed_sessions)
                .into_iter()
                .collect(),
            removed_agents: std::mem::take(&mut self.removed_agents)
                .into_iter()
                .collect(),
            events: std::mem::take(&mut self.events),
            skipped_events: std::mem::take(&mut self.skipped),
        };
        Some(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::*;

    #[test]
    fn coalesces_many_events_into_one_batch() {
        let mut world = WorldState::new();
        let mut batcher = Batcher::new();
        for i in 0..1000 {
            let e = AgentEvent::for_session(
                "demo",
                "s1",
                EventSource::Simulation,
                EventKind::CommandOutput(CommandOutput {
                    command_id: None,
                    stream: OutputStream::Stdout,
                    chunk: format!("line {i}"),
                }),
            );
            let touched = world.apply(&e);
            batcher.note(&e, touched);
        }
        let batch = batcher.flush(&world).unwrap();
        assert_eq!(batch.seq, 1);
        assert_eq!(batch.sessions.len(), 1);
        assert_eq!(batch.agents.len(), 1);
        assert_eq!(batch.events.len(), MAX_EVENTS_PER_BATCH);
        assert_eq!(batch.skipped_events as usize, 1000 - MAX_EVENTS_PER_BATCH);
        assert!(batcher.flush(&world).is_none());
    }
}
