//! Low-level SQLite adapter shared by the audit and token-usage
//! writers.
//!
//! This module is internal to `store/`. External callers write
//! through `store::audit::SqliteAuditSink` (audit_events table) or
//! `store::token_usage::SqliteTokenUsageSink` (token_usage_samples
//! table — Phase F F-3, v1.24.0). The audit writer carries the
//! type-level guarantee that only `AuditEvent` reaches its table
//! (r2 CSF-2 / Gemini G-8).

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SqliteError {
    #[error("sqlite open: {0}")]
    Open(String),
    #[error("sqlite query: {0}")]
    Query(String),
}

pub const AUDIT_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS audit_events (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_utc       TEXT    NOT NULL,
    kind         TEXT    NOT NULL,
    severity     TEXT    NOT NULL,
    pane_id      TEXT    NOT NULL,
    provider     TEXT,
    role         TEXT,
    summary      TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_events(ts_utc);
CREATE INDEX IF NOT EXISTS idx_audit_kind ON audit_events(kind);
";

pub const TOKEN_USAGE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS token_usage_samples (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_unix_ms            INTEGER NOT NULL,
    pane_id               TEXT    NOT NULL,
    provider              TEXT    NOT NULL,
    input_tokens          INTEGER,
    output_tokens         INTEGER,
    cost_usd              REAL,
    cached_input_tokens   INTEGER
);
CREATE INDEX IF NOT EXISTS idx_token_usage_pane_ts
    ON token_usage_samples(pane_id, ts_unix_ms DESC);
";

pub const ANOMALY_EVENTS_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS anomaly_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_unix_secs    INTEGER NOT NULL,
    pane_id         TEXT    NOT NULL,
    kind            TEXT    NOT NULL,
    confidence      TEXT    NOT NULL,
    severity        TEXT    NOT NULL,
    promoted        INTEGER NOT NULL,
    reason          TEXT    NOT NULL DEFAULT '',
    evidence_json   TEXT    NOT NULL DEFAULT '[]'
);
CREATE INDEX IF NOT EXISTS idx_anomaly_events_ts
    ON anomaly_events(ts_unix_secs DESC);
CREATE INDEX IF NOT EXISTS idx_anomaly_events_pane_ts
    ON anomaly_events(pane_id, ts_unix_secs DESC);
";

pub const ANOMALY_HISTORY_SNAPSHOTS_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS anomaly_history_snapshots (
    pane_id                    TEXT    NOT NULL,
    tick_unix_secs             INTEGER NOT NULL,
    identity_provider          TEXT,
    identity_path              TEXT,
    error_hint                 INTEGER,
    cache_hit_ratio            REAL,
    cache_drift_fire           INTEGER,
    cross_pane_edit_paths_json TEXT,
    cost_usd                   REAL,
    input_tokens               INTEGER,
    output_tokens              INTEGER,
    process_memory_mb          REAL,
    agent_memory_bytes         INTEGER,
    subagent_hint              INTEGER,
    PRIMARY KEY (pane_id, tick_unix_secs)
);
CREATE INDEX IF NOT EXISTS idx_anomaly_history_pane_tick
    ON anomaly_history_snapshots(pane_id, tick_unix_secs DESC);
";

pub const COST_USAGE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS cost_usage_events (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_unix_ms            INTEGER NOT NULL,
    pane_id               TEXT    NOT NULL,
    provider              TEXT    NOT NULL,
    cost_usd_delta        REAL    NOT NULL,
    cumulative_cost_usd   REAL    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cost_usage_ts
    ON cost_usage_events(ts_unix_ms DESC);
CREATE INDEX IF NOT EXISTS idx_cost_usage_pane_ts
    ON cost_usage_events(pane_id, ts_unix_ms DESC);

CREATE TABLE IF NOT EXISTS cost_budget_alerts (
    threshold_bps         INTEGER NOT NULL,
    budget_cents          INTEGER NOT NULL,
    fired_at_unix_ms      INTEGER NOT NULL,
    spent_usd             REAL    NOT NULL,
    budget_usd            REAL    NOT NULL,
    PRIMARY KEY (threshold_bps, budget_cents)
);
";

pub struct AuditDb {
    conn: Mutex<Connection>,
}

impl AuditDb {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SqliteError::Open(e.to_string()))?;
        }
        let conn = Connection::open(path).map_err(|e| SqliteError::Open(e.to_string()))?;
        conn.execute_batch(AUDIT_SCHEMA)
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        conn.execute_batch(TOKEN_USAGE_SCHEMA)
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        conn.execute_batch(COST_USAGE_SCHEMA)
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        conn.execute_batch(ANOMALY_EVENTS_SCHEMA)
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        conn.execute_batch(ANOMALY_HISTORY_SNAPSHOTS_SCHEMA)
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        // F-4 (v1.25.0): existing DBs from v1.24.0 lack the
        // cached_input_tokens column. ALTER returns "duplicate column"
        // if it already exists; we swallow that explicitly so the
        // migration is idempotent across cold and warm starts.
        if let Err(e) = conn.execute(
            "ALTER TABLE token_usage_samples ADD COLUMN cached_input_tokens INTEGER",
            [],
        ) {
            // "duplicate column name" is the expected error on warm starts
            let msg = e.to_string();
            if !msg.contains("duplicate column") {
                return Err(SqliteError::Query(msg));
            }
        }
        if let Err(e) = conn.execute(
            "ALTER TABLE anomaly_events ADD COLUMN evidence_json TEXT NOT NULL DEFAULT '[]'",
            [],
        ) {
            let msg = e.to_string();
            if !msg.contains("duplicate column") {
                return Err(SqliteError::Query(msg));
            }
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn connection(&self) -> &Mutex<Connection> {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_db_open_creates_anomaly_tables() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("audit.db");
        let _db = AuditDb::open(&path).expect("open ok");
        let conn = rusqlite::Connection::open(&path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table'
                 AND name IN ('anomaly_events', 'anomaly_history_snapshots')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn audit_db_open_is_idempotent_for_anomaly_tables() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("audit.db");
        let _db1 = AuditDb::open(&path).expect("first open ok");
        let _db2 = AuditDb::open(&path).expect("second open ok (no error on reopen)");
    }
}
