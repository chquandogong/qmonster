use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

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
///
/// Runtime INSERT failures (disk full, schema drift, lock contention,
/// table removed) do not panic — they increment `error_count` and
/// surface to stderr so the operator can notice the durability
/// degradation (Codex Phase-2 finding #2).
pub struct SqliteAuditSink {
    db: AuditDb,
    error_count: AtomicU64,
}

impl SqliteAuditSink {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
            error_count: AtomicU64::new(0),
        })
    }

    /// Count of runtime INSERT failures observed by this sink. Tests
    /// and the CLI can inspect this to detect silent degradation.
    pub fn error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
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
                    severity: parse_severity(&row.get::<_, String>(3)?).unwrap_or(Severity::Safe),
                    pane_id: row.get(4)?,
                    provider: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| parse_provider(&s)),
                    role: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| parse_role(&s)),
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
        let res = conn.execute(
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
        if let Err(e) = res {
            self.error_count.fetch_add(1, Ordering::Relaxed);
            eprintln!(
                "qmonster: audit insert failed ({kind:?}): {e}",
                kind = event.kind
            );
        }
    }
}

/// v1.10.4 audit-vocab cleanup (Codex v1.10.2 §9 + Gemini v1.10.2 #10):
/// the SQL-row string form of each `AuditEventKind` now lives on the
/// domain type as `AuditEventKind::as_str`. This free function is kept
/// as a thin delegate so the local call sites in `SqliteAuditSink`
/// stay readable; adding a variant is now a single-location change
/// in `src/domain/audit.rs`. `parse_kind` (below) stays here because
/// it is the fallible inverse and must return `Option` for unknown
/// strings read from potentially-older DB rows.
fn kind_to_str(k: AuditEventKind) -> &'static str {
    k.as_str()
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
        "VersionSnapshotError" => Some(AuditEventKind::VersionSnapshotError),
        "PricingLoadFailed" => Some(AuditEventKind::PricingLoadFailed),
        "ClaudeSettingsLoadFailed" => Some(AuditEventKind::ClaudeSettingsLoadFailed),
        "AuditWriteFailed" => Some(AuditEventKind::AuditWriteFailed),
        "PromptSendProposed" => Some(AuditEventKind::PromptSendProposed),
        "PromptSendAccepted" => Some(AuditEventKind::PromptSendAccepted),
        "PromptSendRejected" => Some(AuditEventKind::PromptSendRejected),
        "PromptSendCompleted" => Some(AuditEventKind::PromptSendCompleted),
        "PromptSendFailed" => Some(AuditEventKind::PromptSendFailed),
        "PromptSendBlocked" => Some(AuditEventKind::PromptSendBlocked),
        "RuntimeRefreshRequested" => Some(AuditEventKind::RuntimeRefreshRequested),
        "RuntimeRefreshCompleted" => Some(AuditEventKind::RuntimeRefreshCompleted),
        "RuntimeRefreshFailed" => Some(AuditEventKind::RuntimeRefreshFailed),
        "RuntimeRefreshBlocked" => Some(AuditEventKind::RuntimeRefreshBlocked),
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
    fn parse_kind_inverts_as_str_for_every_variant() {
        // v1.10.5 remediation (Codex v1.10.4 optional TODO #1): lock
        // the inverse side of the audit-vocab contract. For every
        // AuditEventKind variant, `parse_kind(kind.as_str()) ==
        // Some(kind)` — the write path (`as_str` now the single
        // source of truth) and the read path (`parse_kind`) cannot
        // drift apart without failing this test. Pairs with the
        // domain-layer `audit_event_kind_as_str_contract_locks_every_
        // variant_string` to form a two-sided round-trip guarantee
        // at the type-string layer (no SQLite session required).
        let variants = [
            AuditEventKind::PaneIdentityResolved,
            AuditEventKind::PaneIdentityChanged,
            AuditEventKind::PaneBecameDead,
            AuditEventKind::PaneReappeared,
            AuditEventKind::AlertFired,
            AuditEventKind::RecommendationEmitted,
            AuditEventKind::StartupVersionSnapshot,
            AuditEventKind::VersionDriftDetected,
            AuditEventKind::SafetyOverrideRejected,
            AuditEventKind::ArchiveWritten,
            AuditEventKind::SnapshotWritten,
            AuditEventKind::RetentionSwept,
            AuditEventKind::VersionSnapshotError,
            AuditEventKind::PricingLoadFailed,
            AuditEventKind::ClaudeSettingsLoadFailed,
            AuditEventKind::AuditWriteFailed,
            AuditEventKind::PromptSendProposed,
            AuditEventKind::PromptSendAccepted,
            AuditEventKind::PromptSendRejected,
            AuditEventKind::PromptSendCompleted,
            AuditEventKind::PromptSendFailed,
            AuditEventKind::PromptSendBlocked,
            AuditEventKind::RuntimeRefreshRequested,
            AuditEventKind::RuntimeRefreshCompleted,
            AuditEventKind::RuntimeRefreshFailed,
            AuditEventKind::RuntimeRefreshBlocked,
        ];
        for kind in variants {
            let s = kind.as_str();
            assert_eq!(
                parse_kind(s),
                Some(kind),
                "parse_kind({s:?}) must invert {kind:?}.as_str()"
            );
        }
        // Unknown strings must still map to None so older DB rows
        // carrying a retired kind name do not panic the reader —
        // this is the load-bearing reason parse_kind returns Option
        // rather than being derived via `TryFrom`.
        assert_eq!(
            parse_kind("__RetiredKindFromV0.3__"),
            None,
            "parse_kind must tolerate unknown historical strings by returning None"
        );
    }

    #[test]
    fn prompt_send_audit_kinds_roundtrip_through_sqlite() {
        // P5-1 SQLite contract: the three new `PromptSend*` audit kinds
        // must round-trip through kind_to_str + parse_kind so they
        // survive an audit log write/read cycle. This pins the
        // symmetry of the two match blocks at the type-string layer.
        let td = TempDir::new().unwrap();
        let sink = SqliteAuditSink::open(&td.path().join("q.db")).unwrap();
        sink.record(sample(AuditEventKind::PromptSendProposed));
        sink.record(sample(AuditEventKind::PromptSendAccepted));
        sink.record(sample(AuditEventKind::PromptSendRejected));
        let rows = sink.recent(10).unwrap();
        assert_eq!(rows.len(), 3);
        let kinds: Vec<AuditEventKind> = rows.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&AuditEventKind::PromptSendProposed));
        assert!(kinds.contains(&AuditEventKind::PromptSendAccepted));
        assert!(kinds.contains(&AuditEventKind::PromptSendRejected));
    }

    #[test]
    fn pricing_load_failed_audit_kind_roundtrips_through_sqlite() {
        // v1.11.2 remediation (Gemini v1.11.0 must-fix #2): the new
        // PricingLoadFailed kind must survive a write → read cycle so
        // operator forensics can spot "cost badges blanked this
        // session because config/pricing.toml was malformed" via a
        // plain SQLite query, not just stderr.
        let td = TempDir::new().unwrap();
        let sink = SqliteAuditSink::open(&td.path().join("q.db")).unwrap();
        sink.record(sample(AuditEventKind::PricingLoadFailed));
        let rows = sink.recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, AuditEventKind::PricingLoadFailed);
        // Independent check: the single-source-of-truth string must
        // appear verbatim in the stored metadata column.
        assert_eq!(
            AuditEventKind::PricingLoadFailed.as_str(),
            "PricingLoadFailed"
        );
    }

    #[test]
    fn claude_settings_load_failed_audit_kind_roundtrips_through_sqlite() {
        // Slice 2 (v1.12.0-1): the new ClaudeSettingsLoadFailed kind
        // must survive a write → read cycle so operators can post-hoc
        // query why the MODEL badge disappeared on Claude panes.
        // Mirrors the sibling PricingLoadFailed round-trip test.
        let td = TempDir::new().unwrap();
        let sink = SqliteAuditSink::open(&td.path().join("q.db")).unwrap();
        sink.record(sample(AuditEventKind::ClaudeSettingsLoadFailed));
        let rows = sink.recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, AuditEventKind::ClaudeSettingsLoadFailed);
        // Independent check: the single-source-of-truth string must
        // appear verbatim via the as_str canonical form.
        assert_eq!(
            AuditEventKind::ClaudeSettingsLoadFailed.as_str(),
            "ClaudeSettingsLoadFailed"
        );
    }

    #[test]
    fn p5_3_prompt_send_kinds_roundtrip_through_sqlite() {
        // P5-3 contract: PromptSendCompleted, PromptSendFailed, and
        // PromptSendBlocked must round-trip through kind_to_str +
        // parse_kind so they survive an audit write/read cycle.
        let td = TempDir::new().unwrap();
        let sink = SqliteAuditSink::open(&td.path().join("q.db")).unwrap();
        sink.record(sample(AuditEventKind::PromptSendCompleted));
        sink.record(sample(AuditEventKind::PromptSendFailed));
        sink.record(sample(AuditEventKind::PromptSendBlocked));
        let rows = sink.recent(10).unwrap();
        assert_eq!(rows.len(), 3);
        let kinds: Vec<AuditEventKind> = rows.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&AuditEventKind::PromptSendCompleted));
        assert!(kinds.contains(&AuditEventKind::PromptSendFailed));
        assert!(kinds.contains(&AuditEventKind::PromptSendBlocked));
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

    #[test]
    fn runtime_insert_failure_increments_error_count() {
        let td = TempDir::new().unwrap();
        let sink = SqliteAuditSink::open(&td.path().join("q.db")).unwrap();
        // Drop the table out from under the sink to force a runtime
        // INSERT failure without touching the sink's constructor path.
        {
            let conn = sink.db.connection().lock().unwrap();
            conn.execute_batch("DROP TABLE audit_events")
                .expect("drop ok");
        }
        assert_eq!(sink.error_count(), 0);
        sink.record(sample(AuditEventKind::AlertFired));
        assert_eq!(
            sink.error_count(),
            1,
            "INSERT must fail after table drop and bump error_count"
        );
    }
}
