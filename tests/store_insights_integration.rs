use chrono::TimeZone as _;
use qmonster::domain::audit::{AuditEvent, AuditEventKind};
use qmonster::domain::identity::{Provider, Role};
use qmonster::domain::recommendation::Severity;
use qmonster::store::{
    CostObservation, EventSink, InsightsWindow, RecommendationEventRecord, RecommendationOutcome,
    RecommendationOutcomeRecord, SqliteAuditSink, SqliteCostUsageSink, SqliteInsightsStore,
    SqliteRecommendationLifecycleSink, SqliteTokenUsageSink, TokenSample,
};
use rusqlite::Connection;
use tempfile::tempdir;

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
    assert!(!snapshot.ignored_available);
}

#[test]
fn insights_read_only_open_does_not_create_missing_db() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("missing").join("qmonster.db");

    let err = match SqliteInsightsStore::open_read_only(&db_path) {
        Ok(_) => panic!("expected read-only open to fail for missing db"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("sqlite open"));
    assert!(!db_path.exists());
    assert!(!db_path.parent().unwrap().exists());
}

#[test]
fn insights_read_only_snapshot_supports_old_schema_db() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let now_ms = chrono::Utc::now().timestamp_millis();
    let now_ts = chrono::Utc.timestamp_millis_opt(now_ms).single().unwrap();
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        r"
        CREATE TABLE audit_events (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            ts_utc       TEXT    NOT NULL,
            kind         TEXT    NOT NULL,
            severity     TEXT    NOT NULL,
            pane_id      TEXT    NOT NULL,
            provider     TEXT,
            role         TEXT,
            summary      TEXT    NOT NULL
        );
        CREATE TABLE token_usage_samples (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            ts_unix_ms    INTEGER NOT NULL,
            pane_id       TEXT    NOT NULL,
            provider      TEXT    NOT NULL,
            input_tokens  INTEGER,
            output_tokens INTEGER,
            cost_usd      REAL
        );
        ",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO audit_events
         (ts_utc, kind, severity, pane_id, provider, role, summary)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        (
            now_ts.to_rfc3339(),
            "RecommendationEmitted",
            "Concern",
            "%1",
            "Codex",
            "Review",
            "cache: avoid /compact while cache is hot: cache hit ratio unavailable",
        ),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO token_usage_samples
         (ts_unix_ms, pane_id, provider, input_tokens, output_tokens, cost_usd)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (1_000_i64, "%1", "Codex", 100_i64, 10_i64, 0.01_f64),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO token_usage_samples
         (ts_unix_ms, pane_id, provider, input_tokens, output_tokens, cost_usd)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (2_000_i64, "%1", "Codex", 300_i64, 20_i64, 0.02_f64),
    )
    .unwrap();
    drop(conn);

    let store = SqliteInsightsStore::open_read_only(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: now_ms + 10_000,
        })
        .unwrap();

    assert_eq!(snapshot.cache.hot_count, 1);
    assert_eq!(snapshot.cache.latest_cache_ratio, None);
    assert_eq!(snapshot.cache.token_growth, Some(200));
    assert_eq!(snapshot.cache.cost_delta_usd, None);
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

#[test]
fn insights_counts_live_colon_actions_by_situation() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let audit = SqliteAuditSink::open(&db_path).unwrap();

    for (kind, pane_id, summary) in [
        (
            AuditEventKind::RecommendationEmitted,
            "%1",
            "aggressive: drop non-essential ingress: quota-tight reason",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%2",
            "aggressive: terse profile + archive: quota-tight reason",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%3",
            "aggressive: strip attribution: quota-tight reason",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%4",
            "aggressive: dedupe + hash: quota-tight reason",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%4b",
            "aggressive: clamp output, archive all: quota-tight reason",
        ),
        (
            AuditEventKind::AlertFired,
            "%5",
            "anomaly: cross-pane edit cluster detected: multiple panes editing same file",
        ),
        (
            AuditEventKind::AlertFired,
            "%6",
            "anomaly: cache discontinuity detected: cache hit ratio dropped",
        ),
        (
            AuditEventKind::AlertFired,
            "%7",
            "anomaly: cost slope detected: cost climbing",
        ),
        (
            AuditEventKind::AlertFired,
            "%7b",
            "anomaly: token slope detected: token count rising",
        ),
        (
            AuditEventKind::AlertFired,
            "%7c",
            "anomaly: error burst detected: failures spiking",
        ),
        (
            AuditEventKind::AlertFired,
            "%7d",
            "anomaly: identity churn detected: provider switching repeatedly",
        ),
        (
            AuditEventKind::AlertFired,
            "%7e",
            "anomaly: memory growth detected: resident set climbing",
        ),
        (
            AuditEventKind::AlertFired,
            "%7f",
            "anomaly: subagent activity correlated with other anomalies: multi-signal cluster",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%8",
            "cache: /compact is safe - cache is cold: cache hit ratio 42%",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%9",
            "cache: /compact is safe — cache is cold: cache hit ratio 41%",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%10",
            "cache: drift detected - /compact will let cache rebuild: cache hit ratio 51%",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%11",
            "cache: drift detected — /compact will let cache rebuild: cache hit ratio 49%",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%12",
            "agent-memory: trim before /compact: memory pressure rising",
        ),
        (
            AuditEventKind::AlertFired,
            "%13",
            "identity-drift: provider changed: codex -> claude",
        ),
        (
            AuditEventKind::AlertFired,
            "%14",
            "identity-drift: worktree changed: switched branches",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%15",
            "quota-pressure: 5h act now: nearing 5h limit",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%16",
            "quota-pressure: weekly pace: nearing weekly limit",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%17",
            "cost-pressure: pace: spend is rising",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%18",
            "cost-pressure: act now: spend is critical",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%19",
            "provider-profile: codex-default: current provider profile is expensive",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%20",
            "security-posture: elevated sandbox required: awaiting approval",
        ),
        (
            AuditEventKind::RecommendationEmitted,
            "%21",
            "auto-memory: summarize stale thread: background compaction suggested",
        ),
    ] {
        audit.record(audit_event(kind, pane_id, summary));
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: now_ms - 60_000,
            until_ms: now_ms + 60_000,
        })
        .unwrap();

    let situation_count = |label| {
        snapshot
            .situations
            .iter()
            .find(|s| s.situation == label)
            .map(|s| s.emitted)
            .unwrap_or(0)
    };

    assert_eq!(situation_count("Log storm / repeated output"), 2);
    assert_eq!(situation_count("Context pressure"), 10);
    assert_eq!(situation_count("Verbose review"), 1);
    assert_eq!(situation_count("Code exploration"), 5);
    assert_eq!(situation_count("Quota-tight / cost"), 7);
    assert_eq!(situation_count("Other"), 2);
}

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

