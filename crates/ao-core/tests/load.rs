//! Phase 2 exit criterion: thousands of events flow through the pipeline,
//! reducer and UI batcher quickly and leave a consistent state.

use ao_core::batch::Batcher;
use ao_core::event::*;
use ao_core::pipeline::{Ingest, Pipeline};
use ao_core::sanitize::SanitizeLimits;
use ao_core::world::WorldState;
use std::time::{Duration, Instant};

const SESSIONS: usize = 20;
const SUBAGENTS_PER_SESSION: usize = 3;

fn scripted_events(total: usize) -> Vec<AgentEvent> {
    let mut events = Vec::with_capacity(total);
    let mut t = 1_800_000_000_000i64;
    for s in 0..SESSIONS {
        events.push(
            AgentEvent::for_session(
                "claude",
                format!("s{s}"),
                EventSource::Hook,
                EventKind::SessionStarted(SessionInfo {
                    cwd: Some(format!("/work/p{s}")),
                    ..Default::default()
                }),
            )
            .at(t),
        );
        for a in 0..SUBAGENTS_PER_SESSION {
            events.push(
                AgentEvent::for_session(
                    "claude",
                    format!("s{s}"),
                    EventSource::Hook,
                    EventKind::SubagentStarted(SubagentInfo {
                        agent_type: Some("Explore".into()),
                        ..Default::default()
                    }),
                )
                .with_agent(format!("s{s}-a{a}"), None)
                .at(t),
            );
        }
    }
    let mut n = 0usize;
    while events.len() < total {
        t += 1;
        let s = n % SESSIONS;
        let agent = n % (SUBAGENTS_PER_SESSION + 1);
        let tool_id = format!("t{n}");
        let base = |kind: EventKind| {
            let e =
                AgentEvent::for_session("claude", format!("s{s}"), EventSource::Hook, kind).at(t);
            if agent == 0 {
                e
            } else {
                e.with_agent(format!("s{s}-a{}", agent - 1), None)
            }
        };
        events.push(base(EventKind::ToolStarted(ToolStarted {
            tool_call_id: Some(tool_id.clone().into()),
            tool_name: "Bash".into(),
            category: ToolCategory::Execute,
            title: Some("cargo test".into()),
        })));
        events.push(base(EventKind::CommandOutput(CommandOutput {
            command_id: Some(tool_id.clone()),
            stream: OutputStream::Stdout,
            chunk: "test result: ok".repeat(4),
        })));
        events.push(base(EventKind::ToolCompleted(ToolFinished {
            tool_call_id: Some(tool_id.into()),
            tool_name: "Bash".into(),
            category: ToolCategory::Execute,
            duration_ms: Some(3),
            detail: None,
        })));
        n += 1;
    }
    events.truncate(total);
    events
}

#[test]
fn ten_thousand_events_are_processed_quickly_and_consistently() {
    let events = scripted_events(10_000);
    let mut pipeline = Pipeline::new(SanitizeLimits::default());
    let mut world = WorldState::new();
    let mut batcher = Batcher::new();
    let mut batches = 0;

    let started = Instant::now();
    for (i, event) in events.into_iter().enumerate() {
        let Ingest::Accepted(event) = pipeline.ingest(event) else {
            panic!("event {i} was not accepted");
        };
        let touched = world.apply(&event);
        batcher.note(&event, touched);
        // The host flushes every 100 ms; flush every 250 events here.
        if i % 250 == 0 && batcher.flush(&world).is_some() {
            batches += 1;
        }
    }
    batcher.flush(&world);
    let elapsed = started.elapsed();

    // Generous bound for unoptimized debug builds on slow CI machines;
    // release builds process this in a few tens of milliseconds.
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    assert!(batches >= 30);
    assert_eq!(world.sessions().count(), SESSIONS);
    assert_eq!(
        world.agents().count(),
        SESSIONS * (1 + SUBAGENTS_PER_SESSION)
    );
    for agent in world.agents() {
        assert!(
            agent.running_tools.len() <= 1,
            "{} leaked running tools",
            agent.key
        );
    }
    let total_tools: u32 = world.sessions().map(|s| s.stats.tool_calls).sum();
    assert!(total_tools > 3_000);
}

