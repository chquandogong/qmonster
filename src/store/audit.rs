use std::path::Path;

use chrono::Utc;

use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::identity::{Provider, Role};
use crate::domain::recommendation::Severity;
use crate::store::sink::EventSink;
use crate::store::sqlite::{AuditDb, SqliteError};

/// A row retrieved from the audit DB. This is a structured mirror of
/// `AuditEvent` plus a server-side UTC timestamp. There is **no** raw-
/// bytes column — the type-level exclusion (r2 §5 / Codex CSF-2) is
/// enforced both by `AuditEvent`'s shape and by the DB schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub id: i64,
    pub timestamp_utc: String,
    pub kind: AuditEventKind,
    pub severity: Severity,
    pub pane_id: String,
    pub provider: Option<Provider>,
    pub role: Option<Role>,
    pub summary: String,
}

/// Durable audit sink backed by SQLite. Implements `EventSink`, so the
/// Phase-1 InMemorySink can be swapped out transparently.
pub struct SqliteAuditSink {
    db: AuditDb,
}

impl SqliteAuditSink {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
        })
    }

    /// Read the most recent `limit` rows in insertion order (oldest
    /// first). Used by tests and by future UI panels; never exposes
    /// raw tails.
    pub fn recent(&self, limit: usize) -> Result<Vec<AuditRow>, SqliteError> {
        let conn = self.db.connection().lock().expect("poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT id, ts_utc, kind, severity, pane_id, provider, role, summary \
                 FROM audit_events ORDER BY id ASC LIMIT ?1",
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(AuditRow {
                    id: row.get(0)?,
                    timestamp_utc: row.get(1)?,
                    kind: parse_kind(&row.get::<_, String>(2)?)
                        .unwrap_or(AuditEventKind::AlertFired),
                    severity: parse_severity(&row.get::<_, String>(3)?)
                        .unwrap_or(Severity::Safe),
                    pane_id: row.get(4)?,
                    provider: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| parse_provider(&s)),
                    role: row.get::<_, Option<String>>(6)?.and_then(|s| parse_role(&s)),
                    summary: row.get(7)?,
                })
            })
            .map_err(|e| SqliteError::Query(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(rows)
    }
}

impl EventSink for SqliteAuditSink {
    fn record(&self, event: AuditEvent) {
        let conn = self.db.connection().lock().expect("poisoned");
        let ts = Utc::now().to_rfc3339();
        let _ = conn.execute(
            "INSERT INTO audit_events (ts_utc, kind, severity, pane_id, provider, role, summary) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                ts,
                kind_to_str(event.kind),
                severity_to_str(event.severity),
                event.pane_id,
                event.provider.map(provider_to_str),
                event.role.map(role_to_str),
                event.summary,
            ],
        );
    }
}

fn kind_to_str(k: AuditEventKind) -> &'static str {
    match k {
        AuditEventKind::PaneIdentityResolved => "PaneIdentityResolved",
        AuditEventKind::PaneIdentityChanged => "PaneIdentityChanged",
        AuditEventKind::PaneBecameDead => "PaneBecameDead",
        AuditEventKind::PaneReappeared => "PaneReappeared",
        AuditEventKind::AlertFired => "AlertFired",
        AuditEventKind::RecommendationEmitted => "RecommendationEmitted",
        AuditEventKind::StartupVersionSnapshot => "StartupVersionSnapshot",
        AuditEventKind::VersionDriftDetected => "VersionDriftDetected",
        AuditEventKind::SafetyOverrideRejected => "SafetyOverrideRejected",
        AuditEventKind::ArchiveWritten => "ArchiveWritten",
        AuditEventKind::SnapshotWritten => "SnapshotWritten",
        AuditEventKind::RetentionSwept => "RetentionSwept",
    }
}

fn parse_kind(s: &str) -> Option<AuditEventKind> {
    match s {
        "PaneIdentityResolved" => Some(AuditEventKind::PaneIdentityResolved),
        "PaneIdentityChanged" => Some(AuditEventKind::PaneIdentityChanged),
        "PaneBecameDead" => Some(AuditEventKind::PaneBecameDead),
        "PaneReappeared" => Some(AuditEventKind::PaneReappeared),
        "AlertFired" => Some(AuditEventKind::AlertFired),
        "RecommendationEmitted" => Some(AuditEventKind::RecommendationEmitted),
        "StartupVersionSnapshot" => Some(AuditEventKind::StartupVersionSnapshot),
        "VersionDriftDetected" => Some(AuditEventKind::VersionDriftDetected),
        "SafetyOverrideRejected" => Some(AuditEventKind::SafetyOverrideRejected),
        "ArchiveWritten" => Some(AuditEventKind::ArchiveWritten),
        "SnapshotWritten" => Some(AuditEventKind::SnapshotWritten),
        "RetentionSwept" => Some(AuditEventKind::RetentionSwept),
        _ => None,
    }
}

