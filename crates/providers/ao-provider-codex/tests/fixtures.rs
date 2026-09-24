//! Fixture-driven mapping tests (fixtures/codex/**). Files named `*-real.json`
//! hold messages recorded from codex-cli 0.156.1.

use ao_provider_codex::appserver::Mapper;
use ao_provider_codex::hooks;
use ao_testkit::fixtures::{assert_events, load_dir};
use serde_json::Value;

const RECEIVED_AT: i64 = 1_790_000_000_000;

#[test]
fn hook_payloads_map_to_expected_events() {
    let fixtures = load_dir("codex/hooks");
    assert!(fixtures.len() >= 15);
    for fixture in fixtures {
        let ctx = hooks::HookContext {
            received_at_ms: RECEIVED_AT,
            permission: fixture.context["permission"]
                .as_array()
                .map(|p| (p[0].as_str().unwrap().to_owned(), p[1].as_bool().unwrap())),
        };
        let events = hooks::map_hook(&fixture.input, &ctx);
        assert_events(&fixture, &events);
        assert!(events.iter().all(|e| e.timestamp == RECEIVED_AT));
    }
}

#[test]
fn app_server_sequences_map_to_expected_events() {
    let fixtures = load_dir("codex/appserver");
    assert!(fixtures.len() >= 6);
    for fixture in fixtures {
        let root = fixture.context["rootThread"].as_str().expect("rootThread");
        let mut mapper = Mapper::new(root);
        if let Some(cwd) = fixture.context["cwd"].as_str() {
            mapper = mapper.with_cwd(cwd);
        }
        let mut events = Vec::new();
        for message in fixture.input.as_array().expect("input is a message list") {
            let method = message["method"].as_str().expect("method");
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            match message.get("id") {
                // Server → client request (approval).
                Some(id) => {
                    if let Some(approval) = mapper.approval(method, id, &params, RECEIVED_AT) {
                        events.push(approval.event);
                    }
                }
                None => events.extend(mapper.notification(method, &params, RECEIVED_AT)),
            }
        }
        for event in &events {
            event
                .validate()
                .unwrap_or_else(|e| panic!("{}: invalid event: {e}", fixture.path.display()));
        }
        assert_events(&fixture, &events);
    }
}
