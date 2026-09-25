//! OpenAI Codex CLI adapter.
//!
//! * **Managed sessions**: `codex app-server` (JSON-RPC over stdio, marked
//!   experimental by Codex) — one process and one root thread per session;
//!   approvals are answered from Agent Office.
//! * **External sessions**: user-level hooks in `~/.codex/hooks.json`, which
//!   the user must trust once in Codex (`/hooks`).
//!
//! See docs/PROVIDER_CAPABILITIES.md §4 for what was verified and how.

pub mod appserver;
pub mod hooks;
pub mod settings;
pub mod tools;
pub mod trust;

use ao_config::JsonConfigFile;
use ao_core::capabilities::{Capabilities, CapabilityProfile, ImplementedFeatures, Support::*};
use ao_core::event::*;
use ao_core::ids::{PermissionRequestId, ProviderId, SessionId};
use ao_core::provider::{
    AdapterContext, EventSink, HookCall, HookReply, InstallationInfo, IntegrationState,
    IntegrationStatus, LaunchRequest, PermissionDecision, ProviderAdapter, ProviderDescriptor,
    ProviderError, ProviderSettings, RelayCommand, SessionHandle, StopMode,
};
use ao_core::time::now_ms;
use ao_jsonrpc::{Incoming, RpcClient};
use ao_process::{ManagedProcess, ProcessEvent, SpawnSpec, Stream};
use appserver::{ApprovalKind, Mapper};
use async_trait::async_trait;
use serde_json::{json, Value};
use settings::{HookPlan, ShellKind};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::oneshot;

pub const PROVIDER_ID: &str = "codex";

/// app-server minor version the protocol mapping was verified against.
pub const TESTED_VERSION: &str = "0.156";

/// Approval policies accepted by `thread/start` (`AskForApproval`).
pub const APPROVAL_POLICIES: &[&str] = &["untrusted", "on-request", "never"];

const EXTRA_DIRS: &[&str] = if cfg!(windows) {
    &["%APPDATA%\\npm", "%USERPROFILE%\\.local\\bin"]
} else {
    &["~/.local/bin", "/usr/local/bin", "/opt/homebrew/bin"]
};

const RPC_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default)]
pub struct CodexOptions {
    /// Use this executable instead of detecting `codex` (tests).
    pub executable: Option<PathBuf>,
    /// Codex home (`CODEX_HOME`, default `~/.codex`). When set, sessions
    /// launched by Agent Office and the trust check use it too.
    pub config_dir: Option<PathBuf>,
}

struct PendingApproval {
    rpc_id: Value,
    kind: ApprovalKind,
    thread: String,
}

struct ManagedSession {
    rpc: Arc<RpcClient>,
    sink: EventSink,
    mapper: Mutex<Option<Mapper>>,
    /// Messages that arrived before `thread/start` answered.
    early: Mutex<Vec<Incoming>>,
    approvals: Mutex<HashMap<String, PendingApproval>>,
}

struct PendingHook {
    session_id: String,
    tx: oneshot::Sender<PermissionDecision>,
}

#[derive(Default)]
struct State {
    managed: HashMap<String, Arc<ManagedSession>>,
    pending_hooks: HashMap<String, PendingHook>,
}

struct Inner {
    options: CodexOptions,
    settings: RwLock<ProviderSettings>,
    state: Mutex<State>,
    executable: Mutex<Option<PathBuf>>,
}

