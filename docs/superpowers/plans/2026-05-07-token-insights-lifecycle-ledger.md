# Token Insights Phase 8 v2 Lifecycle Ledger Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the Phase 8 v2 recommendation lifecycle ledger foundation so Token Insights can report exact persisted recommendation events and conservative TTL ignored classifications.

**Architecture:** Keep this slice storage/query/config only. SQLite owns `recommendation_events` and `recommendation_outcomes`; `store/recommendation_lifecycle.rs` owns typed writes; `store/insights.rs` reads the new tables when present and still degrades on old read-only DBs. Runtime emission hooks, prompt-send correlation, hide/dismiss capture, and the `i` TUI overlay stay out of this slice to avoid Phase 7 event-loop/bootstrap conflicts.

**Tech Stack:** Rust, rusqlite, serde/TOML config, existing `AuditDb` schema bootstrap, existing integration tests under `tests/`.

---

## File Structure

- `src/app/config.rs` — add `[insights]` config with `ignored_ttl_secs = 1800` and `default_window_secs = 86400`.
- `src/insights_report.rs` — resolve config alongside paths and expose the ignored TTL to the report caller.
- `src/main.rs` — pass `config.insights.ignored_ttl_secs` into the snapshot query.
- `src/store/sqlite.rs` — add idempotent recommendation lifecycle schemas.
- `src/store/recommendation_lifecycle.rs` — typed writer API for recommendation event/outcome rows.
- `src/store/insights.rs` — add lifecycle-aware aggregation and read-only TTL ignored classification.
- `src/store/mod.rs` — export the new lifecycle types.
- `src/ui/settings.rs` — surface `[insights]` values in Parameters and TTL rule wording.
- `config/qmonster.example.toml` — document `[insights]`.
- `tests/store_insights_integration.rs` — fixture DB tests for lifecycle rows and TTL ignored.
- `tests/insights_report_integration.rs` — config resolution test for report TTL.

No changes in this slice:

- `src/app/event_loop.rs`
- `src/app/bootstrap.rs`
- `src/app/prompt_send_actions.rs`
- `src/app/tui_loop.rs`
- `src/ui/insights.rs`
- `src/app/insights_overlay.rs`

## Task 1: Config And Report TTL Plumbing

**Files:**
- Modify: `src/app/config.rs`
- Modify: `src/insights_report.rs`
- Modify: `src/main.rs`
- Modify: `src/ui/settings.rs`
- Modify: `config/qmonster.example.toml`
- Test: `tests/insights_report_integration.rs`

- [ ] **Step 1: Write config tests**

Add tests in `src/app/config.rs`:

```rust
#[test]
fn insights_config_defaults_match_phase8_v2_spec() {
    let c = InsightsConfig::default();
    assert_eq!(c.ignored_ttl_secs, 30 * 60);
    assert_eq!(c.default_window_secs, 24 * 60 * 60);
}

#[test]
fn insights_config_loads_from_toml_with_partial_overrides() {
    let toml = r#"
[insights]
ignored_ttl_secs = 600
"#;
    let cfg: QmonsterConfig = toml::from_str(toml).expect("parse");
    assert_eq!(cfg.insights.ignored_ttl_secs, 600);
    assert_eq!(cfg.insights.default_window_secs, 24 * 60 * 60);
}
```

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test --lib app::config::tests::insights_config_
```

Expected: compile failure because `InsightsConfig` and `QmonsterConfig.insights` do not exist.

- [ ] **Step 3: Implement config**

Add `pub insights: InsightsConfig` to `QmonsterConfig`, define:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct InsightsConfig {
    pub ignored_ttl_secs: u64,
    pub default_window_secs: u64,
}

impl Default for InsightsConfig {
    fn default() -> Self {
        Self {
            ignored_ttl_secs: 30 * 60,
            default_window_secs: 24 * 60 * 60,
        }
    }
}
```

Wire it through `QmonsterConfig::defaults()`.

- [ ] **Step 4: Return config from report path resolution**

Change `resolve_insights_paths(...)` to return `(QmonsterPaths, RootSource, QmonsterConfig)`. Update existing callers/tests.

- [ ] **Step 5: Main uses config default window and TTL**

In `src/main.rs`, keep `--since` default as `24h` for CLI compatibility, but pass `config.insights.ignored_ttl_secs` to the snapshot query once Task 3 adds the method.

- [ ] **Step 6: Settings and example TOML**

Add Parameters rows `insights ignored_ttl/default_window` and a Rules row describing `TTL ignored after <duration> without correlated outcome`. Add `[insights]` to `config/qmonster.example.toml`.

- [ ] **Step 7: Verify and commit**

Run:

```bash
cargo test --lib app::config::tests::insights_config_
cargo test --lib ui::settings::tests::parameters_tab_shows_insights_config
cargo test --lib ui::settings::tests::rules_tab_shows_insights_ttl_rule
cargo test --test insights_report_integration
```

