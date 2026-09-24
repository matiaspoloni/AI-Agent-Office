//! Writes the browser-preview timeline used by `npm run dev:web`.
//!
//! Usage: cargo run -p ao-provider-demo --example record_preview -- <output.json> [seconds]

use ao_core::registry::ProviderRegistry;
use std::sync::Arc;

fn main() {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .unwrap_or_else(|| "src/ipc/preview-recording.json".into());
    let seconds: i64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(90);

    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(ao_provider_claude::ClaudeAdapter::new()));
    registry.register(Arc::new(ao_provider_codex::CodexAdapter::new()));
    registry.register(Arc::new(ao_provider_cursor::CursorAdapter::new()));
    registry.register(Arc::new(ao_provider_demo::DemoAdapter::new()));

    let mut recording = ao_provider_demo::record::record_office(seconds * 1000);
    recording.providers = registry.infos();
    let json = serde_json::to_string(&recording).expect("serialize recording");
    std::fs::write(&output, json).expect("write recording");
    println!(
        "wrote {} frames ({} s) to {output}",
        recording.frames.len(),
        seconds
    );
}
