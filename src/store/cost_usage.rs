//! SQLite-backed USD cost ledger.
//!
//! Providers surface `cost_usd` as a cumulative session value. Summing
//! raw samples would over-count, so this writer records only positive
//! deltas per pane. A lower cumulative value is treated as a new
//! session and counted from that new value.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OptionalExtension, params};

use crate::app::config::CostConfig;
use crate::domain::identity::Provider;
use crate::store::sqlite::{AuditDb, SqliteError};

const MIN_COST_DELTA_USD: f64 = 0.000_001;
const WARNING_THRESHOLD_BPS: i64 = 8_000;
const CRITICAL_THRESHOLD_BPS: i64 = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub struct CostObservation {
    pub ts_unix_ms: i64,
    pub pane_id: String,
    pub provider: Provider,
    pub cumulative_cost_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostBudgetAlertLevel {
    Warning80,
    Critical100,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CostBudgetAlert {
    pub level: CostBudgetAlertLevel,
    pub threshold_pct: f64,
    pub spent_usd: f64,
    pub budget_usd: f64,
}

pub struct SqliteCostUsageSink {
    db: AuditDb,
    error_count: AtomicU64,
}

impl SqliteCostUsageSink {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
            error_count: AtomicU64::new(0),
        })
    }

    pub fn error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }

    pub fn record_observation(&self, obs: &CostObservation) -> Result<Option<f64>, SqliteError> {
        let conn = self.db.connection().lock().expect("cost db lock poisoned");
        let res = record_observation_via(&conn, obs);
        if res.is_err() {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }
        res
    }

    pub fn total_spent_usd(&self) -> Result<f64, SqliteError> {
        let conn = self.db.connection().lock().expect("cost db lock poisoned");
        total_spent_usd_via(&conn)
    }

    pub fn claim_budget_alerts(
        &self,
        cost: &CostConfig,
        now_unix_ms: i64,
    ) -> Result<Vec<CostBudgetAlert>, SqliteError> {
        let conn = self.db.connection().lock().expect("cost db lock poisoned");
        claim_budget_alerts_via(&conn, cost, now_unix_ms)
    }
}

