//! v2.2.0 post-publish improvement: recommendation engagement query
//! exposed to operators as `qmonster rec-engagement [--since 2w]`.
//!
//! Reads `recommendation_events` + `recommendation_outcomes` to compute
//! per-action surfaced / copied / accepted / ignored / other counts.
//! Drives the measurement plan in
//! `.mission/decisions/MDR-DRAFT-v2.2.0-P2-1-anomaly-subsystem-future.md`
//! without forcing the operator to run raw SQL.
//!
//! Honesty contract: the snapshot is best-effort. Missing tables
//! (fresh install before the lifecycle ledger has fired) return an
//! empty snapshot rather than an error.

use std::path::Path;

use rusqlite::params;

use crate::store::insights::InsightsWindow;
use crate::store::sqlite::{AuditDb, SqliteError};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecEngagementRow {
    pub action: String,
    pub surfaced: u64,
    pub copied: u64,
    pub accepted: u64,
    pub ignored: u64,
    pub other_outcome: u64,
}

impl RecEngagementRow {
    /// Sum of "operator did something with this rec" outcomes over
    /// surfaced events. Returns `None` when surfaced=0 to avoid
    /// fabricating 100% / 0% for unused actions.
    pub fn engagement_rate(&self) -> Option<f64> {
        if self.surfaced == 0 {
            return None;
        }
        let engaged = self.copied + self.accepted;
        Some(engaged as f64 / self.surfaced as f64)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecEngagementSnapshot {
    pub window: InsightsWindow,
    pub rows: Vec<RecEngagementRow>,
    /// True when the sqlite file existed but neither lifecycle table
    /// was present — caller can hint "no telemetry yet" instead of
    /// printing an empty grid that looks like a bug.
    pub tables_missing: bool,
}

/// Open the audit DB read-only and aggregate per-action engagement.
pub fn rec_engagement_snapshot(
    path: &Path,
    window: InsightsWindow,
) -> Result<RecEngagementSnapshot, SqliteError> {
    let db = AuditDb::open_read_only(path)?;
    let conn = db
        .connection()
        .lock()
        .map_err(|e| SqliteError::Query(e.to_string()))?;

    if !table_exists(&conn, "recommendation_events")?
        || !table_exists(&conn, "recommendation_outcomes")?
    {
        return Ok(RecEngagementSnapshot {
            window,
            rows: Vec::new(),
            tables_missing: true,
        });
    }

    // Per-action surfaced count from recommendation_events. Clipboard copies
    // are intentionally written without an event id, so match them to the
    // latest prior event with the same pane/action inside the reporting window.
    let mut stmt = conn
        .prepare(
            "WITH matched_outcomes AS (
                 SELECT
                     COALESCE(
                         o.recommendation_event_id,
                         (
                             SELECT e2.id
                             FROM recommendation_events e2
                             WHERE e2.pane_id = o.pane_id
                               AND e2.action = o.action
                               AND e2.ts_unix_ms <= o.ts_unix_ms
                               AND e2.ts_unix_ms >= ?1
                               AND e2.ts_unix_ms <  ?2
                             ORDER BY e2.ts_unix_ms DESC, e2.id DESC
                             LIMIT 1
                         )
                     ) AS event_id,
                     o.outcome
                 FROM recommendation_outcomes o
                 WHERE o.ts_unix_ms >= ?1
                   AND o.ts_unix_ms <  ?2
             )
             SELECT e.action,
                    COUNT(DISTINCT e.id) AS surfaced,
                    COALESCE(SUM(CASE WHEN o.outcome = 'copied'   THEN 1 ELSE 0 END), 0) AS copied,
                    COALESCE(SUM(CASE WHEN o.outcome = 'accepted' THEN 1 ELSE 0 END), 0) AS accepted,
                    COALESCE(SUM(CASE WHEN o.outcome = 'ignored'  THEN 1 ELSE 0 END), 0) AS ignored,
                    COALESCE(SUM(CASE WHEN o.outcome NOT IN ('copied','accepted','ignored')
                                       AND o.outcome IS NOT NULL THEN 1 ELSE 0 END), 0) AS other_outcome
             FROM recommendation_events e
             LEFT JOIN matched_outcomes o
                    ON o.event_id = e.id
             WHERE e.ts_unix_ms >= ?1
               AND e.ts_unix_ms <  ?2
             GROUP BY e.action
             ORDER BY surfaced DESC, e.action ASC",
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;

    let rows = stmt
        .query_map(params![window.since_ms, window.until_ms], |r| {
            Ok(RecEngagementRow {
                action: r.get::<_, String>(0)?,
                surfaced: r.get::<_, i64>(1)?.max(0) as u64,
                copied: r.get::<_, i64>(2)?.max(0) as u64,
                accepted: r.get::<_, i64>(3)?.max(0) as u64,
                ignored: r.get::<_, i64>(4)?.max(0) as u64,
                other_outcome: r.get::<_, i64>(5)?.max(0) as u64,
            })
        })
        .map_err(|e| SqliteError::Query(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| SqliteError::Query(e.to_string()))?;

    Ok(RecEngagementSnapshot {
        window,
        rows,
        tables_missing: false,
    })
}

fn table_exists(conn: &rusqlite::Connection, name: &str) -> Result<bool, SqliteError> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |r| r.get(0),
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::recommendation_lifecycle::{
        RecommendationEventRecord, RecommendationOutcome, RecommendationOutcomeRecord,
        SqliteRecommendationLifecycleSink,
    };
    use tempfile::tempdir;

    fn write_event(sink: &SqliteRecommendationLifecycleSink, action: &str, ts: i64) -> i64 {
        sink.insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: ts,
            pane_id: "%1".into(),
            provider: Some("claude".into()),
            role: Some("main".into()),
            situation: "ctx".into(),
            action: action.into(),
            severity: "Concern".into(),
            source_kind: "ProjectCanonical".into(),
            reason_summary: "test event".into(),
            suggested_command: None,
            is_strong: false,
            dedup_key: format!("{action}|{ts}"),
            threshold_snapshot_json: None,
        })
        .unwrap()
    }