Commit:

```bash
git add src/app/config.rs src/insights_report.rs src/main.rs src/ui/settings.rs config/qmonster.example.toml tests/insights_report_integration.rs
git commit -m "phase 8 v2: add insights TTL config"
```

## Task 2: Recommendation Lifecycle Schema And Writer

**Files:**
- Modify: `src/store/sqlite.rs`
- Create: `src/store/recommendation_lifecycle.rs`
- Modify: `src/store/mod.rs`

- [ ] **Step 1: Write writer tests**

In the new module tests, verify:

```rust
#[test]
fn lifecycle_sink_round_trips_event_and_outcome_ids() {
    let tmp = tempfile::TempDir::new().unwrap();
    let sink = SqliteRecommendationLifecycleSink::open(&tmp.path().join("audit.db")).unwrap();
    let event_id = sink.insert_recommendation_event(&RecommendationEventRecord {
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
    }).unwrap();
    let outcome_id = sink.insert_recommendation_outcome(&RecommendationOutcomeRecord {
        ts_unix_ms: 2_000,
        recommendation_event_id: Some(event_id),
        pane_id: "%1".into(),
        action: "/compact".into(),
        outcome: RecommendationOutcome::Completed,
        audit_event_id: Some(42),
        summary: "prompt sent".into(),
    }).unwrap();
    assert!(event_id > 0);
    assert!(outcome_id > 0);
}
```

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test --lib store::recommendation_lifecycle::tests::lifecycle_sink_round_trips_event_and_outcome_ids
```

Expected: compile failure because the module does not exist.

- [ ] **Step 3: Implement schema and writer**

Add `RECOMMENDATION_LIFECYCLE_SCHEMA` in `src/store/sqlite.rs` and execute it from `AuditDb::open`. Add typed structs and `RecommendationOutcome::{label, try_from_label}` in `src/store/recommendation_lifecycle.rs`.

- [ ] **Step 4: Verify and commit**

Run:

```bash
cargo test --lib store::recommendation_lifecycle
cargo test --lib store::sqlite::tests::audit_db_open_creates_recommendation_lifecycle_tables
```

Commit:

```bash
git add src/store/sqlite.rs src/store/recommendation_lifecycle.rs src/store/mod.rs
git commit -m "phase 8 v2: add recommendation lifecycle SQLite ledger"
```

## Task 3: Lifecycle-Aware Insights Query

**Files:**
- Modify: `src/store/insights.rs`
- Modify: `tests/store_insights_integration.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write lifecycle aggregation tests**

Add tests that create a DB through `SqliteRecommendationLifecycleSink`, insert:

- one `/compact` event with `Completed` outcome
- one `cache: avoid /compact while cache is hot` event older than TTL with no outcome
- one event newer than TTL with no outcome

Assert:

- `snapshot_with_ignored_ttl(..., 1800).ignored_available == true`
- `/compact` has `emitted = 1` and `completed = 1`
- the old cache action has `ignored = 1`
- the new pending action has `ignored = 0`

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test --test store_insights_integration insights_lifecycle_counts_outcomes_and_ttl_ignored
```

Expected: compile failure because `snapshot_with_ignored_ttl` does not exist.

- [ ] **Step 3: Implement read-only lifecycle aggregation**

Add:

```rust
pub fn snapshot_with_ignored_ttl(
    &self,
    window: InsightsWindow,
    ignored_ttl_secs: u64,
) -> Result<InsightsSnapshot, SqliteError>
```

Keep `snapshot(window)` as `snapshot_with_ignored_ttl(window, 30 * 60)` for existing callers. If lifecycle tables are missing, preserve audit-only behavior and `ignored_available = false`. If tables exist, aggregate lifecycle events/outcomes and classify unresolved rows older than TTL as ignored without writing to SQLite.

- [ ] **Step 4: Verify and commit**

Run:

```bash
cargo test --test store_insights_integration
cargo test --test cli_insights_integration
```

Commit:

```bash
git add src/store/insights.rs tests/store_insights_integration.rs src/main.rs
git commit -m "phase 8 v2: count lifecycle outcomes in token insights"
```

## Task 4: Full Verification

**Files:** no source changes expected.

- [ ] **Step 1: Run focused tests**

```bash
cargo test --lib app::config::tests::insights_config_
cargo test --lib store::recommendation_lifecycle
cargo test --test store_insights_integration
cargo test --test insights_report_integration
cargo test --test cli_insights_integration
```

- [ ] **Step 2: Run repo gate**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --all-targets
```

- [ ] **Step 3: Smoke report**

```bash
cargo run -- --root /tmp/qmonster-insights-v2-smoke insights --since 24h
```

Expected: prints `Token Insights`; missing root still creates no files.

