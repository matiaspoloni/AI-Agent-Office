//! Deterministic recording of the demo office on a virtual clock. The browser
//! preview (`npm run dev:web`) replays these UI batches, so the UI can be
//! developed without the desktop shell and without duplicating the reducer.

use crate::runner::{Runner, SessionControl, VirtualClock};
use crate::script::{SessionPlan, PRESETS};
use ao_core::batch::{Batcher, UiBatch};
use ao_core::event::AgentEvent;
use ao_core::ids::{EventId, SessionId};
use ao_core::world::WorldState;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Fixed epoch used by recordings; the preview shifts it to "now".
pub const RECORDING_BASE_MS: i64 = 1_700_000_000_000;
const FLUSH_EVERY_MS: i64 = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFrame {
    /// Milliseconds since the start of the recording.
    pub at: i64,
    pub batch: UiBatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRecording {
    pub base_ms: i64,
    pub duration_ms: i64,
    pub frames: Vec<PreviewFrame>,
}

/// Collects every event of every preset session up to `duration_ms`.
pub fn record_events(duration_ms: i64) -> Vec<AgentEvent> {
    let deadline = RECORDING_BASE_MS + duration_ms;
    let collected: Arc<Mutex<Vec<AgentEvent>>> = Arc::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build recorder runtime");

    for (index, preset) in PRESETS.iter().enumerate() {
        let clock = Arc::new(VirtualClock::starting_at(
            RECORDING_BASE_MS + index as i64 * 1_700,
        ));
        let control = Arc::new(SessionControl::default());
        let sink = collected.clone();
        let stop = control.clone();
        let emit: crate::runner::Emit = Arc::new(move |event: AgentEvent| {
            if event.timestamp > deadline {
                stop.stop();
                return;
            }
            sink.lock().expect("recorder lock").push(event);
        });
        let session = SessionId(format!("demo-rec-{}", preset.key));
        let runner = Runner::new(clock, emit, session, control, true);
        let plan = SessionPlan::from(preset);
        runtime.block_on(runner.run_plan(&plan));
    }

    let mut events = Arc::try_unwrap(collected)
        .map(|m| m.into_inner().expect("recorder lock"))
        .unwrap_or_default();
    events.sort_by_key(|e| e.timestamp);
    for (n, event) in events.iter_mut().enumerate() {
        event.event_id = EventId(format!("rec-{n:05}"));
    }
    events
}

/// Folds recorded events into UI batches, one per 100 ms of virtual time.
pub fn record_office(duration_ms: i64) -> PreviewRecording {
    let events = record_events(duration_ms);
    let mut world = WorldState::new();
    let mut batcher = Batcher::new();
    let mut frames = Vec::new();
    let mut window_end = RECORDING_BASE_MS + FLUSH_EVERY_MS;

    for event in &events {
        while event.timestamp >= window_end {
            if let Some(batch) = batcher.flush(&world) {
                frames.push(PreviewFrame {
                    at: window_end - RECORDING_BASE_MS,
                    batch,
                });
            }
            window_end += FLUSH_EVERY_MS;
        }
        let touched = world.apply(event);
        batcher.note(event, touched);
    }
    if let Some(batch) = batcher.flush(&world) {
        frames.push(PreviewFrame {
            at: window_end - RECORDING_BASE_MS,
            batch,
        });
    }
    PreviewRecording {
        base_ms: RECORDING_BASE_MS,
        duration_ms,
        frames,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ao_core::event::EventKind;
    use ao_core::Activity;

    #[test]
    fn recording_covers_every_preset_and_is_ordered() {
        let events = record_events(60_000);
        assert!(events.windows(2).all(|w| w[0].timestamp <= w[1].timestamp));
        for preset in PRESETS {
            let id = format!("demo-rec-{}", preset.key);
            assert!(
                events.iter().any(|e| e.session_id.0 == id && matches!(e.kind, EventKind::SessionStarted(_))),
                "{id} started"
            );
        }
        assert!(events
            .iter()
            .any(|e| matches!(e.kind, EventKind::SubagentStarted(_))));
        assert!(events
            .iter()
            .any(|e| matches!(e.kind, EventKind::PermissionRequested(_))));
        assert!(events
            .iter()
            .all(|e| e.timestamp <= RECORDING_BASE_MS + 60_000));
    }

    #[test]
    fn frames_rebuild_a_consistent_world() {
        let recording = record_office(45_000);
        assert!(!recording.frames.is_empty());
        let mut world = WorldState::new();
        for e in record_events(45_000) {
            world.apply(&e);
        }
        // Every activity used by the office appears somewhere in the recording.
        let seen: std::collections::HashSet<Activity> = recording
            .frames
            .iter()
            .flat_map(|f| f.batch.agents.iter().map(|a| a.activity))
            .collect();
        for activity in [
            Activity::Thinking,
            Activity::Reading,
            Activity::Coding,
            Activity::Testing,
            Activity::Idle,
        ] {
            assert!(seen.contains(&activity), "{activity:?} missing");
        }
        assert_eq!(world.sessions().count(), PRESETS.len());
    }
}