#[test]
fn cache_summary_ignores_trailing_sparse_token_row_for_ratio_and_growth() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");

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
    tokens
        .record_sample(&TokenSample {
            ts_unix_ms: 3_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: None,
            output_tokens: Some(30),
            cost_usd: Some(0.12),
            cached_input_tokens: None,
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

    assert_eq!(snapshot.cache.latest_cache_ratio, Some(0.75));
    assert_eq!(snapshot.cache.token_growth, Some(200));
}

#[test]
fn cache_summary_saturates_token_growth_on_counter_reset() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");

    let tokens = SqliteTokenUsageSink::open(&db_path).unwrap();
    tokens
        .record_sample(&TokenSample {
            ts_unix_ms: 1_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1_000),
            output_tokens: Some(10),
            cost_usd: None,
            cached_input_tokens: Some(500),
        })
        .unwrap();
    tokens
        .record_sample(&TokenSample {
            ts_unix_ms: 2_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(100),
            output_tokens: Some(20),
            cost_usd: None,
            cached_input_tokens: Some(50),
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

    assert_eq!(snapshot.cache.token_growth, Some(0));
}

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
        "%1 /compact (proposal pid-42) (acknowledged by operator; executing)",
    ));
    audit.record(audit_event(
        AuditEventKind::PromptSendCompleted,
        "%1",
        "%1 /compact (proposal pid-42) (sent; operator-confirmed)",
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
    assert!(
        snapshot
            .actions
            .iter()
            .all(|row| row.action != "prompt-send")
    );

    let snapshot_row = snapshot
        .actions
        .iter()
        .find(|row| row.action == "snapshot")
        .unwrap();
    assert_eq!(snapshot_row.snapshot_written, 1);

    assert!(
        snapshot
            .timeline
            .iter()
            .any(|item| item.outcome == "completed" && item.action == "/compact")
    );
}

#[test]
fn insights_lifecycle_counts_outcomes_and_ttl_ignored() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();

    let compact_id = lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 2_000,
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
            threshold_snapshot_json: None,
        })
        .unwrap();
    lifecycle
        .insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: 3_000,
            recommendation_event_id: Some(compact_id),
            pane_id: "%1".into(),
            action: "/compact".into(),
            outcome: RecommendationOutcome::Completed,
            audit_event_id: Some(42),
            summary: "prompt sent".into(),
        })
        .unwrap();
    lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 1_000,
            pane_id: "%2".into(),
            provider: Some("Codex".into()),
            role: Some("Main".into()),
            situation: "Context pressure".into(),
            action: "cache: avoid /compact while cache is hot".into(),
            severity: "concern".into(),
            source_kind: "Estimated".into(),
            reason_summary: "cache is hot".into(),
            suggested_command: None,
            is_strong: false,
            dedup_key: "%2:cache-hot".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();
    lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 9_000,
            pane_id: "%3".into(),
            provider: Some("Codex".into()),
            role: Some("Main".into()),
            situation: "Quota-tight / cost".into(),
            action: "quota-pressure: pace requests".into(),
            severity: "concern".into(),
            source_kind: "Estimated".into(),
            reason_summary: "quota pressure is high".into(),
            suggested_command: None,
            is_strong: false,
            dedup_key: "%3:quota-pressure".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();

    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot_with_ignored_ttl(
            InsightsWindow {
                since_ms: 0,
                until_ms: 10_000,
            },
            5,
        )
        .unwrap();

    assert!(snapshot.ignored_available);
    let compact = snapshot
        .actions
        .iter()
        .find(|row| row.action == "/compact")
        .unwrap();
    assert_eq!(compact.emitted, 1);
    assert_eq!(compact.completed, 1);
    assert_eq!(compact.ignored, 0);

    let cache = snapshot
        .actions
        .iter()
        .find(|row| row.action == "cache: avoid /compact while cache is hot")
        .unwrap();
    assert_eq!(cache.emitted, 1);
    assert_eq!(cache.ignored, 1);

    let quota = snapshot
        .actions
        .iter()
        .find(|row| row.action == "quota-pressure: pace requests")
        .unwrap();
    assert_eq!(quota.emitted, 1);
    assert_eq!(quota.ignored, 0);

    assert!(
        snapshot
            .timeline
            .iter()
            .any(|item| item.outcome == "ignored" && item.action.contains("cache"))
    );
}
