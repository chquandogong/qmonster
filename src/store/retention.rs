use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{Duration as ChronoDuration, Utc};
use thiserror::Error;

use crate::store::paths::QmonsterPaths;

#[derive(Debug, Error)]
pub enum RetentionError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(String),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RetentionReport {
    pub files_removed: u64,
    pub bytes_removed: u64,
    pub files_kept: u64,
    pub token_usage_rows_deleted: u64,
    /// audit_events rows removed (previously unbounded — the dominant
    /// source of runaway qmonster.db growth).
    pub audit_rows_deleted: u64,
    /// anomaly_events + anomaly_history_snapshots rows removed.
    pub anomaly_rows_deleted: u64,
    /// cost_usage_events rows removed.
    pub cost_rows_deleted: u64,
    /// Tables from removed subsystems (narrow-vNext insights store)
    /// dropped from a legacy DB.
    pub legacy_tables_dropped: u64,
}

/// Delete files under `archive/` and `snapshots/` older than
/// `max_age_days`, and prune the time-series SQLite tables to the same
/// horizon (`audit_events`, `token_usage_samples`, `anomaly_events`,
/// `anomaly_history_snapshots`, `cost_usage_events`), plus drop tables
/// from removed subsystems. `max_age_days = 0` is a no-op so operators
/// can disable retention without special-casing downstream.
pub fn sweep(paths: &QmonsterPaths, max_age_days: u64) -> Result<RetentionReport, RetentionError> {
    if max_age_days == 0 {
        return Ok(RetentionReport::default());
    }
    // Clamp absurd retention values so the day/millisecond arithmetic
    // below cannot overflow (Codex P2). 36_500 days = 100 years, far
    // beyond any real retention horizon.
    let max_age_days = max_age_days.min(36_500);
    let horizon = SystemTime::now()
        .checked_sub(Duration::from_secs(max_age_days * 86_400))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut report = RetentionReport::default();
    for dir in [paths.archive_dir(), paths.snapshot_dir()] {
        sweep_dir(&dir, horizon, &mut report)?;
    }

    // Prune the time-series tables to the same horizon. Reuses the
    // shared qmonster.db file; falls through silently if the file does
    // not exist (e.g. fresh test root with no DB yet).
    //
    // Before v3.1.4 only `token_usage_samples` (F-3) was pruned, so
    // `audit_events` (and `anomaly_*` / `cost_usage_events`) grew
    // without bound — audit_events reached 4.7M rows / 1.1 GB in the
    // field. All time-series tables now honour `retention_days`.
    if paths.sqlite_path().exists() {
        let cutoff_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
            - (max_age_days as i64) * 86_400_000;
        let cutoff_secs = cutoff_ms / 1000;
        // audit_events.ts_utc is written as `Utc::now().to_rfc3339()`
        // (fixed-width RFC 3339 UTC), so a lexicographic `<` comparison
        // against an RFC 3339 cutoff is correct — the same contract the
        // audit read path (`recent_audit_max_severity_at`) relies on.
        let cutoff_rfc = (Utc::now() - ChronoDuration::days(max_age_days as i64)).to_rfc3339();

        let conn = rusqlite::Connection::open(paths.sqlite_path())
            .map_err(|e| RetentionError::Sqlite(e.to_string()))?;

        report.token_usage_rows_deleted =
            prune_older_than(&conn, "token_usage_samples", "ts_unix_ms", cutoff_ms)?;
        report.audit_rows_deleted = prune_older_than(&conn, "audit_events", "ts_utc", &cutoff_rfc)?;
        report.anomaly_rows_deleted =
            prune_older_than(&conn, "anomaly_events", "ts_unix_secs", cutoff_secs)?
                + prune_older_than(
                    &conn,
                    "anomaly_history_snapshots",
                    "tick_unix_secs",
                    cutoff_secs,
                )?;
        report.cost_rows_deleted =
            prune_older_than(&conn, "cost_usage_events", "ts_unix_ms", cutoff_ms)?;

        // Drop tables from removed subsystems (narrow-vNext insights
        // store). They are never written by current code; on a legacy
        // DB they are pure dead weight (~350 MB in the field).
        for legacy in ["recommendation_events", "recommendation_outcomes"] {
            if table_exists(&conn, legacy)? {
                conn.execute_batch(&format!("DROP TABLE IF EXISTS {legacy}"))
                    .map_err(|e| RetentionError::Sqlite(e.to_string()))?;
                report.legacy_tables_dropped += 1;
            }
        }
    }

    Ok(report)
}

