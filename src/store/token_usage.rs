//! Phase F F-3 (v1.24.0): SQLite-backed time-series writer for
//! per-pane token usage samples. Reuses the same `qmonster.db` file
//! the audit sink owns; the table schema is applied alongside the
//! audit schema in `AuditDb::open`.
//!
//! `record_sample` writes one row per pane per poll cycle when at
//! least one of `input_tokens` / `output_tokens` / `cost_usd` is
//! `Some(...)` on the resolved `SignalSet`. `recent_samples` returns
//! the newest N rows for a pane in `ts_unix_ms DESC` order.
//!
//! Cumulative semantics: callers persist whatever absolute value the
//! provider surfaced (Codex bottom-status `1.51M in / 20.4K out` is a
//! session cumulative). UI consumers compute deltas between adjacent
//! samples to produce a meaningful "rate of context growth" sparkline.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, params};

use crate::domain::identity::Provider;
use crate::store::sqlite::{AuditDb, SqliteError};

#[derive(Debug, Clone, PartialEq)]
pub struct TokenSample {
    pub ts_unix_ms: i64,
    pub pane_id: String,
    pub provider: Provider,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    /// Phase F F-4 (v1.25.0): cached input tokens; sourced from Codex
    /// `/status` welcome panel (`+ N cached`). None for providers
    /// that don't surface cache info.
    pub cached_input_tokens: Option<u64>,
}

pub struct SqliteTokenUsageSink {
    db: AuditDb,
    error_count: AtomicU64,
}

impl SqliteTokenUsageSink {
    /// Phase F F-3 (v1.24.0): opens a `SqliteTokenUsageSink` against
    /// the given DB path. Note: today this constructs its own
    /// `AuditDb` — i.e., a second `rusqlite::Connection` to the same
    /// `qmonster.db` file the audit sink already opened. This is a
    /// tracked debt: the F-3 plan intended both writers to share a
    /// single `Connection` via a future `Db` facade. Until that
    /// refactor lands, both connections coexist (SQLite handles file-
    /// level locking; impact is purely cosmetic for the workload of
    /// 1 record_sample per pane per 5s).
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
            error_count: AtomicU64::new(0),
        })
    }

    pub fn error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }

    pub fn record_sample(&self, sample: &TokenSample) -> Result<(), SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .expect("token_usage db lock poisoned");
        let res = record_sample_via(&conn, sample);
        if res.is_err() {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }
        res
    }

    pub fn recent_samples(
        &self,
        pane_id: &str,
        limit: usize,
    ) -> Result<Vec<TokenSample>, SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .expect("token_usage db lock poisoned");
        recent_samples_via(&conn, pane_id, limit)
    }
}

fn record_sample_via(conn: &Connection, sample: &TokenSample) -> Result<(), SqliteError> {
    conn.execute(
        "INSERT INTO token_usage_samples \
         (ts_unix_ms, pane_id, provider, input_tokens, output_tokens, cost_usd, cached_input_tokens) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            sample.ts_unix_ms,
            sample.pane_id,
            provider_to_str(sample.provider),
            sample.input_tokens.map(|n| n as i64),
            sample.output_tokens.map(|n| n as i64),
            sample.cost_usd,
            sample.cached_input_tokens.map(|n| n as i64),
        ],
    )
    .map_err(|e| SqliteError::Query(e.to_string()))?;
    Ok(())
}

fn recent_samples_via(
    conn: &Connection,
    pane_id: &str,
    limit: usize,
) -> Result<Vec<TokenSample>, SqliteError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT ts_unix_ms, pane_id, provider, input_tokens, output_tokens, cost_usd, cached_input_tokens \
             FROM token_usage_samples \
             WHERE pane_id = ? \
             ORDER BY ts_unix_ms DESC \
             LIMIT ?",
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let rows = stmt
        .query_map(params![pane_id, limit as i64], |row| {
            Ok(TokenSample {
                ts_unix_ms: row.get(0)?,
                pane_id: row.get(1)?,
                provider: provider_from_str(&row.get::<_, String>(2)?),
                input_tokens: row.get::<_, Option<i64>>(3)?.map(|n| n as u64),
                output_tokens: row.get::<_, Option<i64>>(4)?.map(|n| n as u64),
                cost_usd: row.get(5)?,
                cached_input_tokens: row.get::<_, Option<i64>>(6)?.map(|n| n as u64),
            })
        })
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let mut out = Vec::with_capacity(limit);
    for row in rows {
        out.push(row.map_err(|e| SqliteError::Query(e.to_string()))?);
    }
    Ok(out)
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

