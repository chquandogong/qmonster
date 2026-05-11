use chrono::TimeZone as _;
use qmonster::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
use qmonster::domain::audit::{AuditEvent, AuditEventKind};
use qmonster::domain::identity::{Provider, Role};
use qmonster::domain::recommendation::Severity;
use qmonster::store::{
    CostObservation, EventSink, InsightsWindow, RecommendationEventRecord, RecommendationOutcome,
    RecommendationOutcomeRecord, SqliteAnomalySink, SqliteAuditSink, SqliteCostUsageSink,
    SqliteInsightsStore, SqliteRecommendationLifecycleSink, SqliteTokenUsageSink, TokenSample,
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
    assert!(snapshot.next_best_action.data_available);
    assert!(snapshot.next_best_action.action.is_none());
    assert_eq!(snapshot.next_best_action.open_proposal_count, 0);
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
    assert!(!snapshot.next_best_action.data_available);
    assert!(snapshot.next_best_action.action.is_none());
}

#[test]
fn insights_next_best_action_selects_pending_strong_recommendation() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();

    lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 10_000,
            pane_id: "%1".into(),
            provider: Some("Codex".into()),
            role: Some("Review".into()),
            situation: "Context pressure".into(),
            action: "/compact".into(),
            severity: "risk".into(),
            source_kind: "Estimated".into(),
            reason_summary: "context near critical".into(),
            suggested_command: Some("/compact".into()),
            is_strong: true,
            dedup_key: "%1:/compact".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 20_000,
        })
        .unwrap();

    let action = snapshot
        .next_best_action
        .action
        .as_ref()
        .expect("pending strong rec should be selected");
    assert!(snapshot.next_best_action.data_available);
    assert_eq!(snapshot.next_best_action.open_proposal_count, 1);
    assert_eq!(action.pane_id, "%1");
    assert_eq!(action.action, "/compact");
    assert_eq!(action.reason_summary, "context near critical");
    assert_eq!(action.suggested_command.as_deref(), Some("/compact"));
}

#[test]
fn insights_next_best_action_picks_latest_unaccepted_strong_rec() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();

    let older = lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 10_000,
            pane_id: "%1".into(),
            provider: Some("Claude".into()),
            role: Some("Main".into()),
            situation: "Context pressure".into(),
            action: "/compact".into(),
            severity: "risk".into(),
            source_kind: "Estimated".into(),
            reason_summary: "older pending".into(),
            suggested_command: Some("/compact".into()),
            is_strong: true,
            dedup_key: "%1:/compact".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();
    assert!(older > 0);
    let accepted = lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 20_000,
            pane_id: "%2".into(),
            provider: Some("Codex".into()),
            role: Some("Review".into()),
            situation: "Context pressure".into(),
            action: "/compact".into(),
            severity: "risk".into(),
            source_kind: "Estimated".into(),
            reason_summary: "accepted newer".into(),
            suggested_command: Some("/compact".into()),
            is_strong: true,
            dedup_key: "%2:/compact".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();
    lifecycle
        .insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: 21_000,
            recommendation_event_id: Some(accepted),
            pane_id: "%2".into(),
            action: "/compact".into(),
            outcome: RecommendationOutcome::Accepted,
            audit_event_id: None,
            summary: "accepted".into(),
        })
        .unwrap();
    lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 30_000,
            pane_id: "%3".into(),
            provider: Some("Gemini".into()),
            role: Some("Main".into()),
            situation: "Quota-tight / cost".into(),
            action: "quota-pressure: pace requests".into(),
            severity: "warning".into(),
            source_kind: "Estimated".into(),
            reason_summary: "latest pending".into(),
            suggested_command: None,
            is_strong: true,
            dedup_key: "%3:quota".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 40_000,
        })
        .unwrap();

    let action = snapshot
        .next_best_action
        .action
        .as_ref()
        .expect("latest unaccepted strong rec should be selected");
    assert_eq!(snapshot.next_best_action.open_proposal_count, 2);
    assert_eq!(action.pane_id, "%3");
    assert_eq!(action.reason_summary, "latest pending");
    assert_eq!(action.suggested_command, None);
}

