//! The Agent Office runtime: wires providers → pipeline → state → storage/UI.
//! Independent of Tauri so it can run headless (smoke test) and in tests.

use crate::diagnostics::{self, DiagnosticsReport, ProviderErrorRecord};
use crate::git::{relative_to_tree, GitService, RepositoriesReport};
use crate::hooks::{HookBridge, HookBridgeStatus};
use crate::paths::AppPaths;
use crate::prefs::Preferences;
use ao_core::batch::{Batcher, UiBatch};
use ao_core::event::{AgentEvent, EventKind, EventSource, SessionEnded, SessionMode};
use ao_core::ids::{PermissionRequestId, ProjectId, ProviderId, SessionId};
use ao_core::pipeline::{Ingest, Pipeline, ProjectRoot};
use ao_core::provider::{
    AdapterContext, EventSink, ExternalSessionInfo, HookCall, IntegrationState, IntegrationStatus,
    LaunchRequest, PermissionDecision, RelayCommand, SessionHandle, StopMode,
};
use ao_core::registry::{ProviderInfo, ProviderRegistry};
use ao_core::sanitize::SanitizeLimits;
use ao_core::time::now_ms;
use ao_core::world::{SessionStatus, WorldSnapshot, WorldState};
use ao_ipc::{HookOrigin, HookRequest, HookResponse};
use ao_provider_claude::ClaudeOptions;
use ao_provider_codex::CodexOptions;
use ao_provider_cursor::CursorOptions;
use ao_provider_demo::DemoAdapter;
use ao_store::writer::{StoreWriter, WriteOp};
use ao_store::{NewProject, Project, Store};
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ts_rs::TS;

const UI_FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);
const SILENCE_CHECK_INTERVAL: Duration = Duration::from_secs(15);
/// How long Restart waits for a running session to stop.
const RESTART_STOP_WAIT: Duration = Duration::from_secs(20);
const KEEP_ENDED_IN_MEMORY_MS: i64 = 15 * 60 * 1000;
const RESTORE_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
const MAX_PROVIDER_ERRORS: usize = 100;

pub type UiSink = Box<dyn Fn(UiBatch) + Send + Sync>;

/// How the host is wired to the outside world.
#[derive(Debug, Clone, Default)]
pub struct HostOptions {
    /// Command agent CLIs run to reach Agent Office (`None`: hooks disabled).
    pub relay: Option<RelayCommand>,
    pub claude: ClaudeOptions,
    pub codex: CodexOptions,
    pub cursor: CursorOptions,
    /// The `git` to use (`None`: found on PATH or in Git's usual folders).
    pub git_executable: Option<std::path::PathBuf>,
}

impl HostOptions {
    /// The shipped configuration: this executable is also the hook relay
    /// (`agent-office.exe hook <provider> …`).
    pub fn for_app(paths: &AppPaths) -> Self {
        let relay = match std::env::current_exe() {
            Ok(program) => Some(RelayCommand {
                program,
                prefix_args: vec!["hook".into()],
                // Hooks run outside our environment, so a non-default data
                // folder has to be spelled out for the relay to find the token.
                data_dir: (paths.data_dir != ao_ipc::paths::default_data_dir())
                    .then(|| paths.data_dir.clone()),
            }),
            Err(err) => {
                tracing::error!(%err, "cannot locate the Agent Office executable; hooks disabled");
                None
            }
        };
        Self {
            relay,
            claude: ClaudeOptions::default(),
            codex: CodexOptions::default(),
            cursor: CursorOptions::default(),
            git_executable: None,
        }
    }
}

