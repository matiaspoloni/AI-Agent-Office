//! Strongly typed identifiers. All of them serialize as plain strings.

use serde::{Deserialize, Serialize};
use std::fmt;
use ts_rs::TS;

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
        #[ts(export)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

string_id!(
    /// Stable provider identifier, e.g. `claude`, `codex`, `cursor`, `demo`.
    ProviderId
);
string_id!(
    /// Session id as reported by the provider (Claude `session_id`, Codex `threadId`, ACP `sessionId`).
    SessionId
);
string_id!(
    /// Agent id inside a session. The main agent uses the session id; subagents use the provider's id.
    AgentId
);
string_id!(
    /// Agent Office project id.
    ProjectId
);
string_id!(
    /// Repository id (normalized repository root path).
    RepositoryId
);
string_id!(
    /// Unique event id generated at ingestion.
    EventId
);
string_id!(
    /// Provider-scoped tool call id (Claude `tool_use_id`, Codex item id, ACP `toolCallId`).
    ToolCallId
);
string_id!(
    /// Id of a pending permission request, unique within a session.
    PermissionRequestId
);

impl EventId {
    pub fn random() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

/// Key of a session across providers: `provider:session_id`.
pub fn session_key(provider: &ProviderId, session: &SessionId) -> String {
    format!("{}:{}", provider.0, session.0)
}

/// Key of an agent across providers and sessions: `provider:session_id:agent_id`.
pub fn agent_key(provider: &ProviderId, session: &SessionId, agent: &AgentId) -> String {
    format!("{}:{}:{}", provider.0, session.0, agent.0)
}
