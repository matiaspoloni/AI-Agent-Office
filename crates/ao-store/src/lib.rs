//! Local persistence for Agent Office (SQLite, WAL mode).
//!
//! * [`Store`] — synchronous access (reads, project CRUD, preferences).
//! * [`writer::StoreWriter`] — a background thread that batches event and
//!   session writes into one transaction per flush.

mod migrations;
pub mod writer;

use ao_core::event::AgentEvent;
use ao_core::time::now_ms;
use ao_core::world::{AgentState, SessionState, WorldSnapshot};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

pub use migrations::latest_version;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repository_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_branch: Option<String>,
    /// Free-form workspace settings (provider preferences, etc.).
    #[ts(type = "Record<string, unknown>")]
    pub settings: serde_json::Value,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewProject {
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StoreStats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    #[ts(type = "number")]
    pub schema_version: i64,
    #[ts(type = "number")]
    pub latest_schema_version: i64,
    #[ts(type = "number")]
    pub projects: i64,
    #[ts(type = "number")]
    pub sessions: i64,
    #[ts(type = "number")]
    pub events: i64,
    #[ts(type = "number")]
    pub size_bytes: i64,
    pub integrity_ok: bool,
}

pub struct Store {
    conn: Connection,
    path: Option<PathBuf>,
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

impl Store {
    /// Opens (creating if needed) the database file and applies migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                StoreError::Invalid(format!("cannot create {}: {e}", parent.display()))
            })?;
        }
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        configure(&conn)?;
        let mut store = Self {
            conn,
            path: Some(path),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        configure(&conn)?;
        let mut store = Self { conn, path: None };
        store.migrate()?;
        Ok(store)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn migrate(&mut self) -> Result<()> {
        let current = self.schema_version()?;
        let latest = migrations::latest_version();
        if current > latest {
            return Err(StoreError::Invalid(format!(
                "database schema v{current} is newer than this app (v{latest}); refusing to open"
            )));
        }
        for (index, sql) in migrations::MIGRATIONS.iter().enumerate() {
            let version = index as i64 + 1;
            if version <= current {
                continue;
            }
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
            tracing::info!(version, "applied database migration");
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    // ------------------------------------------------------------------
    // Projects
    // ------------------------------------------------------------------

    fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Project, String)> {
        Ok((
            Project {
                id: row.get("id")?,
                name: row.get("name")?,
                path: row.get("path")?,
                repository_path: row.get("repository_path")?,
                default_provider: row.get("default_provider")?,
                default_model: row.get("default_model")?,
                default_branch: row.get("default_branch")?,
                settings: serde_json::Value::Null,
                created_at: row.get("created_at")?,
                updated_at: row.get("updated_at")?,
            },
            row.get("settings_json")?,
        ))
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT * FROM projects ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], Self::row_to_project)?;
        let mut out = Vec::new();
        for row in rows {
            let (mut project, settings) = row?;
            project.settings =
                serde_json::from_str(&settings).unwrap_or_else(|_| serde_json::json!({}));
            out.push(project);
        }
        Ok(out)
    }

    pub fn get_project(&self, id: &str) -> Result<Option<Project>> {
        Ok(self.list_projects()?.into_iter().find(|p| p.id == id))
    }

    pub fn add_project(&self, new: NewProject) -> Result<Project> {
        let name = new.name.trim();
        let path = new.path.trim();
        if name.is_empty() || path.is_empty() {
            return Err(StoreError::Invalid(
                "project name and path are required".into(),
            ));
        }
        let now = now_ms();
        let project = Project {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_owned(),
            path: path.to_owned(),
            repository_path: None,
            default_provider: new.default_provider,
            default_model: new.default_model,
            default_branch: new.default_branch,
            settings: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        };
        let inserted = self.conn.execute(
            "INSERT INTO projects (id, name, path, repository_path, default_provider, default_model, default_branch, settings_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                project.id,
                project.name,
                project.path,
                project.repository_path,
                project.default_provider,
                project.default_model,
                project.default_branch,
                project.settings.to_string(),
                project.created_at,
                project.updated_at
            ],
        );
        match inserted {
            Ok(_) => Ok(project),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StoreError::Invalid(format!(
                    "a project already uses the folder {path}"
                )))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn update_project(&self, project: &Project) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE projects SET name = ?2, path = ?3, repository_path = ?4, default_provider = ?5,
                 default_model = ?6, default_branch = ?7, settings_json = ?8, updated_at = ?9
             WHERE id = ?1",
            params![
                project.id,
                project.name,
                project.path,
                project.repository_path,
                project.default_provider,
                project.default_model,
                project.default_branch,
                project.settings.to_string(),
                now_ms()
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::Invalid(format!(
                "project {} not found",
                project.id
            )));
        }
        Ok(())
    }

    pub fn remove_project(&self, id: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM projects WHERE id = ?1", [id])?
            > 0)
    }

    // ------------------------------------------------------------------
    // Sessions, agents, events
    // ------------------------------------------------------------------

    /// Writes events, sessions and agents in a single transaction.
    pub fn write_batch(
        &mut self,
        events: &[AgentEvent],
        sessions: &[SessionState],
        agents: &[AgentState],
    ) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut inserted = 0;
        {
            let mut upsert_session = tx.prepare_cached(
                "INSERT INTO sessions (key, provider, session_id, mode, status, project_id, cwd, model, title, started_at, ended_at, last_event_at, state_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, (SELECT id FROM projects WHERE id = ?6), ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(key) DO UPDATE SET mode = excluded.mode, status = excluded.status,
                    project_id = excluded.project_id, cwd = excluded.cwd, model = excluded.model,
                    title = excluded.title, ended_at = excluded.ended_at,
                    last_event_at = excluded.last_event_at, state_json = excluded.state_json",
            )?;
            for s in sessions {
                upsert_session.execute(params![
                    s.key,
                    s.provider.0,
                    s.session_id.0,
                    serde_json::to_value(s.mode)?.as_str().unwrap_or("external"),
                    serde_json::to_value(s.status)?.as_str().unwrap_or("active"),
                    s.project_id.as_ref().map(|p| p.0.clone()),
                    s.cwd,
                    s.model,
                    s.title,
                    s.started_at,
                    s.ended_at,
                    s.last_event_at,
                    serde_json::to_string(s)?
                ])?;
            }

            let mut upsert_agent = tx.prepare_cached(
                "INSERT INTO agents (key, session_key, agent_id, parent_key, name, state_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(key) DO UPDATE SET parent_key = excluded.parent_key, name = excluded.name,
                    state_json = excluded.state_json, updated_at = excluded.updated_at",
            )?;
            for a in agents {
                // Agents reference their session; skip orphans instead of failing the batch.
                let session_exists: bool = tx
                    .prepare_cached("SELECT 1 FROM sessions WHERE key = ?1")?
                    .exists([&a.session_key])?;
                if !session_exists {
                    continue;
                }
                upsert_agent.execute(params![
                    a.key,
                    a.session_key,
                    a.agent_id.0,
                    a.parent_key,
                    a.name,
                    serde_json::to_string(a)?,
                    now_ms()
                ])?;
            }

            let mut insert_event = tx.prepare_cached(
                "INSERT OR IGNORE INTO events (event_id, ts, provider, session_key, agent_id, type, source, json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for e in events {
                inserted += insert_event.execute(params![
                    e.event_id.0,
                    e.timestamp,
                    e.provider.0,
                    ao_core::ids::session_key(&e.provider, &e.session_id),
                    e.agent_id.0,
                    e.type_name(),
                    serde_json::to_value(e.source)?
                        .as_str()
                        .unwrap_or("internal"),
                    serde_json::to_string(e)?
                ])?;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// Most recent events of a session, oldest first.
    pub fn recent_events(&self, session_key: &str, limit: usize) -> Result<Vec<AgentEvent>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT json FROM (SELECT seq, json FROM events WHERE session_key = ?1 ORDER BY seq DESC LIMIT ?2)
             ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![session_key, limit as i64], |r| {
            r.get::<_, String>(0)
        })?;
        let mut out = Vec::new();
        for json in rows {
            match serde_json::from_str::<AgentEvent>(&json?) {
                Ok(e) => out.push(e),
                Err(err) => tracing::warn!(%err, "skipping unreadable stored event"),
            }
        }
        Ok(out)
    }

    /// Sessions (with their agents) active since `since_ms`, used to restore state at startup.
    pub fn load_world_since(&self, since_ms: i64) -> Result<WorldSnapshot> {
        let mut snapshot = WorldSnapshot::default();
        let mut stmt = self.conn.prepare_cached(
            "SELECT state_json FROM sessions WHERE last_event_at >= ?1 ORDER BY last_event_at",
        )?;
        for json in stmt.query_map([since_ms], |r| r.get::<_, String>(0))? {
            if let Ok(s) = serde_json::from_str::<SessionState>(&json?) {
                snapshot.sessions.push(s);
            }
        }
        let mut stmt = self.conn.prepare_cached(
            "SELECT a.state_json FROM agents a JOIN sessions s ON s.key = a.session_key WHERE s.last_event_at >= ?1",
        )?;
        for json in stmt.query_map([since_ms], |r| r.get::<_, String>(0))? {
            if let Ok(a) = serde_json::from_str::<AgentState>(&json?) {
                snapshot.agents.push(a);
            }
        }
        Ok(snapshot)
    }

    /// Deletes events older than `before_ms`. Returns the number removed.
    pub fn purge_events_before(&self, before_ms: i64) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM events WHERE ts < ?1", [before_ms])?)
    }

    // ------------------------------------------------------------------
    // Preferences
    // ------------------------------------------------------------------

    pub fn get_preference<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM preferences WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        match json {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    pub fn set_preference<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn.execute(
            "INSERT INTO preferences (key, value_json, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
            params![key, serde_json::to_string(value)?, now_ms()],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------

    pub fn stats(&self) -> Result<StoreStats> {
        let count = |table: &str| -> Result<i64> {
            Ok(self
                .conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
        };
        let page_count: i64 = self.conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let page_size: i64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        let integrity: String = self
            .conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        Ok(StoreStats {
            path: self.path.as_ref().map(|p| p.display().to_string()),
            schema_version: self.schema_version()?,
            latest_schema_version: latest_version(),
            projects: count("projects")?,
            sessions: count("sessions")?,
            events: count("events")?,
            size_bytes: page_count * page_size,
            integrity_ok: integrity == "ok",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ao_core::event::*;
    use ao_core::world::WorldState;

    fn sample_events() -> Vec<AgentEvent> {
        vec![
            AgentEvent::for_session(
                "claude",
                "s1",
                EventSource::Hook,
                EventKind::SessionStarted(SessionInfo {
                    cwd: Some("C:\\p".into()),
                    ..Default::default()
                }),
            )
            .at(100),
            AgentEvent::for_session(
                "claude",
                "s1",
                EventSource::Hook,
                EventKind::ToolStarted(ToolStarted {
                    tool_call_id: Some("t1".into()),
                    tool_name: "Read".into(),
                    category: ToolCategory::Read,
                    title: None,
                }),
            )
            .at(200),
        ]
    }

    #[test]
    fn migrates_fresh_database() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), latest_version());
        let stats = store.stats().unwrap();
        assert!(stats.integrity_ok);
        assert_eq!(stats.events, 0);
    }

    #[test]
    fn project_crud_and_unique_path() {
        let store = Store::open_in_memory().unwrap();
        let p = store
            .add_project(NewProject {
                name: "Nalu".into(),
                path: "C:\\Nalu".into(),
                ..Default::default()
            })
            .unwrap();
        assert!(store
            .add_project(NewProject {
                name: "Dup".into(),
                path: "C:\\Nalu".into(),
                ..Default::default()
            })
            .is_err());
        assert!(store
            .add_project(NewProject {
                name: " ".into(),
                path: "x".into(),
                ..Default::default()
            })
            .is_err());

        let mut updated = p.clone();
        updated.default_model = Some("opus".into());
        store.update_project(&updated).unwrap();
        let listed = store.list_projects().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].default_model.as_deref(), Some("opus"));
        assert!(store.remove_project(&p.id).unwrap());
        assert!(store.list_projects().unwrap().is_empty());
    }

    #[test]
    fn writes_and_restores_sessions_and_events() {
        let mut store = Store::open_in_memory().unwrap();
        let events = sample_events();
        let mut world = WorldState::new();
        for e in &events {
            world.apply(e);
        }
        let snap = world.snapshot();
        assert_eq!(
            store
                .write_batch(&events, &snap.sessions, &snap.agents)
                .unwrap(),
            2
        );
        // Idempotent on replay.
        assert_eq!(
            store
                .write_batch(&events, &snap.sessions, &snap.agents)
                .unwrap(),
            0
        );

        let recent = store.recent_events("claude:s1", 10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].timestamp, 100);
        assert_eq!(recent, events);

        let restored = store.load_world_since(0).unwrap();
        assert_eq!(restored.sessions.len(), 1);
        assert_eq!(restored.agents.len(), 1);
        assert_eq!(restored.agents[0].tool_calls, 1);

        assert_eq!(store.purge_events_before(150).unwrap(), 1);
        assert_eq!(store.recent_events("claude:s1", 10).unwrap().len(), 1);
    }

    #[test]
    fn preferences_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.get_preference::<u32>("retention_days").unwrap(), None);
        store.set_preference("retention_days", &14u32).unwrap();
        store.set_preference("retention_days", &30u32).unwrap();
        assert_eq!(
            store.get_preference::<u32>("retention_days").unwrap(),
            Some(30)
        );
    }

    #[test]
    fn opens_file_database_in_wal_mode() {
        let dir = std::env::temp_dir().join(format!("ao-store-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("nested").join("agent-office.db");
        {
            let store = Store::open(&path).unwrap();
            let mode: String = store
                .conn
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode.to_lowercase(), "wal");
        }
        // Re-opening does not re-run migrations.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), latest_version());
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }
}
