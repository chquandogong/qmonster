//! Worker-thread fetch path for the `i` Token Insights overlay
//! (v1.50.0). Each `i` open / `r` refresh spawns one thread that
//! opens the audit DB read-only, runs `snapshot_with_ignored_ttl`,
//! and emits an `InsightsLoadOutcome` on the supplied channel. Stale
//! outcomes (whose `request_id` no longer matches the overlay's
//! pending id) are dropped on receipt by `InsightsOverlay::set_
//! snapshot_for`.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::store::{InsightsSnapshot, InsightsWindow, SqliteInsightsStore};

#[derive(Debug)]
pub struct InsightsLoadOutcome {
    pub request_id: u64,
    pub result: Result<InsightsSnapshot, String>,
}

pub fn spawn_insights_load(
    audit_path: PathBuf,
    window: InsightsWindow,
    ignored_ttl_secs: u64,
    sender: Sender<InsightsLoadOutcome>,
    request_id: u64,
) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_insights_load(&audit_path, window, ignored_ttl_secs)
        }))
        .unwrap_or_else(|_| Err("insights load thread panicked".into()));

        // Channel disconnect (Receiver dropped) means the runtime is
        // shutting down — drop the outcome silently.
        let _ = sender.send(InsightsLoadOutcome { request_id, result });
    });
}

fn run_insights_load(
    audit_path: &PathBuf,
    window: InsightsWindow,
    ignored_ttl_secs: u64,
) -> Result<InsightsSnapshot, String> {
    if !audit_path.exists() {
        return Ok(crate::insights_report::empty_insights_snapshot(window));
    }
    SqliteInsightsStore::open_read_only(audit_path)
        .and_then(|store| store.snapshot_with_ignored_ttl(window, ignored_ttl_secs))
        .map_err(|e| format!("open/query {} failed: {e}", audit_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite::AuditDb;
    use std::sync::mpsc;
    use std::time::Duration;

    fn recv_outcome(rx: &mpsc::Receiver<InsightsLoadOutcome>) -> InsightsLoadOutcome {
        rx.recv_timeout(Duration::from_secs(5))
            .expect("worker thread should send outcome within 5s")
    }

    #[test]
    fn spawn_insights_load_emits_outcome_with_matching_request_id() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("audit.db");
        // Initialize the schema by opening + dropping a writer.
        let _ = AuditDb::open(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        let window = InsightsWindow {
            since_ms: 0,
            until_ms: 1_000_000,
        };

        spawn_insights_load(path.clone(), window, 1800, tx, 42);

        let outcome = recv_outcome(&rx);
        assert_eq!(outcome.request_id, 42);
        assert!(outcome.result.is_ok(), "result was {:?}", outcome.result);
    }

    #[test]
    fn spawn_insights_load_emits_ok_for_missing_db() {
        let tmp = tempfile::TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist.db");
        let (tx, rx) = mpsc::channel();
        let window = InsightsWindow {
            since_ms: 0,
            until_ms: 1_000_000,
        };

        spawn_insights_load(missing, window, 1800, tx, 7);

        let outcome = recv_outcome(&rx);
        assert_eq!(outcome.request_id, 7);
        assert!(outcome.result.is_ok());
    }

    #[test]
    fn spawn_insights_load_emits_err_when_path_is_a_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        // A directory exists but is not a SQLite file -> open_read_only should error.
        let (tx, rx) = mpsc::channel();
        let window = InsightsWindow {
            since_ms: 0,
            until_ms: 1_000_000,
        };

        spawn_insights_load(tmp.path().to_path_buf(), window, 1800, tx, 9);

        let outcome = recv_outcome(&rx);
        assert_eq!(outcome.request_id, 9);
        assert!(outcome.result.is_err(), "result was {:?}", outcome.result);
    }
}
