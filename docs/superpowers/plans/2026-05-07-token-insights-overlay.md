# Token Insights Phase 8 v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Phase 8 v1 proof for Token Insights: a read-only SQLite query layer plus `qmonster insights --since 24h` text report.

**Architecture:** This plan implements the first rollout slice from `docs/superpowers/specs/2026-05-07-token-insights-overlay-design.md`: no lifecycle ledger, no ignored classifier, and no TUI `i` overlay yet. `src/store/insights.rs` owns SQLite reads and aggregation into an `InsightsSnapshot`; `src/insights_report.rs` owns window parsing, storage-path resolution, and report formatting; `src/main.rs` adds the `insights` subcommand without starting tmux.

**Tech Stack:** Rust 1.88, clap derive, rusqlite, tempfile integration tests, existing Qmonster storage path/config helpers.

---

## Scope Boundary

This plan intentionally implements **Phase 8 v1 only**:

- Included: read-only aggregates from `audit_events`, `token_usage_samples`, and `cost_usage_events`.
- Included: text report command `qmonster insights --since 24h`.
- Included: conservative situation/action mapping and cache trend reporting.
- Deferred to a later plan: `recommendation_events`, `recommendation_outcomes`, TTL ignored classification, UI-only hide outcome capture, `InsightsRuntimeState`, and the TUI `i` overlay.

Phase 8 v1 does not estimate ignored recommendations. The action ledger keeps `ignored = 0` and the report labels ignored classification as unavailable because the lifecycle ledger does not exist yet.
Prompt-send outcomes are shown as observed action rows in this phase; exact recommendation-to-outcome correlation starts with the Phase 8 v2 lifecycle ledger.
Phase 7 v3 now owns anomaly runtime state, promotion gates, and the `n` overlay. Phase 8 v1 must not touch `src/domain/anomaly.rs`, `src/policy/rules/anomaly.rs`, anomaly overlay modules, `src/app/mod.rs`, shared UI overlay routing, or shared release ledger/docs while that workstream is active.

## File Structure

- Create `src/store/insights.rs`
  - Owns `SqliteInsightsStore`, `InsightsWindow`, `InsightsSnapshot`, `SituationSummary`, `CacheInsightSummary`, `ActionLedgerRow`, `RecommendationTimelineItem`.
  - Opens the shared `qmonster.db` through internal `store::sqlite::AuditDb`.
  - Queries only existing tables.
- Modify `src/store/mod.rs`
  - Exposes `pub mod insights` and re-exports public insights types.
- Create `src/insights_report.rs`
  - Parses `--since` values.
  - Resolves storage paths without starting tmux.
  - Formats `InsightsSnapshot` into stable report lines.
- Modify `src/lib.rs`
  - Exposes `pub mod insights_report`.
- Modify `src/main.rs`
  - Adds `CliCommand::Insights`.
  - Runs the report path before `build_startup_runtime`, so report generation does not require tmux.
- Add `tests/store_insights_integration.rs`
  - Exercises SQLite-backed aggregation with fixture rows.
- Add `tests/insights_report_integration.rs`
  - Exercises report formatting and the `--since` parser helper.

## Execution Cutline

- Active in this subagent-driven run: Tasks 1 through 6.
- Deferred until Phase 7 v3 docs/ledger work lands: user-facing docs and mission ledger synchronization.
- Avoid in this run: `src/app/mod.rs`, `src/app/tui_loop.rs`, `src/app/dashboard_render.rs`, `src/ui/*overlay*`, `docs/ai/*`, `README.md`, `mission.yaml`, `mission-history.yaml`, and `.mission/CURRENT_STATE.md`.

---

### Task 1: Add Empty Insights Query Layer

**Files:**
- Create: `src/store/insights.rs`
- Modify: `src/store/mod.rs`
- Test: `tests/store_insights_integration.rs`

- [ ] **Step 1: Write the failing empty-state integration test**

Create `tests/store_insights_integration.rs`:

```rust
use qmonster::store::{InsightsWindow, SqliteInsightsStore};
use tempfile::tempdir;

#[test]
fn insights_query_empty_db_returns_zero_state() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let store = SqliteInsightsStore::open(&db_path).unwrap();

    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: 1_000,
            until_ms: 2_000,
        })
        .unwrap();

    assert_eq!(snapshot.window.since_ms, 1_000);
    assert_eq!(snapshot.window.until_ms, 2_000);
    assert!(snapshot.situations.is_empty());
    assert_eq!(snapshot.cache.hot_count, 0);
    assert_eq!(snapshot.cache.cold_count, 0);
    assert_eq!(snapshot.cache.drift_count, 0);
    assert!(snapshot.timeline.is_empty());
    assert!(snapshot.actions.is_empty());
    assert_eq!(snapshot.ignored_available, false);
}
```

