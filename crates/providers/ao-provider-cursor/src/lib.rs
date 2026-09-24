//! Cursor CLI (`agent`, formerly `cursor-agent`) adapter.
//!
//! Managed sessions run `agent acp`: the Agent Client Protocol (JSON-RPC 2.0,
//! one message per line over stdio), implemented from the official schema
//! (`@agentclientprotocol/sdk` 1.5.0, protocol version 1). Cursor-specific
//! behaviour reported by integrators is handled defensively:
//!
//! * permission options are chosen by `kind` and their `optionId` echoed
//!   (Cursor's ids are hyphenated, e.g. `allow-once`);
//! * every agent→client request other than `session/request_permission`
//!   (e.g. `cursor/create_plan`) is answered at once with "method not found",
//!   because an unanswered request stalls the turn;
//! * `authenticate` (Cursor advertises `cursor_login`) is called only when
//!   `session/new` fails and the agent offers an authentication method.
//!
//! External (terminal) Cursor sessions are **not** implemented: Cursor's hook
//! format could not be verified (see PROVIDER_CAPABILITIES §5).

pub mod acp;

use acp::Mapper;
use ao_core::capabilities::{Capabilities, CapabilityProfile, ImplementedFeatures, Support::*};
use ao_core::event::*;
use ao_core::ids::{PermissionRequestId, ProviderId, SessionId};
use ao_core::provider::{
    AdapterContext, EventSink, InstallationInfo, LaunchRequest, PermissionDecision,
    ProviderAdapter, ProviderDescriptor, ProviderError, ProviderSettings, SessionHandle, StopMode,
};
use ao_core::time::now_ms;
use ao_jsonrpc::{Incoming, RpcClient, RpcError};
use ao_process::{ManagedProcess, ProcessEvent, SpawnSpec, Stream};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub const PROVIDER_ID: &str = "cursor";

const EXTRA_DIRS: &[&str] = if cfg!(windows) {
    &["%LOCALAPPDATA%\\cursor-agent", "%USERPROFILE%\\.local\\bin"]
} else {
    &["~/.local/bin", "/usr/local/bin", "/opt/homebrew/bin"]
};

const RPC_TIMEOUT: Duration = Duration::from_secs(30);
/// Cursor's login method (reported by integrators; see PROVIDER_CAPABILITIES §5).
const CURSOR_LOGIN: &str = "cursor_login";
/// `authenticate` may open a browser login; give the user time.
const AUTH_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Default)]
pub struct CursorOptions {
    /// Use this executable instead of detecting `cursor-agent` / `agent` (tests).
    pub executable: Option<PathBuf>,
}

struct PendingPermission {
    rpc_id: Value,
    options: Value,
    tool_call_id: Option<String>,
}

struct ManagedSession {
    rpc: Arc<RpcClient>,
    sink: EventSink,
    mapper: Mutex<Mapper>,
    turn_active: AtomicBool,
    /// Agent Office asked the process to stop.
    stopping: AtomicBool,
    permissions: Mutex<HashMap<String, PendingPermission>>,
}

#[derive(Default)]
struct State {
    managed: HashMap<String, Arc<ManagedSession>>,
}

struct Inner {
    options: CursorOptions,
    settings: RwLock<ProviderSettings>,
    state: Mutex<State>,
    executable: Mutex<Option<PathBuf>>,
}

pub struct CursorAdapter {
    inner: Arc<Inner>,
}

impl Default for CursorAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorAdapter {
    pub fn new() -> Self {
        Self::with_options(CursorOptions::default())
    }

    pub fn with_options(options: CursorOptions) -> Self {
        Self {
            inner: Arc::new(Inner {
                options,
                settings: RwLock::new(ProviderSettings::default()),
                state: Mutex::new(State::default()),
                executable: Mutex::new(None),
            }),
        }
    }

    fn managed(&self, session_id: &str) -> Option<Arc<ManagedSession>> {
        self.inner
            .state
            .lock()
            .expect("state lock")
            .managed
            .get(session_id)
            .cloned()
    }

