//! Safe edits of provider configuration files (Claude `settings.json`,
//! Codex `hooks.json`, …) shared by every adapter:
//!
//! * the file is parsed first — invalid JSON is reported and **never written**;
//! * every write is preceded by a timestamped backup next to the file
//!   (`<name>.agent-office-backup-<ms>`, the last [`KEEP_BACKUPS`] are kept);
//! * the new content goes to a temporary file that is renamed over the
//!   original, so a crash never leaves a half-written config.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const KEEP_BACKUPS: usize = 5;
const BACKUP_MARKER: &str = ".agent-office-backup-";
const TMP_SUFFIX: &str = ".agent-office-tmp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonConfigFile {
    pub path: PathBuf,
}

impl JsonConfigFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "config.json".into())
    }

    /// Prefix of this file's backups (`settings.json.agent-office-backup-`).
    pub fn backup_prefix(&self) -> String {
        format!("{}{BACKUP_MARKER}", self.file_name())
    }

    /// `Ok(None)` when the file does not exist; `Err` when it is unreadable or
    /// not JSON. An empty file reads as `{}`.
    pub fn read(&self) -> Result<Option<Value>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) if text.trim().is_empty() => Ok(Some(json!({}))),
            Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| {
                format!(
                    "{} is not valid JSON ({e}); Agent Office will not modify it",
                    self.path.display()
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read {}: {e}", self.path.display())),
        }
    }

    fn backups(&self) -> Vec<PathBuf> {
        let Some(dir) = self.path.parent() else {
            return Vec::new();
        };
        let prefix = self.backup_prefix();
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
                    })
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    }

    pub fn backup_count(&self) -> usize {
        self.backups().len()
    }

    /// Backs up the current file (if any), then writes `value` atomically.
    /// Returns the backup path.
    pub fn write(&self, value: &Value) -> Result<Option<PathBuf>, String> {
        let dir = self
            .path
            .parent()
            .ok_or("config path has no parent folder")?;
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let mut backup = None;
        if self.path.exists() {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or_default();
            let target = unique(dir, &format!("{}{stamp}", self.backup_prefix()));
            std::fs::copy(&self.path, &target)
                .map_err(|e| format!("backup failed, nothing changed: {e}"))?;
            backup = Some(target);
            let backups = self.backups();
            if backups.len() > KEEP_BACKUPS {
                for old in &backups[..backups.len() - KEEP_BACKUPS] {
                    let _ = std::fs::remove_file(old);
                }
            }
        }
        let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
        text.push('\n');
        let tmp = dir.join(format!("{}{TMP_SUFFIX}", self.file_name()));
        std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("cannot replace {}: {e}", self.path.display())
        })?;
        Ok(backup)
    }
}

/// Two writes in the same millisecond must not overwrite each other's backup.
fn unique(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    (1..)
        .map(|n| dir.join(format!("{name}-{n}")))
        .find(|p| !p.exists())
        .expect("free backup name")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ao-config-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn roundtrip_backups_and_invalid_json_protection() {
        let dir = temp("roundtrip");
        let file = JsonConfigFile::new(dir.join("hooks.json"));
        assert_eq!(file.read().unwrap(), None);
        assert_eq!(
            file.write(&json!({"a": 1})).unwrap(),
            None,
            "no backup for a new file"
        );
        assert_eq!(file.read().unwrap(), Some(json!({"a": 1})));
        for i in 0..7 {
            assert!(file.write(&json!({"a": i})).unwrap().is_some());
        }
        assert_eq!(file.backup_count(), KEEP_BACKUPS);
        assert!(file
            .backup_prefix()
            .starts_with("hooks.json.agent-office-backup-"));

        std::fs::write(&file.path, "{ not json").unwrap();
        assert!(file.read().unwrap_err().contains("not valid JSON"));
        std::fs::write(&file.path, "  ").unwrap();
        assert_eq!(file.read().unwrap(), Some(json!({})));
        let _ = std::fs::remove_dir_all(dir);
    }
}