#[test]
fn every_event_type_is_reducible_and_roundtrips() {
    let kinds = vec![
        EventKind::SessionStarted(SessionInfo::default()),
        EventKind::SessionUpdated(SessionInfo::default()),
        EventKind::PromptSubmitted(PromptSubmitted::default()),
        EventKind::AgentThinking(TextNote::default()),
        EventKind::AgentMessage(TextNote::default()),
        EventKind::AgentIdle(TextNote::default()),
        EventKind::AgentWaiting(AgentWaiting {
            reason: WaitingReason::Input,
            message: None,
        }),
        EventKind::AgentError(AgentError {
            message: "x".into(),
            error_type: None,
            recoverable: true,
        }),
        EventKind::ToolStarted(ToolStarted {
            tool_call_id: Some("t".into()),
            tool_name: "Read".into(),
            category: ToolCategory::Read,
            title: None,
        }),
        EventKind::ToolCompleted(ToolFinished {
            tool_call_id: Some("t".into()),
            tool_name: "Read".into(),
            category: ToolCategory::Read,
            duration_ms: None,
            detail: None,
        }),
        EventKind::ToolFailed(ToolFinished {
            tool_call_id: Some("u".into()),
            tool_name: "Read".into(),
            category: ToolCategory::Read,
            duration_ms: None,
            detail: Some("boom".into()),
        }),
        EventKind::FileRead(FileTouched {
            path: "a".into(),
            tool_call_id: None,
        }),
        EventKind::FileCreated(FileTouched {
            path: "b".into(),
            tool_call_id: None,
        }),
        EventKind::FileModified(FileTouched {
            path: "c".into(),
            tool_call_id: None,
        }),
        EventKind::FileDeleted(FileTouched {
            path: "d".into(),
            tool_call_id: None,
        }),
        EventKind::CommandStarted(CommandStarted {
            command_id: Some("c".into()),
            command: "ls".into(),
            cwd: None,
        }),
        EventKind::CommandOutput(CommandOutput {
            command_id: Some("c".into()),
            stream: OutputStream::Stderr,
            chunk: "x".into(),
        }),
        EventKind::CommandCompleted(CommandFinished {
            command_id: Some("c".into()),
            command: None,
            exit_code: Some(0),
            duration_ms: None,
            error: None,
        }),
        EventKind::CommandFailed(CommandFinished {
            command_id: Some("d".into()),
            command: None,
            exit_code: Some(1),
            duration_ms: None,
            error: None,
        }),
        EventKind::PermissionRequested(PermissionRequested {
            request_id: "p".into(),
            tool_name: None,
            description: "x".into(),
            can_resolve: false,
            options: vec![],
        }),
        EventKind::PermissionApproved(PermissionResolved {
            request_id: "p".into(),
            resolved_by: PermissionResolver::App,
            message: None,
        }),
        EventKind::PermissionDenied(PermissionResolved {
            request_id: "q".into(),
            resolved_by: PermissionResolver::Provider,
            message: None,
        }),
        EventKind::PermissionExpired(PermissionResolved {
            request_id: "r".into(),
            resolved_by: PermissionResolver::Timeout,
            message: None,
        }),
        EventKind::GitBranchChanged(GitBranchChanged {
            branch: Some("main".into()),
            previous: None,
        }),
        EventKind::GitCommitCreated(GitCommitCreated {
            sha: "abc".into(),
            summary: "x".into(),
        }),
        EventKind::GitStatusChanged(GitStatusChanged::default()),
        EventKind::ContextCompacted(TextNote::default()),
        EventKind::UsageUpdated(UsageSnapshot {
            input_tokens: Some(1),
            ..Default::default()
        }),
        EventKind::ProviderError(ProviderErrorInfo {
            component: "x".into(),
            message: "y".into(),
        }),
        EventKind::SessionEnded(SessionEnded::default()),
    ];
    let mut world = WorldState::new();
    for (i, kind) in kinds.into_iter().enumerate() {
        let event = AgentEvent::for_session("demo", "s", EventSource::Simulation, kind)
            .at(1_800_000_000_000 + i as i64);
        let json = serde_json::to_string(&event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
        world.apply(&event);
    }
    // Subagent events need a subagent target.
    for kind in [
        EventKind::SubagentStarted(SubagentInfo::default()),
        EventKind::SubagentUpdated(SubagentInfo::default()),
        EventKind::SubagentEnded(SubagentInfo::default()),
    ] {
        let event = AgentEvent::for_session("demo", "s", EventSource::Simulation, kind)
            .with_agent("sub", None);
        assert!(event.validate().is_ok());
        world.apply(&event);
    }
    assert_eq!(world.sessions().count(), 1);
}
