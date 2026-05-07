//! Phase 8 v2 recommendation lifecycle ledger.
//!
//! This module writes the small, structured ledger that lets Token
//! Insights distinguish emitted recommendations, explicit outcomes,
//! and later TTL ignored classifications. It is intentionally separate
//! from `audit_events`: audit remains the durable operator trail, while
//! this table is optimized for lifecycle correlation.

use std::path::Path;

use rusqlite::OptionalExtension;

use crate::store::sqlite::{AuditDb, SqliteError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecommendationEventRecord {
    pub ts_unix_ms: i64,
    pub pane_id: String,
    pub provider: Option<String>,
    pub role: Option<String>,
    pub situation: String,
    pub action: String,
    pub severity: String,
    pub source_kind: String,
    pub reason_summary: String,
    pub suggested_command: Option<String>,
    pub is_strong: bool,
    pub dedup_key: String,
    pub threshold_snapshot_json: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecommendationOutcome {
    Accepted,
    Rejected,
    Completed,
    Failed,
    Blocked,
    Archived,
    SnapshotWritten,
    AutoSnapshotWritten,
    Hidden,
    Ignored,
}

impl RecommendationOutcome {
    pub fn label(self) -> &'static str {
        match self {
            RecommendationOutcome::Accepted => "accepted",
            RecommendationOutcome::Rejected => "rejected",
            RecommendationOutcome::Completed => "completed",
            RecommendationOutcome::Failed => "failed",
            RecommendationOutcome::Blocked => "blocked",
            RecommendationOutcome::Archived => "archived",
            RecommendationOutcome::SnapshotWritten => "snapshot_written",
            RecommendationOutcome::AutoSnapshotWritten => "auto_snapshot_written",
            RecommendationOutcome::Hidden => "hidden",
            RecommendationOutcome::Ignored => "ignored",
        }
    }

    pub fn try_from_label(label: &str) -> Option<Self> {
        Some(match label {
            "accepted" => RecommendationOutcome::Accepted,
            "rejected" => RecommendationOutcome::Rejected,
            "completed" => RecommendationOutcome::Completed,
            "failed" => RecommendationOutcome::Failed,
            "blocked" => RecommendationOutcome::Blocked,
            "archived" => RecommendationOutcome::Archived,
            "snapshot_written" => RecommendationOutcome::SnapshotWritten,
            "auto_snapshot_written" => RecommendationOutcome::AutoSnapshotWritten,
            "hidden" => RecommendationOutcome::Hidden,
            "ignored" => RecommendationOutcome::Ignored,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecommendationOutcomeRecord {
    pub ts_unix_ms: i64,
    pub recommendation_event_id: Option<i64>,
    pub pane_id: String,
    pub action: String,
    pub outcome: RecommendationOutcome,
    pub audit_event_id: Option<i64>,
    pub summary: String,
}

pub struct SqliteRecommendationLifecycleSink {
    db: AuditDb,
}

impl SqliteRecommendationLifecycleSink {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
        })
    }

    pub fn insert_recommendation_event(
        &self,
        event: &RecommendationEventRecord,
    ) -> Result<i64, SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        conn.execute(
            "INSERT INTO recommendation_events
             (ts_unix_ms, pane_id, provider, role, situation, action, severity,
              source_kind, reason_summary, suggested_command, is_strong, dedup_key,
              threshold_snapshot_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                event.ts_unix_ms,
                event.pane_id.as_str(),
                event.provider.as_deref(),
                event.role.as_deref(),
                event.situation.as_str(),
                event.action.as_str(),
                event.severity.as_str(),
                event.source_kind.as_str(),
                event.reason_summary.as_str(),
                event.suggested_command.as_deref(),
                event.is_strong as i64,
                event.dedup_key.as_str(),
                event.threshold_snapshot_json.as_deref(),
            ],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn insert_recommendation_outcome(
        &self,
        outcome: &RecommendationOutcomeRecord,
    ) -> Result<i64, SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        conn.execute(
            "INSERT INTO recommendation_outcomes
             (ts_unix_ms, recommendation_event_id, pane_id, action, outcome, audit_event_id,
              summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                outcome.ts_unix_ms,
                outcome.recommendation_event_id,
                outcome.pane_id.as_str(),
                outcome.action.as_str(),
                outcome.outcome.label(),
                outcome.audit_event_id,
                outcome.summary.as_str(),
            ],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn insert_correlated_recommendation_outcome(
        &self,
        outcome: &RecommendationOutcomeRecord,
    ) -> Result<i64, SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let recommendation_event_id = match outcome.recommendation_event_id {
            Some(id) => Some(id),
            None => conn
                .query_row(
                    "SELECT id
                     FROM recommendation_events
                     WHERE pane_id = ?1
                       AND action = ?2
                       AND ts_unix_ms <= ?3
                     ORDER BY ts_unix_ms DESC, id DESC
                     LIMIT 1",
                    rusqlite::params![
                        outcome.pane_id.as_str(),
                        outcome.action.as_str(),
                        outcome.ts_unix_ms,
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|e| SqliteError::Query(e.to_string()))?,
        };
        conn.execute(
            "INSERT INTO recommendation_outcomes
             (ts_unix_ms, recommendation_event_id, pane_id, action, outcome, audit_event_id,
              summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                outcome.ts_unix_ms,
                recommendation_event_id,
                outcome.pane_id.as_str(),
                outcome.action.as_str(),
                outcome.outcome.label(),
                outcome.audit_event_id,
                outcome.summary.as_str(),
            ],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(conn.last_insert_rowid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_sink_round_trips_event_and_outcome_ids() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sink = SqliteRecommendationLifecycleSink::open(&tmp.path().join("audit.db")).unwrap();
        let event_id = sink
            .insert_recommendation_event(&RecommendationEventRecord {
                ts_unix_ms: 1_000,
                pane_id: "%1".into(),
                provider: Some("Codex".into()),
                role: Some("Review".into()),
                situation: "Context pressure".into(),
                action: "/compact".into(),
                severity: "warning".into(),
                source_kind: "Estimated".into(),
                reason_summary: "context near critical".into(),
                suggested_command: Some("/compact".into()),
                is_strong: true,
                dedup_key: "%1:/compact".into(),
                threshold_snapshot_json: Some(r#"{"context":0.85}"#.into()),
            })
            .unwrap();
        let outcome_id = sink
            .insert_recommendation_outcome(&RecommendationOutcomeRecord {
                ts_unix_ms: 2_000,
                recommendation_event_id: Some(event_id),
                pane_id: "%1".into(),
                action: "/compact".into(),
                outcome: RecommendationOutcome::Completed,
                audit_event_id: Some(42),
                summary: "prompt sent".into(),
            })
            .unwrap();

        assert!(event_id > 0);
        assert!(outcome_id > 0);
    }

    #[test]
    fn lifecycle_outcome_label_round_trips() {
        for outcome in [
            RecommendationOutcome::Accepted,
            RecommendationOutcome::Rejected,
            RecommendationOutcome::Completed,
            RecommendationOutcome::Failed,
            RecommendationOutcome::Blocked,
            RecommendationOutcome::Archived,
            RecommendationOutcome::SnapshotWritten,
            RecommendationOutcome::AutoSnapshotWritten,
            RecommendationOutcome::Hidden,
            RecommendationOutcome::Ignored,
        ] {
            assert_eq!(
                RecommendationOutcome::try_from_label(outcome.label()),
                Some(outcome)
            );
        }
        assert_eq!(
            RecommendationOutcome::try_from_label("accepted_by_user"),
            None
        );
    }

    #[test]
    fn correlated_outcome_links_latest_prior_matching_event() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sink = SqliteRecommendationLifecycleSink::open(&tmp.path().join("audit.db")).unwrap();
        let older = sink
            .insert_recommendation_event(&RecommendationEventRecord {
                ts_unix_ms: 1_000,
                pane_id: "%1".into(),
                provider: Some("Codex".into()),
                role: Some("Main".into()),
                situation: "Context pressure".into(),
                action: "/compact".into(),
                severity: "warning".into(),
                source_kind: "Estimated".into(),
                reason_summary: "older".into(),
                suggested_command: Some("/compact".into()),
                is_strong: true,
                dedup_key: "older".into(),
                threshold_snapshot_json: None,
            })
            .unwrap();
        let newer = sink
            .insert_recommendation_event(&RecommendationEventRecord {
                ts_unix_ms: 2_000,
                pane_id: "%1".into(),
                provider: Some("Codex".into()),
                role: Some("Main".into()),
                situation: "Context pressure".into(),
                action: "/compact".into(),
                severity: "warning".into(),
                source_kind: "Estimated".into(),
                reason_summary: "newer".into(),
                suggested_command: Some("/compact".into()),
                is_strong: true,
                dedup_key: "newer".into(),
                threshold_snapshot_json: None,
            })
            .unwrap();
        assert_ne!(older, newer);

        sink.insert_correlated_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: 2_500,
            recommendation_event_id: None,
            pane_id: "%1".into(),
            action: "/compact".into(),
            outcome: RecommendationOutcome::Accepted,
            audit_event_id: None,
            summary: "operator accepted".into(),
        })
        .unwrap();

        let store = crate::store::SqliteInsightsStore::open(&tmp.path().join("audit.db")).unwrap();
        let snapshot = store
            .snapshot_with_ignored_ttl(
                crate::store::InsightsWindow {
                    since_ms: 0,
                    until_ms: 3_000,
                },
                1,
            )
            .unwrap();
        let row = snapshot
            .actions
            .iter()
            .find(|row| row.action == "/compact")
            .expect("compact row");
        assert_eq!(row.accepted, 1);
    }
}
