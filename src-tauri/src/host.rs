//! The Agent Office runtime: wires providers → pipeline → state → storage/UI.
//! Independent of Tauri so it can run headless (smoke test) and in tests.

use crate::diagnostics::{self, DiagnosticsReport, ProviderErrorRecord};
use crate::paths::AppPaths;
use crate::prefs::Preferences;
use ao_core::batch::{Batcher, UiBatch};
use ao_core::event::{AgentEvent, EventKind, EventSource, SessionEnded, SessionMode};
use ao_core::ids::{PermissionRequestId, ProjectId, ProviderId, SessionId};
use ao_core::pipeline::{Pipeline, ProjectRoot};
use ao_core::provider::{
    AdapterContext, EventSink, LaunchRequest, PermissionDecision, SessionHandle, StopMode,
};
use ao_core::registry::{ProviderInfo, ProviderRegistry};
use ao_core::sanitize::SanitizeLimits;
use ao_core::time::now_ms;
use ao_core::world::{SessionStatus, WorldSnapshot, WorldState};
use ao_provider_demo::DemoAdapter;
use ao_store::writer::{StoreWriter, WriteOp};
use ao_store::{NewProject, Project, Store};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const UI_FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);
const KEEP_ENDED_IN_MEMORY_MS: i64 = 15 * 60 * 1000;
const RESTORE_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
const MAX_PROVIDER_ERRORS: usize = 100;

pub type UiSink = Box<dyn Fn(UiBatch) + Send + Sync>;

struct Inner {
    pipeline: Pipeline,
    world: WorldState,
    batcher: Batcher,
    persist: Vec<AgentEvent>,
}

pub struct Host {
    pub paths: AppPaths,
    pub registry: ProviderRegistry,
    pub demo: Arc<DemoAdapter>,
    sink: EventSink,
    inner: Mutex<Inner>,
    store: Mutex<Store>,
    store_error: Option<String>,
    writer: StoreWriter,
    ui: Mutex<Option<UiSink>>,
    prefs: Mutex<Preferences>,
    provider_errors: Mutex<VecDeque<ProviderErrorRecord>>,
    pub started_at: i64,
    ingested: AtomicU64,
}

fn open_store(paths: &AppPaths) -> (Store, Store, Option<String>) {
    match (Store::open(&paths.db_path), Store::open(&paths.db_path)) {
        (Ok(reader), Ok(writer)) => (reader, writer, None),
        (Err(err), _) | (_, Err(err)) => {
            tracing::error!(%err, path = %paths.db_path.display(), "database unavailable; using in-memory storage");
            let reader = Store::open_in_memory().expect("in-memory sqlite");
            let writer = Store::open_in_memory().expect("in-memory sqlite");
            (
                reader,
                writer,
                Some(format!("{err} — running with temporary in-memory storage")),
            )
        }
    }
}

