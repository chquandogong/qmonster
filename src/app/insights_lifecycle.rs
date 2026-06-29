use std::collections::BTreeSet;

use crate::app::config::{QmonsterConfig, QuotaWindow};
use crate::app::event_loop::PaneReport;
use crate::domain::identity::{Provider, Role};
use crate::domain::recommendation::{Recommendation, RequestedEffect};
use crate::store::{
    RecommendationEventRecord, RecommendationOutcome, RecommendationOutcomeRecord,
    SqliteRecommendationLifecycleSink,
};

pub fn record_recommendation_events(
    sink: Option<&SqliteRecommendationLifecycleSink>,
    config: &QmonsterConfig,
    pane_id: &str,
    provider: Provider,
    role: Role,
    recommendations: &[Recommendation],
    effects: &[RequestedEffect],
) {
    let Some(sink) = sink else {
        return;
    };
    let threshold_snapshot_json = threshold_snapshot_json(config, provider);
    for rec in recommendations {
        let proposal = prompt_proposal_for_rec(pane_id, rec, effects);
        let action = proposal
            .map(|(slash_command, _)| slash_command.to_string())
            .unwrap_or_else(|| rec.action.to_string());
        let dedup_key = proposal
            .map(|(_, proposal_id)| proposal_id.to_string())
            .unwrap_or_else(|| {
                format!(
                    "{pane_id}:{}:{}:{}",
                    rec.action,
                    rec.severity.letter(),
                    rec.reason
                )
            });
        let event = RecommendationEventRecord {
            ts_unix_ms: current_unix_ms(),
            pane_id: pane_id.to_string(),
            provider: Some(format!("{provider:?}")),
            role: Some(format!("{role:?}")),
            situation: situation_for_action(rec.action).to_string(),
            action,
            severity: rec.severity.label().to_string(),
            source_kind: rec.source_kind.to_string(),
            reason_summary: rec.reason.clone(),
            suggested_command: rec.suggested_command.clone(),
            is_strong: rec.is_strong,
            dedup_key,
            threshold_snapshot_json: threshold_snapshot_json.clone(),
        };
        if let Err(e) = sink.insert_recommendation_event(&event) {
            eprintln!(
                "recommendation lifecycle: event insert failed (pane={pane_id}, action={}): {e}",
                rec.action
            );
        }
    }
}

pub fn record_recommendation_outcome(
    sink: Option<&SqliteRecommendationLifecycleSink>,
    pane_id: &str,
    action: &str,
    outcome: RecommendationOutcome,
    summary: impl Into<String>,
) {
    let Some(sink) = sink else {
        return;
    };
    let record = RecommendationOutcomeRecord {
        ts_unix_ms: current_unix_ms(),
        recommendation_event_id: None,
        pane_id: pane_id.to_string(),
        action: action.to_string(),
        outcome,
        audit_event_id: None,
        summary: summary.into(),
    };
    if let Err(e) = sink.insert_correlated_recommendation_outcome(&record) {
        eprintln!(
            "recommendation lifecycle: outcome insert failed (pane={pane_id}, action={action}, outcome={}): {e}",
            outcome.label()
        );
    }
}

pub fn record_hidden_alert_outcomes(
    sink: Option<&SqliteRecommendationLifecycleSink>,
    reports: &[PaneReport],
    alert_keys: impl IntoIterator<Item = String>,
) {
    let Some(sink) = sink else {
        return;
    };
    let mut seen = BTreeSet::new();
    for key in alert_keys {
        for (pane_id, action) in lifecycle_targets_for_alert_key(&key, reports) {
            if !seen.insert((pane_id.clone(), action.clone())) {
                continue;
            }
            record_recommendation_outcome(
                Some(sink),
                &pane_id,
                &action,
                RecommendationOutcome::Hidden,
                "alert hidden by operator",
            );
        }
    }
}