fn insert_completed_compact(
    lifecycle: &SqliteRecommendationLifecycleSink,
    event_ts: i64,
    outcome_ts: i64,
) {
    let id = lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: event_ts,
            pane_id: "%1".into(),
            provider: Some("Codex".into()),
            role: Some("Review".into()),
            situation: "Context pressure".into(),
            action: "/compact".into(),
            severity: "risk".into(),
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
            ts_unix_ms: outcome_ts,
            recommendation_event_id: Some(id),
            pane_id: "%1".into(),
            action: "/compact".into(),
            outcome: RecommendationOutcome::Completed,
            audit_event_id: None,
            summary: "completed".into(),
        })
        .unwrap();
}

fn insert_token_series(db_path: &std::path::Path, points: &[(i64, u64, u64)]) {
    let tokens = SqliteTokenUsageSink::open(db_path).unwrap();
    for (ts, input, cached) in points {
        tokens
            .record_sample(&TokenSample {
                ts_unix_ms: *ts,
                pane_id: "%1".into(),
                provider: Provider::Codex,
                input_tokens: Some(*input),
                output_tokens: Some(0),
                cost_usd: None,
                cached_input_tokens: Some(*cached),
            })
            .unwrap();
    }
}

#[test]
fn compact_payoff_reports_saved_tokens_when_threshold_and_samples_pass() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();
    insert_completed_compact(&lifecycle, 10_000, 20_000);
    insert_token_series(
        &db_path,
        &[
            (1_000, 0, 0),
            (2_000, 1_500, 0),
            (3_000, 3_000, 0),
            (4_000, 4_500, 0),
            (5_000, 6_000, 0),
            (9_000, 8_000, 500),
            (20_000, 8_000, 8_000),
            (21_000, 8_040, 8_000),
            (22_000, 8_080, 8_000),
            (23_000, 8_120, 8_000),
            (24_000, 8_160, 8_000),
            (30_000, 8_200, 8_200),
        ],
    );

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 40_000,
        })
        .unwrap();

    let payoff = snapshot.last_compact_payoff;
    assert_eq!(payoff.status, "saved");
    assert_eq!(payoff.input_tokens_saved, Some(7_800));
    assert_eq!(payoff.before_cache_state.as_deref(), Some("cold"));
    assert_eq!(payoff.after_cache_state.as_deref(), Some("warm"));
}

#[test]
fn compact_payoff_reports_neutral_when_change_is_below_threshold() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();
    insert_completed_compact(&lifecycle, 10_000, 20_000);
    insert_token_series(
        &db_path,
        &[
            (1_000, 0, 0),
            (2_000, 2_000, 0),
            (3_000, 4_000, 0),
            (4_000, 6_000, 0),
            (5_000, 8_000, 0),
            (9_000, 10_000, 4_000),
            (20_000, 10_000, 4_000),
            (21_000, 11_920, 4_000),
            (22_000, 13_840, 4_000),
            (23_000, 15_760, 4_000),
            (24_000, 17_680, 4_000),
            (30_000, 19_600, 4_000),
        ],
    );

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 40_000,
        })
        .unwrap();

    let payoff = snapshot.last_compact_payoff;
    assert_eq!(payoff.status, "neutral");
    assert_eq!(payoff.input_tokens_saved, Some(400));
}

#[test]
fn compact_payoff_reports_regressed_when_after_growth_is_higher() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();
    insert_completed_compact(&lifecycle, 10_000, 20_000);
    insert_token_series(
        &db_path,
        &[
            (1_000, 0, 0),
            (2_000, 200, 0),
            (3_000, 400, 0),
            (4_000, 600, 0),
            (5_000, 800, 0),
            (9_000, 1_000, 500),
            (20_000, 1_000, 500),
            (21_000, 1_800, 500),
            (22_000, 2_600, 500),
            (23_000, 3_400, 500),
            (24_000, 4_200, 500),
            (30_000, 5_000, 500),
        ],
    );

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 40_000,
        })
        .unwrap();

    let payoff = snapshot.last_compact_payoff;
    assert_eq!(payoff.status, "regressed");
    assert_eq!(payoff.input_tokens_saved, Some(-3_000));
}

