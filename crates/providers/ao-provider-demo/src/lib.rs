//! Demo provider: simulated agents for UI development, tests and first-run
//! exploration. It is always labelled as simulated (`descriptor.simulated`,
//! `EventSource::Simulation`, provider id `demo`) and never impersonates a
//! real provider.

pub mod record;
pub mod runner;
pub mod script;

use ao_core::capabilities::{Capabilities, CapabilityProfile, ImplementedFeatures, Support::*};
use ao_core::event::SessionMode;
use ao_core::ids::{PermissionRequestId, SessionId};
use ao_core::provider::{
    AdapterContext, InstallationInfo, LaunchRequest, PermissionDecision, ProviderAdapter,
    ProviderDescriptor, ProviderError, SessionHandle, StopMode,
};
use async_trait::async_trait;
use runner::{provider_id, RealClock, Runner, SessionControl};
use script::{SessionPlan, PRESETS};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub use runner::PROVIDER_ID;

struct DemoSession {
    control: Arc<SessionControl>,
    prompts: mpsc::Sender<String>,
}

#[derive(Default)]
pub struct DemoAdapter {
    sessions: Mutex<HashMap<SessionId, DemoSession>>,
    next_preset: AtomicUsize,
}

impl DemoAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Launches one session per preset (the "demo office").
    pub fn launch_office(&self, ctx: &AdapterContext) -> Vec<SessionHandle> {
        PRESETS
            .iter()
            .map(|p| self.spawn(SessionPlan::from(p), ctx, true))
            .collect()
    }

    fn spawn(&self, plan: SessionPlan, ctx: &AdapterContext, autoplay: bool) -> SessionHandle {
        let session_id = SessionId(format!("demo-{}", uuid::Uuid::new_v4().simple()));
        let control = Arc::new(SessionControl::default());
        let (prompt_tx, prompt_rx) = mpsc::channel(8);
        *control.prompts.try_lock().expect("fresh control") = Some(prompt_rx);

        self.sessions.lock().expect("demo sessions lock").insert(
            session_id.clone(),
            DemoSession {
                control: control.clone(),
                prompts: prompt_tx,
            },
        );

        let sink = ctx.sink.clone();
        let emit: runner::Emit = Arc::new(move |event| sink.emit(event));
        let runner = Runner::new(
            Arc::new(RealClock),
            emit,
            session_id.clone(),
            control,
            autoplay,
        );
        tokio::spawn(async move {
            runner.run_plan(&plan).await;
        });

        SessionHandle {
            provider: provider_id(),
            session_id,
            mode: SessionMode::Managed,
            pid: None,
        }
    }

    fn control(&self, session_id: &SessionId) -> Result<Arc<SessionControl>, ProviderError> {
        self.sessions
            .lock()
            .expect("demo sessions lock")
            .get(session_id)
            .map(|s| s.control.clone())
            .ok_or_else(|| ProviderError::SessionNotFound(session_id.0.clone()))
    }
}

pub fn capability_profile() -> CapabilityProfile {
    let managed = Capabilities {
        launch: Supported,
        attach: Unsupported,
        list_sessions: Unsupported,
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
        resume: Unsupported,
    };
    CapabilityProfile {
        managed,
        external: Capabilities::NONE,
        implemented: ImplementedFeatures {
            detection: true,
            managed_sessions: true,
            external_sessions: false,
            integration_setup: false,
        },
        notes: vec![
            "Simulated agents. No real provider is contacted and no files are changed.".into(),
        ],
    }
}

