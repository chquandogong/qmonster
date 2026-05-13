//! Phase 7 v3 (c) (v1.47.0): persistence sink for anomaly events and
//! per-pane AnomalyHistory deque snapshots. Wraps an `AuditDb`.

use std::path::Path;

use crate::app::event_loop::current_unix_ms;
use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyEvidence, AnomalyKind};
use crate::domain::recommendation::Severity;
use crate::store::anomaly_history::AnomalyHistorySnapshot;
use crate::store::sqlite::{AuditDb, SqliteError};

pub struct SqliteAnomalySink {
    db: AuditDb,
}

impl SqliteAnomalySink {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
        })
    }

    pub fn insert_anomaly_event(&self, event: &AnomalyEvent) -> Result<(), SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let evidence_json = encode_evidence_json(&event.evidence)?;
        conn.execute(
            "INSERT INTO anomaly_events
             (ts_unix_secs, pane_id, kind, confidence, severity, promoted, reason, evidence_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                event.timestamp as i64,
                event.pane_id,
                event.kind.label(),
                event.confidence.label(),
                event.severity.label(),
                event.promoted as i64,
                event.reason,
                evidence_json,
            ],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(())
    }

    pub fn upsert_anomaly_history_snapshot(
        &self,
        pane_id: &str,
        tick_unix_secs: u64,
        snap: &AnomalyHistorySnapshot,
    ) -> Result<(), SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let (identity_provider, identity_path) = match &snap.identity {
            Some((p, path)) => (Some(p.as_str()), Some(path.as_str())),
            None => (None, None),
        };
        let cross_paths_json = serde_json::to_string(&snap.cross_pane_edit_paths)
            .map_err(|e| SqliteError::Query(format!("json encode: {e}")))?;
        conn.execute(
            "INSERT OR REPLACE INTO anomaly_history_snapshots
             (pane_id, tick_unix_secs, identity_provider, identity_path,
              error_hint, cache_hit_ratio, cache_drift_fire,
              cross_pane_edit_paths_json, cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                pane_id,
                tick_unix_secs as i64,
                identity_provider,
                identity_path,
                snap.error_hint.map(|b| b as i64),
                snap.cache_hit_ratio.map(|r| r as f64),
                snap.cache_drift_fire as i64,
                cross_paths_json,
                snap.cost_usd,
            ],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(())
    }

    pub fn with_transaction<F, R>(&self, f: F) -> Result<R, SqliteError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<R, SqliteError>,
    {
        let mut conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let result = f(&tx)?;
        tx.commit().map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(result)
    }

    pub fn fetch_recent_anomaly_events(&self, limit: usize) -> Vec<AnomalyEvent> {
        let conn = match self.db.connection().lock() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_anomaly_events lock: {e}");
                return Vec::new();
            }
        };
        let sql = if table_has_column(&conn, "anomaly_events", "evidence_json") {
            "SELECT ts_unix_secs, pane_id, kind, confidence, severity, promoted, reason, evidence_json
             FROM anomaly_events
             ORDER BY ts_unix_secs DESC, id DESC
             LIMIT ?1"
        } else {
            "SELECT ts_unix_secs, pane_id, kind, confidence, severity, promoted, reason, '[]'
             FROM anomaly_events
             ORDER BY ts_unix_secs DESC, id DESC
             LIMIT ?1"
        };
        let mut stmt = match conn.prepare_cached(sql) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_anomaly_events prepare: {e}");
                return Vec::new();
            }
        };
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            let ts: i64 = row.get(0)?;
            let pane_id: String = row.get(1)?;
            let kind_str: String = row.get(2)?;
            let conf_str: String = row.get(3)?;
            let sev_str: String = row.get(4)?;
            let promoted_i: i64 = row.get(5)?;
            let reason: String = row.get(6)?;
            let evidence_json: String = row.get(7)?;
            Ok((
                ts,
                pane_id,
                kind_str,
                conf_str,
                sev_str,
                promoted_i,
                reason,
                evidence_json,
            ))
        });
        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_anomaly_events query: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for row in rows.flatten() {
            let (ts, pane_id, kind_s, conf_s, sev_s, promoted_i, reason, evidence_json) = row;
            let Some(kind) = AnomalyKind::try_from_label(&kind_s) else {
                continue;
            };
            let Some(confidence) = AnomalyConfidence::try_from_label(&conf_s) else {
                continue;
            };
            let Some(severity) = Severity::try_from_label(&sev_s) else {
                continue;
            };
            out.push(AnomalyEvent {
                timestamp: ts as u64,
                pane_id,
                kind,
                confidence,
                severity,
                promoted: promoted_i != 0,
                reason,
                evidence: decode_evidence_json(&evidence_json),
            });
        }
        out
    }

    pub fn fetch_recent_history_snapshots(
        &self,
        pane_id: &str,
        window_polls: usize,
    ) -> Vec<(u64, AnomalyHistorySnapshot)> {
        let conn = match self.db.connection().lock() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_history_snapshots lock: {e}");
                return Vec::new();
            }
        };
        let mut stmt = match conn.prepare_cached(
            "SELECT tick_unix_secs, identity_provider, identity_path, error_hint,
                    cache_hit_ratio, cache_drift_fire, cross_pane_edit_paths_json,
                    cost_usd
             FROM anomaly_history_snapshots
             WHERE pane_id = ?1
             ORDER BY tick_unix_secs DESC
             LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_history_snapshots prepare: {e}");
                return Vec::new();
            }
        };
        let rows = stmt.query_map(rusqlite::params![pane_id, window_polls as i64], |row| {
            let tick: i64 = row.get(0)?;
            let identity_provider: Option<String> = row.get(1)?;
            let identity_path: Option<String> = row.get(2)?;
            let error_hint: Option<i64> = row.get(3)?;
            let cache_hit_ratio: Option<f64> = row.get(4)?;
            let cache_drift_fire: i64 = row.get(5)?;
            let cross_paths_json: Option<String> = row.get(6)?;
            let cost_usd: Option<f64> = row.get(7)?;
            Ok((
                tick,
                identity_provider,
                identity_path,
                error_hint,
                cache_hit_ratio,
                cache_drift_fire,
                cross_paths_json,
                cost_usd,
            ))
        });
        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_history_snapshots query: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for row in rows.flatten() {
            let (
                tick,
                provider,
                path,
                error_hint,
                cache_hit_ratio,
                cache_drift_fire,
                cross_paths_json,
                cost_usd,
            ) = row;
            let identity = match (provider, path) {
                (Some(p), Some(pa)) => Some((p, pa)),
                _ => None,
            };
            let cross_pane_edit_paths: Vec<String> = cross_paths_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            out.push((
                tick as u64,
                AnomalyHistorySnapshot {
                    identity,
                    error_hint: error_hint.map(|i| i != 0),
                    cache_hit_ratio: cache_hit_ratio.map(|r| r as f32),
                    cache_drift_fire: cache_drift_fire != 0,
                    cross_pane_edit_paths,
                    cost_usd,
                },
            ));
        }
        out
    }

    pub fn run_retention(
        &self,
        retention_days: u64,
        window_polls: usize,
        tick_seconds: u64,
    ) -> Result<(), SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let now_secs = (current_unix_ms() / 1000) as u64;

        let events_threshold = now_secs.saturating_sub(retention_days * 86_400);
        conn.execute(
            "DELETE FROM anomaly_events WHERE ts_unix_secs < ?1",
            rusqlite::params![events_threshold as i64],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;

        conn.execute(
            "DELETE FROM anomaly_events
             WHERE id NOT IN (
                 SELECT id FROM anomaly_events ORDER BY id DESC LIMIT 100000
             )",
            [],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;

        let snapshots_threshold = now_secs.saturating_sub((window_polls as u64) * tick_seconds * 4);
        conn.execute(
            "DELETE FROM anomaly_history_snapshots WHERE tick_unix_secs < ?1",
            rusqlite::params![snapshots_threshold as i64],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;

        Ok(())
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PersistedEvidence {
    metric_name: String,
    before: String,
    after: String,
    sample_count: usize,
    source_kind: crate::domain::origin::SourceKind,
}

pub(crate) fn encode_evidence_json(evidence: &[AnomalyEvidence]) -> Result<String, SqliteError> {
    let persisted: Vec<PersistedEvidence> = evidence
        .iter()
        .map(|row| PersistedEvidence {
            metric_name: row.metric_name.to_string(),
            before: row.before.clone(),
            after: row.after.clone(),
            sample_count: row.sample_count,
            source_kind: row.source_kind,
        })
        .collect();
    serde_json::to_string(&persisted).map_err(|e| SqliteError::Query(format!("json: {e}")))
}

fn decode_evidence_json(raw: &str) -> Vec<AnomalyEvidence> {
    let Ok(rows) = serde_json::from_str::<Vec<PersistedEvidence>>(raw) else {
        return Vec::new();
    };
    rows.into_iter()
        .map(|row| AnomalyEvidence {
            metric_name: Box::leak(row.metric_name.into_boxed_str()),
            before: row.before,
            after: row.after,
            sample_count: row.sample_count,
            source_kind: row.source_kind,
        })
        .collect()
}

fn table_has_column(conn: &rusqlite::Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({table})");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return false;
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return false;
    };
    rows.flatten().any(|name| name == column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
    use crate::domain::recommendation::Severity;

    fn temp_sink() -> (tempfile::TempDir, SqliteAnomalySink) {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("audit.db");
        let sink = SqliteAnomalySink::open(&path).unwrap();
        (tmp, sink)
    }

    fn fixture_event(ts: u64) -> AnomalyEvent {
        AnomalyEvent {
            timestamp: ts,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: format!("cost_usd at ts={ts}"),
            evidence: Vec::new(),
        }
    }

    fn fixture_snapshot() -> AnomalyHistorySnapshot {
        AnomalyHistorySnapshot {
            identity: Some(("Codex".to_string(), "/r".to_string())),
            error_hint: Some(true),
            cache_hit_ratio: Some(0.42),
            cache_drift_fire: true,
            cross_pane_edit_paths: vec!["src/a.rs".to_string()],
            cost_usd: Some(2.5),
        }
    }

    #[test]
    fn insert_and_fetch_anomaly_event_roundtrip() {
        let (_tmp, sink) = temp_sink();
        let event = fixture_event(1_700_000_000);
        sink.insert_anomaly_event(&event).unwrap();
        let fetched = sink.fetch_recent_anomaly_events(10);
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0], event);
    }

    #[test]
    fn fetch_recent_anomaly_events_orders_newest_first() {
        let (_tmp, sink) = temp_sink();
        for ts in [1, 3, 2u64] {
            sink.insert_anomaly_event(&fixture_event(ts)).unwrap();
        }
        let events = sink.fetch_recent_anomaly_events(10);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].timestamp, 3);
        assert_eq!(events[1].timestamp, 2);
        assert_eq!(events[2].timestamp, 1);
    }

    #[test]
    fn upsert_history_snapshot_replaces_on_conflict() {
        let (_tmp, sink) = temp_sink();
        let mut snap = fixture_snapshot();
        sink.upsert_anomaly_history_snapshot("%1", 1_700_000_000, &snap)
            .unwrap();
        snap.cost_usd = Some(99.99);
        sink.upsert_anomaly_history_snapshot("%1", 1_700_000_000, &snap)
            .unwrap();
        let snaps = sink.fetch_recent_history_snapshots("%1", 10);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].1.cost_usd, Some(99.99));
    }

    #[test]
    fn fetch_recent_history_snapshots_filters_by_pane_id() {
        let (_tmp, sink) = temp_sink();
        let snap = fixture_snapshot();
        sink.upsert_anomaly_history_snapshot("%1", 100, &snap)
            .unwrap();
        sink.upsert_anomaly_history_snapshot("%2", 200, &snap)
            .unwrap();
        let only_pane1 = sink.fetch_recent_history_snapshots("%1", 10);
        assert_eq!(only_pane1.len(), 1);
        assert_eq!(only_pane1[0].0, 100);
    }

    #[test]
    fn snapshot_handles_absent_optional_fields() {
        let (_tmp, sink) = temp_sink();
        let snap = AnomalyHistorySnapshot {
            identity: None,
            error_hint: None,
            cache_hit_ratio: None,
            cache_drift_fire: false,
            cross_pane_edit_paths: Vec::new(),
            cost_usd: None,
        };
        sink.upsert_anomaly_history_snapshot("%1", 100, &snap)
            .unwrap();
        let fetched = sink.fetch_recent_history_snapshots("%1", 1);
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].1, snap);
    }
}