fn record_observation_via(
    conn: &Connection,
    obs: &CostObservation,
) -> Result<Option<f64>, SqliteError> {
    if !obs.cumulative_cost_usd.is_finite() || obs.cumulative_cost_usd < 0.0 {
        return Ok(None);
    }

    let previous: Option<f64> = conn
        .query_row(
            "SELECT cumulative_cost_usd \
             FROM cost_usage_events \
             WHERE pane_id = ? \
             ORDER BY ts_unix_ms DESC, id DESC \
             LIMIT 1",
            params![obs.pane_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| SqliteError::Query(e.to_string()))?;

    let delta = match previous {
        Some(prev) if obs.cumulative_cost_usd >= prev => obs.cumulative_cost_usd - prev,
        Some(_) => obs.cumulative_cost_usd,
        None => obs.cumulative_cost_usd,
    };
    if delta <= MIN_COST_DELTA_USD {
        return Ok(None);
    }

    conn.execute(
        "INSERT INTO cost_usage_events \
         (ts_unix_ms, pane_id, provider, cost_usd_delta, cumulative_cost_usd) \
         VALUES (?, ?, ?, ?, ?)",
        params![
            obs.ts_unix_ms,
            obs.pane_id,
            provider_to_str(obs.provider),
            delta,
            obs.cumulative_cost_usd,
        ],
    )
    .map_err(|e| SqliteError::Query(e.to_string()))?;
    Ok(Some(delta))
}

fn total_spent_usd_via(conn: &Connection) -> Result<f64, SqliteError> {
    conn.query_row(
        "SELECT COALESCE(SUM(cost_usd_delta), 0.0) FROM cost_usage_events",
        [],
        |row| row.get(0),
    )
    .map_err(|e| SqliteError::Query(e.to_string()))
}

fn claim_budget_alerts_via(
    conn: &Connection,
    cost: &CostConfig,
    now_unix_ms: i64,
) -> Result<Vec<CostBudgetAlert>, SqliteError> {
    if !cost.budget_enabled() {
        return Ok(Vec::new());
    }
    let spent = total_spent_usd_via(conn)?;
    let budget_usd = cost.budget_usd;
    let budget_cents = (budget_usd * 100.0).round() as i64;
    let thresholds = [
        (WARNING_THRESHOLD_BPS, CostBudgetAlertLevel::Warning80),
        (CRITICAL_THRESHOLD_BPS, CostBudgetAlertLevel::Critical100),
    ];
    let mut out = Vec::new();
    for (threshold_bps, level) in thresholds {
        let threshold_pct = threshold_bps as f64 / 10_000.0;
        if spent + MIN_COST_DELTA_USD < budget_usd * threshold_pct {
            continue;
        }
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO cost_budget_alerts \
                 (threshold_bps, budget_cents, fired_at_unix_ms, spent_usd, budget_usd) \
                 VALUES (?, ?, ?, ?, ?)",
                params![threshold_bps, budget_cents, now_unix_ms, spent, budget_usd],
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        if changed == 1 {
            out.push(CostBudgetAlert {
                level,
                threshold_pct,
                spent_usd: spent,
                budget_usd,
            });
        }
    }
    Ok(out)
}

fn provider_to_str(p: Provider) -> &'static str {
    match p {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Gemini => "Gemini",
        Provider::Antigravity => "Antigravity",
        Provider::Qmonster => "Qmonster",
        Provider::Unknown => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn obs(ts: i64, pane_id: &str, cumulative_cost_usd: f64) -> CostObservation {
        CostObservation {
            ts_unix_ms: ts,
            pane_id: pane_id.into(),
            provider: Provider::Codex,
            cumulative_cost_usd,
        }
    }

    #[test]
    fn records_positive_cost_deltas_without_double_counting_repeated_samples() {
        let td = tempdir().unwrap();
        let sink = SqliteCostUsageSink::open(&td.path().join("q.db")).unwrap();

        assert_eq!(
            sink.record_observation(&obs(100, "%1", 10.0)).unwrap(),
            Some(10.0)
        );
        assert_eq!(
            sink.record_observation(&obs(200, "%1", 12.5)).unwrap(),
            Some(2.5)
        );
        assert_eq!(
            sink.record_observation(&obs(300, "%1", 12.5)).unwrap(),
            None
        );

        assert!((sink.total_spent_usd().unwrap() - 12.5).abs() < 1e-9);
    }

    #[test]
    fn lower_cumulative_cost_counts_as_new_session_delta() {
        let td = tempdir().unwrap();
        let sink = SqliteCostUsageSink::open(&td.path().join("q.db")).unwrap();

        sink.record_observation(&obs(100, "%1", 10.0)).unwrap();
        sink.record_observation(&obs(200, "%1", 2.25)).unwrap();

        assert!((sink.total_spent_usd().unwrap() - 12.25).abs() < 1e-9);
    }

    #[test]
    fn budget_alerts_fire_once_at_80_and_100_percent() {
        let td = tempdir().unwrap();
        let sink = SqliteCostUsageSink::open(&td.path().join("q.db")).unwrap();
        let cost = CostConfig {
            budget_usd: 10.0,
            ..CostConfig::default()
        };

        sink.record_observation(&obs(100, "%1", 8.0)).unwrap();
        let alerts = sink.claim_budget_alerts(&cost, 150).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, CostBudgetAlertLevel::Warning80);
        assert_eq!(
            sink.claim_budget_alerts(&cost, 160).unwrap(),
            Vec::<CostBudgetAlert>::new()
        );

        sink.record_observation(&obs(200, "%1", 10.0)).unwrap();
        let alerts = sink.claim_budget_alerts(&cost, 250).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, CostBudgetAlertLevel::Critical100);
    }

    #[test]
    fn nonpositive_budget_disables_alert_claims() {
        let td = tempdir().unwrap();
        let sink = SqliteCostUsageSink::open(&td.path().join("q.db")).unwrap();
        let cost = CostConfig {
            budget_usd: 0.0,
            ..CostConfig::default()
        };

        sink.record_observation(&obs(100, "%1", 100.0)).unwrap();

        assert!(sink.claim_budget_alerts(&cost, 150).unwrap().is_empty());
    }
}