pub struct CodexAdapter {
    inner: Arc<Inner>,
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self::with_options(CodexOptions::default())
    }

    pub fn with_options(options: CodexOptions) -> Self {
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

    fn hooks_file(&self) -> Option<JsonConfigFile> {
        match &self.inner.options.config_dir {
            Some(dir) => Some(JsonConfigFile::new(dir.join("hooks.json"))),
            None => settings::default_location(),
        }
    }

    fn plan(&self, relay: &RelayCommand) -> HookPlan {
        let settings = self.settings();
        HookPlan {
            relay: relay.clone(),
            answer_permissions: settings.answer_permissions_from_app,
            permission_wait_secs: settings.permission_timeout_secs as u64 + 30,
            shell: ShellKind::native(),
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

    /// Adds Codex's own trust verdict to an `Installed` file status.
    async fn with_trust(
        &self,
        mut status: IntegrationStatus,
        file: &JsonConfigFile,
    ) -> IntegrationStatus {
        if status.state != IntegrationState::Installed {
            return status;
        }
        let exe = match self.inner.executable().await {
            Ok(exe) => exe,
            Err(_) => {
                status
                    .details
                    .push("Codex is not installed, so its trust status cannot be read.".into());
                return status;
            }
        };
        match trust::query(&exe, self.inner.options.config_dir.as_ref(), &file.path).await {
            Ok(entries) if entries.is_empty() => {
                status.details.push("Codex did not list the Agent Office hooks (is the `hooks` feature turned off?).".into());
                status.state = IntegrationState::NeedsUserAction;
            }
            Ok(entries) => {
                if let Some(problem) = trust::verdict(&entries) {
                    status.details.insert(0, problem);
                    status.state = IntegrationState::NeedsUserAction;
                } else {
                    status.details.insert(0, "Trusted in Codex.".into());
                }
            }
            Err(e) => status
                .details
                .push(format!("Could not read the trust status from Codex: {e}")),
        }
        status
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
    ao_detect::detect(&["codex".to_string()], EXTRA_DIRS, &["--version"]).await
}

pub fn capability_profile() -> CapabilityProfile {
    CapabilityProfile {
        managed: Capabilities {
            // app-server is labelled [experimental] by the Codex CLI.
            launch: Experimental,
            attach: Unsupported,
            list_sessions: Supported,
            stop: Supported,
            send_prompt: Supported,
            structured_events: Supported,
            tool_events: Supported,
            file_events: Supported,
            command_events: Supported,
            permissions: Supported,
            subagents: Supported,
            usage: Supported,
            cost: Unsupported,
            model: Supported,
            context_compaction: Supported,
            resume: Supported,
        },
        external: Capabilities {
            launch: Unsupported,
            // Hooks are stable, but each hook must be trusted by the user via /hooks.
            attach: Partial,
            list_sessions: Unsupported,
            stop: Unsupported,
            send_prompt: Unsupported,
            structured_events: Supported,
            tool_events: Supported,
            file_events: Partial,
            command_events: Supported,
            permissions: Partial,
            subagents: Supported,
            usage: Unsupported,
            cost: Unsupported,
            model: Supported,
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
            "Managed sessions use `codex app-server` (JSON-RPC v2), marked experimental by Codex.".into(),
            "External sessions need hooks in %USERPROFILE%\\.codex\\hooks.json trusted once via /hooks.".into(),
            "Codex reports token usage but no cost.".into(),
            "External Codex sessions cannot be listed or stopped from Agent Office.".into(),
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

/// `codex_office/0.156.1 (Ubuntu …) …` → `0.156.1`.
pub fn version_from_user_agent(user_agent: &str) -> Option<&str> {
    let first = user_agent.split_whitespace().next()?;
    let version = first.rsplit('/').next()?;
    version
        .chars()
        .next()
        .filter(char::is_ascii_digit)
        .map(|_| version)
}

impl ManagedSession {
    /// Handles one message from the server (after `thread/start`).
    async fn dispatch(self: &Arc<Self>, incoming: Incoming, timeout: Duration) {
        let sink = &self.sink;
        match incoming {
            Incoming::Notification {
                method,
                params,
                emitted_at_ms,
            } => {
                let at = emitted_at_ms.unwrap_or_else(now_ms);
                if method == "serverRequest/resolved" {
                    let id = format!(
                        "codex-{}",
                        appserver::rpc_id_string(params.get("requestId").unwrap_or(&Value::Null))
                    );
                    // Still pending here = Codex withdrew it (e.g. the turn was interrupted).
                    if let Some(pending) = self.approvals.lock().expect("approvals").remove(&id) {
                        let mapper = self.mapper.lock().expect("mapper");
                        if let Some(mapper) = mapper.as_ref() {
                            sink.emit(mapper.event(
                                &pending.thread,
                                at,
                                EventKind::PermissionDenied(PermissionResolved {
                                    request_id: PermissionRequestId::new(&id),
                                    resolved_by: PermissionResolver::Provider,
                                    message: Some("Withdrawn by Codex".into()),
                                }),
                            ));
                        }
                    }
                    return;
                }
                let events = match self.mapper.lock().expect("mapper").as_mut() {
                    Some(mapper) => mapper.notification(&method, &params, at),
                    None => Vec::new(),
                };
                for event in events {
                    sink.emit(event);
                }
            }
            Incoming::Request { id, method, params } => {
                let approval = self
                    .mapper
                    .lock()
                    .expect("mapper")
                    .as_mut()
                    .and_then(|m| m.approval(&method, &id, &params, now_ms()));
                let Some(approval) = approval else {
                    // e.g. item/tool/requestUserInput, MCP elicitations, auth refresh.
                    let _ = self
                        .rpc
                        .respond_error(
                            &id,
                            -32601,
                            &format!("Agent Office does not support `{method}`"),
                        )
                        .await;
                    return;
                };
                let request_id = approval.request_id.clone();
                self.approvals.lock().expect("approvals").insert(
                    request_id.clone(),
                    PendingApproval {
                        rpc_id: id,
                        kind: approval.kind,
                        thread: approval.thread,
                    },
                );
                sink.emit(approval.event);
                // Nobody answered in time → decline (the agent is told, the turn goes on).
                let session = self.clone();
                let sink = sink.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(timeout).await;
                    let pending = session
                        .approvals
                        .lock()
                        .expect("approvals")
                        .remove(&request_id);
                    if let Some(pending) = pending {
                        let _ = session
                            .rpc
                            .respond(&pending.rpc_id, pending.kind.timeout_response())
                            .await;
                        let event = session.mapper.lock().expect("mapper").as_ref().map(|m| {
                            m.event(
                                &pending.thread,
                                now_ms(),
                                EventKind::PermissionDenied(PermissionResolved {
                                    request_id: PermissionRequestId::new(&request_id),
                                    resolved_by: PermissionResolver::Timeout,
                                    message: Some(format!(
                                        "No answer in Agent Office within {} s",
                                        timeout.as_secs()
                                    )),
                                }),
                            )
                        });
                        if let Some(event) = event {
                            sink.emit(event);
                        }
                    }
                });
            }
        }
    }
}

#[async_trait]
impl ProviderAdapter for CodexAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new(PROVIDER_ID),
            display_name: "Codex CLI".into(),
            accent_color: "#10a37f".into(),
            badge: "CX".into(),
            executable_names: vec!["codex".into()],
            homepage: Some("https://github.com/openai/codex".into()),
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

    async fn handle_hook(&self, call: HookCall, ctx: AdapterContext) -> HookReply {
        let Some(session_id) = hooks::session_id(&call.payload).map(str::to_owned) else {
            return HookReply::default();
        };
        if self.managed(&session_id).is_some() {
            // app-server sessions also run the user's hooks; the protocol
            // already reports everything, and approvals go through it too.
            return HookReply::default();
        }
        let mut hook_ctx = hooks::HookContext {
            received_at_ms: call.received_at_ms,
            permission: None,
        };
        if hooks::event_name(&call.payload) != settings::PERMISSION_EVENT {
            for event in hooks::map_hook(&call.payload, &hook_ctx) {
                ctx.sink.emit(event);
            }
            return HookReply::default();
        }

        // PermissionRequest: Codex sends no id, so Agent Office assigns one.
        let request_id = format!("perm-{}", uuid::Uuid::new_v4().simple());
        let settings = self.settings();
        let can_answer = settings.answer_permissions_from_app;
        hook_ctx.permission = Some((request_id.clone(), can_answer));
        for event in hooks::map_hook(&call.payload, &hook_ctx) {
            ctx.sink.emit(event);
        }
        if !can_answer {
            return HookReply::default();
        }
        let (tx, rx) = oneshot::channel();
        self.inner
            .state
            .lock()
            .expect("state lock")
            .pending_hooks
            .insert(
                request_id.clone(),
                PendingHook {
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
            .pending_hooks
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
                .and_then(Value::as_str)
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
            Ok(Err(_)) | Err(_) => {
                // Hand the decision back to Codex's own prompt.
                emit(EventKind::PermissionExpired(resolved(
                    PermissionResolver::Timeout,
                    Some("No answer in Agent Office — answer in Codex".into()),
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
        if let Some(session) = self.managed(session_id.as_str()) {
            let pending = session
                .approvals
                .lock()
                .expect("approvals")
                .remove(request_id.as_str());
            let pending = pending.ok_or_else(|| {
                ProviderError::Other(
                    "This approval is no longer waiting (answered, withdrawn or timed out).".into(),
                )
            })?;
            session
                .rpc
                .respond(&pending.rpc_id, pending.kind.response(&decision))
                .await
                .map_err(|e| ProviderError::Other(e.to_string()))?;
            let kind = match &decision {
                PermissionDecision::Approve { .. } => {
                    EventKind::PermissionApproved(PermissionResolved {
                        request_id: request_id.clone(),
                        resolved_by: PermissionResolver::App,
                        message: None,
                    })
                }
                PermissionDecision::Reject { message } => {
                    EventKind::PermissionDenied(PermissionResolved {
                        request_id: request_id.clone(),
                        resolved_by: PermissionResolver::App,
                        message: message.clone(),
                    })
                }
            };
            let event = session
                .mapper
                .lock()
                .expect("mapper")
                .as_ref()
                .map(|m| m.event(&pending.thread, now_ms(), kind));
            if let Some(event) = event {
                session.sink.emit(event);
            }
            return Ok(());
        }
        let pending = {
            let mut state = self.inner.state.lock().expect("state lock");
            match state.pending_hooks.get(request_id.as_str()) {
                Some(p) if p.session_id == session_id.as_str() => {
                    state.pending_hooks.remove(request_id.as_str())
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
            .map_err(|_| ProviderError::Other("Codex stopped waiting for this answer.".into()))
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
        let mut spec = SpawnSpec::new(&exe).arg("app-server").cwd(&cwd);
        if let Some(home) = &self.inner.options.config_dir {
            spec = spec.env("CODEX_HOME", home.as_os_str());
        }
        let (process, mut lines) = ManagedProcess::spawn(spec)?;
        process.set_label("Codex CLI · starting");
        let rpc = RpcClient::new(process.clone());
        let session = Arc::new(ManagedSession {
            rpc: rpc.clone(),
            sink: ctx.sink.clone(),
            mapper: Mutex::new(None),
            early: Mutex::new(Vec::new()),
            approvals: Mutex::new(HashMap::new()),
        });
        let timeout = Duration::from_secs(self.settings().permission_timeout_secs.max(5) as u64);

        // Pump: responses complete requests; everything else is mapped.
        let pump_session = session.clone();
        let sink = ctx.sink.clone();
        let inner = self.inner.clone();
        let (exit_tx, exit_rx) = oneshot::channel::<Option<String>>();
        tokio::spawn(async move {
            let mut stderr_tail: VecDeque<String> = VecDeque::new();
            let mut exit_tx = Some(exit_tx);
            while let Some(event) = lines.recv().await {
                match event {
                    ProcessEvent::Line {
                        stream: Stream::Stdout,
                        line,
                    } => {
                        let Some(incoming) = pump_session.rpc.handle_line(&line) else {
                            continue;
                        };
                        let ready = pump_session.mapper.lock().expect("mapper").is_some();
                        if ready {
                            pump_session.dispatch(incoming, timeout).await;
                        } else {
                            pump_session.early.lock().expect("early").push(incoming);
                        }
                    }
                    ProcessEvent::Line {
                        stream: Stream::Stderr,
                        line,
                    } => {
                        tracing::debug!(target: "provider::codex", "stderr: {line}");
                        if stderr_tail.len() == 20 {
                            stderr_tail.pop_front();
                        }
                        stderr_tail.push_back(line);
                    }
                    ProcessEvent::Exited(info) => {
                        pump_session.rpc.fail_all();
                        let detail = stderr_tail
                            .iter()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .cloned();
                        let root = pump_session
                            .mapper
                            .lock()
                            .expect("mapper")
                            .as_ref()
                            .map(|m| m.root().to_owned());
                        let Some(root) = root else {
                            // Died before the thread existed: launch reports it.
                            if let Some(tx) = exit_tx.take() {
                                let _ = tx.send(detail);
                            }
                            break;
                        };
                        inner
                            .state
                            .lock()
                            .expect("state lock")
                            .managed
                            .remove(&root);
                        pump_session.approvals.lock().expect("approvals").clear();
                        if !info.success && !info.killed {
                            sink.emit(process_event(
                                &root,
                                EventKind::AgentError(AgentError {
                                    message: detail
                                        .unwrap_or_else(|| format!("Codex {}", info.describe())),
                                    error_type: Some("process_exit".into()),
                                    recoverable: false,
                                }),
                            ));
                        }
                        let reason = info.describe();
                        sink.emit(process_event(
                            &root,
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

        let fail = |message: String| {
            process.kill_tree();
            ProviderError::Other(message)
        };
        let init = rpc
            .request(
                "initialize",
                json!({
                    "clientInfo": { "name": "agent_office", "title": "Agent Office", "version": env!("CARGO_PKG_VERSION") },
                    "capabilities": null
                }),
                RPC_TIMEOUT,
            )
            .await;
        let init = match init {
            Ok(init) => init,
            Err(e) => {
                let detail = tokio::time::timeout(Duration::from_secs(2), exit_rx)
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten();
                return Err(fail(format!(
                    "`codex app-server` did not start: {e}{}",
                    detail.map(|d| format!(" — {d}")).unwrap_or_default()
                )));
            }
        };
        let version = init
            .get("userAgent")
            .and_then(Value::as_str)
            .and_then(version_from_user_agent)
            .map(str::to_owned);
        rpc.notify("initialized", None)
            .await
            .map_err(|e| fail(e.to_string()))?;

        let mut params = json!({ "cwd": cwd.display().to_string() });
        if let Some(model) = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            params["model"] = json!(model);
        }
        let policy = request
            .permission_mode
            .as_deref()
            .filter(|p| APPROVAL_POLICIES.contains(p));
        if let Some(policy) = policy {
            params["approvalPolicy"] = json!(policy);
        }
        // Restart: `thread/resume` continues the stored thread under its id;
        // `excludeTurns` skips sending back the whole history.
        let resuming = request
            .resume_session_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        if let Some(id) = &resuming {
            if self.managed(id).is_some() {
                return Err(fail(
                    "This Codex thread is still running; stop it before restarting it.".into(),
                ));
            }
            params["threadId"] = json!(id);
            params["excludeTurns"] = json!(true);
        }
        let method = if resuming.is_some() {
            "thread/resume"
        } else {
            "thread/start"
        };
        let started = rpc
            .request(method, params, RPC_TIMEOUT)
            .await
            .map_err(|e| {
                fail(if resuming.is_some() {
                    format!("Codex could not resume the thread: {e}")
                } else {
                    format!("Codex could not start a thread: {e}")
                })
            })?;
        let thread_id = started
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| fail(format!("`{method}` returned no thread id")))?
            .to_owned();
        if resuming.as_deref().is_some_and(|id| id != thread_id) {
            return Err(fail(format!(
                "Codex resumed thread {thread_id} instead of the one asked for"
            )));
        }
        process.set_label(format!("Codex CLI · thread {thread_id}"));
        let model = started
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned);

        // Register before any turn starts, so hooks of this thread are recognised.
        self.inner
            .state
            .lock()
            .expect("state lock")
            .managed
            .insert(thread_id.clone(), session.clone());
        ctx.sink.emit(process_event(
            &thread_id,
            EventKind::SessionStarted(SessionInfo {
                mode: Some(SessionMode::Managed),
                cwd: Some(cwd.display().to_string()),
                model,
                title: request.name.clone().filter(|n| !n.trim().is_empty()),
                reason: Some(
                    if resuming.is_some() {
                        "restarted by Agent Office"
                    } else {
                        "launched by Agent Office"
                    }
                    .into(),
                ),
                permission_mode: policy.map(str::to_owned),
                pid: Some(process.pid()),
                ..Default::default()
            }),
        ));
        if let Some(version) = &version {
            if !version.starts_with(&format!("{TESTED_VERSION}.")) {
                ctx.sink.emit(process_event(
                    &thread_id,
                    EventKind::ProviderError(ProviderErrorInfo {
                        component: "app-server".into(),
                        message: format!(
                            "Codex {version} is not the version Agent Office was tested with ({TESTED_VERSION}.x); the app-server protocol is experimental and may have changed."
                        ),
                    }),
                ));
            }
        }
        *session.mapper.lock().expect("mapper") =
            Some(Mapper::new(thread_id.clone()).with_cwd(cwd.display().to_string()));
        let early: Vec<Incoming> = std::mem::take(&mut *session.early.lock().expect("early"));
        for incoming in early {
            session.dispatch(incoming, timeout).await;
        }

        let handle = SessionHandle {
            provider: ProviderId::new(PROVIDER_ID),
            session_id: SessionId::new(&thread_id),
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
                "Codex sessions started outside Agent Office cannot receive prompts.".into(),
            )
        })?;
        let input = json!([{ "type": "text", "text": prompt, "text_elements": [] }]);
        let active = session
            .mapper
            .lock()
            .expect("mapper")
            .as_ref()
            .and_then(|m| m.active_turn.clone());
        if let Some(turn) = active {
            let steered = session
                .rpc
                .request(
                    "turn/steer",
                    json!({ "threadId": session_id.as_str(), "input": input, "expectedTurnId": turn }),
                    RPC_TIMEOUT,
                )
                .await;
            if steered.is_ok() {
                return Ok(());
            }
            // The turn ended meanwhile: start a new one.
        }
        let result = session
            .rpc
            .request(
                "turn/start",
                json!({ "threadId": session_id.as_str(), "input": input }),
                RPC_TIMEOUT,
            )
            .await
            .map_err(|e| ProviderError::Other(format!("Codex did not accept the prompt: {e}")))?;
        if let Some(turn) = result.pointer("/turn/id").and_then(Value::as_str) {
            if let Some(mapper) = session.mapper.lock().expect("mapper").as_mut() {
                mapper.active_turn = Some(turn.to_owned());
            }
        }
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
        match mode {
            StopMode::Force => process.kill_tree(),
            StopMode::Graceful => {
                let active = session
                    .mapper
                    .lock()
                    .expect("mapper")
                    .as_ref()
                    .and_then(|m| m.active_turn.clone());
                if let Some(turn) = active {
                    let _ = session
                        .rpc
                        .request(
                            "turn/interrupt",
                            json!({ "threadId": session_id.as_str(), "turnId": turn }),
                            Duration::from_secs(5),
                        )
                        .await;
                }
                process.stop(Duration::from_secs(10)).await;
            }
        }
        Ok(())
    }

    async fn integration_status(&self, ctx: &AdapterContext) -> IntegrationStatus {
        let Some(file) = self.hooks_file() else {
            return IntegrationStatus::unsupported(
                "Cannot locate the Codex folder (no home directory).",
            );
        };
        let plan = ctx.relay.as_ref().map(|r| self.plan(r));
        let status = settings::status(&file, plan.as_ref());
        self.with_trust(status, &file).await
    }

    async fn install_integration(
        &self,
        ctx: &AdapterContext,
    ) -> Result<IntegrationStatus, ProviderError> {
        let file = self
            .hooks_file()
            .ok_or(ProviderError::Other("no home directory".into()))?;
        let relay = ctx
            .relay
            .as_ref()
            .ok_or(ProviderError::Other("hook relay unavailable".into()))?;
        let status =
            settings::apply(&file, &self.plan(relay), true).map_err(ProviderError::Other)?;
        Ok(self.with_trust(status, &file).await)
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
            .hooks_file()
            .ok_or(ProviderError::Other("no home directory".into()))?;
        let relay = ctx.relay.clone().unwrap_or(RelayCommand {
            program: PathBuf::from("agent-office"),
            prefix_args: vec!["hook".into()],
            data_dir: None,
        });
        settings::apply(&file, &self.plan(&relay), false).map_err(ProviderError::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_documented_capabilities() {
        let caps = capability_profile();
        assert_eq!(caps.managed.launch, Experimental);
        assert_eq!(caps.managed.cost, Unsupported);
        assert_eq!(caps.external.list_sessions, Unsupported);
        assert_eq!(caps.external.attach, Partial);
        assert!(caps.implemented.managed_sessions && caps.implemented.external_sessions);
    }

    #[test]
    fn version_is_read_from_the_user_agent() {
        assert_eq!(
            version_from_user_agent(
                "agent_office/0.156.1 (Ubuntu 24.4.0; x86_64) linux (agent_office; 0.1.0)"
            ),
            Some("0.156.1")
        );
        assert_eq!(version_from_user_agent("weird"), None);
    }

    #[tokio::test]
    async fn external_sessions_cannot_be_controlled() {
        let adapter = CodexAdapter::new();
        assert!(adapter.send_prompt(&"s".into(), "hi").await.is_err());
        assert!(adapter
            .stop_session(&"s".into(), StopMode::Force)
            .await
            .is_err());
    }
}
