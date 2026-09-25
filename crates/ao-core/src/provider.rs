//! The provider adapter contract. Every provider (Claude Code, Codex, Cursor, …)
//! implements [`ProviderAdapter`]. Methods a provider cannot support keep their
//! default implementation, which returns [`ProviderError::Unsupported`].

use crate::capabilities::CapabilityProfile;
use crate::event::{AgentEvent, SessionMode};
use crate::ids::{PermissionRequestId, ProjectId, ProviderId, SessionId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use ts_rs::TS;

/// Static identity of a provider, used by the UI for names and colors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub display_name: String,
    /// CSS color used to tint characters and badges.
    pub accent_color: String,
    /// 1–3 characters shown on the character's badge. Never a trademarked logo.
    pub badge: String,
    /// Executable names probed during detection (without extension).
    pub executable_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub homepage: Option<String>,
    /// True for the demo provider: its data is simulated.
    #[serde(default)]
    pub simulated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstallationInfo {
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub executable_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version: Option<String>,
    /// Raw first line printed by `--version` (for diagnostics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaunchRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project_id: Option<ProjectId>,
    /// Working directory (project folder).
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub permission_mode: Option<String>,
    /// Continue this earlier session instead of starting a new one (Restart).
    /// Only for providers whose managed sessions support `resume`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resume_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionHandle {
    pub provider: ProviderId,
    pub session_id: SessionId,
    pub mode: SessionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum StopMode {
    /// Cancel through the protocol, close stdin, wait for a clean exit.
    Graceful,
    /// Terminate the process tree owned by Agent Office.
    Force,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "decision", rename_all = "camelCase")]
#[ts(export)]
pub enum PermissionDecision {
    Approve {
        /// Approve only this call (`false`) or for the rest of the session (`true`)
        /// when the provider supports it.
        #[serde(default, rename = "forSession")]
        #[ts(rename = "forSession")]
        for_session: bool,
    },
    Reject {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        message: Option<String>,
    },
}

/// A session discovered through an official listing command (e.g. `claude agents --json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExternalSessionInfo {
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub started_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum IntegrationState {
    NotInstalled,
    Installed,
    /// Our entries exist but are outdated or damaged.
    NeedsRepair,
    /// Installed, but the user must do something in the provider (e.g. trust hooks in Codex).
    NeedsUserAction,
    /// The provider has no integration to install (or it is not implemented yet).
    Unsupported,
    /// The config could not be read (e.g. invalid JSON). Nothing is changed.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IntegrationStatus {
    pub state: IntegrationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub config_path: Option<String>,
    pub details: Vec<String>,
}

impl IntegrationStatus {
    pub fn unsupported(detail: impl Into<String>) -> Self {
        Self {
            state: IntegrationState::Unsupported,
            config_path: None,
            details: vec![detail.into()],
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("capability `{capability}` is not supported by this provider")]
    Unsupported { capability: &'static str },
    #[error("capability `{capability}` is not implemented yet in this build")]
    NotImplemented { capability: &'static str },
    #[error("provider is not installed")]
    NotInstalled,
    #[error("session `{0}` not found")]
    SessionNotFound(String),
    #[error("timed out: {0}")]
    Timeout(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

/// Where adapters push normalized events. Cheap to clone; never blocks.
#[derive(Clone)]
pub struct EventSink {
    tx: mpsc::Sender<AgentEvent>,
    dropped: Arc<AtomicU64>,
}

impl EventSink {
    pub fn new(capacity: usize) -> (Self, mpsc::Receiver<AgentEvent>) {
        let (tx, rx) = mpsc::channel(capacity);
        (
            Self {
                tx,
                dropped: Arc::new(AtomicU64::new(0)),
            },
            rx,
        )
    }

    /// Sends an event. If the pipeline is saturated the event is dropped and
    /// counted (shown in Diagnostics) instead of blocking the provider.
    pub fn emit(&self, event: AgentEvent) {
        if self.tx.try_send(event).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// How agent CLIs must invoke the hook relay (written into hook configs).
/// In the shipped app this is `<install dir>\agent-office.exe hook …`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayCommand {
    /// Absolute path of the executable.
    pub program: PathBuf,
    /// Arguments placed before the provider id (e.g. `["hook"]`).
    pub prefix_args: Vec<String>,
    /// Passed as `--data-dir` when Agent Office runs with a non-default data folder.
    pub data_dir: Option<PathBuf>,
}

impl RelayCommand {
    /// Full argument vector for one hook entry.
    pub fn args(&self, provider: &str, origin: &str, wait_secs: Option<u64>) -> Vec<String> {
        let mut args = self.prefix_args.clone();
        args.push(provider.to_owned());
        args.push("--origin".into());
        args.push(origin.to_owned());
        if let Some(wait) = wait_secs {
            args.push("--wait".into());
            args.push(wait.to_string());
        }
        if let Some(dir) = &self.data_dir {
            args.push("--data-dir".into());
            args.push(dir.display().to_string());
        }
        args
    }
}

/// User preferences that apply to every provider (set in Agent Office).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ProviderSettings {
    /// External sessions: let Agent Office answer permission prompts (the
    /// agent's own prompt appears only after the timeout). Off = observe only.
    pub answer_permissions_from_app: bool,
    /// Seconds Agent Office waits for Approve/Reject before handing the
    /// decision back (external) or denying it (managed).
    pub permission_timeout_secs: u32,
    /// Discover external sessions through official listing commands.
    pub discover_external_sessions: bool,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            answer_permissions_from_app: false,
            permission_timeout_secs: 120,
            discover_external_sessions: true,
        }
    }
}

/// One hook invocation forwarded by the relay.
#[derive(Debug, Clone, PartialEq)]
pub struct HookCall {
    /// `true` for hooks injected into a session Agent Office launched.
    pub managed_origin: bool,
    pub payload: serde_json::Value,
    pub received_at_ms: i64,
    pub payload_truncated: bool,
}

/// What the relay prints to the agent CLI (and its exit code).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookReply {
    pub stdout: Option<String>,
    pub exit_code: i32,
}

/// Everything an adapter receives from the host.
#[derive(Clone)]
pub struct AdapterContext {
    pub sink: EventSink,
    /// How to invoke the hook relay; `None` when it is unavailable.
    pub relay: Option<RelayCommand>,
    /// Agent Office data folder (per-session scratch files live here).
    pub data_dir: PathBuf,
}

impl AdapterContext {
    pub fn new(sink: EventSink, data_dir: PathBuf) -> Self {
        Self {
            sink,
            relay: None,
            data_dir,
        }
    }
}

#[async_trait]
pub trait ProviderAdapter: Send + Sync + 'static {
    fn descriptor(&self) -> ProviderDescriptor;

    fn capabilities(&self) -> CapabilityProfile;

    fn id(&self) -> ProviderId {
        self.descriptor().id
    }

    async fn detect_installation(&self) -> InstallationInfo {
        InstallationInfo {
            installed: false,
            error: Some("detection not implemented".into()),
            ..Default::default()
        }
    }

    async fn get_version(&self) -> Option<String> {
        self.detect_installation().await.version
    }

    async fn launch_session(
        &self,
        _request: LaunchRequest,
        _ctx: AdapterContext,
    ) -> Result<SessionHandle, ProviderError> {
        Err(ProviderError::Unsupported {
            capability: "launch",
        })
    }

    async fn attach_to_session(
        &self,
        _session_id: &SessionId,
        _ctx: AdapterContext,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::Unsupported {
            capability: "attach",
        })
    }

    async fn stop_session(
        &self,
        _session_id: &SessionId,
        _mode: StopMode,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::Unsupported { capability: "stop" })
    }

    async fn send_prompt(
        &self,
        _session_id: &SessionId,
        _prompt: &str,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::Unsupported {
            capability: "sendPrompt",
        })
    }

    async fn resolve_permission(
        &self,
        _session_id: &SessionId,
        _request_id: &PermissionRequestId,
        _decision: PermissionDecision,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::Unsupported {
            capability: "permissions",
        })
    }

    async fn list_sessions(&self) -> Result<Vec<ExternalSessionInfo>, ProviderError> {
        Err(ProviderError::Unsupported {
            capability: "listSessions",
        })
    }

    async fn integration_status(&self, _ctx: &AdapterContext) -> IntegrationStatus {
        IntegrationStatus::unsupported("No integration available for this provider in this build.")
    }

    async fn install_integration(
        &self,
        _ctx: &AdapterContext,
    ) -> Result<IntegrationStatus, ProviderError> {
        Err(ProviderError::Unsupported {
            capability: "integration",
        })
    }

    async fn uninstall_integration(
        &self,
        _ctx: &AdapterContext,
    ) -> Result<IntegrationStatus, ProviderError> {
        Err(ProviderError::Unsupported {
            capability: "integration",
        })
    }

    async fn repair_integration(
        &self,
        _ctx: &AdapterContext,
    ) -> Result<IntegrationStatus, ProviderError> {
        Err(ProviderError::Unsupported {
            capability: "integration",
        })
    }

    /// Applies user preferences. Called at startup and whenever they change.
    fn configure(&self, _settings: &ProviderSettings) {}

    /// Starts background work (e.g. discovering external sessions).
    async fn start(&self, _ctx: AdapterContext) {}

    /// Handles one hook invocation forwarded by the relay. The default
    /// ignores it (empty reply = the agent proceeds normally).
    async fn handle_hook(&self, _call: HookCall, _ctx: AdapterContext) -> HookReply {
        HookReply::default()
    }
}