fn sweep_dir(
    dir: &Path,
    horizon: SystemTime,
    report: &mut RetentionReport,
) -> Result<(), RetentionError> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sweep_dir(&path, horizon, report)?;
            // Prune empty directories so the tree stays tidy.
            if std::fs::read_dir(&path)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false)
            {
                let _ = std::fs::remove_dir(&path);
            }
            continue;
        }
        let meta = entry.metadata()?;
        let modified = meta.modified().unwrap_or(SystemTime::now());
        if modified < horizon {
            let bytes = meta.len();
            std::fs::remove_file(&path)?;
            report.files_removed += 1;
            report.bytes_removed += bytes;
        } else {
            report.files_kept += 1;
        }
    }
    Ok(())
}

fn table_exists(conn: &rusqlite::Connection, name: &str) -> Result<bool, RetentionError> {
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |r| r.get(0),
        )
        .map_err(|e| RetentionError::Sqlite(e.to_string()))?;
    Ok(n > 0)
}

fn column_exists(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
) -> Result<bool, RetentionError> {
    // `table` is a hard-coded call-site constant, never user input.
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| RetentionError::Sqlite(e.to_string()))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| RetentionError::Sqlite(e.to_string()))?;
    while let Some(row) = rows
        .next()
        .map_err(|e| RetentionError::Sqlite(e.to_string()))?
    {
        let name: String = row
            .get(1)
            .map_err(|e| RetentionError::Sqlite(e.to_string()))?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `DELETE FROM {table} WHERE {ts_col} < cutoff`, returning the number
/// of rows removed. Returns 0 when the table — or its timestamp column —
/// is absent, so the sweep tolerates schema variance across historical
/// DBs instead of aborting the whole pass on a `no such column` error
/// (Codex P2).
///
/// `table` / `ts_col` are hard-coded call-site constants (never user
/// input), so the format interpolation is injection-safe.
fn prune_older_than<P: rusqlite::ToSql>(
    conn: &rusqlite::Connection,
    table: &str,
    ts_col: &str,
    cutoff: P,
) -> Result<u64, RetentionError> {
    if !table_exists(conn, table)? || !column_exists(conn, table, ts_col)? {
        return Ok(0);
    }
    let sql = format!("DELETE FROM {table} WHERE {ts_col} < ?1");
    let n = conn
        .execute(&sql, rusqlite::params![cutoff])
        .map_err(|e| RetentionError::Sqlite(e.to_string()))?;
    Ok(n as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::paths::QmonsterPaths;
    use std::fs;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    fn touch_aged(path: &std::path::Path, days_old: u64) {
        fs::write(path, b"x").unwrap();
        let old = SystemTime::now() - Duration::from_secs(days_old * 86_400 + 60);
        let ft = filetime::FileTime::from_system_time(old);
        filetime::set_file_mtime(path, ft).unwrap();
    }

    #[test]
    fn sweep_removes_files_older_than_max_days() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let old = paths.archive_dir().join("old.log");
        let fresh = paths.archive_dir().join("fresh.log");
        touch_aged(&old, 30);
        touch_aged(&fresh, 1);

        let report = sweep(&paths, 14).unwrap();
        assert_eq!(report.files_removed, 1);
        assert_eq!(report.bytes_removed, 1);
        assert!(!old.exists());
        assert!(fresh.exists());
    }

    #[test]
    fn sweep_touches_archive_and_snapshots_only() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        // Place an old file outside archive/ and snapshots/ and make sure
        // sweep leaves it alone even if aged past the retention horizon.
        let outside = paths.root().join("versions.json");
        touch_aged(&outside, 30);
        let old_snap = paths.snapshot_dir().join("old.json");
        touch_aged(&old_snap, 30);

        let report = sweep(&paths, 14).unwrap();
        assert_eq!(report.files_removed, 1);
        assert!(outside.exists(), "retention must not touch versions.json");
        assert!(!old_snap.exists());
    }

    #[test]
    fn zero_retention_is_a_no_op() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let old = paths.archive_dir().join("old.log");
        touch_aged(&old, 30);
        let report = sweep(&paths, 0).unwrap();
        assert_eq!(report.files_removed, 0);
        assert!(old.exists());
    }

    #[test]
    fn sweep_deletes_token_usage_rows_older_than_max_days() {
        use crate::domain::identity::Provider;
        use crate::store::SqliteTokenUsageSink;
        use crate::store::token_usage::TokenSample;

        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let sink = SqliteTokenUsageSink::open(&paths.sqlite_path()).unwrap();
        let now_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let day_ms: i64 = 86_400_000;
        // Old (10 days ago) - must be deleted with max_age=7
        sink.record_sample(&TokenSample {
            ts_unix_ms: now_ms - 10 * day_ms,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: None,
        })
        .unwrap();
        // Fresh (1 day ago) - must survive
        sink.record_sample(&TokenSample {
            ts_unix_ms: now_ms - day_ms,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(2),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: None,
        })
        .unwrap();
        // Drop our writer-side connection so the retention pass owns the lock
        drop(sink);

        let report = sweep(&paths, 7).unwrap();
        assert_eq!(report.token_usage_rows_deleted, 1);

        let read_sink = SqliteTokenUsageSink::open(&paths.sqlite_path()).unwrap();
        let surviving = read_sink.recent_samples("%1", 10).unwrap();
        assert_eq!(surviving.len(), 1);
        assert_eq!(surviving[0].input_tokens, Some(2));
    }

    #[test]
    fn sweep_deletes_audit_events_older_than_max_days() {
        use chrono::{Duration as ChronoDuration, Utc};
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        // Opening the audit sink creates the audit_events schema.
        let sink = crate::store::SqliteAuditSink::open(&paths.sqlite_path()).unwrap();
        drop(sink);

        let conn = rusqlite::Connection::open(paths.sqlite_path()).unwrap();
        let old_ts = (Utc::now() - ChronoDuration::days(20)).to_rfc3339();
        let fresh_ts = (Utc::now() - ChronoDuration::days(1)).to_rfc3339();
        conn.execute(
            "INSERT INTO audit_events (ts_utc, kind, severity, pane_id, summary)
             VALUES (?1, 'AlertFired', 'Warning', '%1', 'old')",
            [&old_ts],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO audit_events (ts_utc, kind, severity, pane_id, summary)
             VALUES (?1, 'AlertFired', 'Warning', '%1', 'fresh')",
            [&fresh_ts],
        )
        .unwrap();
        drop(conn);

        let report = sweep(&paths, 14).unwrap();
        assert_eq!(
            report.audit_rows_deleted, 1,
            "the 20-day-old row must be pruned"
        );

        let conn = rusqlite::Connection::open(paths.sqlite_path()).unwrap();
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 1, "the 1-day-old row must survive");
        let summary: String = conn
            .query_row("SELECT summary FROM audit_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(summary, "fresh");
    }

    #[test]
    fn sweep_drops_legacy_recommendation_tables() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let sink = crate::store::SqliteAuditSink::open(&paths.sqlite_path()).unwrap();
        drop(sink);
        // Simulate an old DB that still carries the removed insights tables.
        let conn = rusqlite::Connection::open(paths.sqlite_path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE recommendation_events (id INTEGER PRIMARY KEY, ts_unix_ms INTEGER);
             CREATE TABLE recommendation_outcomes (id INTEGER PRIMARY KEY);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO recommendation_events (ts_unix_ms) VALUES (1)",
            [],
        )
        .unwrap();
        drop(conn);

        let report = sweep(&paths, 14).unwrap();
        assert_eq!(report.legacy_tables_dropped, 2);

        let conn = rusqlite::Connection::open(paths.sqlite_path()).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'
                 AND name IN ('recommendation_events','recommendation_outcomes')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "legacy tables must be gone");
    }

    #[test]
    fn sweep_prunes_anomaly_and_cost_tables_older_than_max_days() {
        use chrono::{Duration as ChronoDuration, Utc};
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let sink = crate::store::SqliteAuditSink::open(&paths.sqlite_path()).unwrap();
        drop(sink);

        let now = Utc::now();
        let old_secs = (now - ChronoDuration::days(20)).timestamp();
        let fresh_secs = (now - ChronoDuration::days(1)).timestamp();
        let old_ms = (now - ChronoDuration::days(20)).timestamp_millis();
        let fresh_ms = (now - ChronoDuration::days(1)).timestamp_millis();

        let conn = rusqlite::Connection::open(paths.sqlite_path()).unwrap();
        conn.execute(
            "INSERT INTO anomaly_events (ts_unix_secs, pane_id, kind, confidence, severity, promoted)
             VALUES (?1, '%1', 'k', 'High', 'Warning', 0)",
            [old_secs],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO anomaly_events (ts_unix_secs, pane_id, kind, confidence, severity, promoted)
             VALUES (?1, '%1', 'k', 'High', 'Warning', 0)",
            [fresh_secs],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO anomaly_history_snapshots (pane_id, tick_unix_secs) VALUES ('%1', ?1)",
            [old_secs],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO anomaly_history_snapshots (pane_id, tick_unix_secs) VALUES ('%1', ?1)",
            [fresh_secs],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cost_usage_events (ts_unix_ms, pane_id, provider, cost_usd_delta, cumulative_cost_usd)
             VALUES (?1, '%1', 'Codex', 0.1, 0.1)",
            [old_ms],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cost_usage_events (ts_unix_ms, pane_id, provider, cost_usd_delta, cumulative_cost_usd)
             VALUES (?1, '%1', 'Codex', 0.1, 0.2)",
            [fresh_ms],
        )
        .unwrap();
        drop(conn);

        let report = sweep(&paths, 14).unwrap();
        // one old anomaly_events row + one old anomaly_history_snapshots row
        assert_eq!(report.anomaly_rows_deleted, 2);
        assert_eq!(report.cost_rows_deleted, 1);
    }

    #[test]
    fn sweep_tolerates_a_table_missing_its_timestamp_column() {
        // Codex P2: a legacy DB where a target table exists but with a
        // different schema (no ts column) must NOT abort the whole sweep
        // with `no such column` — the table is simply skipped.
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let sink = crate::store::SqliteAuditSink::open(&paths.sqlite_path()).unwrap();
        drop(sink);
        let conn = rusqlite::Connection::open(paths.sqlite_path()).unwrap();
        conn.execute_batch(
            "DROP TABLE audit_events;
             CREATE TABLE audit_events (id INTEGER PRIMARY KEY, other TEXT);",
        )
        .unwrap();
        conn.execute("INSERT INTO audit_events (other) VALUES ('x')", [])
            .unwrap();
        drop(conn);

        // Must return Ok (not Err) and simply prune 0 audit rows.
        let report = sweep(&paths, 14).unwrap();
        assert_eq!(report.audit_rows_deleted, 0);
    }

    #[test]
    fn sweep_does_not_overflow_on_absurd_retention_days() {
        // Codex P2: `max_age_days * 86_400_000` must not overflow i64 /
        // u64 on an absurd config value (debug builds panic on overflow).
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let sink = crate::store::SqliteAuditSink::open(&paths.sqlite_path()).unwrap();
        drop(sink);

        // u64::MAX days: a huge horizon prunes nothing, and above all
        // must not panic.
        let report = sweep(&paths, u64::MAX).unwrap();
        assert_eq!(report.audit_rows_deleted, 0);
    }
}
