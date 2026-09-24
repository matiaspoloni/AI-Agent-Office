//! Executes demo scripts, emitting normalized events. Works with a real clock
//! (desktop app) or a virtual clock (deterministic recordings and tests).

use crate::script::{scenario, FileOp, Scenario, SessionPlan, Step, SubScript};
use ao_core::event::*;
use ao_core::ids::{AgentId, PermissionRequestId, ProviderId, SessionId, ToolCallId};
use ao_core::time::now_ms;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, Notify};

pub const PROVIDER_ID: &str = "demo";

#[async_trait]
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> i64;
    async fn sleep(&self, ms: u64);
    /// Virtual clocks auto-approve permissions after a pause instead of waiting for a user.
    fn is_virtual(&self) -> bool {
        false
    }
}

pub struct RealClock;

#[async_trait]
impl Clock for RealClock {
    fn now(&self) -> i64 {
        now_ms()
    }
    async fn sleep(&self, ms: u64) {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    }
}

pub struct VirtualClock {
    now: AtomicI64,
}

impl VirtualClock {
    pub fn starting_at(ms: i64) -> Self {
        Self {
            now: AtomicI64::new(ms),
        }
    }
}

#[async_trait]
impl Clock for VirtualClock {
    fn now(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }
    async fn sleep(&self, ms: u64) {
        self.now.fetch_add(ms as i64, Ordering::SeqCst);
    }
    fn is_virtual(&self) -> bool {
        true
    }
}

pub type Emit = Arc<dyn Fn(AgentEvent) + Send + Sync>;

/// Controls shared between the adapter and a running session.
#[derive(Default)]
pub struct SessionControl {
    pub stopped: AtomicBool,
    pub stop_notify: Notify,
    pub pending_permissions: Mutex<Vec<(PermissionRequestId, oneshot::Sender<bool>)>>,
    pub prompts: Mutex<Option<mpsc::Receiver<String>>>,
}

impl SessionControl {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.stop_notify.notify_waiters();
        // Also leave a permit in case the runner is between checks.
        self.stop_notify.notify_one();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

pub struct Runner<C: Clock> {
    pub clock: Arc<C>,
    pub emit: Emit,
    pub session: SessionId,
    pub control: Arc<SessionControl>,
    pub autoplay: bool,
    seq: AtomicU64,
}

pub enum Flow {
    Continue,
    Stop,
}

enum NextTask {
    Stop,
    Finish,
    Autoplay,
    Prompt(String),
}

impl<C: Clock> Runner<C> {
    pub fn new(
        clock: Arc<C>,
        emit: Emit,
        session: SessionId,
        control: Arc<SessionControl>,
        autoplay: bool,
    ) -> Self {
        Self {
            clock,
            emit,
            session,
            control,
            autoplay,
            seq: AtomicU64::new(0),
        }
    }

    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.seq.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn event(&self, agent: Option<&AgentId>, kind: EventKind) -> AgentEvent {
        let e = AgentEvent::for_session(
            PROVIDER_ID,
            self.session.clone(),
            EventSource::Simulation,
            kind,
        )
        .at(self.clock.now());
        match agent {
            Some(sub) => e.with_agent(sub.clone(), Some(AgentId(self.session.0.clone()))),
            None => e,
        }
    }

    fn send(&self, agent: Option<&AgentId>, kind: EventKind) {
        (self.emit)(self.event(agent, kind));
    }

    /// Sleeps, returning early with `Stop` if the session is stopped.
    async fn pause(&self, ms: u64) -> Flow {
        if self.control.is_stopped() {
            return Flow::Stop;
        }
        if self.clock.is_virtual() {
            self.clock.sleep(ms).await;
            return Flow::Continue;
        }
        tokio::select! {
            _ = self.clock.sleep(ms) => {}
            _ = self.control.stop_notify.notified() => {}
        }
        if self.control.is_stopped() {
            Flow::Stop
        } else {
            Flow::Continue
        }
    }

