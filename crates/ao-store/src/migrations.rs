//! Schema migrations, tracked with `PRAGMA user_version`.
//! Never edit a released migration; append a new one.

pub const MIGRATIONS: &[&str] = &[
    // v1 — initial schema
    r#"
    CREATE TABLE projects (
        id              TEXT PRIMARY KEY,
        name            TEXT NOT NULL,
        path            TEXT NOT NULL UNIQUE,
        repository_path TEXT,
        default_provider TEXT,
        default_model   TEXT,
        default_branch  TEXT,
        settings_json   TEXT NOT NULL DEFAULT '{}',
        created_at      INTEGER NOT NULL,
        updated_at      INTEGER NOT NULL
    );

    CREATE TABLE sessions (
        key            TEXT PRIMARY KEY,
        provider       TEXT NOT NULL,
        session_id     TEXT NOT NULL,
        mode           TEXT NOT NULL,
        status         TEXT NOT NULL,
        project_id     TEXT REFERENCES projects(id) ON DELETE SET NULL,
        cwd            TEXT,
        model          TEXT,
        title          TEXT,
        started_at     INTEGER NOT NULL,
        ended_at       INTEGER,
        last_event_at  INTEGER NOT NULL,
        state_json     TEXT NOT NULL
    );
    CREATE INDEX idx_sessions_last_event ON sessions(last_event_at);

    CREATE TABLE agents (
        key          TEXT PRIMARY KEY,
        session_key  TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
        agent_id     TEXT NOT NULL,
        parent_key   TEXT,
        name         TEXT NOT NULL,
        state_json   TEXT NOT NULL,
        updated_at   INTEGER NOT NULL
    );
    CREATE INDEX idx_agents_session ON agents(session_key);

    CREATE TABLE events (
        seq         INTEGER PRIMARY KEY AUTOINCREMENT,
        event_id    TEXT NOT NULL UNIQUE,
        ts          INTEGER NOT NULL,
        provider    TEXT NOT NULL,
        session_key TEXT NOT NULL,
        agent_id    TEXT NOT NULL,
        type        TEXT NOT NULL,
        source      TEXT NOT NULL,
        json        TEXT NOT NULL
    );
    CREATE INDEX idx_events_session_ts ON events(session_key, ts);
    CREATE INDEX idx_events_ts ON events(ts);

    CREATE TABLE preferences (
        key         TEXT PRIMARY KEY,
        value_json  TEXT NOT NULL,
        updated_at  INTEGER NOT NULL
    );

    CREATE TABLE layouts (
        id          TEXT PRIMARY KEY,
        name        TEXT NOT NULL,
        layout_json TEXT NOT NULL,
        is_active   INTEGER NOT NULL DEFAULT 0,
        updated_at  INTEGER NOT NULL
    );

    CREATE TABLE integrations (
        provider    TEXT PRIMARY KEY,
        status_json TEXT NOT NULL,
        updated_at  INTEGER NOT NULL
    );

    CREATE TABLE usage_snapshots (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        session_key TEXT NOT NULL,
        ts          INTEGER NOT NULL,
        usage_json  TEXT NOT NULL
    );
    CREATE INDEX idx_usage_session ON usage_snapshots(session_key, ts);
    "#,
];

pub fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}