- [ ] **Step 2: Run the test and confirm it fails**

Run:

```bash
cargo test --test store_insights_integration insights_query_empty_db_returns_zero_state
```

Expected: compile failure naming unresolved `InsightsWindow` / `SqliteInsightsStore`.

- [ ] **Step 3: Implement the minimal store module**

Create `src/store/insights.rs`:

```rust
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
```

Modify `src/store/mod.rs`:

```rust
pub mod insights;

pub use insights::{
    ActionLedgerRow, CacheInsightSummary, InsightsSnapshot, InsightsWindow,
    RecommendationTimelineItem, SituationSummary, SqliteInsightsStore,
};
```

Insert the `pub mod insights;` line with the other public store modules, and insert the exact `pub use insights::{ ActionLedgerRow, CacheInsightSummary, InsightsSnapshot, InsightsWindow, RecommendationTimelineItem, SituationSummary, SqliteInsightsStore };` block after the existing cost-usage re-export.

- [ ] **Step 4: Run the focused test and confirm it passes**

Run:

```bash
cargo test --test store_insights_integration insights_query_empty_db_returns_zero_state
```

Expected: test passes.

- [ ] **Step 5: Commit**

```bash
git add src/store/insights.rs src/store/mod.rs tests/store_insights_integration.rs
git commit -m "feat(store): add empty token insights snapshot"
```

---

### Task 2: Aggregate Recommendations Into Six Situations

**Files:**
- Modify: `src/store/insights.rs`
- Test: `tests/store_insights_integration.rs`

- [ ] **Step 1: Add a failing test for situation counts**

Append to `tests/store_insights_integration.rs`:

```rust
use qmonster::domain::audit::{AuditEvent, AuditEventKind};
use qmonster::domain::identity::{Provider, Role};
use qmonster::domain::recommendation::Severity;
use qmonster::store::{EventSink, SqliteAuditSink};

fn audit_event(kind: AuditEventKind, pane_id: &str, summary: &str) -> AuditEvent {
    AuditEvent {
        kind,
        pane_id: pane_id.to_string(),
        severity: Severity::Concern,
        summary: summary.to_string(),
        provider: Some(Provider::Codex),
        role: Some(Role::Review),
    }
}

#[test]
fn insights_counts_recommendations_by_situation() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let audit = SqliteAuditSink::open(&db_path).unwrap();
    audit.record(audit_event(
        AuditEventKind::RecommendationEmitted,
        "%1",
        "cache: avoid /compact while cache is hot: cache hit ratio 88%",
    ));
    audit.record(audit_event(
        AuditEventKind::RecommendationEmitted,
        "%2",
        "verbose-review: terse profile: review pane is verbose",
    ));
    audit.record(audit_event(
        AuditEventKind::AlertFired,
        "%3",
        "pane-state: Codex pane state: WAIT (input)",
    ));

    let now_ms = chrono::Utc::now().timestamp_millis();
    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: now_ms - 60_000,
            until_ms: now_ms + 60_000,
        })
        .unwrap();

    let context = snapshot
        .situations
        .iter()
        .find(|s| s.situation == "Context pressure")
        .unwrap();
    let verbose = snapshot
        .situations
        .iter()
        .find(|s| s.situation == "Verbose review")
        .unwrap();
    let wait = snapshot
        .situations
        .iter()
        .find(|s| s.situation == "Input / permission wait")
        .unwrap();

    assert_eq!(context.emitted, 1);
    assert_eq!(verbose.emitted, 1);
    assert_eq!(wait.emitted, 1);
}
```

- [ ] **Step 2: Run the test and confirm it fails**

Run:

```bash
cargo test --test store_insights_integration insights_counts_recommendations_by_situation
```

Expected: test fails because `snapshot.situations` is empty.

- [ ] **Step 3: Add action parsing and situation aggregation**

In `src/store/insights.rs`, extend the imports:

```rust
use chrono::TimeZone as _;
use rusqlite::params;
```

Then add these helpers above `impl SqliteInsightsStore`:

