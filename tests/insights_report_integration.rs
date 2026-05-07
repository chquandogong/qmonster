use qmonster::insights_report::{
    empty_insights_snapshot, format_insights_report_lines, parse_since_arg, resolve_insights_paths,
};
use qmonster::store::{
    ActionLedgerRow, CacheInsightSummary, InsightsSnapshot, InsightsWindow,
    RecommendationTimelineItem, SituationSummary,
};
use tempfile::tempdir;

#[test]
fn parse_since_accepts_hours_and_days() {
    assert_eq!(parse_since_arg("24h").unwrap(), 86_400);
    assert_eq!(parse_since_arg("7d").unwrap(), 604_800);
    assert_eq!(parse_since_arg("900s").unwrap(), 900);
}

#[test]
fn parse_since_accepts_minutes() {
    assert_eq!(parse_since_arg("30m").unwrap(), 1_800);
}

#[test]
fn parse_since_rejects_empty_or_unknown_units() {
    assert!(parse_since_arg("").is_err());
    assert!(parse_since_arg("24x").is_err());
    assert!(parse_since_arg("0s").is_err());
    assert!(parse_since_arg("0m").is_err());
}

#[test]
fn parse_since_rejects_overflow() {
    assert!(parse_since_arg(&format!("{}d", u64::MAX)).is_err());
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
    assert!(joined.contains("ignored=n/a"));
    assert!(!joined.contains("ignored=0"));
    assert!(joined.contains("Evidence"));
    assert!(joined.contains(
        "source tables: audit_events, token_usage_samples, cost_usage_events (when present)"
    ));
}

#[test]
fn insights_report_renders_available_recommendation_lifecycle() {
    let snapshot = InsightsSnapshot {
        window: InsightsWindow {
            since_ms: 0,
            until_ms: 86_400_000,
        },
        situations: vec![],
        cache: CacheInsightSummary::default(),
        timeline: vec![],
        actions: vec![ActionLedgerRow {
            action: "/compact".into(),
            ignored: 3,
            ..ActionLedgerRow::default()
        }],
        ignored_available: true,
    };

    let lines = format_insights_report_lines(&snapshot);
    let joined = lines.join("\n");

    assert!(joined.contains("ignored classification: available"));
    assert!(joined.contains("ignored=3"));
    assert!(joined.contains("lifecycle: recommendation outcomes available"));
    assert!(!joined.contains("correlation unavailable"));
}

#[test]
fn empty_snapshot_helper_returns_zero_state() {
    let snapshot = empty_insights_snapshot(InsightsWindow {
        since_ms: 100,
        until_ms: 200,
    });

    assert_eq!(snapshot.window.since_ms, 100);
    assert_eq!(snapshot.window.until_ms, 200);
    assert!(snapshot.situations.is_empty());
    assert!(snapshot.actions.is_empty());
    assert!(snapshot.timeline.is_empty());
    assert_eq!(snapshot.cache, CacheInsightSummary::default());
    assert!(!snapshot.ignored_available);
}

#[test]
fn resolve_insights_paths_uses_cli_root_without_tmux() {
    let td = tempdir().unwrap();
    let root = td.path().join("missing-root");

    let (paths, source) = resolve_insights_paths(None, Some(&root), &[], None).unwrap();

    assert_eq!(paths.root(), root);
    assert_eq!(format!("{source:?}"), "Cli");
    assert!(!paths.root().exists());
    assert!(!paths.config_dir().exists());
    assert!(!paths.archive_dir().exists());
    assert!(!paths.snapshot_dir().exists());
    assert!(!paths.sqlite_path().exists());
}
