//! Low-level SQLite adapter shared by the audit writer.
//!
//! This module is internal to `store/`. All external callers write
//! through `store::audit::SqliteAuditSink`, which carries the
//! type-level guarantee that only `AuditEvent` reaches the DB (r2
//! CSF-2 / Gemini G-8).

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

pub(crate) struct AuditDb {
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
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn connection(&self) -> &Mutex<Connection> {
        &self.conn
    }
}