    fn permission_timeout(&self) -> Duration {
        Duration::from_secs(
            self.inner
                .settings
                .read()
                .expect("settings lock")
                .permission_timeout_secs
                .max(5) as u64,
        )
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
        let info = detect().await;
        match info.executable_path {
            Some(path) if info.installed => {
                let path = PathBuf::from(path);
                *self.executable.lock().expect("exe lock") = Some(path.clone());
                Ok(path)
            }
            _ => Err(ProviderError::NotInstalled),
        }
    }
}

async fn detect() -> InstallationInfo {
    // `cursor-agent` first: `agent` is a generic name another tool could use.
    ao_detect::detect(
        &["cursor-agent".to_string(), "agent".to_string()],
        EXTRA_DIRS,
        &["--version"],
    )
    .await
}

pub fn capability_profile() -> CapabilityProfile {
    CapabilityProfile {
        managed: Capabilities {
            // Implemented from the ACP spec; not yet run against a real Cursor CLI.
            launch: Experimental,
            attach: Unsupported,
            // ACP `session/list` / `session/load` exist but are not used yet.
            list_sessions: Unsupported,
            stop: Supported,
            send_prompt: Supported,
            structured_events: Supported,
            tool_events: Supported,
            file_events: Supported,
            command_events: Supported,
            permissions: Supported,
            subagents: Unsupported,
            usage: Runtime,
            cost: Runtime,
            model: Runtime,
            context_compaction: Runtime,
            resume: Unsupported,
        },
        external: Capabilities {
            launch: Unsupported,
            attach: Experimental,
            list_sessions: Unsupported,
            stop: Unsupported,
            send_prompt: Unsupported,
            structured_events: Experimental,
            tool_events: Experimental,
            file_events: Experimental,
            command_events: Experimental,
            permissions: Unsupported,
            subagents: Unsupported,
            usage: Unsupported,
            cost: Unsupported,
            model: Unsupported,
            context_compaction: Experimental,
            resume: Unsupported,
        },
        implemented: ImplementedFeatures {
            detection: true,
            managed_sessions: true,
            external_sessions: false,
            integration_setup: false,
        },
        notes: vec![
            "Managed sessions use `agent acp` (Agent Client Protocol, JSON-RPC over stdio), implemented from the official ACP schema and tested against the official ACP example agent; not yet verified against a real Cursor CLI.".into(),
            "Log in once with `agent login` in a terminal; Agent Office then calls Cursor's `cursor_login` for each session.".into(),
            "Values marked runtime (usage, cost, model, compaction) are shown only when the agent sends them.".into(),
            "Cursor's own extension requests (e.g. plans, questions) are declined at once so the turn does not stall; Agent Office cannot show them yet.".into(),
            "External Cursor sessions (hooks) are not implemented: the hook format could not be verified.".into(),
            "ACP has no standard subagent concept; subagents are not shown for Cursor.".into(),
        ],
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

fn rpc_id_string(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl ManagedSession {
    fn emit(&self, events: Vec<AgentEvent>) {
        for event in events {
            self.sink.emit(event);
        }
    }

    async fn dispatch(self: &Arc<Self>, incoming: Incoming, timeout: Duration) {
        match incoming {
            Incoming::Notification { method, params, .. } => {
                if method == "session/update" {
                    let events = self
                        .mapper
                        .lock()
                        .expect("mapper")
                        .update(&params["update"], now_ms());
                    self.emit(events);
                }
                // Other notifications (including agent extensions) carry
                // nothing Agent Office shows.
            }
            Incoming::Request { id, method, params } if method == "session/request_permission" => {
                let request_id = format!("acp-{}", rpc_id_string(&id));
                let events =
                    self.mapper
                        .lock()
                        .expect("mapper")
                        .permission(&request_id, &params, now_ms());
                self.permissions.lock().expect("permissions").insert(
                    request_id.clone(),
                    PendingPermission {
                        rpc_id: id,
                        options: params.get("options").cloned().unwrap_or(Value::Null),
                        tool_call_id: params
                            .pointer("/toolCall/toolCallId")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    },
                );
                self.emit(events);
                // An unanswered request blocks the tool: decline after the timeout.
                let session = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(timeout).await;
                    let pending = session
                        .permissions
                        .lock()
                        .expect("permissions")
                        .remove(&request_id);
                    if let Some(pending) = pending {
                        let option = acp::select_option(
                            &pending.options,
                            &PermissionDecision::Reject { message: None },
                        );
                        let _ = session
                            .rpc
                            .respond(&pending.rpc_id, acp::permission_response(option.as_deref()))
                            .await;
                        let mut mapper = session.mapper.lock().expect("mapper");
                        if let Some(tool) = &pending.tool_call_id {
                            mapper.permission_rejected(tool);
                        }
                        let event = mapper.event(
                            now_ms(),
                            EventKind::PermissionDenied(PermissionResolved {
                                request_id: PermissionRequestId::new(&request_id),
                                resolved_by: PermissionResolver::Timeout,
                                message: Some(format!(
                                    "No answer in Agent Office within {} s",
                                    timeout.as_secs()
                                )),
                            }),
                        );
                        drop(mapper);
                        session.sink.emit(event);
                    }
                });
            }
            Incoming::Request { id, method, .. } => {
                // fs/* and terminal/* are not offered (no client capabilities
                // declared); Cursor's extension requests are not supported.
                let _ = self
                    .rpc
                    .respond_error(&id, -32601, &format!("Method not found: {method}"))
                    .await;
                let event = self.mapper.lock().expect("mapper").event(
                    now_ms(),
                    EventKind::AgentError(AgentError {
                        message: format!("The agent sent `{method}`, which Agent Office cannot handle yet; it was declined."),
                        error_type: Some("unsupported_request".into()),
                        recoverable: true,
                    }),
                );
                self.sink.emit(event);
            }
        }
    }

    /// Answers every pending permission request with `cancelled` (required by
    /// ACP when a turn is cancelled).
    async fn cancel_permissions(&self, reason: &str) {
        let pending: Vec<(String, PendingPermission)> = self
            .permissions
            .lock()
            .expect("permissions")
            .drain()
            .collect();
        for (request_id, p) in pending {
            let _ = self
                .rpc
                .respond(&p.rpc_id, acp::permission_response(None))
                .await;
            let mut mapper = self.mapper.lock().expect("mapper");
            if let Some(tool) = &p.tool_call_id {
                mapper.permission_rejected(tool);
            }
            let event = mapper.event(
                now_ms(),
                EventKind::PermissionDenied(PermissionResolved {
                    request_id: PermissionRequestId::new(&request_id),
                    resolved_by: PermissionResolver::Provider,
                    message: Some(reason.to_owned()),
                }),
            );
            drop(mapper);
            self.sink.emit(event);
        }
    }
}

#[async_trait]
impl ProviderAdapter for CursorAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new(PROVIDER_ID),
            display_name: "Cursor CLI".into(),
            accent_color: "#6e8efb".into(),
            badge: "CU".into(),
            // `cursor-agent` first: `agent` is a generic name another tool could use.
            executable_names: vec!["cursor-agent".into(), "agent".into()],
            homepage: Some("https://cursor.com/docs/cli/overview".into()),
            simulated: false,
        }
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile()
    }

    async fn detect_installation(&self) -> InstallationInfo {
        let info = match &self.inner.options.executable {
            Some(exe) => ao_detect::detect(&[exe.display().to_string()], &[], &["--version"]).await,
            None => detect().await,
        };
        if let (true, Some(path)) = (info.installed, &info.executable_path) {
            *self.inner.executable.lock().expect("exe lock") = Some(PathBuf::from(path));
        }
        info
    }

    fn configure(&self, settings: &ProviderSettings) {
        *self.inner.settings.write().expect("settings lock") = settings.clone();
    }

    async fn launch_session(
        &self,
        request: LaunchRequest,
        ctx: AdapterContext,
    ) -> Result<SessionHandle, ProviderError> {
        let cwd = PathBuf::from(request.cwd.trim());
        if request.cwd.trim().is_empty() || !cwd.is_dir() {
            return Err(ProviderError::Other(format!(
                "Project folder `{}` does not exist.",
                request.cwd
            )));
        }
        let exe = self.inner.executable().await?;
        let (process, mut lines) =
            ManagedProcess::spawn(SpawnSpec::new(&exe).arg("acp").cwd(&cwd))?;
        let rpc = RpcClient::new(process.clone());
        let session = Arc::new(ManagedSession {
            rpc: rpc.clone(),
            sink: ctx.sink.clone(),
            mapper: Mutex::new(Mapper::new(String::new())),
            turn_active: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            permissions: Mutex::new(HashMap::new()),
        });
        let timeout = self.permission_timeout();
        let session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        // Pump: responses complete requests; notifications and requests are dispatched.
        let pump_session = session.clone();
        let pump_id = session_id.clone();
        let inner = self.inner.clone();
        let sink = ctx.sink.clone();
        let stderr: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
        let pump_stderr = stderr.clone();
        tokio::spawn(async move {
            while let Some(event) = lines.recv().await {
                match event {
                    ProcessEvent::Line {
                        stream: Stream::Stdout,
                        line,
                    } => {
                        if let Some(incoming) = pump_session.rpc.handle_line(&line) {
                            pump_session.dispatch(incoming, timeout).await;
                        }
                    }
                    ProcessEvent::Line {
                        stream: Stream::Stderr,
                        line,
                    } => {
                        tracing::debug!(target: "provider::cursor", "stderr: {line}");
                        let mut tail = pump_stderr.lock().expect("stderr");
                        if tail.len() == 20 {
                            tail.pop_front();
                        }
                        tail.push_back(line);
                    }
                    ProcessEvent::Exited(info) => {
                        pump_session.rpc.fail_all();
                        let Some(id) = pump_id.lock().expect("id").clone() else {
                            break;
                        };
                        inner.state.lock().expect("state lock").managed.remove(&id);
                        pump_session
                            .permissions
                            .lock()
                            .expect("permissions")
                            .clear();
                        let stopped = info.killed || pump_session.stopping.load(Ordering::SeqCst);
                        if !info.success && !stopped {
                            let detail = pump_stderr
                                .lock()
                                .expect("stderr")
                                .iter()
                                .rev()
                                .find(|l| !l.trim().is_empty())
                                .cloned();
                            sink.emit(process_event(
                                &id,
                                EventKind::AgentError(AgentError {
                                    message: detail.unwrap_or_else(|| {
                                        format!("Cursor exited with code {:?}", info.code)
                                    }),
                                    error_type: Some("process_exit".into()),
                                    recoverable: false,
                                }),
                            ));
                        }
                        let reason = if stopped {
                            "stopped by Agent Office"
                        } else if info.success {
                            "finished"
                        } else {
                            "exited with an error"
                        };
                        sink.emit(process_event(
                            &id,
                            EventKind::SessionEnded(SessionEnded {
                                reason: Some(reason.into()),
                                exit_code: info.code,
                            }),
                        ));
                        break;
                    }
                }
            }
        });

        let stderr_hint = |stderr: &Arc<Mutex<VecDeque<String>>>| {
            stderr
                .lock()
                .expect("stderr")
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(|l| format!(" — {l}"))
                .unwrap_or_default()
        };
        let fail = |message: String| {
            process.kill_tree();
            ProviderError::Other(message)
        };

        let init = rpc
            .request(
                "initialize",
                json!({
                    "protocolVersion": acp::PROTOCOL_VERSION,
                    // Agent Office offers no file system or terminal to the
                    // agent: it keeps using its own tools.
                    "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false },
                    "clientInfo": { "name": "agent-office", "title": "Agent Office", "version": env!("CARGO_PKG_VERSION") }
                }),
                RPC_TIMEOUT,
            )
            .await;
        let init = match init {
            Ok(init) => init,
            Err(e) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
                return Err(fail(format!(
                    "`{} acp` did not start: {e}{}",
                    exe.display(),
                    stderr_hint(&stderr)
                )));
            }
        };
        let version = init.get("protocolVersion").and_then(Value::as_u64);
        if version != Some(acp::PROTOCOL_VERSION) {
            return Err(fail(format!(
                "The agent speaks ACP protocol version {version:?}; Agent Office supports version {}.",
                acp::PROTOCOL_VERSION
            )));
        }
        let auth_method = init
            .get("authMethods")
            .and_then(Value::as_array)
            .and_then(|methods| {
                methods
                    .iter()
                    .find(|m| is_agent_auth(m.get("type").and_then(Value::as_str)))
                    .and_then(|m| m.get("id").and_then(Value::as_str))
                    .map(str::to_owned)
            });
        // Cursor is reported to need an explicit `authenticate` with
        // `cursor_login` before its sessions work (it answers at once when the
        // user already ran `agent login`). Other agents authenticate only when
        // `session/new` asks for it, as ACP describes.
        let eager_auth = auth_method.as_deref() == Some(CURSOR_LOGIN);
        let mut auth_error = None;
        if eager_auth {
            let params = json!({ "methodId": CURSOR_LOGIN });
            if let Err(e) = rpc.request("authenticate", params, AUTH_TIMEOUT).await {
                tracing::warn!(target: "provider::cursor", %e, "cursor_login failed");
                auth_error = Some(e);
            }
        }
        let login_hint = |auth_error: &Option<RpcError>| match auth_error {
            Some(e) => format!(
                " — Cursor did not accept the login ({e}); run `agent login` in a terminal first"
            ),
            None => String::new(),
        };

        let new_session = json!({ "cwd": cwd.display().to_string(), "mcpServers": [] });
        let created = match rpc
            .request("session/new", new_session.clone(), RPC_TIMEOUT)
            .await
        {
            Ok(created) => created,
            Err(RpcError::Server { code, message }) if auth_method.is_some() && !eager_auth => {
                // Most likely "authentication required" (-32000): log in, retry once.
                let method = auth_method.clone().unwrap_or_default();
                tracing::info!(target: "provider::cursor", code, %message, %method, "session/new failed; authenticating");
                rpc.request("authenticate", json!({ "methodId": method }), AUTH_TIMEOUT)
                    .await
                    .map_err(|e| {
                        fail(format!(
                            "Cursor needs you to log in (run `agent login` in a terminal): {e}"
                        ))
                    })?;
                rpc.request("session/new", new_session, RPC_TIMEOUT)
                    .await
                    .map_err(|e| fail(format!("Cursor could not start a session: {e}")))?
            }
            Err(e) => {
                return Err(fail(format!(
                    "Cursor could not start a session: {e}{}{}",
                    login_hint(&auth_error),
                    stderr_hint(&stderr)
                )))
            }
        };
        let sid = created
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| fail("`session/new` returned no sessionId".into()))?
            .to_owned();

        // The session exists from here on: announce it, then apply the
        // requested model and mode (the agent may already send updates).
        *session.mapper.lock().expect("mapper") =
            Mapper::new(sid.clone()).with_cwd(cwd.display().to_string());
        *session_id.lock().expect("id") = Some(sid.clone());
        self.inner
            .state
            .lock()
            .expect("state lock")
            .managed
            .insert(sid.clone(), session.clone());
        ctx.sink.emit(process_event(
            &sid,
            EventKind::SessionStarted(SessionInfo {
                mode: Some(SessionMode::Managed),
                cwd: Some(cwd.display().to_string()),
                model: acp::current_model(&created),
                title: request.name.clone().filter(|n| !n.trim().is_empty()),
                reason: Some("launched by Agent Office".into()),
                permission_mode: created
                    .pointer("/modes/currentModeId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                pid: Some(process.pid()),
                ..Default::default()
            }),
        ));
        let exited =
            || ProviderError::Other("Cursor exited while the session was starting.".into());
        let warn = |kind: &str, message: String| {
            ctx.sink.emit(process_event(
                &sid,
                EventKind::AgentError(AgentError {
                    message,
                    error_type: Some(kind.into()),
                    recoverable: true,
                }),
            ))
        };
        let updated = |info: SessionInfo| {
            ctx.sink
                .emit(process_event(&sid, EventKind::SessionUpdated(info)))
        };

        // Model: the `model` session config option (ACP 1.x) or, when only
        // that is offered, the older `models` state with `session/set_model`.
        if let Some(wanted) = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            let switched = match acp::find_model(&created, wanted) {
                Some(acp::ModelChoice::ConfigOption {
                    config_id,
                    value,
                    name,
                }) => rpc
                    .request(
                        "session/set_config_option",
                        json!({ "sessionId": sid, "configId": config_id, "value": value }),
                        RPC_TIMEOUT,
                    )
                    .await
                    .map(|_| name),
                Some(acp::ModelChoice::Legacy { model_id, name }) => rpc
                    .request(
                        "session/set_model",
                        json!({ "sessionId": sid, "modelId": model_id }),
                        RPC_TIMEOUT,
                    )
                    .await
                    .map(|_| name),
                None => Err(RpcError::Io("it is not offered here".into())),
            };
            match switched {
                Ok(name) => updated(SessionInfo {
                    model: Some(name),
                    ..Default::default()
                }),
                Err(RpcError::Closed) => return Err(exited()),
                Err(why) => warn(
                    "model",
                    format!(
                        "Cursor did not switch to model `{wanted}` ({why}); its default is used."
                    ),
                ),
            }
        }
        // Mode (e.g. `plan`): only an id the agent advertised.
        if let Some(wanted) = request.permission_mode.as_deref().filter(|m| !m.is_empty()) {
            let offered = created
                .pointer("/modes/availableModes")
                .and_then(Value::as_array)
                .is_some_and(|m| {
                    m.iter()
                        .any(|m| m.get("id").and_then(Value::as_str) == Some(wanted))
                });
            let set = if offered {
                rpc.request(
                    "session/set_mode",
                    json!({ "sessionId": sid, "modeId": wanted }),
                    RPC_TIMEOUT,
                )
                .await
            } else {
                Err(RpcError::Io(
                    "it is not offered by this Cursor version".into(),
                ))
            };
            match set {
                Ok(_) => updated(SessionInfo {
                    permission_mode: Some(wanted.to_owned()),
                    ..Default::default()
                }),
                Err(RpcError::Closed) => return Err(exited()),
                Err(why) => warn(
                    "mode",
                    format!(
                        "Cursor did not switch to mode `{wanted}` ({why}); its default is used."
                    ),
                ),
            }
        }

        let handle = SessionHandle {
            provider: ProviderId::new(PROVIDER_ID),
            session_id: SessionId::new(&sid),
            mode: SessionMode::Managed,
            pid: Some(process.pid()),
        };
        if let Some(prompt) = request.prompt.as_deref().filter(|p| !p.trim().is_empty()) {
            self.send_prompt(&handle.session_id, prompt).await?;
        }
        Ok(handle)
    }

    async fn send_prompt(&self, session_id: &SessionId, prompt: &str) -> Result<(), ProviderError> {
        let session = self.managed(session_id.as_str()).ok_or_else(|| {
            ProviderError::Other(
                "Cursor sessions started outside Agent Office cannot receive prompts.".into(),
            )
        })?;
        if session.turn_active.swap(true, Ordering::SeqCst) {
            return Err(ProviderError::Other(
                "Cursor is still working on the previous prompt; wait for it or stop the session."
                    .into(),
            ));
        }
        {
            let mapper = session.mapper.lock().expect("mapper");
            session.sink.emit(mapper.event(
                now_ms(),
                EventKind::PromptSubmitted(PromptSubmitted {
                    text: Some(prompt.to_owned()),
                }),
            ));
            session.sink.emit(mapper.event(
                now_ms(),
                EventKind::AgentThinking(TextNote {
                    text: Some("Working".into()),
                }),
            ));
        }
        // `session/prompt` answers when the whole turn is over.
        let params = json!({ "sessionId": session_id.as_str(), "prompt": [{ "type": "text", "text": prompt }] });
        let turn = session.clone();
        tokio::spawn(async move {
            let result = turn.rpc.request_until("session/prompt", params, None).await;
            let events = {
                let mut mapper = turn.mapper.lock().expect("mapper");
                match &result {
                    Ok(value) => mapper.turn_ended(Ok(value), now_ms()),
                    Err(RpcError::Closed) => Vec::new(),
                    Err(e) => {
                        mapper.turn_ended(Err(&format!("Cursor reported an error: {e}")), now_ms())
                    }
                }
            };
            // Free the turn first: whoever sees the turn end may prompt again.
            turn.turn_active.store(false, Ordering::SeqCst);
            turn.emit(events);
        });
        Ok(())
    }

    async fn resolve_permission(
        &self,
        session_id: &SessionId,
        request_id: &PermissionRequestId,
        decision: PermissionDecision,
    ) -> Result<(), ProviderError> {
        let session = self
            .managed(session_id.as_str())
            .ok_or_else(|| ProviderError::SessionNotFound(session_id.to_string()))?;
        let pending = session
            .permissions
            .lock()
            .expect("permissions")
            .remove(request_id.as_str())
            .ok_or_else(|| {
                ProviderError::Other(
                    "This request is no longer waiting (answered, cancelled or timed out).".into(),
                )
            })?;
        let option = acp::select_option(&pending.options, &decision);
        session
            .rpc
            .respond(&pending.rpc_id, acp::permission_response(option.as_deref()))
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        let resolved = PermissionResolved {
            request_id: request_id.clone(),
            resolved_by: PermissionResolver::App,
            message: match &decision {
                PermissionDecision::Reject { message } => message.clone(),
                PermissionDecision::Approve { .. } => None,
            },
        };
        let approved = matches!(decision, PermissionDecision::Approve { .. }) && option.is_some();
        let event = {
            let mut mapper = session.mapper.lock().expect("mapper");
            if let (false, Some(tool)) = (approved, &pending.tool_call_id) {
                mapper.permission_rejected(tool);
            }
            mapper.event(
                now_ms(),
                if approved {
                    EventKind::PermissionApproved(resolved)
                } else {
                    EventKind::PermissionDenied(resolved)
                },
            )
        };
        session.sink.emit(event);
        Ok(())
    }

    async fn stop_session(
        &self,
        session_id: &SessionId,
        mode: StopMode,
    ) -> Result<(), ProviderError> {
        let Some(session) = self.managed(session_id.as_str()) else {
            return Err(ProviderError::Unsupported { capability: "stop" });
        };
        let process = session.rpc.process().clone();
        session.stopping.store(true, Ordering::SeqCst);
        match mode {
            StopMode::Force => process.kill_tree(),
            StopMode::Graceful => {
                if session.turn_active.load(Ordering::SeqCst) {
                    let _ = session
                        .rpc
                        .notify(
                            "session/cancel",
                            Some(json!({ "sessionId": session_id.as_str() })),
                        )
                        .await;
                    session
                        .cancel_permissions("Cancelled by Agent Office")
                        .await;
                    // Give the turn a moment to end with `cancelled`.
                    for _ in 0..50 {
                        if !session.turn_active.load(Ordering::SeqCst) {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
                process.stop(Duration::from_secs(10)).await;
            }
        }
        Ok(())
    }
}

/// ACP auth methods without a `type` are `agent` methods, the only kind
/// handled through `authenticate` (others need a terminal or env variable).
fn is_agent_auth(kind: Option<&str>) -> bool {
    matches!(kind, None | Some("agent"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_documented_capabilities() {
        let caps = capability_profile();
        assert_eq!(caps.managed.subagents, Unsupported);
        assert_eq!(
            caps.managed.launch, Experimental,
            "not verified against a real Cursor CLI"
        );
        assert_eq!(caps.managed.usage, Runtime);
        assert_eq!(
            caps.managed.resume, Unsupported,
            "session/load is not used yet"
        );
        assert_eq!(caps.managed.list_sessions, Unsupported);
        assert!(caps.implemented.managed_sessions);
        assert!(!caps.implemented.external_sessions);
    }

    #[tokio::test]
    async fn external_sessions_cannot_be_controlled() {
        let adapter = CursorAdapter::new();
        assert!(adapter.send_prompt(&"s".into(), "hi").await.is_err());
        assert!(adapter
            .stop_session(&"s".into(), StopMode::Force)
            .await
            .is_err());
    }
}
