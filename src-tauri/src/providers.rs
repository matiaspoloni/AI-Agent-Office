//! The one place where providers are registered. Adding a provider means
//! adding its crate to Cargo.toml and one `register` line here.

use ao_core::registry::ProviderRegistry;
use ao_provider_claude::{ClaudeAdapter, ClaudeOptions};
use ao_provider_codex::{CodexAdapter, CodexOptions};
use ao_provider_cursor::{CursorAdapter, CursorOptions};
use ao_provider_demo::DemoAdapter;
use std::sync::Arc;

pub fn build_registry(
    demo: Arc<DemoAdapter>,
    claude: ClaudeOptions,
    codex: CodexOptions,
    cursor: CursorOptions,
) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(ClaudeAdapter::with_options(claude)));
    registry.register(Arc::new(CodexAdapter::with_options(codex)));
    registry.register(Arc::new(CursorAdapter::with_options(cursor)));
    registry.register(demo);
    registry
}