    pub async fn run_plan(&self, preset: &SessionPlan) {
        self.send(
            None,
            EventKind::SessionStarted(SessionInfo {
                mode: Some(SessionMode::Managed),
                cwd: Some(preset.cwd.clone()),
                model: Some(preset.model.clone()),
                title: Some(preset.title.clone()),
                reason: Some("startup".into()),
                ..Default::default()
            }),
        );
        self.send(
            None,
            EventKind::GitBranchChanged(GitBranchChanged {
                branch: Some(format!("demo/{}", preset.key)),
                previous: None,
            }),
        );
        if let Flow::Stop = self.pause(800).await {
            return self.end("stopped");
        }

        let mut prompts = self.control.prompts.lock().await.take();
        let mut round = 0usize;
        let mut user_prompt: Option<String> = None;
        loop {
            let key = preset.scenarios[round % preset.scenarios.len()];
            let script = scenario(key);
            if let Flow::Stop = self.run_scenario(&script, user_prompt.as_deref()).await {
                return self.end("stopped");
            }
            self.send(
                None,
                EventKind::AgentIdle(TextNote {
                    text: Some("Waiting for the next task".into()),
                }),
            );
            round += 1;
            match self
                .next_task(&mut prompts, 6_000 + (round as u64 % 3) * 2_000)
                .await
            {
                NextTask::Stop => return self.end("stopped"),
                NextTask::Finish => return,
                NextTask::Autoplay => user_prompt = None,
                NextTask::Prompt(text) => user_prompt = Some(text),
            }
        }
    }

    /// Waits for a user prompt, the autoplay timer, or a stop request.
    async fn next_task(
        &self,
        prompts: &mut Option<mpsc::Receiver<String>>,
        autoplay_ms: u64,
    ) -> NextTask {
        if self.control.is_stopped() {
            return NextTask::Stop;
        }
        if self.clock.is_virtual() {
            self.clock.sleep(autoplay_ms).await;
            return if self.autoplay {
                NextTask::Autoplay
            } else {
                NextTask::Finish
            };
        }
        let autoplay = self.autoplay;
        let wait_prompt = async {
            match prompts.as_mut() {
                Some(rx) => rx.recv().await,
                None => std::future::pending().await,
            }
        };
        let timer = async {
            if autoplay {
                self.clock.sleep(autoplay_ms).await
            } else {
                std::future::pending::<()>().await
            }
        };
        tokio::select! {
            prompt = wait_prompt => match prompt {
                Some(text) => NextTask::Prompt(text),
                None => NextTask::Finish,
            },
            _ = timer => NextTask::Autoplay,
            _ = self.control.stop_notify.notified() => NextTask::Stop,
        }
    }

    /// Runs a scenario; `prompt_override` is used when the user typed a prompt.
    pub async fn run_scenario(&self, script: &Scenario, prompt_override: Option<&str>) -> Flow {
        self.send(
            None,
            EventKind::PromptSubmitted(PromptSubmitted {
                text: Some(prompt_override.unwrap_or(script.prompt).to_owned()),
            }),
        );
        for step in &script.steps {
            if let Flow::Stop = self.step(None, step).await {
                return Flow::Stop;
            }
        }
        Flow::Continue
    }

