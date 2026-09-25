//! Claude Code adapter.
//!
//! * **External sessions** (`claude` in your own terminal): user-level hooks in
//!   `~/.claude/settings.json` forward every lifecycle event to the app via the
//!   hook relay; `claude agents --json` discovers sessions and notices when
//!   they end. Permission prompts are observed, or answered from the app when
//!   the user opts in.
//! * **Managed sessions**: `claude -p --input-format stream-json
//!   --output-format stream-json` launched by Agent Office with per-session
//!   hooks passed through `--settings`. Prompts go in on stdin; permission
//!   prompts are answered through the documented `PermissionRequest` hook.
//!
//! See docs/PROVIDER_CAPABILITIES.md §3 for the sources of every field.

pub mod hooks;
pub mod listing;
pub mod settings;
pub mod stream;
pub mod tools;

use ao_core::capabilities::{Capabilities, CapabilityProfile, ImplementedFeatures, Support::*};
use ao_core::event::*;
use ao_core::ids::{PermissionRequestId, ProviderId, SessionId};
use ao_core::provider::{
    AdapterContext, ExternalSessionInfo, HookCall, HookReply, InstallationInfo, IntegrationStatus,
    LaunchRequest, PermissionDecision, ProviderAdapter, ProviderDescriptor, ProviderError,
    ProviderSettings, RelayCommand, SessionHandle, StopMode,
};
use ao_core::time::now_ms;
use ao_process::{ManagedProcess, ProcessEvent, SpawnSpec, Stream};
use async_trait::async_trait;
use settings::{HookPlan, SettingsFile};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::oneshot;

pub const PROVIDER_ID: &str = "claude";

/// Install locations checked after `PATH`.
const EXTRA_DIRS: &[&str] = if cfg!(windows) {
    &[
        "%USERPROFILE%\\.local\\bin", // native installer
        "%APPDATA%\\npm",             // npm global install (claude.cmd)
    ]
} else {
    &[
        "~/.local/bin",
        "~/.claude/local",
        "/usr/local/bin",
        "/opt/homebrew/bin",
    ]
};

/// Permission modes accepted by `claude --permission-mode` (2.1.281 help).
const PERMISSION_MODES: &[&str] = &[
    "acceptEdits",
    "auto",
    "bypassPermissions",
    "manual",
    "dontAsk",
    "plan",
    "default",
];

#[derive(Debug, Clone)]
pub struct ClaudeOptions {
    /// Use this executable instead of detecting `claude` (tests).
    pub executable: Option<PathBuf>,
    /// Claude configuration folder (hooks go into its `settings.json`).
    /// Defaults to `$CLAUDE_CONFIG_DIR` or `~/.claude`. When set, sessions
    /// launched by Agent Office also run with `CLAUDE_CONFIG_DIR` pointing here.
    pub config_dir: Option<PathBuf>,
    /// How often `claude agents --json` is polled for external sessions.
    pub discovery_interval: Duration,
}

impl Default for ClaudeOptions {
    fn default() -> Self {
        Self {
            executable: None,
            config_dir: None,
            discovery_interval: Duration::from_secs(30),
        }
    }
}

struct ManagedSession {
    process: Arc<ManagedProcess>,
}

struct Pending {
    session_id: String,
    tx: oneshot::Sender<PermissionDecision>,
}

#[derive(Default)]
struct State {
    managed: HashMap<String, ManagedSession>,
    /// Ids of managed sessions being spawned (their hooks may arrive before
    /// the process handle is stored).
    launching: HashSet<String>,
    pending: HashMap<String, Pending>,
    session_agent_type: HashMap<String, String>,
    hooked: HashSet<String>,
    discovery: listing::Discovery,
}

struct Inner {
    options: ClaudeOptions,
    settings: RwLock<ProviderSettings>,
    state: Mutex<State>,
    executable: Mutex<Option<PathBuf>>,
}