#[test]
fn compact_payoff_reports_na_when_sample_count_is_too_low() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();
    insert_completed_compact(&lifecycle, 10_000, 20_000);
    insert_token_series(&db_path, &[(9_000, 8_000, 0), (20_000, 8_000, 0)]);

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 40_000,
        })
        .unwrap();

    let payoff = snapshot.last_compact_payoff;
    assert_eq!(payoff.status, "n/a");
    assert!(payoff.input_tokens_saved.is_none());
}

#[test]
fn cost_breakdown_groups_window_cost_by_pane_model_and_situation() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();
    lifecycle
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
            threshold_snapshot_json: None,
        })
        .unwrap();
    lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 1_000,
            pane_id: "%2".into(),
            provider: Some("Claude".into()),
            role: Some("Main".into()),
            situation: "Verbose review".into(),
            action: "verbose-output".into(),
            severity: "concern".into(),
            source_kind: "Heuristic".into(),
            reason_summary: "verbose output".into(),
            suggested_command: None,
            is_strong: false,
            dedup_key: "%2:verbose".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();
    let costs = SqliteCostUsageSink::open(&db_path).unwrap();
    for obs in [
        CostObservation {
            ts_unix_ms: 2_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            cumulative_cost_usd: 0.40,
        },
        CostObservation {
            ts_unix_ms: 3_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            cumulative_cost_usd: 0.74,
        },
        CostObservation {
            ts_unix_ms: 2_000,
            pane_id: "%2".into(),
            provider: Provider::Claude,
            cumulative_cost_usd: 0.38,
        },
    ] {
        costs.record_observation(&obs).unwrap();
    }

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 10_000,
        })
        .unwrap();

    assert!((snapshot.cost_breakdown.total_usd - 1.12).abs() < 1e-9);
    assert!(
        snapshot
            .cost_breakdown
            .by_pane
            .iter()
            .any(|row| row.label == "%1 Codex" && (row.cost_delta_usd - 0.74).abs() < 1e-9)
    );
    assert!(
        snapshot
            .cost_breakdown
            .by_model
            .iter()
            .any(|row| row.label == "Codex" && (row.cost_delta_usd - 0.74).abs() < 1e-9)
    );
    assert!(
        snapshot
            .cost_breakdown
            .by_situation
            .iter()
            .any(|row| row.label == "Context pressure" && (row.cost_delta_usd - 0.74).abs() < 1e-9)
    );
}

#[test]
fn cost_breakdown_no_data_returns_empty_groups() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 10_000,
        })
        .unwrap();

    assert_eq!(snapshot.cost_breakdown.total_usd, 0.0);
    assert!(snapshot.cost_breakdown.by_pane.is_empty());
    assert!(snapshot.cost_breakdown.by_model.is_empty());
    assert!(snapshot.cost_breakdown.by_situation.is_empty());
}

#[test]
fn anomaly_correlations_group_two_panes_with_same_kind_and_confidence_in_same_minute() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let sink = SqliteAnomalySink::open(&db_path).unwrap();
    for (pane, kind, confidence) in [
        ("%1", AnomalyKind::ErrorBurst, AnomalyConfidence::High),
        ("%2", AnomalyKind::ErrorBurst, AnomalyConfidence::High),
    ] {
        sink.insert_anomaly_event(&AnomalyEvent {
            timestamp: 1_700_000_020,
            pane_id: pane.into(),
            kind,
            confidence,
            severity: Severity::Warning,
            promoted: true,
            reason: "correlated".into(),
            evidence: Vec::new(),
        })
        .unwrap();
    }

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 1_700_000_000_000,
            until_ms: 1_700_000_120_000,
        })
        .unwrap();

    assert_eq!(snapshot.anomaly_correlations.len(), 1);
    let row = &snapshot.anomaly_correlations[0];
    assert_eq!(row.pane_count, 2);
    assert!(row.summary.contains("%1(ErrorBurst:high)"));
    assert!(row.summary.contains("%2(ErrorBurst:high)"));
}