    async fn step(&self, agent: Option<&AgentId>, step: &Step) -> Flow {
        match step {
            Step::Think(text, ms) => {
                self.send(
                    agent,
                    EventKind::AgentThinking(TextNote {
                        text: Some((*text).into()),
                    }),
                );
                self.pause(*ms).await
            }
            Step::Message(text) => {
                self.send(
                    agent,
                    EventKind::AgentMessage(TextNote {
                        text: Some((*text).into()),
                    }),
                );
                self.pause(400).await
            }
            Step::Tool {
                name,
                category,
                title,
                ms,
                ok,
                file,
            } => {
                let id = ToolCallId(self.next_id("tool"));
                self.send(
                    agent,
                    EventKind::ToolStarted(ToolStarted {
                        tool_call_id: Some(id.clone()),
                        tool_name: (*name).into(),
                        category: *category,
                        title: Some((*title).into()),
                    }),
                );
                if let Flow::Stop = self.pause(*ms).await {
                    return Flow::Stop;
                }
                if let Some((op, path)) = file {
                    let touched = FileTouched {
                        path: (*path).into(),
                        tool_call_id: Some(id.clone()),
                    };
                    let kind = match op {
                        FileOp::Read => EventKind::FileRead(touched),
                        FileOp::Create => EventKind::FileCreated(touched),
                        FileOp::Modify => EventKind::FileModified(touched),
                    };
                    self.send(agent, kind);
                }
                let finished = ToolFinished {
                    tool_call_id: Some(id),
                    tool_name: (*name).into(),
                    category: *category,
                    duration_ms: Some(*ms),
                    detail: None,
                };
                self.send(
                    agent,
                    if *ok {
                        EventKind::ToolCompleted(finished)
                    } else {
                        EventKind::ToolFailed(finished)
                    },
                );
                Flow::Continue
            }
            Step::Command {
                command,
                ms,
                exit_code,
                output,
            } => {
                let tool_id = ToolCallId(self.next_id("tool"));
                let command_id = self.next_id("cmd");
                self.send(
                    agent,
                    EventKind::ToolStarted(ToolStarted {
                        tool_call_id: Some(tool_id.clone()),
                        tool_name: "Shell".into(),
                        category: ToolCategory::Execute,
                        title: Some((*command).into()),
                    }),
                );
                self.send(
                    agent,
                    EventKind::CommandStarted(CommandStarted {
                        command_id: Some(command_id.clone()),
                        command: (*command).into(),
                        cwd: None,
                    }),
                );
                let slice = (*ms / (output.len() as u64 + 1)).max(1);
                for line in output.iter() {
                    if let Flow::Stop = self.pause(slice).await {
                        return Flow::Stop;
                    }
                    self.send(
                        agent,
                        EventKind::CommandOutput(CommandOutput {
                            command_id: Some(command_id.clone()),
                            stream: OutputStream::Stdout,
                            chunk: (*line).into(),
                        }),
                    );
                }
                if let Flow::Stop = self.pause(slice).await {
                    return Flow::Stop;
                }
                let finished = CommandFinished {
                    command_id: Some(command_id),
                    command: Some((*command).into()),
                    exit_code: Some(*exit_code),
                    duration_ms: Some(*ms),
                    error: None,
                };
                let tool_done = ToolFinished {
                    tool_call_id: Some(tool_id),
                    tool_name: "Shell".into(),
                    category: ToolCategory::Execute,
                    duration_ms: Some(*ms),
                    detail: output.last().map(|l| (*l).to_owned()),
                };
                if *exit_code == 0 {
                    self.send(agent, EventKind::CommandCompleted(finished));
                    self.send(agent, EventKind::ToolCompleted(tool_done));
                } else {
                    self.send(agent, EventKind::CommandFailed(finished));
                    self.send(agent, EventKind::ToolFailed(tool_done));
                }
                Flow::Continue
            }
            Step::Permission { tool, description } => {
                self.permission(agent, tool, description).await
            }
            Step::Usage {
                input,
                output,
                cached,
            } => {
                self.send(
                    agent,
                    EventKind::UsageUpdated(UsageSnapshot {
                        input_tokens: Some(*input),
                        output_tokens: Some(*output),
                        cached_input_tokens: Some(*cached),
                        total_tokens: Some(input + output),
                        ..Default::default()
                    }),
                );
                Flow::Continue
            }
            Step::Subagents(subs) => Box::pin(self.subagents(subs)).await,
            Step::Compact => {
                self.send(
                    agent,
                    EventKind::ContextCompacted(TextNote {
                        text: Some("auto".into()),
                    }),
                );
                self.pause(1_500).await
            }
            Step::Error(message) => {
                self.send(
                    agent,
                    EventKind::AgentError(AgentError {
                        message: (*message).into(),
                        error_type: Some("simulated".into()),
                        recoverable: true,
                    }),
                );
                self.pause(3_000).await
            }
            Step::Wait(ms) => self.pause(*ms).await,
        }
    }