#[async_trait]
impl ProviderAdapter for DemoAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: provider_id(),
            display_name: "Demo (simulated)".into(),
            accent_color: "#f2c94c".into(),
            badge: "SIM".into(),
            executable_names: vec![],
            homepage: None,
            simulated: true,
        }
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile()
    }

    async fn detect_installation(&self) -> InstallationInfo {
        InstallationInfo {
            installed: true,
            executable_path: None,
            version: Some(env!("CARGO_PKG_VERSION").into()),
            version_output: Some("built into Agent Office".into()),
            error: None,
        }
    }

    async fn launch_session(
        &self,
        request: LaunchRequest,
        ctx: AdapterContext,
    ) -> Result<SessionHandle, ProviderError> {
        let index = self.next_preset.fetch_add(1, Ordering::Relaxed) % PRESETS.len();
        let mut plan = SessionPlan::from(&PRESETS[index]);
        if let Some(name) = request.name.clone().filter(|n| !n.trim().is_empty()) {
            plan.title = name;
        }
        if !request.cwd.trim().is_empty() {
            plan.cwd = request.cwd.clone();
        }
        if let Some(model) = request.model.clone().filter(|m| !m.trim().is_empty()) {
            plan.model = model;
        }
        let handle = self.spawn(plan, &ctx, request.prompt.is_none());
        if let Some(prompt) = request.prompt {
            let _ = self.send_prompt(&handle.session_id, &prompt).await;
        }
        Ok(handle)
    }

    async fn stop_session(
        &self,
        session_id: &SessionId,
        _mode: StopMode,
    ) -> Result<(), ProviderError> {
        let control = self.control(session_id)?;
        control.stop();
        self.sessions
            .lock()
            .expect("demo sessions lock")
            .remove(session_id);
        Ok(())
    }

    async fn send_prompt(&self, session_id: &SessionId, prompt: &str) -> Result<(), ProviderError> {
        let sender = self
            .sessions
            .lock()
            .expect("demo sessions lock")
            .get(session_id)
            .map(|s| s.prompts.clone())
            .ok_or_else(|| ProviderError::SessionNotFound(session_id.0.clone()))?;
        sender
            .send(prompt.to_owned())
            .await
            .map_err(|_| ProviderError::Other("demo session is no longer running".into()))
    }

    async fn resolve_permission(
        &self,
        session_id: &SessionId,
        request_id: &PermissionRequestId,
        decision: PermissionDecision,
    ) -> Result<(), ProviderError> {
        let control = self.control(session_id)?;
        let mut pending = control.pending_permissions.lock().await;
        let index = pending
            .iter()
            .position(|(id, _)| id == request_id)
            .ok_or_else(|| {
                ProviderError::Other(format!(
                    "permission request {request_id} is no longer pending"
                ))
            })?;
        let (_, tx) = pending.remove(index);
        let approved = matches!(decision, PermissionDecision::Approve { .. });
        tx.send(approved)
            .map_err(|_| ProviderError::Other("session stopped waiting".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ao_core::event::{AgentEvent, EventKind};
    use ao_core::provider::EventSink;
    use std::time::Duration;

    async fn next_matching(
        rx: &mut mpsc::Receiver<AgentEvent>,
        pred: impl Fn(&AgentEvent) -> bool,
    ) -> AgentEvent {
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                let e = rx.recv().await.expect("stream open");
                if pred(&e) {
                    return e;
                }
            }
        })
        .await
        .expect("event arrives")
    }

    #[tokio::test(start_paused = true)]
    async fn permission_can_be_approved_and_session_stopped() {
        let adapter = DemoAdapter::new();
        let (sink, mut rx) = EventSink::new(10_000);
        let ctx = AdapterContext::new(sink, std::env::temp_dir());
        // Preset index 1 ("munder") runs the tests scenario that asks for permission.
        adapter.next_preset.store(1, Ordering::Relaxed);
        let handle = adapter
            .launch_session(
                LaunchRequest {
                    project_id: None,
                    cwd: String::new(),
                    model: None,
                    prompt: None,
                    name: None,
                    resume_session_id: None,
                    permission_mode: None,
                },
                ctx,
            )
            .await
            .unwrap();

        let request = next_matching(&mut rx, |e| {
            matches!(e.kind, EventKind::PermissionRequested(_))
        })
        .await;
        let EventKind::PermissionRequested(p) = request.kind else {
            unreachable!()
        };
        assert!(p.can_resolve);
        assert_eq!(request.source, ao_core::event::EventSource::Simulation);

        adapter
            .resolve_permission(
                &handle.session_id,
                &p.request_id,
                PermissionDecision::Approve { for_session: false },
            )
            .await
            .unwrap();
        next_matching(&mut rx, |e| {
            matches!(e.kind, EventKind::PermissionApproved(_))
        })
        .await;

        adapter
            .stop_session(&handle.session_id, StopMode::Graceful)
            .await
            .unwrap();
        let ended = next_matching(&mut rx, |e| matches!(e.kind, EventKind::SessionEnded(_))).await;
        assert_eq!(ended.session_id, handle.session_id);
    }
}
