//! User preferences persisted in the `preferences` table.

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
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            retention_days: 14,
            max_output_chars: 8 * 1024,
            store_prompts: true,
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
        self
    }

    pub fn sanitize_limits(&self) -> SanitizeLimits {
        SanitizeLimits {
            max_string_chars: self.max_output_chars as usize,
            store_prompts: self.store_prompts,
        }
    }
}