#[test]
fn anomaly_correlations_do_not_group_different_kinds_in_same_minute() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let sink = SqliteAnomalySink::open(&db_path).unwrap();
    for (pane, kind, confidence) in [
        ("%1", AnomalyKind::ErrorBurst, AnomalyConfidence::High),
        (
            "%2",
            AnomalyKind::CacheDiscontinuity,
            AnomalyConfidence::High,
        ),
    ] {
        sink.insert_anomaly_event(&AnomalyEvent {
            timestamp: 1_700_000_020,
            pane_id: pane.into(),
            kind,
            confidence,
            severity: Severity::Warning,
            promoted: true,
            reason: "same minute but different detector".into(),
            evidence: Vec::new(),
        })
        .unwrap();
    }

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 1_700_000_000_000,
            until_ms: 1_700_000_120_000,
        })
        .unwrap();

    assert!(snapshot.anomaly_correlations.is_empty());
}

#[test]
fn anomaly_correlations_ignore_same_pane_or_different_minutes() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let sink = SqliteAnomalySink::open(&db_path).unwrap();
    for (ts, pane) in [
        (1_700_000_020, "%1"),
        (1_700_000_030, "%1"),
        (1_700_000_180, "%2"),
    ] {
        sink.insert_anomaly_event(&AnomalyEvent {
            timestamp: ts,
            pane_id: pane.into(),
            kind: AnomalyKind::ErrorBurst,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "not correlated".into(),
            evidence: Vec::new(),
        })
        .unwrap();
    }

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot(InsightsWindow {
            since_ms: 1_700_000_000_000,
            until_ms: 1_700_000_240_000,
        })
        .unwrap();

    assert!(snapshot.anomaly_correlations.is_empty());
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
fn cache_summary_buckets_token_growth_by_pane_and_provider() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");

    let tokens = SqliteTokenUsageSink::open(&db_path).unwrap();
    for sample in [
        TokenSample {
            ts_unix_ms: 1_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1_000),
            output_tokens: Some(10),
            cost_usd: None,
            cached_input_tokens: Some(0),
        },
        TokenSample {
            ts_unix_ms: 2_000,
            pane_id: "%2".into(),
            provider: Provider::Gemini,
            input_tokens: Some(10),
            output_tokens: Some(5),
            cost_usd: None,
            cached_input_tokens: Some(90),
        },
        TokenSample {
            ts_unix_ms: 3_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1_100),
            output_tokens: Some(20),
            cost_usd: None,
            cached_input_tokens: Some(100),
        },
        TokenSample {
            ts_unix_ms: 4_000,
            pane_id: "%2".into(),
            provider: Provider::Gemini,
            input_tokens: Some(20),
            output_tokens: Some(10),
            cost_usd: None,
            cached_input_tokens: Some(80),
        },
    ] {
        tokens.record_sample(&sample).unwrap();
    }

    let costs = SqliteCostUsageSink::open(&db_path).unwrap();
    costs
        .record_observation(&CostObservation {
            ts_unix_ms: 1_500,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            cumulative_cost_usd: 0.20,
        })
        .unwrap();
    costs
        .record_observation(&CostObservation {
            ts_unix_ms: 2_500,
            pane_id: "%2".into(),
            provider: Provider::Gemini,
            cumulative_cost_usd: 0.03,
        })
        .unwrap();

    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 5_000,
        })
        .unwrap();

    assert_eq!(snapshot.cache.token_growth, Some(110));
    assert!((snapshot.cache.cost_delta_usd.unwrap() - 0.23).abs() < 0.000_001);
    assert_eq!(snapshot.cache.latest_cache_ratio, Some(0.8));
    assert_eq!(snapshot.data_completeness.pane_bucket_count, 2);
    assert_eq!(snapshot.data_completeness.token_sample_count, 4);
    assert_eq!(snapshot.data_completeness.cache_ratio_bucket_count, 2);
    assert_eq!(snapshot.data_completeness.cost_delta_bucket_count, 2);
    assert_eq!(snapshot.data_completeness.panes.len(), 2);
    assert!(snapshot.data_completeness.panes.iter().any(|pane| {
        pane.pane_id == "%1"
            && pane.provider == "Codex"
            && pane.status == "ok"
            && pane.coverage_pct == Some(100.0)
            && pane.missing_polls == Some(0)
    }));

    let codex = snapshot
        .pane_buckets
        .iter()
        .find(|bucket| bucket.pane_id == "%1" && bucket.provider == "Codex")
        .unwrap();
    assert_eq!(codex.token_growth, Some(100));
    assert_eq!(codex.latest_cache_ratio, Some(0.08));
    assert!((codex.cost_delta_usd.unwrap() - 0.20).abs() < 0.000_001);
    assert_eq!(codex.sample_count, 2);

    let gemini = snapshot
        .pane_buckets
        .iter()
        .find(|bucket| bucket.pane_id == "%2" && bucket.provider == "Gemini")
        .unwrap();
    assert_eq!(gemini.token_growth, Some(10));
    assert_eq!(gemini.latest_cache_ratio, Some(0.8));
    assert!((gemini.cost_delta_usd.unwrap() - 0.03).abs() < 0.000_001);
    assert_eq!(gemini.sample_count, 2);
}

