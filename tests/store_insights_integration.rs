use qmonster::domain::audit::{AuditEvent, AuditEventKind};
use qmonster::domain::identity::{Provider, Role};
use qmonster::domain::recommendation::Severity;
use qmonster::store::{EventSink, InsightsWindow, SqliteAuditSink, SqliteInsightsStore};
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
    assert_eq!(snapshot.ignored_available, false);
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
    assert_eq!(situation_count("Context pressure"), 6);
    assert_eq!(situation_count("Verbose review"), 1);
    assert_eq!(situation_count("Code exploration"), 1);
    assert_eq!(situation_count("Quota-tight / cost"), 1);
}
