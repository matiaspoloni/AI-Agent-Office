//! Cursor CLI (`agent`) adapter.
//!
//! Phase 1 implements detection only. Managed sessions will use `agent acp`
//! (Agent Client Protocol over stdio). Many values are negotiated at runtime
//! from the ACP `initialize` response; external (hook) sessions are
//! experimental — see docs/PROVIDER_CAPABILITIES.md §5.

use ao_core::capabilities::{Capabilities, CapabilityProfile, ImplementedFeatures, Support::*};
use ao_core::ids::ProviderId;
use ao_core::provider::{InstallationInfo, ProviderAdapter, ProviderDescriptor};
use async_trait::async_trait;

pub const PROVIDER_ID: &str = "cursor";

const EXTRA_DIRS: &[&str] = if cfg!(windows) {
    &["%LOCALAPPDATA%\\cursor-agent", "%USERPROFILE%\\.local\\bin"]
} else {
    &["~/.local/bin", "/usr/local/bin", "/opt/homebrew/bin"]
};

#[derive(Debug, Default)]
pub struct CursorAdapter;

impl CursorAdapter {
    pub fn new() -> Self {
        Self
    }
}

pub fn capability_profile() -> CapabilityProfile {
    CapabilityProfile {
        managed: Capabilities {
            launch: Supported,
            attach: Unsupported,
            list_sessions: Runtime,
            stop: Supported,
            send_prompt: Supported,
            structured_events: Supported,
            tool_events: Supported,
            file_events: Supported,
            command_events: Supported,
            permissions: Supported,
            subagents: Unsupported,
            usage: Runtime,
            cost: Runtime,
            model: Runtime,
            context_compaction: Runtime,
            resume: Runtime,
        },
        external: Capabilities {
            launch: Unsupported,
            attach: Experimental,
            list_sessions: Unsupported,
            stop: Unsupported,
            send_prompt: Unsupported,
            structured_events: Experimental,
            tool_events: Experimental,
            file_events: Experimental,
            command_events: Experimental,
            permissions: Unsupported,
            subagents: Unsupported,
            usage: Unsupported,
            cost: Unsupported,
            model: Unsupported,
            context_compaction: Experimental,
            resume: Unsupported,
        },
        implemented: ImplementedFeatures {
            detection: true,
            managed_sessions: false,
            external_sessions: false,
            integration_setup: false,
        },
        notes: vec![
            "Managed sessions use `agent acp` (Agent Client Protocol, JSON-RPC over stdio).".into(),
            "Values marked runtime are negotiated from the ACP initialize response per session.".into(),
            "Cursor CLI hooks are reported to fire only partially; external sessions are experimental.".into(),
            "ACP has no standard subagent concept; subagents are not shown for Cursor.".into(),
            "Cursor CLI could not be executed during research; Phase 5 verifies on Windows 11.".into(),
        ],
    }
}

#[async_trait]
impl ProviderAdapter for CursorAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new(PROVIDER_ID),
            display_name: "Cursor CLI".into(),
            accent_color: "#6e8efb".into(),
            badge: "CU".into(),
            // `cursor-agent` first: `agent` is a generic name another tool could use.
            executable_names: vec!["cursor-agent".into(), "agent".into()],
            homepage: Some("https://cursor.com/docs/cli/overview".into()),
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
        assert_eq!(caps.managed.subagents, Unsupported);
        assert_eq!(caps.managed.usage, Runtime);
        assert_eq!(caps.external.attach, Experimental);
        assert_eq!(caps.external.permissions, Unsupported);
    }
}
