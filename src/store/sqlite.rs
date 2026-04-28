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
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_unix_ms    INTEGER NOT NULL,
    pane_id       TEXT    NOT NULL,
    provider      TEXT    NOT NULL,
    input_tokens  INTEGER,
    output_tokens INTEGER,
    cost_usd      REAL
);
CREATE INDEX IF NOT EXISTS idx_token_usage_pane_ts
    ON token_usage_samples(pane_id, ts_unix_ms DESC);
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
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn connection(&self) -> &Mutex<Connection> {
        &self.conn
    }
}
