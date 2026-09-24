//! Claude Code adapter.
//!
//! Phase 1 implements detection only. Capabilities below describe what Claude
//! Code offers (docs/PROVIDER_CAPABILITIES.md §3); `implemented` says what this
//! build can already use. Hooks, managed sessions (`claude -p` stream-json) and
//! `claude agents --json` arrive in Phase 3.

use ao_core::capabilities::{Capabilities, CapabilityProfile, ImplementedFeatures, Support::*};
use ao_core::ids::ProviderId;
use ao_core::provider::{InstallationInfo, ProviderAdapter, ProviderDescriptor};
use async_trait::async_trait;

pub const PROVIDER_ID: &str = "claude";

/// Install locations checked after `PATH`.
const EXTRA_DIRS: &[&str] = if cfg!(windows) {
    &[
        "%USERPROFILE%\\.local\\bin", // native installer
        "%APPDATA%\\npm",             // npm global install (claude.cmd)
    ]
} else {
    &[
        "~/.local/bin",
        "~/.claude/local",
        "/usr/local/bin",
        "/opt/homebrew/bin",
    ]
};

#[derive(Debug, Default)]
pub struct ClaudeAdapter;

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self
    }
}

pub fn capability_profile() -> CapabilityProfile {
    CapabilityProfile {
        managed: Capabilities {
            launch: Supported,
            attach: Unsupported,
            list_sessions: Supported,
            stop: Supported,
            send_prompt: Supported,
            structured_events: Supported,
            tool_events: Supported,
            file_events: Partial,
            command_events: Supported,
            permissions: Supported,
            subagents: Supported,
            usage: Supported,
            cost: Partial,
            model: Supported,
            context_compaction: Supported,
            resume: Supported,
        },
        external: Capabilities {
            launch: Unsupported,
            attach: Supported,
            list_sessions: Supported,
            stop: Partial,
            send_prompt: Unsupported,
            structured_events: Supported,
            tool_events: Supported,
            file_events: Partial,
            command_events: Supported,
            permissions: Partial,
            subagents: Supported,
            usage: Unsupported,
            cost: Unsupported,
            model: Partial,
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
            "External sessions are observed through user-level hooks in %USERPROFILE%\\.claude\\settings.json.".into(),
            "File events are tool-level evidence (Edit/Write/Read inputs), not a filesystem watch.".into(),
            "Cost is Claude Code's client-side estimate (total_cost_usd) and is labelled as such.".into(),
            "Stop for external sessions only applies to background sessions (claude stop <id>).".into(),
            "Approving from Agent Office in external sessions is opt-in (PermissionRequest hook).".into(),
        ],
    }
}

#[async_trait]
impl ProviderAdapter for ClaudeAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new(PROVIDER_ID),
            display_name: "Claude Code".into(),
            accent_color: "#d97757".into(),
            badge: "CC".into(),
            executable_names: vec!["claude".into()],
            homepage: Some("https://code.claude.com/docs".into()),
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
        assert_eq!(caps.managed.launch, Supported);
        assert_eq!(caps.external.send_prompt, Unsupported);
        assert_eq!(
            caps.external.usage, Unsupported,
            "hooks carry no token usage"
        );
        assert_eq!(caps.managed.cost, Partial, "cost is an estimate");
        assert!(caps.implemented.detection);
        assert!(
            !caps.implemented.managed_sessions,
            "not implemented until Phase 3"
        );
    }

    #[tokio::test]
    async fn unimplemented_actions_are_reported_not_faked() {
        let adapter = ClaudeAdapter::new();
        assert!(adapter.send_prompt(&"s".into(), "hello").await.is_err());
    }
}