pub struct ClaudeAdapter {
    inner: Arc<Inner>,
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self::with_options(ClaudeOptions::default())
    }

    pub fn with_options(options: ClaudeOptions) -> Self {
        Self {
            inner: Arc::new(Inner {
                options,
                settings: RwLock::new(ProviderSettings::default()),
                state: Mutex::new(State::default()),
                executable: Mutex::new(None),
            }),
        }
    }

    fn settings(&self) -> ProviderSettings {
        self.inner.settings.read().expect("settings lock").clone()
    }

    fn settings_file(&self) -> Option<SettingsFile> {
        match &self.inner.options.config_dir {
            Some(dir) => Some(SettingsFile::new(dir.join("settings.json"))),
            None => settings::default_location(),
        }
    }

    fn global_plan(&self, relay: &RelayCommand) -> HookPlan {
        let settings = self.settings();
        HookPlan {
            relay: relay.clone(),
            origin: "global",
            answer_permissions: settings.answer_permissions_from_app,
            permission_wait_secs: settings.permission_timeout_secs as u64 + 30,
        }
    }

    fn is_managed(&self, session_id: &str) -> bool {
        let state = self.inner.state.lock().expect("state lock");
        state.managed.contains_key(session_id) || state.launching.contains(session_id)
    }
}

impl Inner {
    async fn executable(&self) -> Result<PathBuf, ProviderError> {
        if let Some(path) = &self.options.executable {
            return Ok(path.clone());
        }
        if let Some(path) = self.executable.lock().expect("exe lock").clone() {
            return Ok(path);
        }
        let info = detect(&self.options).await;
        match info.executable_path {
            Some(path) if info.installed => {
                let path = PathBuf::from(path);
                *self.executable.lock().expect("exe lock") = Some(path.clone());
                Ok(path)
            }
            _ => Err(ProviderError::NotInstalled),
        }
    }

    async fn list_sessions(&self) -> Result<Vec<listing::ListedSession>, ProviderError> {
        let exe = self.executable().await?;
        let output = ao_detect::run_capture(&exe, &["agents", "--json"], Duration::from_secs(15))
            .await
            .map_err(ProviderError::Other)?;
        listing::parse_agents_json(&output)
            .map_err(|e| ProviderError::Protocol(format!("`claude agents --json`: {e}")))
    }

    fn cancel_pending_for(&self, session_id: &str) {
        let mut state = self.state.lock().expect("state lock");
        state.pending.retain(|_, p| p.session_id != session_id);
    }
}

async fn detect(options: &ClaudeOptions) -> InstallationInfo {
    match &options.executable {
        Some(path) => {
            let dir = path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let name = path
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            ao_detect::detect(&[name], &[dir.as_str()], &["--version"]).await
        }
        None => ao_detect::detect(&["claude".to_string()], EXTRA_DIRS, &["--version"]).await,
    }
}

pub fn capability_profile() -> CapabilityProfile {
    CapabilityProfile {
        managed: Capabilities {
            launch: Supported,
            attach: Unsupported,
            list_sessions: Supported,
            stop: Supported,
            send_prompt: Supported,
            structured_events: Supported,
            tool_events: Supported,
            file_events: Partial,
            command_events: Supported,
            permissions: Supported,
            subagents: Supported,
            usage: Supported,
            cost: Partial,
            model: Supported,
            context_compaction: Supported,
            resume: Supported,
        },
        external: Capabilities {
            launch: Unsupported,
            attach: Supported,
            list_sessions: Supported,
            stop: Partial,
            send_prompt: Unsupported,
            structured_events: Supported,
            tool_events: Supported,
            file_events: Partial,
            command_events: Supported,
            permissions: Partial,
            subagents: Supported,
            usage: Unsupported,
            cost: Unsupported,
            model: Partial,
            context_compaction: Supported,
            resume: Unsupported,
        },
        implemented: ImplementedFeatures {
            detection: true,
            managed_sessions: true,
            external_sessions: true,
            integration_setup: true,
        },
        notes: vec![
            "External sessions are observed through user-level hooks in %USERPROFILE%\\.claude\\settings.json (Install integration).".into(),
            "Managed sessions run `claude -p` with stream-json I/O; prompts are sent from Agent Office.".into(),
            "File events are tool-level evidence (Edit/Write/Read inputs), not a filesystem watch.".into(),
            "Cost is Claude Code's client-side estimate (total_cost_usd) and is labelled as such.".into(),
            "Stop for external sessions only applies to background sessions (claude stop <id>).".into(),
            "Answering permission prompts of external sessions from Agent Office is opt-in.".into(),
        ],
    }
}

fn descriptor() -> ProviderDescriptor {
    ProviderDescriptor {
        id: ProviderId::new(PROVIDER_ID),
        display_name: "Claude Code".into(),
        accent_color: "#d97757".into(),
        badge: "CC".into(),
        executable_names: vec!["claude".into()],
        homepage: Some("https://code.claude.com/docs".into()),
        simulated: false,
    }
}