impl Host {
    /// Builds the host and starts its background tasks. Must be called from
    /// inside a Tokio runtime.
    pub fn start(paths: AppPaths) -> Arc<Host> {
        let (sink, mut rx) = EventSink::new(65_536);
        let demo = Arc::new(DemoAdapter::new());
        let registry = crate::providers::build_registry(demo.clone());
        let (reader, writer_store, store_error) = open_store(&paths);
        let prefs = Preferences::load(&reader);

        let mut pipeline = Pipeline::new(prefs.sanitize_limits());
        pipeline.set_projects(project_roots(&reader));

        let mut world = WorldState::new();
        if let Ok(snapshot) = reader.load_world_since(now_ms() - RESTORE_WINDOW_MS) {
            world.restore(snapshot);
        }

        let host = Arc::new(Host {
            paths,
            registry,
            demo,
            sink,
            inner: Mutex::new(Inner {
                pipeline,
                world,
                batcher: Batcher::new(),
                persist: Vec::new(),
            }),
            store: Mutex::new(reader),
            store_error,
            writer: StoreWriter::spawn_with(writer_store),
            ui: Mutex::new(None),
            prefs: Mutex::new(prefs),
            provider_errors: Mutex::new(VecDeque::new()),
            started_at: now_ms(),
            ingested: AtomicU64::new(0),
        });

        host.close_orphaned_managed_sessions();
        host.purge_old_events();

        let ingest_host = host.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                ingest_host.ingest(event);
            }
        });

        let tick_host = Arc::downgrade(&host);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(UI_FLUSH_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let Some(host) = tick_host.upgrade() else {
                    break;
                };
                host.flush();
            }
        });

        let housekeeping_host = Arc::downgrade(&host);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(PRUNE_INTERVAL);
            let mut ticks = 0u64;
            loop {
                interval.tick().await;
                let Some(host) = housekeeping_host.upgrade() else {
                    break;
                };
                host.prune_memory();
                ticks += 1;
                if ticks % 360 == 0 {
                    host.purge_old_events();
                }
            }
        });

        tracing::info!(data_dir = %host.paths.data_dir.display(), providers = host.registry.len(), "host started");
        host
    }

    pub fn adapter_context(&self) -> AdapterContext {
        AdapterContext {
            sink: self.sink.clone(),
        }
    }

    /// Managed sessions cannot survive an app restart (their process belonged to
    /// the previous run), so they are closed explicitly instead of lingering.
    fn close_orphaned_managed_sessions(&self) {
        let orphaned: Vec<_> = {
            let inner = self.inner.lock().expect("host lock");
            inner
                .world
                .sessions()
                .filter(|s| s.status == SessionStatus::Active && s.mode == SessionMode::Managed)
                .map(|s| (s.provider.clone(), s.session_id.clone()))
                .collect()
        };
        for (provider, session) in orphaned {
            self.sink.emit(AgentEvent::for_session(
                provider,
                session,
                EventSource::Internal,
                EventKind::SessionEnded(SessionEnded {
                    reason: Some("Agent Office restarted".into()),
                    exit_code: None,
                }),
            ));
        }
    }

    fn ingest(&self, event: AgentEvent) {
        self.ingested.fetch_add(1, Ordering::Relaxed);
        if let EventKind::ProviderError(err) = &event.kind {
            let mut errors = self.provider_errors.lock().expect("errors lock");
            errors.push_back(ProviderErrorRecord {
                at: event.timestamp,
                provider: event.provider.0.clone(),
                component: err.component.clone(),
                message: err.message.clone(),
            });
            while errors.len() > MAX_PROVIDER_ERRORS {
                errors.pop_front();
            }
        }
        let mut inner = self.inner.lock().expect("host lock");
        let Some(event) = inner.pipeline.process(event) else {
            return;
        };
        let touched = inner.world.apply(&event);
        inner.batcher.note(&event, touched);
        inner.persist.push(event);
    }

    /// Sends pending changes to the UI and to the database writer.
    pub fn flush(&self) {
        let (batch, events) = {
            let mut inner = self.inner.lock().expect("host lock");
            let Inner {
                world,
                batcher,
                persist,
                ..
            } = &mut *inner;
            (batcher.flush(world), std::mem::take(persist))
        };
        if !events.is_empty() {
            self.writer.send(WriteOp::Events(events));
        }
        if let Some(batch) = batch {
            if !batch.sessions.is_empty() || !batch.agents.is_empty() {
                self.writer.send(WriteOp::State {
                    sessions: batch.sessions.clone(),
                    agents: batch.agents.clone(),
                });
            }
            if let Some(ui) = self.ui.lock().expect("ui lock").as_ref() {
                ui(batch);
            }
        }
    }

    fn prune_memory(&self) {
        let mut inner = self.inner.lock().expect("host lock");
        let (sessions, agents) = inner.world.prune_ended(now_ms() - KEEP_ENDED_IN_MEMORY_MS);
        if !sessions.is_empty() || !agents.is_empty() {
            inner.batcher.note_removed(sessions, agents);
        }
    }

    fn purge_old_events(&self) {
        let days = self.prefs.lock().expect("prefs lock").retention_days.max(1) as i64;
        self.writer.send(WriteOp::PurgeEventsBefore(
            now_ms() - days * 24 * 60 * 60 * 1000,
        ));
    }

    pub fn set_ui_sink(&self, sink: Option<UiSink>) {
        *self.ui.lock().expect("ui lock") = sink;
    }

    pub fn ui_subscribed(&self) -> bool {
        self.ui.lock().expect("ui lock").is_some()
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        self.inner.lock().expect("host lock").world.snapshot()
    }

    pub fn providers(&self) -> Vec<ProviderInfo> {
        self.registry.infos()
    }

    pub fn ingested(&self) -> u64 {
        self.ingested.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.sink.dropped()
    }

    pub fn duplicates(&self) -> u64 {
        self.inner.lock().expect("host lock").pipeline.duplicates()
    }

    pub fn store_error(&self) -> Option<String> {
        self.store_error.clone()
    }

    pub fn with_store<T>(&self, f: impl FnOnce(&Store) -> T) -> T {
        f(&self.store.lock().expect("store lock"))
    }

    pub fn provider_errors(&self) -> Vec<ProviderErrorRecord> {
        self.provider_errors
            .lock()
            .expect("errors lock")
            .iter()
            .cloned()
            .collect()
    }

    pub fn active_sessions(&self) -> usize {
        self.inner
            .lock()
            .expect("host lock")
            .world
            .sessions()
            .filter(|s| s.status == SessionStatus::Active)
            .count()
    }

    /// Blocks until queued writes are committed (used on shutdown and in tests).
    pub fn flush_to_disk(&self) {
        self.flush();
        self.writer.flush();
    }

    // ------------------------------------------------------------------
    // Projects & preferences
    // ------------------------------------------------------------------

    fn refresh_projects(&self) {
        let roots = self.with_store(project_roots);
        self.inner
            .lock()
            .expect("host lock")
            .pipeline
            .set_projects(roots);
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, String> {
        self.with_store(|s| s.list_projects())
            .map_err(|e| e.to_string())
    }

    pub fn add_project(&self, project: NewProject) -> Result<Project, String> {
        let created = self
            .with_store(|s| s.add_project(project))
            .map_err(|e| e.to_string())?;
        self.refresh_projects();
        Ok(created)
    }

    pub fn update_project(&self, project: Project) -> Result<(), String> {
        self.with_store(|s| s.update_project(&project))
            .map_err(|e| e.to_string())?;
        self.refresh_projects();
        Ok(())
    }

    pub fn remove_project(&self, id: &str) -> Result<bool, String> {
        let removed = self
            .with_store(|s| s.remove_project(id))
            .map_err(|e| e.to_string())?;
        self.refresh_projects();
        Ok(removed)
    }

    pub fn preferences(&self) -> Preferences {
        self.prefs.lock().expect("prefs lock").clone()
    }

    pub fn set_preferences(&self, prefs: Preferences) -> Result<Preferences, String> {
        let prefs = prefs.normalized();
        self.with_store(|s| prefs.save(s))
            .map_err(|e| e.to_string())?;
        self.inner
            .lock()
            .expect("host lock")
            .pipeline
            .set_limits(prefs.sanitize_limits());
        *self.prefs.lock().expect("prefs lock") = prefs.clone();
        self.purge_old_events();
        Ok(prefs)
    }

    pub fn recent_events(
        &self,
        session_key: &str,
        limit: usize,
    ) -> Result<Vec<AgentEvent>, String> {
        self.flush_to_disk();
        self.with_store(|s| s.recent_events(session_key, limit.clamp(1, 2_000)))
            .map_err(|e| e.to_string())
    }

    // ------------------------------------------------------------------
    // Session actions (capability-gated)
    // ------------------------------------------------------------------

    fn adapter(&self, provider: &ProviderId) -> Result<Arc<dyn ao_core::ProviderAdapter>, String> {
        self.registry
            .get(provider)
            .ok_or_else(|| format!("Unknown provider `{provider}`"))
    }

    fn session_mode(&self, provider: &ProviderId, session: &SessionId) -> SessionMode {
        let key = ao_core::ids::session_key(provider, session);
        self.inner
            .lock()
            .expect("host lock")
            .world
            .session(&key)
            .map(|s| s.mode)
            .unwrap_or(SessionMode::External)
    }

    fn ensure_capability(
        &self,
        provider: &ProviderId,
        session: Option<&SessionId>,
        name: &str,
        pick: impl Fn(&ao_core::Capabilities) -> ao_core::Support,
    ) -> Result<Arc<dyn ao_core::ProviderAdapter>, String> {
        let adapter = self.adapter(provider)?;
        let descriptor = adapter.descriptor();
        let profile = adapter.capabilities();
        let mode = session
            .map(|s| self.session_mode(provider, s))
            .unwrap_or(SessionMode::Managed);
        let (caps, implemented) = match mode {
            SessionMode::Managed => (profile.managed, profile.implemented.managed_sessions),
            SessionMode::External => (profile.external, profile.implemented.external_sessions),
        };
        if !pick(&caps).is_available() {
            return Err(format!(
                "{} does not support `{name}` for {mode:?} sessions.",
                descriptor.display_name
            ));
        }
        if !implemented {
            return Err(format!(
                "{} {mode:?} sessions are not implemented yet in this build of Agent Office.",
                descriptor.display_name
            ));
        }
        Ok(adapter)
    }

    pub async fn launch(
        &self,
        provider: ProviderId,
        mut request: LaunchRequest,
    ) -> Result<SessionHandle, String> {
        let adapter = self.ensure_capability(&provider, None, "launch", |c| c.launch)?;
        if request.cwd.trim().is_empty() {
            if let Some(project_id) = &request.project_id {
                let project = self
                    .with_store(|s| s.get_project(&project_id.0))
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("Project {project_id} not found"))?;
                request.cwd = project.path;
            }
        }
        adapter
            .launch_session(request, self.adapter_context())
            .await
            .map_err(|e| e.to_string())
    }

    pub fn start_demo_office(&self) -> Vec<SessionHandle> {
        self.demo.launch_office(&self.adapter_context())
    }

    pub async fn stop(
        &self,
        provider: ProviderId,
        session: SessionId,
        mode: StopMode,
    ) -> Result<(), String> {
        let adapter = self.ensure_capability(&provider, Some(&session), "stop", |c| c.stop)?;
        adapter
            .stop_session(&session, mode)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn send_prompt(
        &self,
        provider: ProviderId,
        session: SessionId,
        prompt: String,
    ) -> Result<(), String> {
        if prompt.trim().is_empty() {
            return Err("Prompt is empty".into());
        }
        let adapter =
            self.ensure_capability(&provider, Some(&session), "sendPrompt", |c| c.send_prompt)?;
        adapter
            .send_prompt(&session, &prompt)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn resolve_permission(
        &self,
        provider: ProviderId,
        session: SessionId,
        request: PermissionRequestId,
        decision: PermissionDecision,
    ) -> Result<(), String> {
        let adapter =
            self.ensure_capability(&provider, Some(&session), "permissions", |c| c.permissions)?;
        adapter
            .resolve_permission(&session, &request, decision)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn diagnostics(self: &Arc<Self>) -> DiagnosticsReport {
        diagnostics::run(self.clone()).await
    }

    /// Current sanitize limits (exposed for diagnostics).
    pub fn sanitize_limits(&self) -> SanitizeLimits {
        self.prefs.lock().expect("prefs lock").sanitize_limits()
    }
}

fn project_roots(store: &Store) -> Vec<ProjectRoot> {
    store
        .list_projects()
        .unwrap_or_default()
        .into_iter()
        .map(|p| ProjectRoot {
            id: ProjectId(p.id),
            path: p.path,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ao_core::event::{SessionInfo, TextNote};

    fn temp_paths() -> AppPaths {
        AppPaths::at(std::env::temp_dir().join(format!("ao-host-test-{}", uuid_like())))
    }

    fn uuid_like() -> String {
        format!("{}-{}", std::process::id(), now_ms())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn events_flow_to_state_ui_and_disk() {
        let paths = temp_paths();
        let host = Host::start(paths.clone());
        let batches = Arc::new(Mutex::new(Vec::<UiBatch>::new()));
        let sink = batches.clone();
        host.set_ui_sink(Some(Box::new(move |b| sink.lock().unwrap().push(b))));

        let project = host
            .add_project(NewProject {
                name: "Nalu".into(),
                path: "/work/nalu".into(),
                ..Default::default()
            })
            .unwrap();

        let ctx = host.adapter_context();
        ctx.sink.emit(AgentEvent::for_session(
            "claude",
            "s1",
            EventSource::Hook,
            EventKind::SessionStarted(SessionInfo {
                cwd: Some("/work/nalu/app".into()),
                ..Default::default()
            }),
        ));
        ctx.sink.emit(AgentEvent::for_session(
            "claude",
            "s1",
            EventSource::Hook,
            EventKind::AgentThinking(TextNote::default()),
        ));

        tokio::time::sleep(Duration::from_millis(350)).await;
        host.flush_to_disk();

        let snapshot = host.snapshot();
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(
            snapshot.sessions[0]
                .project_id
                .as_ref()
                .map(|p| p.0.clone()),
            Some(project.id)
        );
        assert!(!batches.lock().unwrap().is_empty());
        let events = host.recent_events("claude:s1", 10).unwrap();
        assert_eq!(events.len(), 2);

        // Unimplemented real-provider actions are refused, not faked.
        let err = host
            .send_prompt(ProviderId::new("claude"), SessionId::new("s1"), "hi".into())
            .await
            .unwrap_err();
        assert!(
            err.contains("does not support") || err.contains("not implemented"),
            "{err}"
        );

        let _ = std::fs::remove_dir_all(paths.data_dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn restart_closes_orphaned_managed_sessions() {
        let paths = temp_paths();
        {
            let host = Host::start(paths.clone());
            host.adapter_context().sink.emit(AgentEvent::for_session(
                "demo",
                "d1",
                EventSource::Simulation,
                EventKind::SessionStarted(SessionInfo {
                    mode: Some(SessionMode::Managed),
                    ..Default::default()
                }),
            ));
            tokio::time::sleep(Duration::from_millis(300)).await;
            host.flush_to_disk();
        }
        let host = Host::start(paths.clone());
        tokio::time::sleep(Duration::from_millis(300)).await;
        let snapshot = host.snapshot();
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.session_id.0 == "d1")
            .unwrap();
        assert_eq!(session.status, SessionStatus::Ended);
        assert_eq!(
            session.end_reason.as_deref(),
            Some("Agent Office restarted")
        );
        let _ = std::fs::remove_dir_all(paths.data_dir);
    }
}
