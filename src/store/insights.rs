use std::path::Path;

use chrono::TimeZone as _;
use rusqlite::params;

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

const KNOWN_ACTION_PREFIXES: &[&str] = &[
    "archive-preview-suggested",
    "repeated-output-cache",
    "log-storm: ingress filter + summary",
    "repeated-output: result-hash cache",
    "aggressive: drop non-essential ingress",
    "aggressive: terse profile + archive",
    "aggressive: clamp output, archive all",
    "aggressive: strip attribution",
    "aggressive: dedupe + hash",
    "code-exploration: graph/symbol",
    "context-pressure: checkpoint",
    "context-pressure: act now",
    "cache: avoid /compact while cache is hot",
    "cache: /compact is safe - cache is cold",
    "cache: /compact is safe — cache is cold",
    "cache: drift detected - /compact will let cache rebuild",
    "cache: drift detected — /compact will let cache rebuild",
    "quota: pause until 5h window resets",
    "quota: pause until weekly window resets",
    "snapshot before 5h window resets",
    "snapshot before weekly window resets",
    "verbose-output",
    "verbose-review: terse profile",
    "pane-state",
    "anomaly: cross-pane edit cluster detected",
    "anomaly: cache discontinuity detected",
    "anomaly: token slope detected",
    "anomaly: cost slope detected",
    "anomaly: error burst detected",
    "anomaly: identity churn detected",
    "anomaly: memory growth detected",
    "anomaly: subagent activity correlated with other anomalies",
    "quota-tight: consider enabling",
    "quota-pressure: pace requests",
    "cost-budget: 80% reached",
    "cost-budget: exhausted",
    "profile-switch: elevated error rate",
    "provider-profile: switch suggested",
];

const COLON_ACTION_FAMILIES: &[&str] = &[
    "agent-memory",
    "auto-memory",
    "identity-drift",
    "security-posture",
    "quota-pressure",
    "cost-pressure",
    "provider-profile",
    "cache",
    "anomaly",
    "aggressive",
    "context-pressure",
    "code-exploration",
    "verbose-review",
    "profile-switch",
    "cost-budget",
];

fn summary_has_action_prefix(summary: &str, prefix: &str) -> bool {
    summary == prefix
        || summary
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(": "))
}

fn action_from_known_family(summary: &str) -> Option<&str> {
    for family in COLON_ACTION_FAMILIES {
        let Some(rest) = summary
            .strip_prefix(family)
            .and_then(|rest| rest.strip_prefix(": "))
        else {
            continue;
        };

        return Some(match rest.split_once(": ") {
            Some((detail, _)) => &summary[..family.len() + 2 + detail.len()],
            None => summary,
        });
    }
    None
}

fn action_from_summary(summary: &str) -> &str {
    for prefix in KNOWN_ACTION_PREFIXES {
        if summary_has_action_prefix(summary, prefix) {
            return prefix;
        }
    }
    if let Some(action) = action_from_known_family(summary) {
        return action;
    }
    summary
        .split_once(": ")
        .map(|(action, _)| action)
        .unwrap_or(summary)
}

fn rfc3339_from_ms(ms: i64) -> String {
    chrono::Utc
        .timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(|| chrono::Utc.timestamp_millis_opt(0).single().unwrap())
        .to_rfc3339()
}

fn window_rfc3339_bounds(window: InsightsWindow) -> (String, String) {
    (
        rfc3339_from_ms(window.since_ms),
        rfc3339_from_ms(window.until_ms),
    )
}

fn cache_ratio(input: Option<i64>, cached: Option<i64>) -> Option<f64> {
    let cached = cached? as f64;
    let input = input.unwrap_or(0) as f64;
    let total = input + cached;
    if total <= 0.0 {
        None
    } else {
        Some((cached / total * 100.0).round() / 100.0)
    }
}

fn cache_state_from_action(action: &str) -> Option<&'static str> {
    if action == "cache: avoid /compact while cache is hot" {
        Some("hot")
    } else if action == "cache: /compact is safe - cache is cold"
        || action == "cache: /compact is safe — cache is cold"
    {
        Some("cold")
    } else if action == "cache: drift detected - /compact will let cache rebuild"
        || action == "cache: drift detected — /compact will let cache rebuild"
    {
        Some("drift")
    } else {
        None
    }
}

