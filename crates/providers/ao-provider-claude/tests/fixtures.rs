//! Fixture-driven mapping tests (fixtures/claude/**).

use ao_provider_claude::{hooks, stream};
use ao_testkit::fixtures::{assert_events, load_dir};

const RECEIVED_AT: i64 = 1_790_000_000_000;

#[test]
fn hook_payloads_map_to_expected_events() {
    let fixtures = load_dir("claude/hooks");
    assert!(fixtures.len() >= 20);
    for fixture in fixtures {
        let ctx = hooks::HookContext {
            received_at_ms: RECEIVED_AT,
            managed: fixture.context["managed"].as_bool().unwrap_or(false),
            permission: fixture.context["permission"]
                .as_array()
                .map(|p| (p[0].as_str().unwrap().to_owned(), p[1].as_bool().unwrap())),
            session_agent_type: fixture.context["sessionAgentType"]
                .as_str()
                .map(str::to_owned),
        };
        let events = hooks::map_hook(&fixture.input, &ctx);
        assert_events(&fixture, &events);
        for event in &events {
            assert_eq!(
                event.timestamp,
                RECEIVED_AT,
                "{}: hook events use the relay timestamp",
                fixture.path.display()
            );
        }
    }
}

#[test]
fn stream_lines_map_to_expected_events() {
    for fixture in load_dir("claude/stream") {
        let line = fixture.input.to_string();
        let events =
            stream::map_stream_line(&line, "11111111-2222-4333-8444-555555555555", RECEIVED_AT);
        assert_events(&fixture, &events);
    }
}