    async fn permission(&self, agent: Option<&AgentId>, tool: &str, description: &str) -> Flow {
        let request_id = PermissionRequestId(self.next_id("perm"));
        self.send(
            agent,
            EventKind::PermissionRequested(PermissionRequested {
                request_id: request_id.clone(),
                tool_name: Some(tool.into()),
                description: description.into(),
                can_resolve: true,
                options: vec![
                    PermissionOption {
                        id: "allow".into(),
                        kind: PermissionOptionKind::AllowOnce,
                        label: "Approve".into(),
                    },
                    PermissionOption {
                        id: "reject".into(),
                        kind: PermissionOptionKind::RejectOnce,
                        label: "Reject".into(),
                    },
                ],
            }),
        );

        let (approved, resolver) = if self.clock.is_virtual() {
            self.clock.sleep(4_000).await;
            (true, PermissionResolver::App)
        } else {
            let (tx, rx) = oneshot::channel();
            self.control
                .pending_permissions
                .lock()
                .await
                .push((request_id.clone(), tx));
            tokio::select! {
                decision = rx => match decision {
                    Ok(approved) => (approved, PermissionResolver::App),
                    Err(_) => (false, PermissionResolver::Unknown),
                },
                _ = self.clock.sleep(180_000) => (false, PermissionResolver::Timeout),
                _ = self.control.stop_notify.notified() => return Flow::Stop,
            }
        };
        self.control
            .pending_permissions
            .lock()
            .await
            .retain(|(id, _)| id != &request_id);

        let resolved = PermissionResolved {
            request_id,
            resolved_by: resolver,
            message: None,
        };
        if approved {
            self.send(agent, EventKind::PermissionApproved(resolved));
            self.pause(500).await
        } else {
            self.send(agent, EventKind::PermissionDenied(resolved));
            self.send(
                agent,
                EventKind::AgentThinking(TextNote {
                    text: Some("Permission denied; choosing another approach".into()),
                }),
            );
            self.pause(1_500).await
        }
    }

    /// Runs subagents "in parallel" by interleaving their steps round-robin.
    async fn subagents(&self, subs: &[SubScript]) -> Flow {
        let task_id = ToolCallId(self.next_id("tool"));
        self.send(
            None,
            EventKind::ToolStarted(ToolStarted {
                tool_call_id: Some(task_id.clone()),
                tool_name: "Task".into(),
                category: ToolCategory::Subagent,
                title: Some(format!("Delegating to {} subagent(s)", subs.len())),
            }),
        );
        let ids: Vec<AgentId> = subs.iter().map(|_| AgentId(self.next_id("sub"))).collect();
        for (sub, id) in subs.iter().zip(&ids) {
            self.send(
                Some(id),
                EventKind::SubagentStarted(SubagentInfo {
                    agent_type: Some(sub.agent_type.into()),
                    description: Some(sub.description.into()),
                    reason: None,
                }),
            );
            if let Flow::Stop = self.pause(300).await {
                return Flow::Stop;
            }
        }
        let longest = subs.iter().map(|s| s.steps.len()).max().unwrap_or(0);
        for index in 0..longest {
            for (sub, id) in subs.iter().zip(&ids) {
                if let Some(step) = sub.steps.get(index) {
                    if let Flow::Stop = self.step(Some(id), step).await {
                        return Flow::Stop;
                    }
                }
            }
        }
        for id in &ids {
            self.send(
                Some(id),
                EventKind::SubagentEnded(SubagentInfo {
                    reason: Some("completed".into()),
                    ..Default::default()
                }),
            );
            if let Flow::Stop = self.pause(400).await {
                return Flow::Stop;
            }
        }
        self.send(
            None,
            EventKind::ToolCompleted(ToolFinished {
                tool_call_id: Some(task_id),
                tool_name: "Task".into(),
                category: ToolCategory::Subagent,
                duration_ms: None,
                detail: None,
            }),
        );
        Flow::Continue
    }

    pub fn end(&self, reason: &str) {
        self.send(
            None,
            EventKind::SessionEnded(SessionEnded {
                reason: Some(reason.into()),
                exit_code: None,
            }),
        );
    }
}

/// The provider id used by every demo event.
pub fn provider_id() -> ProviderId {
    ProviderId::new(PROVIDER_ID)
}