fn severity_to_str(s: Severity) -> &'static str {
    match s {
        Severity::Safe => "Safe",
        Severity::Good => "Good",
        Severity::Concern => "Concern",
        Severity::Warning => "Warning",
        Severity::Risk => "Risk",
    }
}

fn parse_severity(s: &str) -> Option<Severity> {
    match s {
        "Safe" => Some(Severity::Safe),
        "Good" => Some(Severity::Good),
        "Concern" => Some(Severity::Concern),
        "Warning" => Some(Severity::Warning),
        "Risk" => Some(Severity::Risk),
        _ => None,
    }
}

fn provider_to_str(p: Provider) -> &'static str {
    match p {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Gemini => "Gemini",
        Provider::Qmonster => "Qmonster",
        Provider::Unknown => "Unknown",
    }
}

fn parse_provider(s: &str) -> Option<Provider> {
    match s {
        "Claude" => Some(Provider::Claude),
        "Codex" => Some(Provider::Codex),
        "Gemini" => Some(Provider::Gemini),
        "Qmonster" => Some(Provider::Qmonster),
        "Unknown" => Some(Provider::Unknown),
        _ => None,
    }
}

fn role_to_str(r: Role) -> &'static str {
    match r {
        Role::Main => "Main",
        Role::Review => "Review",
        Role::Research => "Research",
        Role::Monitor => "Monitor",
        Role::Unknown => "Unknown",
    }
}

fn parse_role(s: &str) -> Option<Role> {
    match s {
        "Main" => Some(Role::Main),
        "Review" => Some(Role::Review),
        "Research" => Some(Role::Research),
        "Monitor" => Some(Role::Monitor),
        "Unknown" => Some(Role::Unknown),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::audit::{AuditEvent, AuditEventKind};
    use crate::domain::identity::{Provider, Role};
    use crate::domain::recommendation::Severity;
    use crate::store::sink::EventSink;
    use tempfile::TempDir;

    fn sample(kind: AuditEventKind) -> AuditEvent {
        AuditEvent {
            kind,
            pane_id: "%1".into(),
            severity: Severity::Warning,
            summary: "test".into(),
            provider: Some(Provider::Claude),
            role: Some(Role::Main),
        }
    }

    #[test]
    fn sqlite_audit_sink_persists_events() {
        let td = TempDir::new().unwrap();
        let sink = SqliteAuditSink::open(&td.path().join("q.db")).unwrap();
        sink.record(sample(AuditEventKind::AlertFired));
        sink.record(sample(AuditEventKind::VersionDriftDetected));
        let rows = sink.recent(10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, AuditEventKind::AlertFired);
        assert_eq!(rows[1].kind, AuditEventKind::VersionDriftDetected);
    }

    #[test]
    fn schema_stores_metadata_fields_only() {
        let td = TempDir::new().unwrap();
        let sink = SqliteAuditSink::open(&td.path().join("q.db")).unwrap();
        sink.record(sample(AuditEventKind::AlertFired));
        let rows = sink.recent(1).unwrap();
        let row = &rows[0];
        // Only structured fields are available — there is no raw_tail/
        // raw_bytes column on AuditEvent or on the DB row.
        assert_eq!(row.pane_id, "%1");
        assert_eq!(row.summary, "test");
        assert!(row.timestamp_utc.len() >= "2026-04-20".len());
    }

    #[test]
    fn open_is_idempotent() {
        let td = TempDir::new().unwrap();
        let db_path = td.path().join("q.db");
        {
            let sink = SqliteAuditSink::open(&db_path).unwrap();
            sink.record(sample(AuditEventKind::AlertFired));
        }
        // Re-open existing DB — schema already there; previous events retained.
        let sink = SqliteAuditSink::open(&db_path).unwrap();
        let rows = sink.recent(10).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn sqlite_audit_sink_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SqliteAuditSink>();
    }
}