#[test]
fn data_completeness_marks_identity_suppressed_panes() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let _tokens = SqliteTokenUsageSink::open(&db_path).unwrap();
    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "INSERT INTO audit_events
         (ts_utc, kind, severity, pane_id, provider, role, summary)
         VALUES (?1, 'IdentitySuppressed', 'Concern', '%9', 'Codex', 'Review', 'identity conflict')",
        [chrono::Utc
            .timestamp_millis_opt(5_000)
            .single()
            .unwrap()
            .to_rfc3339()],
    )
    .unwrap();

    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 10_000,
        })
        .unwrap();

    let pane = snapshot
        .data_completeness
        .panes
        .iter()
        .find(|pane| pane.pane_id == "%9")
        .unwrap();
    assert_eq!(pane.provider, "Codex");
    assert_eq!(pane.status, "suppressed");
    assert_eq!(pane.coverage_pct, None);
    assert_eq!(pane.missing_polls, None);
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
fn cache_summary_continues_token_growth_after_counter_reset() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");

    let tokens = SqliteTokenUsageSink::open(&db_path).unwrap();
    for sample in [
        TokenSample {
            ts_unix_ms: 1_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1_000),
            output_tokens: Some(10),
            cost_usd: None,
            cached_input_tokens: Some(500),
        },
        TokenSample {
            ts_unix_ms: 2_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(100),
            output_tokens: Some(20),
            cost_usd: None,
            cached_input_tokens: Some(50),
        },
        TokenSample {
            ts_unix_ms: 3_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(160),
            output_tokens: Some(30),
            cost_usd: None,
            cached_input_tokens: Some(40),
        },
    ] {
        tokens.record_sample(&sample).unwrap();
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: now_ms + 60_000,
        })
        .unwrap();

    assert_eq!(snapshot.cache.token_growth, Some(60));
    assert_eq!(snapshot.pane_buckets[0].token_growth, Some(60));
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

#[test]
fn insights_lifecycle_dedupes_repeated_active_recommendation() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();

    let first_id = lifecycle
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
            threshold_snapshot_json: None,
        })
        .unwrap();
    let second_id = lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 8_000,
            pane_id: "%1".into(),
            provider: Some("Codex".into()),
            role: Some("Review".into()),
            situation: "Context pressure".into(),
            action: "/compact".into(),
            severity: "warning".into(),
            source_kind: "Estimated".into(),
            reason_summary: "context still near critical".into(),
            suggested_command: Some("/compact".into()),
            is_strong: true,
            dedup_key: "%1:/compact".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();

    assert_eq!(first_id, second_id);

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

    let compact = snapshot
        .actions
        .iter()
        .find(|row| row.action == "/compact")
        .unwrap();
    assert_eq!(compact.emitted, 1);
    assert_eq!(compact.ignored, 0);
    assert_eq!(snapshot.next_best_action.open_proposal_count, 1);
    assert_eq!(
        snapshot
            .next_best_action
            .action
            .as_ref()
            .map(|action| action.reason_summary.as_str()),
        Some("context still near critical")
    );
}

