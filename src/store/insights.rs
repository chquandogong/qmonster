use std::{cmp::Ordering, collections::BTreeMap, path::Path};

use chrono::TimeZone as _;
use rusqlite::{Connection, OptionalExtension, params};

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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneInsightBucket {
    pub pane_id: String,
    pub provider: String,
    pub latest_cache_ratio: Option<f64>,
    pub token_growth: Option<i64>,
    pub cost_delta_usd: Option<f64>,
    pub sample_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextBestActionItem {
    pub ts_label: String,
    pub pane_id: String,
    pub provider: Option<String>,
    pub situation: &'static str,
    pub action: String,
    pub severity: String,
    pub reason_summary: String,
    pub suggested_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextBestActionSummary {
    pub data_available: bool,
    pub action: Option<NextBestActionItem>,
    pub open_proposal_count: u64,
}

impl NextBestActionSummary {
    fn unavailable() -> Self {
        Self {
            data_available: false,
            action: None,
            open_proposal_count: 0,
        }
    }

    fn no_pending() -> Self {
        Self {
            data_available: true,
            action: None,
            open_proposal_count: 0,
        }
    }
}

impl Default for NextBestActionSummary {
    fn default() -> Self {
        Self::unavailable()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LastCompactPayoffSummary {
    pub status: String,
    pub outcome_ts_label: Option<String>,
    pub age_label: Option<String>,
    pub pane_id: Option<String>,
    pub input_tokens_saved: Option<i64>,
    pub cost_usd_saved: Option<f64>,
    pub before_cache_state: Option<String>,
    pub after_cache_state: Option<String>,
    pub sample_count: u64,
}

impl Default for LastCompactPayoffSummary {
    fn default() -> Self {
        Self {
            status: "n/a".into(),
            outcome_ts_label: None,
            age_label: None,
            pane_id: None,
            input_tokens_saved: None,
            cost_usd_saved: None,
            before_cache_state: None,
            after_cache_state: None,
            sample_count: 0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostBreakdownRow {
    pub label: String,
    pub cost_delta_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostBreakdownSummary {
    pub total_usd: f64,
    pub by_pane: Vec<CostBreakdownRow>,
    pub by_model: Vec<CostBreakdownRow>,
    pub by_situation: Vec<CostBreakdownRow>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrossPaneAnomalyCorrelation {
    pub window_label: String,
    pub pane_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActionImpactSummary {
    pub event_id: i64,
    pub outcome_ts_label: String,
    pub pane_id: String,
    pub provider: Option<String>,
    pub action: String,
    pub outcome: String,
    pub before_token_growth: Option<i64>,
    pub after_token_growth: Option<i64>,
    pub token_growth_delta: Option<i64>,
    pub before_cost_delta_usd: Option<f64>,
    pub after_cost_delta_usd: Option<f64>,
    pub cost_delta_usd: Option<f64>,
    pub before_cache_ratio: Option<f64>,
    pub after_cache_ratio: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneDataCompleteness {
    pub pane_id: String,
    pub provider: String,
    pub status: String,
    pub coverage_pct: Option<f64>,
    pub elapsed_secs: Option<u64>,
    pub missing_polls: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InsightsDataCompleteness {
    pub pane_bucket_count: usize,
    pub token_sample_count: u64,
    pub cache_ratio_bucket_count: usize,
    pub cost_delta_bucket_count: usize,
    pub action_impact_count: usize,
    pub panes: Vec<PaneDataCompleteness>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionLedgerRow {
    pub action: String,
    pub emitted: u64,
    pub copied: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub blocked: u64,
    pub completed: u64,
    pub failed: u64,
    pub archived: u64,
    pub snapshot_written: u64,
    pub hidden: u64,
    pub ignored: u64,
    pub avoided: u64,
    pub expired: u64,
    pub suppressed: u64,
    pub no_effect: u64,
    pub improved: u64,
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
    pub pane_buckets: Vec<PaneInsightBucket>,
    pub next_best_action: NextBestActionSummary,
    pub last_compact_payoff: LastCompactPayoffSummary,
    pub cost_breakdown: CostBreakdownSummary,
    pub anomaly_correlations: Vec<CrossPaneAnomalyCorrelation>,
    pub action_impacts: Vec<ActionImpactSummary>,
    pub data_completeness: InsightsDataCompleteness,
    pub timeline: Vec<RecommendationTimelineItem>,
    pub actions: Vec<ActionLedgerRow>,
    pub ignored_available: bool,
}

#[derive(Debug, Default)]
struct PaneInsightRollup {
    pane_buckets: Vec<PaneInsightBucket>,
    latest_cache_ratio: Option<f64>,
    token_growth: Option<i64>,
    cost_delta_usd: Option<f64>,
}

#[derive(Debug)]
struct PaneInsightAccumulator {
    pane_id: String,
    provider: String,
    latest_cache_ratio: Option<f64>,
    previous_input_tokens: Option<i64>,
    token_growth: i64,
    saw_input_tokens: bool,
    cost_delta_usd: Option<f64>,
    sample_count: u64,
}

impl PaneInsightAccumulator {
    fn new(pane_id: String, provider: String) -> Self {
        Self {
            pane_id,
            provider,
            latest_cache_ratio: None,
            previous_input_tokens: None,
            token_growth: 0,
            saw_input_tokens: false,
            cost_delta_usd: None,
            sample_count: 0,
        }
    }

    fn observe_token_sample(&mut self, input_tokens: Option<i64>, cache_ratio: Option<f64>) {
        self.sample_count += 1;
        if let Some(input_tokens) = input_tokens {
            if let Some(previous) = self.previous_input_tokens
                && input_tokens >= previous
            {
                self.token_growth += input_tokens - previous;
            }
            self.previous_input_tokens = Some(input_tokens);
            self.saw_input_tokens = true;
        }
        if let Some(cache_ratio) = cache_ratio {
            self.latest_cache_ratio = Some(cache_ratio);
        }
    }

    fn observe_cost_delta(&mut self, cost_delta_usd: f64) {
        self.cost_delta_usd = Some(self.cost_delta_usd.unwrap_or(0.0) + cost_delta_usd);
    }

    fn into_bucket(self) -> PaneInsightBucket {
        PaneInsightBucket {
            pane_id: self.pane_id,
            provider: self.provider,
            latest_cache_ratio: self.latest_cache_ratio,
            token_growth: self.saw_input_tokens.then_some(self.token_growth),
            cost_delta_usd: self.cost_delta_usd,
            sample_count: self.sample_count,
        }
    }
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
    crate::domain::anomaly::SUBAGENT_SIDE_EFFECT_ACTION,
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

fn prompt_command_from_summary(summary: &str) -> Option<String> {
    if let Some(arrow) = summary.find("->") {
        let after_arrow = &summary[arrow + 2..];
        let start = after_arrow.find('`')?;
        let rest = &after_arrow[start + 1..];
        let end = rest.find('`')?;
        return Some(rest[..end].to_string());
    }

    let mut tokens = summary.split_whitespace();
    let _target = tokens.next()?;
    let command = tokens.next()?;
    let command = command
        .trim_matches('`')
        .trim_end_matches(&[',', '.', ';', ':', '!', '?', ')', ']', '}'][..]);
    command.starts_with('/').then(|| command.to_string())
}

fn action_for_audit(kind: &str, summary: &str) -> String {
    match kind {
        "PromptSendAccepted"
        | "PromptSendCompleted"
        | "PromptSendFailed"
        | "PromptSendBlocked"
        | "PromptSendRejected" => {
            prompt_command_from_summary(summary).unwrap_or_else(|| "prompt-send".into())
        }
        "SnapshotWritten" => "snapshot".into(),
        "ArchiveWritten" => "archive".into(),
        _ => action_from_summary(summary).to_string(),
    }
}

fn apply_outcome(row: &mut ActionLedgerRow, kind: &str) {
    match kind {
        "RecommendationEmitted" | "AlertFired" => row.emitted += 1,
        "PromptSendAccepted" => row.accepted += 1,
        "PromptSendRejected" => row.rejected += 1,
        "PromptSendBlocked" => row.blocked += 1,
        "PromptSendCompleted" => row.completed += 1,
        "PromptSendFailed" => row.failed += 1,
        "ArchiveWritten" => row.archived += 1,
        "SnapshotWritten" => row.snapshot_written += 1,
        _ => {}
    }
}

fn outcome_for_kind(kind: &str) -> &'static str {
    match kind {
        "RecommendationEmitted" | "AlertFired" => "emitted",
        "PromptSendAccepted" => "accepted",
        "PromptSendRejected" => "rejected",
        "PromptSendBlocked" => "blocked",
        "PromptSendCompleted" => "completed",
        "PromptSendFailed" => "failed",
        "ArchiveWritten" => "archived",
        "SnapshotWritten" => "snapshot_written",
        _ => "observed",
    }
}

fn metric_delta_i64(before: Option<i64>, after: Option<i64>) -> Option<i64> {
    before.zip(after).map(|(before, after)| after - before)
}

fn metric_delta_f64(before: Option<f64>, after: Option<f64>) -> Option<f64> {
    before.zip(after).map(|(before, after)| after - before)
}

fn apply_lifecycle_outcome(row: &mut ActionLedgerRow, outcome: &str) {
    match outcome {
        "copied" => row.copied += 1,
        "accepted" => row.accepted += 1,
        "rejected" => row.rejected += 1,
        "completed" => row.completed += 1,
        "failed" => row.failed += 1,
        "blocked" => row.blocked += 1,
        "archived" => row.archived += 1,
        "snapshot_written" | "auto_snapshot_written" => row.snapshot_written += 1,
        "hidden" => row.hidden += 1,
        "ignored" => row.ignored += 1,
        "avoided" => row.avoided += 1,
        "expired" => row.expired += 1,
        "suppressed" => row.suppressed += 1,
        "no_effect" => row.no_effect += 1,
        "improved" => row.improved += 1,
        _ => {}
    }
}

// Terminal = closes the recommendation lifecycle so TTL-ignored counting no
// longer applies to the originating event. Engagement-only signals like
// `copied` must NOT close the lifecycle (the operator still hasn't accepted
// or dismissed the rec). The default is intentionally terminal so unknown /
// historical outcome strings fail closed: a new non-terminal outcome must
// opt in explicitly here.
fn is_terminal_lifecycle_outcome(outcome: &str) -> bool {
    !matches!(outcome, "copied")
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

fn table_exists(conn: &Connection, table: &str) -> Result<bool, SqliteError> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        params![table],
        |_| Ok(()),
    )
    .optional()
    .map(|row| row.is_some())
    .map_err(|e| SqliteError::Query(e.to_string()))
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, SqliteError> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn
        .prepare(&pragma)
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    for row in rows {
        if row.map_err(|e| SqliteError::Query(e.to_string()))? == column {
            return Ok(true);
        }
    }
    Ok(false)
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

fn bucket_entry(
    buckets: &mut BTreeMap<(String, String), PaneInsightAccumulator>,
    pane_id: String,
    provider: String,
) -> &mut PaneInsightAccumulator {
    buckets
        .entry((pane_id.clone(), provider.clone()))
        .or_insert_with(|| PaneInsightAccumulator::new(pane_id, provider))
}

fn sort_pane_buckets(buckets: &mut [PaneInsightBucket]) {
    buckets.sort_by(|a, b| {
        b.token_growth
            .unwrap_or(0)
            .cmp(&a.token_growth.unwrap_or(0))
            .then_with(|| {
                b.cost_delta_usd
                    .unwrap_or(0.0)
                    .partial_cmp(&a.cost_delta_usd.unwrap_or(0.0))
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| b.sample_count.cmp(&a.sample_count))
            .then_with(|| a.pane_id.cmp(&b.pane_id))
            .then_with(|| a.provider.cmp(&b.provider))
    });
}

fn collect_pane_insight_rollup(
    conn: &Connection,
    window: InsightsWindow,
) -> Result<PaneInsightRollup, SqliteError> {
    let mut buckets = BTreeMap::<(String, String), PaneInsightAccumulator>::new();
    let mut latest_cache_ratio = None;

    if table_exists(conn, "token_usage_samples")? {
        if table_has_column(conn, "token_usage_samples", "cached_input_tokens")? {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT pane_id, provider, input_tokens, cached_input_tokens \
                     FROM token_usage_samples \
                     WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2 \
                     ORDER BY ts_unix_ms ASC, id ASC",
                )
                .map_err(|e| SqliteError::Query(e.to_string()))?;
            let rows = stmt
                .query_map(params![window.since_ms, window.until_ms], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                })
                .map_err(|e| SqliteError::Query(e.to_string()))?;
            for row in rows {
                let (pane_id, provider, input_tokens, cached_input_tokens) =
                    row.map_err(|e| SqliteError::Query(e.to_string()))?;
                let ratio = cache_ratio(input_tokens, cached_input_tokens);
                bucket_entry(&mut buckets, pane_id, provider)
                    .observe_token_sample(input_tokens, ratio);
                if let Some(ratio) = ratio {
                    latest_cache_ratio = Some(ratio);
                }
            }
        } else {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT pane_id, provider, input_tokens \
                     FROM token_usage_samples \
                     WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2 \
                     ORDER BY ts_unix_ms ASC, id ASC",
                )
                .map_err(|e| SqliteError::Query(e.to_string()))?;
            let rows = stmt
                .query_map(params![window.since_ms, window.until_ms], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                })
                .map_err(|e| SqliteError::Query(e.to_string()))?;
            for row in rows {
                let (pane_id, provider, input_tokens) =
                    row.map_err(|e| SqliteError::Query(e.to_string()))?;
                bucket_entry(&mut buckets, pane_id, provider)
                    .observe_token_sample(input_tokens, None);
            }
        }
    }

    let mut cost_delta_usd = None;
    if table_exists(conn, "cost_usage_events")? {
        let mut stmt = conn
            .prepare_cached(
                "SELECT pane_id, provider, SUM(cost_usd_delta) \
                 FROM cost_usage_events \
                 WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2 \
                 GROUP BY pane_id, provider \
                 ORDER BY pane_id ASC, provider ASC",
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let rows = stmt
            .query_map(params![window.since_ms, window.until_ms], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        for row in rows {
            let (pane_id, provider, delta) = row.map_err(|e| SqliteError::Query(e.to_string()))?;
            bucket_entry(&mut buckets, pane_id, provider).observe_cost_delta(delta);
            cost_delta_usd = Some(cost_delta_usd.unwrap_or(0.0) + delta);
        }
    }

    let mut pane_buckets: Vec<PaneInsightBucket> = buckets
        .into_values()
        .map(PaneInsightAccumulator::into_bucket)
        .collect();
    sort_pane_buckets(&mut pane_buckets);

    let mut token_growth = None;
    for bucket in &pane_buckets {
        if let Some(growth) = bucket.token_growth {
            token_growth = Some(token_growth.unwrap_or(0) + growth);
        }
    }

    Ok(PaneInsightRollup {
        pane_buckets,
        latest_cache_ratio,
        token_growth,
        cost_delta_usd,
    })
}

fn collect_metric_window_for_pane(
    conn: &Connection,
    window: InsightsWindow,
    pane_id: &str,
    provider: Option<&str>,
) -> Result<MetricWindowSummary, SqliteError> {
    let mut buckets = BTreeMap::<(String, String), PaneInsightAccumulator>::new();
    let mut latest_cache_ratio = None;

    if table_exists(conn, "token_usage_samples")? {
        if table_has_column(conn, "token_usage_samples", "cached_input_tokens")? {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT pane_id, provider, input_tokens, cached_input_tokens \
                     FROM token_usage_samples \
                     WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2 \
                       AND pane_id = ?3 \
                       AND (?4 IS NULL OR provider = ?4) \
                     ORDER BY ts_unix_ms ASC, id ASC",
                )
                .map_err(|e| SqliteError::Query(e.to_string()))?;
            let rows = stmt
                .query_map(
                    params![window.since_ms, window.until_ms, pane_id, provider],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                        ))
                    },
                )
                .map_err(|e| SqliteError::Query(e.to_string()))?;
            for row in rows {
                let (pane_id, provider, input_tokens, cached_input_tokens) =
                    row.map_err(|e| SqliteError::Query(e.to_string()))?;
                let ratio = cache_ratio(input_tokens, cached_input_tokens);
                bucket_entry(&mut buckets, pane_id, provider)
                    .observe_token_sample(input_tokens, ratio);
                if let Some(ratio) = ratio {
                    latest_cache_ratio = Some(ratio);
                }
            }
        } else {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT pane_id, provider, input_tokens \
                     FROM token_usage_samples \
                     WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2 \
                       AND pane_id = ?3 \
                       AND (?4 IS NULL OR provider = ?4) \
                     ORDER BY ts_unix_ms ASC, id ASC",
                )
                .map_err(|e| SqliteError::Query(e.to_string()))?;
            let rows = stmt
                .query_map(
                    params![window.since_ms, window.until_ms, pane_id, provider],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    },
                )
                .map_err(|e| SqliteError::Query(e.to_string()))?;
            for row in rows {
                let (pane_id, provider, input_tokens) =
                    row.map_err(|e| SqliteError::Query(e.to_string()))?;
                bucket_entry(&mut buckets, pane_id, provider)
                    .observe_token_sample(input_tokens, None);
            }
        }
    }

    let mut cost_delta_usd = None;
    if table_exists(conn, "cost_usage_events")? {
        let mut stmt = conn
            .prepare_cached(
                "SELECT pane_id, provider, SUM(cost_usd_delta) \
                 FROM cost_usage_events \
                 WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2 \
                   AND pane_id = ?3 \
                   AND (?4 IS NULL OR provider = ?4) \
                 GROUP BY pane_id, provider",
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let rows = stmt
            .query_map(
                params![window.since_ms, window.until_ms, pane_id, provider],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, f64>(2)?,
                    ))
                },
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        for row in rows {
            let (pane_id, provider, delta) = row.map_err(|e| SqliteError::Query(e.to_string()))?;
            bucket_entry(&mut buckets, pane_id, provider).observe_cost_delta(delta);
            cost_delta_usd = Some(cost_delta_usd.unwrap_or(0.0) + delta);
        }
    }

    let mut token_growth = None;
    let mut sample_count = 0;
    for bucket in buckets
        .into_values()
        .map(PaneInsightAccumulator::into_bucket)
    {
        sample_count += bucket.sample_count;
        if let Some(growth) = bucket.token_growth {
            token_growth = Some(token_growth.unwrap_or(0) + growth);
        }
    }

    Ok(MetricWindowSummary {
        latest_cache_ratio,
        token_growth,
        cost_delta_usd,
        sample_count,
    })
}

const DATA_COMPLETENESS_EXPECTED_POLL_MS: i64 = 2_000;

fn data_completeness_for(
    conn: &Connection,
    window: InsightsWindow,
    pane_buckets: &[PaneInsightBucket],
    action_impacts: &[ActionImpactSummary],
) -> Result<InsightsDataCompleteness, SqliteError> {
    Ok(InsightsDataCompleteness {
        pane_bucket_count: pane_buckets.len(),
        token_sample_count: pane_buckets.iter().map(|bucket| bucket.sample_count).sum(),
        cache_ratio_bucket_count: pane_buckets
            .iter()
            .filter(|bucket| bucket.latest_cache_ratio.is_some())
            .count(),
        cost_delta_bucket_count: pane_buckets
            .iter()
            .filter(|bucket| bucket.cost_delta_usd.is_some())
            .count(),
        action_impact_count: action_impacts.len(),
        panes: collect_pane_data_completeness(conn, window)?,
    })
}

fn collect_pane_data_completeness(
    conn: &Connection,
    window: InsightsWindow,
) -> Result<Vec<PaneDataCompleteness>, SqliteError> {
    let mut panes = BTreeMap::<(String, String), PaneDataCompleteness>::new();
    if table_exists(conn, "token_usage_samples")? {
        let mut stmt = conn
            .prepare_cached(
                "SELECT pane_id, provider, COUNT(*), MIN(ts_unix_ms), MAX(ts_unix_ms)
                 FROM token_usage_samples
                 WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2
                 GROUP BY pane_id, provider",
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let rows = stmt
            .query_map(params![window.since_ms, window.until_ms], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        for row in rows {
            let (pane_id, provider, sample_count, min_ts, max_ts) =
                row.map_err(|e| SqliteError::Query(e.to_string()))?;
            let elapsed_ms = max_ts.saturating_sub(min_ts).max(0);
            let expected_polls =
                (elapsed_ms / DATA_COMPLETENESS_EXPECTED_POLL_MS + 1).max(1) as u64;
            let missing_polls = expected_polls.saturating_sub(sample_count);
            let coverage_pct = ((sample_count as f64 / expected_polls as f64) * 100.0).min(100.0);
            let coverage_pct = (coverage_pct * 10.0).round() / 10.0;
            let status = if matches!(provider.as_str(), "Qmonster" | "Unknown") {
                "unsupported"
            } else if coverage_pct >= 80.0 {
                "ok"
            } else {
                "?"
            };
            panes.insert(
                (pane_id.clone(), provider.clone()),
                PaneDataCompleteness {
                    pane_id,
                    provider,
                    status: status.into(),
                    coverage_pct: Some(coverage_pct),
                    elapsed_secs: Some((elapsed_ms / 1000) as u64),
                    missing_polls: Some(missing_polls),
                },
            );
        }
    }

    if table_exists(conn, "audit_events")? {
        let (since_utc, until_utc) = window_rfc3339_bounds(window);
        let mut stmt = conn
            .prepare_cached(
                "SELECT pane_id, provider
                 FROM audit_events
                 WHERE kind = 'IdentitySuppressed'
                   AND ts_utc >= ?1
                   AND ts_utc <= ?2
                 ORDER BY ts_utc ASC, id ASC",
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let rows = stmt
            .query_map(params![since_utc, until_utc], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?
                        .unwrap_or_else(|| "unknown".into()),
                ))
            })
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        for row in rows {
            let (pane_id, provider) = row.map_err(|e| SqliteError::Query(e.to_string()))?;
            panes.insert(
                (pane_id.clone(), provider.clone()),
                PaneDataCompleteness {
                    pane_id,
                    provider,
                    status: "suppressed".into(),
                    coverage_pct: None,
                    elapsed_secs: None,
                    missing_polls: None,
                },
            );
        }
    }

    Ok(panes.into_values().collect())
}

#[derive(Debug, Clone)]
struct CostEventRow {
    ts_unix_ms: i64,
    pane_id: String,
    provider: String,
    cost_delta_usd: f64,
}

#[derive(Debug, Clone)]
struct CostSituationEvent {
    ts_unix_ms: i64,
    pane_id: String,
    situation: &'static str,
}

fn collect_cost_breakdown(
    conn: &Connection,
    window: InsightsWindow,
) -> Result<CostBreakdownSummary, SqliteError> {
    if !table_exists(conn, "cost_usage_events")? {
        return Ok(CostBreakdownSummary::default());
    }
    let mut stmt = conn
        .prepare_cached(
            "SELECT ts_unix_ms, pane_id, provider, cost_usd_delta
             FROM cost_usage_events
             WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2
             ORDER BY ts_unix_ms ASC, id ASC",
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let rows = stmt
        .query_map(params![window.since_ms, window.until_ms], |row| {
            Ok(CostEventRow {
                ts_unix_ms: row.get(0)?,
                pane_id: row.get(1)?,
                provider: row.get(2)?,
                cost_delta_usd: row.get(3)?,
            })
        })
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let mut cost_rows = Vec::new();
    for row in rows {
        cost_rows.push(row.map_err(|e| SqliteError::Query(e.to_string()))?);
    }
    if cost_rows.is_empty() {
        return Ok(CostBreakdownSummary::default());
    }

    let situation_events = collect_cost_situation_events(conn, window)?;
    let mut by_pane = BTreeMap::<String, f64>::new();
    let mut by_model = BTreeMap::<String, f64>::new();
    let mut by_situation = BTreeMap::<String, f64>::new();
    let mut total_usd = 0.0;
    for row in cost_rows {
        total_usd += row.cost_delta_usd;
        *by_pane
            .entry(format!("{} {}", row.pane_id, row.provider))
            .or_default() += row.cost_delta_usd;
        *by_model.entry(row.provider.clone()).or_default() += row.cost_delta_usd;
        let situation = situation_for_cost_row(&row, &situation_events);
        *by_situation.entry(situation.to_string()).or_default() += row.cost_delta_usd;
    }

    Ok(CostBreakdownSummary {
        total_usd,
        by_pane: sorted_cost_rows(by_pane),
        by_model: sorted_cost_rows(by_model),
        by_situation: sorted_cost_rows(by_situation),
    })
}

fn collect_cost_situation_events(
    conn: &Connection,
    window: InsightsWindow,
) -> Result<Vec<CostSituationEvent>, SqliteError> {
    if !table_exists(conn, "recommendation_events")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare_cached(
            "SELECT ts_unix_ms, pane_id, situation, action
             FROM recommendation_events
             WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2
             ORDER BY ts_unix_ms ASC, id ASC",
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let rows = stmt
        .query_map(params![window.since_ms, window.until_ms], |row| {
            let situation: String = row.get(2)?;
            let action: String = row.get(3)?;
            Ok(CostSituationEvent {
                ts_unix_ms: row.get(0)?,
                pane_id: row.get(1)?,
                situation: static_situation_label(&situation, &action),
            })
        })
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| SqliteError::Query(e.to_string()))?);
    }
    Ok(out)
}

fn situation_for_cost_row<'a>(row: &CostEventRow, events: &'a [CostSituationEvent]) -> &'a str {
    events
        .iter()
        .filter(|event| event.pane_id == row.pane_id && event.ts_unix_ms <= row.ts_unix_ms)
        .max_by_key(|event| event.ts_unix_ms)
        .map(|event| event.situation)
        .unwrap_or("Other")
}

fn sorted_cost_rows(rows: BTreeMap<String, f64>) -> Vec<CostBreakdownRow> {
    let mut rows: Vec<CostBreakdownRow> = rows
        .into_iter()
        .map(|(label, cost_delta_usd)| CostBreakdownRow {
            label,
            cost_delta_usd,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.cost_delta_usd
            .partial_cmp(&a.cost_delta_usd)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.label.cmp(&b.label))
    });
    rows
}

#[derive(Debug, Clone)]
struct AnomalyCorrelationEvent {
    minute: i64,
    pane_id: String,
    kind: String,
    confidence: String,
}

fn collect_anomaly_correlations(
    conn: &Connection,
    window: InsightsWindow,
) -> Result<Vec<CrossPaneAnomalyCorrelation>, SqliteError> {
    if !table_exists(conn, "anomaly_events")? {
        return Ok(Vec::new());
    }
    let since_secs = window.since_ms.div_euclid(1000);
    let until_secs = window.until_ms.div_euclid(1000);
    let mut stmt = conn
        .prepare_cached(
            "SELECT ts_unix_secs, pane_id, kind, confidence
             FROM anomaly_events
             WHERE ts_unix_secs >= ?1 AND ts_unix_secs <= ?2
             ORDER BY ts_unix_secs ASC, id ASC",
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let rows = stmt
        .query_map(params![since_secs, until_secs], |row| {
            let ts: i64 = row.get(0)?;
            Ok(AnomalyCorrelationEvent {
                minute: ts.div_euclid(60),
                pane_id: row.get(1)?,
                kind: row.get(2)?,
                confidence: row.get(3)?,
            })
        })
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let mut buckets = BTreeMap::<(i64, String, String), Vec<AnomalyCorrelationEvent>>::new();
    for row in rows {
        let event = row.map_err(|e| SqliteError::Query(e.to_string()))?;
        buckets
            .entry((event.minute, event.kind.clone(), event.confidence.clone()))
            .or_default()
            .push(event);
    }
    let mut out = Vec::new();
    for ((minute, _kind, _confidence), events) in buckets {
        let pane_count = events
            .iter()
            .map(|event| event.pane_id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if pane_count < 2 {
            continue;
        }
        let summary = events
            .iter()
            .map(|event| format!("{}({}:{})", event.pane_id, event.kind, event.confidence))
            .collect::<Vec<_>>()
            .join(" + ");
        out.push(CrossPaneAnomalyCorrelation {
            window_label: rfc3339_from_ms(minute.saturating_mul(60_000)),
            pane_count,
            summary,
        });
    }
    out.reverse();
    out.truncate(10);
    Ok(out)
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
    // Keep the pre-v2.2 labels here for historical audit/lifecycle rows. They
    // are no longer current policy emissions, but old DBs still need grouping.
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
        "anomaly: cross-pane edit cluster detected" | "anomaly: identity churn detected"
    ) || action.starts_with("anomaly: subagent activity")
        || action.contains("code-exploration")
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

fn static_situation_label(situation: &str, action: &str) -> &'static str {
    match situation {
        "Log storm / repeated output" => "Log storm / repeated output",
        "Code exploration" => "Code exploration",
        "Context pressure" => "Context pressure",
        "Verbose review" => "Verbose review",
        "Input / permission wait" => "Input / permission wait",
        "Quota-tight / cost" => "Quota-tight / cost",
        "Other" => "Other",
        _ => situation_for_action(action),
    }
}

#[derive(Debug, Clone)]
struct LifecycleEventRow {
    id: i64,
    ts_unix_ms: i64,
    last_seen_unix_ms: i64,
    pane_id: String,
    provider: Option<String>,
    situation: &'static str,
    action: String,
    severity: String,
    reason_summary: String,
    suggested_command: Option<String>,
    is_strong: bool,
}

#[derive(Debug, Clone)]
struct LifecycleOutcomeRow {
    ts_unix_ms: i64,
    recommendation_event_id: Option<i64>,
    pane_id: String,
    action: String,
    outcome: String,
}

#[derive(Debug, Default)]
struct MetricWindowSummary {
    latest_cache_ratio: Option<f64>,
    token_growth: Option<i64>,
    cost_delta_usd: Option<f64>,
    sample_count: u64,
}

const ACTION_IMPACT_WINDOW_MS: i64 = 10 * 60 * 1000;
const NEXT_BEST_ACTION_LOOKBACK_MS: i64 = 24 * 60 * 60 * 1000;

impl SqliteInsightsStore {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
        })
    }

    pub fn open_read_only(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open_read_only(path)?,
        })
    }

    pub fn snapshot(&self, window: InsightsWindow) -> Result<InsightsSnapshot, SqliteError> {
        self.snapshot_with_ignored_ttl(window, 30 * 60)
    }

    pub fn snapshot_with_ignored_ttl(
        &self,
        window: InsightsWindow,
        ignored_ttl_secs: u64,
    ) -> Result<InsightsSnapshot, SqliteError> {
        let mut snapshot = self.snapshot_audit_only(window)?;
        let _conn = self
            .db
            .connection()
            .lock()
            .expect("insights db lock poisoned");
        if !table_exists(&_conn, "recommendation_events")?
            || !table_exists(&_conn, "recommendation_outcomes")?
        {
            return Ok(snapshot);
        }

        let lifecycle = lifecycle_snapshot(&_conn, window, ignored_ttl_secs)?;
        snapshot.next_best_action = lifecycle.next_best_action;
        snapshot.last_compact_payoff = lifecycle.last_compact_payoff;
        if lifecycle.events_seen {
            snapshot.situations = lifecycle.situations;
            snapshot.actions = lifecycle.actions;
            snapshot.timeline = lifecycle.timeline;
            snapshot.action_impacts = lifecycle.action_impacts;
            snapshot.data_completeness = data_completeness_for(
                &_conn,
                window,
                &snapshot.pane_buckets,
                &snapshot.action_impacts,
            )?;
            snapshot.ignored_available = true;
        }
        Ok(snapshot)
    }

    fn snapshot_audit_only(&self, window: InsightsWindow) -> Result<InsightsSnapshot, SqliteError> {
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

        let pane_rollup = collect_pane_insight_rollup(&_conn, window)?;
        cache.latest_cache_ratio = pane_rollup.latest_cache_ratio;
        cache.token_growth = pane_rollup.token_growth;
        cache.cost_delta_usd = pane_rollup.cost_delta_usd;
        let cost_breakdown = collect_cost_breakdown(&_conn, window)?;
        let anomaly_correlations = collect_anomaly_correlations(&_conn, window)?;

        let mut ledger_stmt = _conn
            .prepare_cached(
                "SELECT ts_utc, kind, pane_id, summary FROM audit_events \
                 WHERE kind IN (\
                   'RecommendationEmitted', 'AlertFired',\
                   'PromptSendAccepted', 'PromptSendRejected', 'PromptSendCompleted',\
                   'PromptSendFailed', 'PromptSendBlocked',\
                   'ArchiveWritten', 'SnapshotWritten'\
                 ) \
                   AND ts_utc >= ?1 \
                   AND ts_utc <= ?2 \
                 ORDER BY id ASC",
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let ledger_rows = ledger_stmt
            .query_map(params![&since_ts, &until_ts], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let mut actions_by_name = std::collections::BTreeMap::<String, ActionLedgerRow>::new();
        let mut timeline = Vec::new();
        for row in ledger_rows {
            let (ts_label, kind, pane_id, summary) =
                row.map_err(|e| SqliteError::Query(e.to_string()))?;
            let action = action_for_audit(&kind, &summary);
            let entry = actions_by_name
                .entry(action.clone())
                .or_insert_with(|| ActionLedgerRow {
                    action: action.clone(),
                    ..ActionLedgerRow::default()
                });
            apply_outcome(entry, &kind);
            timeline.push(RecommendationTimelineItem {
                ts_label,
                pane_id,
                situation: situation_for_action(action_from_summary(&summary)),
                action,
                outcome: outcome_for_kind(&kind).to_string(),
            });
        }
        let actions = actions_by_name.into_values().collect();
        timeline.reverse();
        timeline.truncate(10);
        let data_completeness =
            data_completeness_for(&_conn, window, &pane_rollup.pane_buckets, &[])?;

        Ok(InsightsSnapshot {
            window,
            situations,
            cache,
            pane_buckets: pane_rollup.pane_buckets,
            next_best_action: NextBestActionSummary::unavailable(),
            last_compact_payoff: LastCompactPayoffSummary::default(),
            cost_breakdown,
            anomaly_correlations,
            action_impacts: Vec::new(),
            data_completeness,
            timeline,
            actions,
            ignored_available: false,
        })
    }
}

struct LifecycleSnapshotParts {
    events_seen: bool,
    next_best_action: NextBestActionSummary,
    last_compact_payoff: LastCompactPayoffSummary,
    situations: Vec<SituationSummary>,
    timeline: Vec<RecommendationTimelineItem>,
    actions: Vec<ActionLedgerRow>,
    action_impacts: Vec<ActionImpactSummary>,
}

fn lifecycle_snapshot(
    conn: &Connection,
    window: InsightsWindow,
    ignored_ttl_secs: u64,
) -> Result<LifecycleSnapshotParts, SqliteError> {
    let last_seen_expr = if table_has_column(conn, "recommendation_events", "last_seen_unix_ms")? {
        "COALESCE(last_seen_unix_ms, ts_unix_ms)"
    } else {
        "ts_unix_ms"
    };
    let event_sql = format!(
        "SELECT id, ts_unix_ms, {last_seen_expr}, pane_id, provider, situation, action,
                severity, reason_summary, suggested_command, is_strong
         FROM recommendation_events
         WHERE {last_seen_expr} >= ?1 AND ts_unix_ms <= ?2
         ORDER BY id ASC"
    );
    let mut event_stmt = conn
        .prepare_cached(&event_sql)
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let event_rows = event_stmt
        .query_map(params![window.since_ms, window.until_ms], |row| {
            let action: String = row.get(6)?;
            let situation: String = row.get(5)?;
            Ok(LifecycleEventRow {
                id: row.get(0)?,
                ts_unix_ms: row.get(1)?,
                last_seen_unix_ms: row.get(2)?,
                pane_id: row.get(3)?,
                provider: row.get(4)?,
                situation: static_situation_label(&situation, &action),
                action,
                severity: row.get(7)?,
                reason_summary: row.get(8)?,
                suggested_command: row.get(9)?,
                is_strong: row.get::<_, i64>(10)? != 0,
            })
        })
        .map_err(|e| SqliteError::Query(e.to_string()))?;

    let mut events = Vec::new();
    for row in event_rows {
        events.push(row.map_err(|e| SqliteError::Query(e.to_string()))?);
    }
    if events.is_empty() {
        return Ok(LifecycleSnapshotParts {
            events_seen: false,
            next_best_action: NextBestActionSummary::no_pending(),
            last_compact_payoff: LastCompactPayoffSummary::default(),
            situations: Vec::new(),
            timeline: Vec::new(),
            actions: Vec::new(),
            action_impacts: Vec::new(),
        });
    }

    let mut outcome_stmt = conn
        .prepare_cached(
            "SELECT ts_unix_ms, recommendation_event_id, pane_id, action, outcome
             FROM recommendation_outcomes
             WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2
             ORDER BY id ASC",
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
    let outcome_rows = outcome_stmt
        .query_map(params![window.since_ms, window.until_ms], |row| {
            Ok(LifecycleOutcomeRow {
                ts_unix_ms: row.get(0)?,
                recommendation_event_id: row.get(1)?,
                pane_id: row.get(2)?,
                action: row.get(3)?,
                outcome: row.get(4)?,
            })
        })
        .map_err(|e| SqliteError::Query(e.to_string()))?;

    let mut outcomes = Vec::new();
    for row in outcome_rows {
        outcomes.push(row.map_err(|e| SqliteError::Query(e.to_string()))?);
    }

    let mut situations_by_name = std::collections::BTreeMap::<&'static str, u64>::new();
    let mut actions_by_name = std::collections::BTreeMap::<String, ActionLedgerRow>::new();
    let mut event_situations = std::collections::HashMap::<i64, &'static str>::new();
    let mut events_by_id = std::collections::HashMap::<i64, LifecycleEventRow>::new();
    let mut terminal_event_ids = std::collections::BTreeSet::<i64>::new();
    let mut timeline = Vec::new();

    for event in &events {
        *situations_by_name.entry(event.situation).or_default() += 1;
        event_situations.insert(event.id, event.situation);
        events_by_id.insert(event.id, event.clone());
        let entry = actions_by_name
            .entry(event.action.clone())
            .or_insert_with(|| ActionLedgerRow {
                action: event.action.clone(),
                ..ActionLedgerRow::default()
            });
        entry.emitted += 1;
        timeline.push(RecommendationTimelineItem {
            ts_label: rfc3339_from_ms(event.ts_unix_ms),
            pane_id: event.pane_id.clone(),
            situation: event.situation,
            action: event.action.clone(),
            outcome: "emitted".into(),
        });
    }

    for outcome in &outcomes {
        if let Some(event_id) = outcome.recommendation_event_id
            && is_terminal_lifecycle_outcome(&outcome.outcome)
        {
            terminal_event_ids.insert(event_id);
        }
    }

    let mut matched_unlinked_outcomes = std::collections::HashMap::<usize, i64>::new();
    let mut consumed_event_ids = terminal_event_ids.clone();
    for (idx, outcome) in outcomes.iter().enumerate() {
        if outcome.recommendation_event_id.is_some() {
            continue;
        }
        if let Some(event) = events
            .iter()
            .filter(|event| {
                event.pane_id == outcome.pane_id
                    && event.action == outcome.action
                    && event.ts_unix_ms <= outcome.ts_unix_ms
                    && !consumed_event_ids.contains(&event.id)
            })
            .max_by_key(|event| (event.ts_unix_ms, event.id))
        {
            if is_terminal_lifecycle_outcome(&outcome.outcome) {
                consumed_event_ids.insert(event.id);
            }
            matched_unlinked_outcomes.insert(idx, event.id);
        }
    }
    terminal_event_ids = consumed_event_ids;

    let accepted_event_ids: std::collections::BTreeSet<i64> = outcomes
        .iter()
        .enumerate()
        .filter(|(_, outcome)| outcome.outcome == "accepted")
        .filter_map(|(idx, outcome)| {
            outcome
                .recommendation_event_id
                .or_else(|| matched_unlinked_outcomes.get(&idx).copied())
        })
        .collect();
    let next_best_action = next_best_action_for(&events, &accepted_event_ids, window);

    for (idx, outcome) in outcomes.iter().enumerate() {
        let matched_event_id = outcome
            .recommendation_event_id
            .or_else(|| matched_unlinked_outcomes.get(&idx).copied());
        let situation = matched_event_id
            .and_then(|id| event_situations.get(&id).copied())
            .unwrap_or_else(|| situation_for_action(&outcome.action));
        let entry = actions_by_name
            .entry(outcome.action.clone())
            .or_insert_with(|| ActionLedgerRow {
                action: outcome.action.clone(),
                ..ActionLedgerRow::default()
            });
        apply_lifecycle_outcome(entry, &outcome.outcome);
        timeline.push(RecommendationTimelineItem {
            ts_label: rfc3339_from_ms(outcome.ts_unix_ms),
            pane_id: outcome.pane_id.clone(),
            situation,
            action: outcome.action.clone(),
            outcome: outcome.outcome.clone(),
        });
    }

    let mut action_impacts = Vec::new();
    let mut last_compact_payoff = LastCompactPayoffSummary::default();
    let mut last_compact_outcome_ts = None;
    for (idx, outcome) in outcomes.iter().enumerate() {
        if matches!(outcome.outcome.as_str(), "accepted" | "copied" | "hidden") {
            continue;
        }
        let Some(event_id) = outcome
            .recommendation_event_id
            .or_else(|| matched_unlinked_outcomes.get(&idx).copied())
        else {
            continue;
        };
        let Some(event) = events_by_id.get(&event_id) else {
            continue;
        };
        let provider = event.provider.as_deref();
        let before = collect_metric_window_for_pane(
            conn,
            InsightsWindow {
                since_ms: window
                    .since_ms
                    .max(event.ts_unix_ms.saturating_sub(ACTION_IMPACT_WINDOW_MS)),
                until_ms: event.ts_unix_ms,
            },
            &event.pane_id,
            provider,
        )?;
        let after = collect_metric_window_for_pane(
            conn,
            InsightsWindow {
                since_ms: outcome.ts_unix_ms,
                until_ms: window
                    .until_ms
                    .min(outcome.ts_unix_ms.saturating_add(ACTION_IMPACT_WINDOW_MS)),
            },
            &event.pane_id,
            provider,
        )?;
        action_impacts.push(ActionImpactSummary {
            event_id,
            outcome_ts_label: rfc3339_from_ms(outcome.ts_unix_ms),
            pane_id: event.pane_id.clone(),
            provider: event.provider.clone(),
            action: event.action.clone(),
            outcome: outcome.outcome.clone(),
            before_token_growth: before.token_growth,
            after_token_growth: after.token_growth,
            token_growth_delta: metric_delta_i64(before.token_growth, after.token_growth),
            before_cost_delta_usd: before.cost_delta_usd,
            after_cost_delta_usd: after.cost_delta_usd,
            cost_delta_usd: metric_delta_f64(before.cost_delta_usd, after.cost_delta_usd),
            before_cache_ratio: before.latest_cache_ratio,
            after_cache_ratio: after.latest_cache_ratio,
        });
        if event.action == "/compact"
            && matches!(outcome.outcome.as_str(), "completed" | "improved")
            && last_compact_outcome_ts.is_none_or(|ts| outcome.ts_unix_ms >= ts)
        {
            last_compact_payoff = compact_payoff_from(event, outcome, &before, &after, window);
            last_compact_outcome_ts = Some(outcome.ts_unix_ms);
        }
    }
    action_impacts.reverse();
    action_impacts.truncate(10);

    let ttl_ms =
        i64::try_from(u128::from(ignored_ttl_secs).saturating_mul(1000)).unwrap_or(i64::MAX);
    for event in &events {
        if terminal_event_ids.contains(&event.id) {
            continue;
        }
        if window.until_ms.saturating_sub(event.ts_unix_ms) < ttl_ms {
            continue;
        }
        if window.until_ms.saturating_sub(event.last_seen_unix_ms) < ttl_ms {
            continue;
        }
        let entry = actions_by_name
            .entry(event.action.clone())
            .or_insert_with(|| ActionLedgerRow {
                action: event.action.clone(),
                ..ActionLedgerRow::default()
            });
        entry.ignored += 1;
        timeline.push(RecommendationTimelineItem {
            ts_label: rfc3339_from_ms(window.until_ms),
            pane_id: event.pane_id.clone(),
            situation: event.situation,
            action: event.action.clone(),
            outcome: "ignored".into(),
        });
    }

    let situations = situations_by_name
        .into_iter()
        .map(|(situation, emitted)| SituationSummary { situation, emitted })
        .collect();
    let actions = actions_by_name.into_values().collect();
    timeline.reverse();
    timeline.truncate(10);

    Ok(LifecycleSnapshotParts {
        events_seen: true,
        next_best_action,
        last_compact_payoff,
        situations,
        timeline,
        actions,
        action_impacts,
    })
}

fn next_best_action_for(
    events: &[LifecycleEventRow],
    accepted_event_ids: &std::collections::BTreeSet<i64>,
    window: InsightsWindow,
) -> NextBestActionSummary {
    let since_ms = window.until_ms.saturating_sub(NEXT_BEST_ACTION_LOOKBACK_MS);
    let pending: Vec<&LifecycleEventRow> = events
        .iter()
        .filter(|event| {
            event.is_strong
                && !accepted_event_ids.contains(&event.id)
                && event.last_seen_unix_ms >= since_ms
                && event.ts_unix_ms <= window.until_ms
        })
        .collect();
    let action = pending
        .iter()
        .copied()
        .max_by_key(|event| (event.last_seen_unix_ms, event.id))
        .map(|event| NextBestActionItem {
            ts_label: rfc3339_from_ms(event.last_seen_unix_ms),
            pane_id: event.pane_id.clone(),
            provider: event.provider.clone(),
            situation: event.situation,
            action: event.action.clone(),
            severity: event.severity.clone(),
            reason_summary: event.reason_summary.clone(),
            suggested_command: event.suggested_command.clone(),
        });

    NextBestActionSummary {
        data_available: true,
        action,
        open_proposal_count: pending.len() as u64,
    }
}

fn compact_payoff_from(
    event: &LifecycleEventRow,
    outcome: &LifecycleOutcomeRow,
    before: &MetricWindowSummary,
    after: &MetricWindowSummary,
    window: InsightsWindow,
) -> LastCompactPayoffSummary {
    let sample_count = before.sample_count.saturating_add(after.sample_count);
    let mut summary = LastCompactPayoffSummary {
        status: "n/a".into(),
        outcome_ts_label: Some(rfc3339_from_ms(outcome.ts_unix_ms)),
        age_label: Some(payoff_age_label(
            window.until_ms.saturating_sub(outcome.ts_unix_ms),
        )),
        pane_id: Some(event.pane_id.clone()),
        input_tokens_saved: None,
        cost_usd_saved: metric_delta_f64(after.cost_delta_usd, before.cost_delta_usd),
        before_cache_state: before.latest_cache_ratio.map(cache_state_label),
        after_cache_state: after.latest_cache_ratio.map(cache_state_label),
        sample_count,
    };
    if sample_count < 6 {
        return summary;
    }
    let Some(before_growth) = before.token_growth else {
        return summary;
    };
    let Some(after_growth) = after.token_growth else {
        return summary;
    };
    let saved = before_growth.saturating_sub(after_growth);
    summary.input_tokens_saved = Some(saved);
    if !payoff_delta_is_significant(saved, before_growth) {
        summary.status = "neutral".into();
    } else if saved > 0 {
        summary.status = "saved".into();
    } else {
        summary.status = "regressed".into();
    }
    summary
}

fn payoff_delta_is_significant(saved: i64, before_growth: i64) -> bool {
    let abs = saved.unsigned_abs();
    if abs >= 1_000 {
        return true;
    }
    let baseline = before_growth.unsigned_abs();
    baseline > 0 && (abs as f64 / baseline as f64) >= 0.05
}

fn cache_state_label(ratio: f64) -> String {
    if ratio < 0.25 {
        "cold".into()
    } else if ratio < 0.70 {
        "warm".into()
    } else {
        "hot".into()
    }
}

fn payoff_age_label(age_ms: i64) -> String {
    let secs = (age_ms.max(0) / 1000) as u64;
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h ago", secs / 3_600)
    }
}
