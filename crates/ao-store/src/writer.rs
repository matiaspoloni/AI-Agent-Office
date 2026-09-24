//! Background writer: one thread owns a write connection and commits queued
//! events/sessions in batches, so the event pipeline never waits on disk I/O.

use crate::{Store, StoreError};
use ao_core::event::AgentEvent;
use ao_core::world::{AgentState, SessionState};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const MAX_PENDING_EVENTS: usize = 2_000;

pub enum WriteOp {
    Events(Vec<AgentEvent>),
    State {
        sessions: Vec<SessionState>,
        agents: Vec<AgentState>,
    },
    PurgeEventsBefore(i64),
    /// Forces a commit and acknowledges it.
    Flush(Sender<()>),
    /// Commits what is pending and stops the thread.
    Shutdown,
}

#[derive(Default)]
struct Pending {
    events: Vec<AgentEvent>,
    sessions: BTreeMap<String, SessionState>,
    agents: BTreeMap<String, AgentState>,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.events.is_empty() && self.sessions.is_empty() && self.agents.is_empty()
    }
}

pub struct StoreWriter {
    tx: Sender<WriteOp>,
    handle: Option<JoinHandle<()>>,
}

impl StoreWriter {
    /// Spawns the writer thread with its own connection to `path`.
    pub fn spawn(path: PathBuf) -> Result<Self, StoreError> {
        let store = Store::open(&path)?;
        Ok(Self::spawn_with(store))
    }

    pub fn spawn_with(store: Store) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("ao-store-writer".into())
            .spawn(move || run(store, rx))
            .expect("spawn store writer thread");
        Self {
            tx,
            handle: Some(handle),
        }
    }

    pub fn sender(&self) -> Sender<WriteOp> {
        self.tx.clone()
    }

    pub fn send(&self, op: WriteOp) {
        if self.tx.send(op).is_err() {
            tracing::error!("store writer thread is gone; dropping write");
        }
    }

    /// Blocks until everything queued so far is committed.
    pub fn flush(&self) {
        let (ack_tx, ack_rx) = mpsc::channel();
        self.send(WriteOp::Flush(ack_tx));
        let _ = ack_rx.recv_timeout(Duration::from_secs(10));
    }
}

impl Drop for StoreWriter {
    fn drop(&mut self) {
        let _ = self.tx.send(WriteOp::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn commit(store: &mut Store, pending: &mut Pending) {
    if pending.is_empty() {
        return;
    }
    let events = std::mem::take(&mut pending.events);
    let sessions: Vec<_> = std::mem::take(&mut pending.sessions)
        .into_values()
        .collect();
    let agents: Vec<_> = std::mem::take(&mut pending.agents).into_values().collect();
    if let Err(err) = store.write_batch(&events, &sessions, &agents) {
        tracing::error!(%err, events = events.len(), "failed to persist batch");
    }
}

fn run(mut store: Store, rx: Receiver<WriteOp>) {
    let mut pending = Pending::default();
    let mut last_commit = Instant::now();
    loop {
        let timeout = FLUSH_INTERVAL.saturating_sub(last_commit.elapsed());
        match rx.recv_timeout(timeout) {
            Ok(WriteOp::Events(events)) => pending.events.extend(events),
            Ok(WriteOp::State { sessions, agents }) => {
                for s in sessions {
                    pending.sessions.insert(s.key.clone(), s);
                }
                for a in agents {
                    pending.agents.insert(a.key.clone(), a);
                }
            }
            Ok(WriteOp::PurgeEventsBefore(ts)) => {
                commit(&mut store, &mut pending);
                match store.purge_events_before(ts) {
                    Ok(n) if n > 0 => tracing::info!(removed = n, "retention purge"),
                    Ok(_) => {}
                    Err(err) => tracing::error!(%err, "retention purge failed"),
                }
            }
            Ok(WriteOp::Flush(ack)) => {
                commit(&mut store, &mut pending);
                last_commit = Instant::now();
                let _ = ack.send(());
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Ok(WriteOp::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                commit(&mut store, &mut pending);
                return;
            }
        }
        if pending.events.len() >= MAX_PENDING_EVENTS || last_commit.elapsed() >= FLUSH_INTERVAL {
            commit(&mut store, &mut pending);
            last_commit = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ao_core::event::*;
    use ao_core::world::WorldState;

    #[test]
    fn batches_writes_and_flushes_on_demand() {
        let dir = std::env::temp_dir().join(format!("ao-writer-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("db.sqlite");
        let writer = StoreWriter::spawn(path.clone()).unwrap();

        let mut world = WorldState::new();
        let mut events = Vec::new();
        for i in 0..500 {
            let e = AgentEvent::for_session(
                "demo",
                "s1",
                EventSource::Simulation,
                EventKind::AgentThinking(TextNote {
                    text: Some(format!("step {i}")),
                }),
            );
            world.apply(&e);
            events.push(e);
        }
        let snap = world.snapshot();
        writer.send(WriteOp::State {
            sessions: snap.sessions,
            agents: snap.agents,
        });
        writer.send(WriteOp::Events(events));
        writer.flush();

        let reader = Store::open(&path).unwrap();
        assert_eq!(reader.stats().unwrap().events, 500);
        assert_eq!(reader.stats().unwrap().sessions, 1);
        drop(writer);
        let _ = std::fs::remove_dir_all(dir);
    }
}