```rust
const KNOWN_ACTION_PREFIXES: &[&str] = &[
    "archive-preview-suggested",
    "repeated-output-cache",
    "log-storm: ingress filter + summary",
    "repeated-output: result-hash cache",
    "code-exploration: graph/symbol",
    "context-pressure: checkpoint",
    "context-pressure: act now",
    "cache: avoid /compact while cache is hot",
    "cache: /compact is safe - cache is cold",
    "cache: drift detected - /compact will let cache rebuild",
    "quota: pause until 5h window resets",
    "quota: pause until weekly window resets",
    "snapshot before 5h window resets",
    "snapshot before weekly window resets",
    "verbose-output",
    "verbose-review: terse profile",
    "pane-state",
    "quota-tight: consider enabling",
    "quota-pressure: pace requests",
    "cost-budget: 80% reached",
    "cost-budget: exhausted",
    "profile-switch: elevated error rate",
    "provider-profile: switch suggested",
];

fn summary_has_action_prefix(summary: &str, prefix: &str) -> bool {
    summary == prefix || summary
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with(": "))
}

fn action_from_summary(summary: &str) -> &str {
    for prefix in KNOWN_ACTION_PREFIXES {
        if summary_has_action_prefix(summary, prefix) {
            return *prefix;
        }
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
    (rfc3339_from_ms(window.since_ms), rfc3339_from_ms(window.until_ms))
}

fn situation_for_action(action: &str) -> &'static str {
    if action.contains("log-storm")
        || action == "archive-preview-suggested"
        || action.contains("repeated-output")
    {
        "Log storm / repeated output"
    } else if action.contains("code-exploration")
        || action.contains("cross-pane")
        || action.contains("ConcurrentFileEdit")
    {
        "Code exploration"
    } else if action.contains("context-pressure")
        || action.starts_with("cache")
        || action.starts_with("quota: pause")
        || action.starts_with("snapshot before")
    {
        "Context pressure"
    } else if action.contains("verbose-review") || action == "verbose-output" {
        "Verbose review"
    } else if action == "pane-state" || action.contains("permission") || action.contains("input")
    {
        "Input / permission wait"
    } else if action.contains("quota-pressure")
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
```

Replace the empty `situations: Vec::new(),` assignment in `snapshot()` with:

```rust
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
    if situation != "Other" {
        *counts.entry(situation).or_default() += 1;
    }
}
let situations = counts
    .into_iter()
    .map(|(situation, emitted)| SituationSummary { situation, emitted })
    .collect();
```

Then return `situations,` in `InsightsSnapshot`.

- [ ] **Step 4: Run the focused tests and confirm they pass**

Run:

```bash
cargo test --test store_insights_integration
```

Expected: all tests in `store_insights_integration` pass.

- [ ] **Step 5: Commit**

```bash
git add src/store/insights.rs tests/store_insights_integration.rs
git commit -m "feat(store): aggregate insights by situation"
```

---

### Task 3: Aggregate Cache Trend, Token Growth, and Cost Delta

**Files:**
- Modify: `src/store/insights.rs`
- Test: `tests/store_insights_integration.rs`

- [ ] **Step 1: Add a failing cache summary test**

In `tests/store_insights_integration.rs`, replace the existing store import:

```rust
use qmonster::store::{EventSink, SqliteAuditSink};
```

with:

```rust
use qmonster::store::{
    CostObservation, EventSink, SqliteAuditSink, SqliteCostUsageSink, SqliteTokenUsageSink,
    TokenSample,
};
```

Then append:

```rust
#[test]
fn cache_summary_reports_cache_states_latest_ratio_token_growth_and_cost_delta() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let audit = SqliteAuditSink::open(&db_path).unwrap();
    audit.record(audit_event(
        AuditEventKind::RecommendationEmitted,
        "%1",
        "cache: avoid /compact while cache is hot: cache hit ratio 88%",
    ));
    audit.record(audit_event(
        AuditEventKind::RecommendationEmitted,
        "%1",
        "cache: /compact is safe - cache is cold: cache hit ratio 12%",
    ));
    audit.record(audit_event(
        AuditEventKind::RecommendationEmitted,
        "%1",
        "cache: drift detected - /compact will let cache rebuild: cache hit ratio dropped",
    ));

    let tokens = SqliteTokenUsageSink::open(&db_path).unwrap();
    tokens
        .record_sample(&TokenSample {
            ts_unix_ms: 1_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(100),
            output_tokens: Some(10),
            cost_usd: None,
            cached_input_tokens: Some(100),
        })
        .unwrap();
    tokens
        .record_sample(&TokenSample {
            ts_unix_ms: 2_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(300),
            output_tokens: Some(20),
            cost_usd: None,
            cached_input_tokens: Some(900),
        })
        .unwrap();

    let costs = SqliteCostUsageSink::open(&db_path).unwrap();
    costs
        .record_observation(&CostObservation {
            ts_unix_ms: 1_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            cumulative_cost_usd: 0.05,
        })
        .unwrap();
    costs
        .record_observation(&CostObservation {
            ts_unix_ms: 2_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            cumulative_cost_usd: 0.08,
        })
        .unwrap();

    let now_ms = chrono::Utc::now().timestamp_millis();
    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: now_ms + 60_000,
        })
        .unwrap();

    assert_eq!(snapshot.cache.hot_count, 1);
    assert_eq!(snapshot.cache.cold_count, 1);
    assert_eq!(snapshot.cache.drift_count, 1);
    assert_eq!(snapshot.cache.latest_cache_ratio, Some(0.75));
    assert_eq!(snapshot.cache.token_growth, Some(200));
    assert!((snapshot.cache.cost_delta_usd.unwrap() - 0.08).abs() < 0.000_001);
}
```