fn provider_from_str(s: &str) -> Provider {
    match s {
        "Claude" => Provider::Claude,
        "Codex" => Provider::Codex,
        "Gemini" => Provider::Gemini,
        "Qmonster" => Provider::Qmonster,
        _ => Provider::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample(ts_unix_ms: i64, pane_id: &str, in_tok: Option<u64>) -> TokenSample {
        TokenSample {
            ts_unix_ms,
            pane_id: pane_id.into(),
            provider: Provider::Codex,
            input_tokens: in_tok,
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: None,
        }
    }

    #[test]
    fn record_sample_round_trip_returns_inserted_row() {
        let td = tempdir().unwrap();
        let sink = SqliteTokenUsageSink::open(&td.path().join("q.db")).unwrap();
        let original = TokenSample {
            ts_unix_ms: 1000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1234),
            output_tokens: Some(56),
            cost_usd: Some(0.078),
            cached_input_tokens: None,
        };
        sink.record_sample(&original).unwrap();
        let got = sink.recent_samples("%1", 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0], original);
    }

    #[test]
    fn recent_samples_returns_newest_first_capped_at_limit() {
        let td = tempdir().unwrap();
        let sink = SqliteTokenUsageSink::open(&td.path().join("q.db")).unwrap();
        for i in 0..5 {
            sink.record_sample(&sample((i * 100) as i64, "%1", Some(i as u64)))
                .unwrap();
        }
        let got = sink.recent_samples("%1", 3).unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].ts_unix_ms, 400);
        assert_eq!(got[1].ts_unix_ms, 300);
        assert_eq!(got[2].ts_unix_ms, 200);
    }

    #[test]
    fn recent_samples_filters_by_pane_id() {
        let td = tempdir().unwrap();
        let sink = SqliteTokenUsageSink::open(&td.path().join("q.db")).unwrap();
        sink.record_sample(&sample(100, "%1", Some(10))).unwrap();
        sink.record_sample(&sample(200, "%2", Some(20))).unwrap();
        sink.record_sample(&sample(300, "%1", Some(30))).unwrap();
        let got = sink.recent_samples("%1", 10).unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|s| s.pane_id == "%1"));
    }

    #[test]
    fn record_sample_accepts_none_in_token_fields() {
        let td = tempdir().unwrap();
        let sink = SqliteTokenUsageSink::open(&td.path().join("q.db")).unwrap();
        let s = TokenSample {
            ts_unix_ms: 500,
            pane_id: "%1".into(),
            provider: Provider::Claude,
            input_tokens: None,
            output_tokens: None,
            cost_usd: Some(0.42),
            cached_input_tokens: None,
        };
        sink.record_sample(&s).unwrap();
        let got = sink.recent_samples("%1", 1).unwrap();
        assert_eq!(got[0].input_tokens, None);
        assert_eq!(got[0].cost_usd, Some(0.42));
    }

    #[test]
    fn provider_round_trips_through_sqlite_for_every_variant() {
        let td = tempdir().unwrap();
        let sink = SqliteTokenUsageSink::open(&td.path().join("q.db")).unwrap();
        let providers = [
            Provider::Claude,
            Provider::Codex,
            Provider::Gemini,
            Provider::Qmonster,
            Provider::Unknown,
        ];
        for (i, p) in providers.iter().enumerate() {
            sink.record_sample(&TokenSample {
                ts_unix_ms: i as i64 * 100,
                pane_id: format!("%{}", i),
                provider: *p,
                input_tokens: None,
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: None,
            })
            .unwrap();
        }
        for (i, p) in providers.iter().enumerate() {
            let got = sink.recent_samples(&format!("%{}", i), 1).unwrap();
            assert_eq!(got[0].provider, *p, "provider must round-trip for {:?}", p);
        }
    }

    #[test]
    fn record_sample_round_trips_cached_input_tokens() {
        let td = tempdir().unwrap();
        let sink = SqliteTokenUsageSink::open(&td.path().join("q.db")).unwrap();
        let s = TokenSample {
            ts_unix_ms: 1000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(189_703),
            output_tokens: Some(20_355),
            cost_usd: None,
            cached_input_tokens: Some(1_317_376),
        };
        sink.record_sample(&s).unwrap();
        let got = sink.recent_samples("%1", 1).unwrap();
        assert_eq!(got[0].cached_input_tokens, Some(1_317_376));
        assert_eq!(got[0].input_tokens, Some(189_703));
    }

    #[test]
    fn record_sample_error_count_increments_on_forced_failure() {
        let td = tempdir().unwrap();
        let sink = SqliteTokenUsageSink::open(&td.path().join("q.db")).unwrap();
        // Sanity: counter starts at 0
        assert_eq!(sink.error_count(), 0);

        // Drop the table out from under the sink to force INSERT failure.
        {
            let conn = sink.db.connection().lock().unwrap();
            conn.execute("DROP TABLE token_usage_samples", []).unwrap();
        }

        let s = TokenSample {
            ts_unix_ms: 100,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: None,
        };
        let res = sink.record_sample(&s);
        assert!(res.is_err(), "INSERT into dropped table must fail");
        assert_eq!(sink.error_count(), 1);

        // Second failure increments to 2
        let _ = sink.record_sample(&s);
        assert_eq!(sink.error_count(), 2);
    }
}