fn process_event(session: &str, kind: EventKind) -> AgentEvent {
    AgentEvent::for_session(
        PROVIDER_ID,
        SessionId::new(session),
        EventSource::Process,
        kind,
    )
}

#[async_trait]
impl ProviderAdapter for ClaudeAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        descriptor()
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile()
    }

    async fn detect_installation(&self) -> InstallationInfo {
        let info = detect(&self.inner.options).await;
        if let (true, Some(path)) = (info.installed, &info.executable_path) {
            *self.inner.executable.lock().expect("exe lock") = Some(PathBuf::from(path));
        }
        info
    }

    fn configure(&self, settings: &ProviderSettings) {
        *self.inner.settings.write().expect("settings lock") = settings.clone();
    }

    async fn start(&self, ctx: AdapterContext) {
        let inner = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            loop {
                let Some(strong) = inner.upgrade() else { break };
                let interval = strong.options.discovery_interval;
                let enabled = strong
                    .settings
                    .read()
                    .expect("settings lock")
                    .discover_external_sessions;
                if enabled {
                    match strong.list_sessions().await {
                        Ok(listed) => {
                            let events = {
                                let mut state = strong.state.lock().expect("state lock");
                                let managed: HashSet<String> =
                                    state.managed.keys().cloned().collect();
                                let hooked = state.hooked.clone();
                                state
                                    .discovery
                                    .reconcile(listed, &hooked, &managed, now_ms())
                            };
                            for event in events {
                                ctx.sink.emit(event);
                            }
                        }
                        Err(ProviderError::NotInstalled) => {}
                        Err(err) => {
                            tracing::debug!(target: "provider::claude", %err, "session discovery failed")
                        }
                    }
                }
                drop(strong);
                tokio::time::sleep(interval).await;
            }
        });
    }

    async fn handle_hook(&self, call: HookCall, ctx: AdapterContext) -> HookReply {
        let Some(session_id) = hooks::session_id(&call.payload).map(str::to_owned) else {
            return HookReply::default();
        };
        let managed = self.is_managed(&session_id);
        if managed && !call.managed_origin {
            // The session's own injected hooks already report it.
            return HookReply::default();
        }
        let event_name = hooks::event_name(&call.payload).to_owned();
        let session_agent_type = {
            let mut state = self.inner.state.lock().expect("state lock");
            state.hooked.insert(session_id.clone());
            if event_name == "SessionStart" {
                if let Some(agent_type) = call.payload.get("agent_type").and_then(|v| v.as_str()) {
                    state
                        .session_agent_type
                        .insert(session_id.clone(), agent_type.to_owned());
                }
            }
            state.session_agent_type.get(&session_id).cloned()
        };
        let mut hook_ctx = hooks::HookContext {
            received_at_ms: call.received_at_ms,
            managed,
            permission: None,
            session_agent_type,
        };

        if event_name != settings::PERMISSION_EVENT {
            for event in hooks::map_hook(&call.payload, &hook_ctx) {
                ctx.sink.emit(event);
            }
            return HookReply::default();
        }

        // PermissionRequest: Claude sends no id, so Agent Office assigns one.
        let request_id = format!("perm-{}", uuid::Uuid::new_v4().simple());
        let settings = self.settings();
        let can_answer = call.managed_origin || settings.answer_permissions_from_app;
        hook_ctx.permission = Some((request_id.clone(), can_answer));
        for event in hooks::map_hook(&call.payload, &hook_ctx) {
            ctx.sink.emit(event);
        }
        if !can_answer {
            return HookReply::default();
        }

        let (tx, rx) = oneshot::channel();
        self.inner.state.lock().expect("state lock").pending.insert(
            request_id.clone(),
            Pending {
                session_id: session_id.clone(),
                tx,
            },
        );
        let timeout = Duration::from_secs(settings.permission_timeout_secs.max(5) as u64);
        let outcome = tokio::time::timeout(timeout, rx).await;
        self.inner
            .state
            .lock()
            .expect("state lock")
            .pending
            .remove(&request_id);

        let resolved = |by, message: Option<String>| PermissionResolved {
            request_id: PermissionRequestId::new(&request_id),
            resolved_by: by,
            message,
        };
        let emit = |kind| {
            let mut e = AgentEvent::for_session(
                PROVIDER_ID,
                SessionId::new(&session_id),
                EventSource::Hook,
                kind,
            );
            if let Some(agent) = call
                .payload
                .get("agent_id")
                .and_then(|v| v.as_str())
                .filter(|a| !a.is_empty())
            {
                e = e.with_agent(agent, None);
            }
            ctx.sink.emit(e);
        };
        match outcome {
            Ok(Ok(PermissionDecision::Approve { .. })) => {
                emit(EventKind::PermissionApproved(resolved(
                    PermissionResolver::App,
                    None,
                )));
                HookReply {
                    stdout: Some(hooks::permission_decision_output(true, None)),
                    exit_code: 0,
                }
            }
            Ok(Ok(PermissionDecision::Reject { message })) => {
                let message = message.unwrap_or_else(|| "Rejected in Agent Office".into());
                emit(EventKind::PermissionDenied(resolved(
                    PermissionResolver::App,
                    Some(message.clone()),
                )));
                HookReply {
                    stdout: Some(hooks::permission_decision_output(false, Some(&message))),
                    exit_code: 0,
                }
            }
            // Session ended or timed out.
            Ok(Err(_)) | Err(_) if managed => {
                let message = format!("No answer in Agent Office within {} s", timeout.as_secs());
                emit(EventKind::PermissionDenied(resolved(
                    PermissionResolver::Timeout,
                    Some(message.clone()),
                )));
                HookReply {
                    stdout: Some(hooks::permission_decision_output(false, Some(&message))),
                    exit_code: 0,
                }
            }
            Ok(Err(_)) | Err(_) => {
                // External session: hand the decision back to Claude's own prompt.
                emit(EventKind::PermissionExpired(resolved(
                    PermissionResolver::Timeout,
                    Some("No answer in Agent Office — answer in Claude's terminal".into()),
                )));
                HookReply::default()
            }
        }
    }

    async fn resolve_permission(
        &self,
        session_id: &SessionId,
        request_id: &PermissionRequestId,
        decision: PermissionDecision,
    ) -> Result<(), ProviderError> {
        let pending = {
            let mut state = self.inner.state.lock().expect("state lock");
            match state.pending.get(request_id.as_str()) {
                Some(p) if p.session_id == session_id.as_str() => {
                    state.pending.remove(request_id.as_str())
                }
                _ => None,
            }
        };
        let pending = pending.ok_or_else(|| {
            ProviderError::Other(
                "This permission request is no longer waiting (answered elsewhere or timed out)."
                    .into(),
            )
        })?;
        pending
            .tx
            .send(decision)
            .map_err(|_| ProviderError::Other("Claude stopped waiting for this answer.".into()))
    }

    async fn launch_session(
        &self,
        request: LaunchRequest,
        ctx: AdapterContext,
    ) -> Result<SessionHandle, ProviderError> {
        let relay = ctx.relay.clone().ok_or_else(|| {
            ProviderError::Other(
                "The hook relay is unavailable; managed Claude sessions need it.".into(),
            )
        })?;
        let cwd = PathBuf::from(request.cwd.trim());
        if request.cwd.trim().is_empty() || !cwd.is_dir() {
            return Err(ProviderError::Other(format!(
                "Project folder `{}` does not exist.",
                request.cwd
            )));
        }
        let exe = self.inner.executable().await?;
        // Restart: `--resume <id>` continues the conversation under the same
        // session id (a new id would need `--fork-session`).
        let resuming = match request.resume_session_id.as_deref().map(str::trim) {
            Some(id) if !id.is_empty() => {
                let id = uuid::Uuid::parse_str(id)
                    .map_err(|_| {
                        ProviderError::Other(format!("`{id}` is not a Claude Code session id."))
                    })?
                    .to_string();
                if self
                    .inner
                    .state
                    .lock()
                    .expect("state lock")
                    .managed
                    .contains_key(&id)
                {
                    return Err(ProviderError::Other(
                        "This Claude Code session is still running; stop it before restarting it."
                            .into(),
                    ));
                }
                Some(id)
            }
            _ => None,
        };
        let session_id = resuming
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let settings = self.settings();

        let plan = HookPlan {
            relay,
            origin: "managed",
            answer_permissions: true,
            permission_wait_secs: settings.permission_timeout_secs as u64 + 30,
        };
        let sessions_dir = ctx.data_dir.join("sessions");
        std::fs::create_dir_all(&sessions_dir)?;
        let settings_path = sessions_dir.join(format!("claude-{session_id}.settings.json"));
        std::fs::write(&settings_path, plan.settings_document().to_string())?;

        let mut spec = SpawnSpec::new(&exe)
            .args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
            ])
            .arg(if resuming.is_some() {
                "--resume"
            } else {
                "--session-id"
            })
            .arg(&session_id)
            .arg("--settings")
            .arg(settings_path.as_os_str())
            .cwd(&cwd);
        if let Some(dir) = &self.inner.options.config_dir {
            spec = spec.env("CLAUDE_CONFIG_DIR", dir.as_os_str());
        }
        if let Some(model) = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            spec = spec.arg("--model").arg(model);
        }
        let permission_mode = request
            .permission_mode
            .as_deref()
            .filter(|m| PERMISSION_MODES.contains(m));
        if let Some(mode) = permission_mode {
            spec =
                spec.arg("--permission-mode")
                    .arg(if mode == "default" { "manual" } else { mode });
        }
        if let Some(name) = request
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty() && resuming.is_none())
        {
            spec = spec.arg("-n").arg(name);
        }

        self.inner
            .state
            .lock()
            .expect("state lock")
            .launching
            .insert(session_id.clone());
        let spawned = ManagedProcess::spawn(spec);
        let spawned = {
            let mut state = self.inner.state.lock().expect("state lock");
            state.launching.remove(&session_id);
            if let Ok((process, _)) = &spawned {
                state.managed.insert(
                    session_id.clone(),
                    ManagedSession {
                        process: process.clone(),
                    },
                );
            }
            spawned
        };
        let (process, mut events) = match spawned {
            Ok(spawned) => spawned,
            Err(err) => {
                let _ = std::fs::remove_file(&settings_path);
                return Err(err.into());
            }
        };
        process.set_label(format!("Claude Code · session {session_id}"));

        ctx.sink.emit(process_event(
            &session_id,
            EventKind::SessionStarted(SessionInfo {
                mode: Some(SessionMode::Managed),
                cwd: Some(cwd.display().to_string()),
                model: request.model.clone().filter(|m| !m.trim().is_empty()),
                title: request.name.clone().filter(|n| !n.trim().is_empty()),
                reason: Some(
                    if resuming.is_some() {
                        "restarted by Agent Office"
                    } else {
                        "launched by Agent Office"
                    }
                    .into(),
                ),
                permission_mode: permission_mode.map(str::to_owned),
                pid: Some(process.pid()),
                ..Default::default()
            }),
        ));

        let inner = self.inner.clone();
        let sink = ctx.sink.clone();
        let id = session_id.clone();
        tokio::spawn(async move {
            let mut stderr_tail: VecDeque<String> = VecDeque::new();
            while let Some(event) = events.recv().await {
                match event {
                    ProcessEvent::Line {
                        stream: Stream::Stdout,
                        line,
                    } => {
                        for e in stream::map_stream_line(&line, &id, now_ms()) {
                            sink.emit(e);
                        }
                    }
                    ProcessEvent::Line {
                        stream: Stream::Stderr,
                        line,
                    } => {
                        tracing::debug!(target: "provider::claude", session = %id, "stderr: {line}");
                        if stderr_tail.len() == 20 {
                            stderr_tail.pop_front();
                        }
                        stderr_tail.push_back(line);
                    }
                    ProcessEvent::Exited(info) => {
                        inner.state.lock().expect("state lock").managed.remove(&id);
                        inner.cancel_pending_for(&id);
                        let _ = std::fs::remove_file(&settings_path);
                        if !info.success && !info.killed {
                            let detail = stderr_tail
                                .iter()
                                .rev()
                                .find(|l| !l.trim().is_empty())
                                .cloned();
                            sink.emit(process_event(
                                &id,
                                EventKind::AgentError(AgentError {
                                    message: detail.unwrap_or_else(|| {
                                        format!("Claude Code {}", info.describe())
                                    }),
                                    error_type: Some("process_exit".into()),
                                    recoverable: false,
                                }),
                            ));
                        }
                        let reason = info.describe();
                        sink.emit(process_event(
                            &id,
                            EventKind::SessionEnded(SessionEnded {
                                reason: Some(reason),
                                exit_code: info.code,
                            }),
                        ));
                        break;
                    }
                }
            }
        });

        let handle = SessionHandle {
            provider: ProviderId::new(PROVIDER_ID),
            session_id: SessionId::new(&session_id),
            mode: SessionMode::Managed,
            pid: Some(process.pid()),
        };
        if let Some(prompt) = request.prompt.as_deref().filter(|p| !p.trim().is_empty()) {
            self.send_prompt(&handle.session_id, prompt).await?;
        }
        Ok(handle)
    }

    async fn send_prompt(&self, session_id: &SessionId, prompt: &str) -> Result<(), ProviderError> {
        let process = self
            .inner
            .state
            .lock()
            .expect("state lock")
            .managed
            .get(session_id.as_str())
            .map(|m| m.process.clone());
        let process = process.ok_or(ProviderError::Unsupported {
            capability: "sendPrompt",
        })?;
        process
            .write_line(&stream::user_message_line(prompt))
            .await?;
        Ok(())
    }

    async fn stop_session(
        &self,
        session_id: &SessionId,
        mode: StopMode,
    ) -> Result<(), ProviderError> {
        let process = self
            .inner
            .state
            .lock()
            .expect("state lock")
            .managed
            .get(session_id.as_str())
            .map(|m| m.process.clone());
        if let Some(process) = process {
            match mode {
                StopMode::Force => process.kill_tree(),
                StopMode::Graceful => {
                    tokio::spawn(async move {
                        process.stop(Duration::from_secs(10)).await;
                    });
                }
            }
            return Ok(());
        }
        // External: only background sessions have an official stop command.
        let background = self
            .inner
            .state
            .lock()
            .expect("state lock")
            .discovery
            .is_background(session_id.as_str());
        if !background {
            return Err(ProviderError::Other(
                "Agent Office can only stop Claude sessions it launched, or background sessions (`claude stop`). \
                 Stop this one in its terminal."
                    .into(),
            ));
        }
        let exe = self.inner.executable().await?;
        ao_detect::run_capture(
            &exe,
            &["stop", session_id.as_str()],
            Duration::from_secs(20),
        )
        .await
        .map(|_| ())
        .map_err(ProviderError::Other)
    }

    async fn list_sessions(&self) -> Result<Vec<ExternalSessionInfo>, ProviderError> {
        Ok(self
            .inner
            .list_sessions()
            .await?
            .iter()
            .map(listing::ListedSession::to_info)
            .collect())
    }

    async fn integration_status(&self, ctx: &AdapterContext) -> IntegrationStatus {
        let Some(file) = self.settings_file() else {
            return IntegrationStatus::unsupported(
                "Cannot locate the Claude Code settings folder (no home directory).",
            );
        };
        let plan = ctx.relay.as_ref().map(|r| self.global_plan(r));
        settings::status(&file, plan.as_ref())
    }

    async fn install_integration(
        &self,
        ctx: &AdapterContext,
    ) -> Result<IntegrationStatus, ProviderError> {
        let file = self
            .settings_file()
            .ok_or(ProviderError::Other("no home directory".into()))?;
        let relay = ctx
            .relay
            .as_ref()
            .ok_or(ProviderError::Other("hook relay unavailable".into()))?;
        settings::apply(&file, &self.global_plan(relay), true).map_err(ProviderError::Other)
    }

    async fn repair_integration(
        &self,
        ctx: &AdapterContext,
    ) -> Result<IntegrationStatus, ProviderError> {
        self.install_integration(ctx).await
    }

    async fn uninstall_integration(
        &self,
        ctx: &AdapterContext,
    ) -> Result<IntegrationStatus, ProviderError> {
        let file = self
            .settings_file()
            .ok_or(ProviderError::Other("no home directory".into()))?;
        let relay = ctx.relay.clone().unwrap_or(RelayCommand {
            program: PathBuf::from("agent-office"),
            prefix_args: vec!["hook".into()],
            data_dir: None,
        });
        settings::apply(&file, &self.global_plan(&relay), false).map_err(ProviderError::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_documented_capabilities() {
        let caps = capability_profile();
        assert_eq!(caps.managed.launch, Supported);
        assert_eq!(caps.external.send_prompt, Unsupported);
        assert_eq!(
            caps.external.usage, Unsupported,
            "hooks carry no token usage"
        );
        assert_eq!(caps.managed.cost, Partial, "cost is an estimate");
        assert!(caps.implemented.managed_sessions && caps.implemented.external_sessions);
    }

    #[tokio::test]
    async fn prompts_to_external_sessions_are_refused() {
        let adapter = ClaudeAdapter::new();
        assert!(adapter.send_prompt(&"s".into(), "hello").await.is_err());
        assert!(adapter
            .resolve_permission(
                &"s".into(),
                &"perm-x".into(),
                PermissionDecision::Approve { for_session: false }
            )
            .await
            .is_err());
    }
}