- [ ] **Step 2: Run the test and confirm it fails**

Run:

```bash
cargo test --test store_insights_integration cache_summary_reports_cache_states_latest_ratio_token_growth_and_cost_delta
```

Expected: test fails because cache summary returns defaults.

- [ ] **Step 3: Implement cache summary aggregation**

Add this helper in `src/store/insights.rs`:

```rust
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
    } else if action == "cache: /compact is safe - cache is cold" {
        Some("cold")
    } else if action == "cache: drift detected - /compact will let cache rebuild" {
        Some("drift")
    } else {
        None
    }
}
```

In `snapshot()`, after building `situations`, add:

```rust
let mut cache = CacheInsightSummary::default();
let mut cache_state_stmt = _conn
    .prepare_cached(
        "SELECT summary FROM audit_events \
         WHERE kind IN ('RecommendationEmitted', 'AlertFired') \
           AND ts_utc >= ?1 \
           AND ts_utc <= ?2 \
         ORDER BY id ASC",
    )
    .map_err(|e| SqliteError::Query(e.to_string()))?;
let cache_state_rows = cache_state_stmt
    .query_map(params![&since_ts, &until_ts], |row| row.get::<_, String>(0))
    .map_err(|e| SqliteError::Query(e.to_string()))?;
for row in cache_state_rows {
    let summary = row.map_err(|e| SqliteError::Query(e.to_string()))?;
    let action = action_from_summary(&summary);
    match cache_state_from_action(action) {
        Some("hot") => cache.hot_count += 1,
        Some("cold") => cache.cold_count += 1,
        Some("drift") => cache.drift_count += 1,
        _ => {}
    }
}

let mut token_stmt = _conn
    .prepare_cached(
        "SELECT ts_unix_ms, input_tokens, cached_input_tokens \
         FROM token_usage_samples \
         WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2 \
         ORDER BY ts_unix_ms ASC, id ASC",
    )
    .map_err(|e| SqliteError::Query(e.to_string()))?;
let token_rows = token_stmt
    .query_map([window.since_ms, window.until_ms], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<i64>>(2)?,
        ))
    })
    .map_err(|e| SqliteError::Query(e.to_string()))?;
let mut first_input: Option<i64> = None;
let mut latest_input: Option<i64> = None;
let mut latest_ratio: Option<f64> = None;
for row in token_rows {
    let (_ts, input, cached) = row.map_err(|e| SqliteError::Query(e.to_string()))?;
    if first_input.is_none() {
        first_input = input;
    }
    latest_input = input;
    latest_ratio = cache_ratio(input, cached);
}
let token_growth = match (first_input, latest_input) {
    (Some(first), Some(latest)) => Some(latest.saturating_sub(first)),
    _ => None,
};
let mut cost_stmt = _conn
    .prepare_cached(
        "SELECT SUM(cost_usd_delta) \
         FROM cost_usage_events \
         WHERE ts_unix_ms >= ?1 AND ts_unix_ms <= ?2",
    )
    .map_err(|e| SqliteError::Query(e.to_string()))?;
let cost_delta_usd = cost_stmt
    .query_row([window.since_ms, window.until_ms], |row| row.get::<_, Option<f64>>(0))
    .map_err(|e| SqliteError::Query(e.to_string()))?;
cache.latest_cache_ratio = latest_ratio;
cache.token_growth = token_growth;
cache.cost_delta_usd = cost_delta_usd;
```

Return `cache,` in `InsightsSnapshot`.

- [ ] **Step 4: Run the focused store tests**

Run:

```bash
cargo test --test store_insights_integration
```

Expected: all tests in `store_insights_integration` pass.

- [ ] **Step 5: Commit**