pub fn record_operator_snapshot_outcomes(
    sink: Option<&SqliteRecommendationLifecycleSink>,
    reports: &[PaneReport],
    summary: impl Into<String>,
) {
    let Some(sink) = sink else {
        return;
    };
    let summary = summary.into();
    let mut seen = BTreeSet::new();
    for report in reports {
        for rec in &report.recommendations {
            if !rec.action.starts_with("snapshot before") {
                continue;
            }
            if !seen.insert((report.pane_id.clone(), rec.action.to_string())) {
                continue;
            }
            record_recommendation_outcome(
                Some(sink),
                &report.pane_id,
                rec.action,
                RecommendationOutcome::SnapshotWritten,
                summary.clone(),
            );
        }
    }
    if seen.is_empty() {
        record_recommendation_outcome(
            Some(sink),
            "n/a",
            "snapshot",
            RecommendationOutcome::SnapshotWritten,
            summary,
        );
    }
}

pub fn lifecycle_action_for_rec(
    pane_id: &str,
    rec: &Recommendation,
    effects: &[RequestedEffect],
) -> String {
    prompt_proposal_for_rec(pane_id, rec, effects)
        .map(|(slash_command, _)| slash_command.to_string())
        .unwrap_or_else(|| rec.action.to_string())
}

fn prompt_proposal_for_rec<'a>(
    pane_id: &str,
    rec: &Recommendation,
    effects: &'a [RequestedEffect],
) -> Option<(&'a str, &'a str)> {
    if !rec.is_strong {
        return None;
    }
    let suggested = rec.suggested_command.as_deref()?;
    effects.iter().find_map(|effect| match effect {
        RequestedEffect::PromptSendProposed {
            target_pane_id,
            slash_command,
            proposal_id,
        } if target_pane_id == pane_id && slash_command == suggested => {
            Some((slash_command.as_str(), proposal_id.as_str()))
        }
        _ => None,
    })
}

fn lifecycle_target_for_recommendation_key(
    key: &str,
    reports: &[PaneReport],
) -> Option<(String, String)> {
    let mut parts = key.splitn(5, '|');
    let kind = parts.next()?;
    if kind != "rec" {
        return None;
    }
    let pane_id = parts.next()?.to_string();
    let action = parts.next()?.to_string();
    let severity = parts.next()?;
    let reason = parts.next()?;
    let Some(report) = reports.iter().find(|report| report.pane_id == pane_id) else {
        return Some((pane_id, action));
    };
    let Some(rec) = report.recommendations.iter().find(|rec| {
        rec.action == action && rec.severity.letter() == severity && rec.reason == reason
    }) else {
        return Some((pane_id, action));
    };
    Some((
        pane_id,
        lifecycle_action_for_rec(&report.pane_id, rec, &report.effects),
    ))
}

fn lifecycle_targets_for_alert_key(key: &str, reports: &[PaneReport]) -> Vec<(String, String)> {
    if let Some(target) = lifecycle_target_for_recommendation_key(key, reports) {
        return vec![target];
    }

    let Some(rest) = key.strip_prefix("flow|context-recovery|") else {
        return Vec::new();
    };
    let Some((_, included_keys)) = rest.split_once('|') else {
        return Vec::new();
    };
    included_keys
        .split('\u{1f}')
        .filter_map(|included_key| lifecycle_target_for_recommendation_key(included_key, reports))
        .collect()
}

