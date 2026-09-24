//! OpenAI Codex CLI adapter.
//!
//! Phase 1 implements detection only. Managed sessions use `codex app-server`
//! (JSON-RPC over stdio, experimental upstream) and external sessions use
//! trusted hooks in `~/.codex/hooks.json` — see docs/PROVIDER_CAPABILITIES.md §4.

use ao_core::capabilities::{Capabilities, CapabilityProfile, ImplementedFeatures, Support::*};
use ao_core::ids::ProviderId;
use ao_core::provider::{InstallationInfo, ProviderAdapter, ProviderDescriptor};
use async_trait::async_trait;

pub const PROVIDER_ID: &str = "codex";

const EXTRA_DIRS: &[&str] = if cfg!(windows) {
    &["%APPDATA%\\npm", "%USERPROFILE%\\.local\\bin"]
} else {
    &["~/.local/bin", "/usr/local/bin", "/opt/homebrew/bin"]
};

#[derive(Debug, Default)]
pub struct CodexAdapter;

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }
}

pub fn capability_profile() -> CapabilityProfile {
    CapabilityProfile {
        managed: Capabilities {
            // app-server is labelled [experimental] by the Codex CLI.
            launch: Experimental,
            attach: Unsupported,
            list_sessions: Supported,
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
            resume: Supported,
        },
        external: Capabilities {
            launch: Unsupported,
            // Hooks are stable, but each hook must be trusted by the user via /hooks.
            attach: Partial,
            list_sessions: Unsupported,
            stop: Unsupported,
            send_prompt: Unsupported,
            structured_events: Supported,
            tool_events: Supported,
            file_events: Partial,
            command_events: Supported,
            permissions: Partial,
            subagents: Supported,
            usage: Unsupported,
            cost: Unsupported,
            model: Supported,
            context_compaction: Supported,
            resume: Unsupported,
        },
        implemented: ImplementedFeatures {
            detection: true,
            managed_sessions: false,
            external_sessions: false,
            integration_setup: false,
        },
        notes: vec![
            "Managed sessions use `codex app-server` (JSON-RPC v2), marked experimental by Codex.".into(),
            "External sessions need hooks in %USERPROFILE%\\.codex\\hooks.json trusted once via /hooks.".into(),
            "Codex reports token usage but no cost.".into(),
            "External Codex sessions cannot be listed or stopped from Agent Office.".into(),
        ],
    }
}

#[async_trait]
impl ProviderAdapter for CodexAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new(PROVIDER_ID),
            display_name: "Codex CLI".into(),
            accent_color: "#10a37f".into(),
            badge: "CX".into(),
            executable_names: vec!["codex".into()],
            homepage: Some("https://github.com/openai/codex".into()),
            simulated: false,
        }
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile()
    }

    async fn detect_installation(&self) -> InstallationInfo {
        ao_detect::detect(
            &self.descriptor().executable_names,
            EXTRA_DIRS,
            &["--version"],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_documented_capabilities() {
        let caps = capability_profile();
        assert_eq!(caps.managed.launch, Experimental);
        assert_eq!(caps.managed.cost, Unsupported);
        assert_eq!(caps.external.list_sessions, Unsupported);
        assert_eq!(caps.external.attach, Partial);
    }
}