```bash
git add src/store/insights.rs tests/store_insights_integration.rs
git commit -m "feat(store): summarize cache reuse token growth and cost"
```

---

### Task 4: Aggregate Action Ledger and Timeline

**Files:**
- Modify: `src/store/insights.rs`
- Test: `tests/store_insights_integration.rs`

- [ ] **Step 1: Add a failing action ledger test**

Append to `tests/store_insights_integration.rs`:

```rust
#[test]
fn action_ledger_counts_prompt_send_terminal_outcomes() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let audit = SqliteAuditSink::open(&db_path).unwrap();
    audit.record(audit_event(
        AuditEventKind::RecommendationEmitted,
        "%1",
        "context-pressure: act now: context near critical",
    ));
    audit.record(audit_event(
        AuditEventKind::PromptSendAccepted,
        "%1",
        "%1 -> `/compact` (proposal_id `%1:/compact`)",
    ));
    audit.record(audit_event(
        AuditEventKind::PromptSendCompleted,
        "%1",
        "%1 -> `/compact` completed (proposal_id `%1:/compact`)",
    ));
    audit.record(audit_event(
        AuditEventKind::SnapshotWritten,
        "%1",
        "snapshot written to /tmp/demo.json",
    ));

    let now_ms = chrono::Utc::now().timestamp_millis();
    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: now_ms - 60_000,
            until_ms: now_ms + 60_000,
        })
        .unwrap();

    let compact = snapshot
        .actions
        .iter()
        .find(|row| row.action == "/compact")
        .unwrap();
    assert_eq!(compact.accepted, 1);
    assert_eq!(compact.completed, 1);
    assert_eq!(compact.ignored, 0);

    let snapshot_row = snapshot
        .actions
        .iter()
        .find(|row| row.action == "snapshot")
        .unwrap();
    assert_eq!(snapshot_row.snapshot_written, 1);

    assert!(snapshot
        .timeline
        .iter()
        .any(|item| item.outcome == "completed" && item.action == "/compact"));
}
```

- [ ] **Step 2: Run the test and confirm it fails**

Run:

```bash
cargo test --test store_insights_integration action_ledger_counts_prompt_send_terminal_outcomes
```

Expected: test fails because `actions` and `timeline` are empty.

- [ ] **Step 3: Implement action extraction and ledger aggregation**

Add these helpers in `src/store/insights.rs`:

