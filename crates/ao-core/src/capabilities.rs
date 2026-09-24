//! Explicit, honest capability declarations. See docs/PROVIDER_CAPABILITIES.md.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How well a provider supports a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Support {
    /// Official, documented and verified.
    Supported,
    /// Available with a documented limitation.
    Partial,
    /// Exists upstream but experimental or reported incomplete.
    Experimental,
    /// Negotiated per session at runtime (e.g. ACP `initialize`).
    Runtime,
    /// Not available. Never simulated.
    Unsupported,
}

impl Support {
    /// Whether the UI may offer the feature (possibly with a badge).
    pub fn is_available(self) -> bool {
        !matches!(self, Support::Unsupported)
    }
}

/// Capability set for one integration mode (managed or external sessions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Capabilities {
    pub launch: Support,
    pub attach: Support,
    pub list_sessions: Support,
    pub stop: Support,
    pub send_prompt: Support,
    pub structured_events: Support,
    pub tool_events: Support,
    pub file_events: Support,
    pub command_events: Support,
    pub permissions: Support,
    pub subagents: Support,
    pub usage: Support,
    pub cost: Support,
    pub model: Support,
    pub context_compaction: Support,
    pub resume: Support,
}

impl Capabilities {
    /// Everything unsupported. Adapters start from here and opt in explicitly.
    pub const NONE: Capabilities = Capabilities {
        launch: Support::Unsupported,
        attach: Support::Unsupported,
        list_sessions: Support::Unsupported,
        stop: Support::Unsupported,
        send_prompt: Support::Unsupported,
        structured_events: Support::Unsupported,
        tool_events: Support::Unsupported,
        file_events: Support::Unsupported,
        command_events: Support::Unsupported,
        permissions: Support::Unsupported,
        subagents: Support::Unsupported,
        usage: Support::Unsupported,
        cost: Support::Unsupported,
        model: Support::Unsupported,
        context_compaction: Support::Unsupported,
        resume: Support::Unsupported,
    };
}

/// Capabilities for both integration modes plus human-readable notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CapabilityProfile {
    pub managed: Capabilities,
    pub external: Capabilities,
    /// Whether the adapter itself is implemented in this build. Capabilities
    /// describe what the provider offers; `implemented` says what Agent Office
    /// can already use. The UI enables actions only when both are true.
    pub implemented: ImplementedFeatures,
    pub notes: Vec<String>,
}

/// What this build of Agent Office has implemented for a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImplementedFeatures {
    pub detection: bool,
    pub managed_sessions: bool,
    pub external_sessions: bool,
    pub integration_setup: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_fully_unsupported() {
        let json = serde_json::to_value(Capabilities::NONE).unwrap();
        for (_, v) in json.as_object().unwrap() {
            assert_eq!(v, "unsupported");
        }
        assert!(!Support::Unsupported.is_available());
        assert!(Support::Experimental.is_available());
    }
}
