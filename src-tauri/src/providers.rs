//! The one place where providers are registered. Adding a provider means
//! adding its crate to Cargo.toml and one `register` line here.

use ao_core::registry::ProviderRegistry;
use ao_provider_demo::DemoAdapter;
use std::sync::Arc;

pub fn build_registry(demo: Arc<DemoAdapter>) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(ao_provider_claude::ClaudeAdapter::new()));
    registry.register(Arc::new(ao_provider_codex::CodexAdapter::new()));
    registry.register(Arc::new(ao_provider_cursor::CursorAdapter::new()));
    registry.register(demo);
    registry
}