#[test]
fn insights_lifecycle_counts_learning_outcomes() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();

    let compact_id = lifecycle
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
            threshold_snapshot_json: None,
        })
        .unwrap();
    lifecycle
        .insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: 2_000,
            recommendation_event_id: Some(compact_id),
            pane_id: "%1".into(),
            action: "/compact".into(),
            outcome: RecommendationOutcome::Improved,
            audit_event_id: None,
            summary: "token growth improved after compact".into(),
        })
        .unwrap();

    let cache_id = lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 3_000,
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
        .insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: 4_000,
            recommendation_event_id: Some(cache_id),
            pane_id: "%2".into(),
            action: "cache: avoid /compact while cache is hot".into(),
            outcome: RecommendationOutcome::Avoided,
            audit_event_id: None,
            summary: "operator avoided compact and cache stayed useful".into(),
        })
        .unwrap();

    let noisy_id = lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 5_000,
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
    lifecycle
        .insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: 6_000,
            recommendation_event_id: Some(noisy_id),
            pane_id: "%3".into(),
            action: "quota-pressure: pace requests".into(),
            outcome: RecommendationOutcome::NoEffect,
            audit_event_id: None,
            summary: "quota pressure persisted after action".into(),
        })
        .unwrap();

    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 10_000,
        })
        .unwrap();

    let compact = snapshot
        .actions
        .iter()
        .find(|row| row.action == "/compact")
        .unwrap();
    assert_eq!(compact.improved, 1);

    let cache = snapshot
        .actions
        .iter()
        .find(|row| row.action == "cache: avoid /compact while cache is hot")
        .unwrap();
    assert_eq!(cache.avoided, 1);

    let quota = snapshot
        .actions
        .iter()
        .find(|row| row.action == "quota-pressure: pace requests")
        .unwrap();
    assert_eq!(quota.no_effect, 1);
}

#[test]
fn insights_lifecycle_reports_action_metric_impact_windows() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();

    let compact_id = lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 10_000,
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
            ts_unix_ms: 20_000,
            recommendation_event_id: Some(compact_id),
            pane_id: "%1".into(),
            action: "/compact".into(),
            outcome: RecommendationOutcome::Completed,
            audit_event_id: Some(42),
            summary: "prompt sent".into(),
        })
        .unwrap();

    let tokens = SqliteTokenUsageSink::open(&db_path).unwrap();
    for sample in [
        TokenSample {
            ts_unix_ms: 1_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(100),
            output_tokens: Some(10),
            cost_usd: None,
            cached_input_tokens: Some(100),
        },
        TokenSample {
            ts_unix_ms: 9_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(300),
            output_tokens: Some(20),
            cost_usd: None,
            cached_input_tokens: Some(700),
        },
        TokenSample {
            ts_unix_ms: 20_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(300),
            output_tokens: Some(20),
            cost_usd: None,
            cached_input_tokens: Some(700),
        },
        TokenSample {
            ts_unix_ms: 30_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(340),
            output_tokens: Some(25),
            cost_usd: None,
            cached_input_tokens: Some(160),
        },
    ] {
        tokens.record_sample(&sample).unwrap();
    }

    let costs = SqliteCostUsageSink::open(&db_path).unwrap();
    costs
        .record_observation(&CostObservation {
            ts_unix_ms: 1_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            cumulative_cost_usd: 0.20,
        })
        .unwrap();
    costs
        .record_observation(&CostObservation {
            ts_unix_ms: 30_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            cumulative_cost_usd: 0.24,
        })
        .unwrap();

    let store = SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 40_000,
        })
        .unwrap();

    let impact = snapshot
        .action_impacts
        .iter()
        .find(|impact| impact.action == "/compact")
        .unwrap();
    assert_eq!(impact.event_id, compact_id);
    assert_eq!(impact.pane_id, "%1");
    assert_eq!(impact.provider.as_deref(), Some("Codex"));
    assert_eq!(impact.outcome, "completed");
    assert_eq!(impact.before_token_growth, Some(200));
    assert_eq!(impact.after_token_growth, Some(40));
    assert_eq!(impact.token_growth_delta, Some(-160));
    assert!((impact.before_cost_delta_usd.unwrap() - 0.20).abs() < 0.000_001);
    assert!((impact.after_cost_delta_usd.unwrap() - 0.04).abs() < 0.000_001);
    assert!((impact.cost_delta_usd.unwrap() + 0.16).abs() < 0.000_001);
    assert_eq!(impact.before_cache_ratio, Some(0.7));
    assert_eq!(impact.after_cache_ratio, Some(0.32));
}

