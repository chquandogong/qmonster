use std::path::Path;

use crate::store::sqlite::{AuditDb, SqliteError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsightsWindow {
    pub since_ms: i64,
    pub until_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SituationSummary {
    pub situation: &'static str,
    pub emitted: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CacheInsightSummary {
    pub hot_count: u64,
    pub cold_count: u64,
    pub drift_count: u64,
    pub latest_cache_ratio: Option<f64>,
    pub token_growth: Option<i64>,
    pub cost_delta_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionLedgerRow {
    pub action: String,
    pub emitted: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub blocked: u64,
    pub completed: u64,
    pub failed: u64,
    pub archived: u64,
    pub snapshot_written: u64,
    pub ignored: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecommendationTimelineItem {
    pub ts_label: String,
    pub pane_id: String,
    pub situation: &'static str,
    pub action: String,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InsightsSnapshot {
    pub window: InsightsWindow,
    pub situations: Vec<SituationSummary>,
    pub cache: CacheInsightSummary,
    pub timeline: Vec<RecommendationTimelineItem>,
    pub actions: Vec<ActionLedgerRow>,
    pub ignored_available: bool,
}

pub struct SqliteInsightsStore {
    db: AuditDb,
}

impl SqliteInsightsStore {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
        })
    }

    pub fn snapshot(&self, window: InsightsWindow) -> Result<InsightsSnapshot, SqliteError> {
        let _conn = self.db.connection().lock().expect("insights db lock poisoned");
        Ok(InsightsSnapshot {
            window,
            situations: Vec::new(),
            cache: CacheInsightSummary::default(),
            timeline: Vec::new(),
            actions: Vec::new(),
            ignored_available: false,
        })
    }
}