fn threshold_snapshot_json(config: &QmonsterConfig, provider: Provider) -> Option<String> {
    serde_json::to_string(&serde_json::json!({
        "context": {
            "warning": config.context.warning_for(provider),
            "critical": config.context.critical_for(provider),
        },
        "quota": {
            "warning": config.quota.warning_for(provider),
            "critical": config.quota.critical_for(provider),
            "five_hour_warning": config.quota.warning_for_window(provider, QuotaWindow::FiveHour),
            "five_hour_critical": config.quota.critical_for_window(provider, QuotaWindow::FiveHour),
            "weekly_warning": config.quota.warning_for_window(provider, QuotaWindow::Weekly),
            "weekly_critical": config.quota.critical_for_window(provider, QuotaWindow::Weekly),
        },
        "cache": {
            "hot_ratio": config.cache.hot_ratio_threshold,
            "cold_ratio": config.cache.cold_ratio_threshold,
            "hot_low_context": config.cache.hot_low_ctx_threshold,
            "cold_high_context": config.cache.cold_high_ctx_threshold,
            "drift_drop": config.cache.drift_drop_threshold,
            "drift_min_samples": config.cache.drift_min_samples,
        },
        "reset": {
            "wait_pressure": config.reset.wait_pressure_threshold,
            "wait_eta_secs": config.reset.wait_eta_secs,
            "snapshot_pressure": config.reset.snapshot_pressure_threshold,
            "snapshot_eta_secs": config.reset.snapshot_eta_secs,
            "auto_snapshot": config.reset.auto_snapshot,
        },
        "cost": {
            "warning_usd": config.cost.warning_for(provider),
            "critical_usd": config.cost.critical_for(provider),
            "budget_usd": config.cost.budget_usd,
        },
        "insights": {
            "ignored_ttl_secs": config.insights.ignored_ttl_secs,
            "default_window_secs": config.insights.default_window_secs,
        }
    }))
    .ok()
}

fn situation_for_action(action: &str) -> &'static str {
    // Legacy labels remain for already-written audit/lifecycle rows; current
    // policy emissions use newer anomaly names.
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