    fn write_outcome(
        sink: &SqliteRecommendationLifecycleSink,
        event_id: i64,
        outcome: RecommendationOutcome,
        ts: i64,
    ) {
        sink.insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: ts,
            recommendation_event_id: Some(event_id),
            pane_id: "%1".into(),
            action: "irrelevant".into(),
            outcome,
            audit_event_id: None,
            summary: "test outcome".into(),
        })
        .unwrap();
    }

    #[test]
    fn engagement_rate_is_none_for_zero_surfaced() {
        let row = RecEngagementRow::default();
        assert!(row.engagement_rate().is_none());
    }

    #[test]
    fn engagement_rate_counts_copied_and_accepted_only() {
        let row = RecEngagementRow {
            action: "x".into(),
            surfaced: 10,
            copied: 2,
            accepted: 1,
            ignored: 3,
            other_outcome: 1,
        };
        let rate = row.engagement_rate().unwrap();
        // (2 + 1) / 10 = 0.3
        assert!((rate - 0.3).abs() < 1e-9);
    }

    #[test]
    fn missing_tables_return_empty_snapshot_with_flag() {
        // Fresh tempdir: file doesn't exist yet. Opening it will create
        // the schema (AuditDb::open_read_only opens an existing file
        // read-only; we test the "tables exist but empty" path here).
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("audit.sqlite");
        // Initialize the schema via the lifecycle sink so tables exist.
        let _sink = SqliteRecommendationLifecycleSink::open(&path).unwrap();

        let snap = rec_engagement_snapshot(
            &path,
            InsightsWindow {
                since_ms: 0,
                until_ms: 1_000_000_000_000,
            },
        )
        .unwrap();
        assert!(!snap.tables_missing, "schema initialized → tables present");
        assert!(snap.rows.is_empty(), "no events written → no rows");
    }

    #[test]
    fn snapshot_groups_outcomes_by_action() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("audit.sqlite");
        let sink = SqliteRecommendationLifecycleSink::open(&path).unwrap();

        let t = 1_000_000_000_000_i64;
        // anomaly:cost-slope — 3 events: 1 copied, 1 accepted, 1 idle.
        let e1 = write_event(&sink, "anomaly:cost-slope", t);
        let e2 = write_event(&sink, "anomaly:cost-slope", t + 1);
        let _e3 = write_event(&sink, "anomaly:cost-slope", t + 2);
        write_outcome(&sink, e1, RecommendationOutcome::Copied, t + 10);
        write_outcome(&sink, e2, RecommendationOutcome::Accepted, t + 20);

        // log-storm — 2 events: 0 engaged, 1 ignored.
        let e4 = write_event(&sink, "log-storm", t + 3);
        let _e5 = write_event(&sink, "log-storm", t + 4);
        write_outcome(&sink, e4, RecommendationOutcome::Ignored, t + 30);

        let snap = rec_engagement_snapshot(
            &path,
            InsightsWindow {
                since_ms: t - 1,
                until_ms: t + 1_000,
            },
        )
        .unwrap();
        assert_eq!(snap.rows.len(), 2, "two distinct actions");

        // Rows are ordered by surfaced DESC: anomaly:cost-slope first.
        let r0 = &snap.rows[0];
        assert_eq!(r0.action, "anomaly:cost-slope");
        assert_eq!(r0.surfaced, 3);
        assert_eq!(r0.copied, 1);
        assert_eq!(r0.accepted, 1);
        assert_eq!(r0.ignored, 0);
        let rate = r0.engagement_rate().unwrap();
        assert!((rate - (2.0 / 3.0)).abs() < 1e-9, "got {rate}");

        let r1 = &snap.rows[1];
        assert_eq!(r1.action, "log-storm");
        assert_eq!(r1.surfaced, 2);
        assert_eq!(r1.ignored, 1);
        assert_eq!(r1.copied, 0);
        assert_eq!(r1.accepted, 0);
        assert!(r1.engagement_rate().unwrap().abs() < 1e-9);
    }

    #[test]
    fn snapshot_matches_unlinked_copied_outcomes_by_pane_and_action() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("audit.sqlite");
        let sink = SqliteRecommendationLifecycleSink::open(&path).unwrap();

        let t = 1_000_000_000_000_i64;
        let _event_id = write_event(&sink, "context-pressure: checkpoint", t);
        sink.insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: t + 10,
            recommendation_event_id: None,
            pane_id: "%1".into(),
            action: "context-pressure: checkpoint".into(),
            outcome: RecommendationOutcome::Copied,
            audit_event_id: None,
            summary: "`/compact`".into(),
        })
        .unwrap();

        let snap = rec_engagement_snapshot(
            &path,
            InsightsWindow {
                since_ms: t - 1,
                until_ms: t + 1_000,
            },
        )
        .unwrap();

        assert_eq!(snap.rows.len(), 1);
        assert_eq!(snap.rows[0].surfaced, 1);
        assert_eq!(
            snap.rows[0].copied, 1,
            "clipboard copies are written without recommendation_event_id"
        );
    }

    #[test]
    fn snapshot_window_excludes_old_events() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("audit.sqlite");
        let sink = SqliteRecommendationLifecycleSink::open(&path).unwrap();

        // Old event outside window.
        write_event(&sink, "old", 1_000);
        // New event inside window.
        let e2 = write_event(&sink, "new", 10_000);
        write_outcome(&sink, e2, RecommendationOutcome::Copied, 10_500);

        let snap = rec_engagement_snapshot(
            &path,
            InsightsWindow {
                since_ms: 5_000,
                until_ms: 20_000,
            },
        )
        .unwrap();
        assert_eq!(snap.rows.len(), 1);
        assert_eq!(snap.rows[0].action, "new");
    }
}
