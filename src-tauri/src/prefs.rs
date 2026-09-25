//! User preferences persisted in the `preferences` table.

use ao_core::provider::ProviderSettings;
use ao_core::sanitize::SanitizeLimits;
use ao_store::Store;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const KEY: &str = "app.preferences";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Preferences {
    /// Events older than this are deleted from the local database.
    pub retention_days: u32,
    /// Maximum characters kept per string in stored events (tool output etc.).
    pub max_output_chars: u32,
    /// Store prompt text. When off, only the fact that a prompt was sent is kept.
    pub store_prompts: bool,
    /// External sessions: permission requests wait for an answer in Agent
    /// Office before Claude shows its own prompt. Off = observe only.
    pub answer_permissions_from_app: bool,
    /// Seconds Agent Office waits for Approve/Reject.
    pub permission_timeout_secs: u32,
    /// Poll official listing commands (`claude agents --json`) for sessions.
    pub discover_external_sessions: bool,
    /// Warn when a busy agent has been silent this many minutes (0 = never).
    /// Only a warning: quiet sessions are never stopped automatically.
    pub silence_warning_minutes: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            retention_days: 14,
            max_output_chars: 8 * 1024,
            store_prompts: true,
            answer_permissions_from_app: false,
            permission_timeout_secs: 120,
            discover_external_sessions: true,
            silence_warning_minutes: 5,
        }
    }
}

impl Preferences {
    pub fn load(store: &Store) -> Self {
        store
            .get_preference::<Preferences>(KEY)
            .ok()
            .flatten()
            .unwrap_or_default()
            .normalized()
    }

    pub fn save(&self, store: &Store) -> ao_store::Result<()> {
        store.set_preference(KEY, self)
    }

    pub fn normalized(mut self) -> Self {
        self.retention_days = self.retention_days.clamp(1, 365);
        self.max_output_chars = self.max_output_chars.clamp(256, 256 * 1024);
        self.permission_timeout_secs = self.permission_timeout_secs.clamp(10, 3600);
        self.silence_warning_minutes = self.silence_warning_minutes.min(24 * 60);
        self
    }

    pub fn provider_settings(&self) -> ProviderSettings {
        ProviderSettings {
            answer_permissions_from_app: self.answer_permissions_from_app,
            permission_timeout_secs: self.permission_timeout_secs,
            discover_external_sessions: self.discover_external_sessions,
        }
    }

    pub fn sanitize_limits(&self) -> SanitizeLimits {
        SanitizeLimits {
            max_string_chars: self.max_output_chars as usize,
            store_prompts: self.store_prompts,
        }
    }
}