```rust
fn prompt_command_from_summary(summary: &str) -> Option<String> {
    let start = summary.find('`')?;
    let rest = &summary[start + 1..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

fn action_for_audit(kind: &str, summary: &str) -> String {
    match kind {
        "PromptSendAccepted" | "PromptSendCompleted" | "PromptSendFailed" | "PromptSendBlocked" => {
            prompt_command_from_summary(summary).unwrap_or_else(|| "prompt-send".into())
        }
        "PromptSendRejected" => prompt_command_from_summary(summary).unwrap_or_else(|| "prompt-send".into()),
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
```

In `snapshot()`, add a second audit query:

```rust
let mut ledger_stmt = _conn
    .prepare_cached(
        "SELECT ts_utc, kind, pane_id, summary FROM audit_events \
         WHERE kind IN (
           'RecommendationEmitted', 'AlertFired',
           'PromptSendAccepted', 'PromptSendRejected', 'PromptSendCompleted',
           'PromptSendFailed', 'PromptSendBlocked',
           'ArchiveWritten', 'SnapshotWritten'
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
    let entry = actions_by_name.entry(action.clone()).or_insert_with(|| ActionLedgerRow {
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
```

Return `timeline,` and `actions,` in `InsightsSnapshot`.

- [ ] **Step 4: Run all store insights tests**

Run:

```bash
cargo test --test store_insights_integration
```

Expected: all store insights integration tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/store/insights.rs tests/store_insights_integration.rs
git commit -m "feat(store): aggregate token insight action ledger"
```

---

### Task 5: Add Report Formatting and Since Parser

**Files:**
- Create: `src/insights_report.rs`
- Modify: `src/lib.rs`
- Test: `tests/insights_report_integration.rs`

- [ ] **Step 1: Write failing report tests**

Create `tests/insights_report_integration.rs`:

```rust
use qmonster::insights_report::{format_insights_report_lines, parse_since_arg};
use qmonster::store::{
    ActionLedgerRow, CacheInsightSummary, InsightsSnapshot, InsightsWindow,
    RecommendationTimelineItem, SituationSummary,
};

#[test]
fn parse_since_accepts_hours_and_days() {
    assert_eq!(parse_since_arg("24h").unwrap(), 86_400);
    assert_eq!(parse_since_arg("7d").unwrap(), 604_800);
    assert_eq!(parse_since_arg("900s").unwrap(), 900);
}

#[test]
fn parse_since_rejects_empty_or_unknown_units() {
    assert!(parse_since_arg("").is_err());
    assert!(parse_since_arg("24x").is_err());
}

#[test]
fn insights_report_renders_action_ledger() {
    let snapshot = InsightsSnapshot {
        window: InsightsWindow {
            since_ms: 0,
            until_ms: 86_400_000,
        },
        situations: vec![SituationSummary {
            situation: "Context pressure",
            emitted: 2,
        }],
        cache: CacheInsightSummary {
            hot_count: 1,
            cold_count: 1,
            drift_count: 0,
            latest_cache_ratio: Some(0.75),
            token_growth: Some(200),
            cost_delta_usd: Some(0.08),
        },
        timeline: vec![RecommendationTimelineItem {
            ts_label: "2026-05-07T12:00:00Z".into(),
            pane_id: "%1".into(),
            situation: "Context pressure",
            action: "/compact".into(),
            outcome: "completed".into(),
        }],
        actions: vec![ActionLedgerRow {
            action: "/compact".into(),
            emitted: 1,
            accepted: 1,
            completed: 1,
            ..ActionLedgerRow::default()
        }],
        ignored_available: false,
    };

    let lines = format_insights_report_lines(&snapshot);
    let joined = lines.join("\n");

    assert!(joined.contains("Token Insights"));
    assert!(joined.contains("Context pressure"));
    assert!(joined.contains("cache reuse: 75.0%"));
    assert!(joined.contains("token growth: +200"));
    assert!(joined.contains("cost delta: $0.0800"));
    assert!(joined.contains("/compact"));
    assert!(joined.contains("ignored classification: unavailable"));
    assert!(joined.contains("Evidence"));
    assert!(joined.contains("source tables: audit_events, token_usage_samples, cost_usage_events"));
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run:

```bash
cargo test --test insights_report_integration
```

Expected: compile failure because `insights_report` does not exist.

- [ ] **Step 3: Implement report helpers**

Create `src/insights_report.rs`:

```rust
use anyhow::{Context as _, Result};

use crate::store::InsightsSnapshot;

pub fn parse_since_arg(input: &str) -> Result<u64> {
    let trimmed = input.trim();
    if trimmed.len() < 2 {
        anyhow::bail!("--since expects a value like 24h, 7d, or 900s");
    }
    let (digits, unit) = trimmed.split_at(trimmed.len() - 1);
    let value: u64 = digits
        .parse()
        .with_context(|| format!("invalid --since value {input:?}"))?;
    let seconds = match unit {
        "s" => value,
        "m" => value * 60,
        "h" => value * 60 * 60,
        "d" => value * 24 * 60 * 60,
        _ => anyhow::bail!("--since unit must be one of s, m, h, d"),
    };
    if seconds == 0 {
        anyhow::bail!("--since must be greater than zero");
    }
    Ok(seconds)
}

pub fn format_insights_report_lines(snapshot: &InsightsSnapshot) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push("Token Insights".into());
    lines.push(format!(
        "window: {}..{}",
        snapshot.window.since_ms, snapshot.window.until_ms
    ));
    lines.push(format!(
        "ignored classification: {}",
        if snapshot.ignored_available {
            "available"
        } else {
            "unavailable"
        }
    ));
    lines.push(String::new());
    lines.push("Situations".into());
    if snapshot.situations.is_empty() {
        lines.push("  none".into());
    } else {
        for s in &snapshot.situations {
            lines.push(format!("  {}: {}", s.situation, s.emitted));
        }
    }
    lines.push(String::new());
    lines.push("Cache".into());
    match snapshot.cache.latest_cache_ratio {
        Some(ratio) => lines.push(format!("  cache reuse: {:.1}%", ratio * 100.0)),
        None => lines.push("  cache reuse: n/a".into()),
    }
    match snapshot.cache.token_growth {
        Some(growth) => lines.push(format!("  token growth: +{growth}")),
        None => lines.push("  token growth: n/a".into()),
    }
    match snapshot.cache.cost_delta_usd {
        Some(cost) => lines.push(format!("  cost delta: ${cost:.4}")),
        None => lines.push("  cost delta: n/a".into()),
    }
    lines.push(format!("  hot/cold/drift: {}/{}/{}", snapshot.cache.hot_count, snapshot.cache.cold_count, snapshot.cache.drift_count));
    lines.push(String::new());
    lines.push("Action Ledger".into());
    if snapshot.actions.is_empty() {
        lines.push("  none".into());
    } else {
        for row in &snapshot.actions {
            lines.push(format!(
                "  {} emitted={} accepted={} rejected={} blocked={} completed={} failed={} archived={} snapshot={} ignored={}",
                row.action,
                row.emitted,
                row.accepted,
                row.rejected,
                row.blocked,
                row.completed,
                row.failed,
                row.archived,
                row.snapshot_written,
                row.ignored,
            ));
        }
    }
    lines.push(String::new());
    lines.push("Recent Timeline".into());
    if snapshot.timeline.is_empty() {
        lines.push("  none".into());
    } else {
        for item in &snapshot.timeline {
            lines.push(format!(
                "  {} {} {} {} -> {}",
                item.ts_label, item.pane_id, item.situation, item.action, item.outcome
            ));
        }
    }
    lines.push(String::new());
    lines.push("Evidence".into());
    lines.push("  source tables: audit_events, token_usage_samples, cost_usage_events".into());
    lines.push("  lifecycle: audit-only, recommendation correlation unavailable".into());
    lines
}
```

Modify `src/lib.rs`:

```rust
pub mod insights_report;
```

Insert the module line after the existing `pub mod domain;` line.

- [ ] **Step 4: Run report tests**

Run:

```bash
cargo test --test insights_report_integration
```

Expected: all report integration tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/insights_report.rs src/lib.rs tests/insights_report_integration.rs
git commit -m "feat(app): format token insights report"
```

---

### Task 6: Add `qmonster insights --since` Subcommand

**Files:**
- Modify: `src/main.rs`
- Modify: `src/insights_report.rs`
- Test: `tests/insights_report_integration.rs`

- [ ] **Step 1: Add a failing test for report path resolution**

Append to `tests/insights_report_integration.rs`:

```rust
use qmonster::insights_report::resolve_insights_paths;

#[test]
fn resolve_insights_paths_uses_cli_root_without_tmux() {
    let td = tempfile::tempdir().unwrap();
    let (paths, source) = resolve_insights_paths(None, Some(td.path()), &[], None).unwrap();

    assert_eq!(paths.root(), td.path());
    assert_eq!(format!("{source:?}"), "Cli");
    assert!(paths.root().is_dir());
    assert!(paths.config_dir().is_dir());
}
```

- [ ] **Step 2: Run the test and confirm it fails**

Run:

```bash
cargo test --test insights_report_integration resolve_insights_paths_uses_cli_root_without_tmux
```

Expected: compile failure because `resolve_insights_paths` does not exist.

- [ ] **Step 3: Implement storage-only path resolution**

Add to `src/insights_report.rs`:

```rust
use std::path::Path;

use crate::app::config::{QmonsterConfig, load_with_local_override};
use crate::app::path_resolution::{RootSource, default_config_path, pick_root};
use crate::app::safety_audit::apply_override_with_audit;
use crate::store::{InMemorySink, QmonsterPaths};

pub fn resolve_insights_paths(
    config_path: Option<&Path>,
    root: Option<&Path>,
    set: &[String],
    env_root: Option<&str>,
) -> Result<(QmonsterPaths, RootSource)> {
    let default_config = default_config_path(root, env_root);
    let loaded_config = config_path
        .map(|p| p.to_path_buf())
        .or_else(|| if default_config.exists() { Some(default_config) } else { None });
    let mut config = match loaded_config.as_ref() {
        Some(path) => load_with_local_override(path)
            .with_context(|| format!("loading insights config {path:?}"))?,
        None => QmonsterConfig::defaults(),
    };

    let pairs = parse_set_pairs_for_insights(set)?;
    if !pairs.is_empty() {
        let refs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let sink = InMemorySink::new();
        apply_override_with_audit(&mut config, &refs, &sink);
    }

    let resolved = pick_root(root, env_root, &config);
    let source = resolved.source.clone();
    let paths = resolved.into_paths();
    paths.ensure().context("ensure qmonster insights storage root")?;
    Ok((paths, source))
}

fn parse_set_pairs_for_insights(set: &[String]) -> Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    for kv in set {
        let Some((key, value)) = kv.split_once('=') else {
            anyhow::bail!("--set expects key=value, got {kv}");
        };
        pairs.push((key.trim().to_string(), value.trim().to_string()));
    }
    Ok(pairs)
}
```

- [ ] **Step 4: Modify `src/main.rs` for the subcommand**

Change imports:

```rust
use clap::{Parser, Subcommand};
use qmonster::insights_report::{
    format_insights_report_lines, parse_since_arg, resolve_insights_paths,
};
use qmonster::store::{InsightsWindow, SqliteInsightsStore};
```

Add the command enum below `Cli`:

```rust
#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Print token optimization insights from the local SQLite store.
    Insights {
        /// Time window to inspect, such as 24h, 7d, 30m, or 900s.
        #[arg(long, default_value = "24h")]
        since: String,
    },
}
```

Add to `Cli`:

```rust
    #[command(subcommand)]
    command: Option<CliCommand>,
```

At the start of `main()`, after `let env_root = std::env::var("QMONSTER_ROOT").ok();`, add:

```rust
    if let Some(CliCommand::Insights { since }) = cli.command.as_ref() {
        let seconds = parse_since_arg(since)?;
        let (paths, root_source) = resolve_insights_paths(
            cli.config.as_deref(),
            cli.root.as_deref(),
            &cli.set,
            env_root.as_deref(),
        )?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let since_ms = now_ms.saturating_sub((seconds as i64).saturating_mul(1000));
        let store = SqliteInsightsStore::open(&paths.sqlite_path())?;
        let snapshot = store.snapshot(InsightsWindow {
            since_ms,
            until_ms: now_ms,
        })?;
        println!(
            "qmonster paths: {} (source: {:?})",
            paths.root().display(),
            root_source
        );
        for line in format_insights_report_lines(&snapshot) {
            println!("{line}");
        }
        return Ok(());
    }
```

This block must run before the existing startup runtime construction in `main()`.
Parent-level options such as `--root`, `--config`, and `--set` stay before the subcommand, for example `qmonster --root /tmp/qmonster insights --since 24h`.

- [ ] **Step 5: Run tests and command help**

Run:

```bash
cargo test --test insights_report_integration
cargo run -- --root /tmp/qmonster-insights-smoke insights --since 24h
```

Expected: tests pass. The command prints `qmonster paths:` and `Token Insights`; it does not require tmux.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/insights_report.rs tests/insights_report_integration.rs
git commit -m "feat(cli): add token insights report command"
```

---

### Deferred Task 7: Document Phase 8 v1 After Phase 7 v3 Docs Land

**Files:**
- Modify: `docs/ai/UI_MANUAL.md`
- Modify: `docs/ai/VALIDATION.md`
- Modify: `README.md`
- Test: documentation and full focused test set

Do not execute this deferred task in the current subagent-driven run. Phase 7 v3 is actively updating shared docs and ledger surfaces.

- [ ] **Step 1: Update user-facing docs**

In `docs/ai/UI_MANUAL.md`, add a short subsection after the Metrics Overlay section:

```markdown
### Token Insights report (Phase 8 v1)

`qmonster insights --since 24h` prints a read-only token optimization
report from the local SQLite store. It does not start tmux and does not
classify ignored recommendations yet. The report includes situation
counts, cache reuse, token growth, cost delta, action ledger counts, and
a recent timeline. Evidence labels name the SQLite source tables and
the audit-only lifecycle limitation. Exact token savings and
per-subagent attribution remain unsupported.
```

In `docs/ai/VALIDATION.md`, add:

```markdown
- Token Insights report smoke:
  `cargo run -- --root /tmp/qmonster-insights-smoke insights --since 24h`
  must print `Token Insights` without requiring a tmux session.
```

In `README.md`, add the command near the existing smoke checks:

```bash
cargo run -- --root /tmp/qmonster-insights-smoke insights --since 24h
```

- [ ] **Step 2: Run the focused validation**

Run:

```bash
cargo fmt --all --check
cargo test --test store_insights_integration
cargo test --test insights_report_integration
cargo run -- --root /tmp/qmonster-insights-smoke insights --since 24h
git diff --check
```

Expected:

- `cargo fmt --all --check` exits 0.
- Both integration test files pass.
- Report command prints `Token Insights`.
- `git diff --check` exits 0.

- [ ] **Step 3: Commit**

```bash
git add docs/ai/UI_MANUAL.md docs/ai/VALIDATION.md README.md
git commit -m "docs: document token insights report"
```

---

## Final Verification

Run:

```bash
cargo test --all-targets
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
git diff --check
```

Expected:

- All tests pass.
- Clippy exits 0.
- Diff check exits 0.

If these pass, Phase 8 v1 code is ready for review. Do not start the lifecycle ledger, docs/ledger sync, or TUI overlay in the same implementation run; those are Phase 8 v2/v3 or post-Phase-7-v3 follow-up work after this report proof is evaluated.