fn situation_for_action(action: &str) -> &'static str {
    if matches!(
        action,
        "archive-preview-suggested"
            | "repeated-output-cache"
            | "log-storm: ingress filter + summary"
            | "repeated-output: result-hash cache"
            | "aggressive: drop non-essential ingress"
            | "aggressive: dedupe + hash"
    ) || action.contains("log-storm")
        || action.contains("repeated-output")
    {
        "Log storm / repeated output"
    } else if matches!(
        action,
        "anomaly: cross-pane edit cluster detected"
            | "anomaly: identity churn detected"
            | "anomaly: subagent activity correlated with other anomalies"
    ) || action.contains("code-exploration")
        || action.starts_with("identity-drift:")
        || action.contains("cross-pane")
        || action.contains("ConcurrentFileEdit")
    {
        "Code exploration"
    } else if matches!(
        action,
        "aggressive: terse profile + archive"
            | "aggressive: clamp output, archive all"
            | "anomaly: cache discontinuity detected"
            | "anomaly: token slope detected"
            | "anomaly: memory growth detected"
    ) || action.contains("context-pressure")
        || action.starts_with("agent-memory:")
        || action.starts_with("cache")
        || action.starts_with("quota: pause")
        || action.starts_with("snapshot before")
    {
        "Context pressure"
    } else if matches!(action, "aggressive: strip attribution")
        || action.contains("verbose-review")
        || action == "verbose-output"
    {
        "Verbose review"
    } else if action == "pane-state" || action.contains("permission") || action.contains("input") {
        "Input / permission wait"
    } else if matches!(
        action,
        "anomaly: cost slope detected" | "anomaly: error burst detected"
    ) || action.contains("quota-pressure")
        || action.contains("quota-tight")
        || action.contains("cost-budget")
        || action.contains("cost-pressure")
        || action.contains("profile-switch")
        || action.contains("provider-profile")
    {
        "Quota-tight / cost"
    } else {
        "Other"
    }
}

impl SqliteInsightsStore {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
        })
    }

    pub fn snapshot(&self, window: InsightsWindow) -> Result<InsightsSnapshot, SqliteError> {
        let _conn = self
            .db
            .connection()
            .lock()
            .expect("insights db lock poisoned");
        let (since_ts, until_ts) = window_rfc3339_bounds(window);
        let mut stmt = _conn
            .prepare_cached(
                "SELECT summary FROM audit_events \
                 WHERE kind IN ('RecommendationEmitted', 'AlertFired') \
                   AND ts_utc >= ?1 \
                   AND ts_utc <= ?2 \
                 ORDER BY id ASC",
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let rows = stmt
            .query_map(params![&since_ts, &until_ts], |row| row.get::<_, String>(0))
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let mut counts = std::collections::BTreeMap::<&'static str, u64>::new();
        for row in rows {
            let summary = row.map_err(|e| SqliteError::Query(e.to_string()))?;
            let action = action_from_summary(&summary);
            let situation = situation_for_action(action);
            *counts.entry(situation).or_default() += 1;
        }
        let situations = counts
            .into_iter()
            .map(|(situation, emitted)| SituationSummary { situation, emitted })
            .collect();
        let mut cache = CacheInsightSummary::default();

        let mut stmt = _conn
            .prepare_cached(
                "SELECT summary FROM audit_events \
                 WHERE kind IN ('RecommendationEmitted', 'AlertFired') \
                   AND ts_utc >= ?1 \
                   AND ts_utc <= ?2 \
                 ORDER BY id ASC",
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let rows = stmt
            .query_map(params![&since_ts, &until_ts], |row| row.get::<_, String>(0))
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        for row in rows {
            let summary = row.map_err(|e| SqliteError::Query(e.to_string()))?;
            let action = action_from_summary(&summary);
            match cache_state_from_action(action) {
                Some("hot") => cache.hot_count += 1,
                Some("cold") => cache.cold_count += 1,
                Some("drift") => cache.drift_count += 1,
                _ => {}
            }
        }

        let mut stmt = _conn
            .prepare_cached(
                "SELECT input_tokens, cached_input_tokens \
                 FROM token_usage_samples \
                 WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2 \
                 ORDER BY ts_unix_ms ASC, id ASC",
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let rows = stmt
            .query_map(params![window.since_ms, window.until_ms], |row| {
                Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
            })
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let mut first_input = None;
        let mut latest_input = None;
        let mut latest_ratio = None;
        for row in rows {
            let (input_tokens, cached_input_tokens) =
                row.map_err(|e| SqliteError::Query(e.to_string()))?;
            if let Some(input_tokens) = input_tokens {
                if first_input.is_none() {
                    first_input = Some(input_tokens);
                }
                latest_input = Some(input_tokens);
            }
            if let Some(ratio) = cache_ratio(input_tokens, cached_input_tokens) {
                latest_ratio = Some(ratio);
            }
        }
        cache.latest_cache_ratio = latest_ratio;
        cache.token_growth =
            first_input.zip(latest_input).map(
                |(first, latest)| {
                    if latest >= first { latest - first } else { 0 }
                },
            );

        cache.cost_delta_usd = _conn
            .query_row(
                "SELECT SUM(cost_usd_delta) FROM cost_usage_events \
                 WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2",
                params![window.since_ms, window.until_ms],
                |row| row.get::<_, Option<f64>>(0),
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;

        Ok(InsightsSnapshot {
            window,
            situations,
            cache,
            timeline: Vec::new(),
            actions: Vec::new(),
            ignored_available: false,
        })
    }
}
