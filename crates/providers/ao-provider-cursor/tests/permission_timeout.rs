//! A permission request nobody answers is rejected after the configured
//! wait, so the agent is never left blocked (driven by `fake-cursor`).

use ao_core::event::{AgentEvent, EventKind, PermissionResolver};
use ao_core::provider::{
    AdapterContext, EventSink, LaunchRequest, ProviderAdapter, ProviderSettings, StopMode,
};
use ao_provider_cursor::{CursorAdapter, CursorOptions};
use ao_testkit::bins::cargo_bin;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Receiver;

async fn until(rx: &mut Receiver<AgentEvent>, done: impl Fn(&EventKind) -> bool) -> Vec<EventKind> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = Vec::new();
    loop {
        let event = tokio::time::timeout(deadline - Instant::now(), rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out; seen {seen:#?}"))
            .expect("sink open");
        let stop = done(&event.kind);
        seen.push(event.kind);
        if stop {
            return seen;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn unanswered_permission_is_rejected_after_the_timeout() {
    let adapter = CursorAdapter::with_options(CursorOptions {
        executable: Some(cargo_bin("ao-testkit", "fake-cursor")),
    });
    adapter.configure(&ProviderSettings {
        permission_timeout_secs: 5,
        ..Default::default()
    });
    let (sink, mut rx) = EventSink::new(1_000);
    let cwd = std::env::temp_dir();
    let request = LaunchRequest {
        project_id: None,
        cwd: cwd.display().to_string(),
        model: None,
        prompt: Some("needs permission".into()),
        name: None,
        permission_mode: None,
    };
    let started = Instant::now();
    let handle = adapter
        .launch_session(request, AdapterContext::new(sink, cwd))
        .await
        .expect("launch");

    let seen = until(&mut rx, |k| matches!(k, EventKind::AgentIdle(_))).await;
    assert!(started.elapsed() >= Duration::from_secs(5));
    assert!(seen.iter().any(|k| matches!(k,
        EventKind::PermissionDenied(p) if p.resolved_by == PermissionResolver::Timeout)));
    assert!(
        seen.iter().any(|k| matches!(k, EventKind::AgentMessage(m)
        if m.text.as_deref() == Some("Okay, I did not run it."))),
        "the agent got `reject`: {seen:#?}"
    );
    assert!(seen
        .iter()
        .any(|k| matches!(k, EventKind::CommandFailed(_))));

    adapter
        .stop_session(&handle.session_id, StopMode::Graceful)
        .await
        .expect("stop");
    let seen = until(&mut rx, |k| matches!(k, EventKind::SessionEnded(_))).await;
    assert!(matches!(seen.last(), Some(EventKind::SessionEnded(e))
        if e.reason.as_deref() == Some("stopped by Agent Office")));
}
