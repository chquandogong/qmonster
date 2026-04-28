use std::path::Path;
use std::time::{Duration, SystemTime};

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
}

/// Delete files under `archive/` and `snapshots/` older than
/// `max_age_days`. `max_age_days = 0` is a no-op so operators can
/// disable retention without special-casing downstream.
pub fn sweep(paths: &QmonsterPaths, max_age_days: u64) -> Result<RetentionReport, RetentionError> {
    if max_age_days == 0 {
        return Ok(RetentionReport::default());
    }
    let horizon = SystemTime::now()
        .checked_sub(Duration::from_secs(max_age_days * 86_400))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut report = RetentionReport::default();
    for dir in [paths.archive_dir(), paths.snapshot_dir()] {
        sweep_dir(&dir, horizon, &mut report)?;
    }

    // F-3: delete token_usage_samples older than cutoff. Reuses the
    // shared qmonster.db file; falls through silently if the file
    // does not exist (e.g. fresh test root with no DB yet).
    if paths.sqlite_path().exists() {
        let cutoff_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
            - (max_age_days as i64) * 86_400_000;
        let conn = rusqlite::Connection::open(paths.sqlite_path())
            .map_err(|e| RetentionError::Sqlite(e.to_string()))?;
        let n = conn
            .execute(
                "DELETE FROM token_usage_samples WHERE ts_unix_ms < ?",
                rusqlite::params![cutoff_ms],
            )
            .map_err(|e| RetentionError::Sqlite(e.to_string()))?;
        report.token_usage_rows_deleted = n as u64;
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
}