/// Integration buttons in Diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum IntegrationAction {
    Status,
    Install,
    Repair,
    Uninstall,
}

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
    pub git: Arc<GitService>,
    sink: EventSink,
    relay: Option<RelayCommand>,
    hooks: HookBridge,
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
    pub fn start(paths: AppPaths, options: HostOptions) -> Arc<Host> {
        let (sink, mut rx) = EventSink::new(65_536);
        let demo = Arc::new(DemoAdapter::new());
        let registry = crate::providers::build_registry(
            demo.clone(),
            options.claude,
            options.codex,
            options.cursor,
        );
        let endpoint = ao_ipc::Endpoint::for_data_dir(&paths.data_dir);
        let (reader, writer_store, store_error) = open_store(&paths);
        let prefs = Preferences::load(&reader);

        let mut pipeline = Pipeline::new(prefs.sanitize_limits());
        pipeline.set_projects(project_roots(&reader));

        let mut world = WorldState::new();
        if let Ok(snapshot) = reader.load_world_since(now_ms() - RESTORE_WINDOW_MS) {
            world.restore(snapshot);
        }
        let git = GitService::start(sink.clone(), options.git_executable.clone());
        git.set_projects(project_folders(&reader));

        let host = Arc::new(Host {
            paths,
            registry,
            demo,
            git,
            sink,
            relay: options.relay,
            hooks: HookBridge::new(endpoint),
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

        let settings = host.preferences().provider_settings();
        for adapter in host.registry.adapters() {
            adapter.configure(&settings);
            let ctx = host.adapter_context();
            tokio::spawn(async move { adapter.start(ctx).await });
        }
        if host.relay.is_some() {
            let bridge_host = host.clone();
            tokio::spawn(async move { bridge_host.start_hook_bridge().await });
        }

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

        let silence_host = Arc::downgrade(&host);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(SILENCE_CHECK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let Some(host) = silence_host.upgrade() else {
                    break;
                };
                host.mark_silent_sessions(now_ms());
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
            relay: self.relay.clone(),
            data_dir: self.paths.data_dir.clone(),
        }
    }

    /// Starts the local IPC server the hook relay talks to. Holds only a weak
    /// reference so the host can shut down while the listener is running.
    async fn start_hook_bridge(self: Arc<Self>) {
        let token = match ao_ipc::token::ensure_token(&self.paths.data_dir) {
            Ok(token) => token,
            Err(err) => {
                let message = format!("cannot create the IPC token: {err}");
                self.record_provider_error("agent-office", "hooks", &message);
                *self.hooks.error.lock().expect("error lock") = Some(message);
                return;
            }
        };
        let weak = Arc::downgrade(&self);
        let handler = ao_ipc::server::handler(move |request: HookRequest| {
            let weak = weak.clone();
            async move {
                match weak.upgrade() {
                    Some(host) => host.handle_hook(request).await,
                    None => HookResponse::default(),
                }
            }
        });
        match ao_ipc::server::start(self.hooks.endpoint.clone(), token, handler).await {
            Ok(server) => {
                tracing::info!(endpoint = %self.hooks.endpoint.display(), "hook bridge listening");
                *self.hooks.server.lock().expect("server lock") = Some(server);
            }
            Err(err) => {
                let message = format!("hook bridge unavailable: {err}");
                self.record_provider_error("agent-office", "hooks", &message);
                *self.hooks.error.lock().expect("error lock") = Some(message);
            }
        }
    }

    /// One hook invocation from an agent CLI. Unknown providers and failures
    /// answer with an empty response, which lets the agent continue normally.
    pub async fn handle_hook(&self, request: HookRequest) -> HookResponse {
        let event = request
            .payload
            .get("hook_event_name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        self.hooks
            .record(&request.provider, event, request.received_at_ms);
        let Some(adapter) = self.registry.get(&ProviderId::new(&request.provider)) else {
            return HookResponse::default();
        };
        let reply = adapter
            .handle_hook(
                HookCall {
                    managed_origin: request.origin == HookOrigin::Managed,
                    payload: request.payload,
                    received_at_ms: request.received_at_ms,
                    payload_truncated: request.payload_truncated,
                },
                self.adapter_context(),
            )
            .await;
        HookResponse {
            stdout: reply.stdout,
            exit_code: reply.exit_code,
        }
    }

    pub fn hook_status(&self) -> HookBridgeStatus {
        self.hooks.status(self.relay.as_ref())
    }

    pub async fn integration_action(
        &self,
        provider: ProviderId,
        action: IntegrationAction,
    ) -> Result<IntegrationStatus, String> {
        let adapter = self.adapter(&provider)?;
        let ctx = self.adapter_context();
        let result = match action {
            IntegrationAction::Status => Ok(adapter.integration_status(&ctx).await),
            IntegrationAction::Install => adapter.install_integration(&ctx).await,
            IntegrationAction::Repair => adapter.repair_integration(&ctx).await,
            IntegrationAction::Uninstall => adapter.uninstall_integration(&ctx).await,
        };
        match &result {
            Ok(status) if action != IntegrationAction::Status => {
                tracing::info!(%provider, ?action, state = ?status.state, "integration updated");
            }
            Err(err) => self.record_provider_error(&provider.0, "integration", &err.to_string()),
            _ => {}
        }
        result.map_err(|e| e.to_string())
    }

    pub async fn list_external_sessions(
        &self,
        provider: ProviderId,
    ) -> Result<Vec<ExternalSessionInfo>, String> {
        self.adapter(&provider)?
            .list_sessions()
            .await
            .map_err(|e| e.to_string())
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

    /// Records an adapter/pipeline problem for Diagnostics.
    pub fn record_provider_error(&self, provider: &str, component: &str, message: &str) {
        tracing::warn!(target: "provider", provider, component, message, "provider error");
        let mut errors = self.provider_errors.lock().expect("errors lock");
        errors.push_back(ProviderErrorRecord {
            at: now_ms(),
            provider: provider.to_owned(),
            component: component.to_owned(),
            message: message.to_owned(),
        });
        while errors.len() > MAX_PROVIDER_ERRORS {
            errors.pop_front();
        }
    }

    fn ingest(&self, event: AgentEvent) {
        self.ingested.fetch_add(1, Ordering::Relaxed);
        if let EventKind::ProviderError(err) = &event.kind {
            self.record_provider_error(&event.provider.0, &err.component, &err.message);
        }
        let mut inner = self.inner.lock().expect("host lock");
        let event = match inner.pipeline.ingest(event) {
            Ingest::Accepted(event) => event,
            Ingest::Duplicate => return,
            Ingest::Rejected {
                provider,
                event_type,
                reason,
            } => {
                drop(inner);
                self.record_provider_error(
                    &provider,
                    "pipeline",
                    &format!("rejected {event_type}: {reason}"),
                );
                return;
            }
        };
        let touched = inner.world.apply(&event);
        let key = ao_core::ids::session_key(&event.provider, &event.session_id);
        let folder = inner.world.session(&key).and_then(|s| s.cwd.clone());
        self.git.observe(&event, folder.as_deref());
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

    /// Flags busy sessions that went quiet (see `WorldState::mark_silent`).
    pub fn mark_silent_sessions(&self, now: i64) {
        let minutes = self
            .prefs
            .lock()
            .expect("prefs lock")
            .silence_warning_minutes;
        let mut inner = self.inner.lock().expect("host lock");
        let touched = inner.world.mark_silent(now, minutes as i64 * 60_000);
        if !touched.is_empty() {
            inner.batcher.note_touched(touched);
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

    pub fn rejected(&self) -> u64 {
        self.inner.lock().expect("host lock").pipeline.rejected()
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
        self.git.set_projects(self.with_store(project_folders));
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

    pub async fn set_preferences(&self, prefs: Preferences) -> Result<Preferences, String> {
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

        // Hook settings depend on preferences (e.g. whether permission
        // requests wait for Agent Office), so installed hooks are refreshed.
        let settings = prefs.provider_settings();
        let ctx = self.adapter_context();
        for adapter in self.registry.adapters() {
            adapter.configure(&settings);
            if adapter.integration_status(&ctx).await.state == IntegrationState::NeedsRepair {
                if let Err(err) = adapter.repair_integration(&ctx).await {
                    self.record_provider_error(&adapter.id().0, "integration", &err.to_string());
                }
            }
        }
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

    /// Restart: continues a managed session's conversation in a new process
    /// (Claude `--resume`, Codex `thread/resume`) with the folder, model and
    /// permission mode it had. A running session is stopped gracefully first.
    pub async fn restart(
        &self,
        provider: ProviderId,
        session: SessionId,
    ) -> Result<SessionHandle, String> {
        let adapter = self.ensure_capability(&provider, Some(&session), "resume", |c| c.resume)?;
        let key = ao_core::ids::session_key(&provider, &session);
        let state = self
            .inner
            .lock()
            .expect("host lock")
            .world
            .session(&key)
            .cloned()
            .ok_or_else(|| format!("Session {session} is not known"))?;
        if state.mode != SessionMode::Managed {
            return Err("Only sessions started from Agent Office can be restarted.".into());
        }
        let cwd = state
            .cwd
            .clone()
            .filter(|c| !c.trim().is_empty())
            .ok_or_else(|| "This session has no project folder to restart in.".to_string())?;
        if state.status == SessionStatus::Active {
            if let Err(err) = adapter.stop_session(&session, StopMode::Graceful).await {
                // The process may already be gone; its end is on the way.
                tracing::info!(%err, %key, "stop before restart");
            }
            let deadline = std::time::Instant::now() + RESTART_STOP_WAIT;
            loop {
                let ended = self
                    .inner
                    .lock()
                    .expect("host lock")
                    .world
                    .session(&key)
                    .map_or(true, |s| s.status == SessionStatus::Ended);
                if ended {
                    break;
                }
                if std::time::Instant::now() > deadline {
                    return Err(
                        "The session did not stop in time; use Force stop, then Restart.".into(),
                    );
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        let request = LaunchRequest {
            project_id: state.project_id.clone(),
            cwd,
            model: state.model.clone(),
            prompt: None,
            name: None,
            permission_mode: state.permission_mode.clone(),
            resume_session_id: Some(session.0.clone()),
        };
        adapter
            .launch_session(request, self.adapter_context())
            .await
            .map_err(|e| e.to_string())
    }

    /// Repositories agents work in and project folders, with the changed
    /// files each session's tools are known to have written. `refresh`
    /// re-reads trees older than a few seconds first.
    pub async fn repositories(&self, refresh: bool) -> RepositoriesReport {
        if refresh {
            self.git.refresh_now(Duration::from_secs(5)).await;
        }
        let (mut report, links) = self.git.report();
        let inner = self.inner.lock().expect("host lock");
        for repo in &mut report.repositories {
            let (Some(snapshot), Some(linked)) =
                (&repo.snapshot, links.get(&repo.location.worktree_root))
            else {
                continue;
            };
            let root = &repo.location.worktree_root;
            // Each linked session's written files, relative to the tree.
            let touched: Vec<(&str, Vec<String>)> = linked
                .iter()
                .filter_map(|link| {
                    let session = inner.world.session(&link.key)?;
                    let cwd = session.cwd.as_deref().unwrap_or(&link.folder);
                    let files = session
                        .stats
                        .files_changed
                        .iter()
                        .filter_map(|f| relative_to_tree(root, cwd, f))
                        .collect::<Vec<_>>();
                    (!files.is_empty()).then_some((link.key.as_str(), files))
                })
                .collect();
            for file in &snapshot.status.files {
                let sessions: Vec<String> = touched
                    .iter()
                    .filter(|(_, files)| {
                        files.iter().any(|t| {
                            crate::git::covers(&file.path, t)
                                || file
                                    .orig_path
                                    .as_deref()
                                    .is_some_and(|o| crate::git::covers(o, t))
                        })
                    })
                    .map(|(key, _)| (*key).to_owned())
                    .collect();
                if !sessions.is_empty() {
                    repo.file_sessions.push(crate::git::FileSessions {
                        path: file.path.clone(),
                        sessions,
                    });
                }
            }
        }
        report
    }

    /// Opens the user's terminal in a session's folder (see `terminal.rs`).
    /// Works for any session whose folder exists on this computer.
    pub fn open_terminal(&self, provider: &ProviderId, session: &SessionId) -> Result<(), String> {
        let key = ao_core::ids::session_key(provider, session);
        let cwd = self
            .inner
            .lock()
            .expect("host lock")
            .world
            .session(&key)
            .ok_or_else(|| format!("Session {session} is not known"))?
            .cwd
            .clone()
            .unwrap_or_default();
        let dir = crate::terminal::resolve_folder(&cwd)?;
        crate::terminal::open_in(&dir).map(|_| ())
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

fn project_folders(store: &Store) -> Vec<(String, String)> {
    store
        .list_projects()
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.id, p.path))
        .collect()
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

    /// Unique per call: tests run in parallel and may start in the same
    /// millisecond; a shared folder would let one test delete another's data.
    fn uuid_like() -> String {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{}-{}-{n}", std::process::id(), now_ms())
    }

    /// No relay, and a Claude executable that does not exist, so tests never
    /// see real sessions from the machine they run on.
    fn test_options() -> HostOptions {
        HostOptions {
            relay: None,
            claude: ClaudeOptions {
                executable: Some("/nonexistent/claude".into()),
                config_dir: Some(std::env::temp_dir().join("ao-host-test-no-claude-config")),
                ..Default::default()
            },
            codex: CodexOptions {
                executable: Some("/nonexistent/codex".into()),
                config_dir: Some(std::env::temp_dir().join("ao-host-test-no-codex-home")),
            },
            cursor: CursorOptions {
                executable: Some("/nonexistent/cursor-agent".into()),
            },
            git_executable: None,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn events_flow_to_state_ui_and_disk() {
        let paths = temp_paths();
        let host = Host::start(paths.clone(), test_options());
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
    async fn busy_sessions_that_go_quiet_are_flagged_not_stopped() {
        let paths = temp_paths();
        let host = Host::start(paths.clone(), test_options());
        let sink = host.adapter_context().sink;
        let t0 = now_ms();
        let event = |kind: EventKind| {
            AgentEvent::for_session("demo", "quiet", EventSource::Simulation, kind).at(t0)
        };
        sink.emit(event(EventKind::SessionStarted(SessionInfo::default())));
        sink.emit(event(EventKind::AgentThinking(TextNote::default())));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while host.snapshot().sessions.is_empty() {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let silence = Preferences::default().silence_warning_minutes as i64 * 60_000;
        host.mark_silent_sessions(t0 + silence - 1);
        assert_eq!(host.snapshot().sessions[0].silent_since, None);
        host.mark_silent_sessions(t0 + silence);
        let session = host.snapshot().sessions[0].clone();
        assert_eq!(session.silent_since, Some(t0));
        assert_eq!(
            session.status,
            SessionStatus::Active,
            "a quiet session is never ended"
        );
        let _ = std::fs::remove_dir_all(paths.data_dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn open_terminal_only_uses_known_sessions_with_existing_folders() {
        let paths = temp_paths();
        let host = Host::start(paths.clone(), test_options());
        let demo = ProviderId::new("demo");
        let err = host
            .open_terminal(&demo, &SessionId::new("nobody"))
            .unwrap_err();
        assert!(err.contains("not known"), "{err}");
        let gone = paths.data_dir.join("deleted-project");
        host.adapter_context().sink.emit(AgentEvent::for_session(
            "demo",
            "t1",
            EventSource::Simulation,
            EventKind::SessionStarted(SessionInfo {
                cwd: Some(gone.display().to_string()),
                ..Default::default()
            }),
        ));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while host.snapshot().sessions.is_empty() {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // No terminal is started for a folder that is not there.
        let err = host
            .open_terminal(&demo, &SessionId::new("t1"))
            .unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
        let _ = std::fs::remove_dir_all(paths.data_dir);
    }

    #[test]
    fn test_folders_are_unique_within_one_millisecond() {
        // Two tests starting together once shared a folder (and a database):
        // one waited 10 s on the other's lock and lost its data.
        let a = temp_paths();
        let b = temp_paths();
        assert_ne!(a.data_dir, b.data_dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn restart_closes_orphaned_managed_sessions() {
        let paths = temp_paths();
        {
            let host = Host::start(paths.clone(), test_options());
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
        let host = Host::start(paths.clone(), test_options());
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
