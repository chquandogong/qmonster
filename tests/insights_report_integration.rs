use qmonster::insights_report::{
    empty_insights_snapshot, format_insights_report_lines, parse_since_arg, resolve_insights_paths,
};
use qmonster::store::{
    ActionImpactSummary, ActionLedgerRow, CacheInsightSummary, CostBreakdownRow,
    CostBreakdownSummary, InsightsDataCompleteness, InsightsSnapshot, InsightsWindow,
    NextBestActionItem, NextBestActionSummary, PaneDataCompleteness, PaneInsightBucket,
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
        pane_buckets: vec![PaneInsightBucket {
            pane_id: "%1".into(),
            provider: "Codex".into(),
            latest_cache_ratio: Some(0.75),
            token_growth: Some(200),
            cost_delta_usd: Some(0.08),
            sample_count: 2,
        }],
        next_best_action: NextBestActionSummary {
            data_available: true,
            action: Some(NextBestActionItem {
                ts_label: "2026-05-07T12:00:00Z".into(),
                pane_id: "%1".into(),
                provider: Some("Codex".into()),
                situation: "Context pressure",
                action: "/compact".into(),
                severity: "risk".into(),
                reason_summary: "context near critical".into(),
                suggested_command: Some("/compact".into()),
            }),
            open_proposal_count: 1,
        },
        last_compact_payoff: Default::default(),
        cost_breakdown: Default::default(),
        anomaly_correlations: Vec::new(),
        action_impacts: vec![ActionImpactSummary {
            event_id: 42,
            outcome_ts_label: "2026-05-07T12:05:00Z".into(),
            pane_id: "%1".into(),
            provider: Some("Codex".into()),
            action: "/compact".into(),
            outcome: "completed".into(),
            before_token_growth: Some(200),
            after_token_growth: Some(40),
            token_growth_delta: Some(-160),
            before_cost_delta_usd: Some(0.20),
            after_cost_delta_usd: Some(0.04),
            cost_delta_usd: Some(-0.16),
            before_cache_ratio: Some(0.70),
            after_cache_ratio: Some(0.32),
        }],
        data_completeness: InsightsDataCompleteness {
            pane_bucket_count: 1,
            token_sample_count: 2,
            cache_ratio_bucket_count: 1,
            cost_delta_bucket_count: 1,
            action_impact_count: 1,
            panes: vec![],
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
    assert!(
        joined.find("Now Suggested Action").unwrap() < joined.find("Situations").unwrap(),
        "suggested action should render before Situations: {joined}"
    );
    assert!(joined.contains("Now Suggested Action"));
    assert!(joined.contains("%1 Codex risk Context pressure: context near critical"));
    assert!(joined.contains("run: /compact"));
    assert!(joined.contains("open ★p:1"));
    assert!(joined.contains("/compact Payoff"));
    assert!(joined.contains("Context pressure"));
    assert!(joined.contains("cache reuse: 75.0%"));
    assert!(joined.contains("token growth: +200"));
    assert!(joined.contains("cost delta: $0.0800"));
    assert!(joined.contains("Cost Breakdown"));
    assert!(joined.contains("by pane: none"));
    assert!(joined.contains("Pane Buckets"));
    assert!(joined.contains("%1 Codex"));
    assert!(joined.contains("samples=2 cache=75.0% token growth=+200 cost delta=$0.0800"));
    assert!(joined.contains("Action Impact"));
    assert!(joined.contains("2026-05-07T12:05:00Z %1 Codex /compact completed"));
    assert!(joined.contains("tokens before=+200 after=+40 delta=-160"));
    assert!(joined.contains("cost before=$0.2000 after=$0.0400 delta=$-0.1600"));
    assert!(joined.contains("cache 70.0% -> 32.0%"));
    assert!(joined.contains("Data Completeness"));
    assert!(joined.contains("pane buckets: 1"));
    assert!(joined.contains("token samples: 2"));
    assert!(joined.contains("cache ratios: 1/1 buckets"));
    assert!(joined.contains("cost deltas: 1/1 buckets"));
    assert!(joined.contains("action impacts: 1"));
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
fn insights_report_labels_cost_breakdown_provider_group_honestly() {
    let snapshot = InsightsSnapshot {
        window: InsightsWindow {
            since_ms: 0,
            until_ms: 86_400_000,
        },
        situations: vec![],
        cache: CacheInsightSummary::default(),
        pane_buckets: vec![],
        next_best_action: NextBestActionSummary {
            data_available: true,
            action: None,
            open_proposal_count: 0,
        },
        last_compact_payoff: Default::default(),
        cost_breakdown: CostBreakdownSummary {
            total_usd: 0.42,
            by_pane: vec![],
            by_model: vec![CostBreakdownRow {
                label: "Codex".into(),
                cost_delta_usd: 0.42,
            }],
            by_situation: vec![],
        },
        anomaly_correlations: Vec::new(),
        action_impacts: vec![],
        data_completeness: InsightsDataCompleteness::default(),
        timeline: vec![],
        actions: vec![],
        ignored_available: false,
    };

    let joined = format_insights_report_lines(&snapshot).join("\n");

    assert!(joined.contains("by provider: Codex $0.4200"));
    assert!(!joined.contains("by model: Codex $0.4200"));
}

#[test]
fn insights_report_renders_pane_data_completeness_statuses() {
    let snapshot = InsightsSnapshot {
        window: InsightsWindow {
            since_ms: 0,
            until_ms: 86_400_000,
        },
        situations: vec![],
        cache: CacheInsightSummary::default(),
        pane_buckets: vec![],
        next_best_action: NextBestActionSummary {
            data_available: true,
            action: None,
            open_proposal_count: 0,
        },
        last_compact_payoff: Default::default(),
        cost_breakdown: Default::default(),
        anomaly_correlations: Vec::new(),
        action_impacts: vec![],
        data_completeness: InsightsDataCompleteness {
            pane_bucket_count: 1,
            token_sample_count: 3,
            cache_ratio_bucket_count: 1,
            cost_delta_bucket_count: 0,
            action_impact_count: 0,
            panes: vec![
                PaneDataCompleteness {
                    pane_id: "%1".into(),
                    provider: "Codex".into(),
                    status: "ok".into(),
                    coverage_pct: Some(100.0),
                    elapsed_secs: Some(4),
                    missing_polls: Some(0),
                },
                PaneDataCompleteness {
                    pane_id: "%9".into(),
                    provider: "Codex".into(),
                    status: "suppressed".into(),
                    coverage_pct: None,
                    elapsed_secs: None,
                    missing_polls: None,
                },
            ],
        },
        timeline: vec![],
        actions: vec![],
        ignored_available: false,
    };

    let joined = format_insights_report_lines(&snapshot).join("\n");

    assert!(joined.contains("%1 Codex ok coverage 100.0% missing 0 elapsed 4s"));
    assert!(joined.contains("%9 Codex suppressed coverage n/a missing n/a elapsed n/a"));
}

#[test]
fn insights_report_renders_action_rate_summary() {
    let snapshot = InsightsSnapshot {
        window: InsightsWindow {
            since_ms: 0,
            until_ms: 86_400_000,
        },
        situations: vec![],
        cache: CacheInsightSummary::default(),
        pane_buckets: vec![],
        next_best_action: NextBestActionSummary {
            data_available: true,
            action: None,
            open_proposal_count: 0,
        },
        last_compact_payoff: Default::default(),
        cost_breakdown: Default::default(),
        anomaly_correlations: Vec::new(),
        action_impacts: vec![],
        data_completeness: InsightsDataCompleteness::default(),
        timeline: vec![],
        actions: vec![
            ActionLedgerRow {
                action: "/compact".into(),
                emitted: 4,
                accepted: 2,
                completed: 1,
                ignored: 1,
                ..ActionLedgerRow::default()
            },
            ActionLedgerRow {
                action: "snapshot before 5h window resets".into(),
                emitted: 1,
                hidden: 1,
                ..ActionLedgerRow::default()
            },
        ],
        ignored_available: true,
    };

    let lines = format_insights_report_lines(&snapshot);
    let joined = lines.join("\n");

    assert!(joined.contains("Action Rates"));
    assert!(joined.contains("accepted rate: 40.0%"));
    assert!(joined.contains("completion rate: 50.0%"));
    assert!(joined.contains("ignored rate: 20.0%"));
}

#[test]
fn insights_report_renders_learning_outcomes_and_rule_tuning_candidates() {
    let snapshot = InsightsSnapshot {
        window: InsightsWindow {
            since_ms: 0,
            until_ms: 86_400_000,
        },
        situations: vec![],
        cache: CacheInsightSummary::default(),
        pane_buckets: vec![],
        next_best_action: NextBestActionSummary {
            data_available: true,
            action: None,
            open_proposal_count: 0,
        },
        last_compact_payoff: Default::default(),
        cost_breakdown: Default::default(),
        anomaly_correlations: Vec::new(),
        action_impacts: vec![],
        data_completeness: InsightsDataCompleteness::default(),
        timeline: vec![],
        actions: vec![
            ActionLedgerRow {
                action: "/compact".into(),
                emitted: 2,
                accepted: 1,
                improved: 1,
                ..ActionLedgerRow::default()
            },
            ActionLedgerRow {
                action: "quota-pressure: pace requests".into(),
                emitted: 3,
                no_effect: 2,
                ignored: 1,
                ..ActionLedgerRow::default()
            },
            ActionLedgerRow {
                action: "cache: avoid /compact while cache is hot".into(),
                emitted: 1,
                avoided: 1,
                ..ActionLedgerRow::default()
            },
        ],
        ignored_available: true,
    };

    let joined = format_insights_report_lines(&snapshot).join("\n");

    assert!(joined.contains("Outcome Learning"));
    assert!(joined.contains("/compact: accepted 1, improved 1, avoided 0, no_effect 0"));
    assert!(joined.contains(
        "cache: avoid /compact while cache is hot: accepted 0, improved 0, avoided 1, no_effect 0"
    ));
    assert!(joined.contains("Rule Tuning Candidates"));
    assert!(joined.contains("quota-pressure: pace requests: no_effect 2, ignored 1"));
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
        pane_buckets: vec![],
        next_best_action: NextBestActionSummary {
            data_available: true,
            action: None,
            open_proposal_count: 0,
        },
        last_compact_payoff: Default::default(),
        cost_breakdown: Default::default(),
        anomaly_correlations: Vec::new(),
        action_impacts: vec![],
        data_completeness: InsightsDataCompleteness::default(),
        timeline: vec![],
        actions: vec![ActionLedgerRow {
            action: "/compact".into(),
            hidden: 2,
            ignored: 3,
            ..ActionLedgerRow::default()
        }],
        ignored_available: true,
    };

    let lines = format_insights_report_lines(&snapshot);
    let joined = lines.join("\n");

    assert!(joined.contains("ignored classification: available"));
    assert!(joined.contains("hidden=2"));
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
    assert!(snapshot.pane_buckets.is_empty());
    assert!(snapshot.action_impacts.is_empty());
    assert_eq!(
        snapshot.data_completeness,
        InsightsDataCompleteness::default()
    );
    assert!(!snapshot.ignored_available);
}

#[test]
fn resolve_insights_paths_uses_cli_root_without_tmux() {
    let td = tempdir().unwrap();
    let root = td.path().join("missing-root");

    let (paths, source, config) = resolve_insights_paths(None, Some(&root), &[], None).unwrap();

    assert_eq!(paths.root(), root);
    assert_eq!(format!("{source:?}"), "Cli");
    assert_eq!(config.insights.ignored_ttl_secs, 30 * 60);
    assert_eq!(config.insights.default_window_secs, 24 * 60 * 60);
    assert!(!paths.root().exists());
    assert!(!paths.config_dir().exists());
    assert!(!paths.archive_dir().exists());
    assert!(!paths.snapshot_dir().exists());
    assert!(!paths.sqlite_path().exists());
}
