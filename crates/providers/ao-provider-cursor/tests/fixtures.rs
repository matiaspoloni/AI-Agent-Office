//! Fixture-driven mapping tests (fixtures/cursor/acp/**). Files named
//! `*-sdk.json` hold messages of the official ACP example agent
//! (`@agentclientprotocol/sdk` 1.5.0); `*-schema.json` follow the ACP schema.
//! None come from a real Cursor CLI (see PROVIDER_CAPABILITIES §5).
//!
//! Besides ACP messages, `input` may contain two test steps:
//! `{"answered": {"requestId", "approved", "toolCallId"?}}` (Agent Office
//! answered a permission request) and `{"turnEnded": <PromptResponse>}`.

use ao_provider_cursor::acp::Mapper;
use ao_testkit::fixtures::{assert_events, load_dir};
use serde_json::Value;

const RECEIVED_AT: i64 = 1_790_000_000_000;

#[test]
fn acp_sequences_map_to_expected_events() {
    let fixtures = load_dir("cursor/acp");
    assert!(fixtures.len() >= 5);
    for fixture in fixtures {
        let session = fixture.context["sessionId"].as_str().expect("sessionId");
        let mut mapper = Mapper::new(session);
        if let Some(cwd) = fixture.context["cwd"].as_str() {
            mapper = mapper.with_cwd(cwd);
        }
        let mut events = Vec::new();
        for step in fixture.input.as_array().expect("input is a list") {
            if let Some(answered) = step.get("answered") {
                if answered["approved"] == Value::Bool(false) {
                    if let Some(tool) = answered["toolCallId"].as_str() {
                        mapper.permission_rejected(tool);
                    }
                }
            } else if let Some(result) = step.get("turnEnded") {
                events.extend(mapper.turn_ended(Ok(result), RECEIVED_AT));
            } else if step["method"] == "session/update" {
                events.extend(mapper.update(&step["params"]["update"], RECEIVED_AT));
            } else if step["method"] == "session/request_permission" {
                let id = format!("acp-{}", step["id"]);
                events.extend(mapper.permission(&id, &step["params"], RECEIVED_AT));
            } else {
                panic!("{}: unknown step {step}", fixture.path.display());
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