fn current_unix_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;
    use crate::domain::signal::SignalSet;
    use crate::store::{
        InsightsWindow, RecommendationEventRecord, SqliteInsightsStore,
        SqliteRecommendationLifecycleSink,
    };
    use std::time::Instant;

    #[test]
    fn strong_prompt_rec_uses_slash_command_as_lifecycle_action() {
        let rec = Recommendation {
            action: "context-pressure: act now",
            reason: "context is high".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/compact".into()),
            side_effects: vec![],
            is_strong: true,
            next_step: None,
        };
        let effects = vec![RequestedEffect::PromptSendProposed {
            target_pane_id: "%1".into(),
            slash_command: "/compact".into(),
            proposal_id: "%1:/compact".into(),
        }];

        assert_eq!(lifecycle_action_for_rec("%1", &rec, &effects), "/compact");
    }

    #[test]
    fn operator_snapshot_outcome_matches_reset_recommendation_action() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("qmonster.db");
        let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();
        lifecycle
            .insert_recommendation_event(&RecommendationEventRecord {
                ts_unix_ms: 1_000,
                pane_id: "%1".into(),
                provider: Some("Claude".into()),
                role: Some("Main".into()),
                situation: "Context pressure".into(),
                action: "snapshot before 5h window resets".into(),
                severity: "good".into(),
                source_kind: "ProjectCanonical".into(),
                reason_summary: "reset soon".into(),
                suggested_command: None,
                is_strong: false,
                dedup_key: "%1:snapshot-before-5h".into(),
                threshold_snapshot_json: None,
            })
            .unwrap();
        let reports = vec![PaneReport {
            pane_id: "%1".into(),
            session_name: "qwork".into(),
            window_index: "1".into(),
            provider: Provider::Claude,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Claude,
                    instance: 1,
                    role: Role::Main,
                    pane_id: "%1".into(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations: vec![Recommendation {
                action: "snapshot before 5h window resets",
                reason: "reset soon".into(),
                severity: Severity::Good,
                source_kind: SourceKind::ProjectCanonical,
                suggested_command: None,
                side_effects: vec![],
                is_strong: false,
                next_step: None,
            }],
            effects: Vec::new(),
            dead: false,
            current_path: String::new(),
            worktree_role: None,
            current_command: String::new(),
            cross_pane_findings: Vec::new(),
            idle_state: None,
            idle_state_entered_at: Some(Instant::now()),
            recent_token_samples: Vec::new(),
            anomalies: Vec::new(),
        }];

        record_operator_snapshot_outcomes(Some(&lifecycle), &reports, "operator snapshot test");

        let snapshot = SqliteInsightsStore::open(&db_path)
            .unwrap()
            .snapshot_with_ignored_ttl(
                InsightsWindow {
                    since_ms: 0,
                    until_ms: current_unix_ms().saturating_add(1_000),
                },
                0,
            )
            .unwrap();
        let row = snapshot
            .actions
            .iter()
            .find(|row| row.action == "snapshot before 5h window resets")
            .unwrap();
        assert_eq!(row.emitted, 1);
        assert_eq!(row.snapshot_written, 1);
        assert_eq!(row.ignored, 0);
        assert!(snapshot.actions.iter().all(|row| row.action != "snapshot"));
    }

    #[test]
    fn hidden_flow_key_records_hidden_outcomes_for_included_recommendations() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("qmonster.db");
        let lifecycle = SqliteRecommendationLifecycleSink::open(&db_path).unwrap();
        let context = Recommendation {
            action: "context-pressure: checkpoint",
            reason: "context near warning threshold".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/compact".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
        };
        let cache = Recommendation {
            action: "cache: drift detected — /compact will let cache rebuild",
            reason: "cache hit ratio dropped from 0.72 to 0.38".into(),
            severity: Severity::Concern,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
        };
        for (idx, rec) in [context.clone(), cache.clone()].into_iter().enumerate() {
            lifecycle
                .insert_recommendation_event(&RecommendationEventRecord {
                    ts_unix_ms: 1_000 + idx as i64,
                    pane_id: "%1".into(),
                    provider: Some("Claude".into()),
                    role: Some("Main".into()),
                    situation: "Context pressure".into(),
                    action: rec.action.to_string(),
                    severity: rec.severity.label().into(),
                    source_kind: rec.source_kind.to_string(),
                    reason_summary: rec.reason.clone(),
                    suggested_command: rec.suggested_command.clone(),
                    is_strong: rec.is_strong,
                    dedup_key: format!("%1:{}:{}", rec.action, idx),
                    threshold_snapshot_json: None,
                })
                .unwrap();
        }
        let reports = vec![PaneReport {
            pane_id: "%1".into(),
            session_name: "qwork".into(),
            window_index: "1".into(),
            provider: Provider::Claude,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Claude,
                    instance: 1,
                    role: Role::Main,
                    pane_id: "%1".into(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations: vec![context.clone(), cache.clone()],
            effects: Vec::new(),
            dead: false,
            current_path: String::new(),
            worktree_role: None,
            current_command: String::new(),
            cross_pane_findings: Vec::new(),
            idle_state: None,
            idle_state_entered_at: Some(Instant::now()),
            recent_token_samples: Vec::new(),
            anomalies: Vec::new(),
        }];
        let flow_key = format!(
            "flow|context-recovery|%1|{}",
            [
                format!(
                    "rec|%1|{}|{}|{}",
                    cache.action,
                    cache.severity.letter(),
                    cache.reason
                ),
                format!(
                    "rec|%1|{}|{}|{}",
                    context.action,
                    context.severity.letter(),
                    context.reason
                ),
            ]
            .join("\u{1f}")
        );

        record_hidden_alert_outcomes(Some(&lifecycle), &reports, [flow_key]);

        let snapshot = SqliteInsightsStore::open(&db_path)
            .unwrap()
            .snapshot_with_ignored_ttl(
                InsightsWindow {
                    since_ms: 0,
                    until_ms: current_unix_ms().saturating_add(1_000),
                },
                0,
            )
            .unwrap();
        let context_row = snapshot
            .actions
            .iter()
            .find(|row| row.action == context.action)
            .expect("context row should exist");
        let cache_row = snapshot
            .actions
            .iter()
            .find(|row| row.action == cache.action)
            .expect("cache row should exist");
        assert_eq!(context_row.hidden, 1);
        assert_eq!(cache_row.hidden, 1);
    }
}