#[test]
fn insights_lifecycle_counts_hidden_and_suppresses_ignored_for_unlinked_matches() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();

    let hidden_id = lifecycle
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
            threshold_snapshot_json: None,
        })
        .unwrap();
    lifecycle
        .insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: 2_000,
            recommendation_event_id: Some(hidden_id),
            pane_id: "%1".into(),
            action: "/compact".into(),
            outcome: RecommendationOutcome::Hidden,
            audit_event_id: None,
            summary: "dismissed from queue".into(),
        })
        .unwrap();

    lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 1_500,
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
        .insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: 6_000,
            recommendation_event_id: None,
            pane_id: "%2".into(),
            action: "cache: avoid /compact while cache is hot".into(),
            outcome: RecommendationOutcome::Hidden,
            audit_event_id: None,
            summary: "queue row disappeared".into(),
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

    let compact = snapshot
        .actions
        .iter()
        .find(|row| row.action == "/compact")
        .unwrap();
    assert_eq!(compact.emitted, 1);
    assert_eq!(compact.hidden, 1);
    assert_eq!(compact.ignored, 0);

    let cache = snapshot
        .actions
        .iter()
        .find(|row| row.action == "cache: avoid /compact while cache is hot")
        .unwrap();
    assert_eq!(cache.emitted, 1);
    assert_eq!(cache.hidden, 1);
    assert_eq!(cache.ignored, 0);

    assert!(
        snapshot
            .timeline
            .iter()
            .any(|item| item.outcome == "hidden" && item.action == "/compact")
    );
    assert!(!snapshot.timeline.iter().any(|item| {
        item.outcome == "ignored" && item.action == "cache: avoid /compact while cache is hot"
    }));
}

#[test]
fn insights_lifecycle_unlinked_outcome_matches_only_one_prior_event() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();
    let action = "cache: avoid /compact while cache is hot";

    lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 1_000,
            pane_id: "%2".into(),
            provider: Some("Codex".into()),
            role: Some("Main".into()),
            situation: "Context pressure".into(),
            action: action.into(),
            severity: "concern".into(),
            source_kind: "Estimated".into(),
            reason_summary: "first cache warning".into(),
            suggested_command: None,
            is_strong: false,
            dedup_key: "%2:cache-hot:first".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();
    lifecycle
        .insert_recommendation_outcome(&RecommendationOutcomeRecord {
            ts_unix_ms: 2_000,
            recommendation_event_id: None,
            pane_id: "%2".into(),
            action: action.into(),
            outcome: RecommendationOutcome::Hidden,
            audit_event_id: None,
            summary: "first warning hidden".into(),
        })
        .unwrap();
    lifecycle
        .insert_recommendation_event(&RecommendationEventRecord {
            ts_unix_ms: 3_000,
            pane_id: "%2".into(),
            provider: Some("Codex".into()),
            role: Some("Main".into()),
            situation: "Context pressure".into(),
            action: action.into(),
            severity: "concern".into(),
            source_kind: "Estimated".into(),
            reason_summary: "second cache warning".into(),
            suggested_command: None,
            is_strong: false,
            dedup_key: "%2:cache-hot:second".into(),
            threshold_snapshot_json: None,
        })
        .unwrap();

    let snapshot = SqliteInsightsStore::open(&db_path)
        .unwrap()
        .snapshot_with_ignored_ttl(
            InsightsWindow {
                since_ms: 0,
                until_ms: 10_000,
            },
            5,
        )
        .unwrap();
    let row = snapshot
        .actions
        .iter()
        .find(|row| row.action == action)
        .unwrap();

    assert_eq!(row.emitted, 2);
    assert_eq!(row.hidden, 1);
    assert_eq!(
        row.ignored, 1,
        "the later unresolved re-emission must still age into ignored"
    );
}
