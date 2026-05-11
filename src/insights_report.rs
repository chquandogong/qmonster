use std::path::Path;

use anyhow::{Context as _, Result};

use crate::app::config::{QmonsterConfig, load_with_local_override};
use crate::app::path_resolution::{RootSource, default_config_path, pick_root};
use crate::app::safety_audit::apply_override_with_audit;
use crate::store::{
    CacheInsightSummary, InMemorySink, InsightsSnapshot, InsightsWindow, QmonsterPaths,
};

fn checked_seconds(value: u64, multiplier: u64, unit: &str) -> Result<u64> {
    value
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow::anyhow!("--since {unit} value is too large"))
}

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
        "m" => checked_seconds(value, 60, unit)?,
        "h" => checked_seconds(value, 60 * 60, unit)?,
        "d" => checked_seconds(value, 24 * 60 * 60, unit)?,
        _ => anyhow::bail!("--since unit must be one of s, m, h, d"),
    };
    if seconds == 0 {
        anyhow::bail!("--since must be greater than zero");
    }
    Ok(seconds)
}

pub fn resolve_insights_paths(
    config_path: Option<&Path>,
    root: Option<&Path>,
    set: &[String],
    env_root: Option<&str>,
) -> anyhow::Result<(QmonsterPaths, RootSource, QmonsterConfig)> {
    let default_path = default_config_path(root, env_root);
    let loaded_config_path = config_path.map(|path| path.to_path_buf()).or_else(|| {
        if default_path.exists() {
            Some(default_path.clone())
        } else {
            None
        }
    });
    let mut config = match loaded_config_path.as_ref() {
        Some(path) => {
            load_with_local_override(path).with_context(|| format!("loading {path:?}"))?
        }
        None => QmonsterConfig::defaults(),
    };
    let pairs = parse_set_pairs(set)?;
    if !pairs.is_empty() {
        let refs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let sink = InMemorySink::new();
        let stats = apply_override_with_audit(&mut config, &refs, &sink);
        if stats.rejected + stats.unknown > 0 {
            eprintln!(
                "qmonster: {} override(s) rejected, {} unknown key(s); see audit log",
                stats.rejected, stats.unknown
            );
        }
    }

    let resolved = pick_root(root, env_root, &config);
    let source = resolved.source.clone();
    let paths = resolved.into_paths();
    Ok((paths, source, config))
}

fn parse_set_pairs(set: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    for kv in set {
        let Some((key, value)) = kv.split_once('=') else {
            anyhow::bail!("--set expects key=value, got {kv}");
        };
        pairs.push((key.trim().into(), value.trim().into()));
    }
    Ok(pairs)
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
    lines.push("Now Suggested Action".into());
    lines.extend(format_next_best_action_lines(&snapshot.next_best_action));
    lines.push(String::new());
    lines.push("/compact Payoff".into());
    lines.push(format_last_compact_payoff_line(
        &snapshot.last_compact_payoff,
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
    lines.push(format!(
        "  hot/cold/drift: {}/{}/{}",
        snapshot.cache.hot_count, snapshot.cache.cold_count, snapshot.cache.drift_count
    ));
    lines.push(String::new());
    lines.push(format!(
        "Cost Breakdown (window total ${:.4})",
        snapshot.cost_breakdown.total_usd
    ));
    lines.push(format!(
        "  by pane: {}",
        format_cost_breakdown_rows(&snapshot.cost_breakdown.by_pane)
    ));
    lines.push(format!(
        "  by provider: {}",
        format_cost_breakdown_rows(&snapshot.cost_breakdown.by_model)
    ));
    lines.push(format!(
        "  by situation: {}",
        format_cost_breakdown_rows(&snapshot.cost_breakdown.by_situation)
    ));
    lines.push(String::new());
    lines.push("Pane Buckets".into());
    if snapshot.pane_buckets.is_empty() {
        lines.push("  none".into());
    } else {
        for bucket in snapshot.pane_buckets.iter().take(5) {
            let cache = match bucket.latest_cache_ratio {
                Some(ratio) => format!("{:.1}%", ratio * 100.0),
                None => "n/a".into(),
            };
            let growth = match bucket.token_growth {
                Some(growth) => format!("+{growth}"),
                None => "n/a".into(),
            };
            let cost = match bucket.cost_delta_usd {
                Some(cost) => format!("${cost:.4}"),
                None => "n/a".into(),
            };
            lines.push(format!(
                "  {} {} samples={} cache={} token growth={} cost delta={}",
                bucket.pane_id, bucket.provider, bucket.sample_count, cache, growth, cost
            ));
        }
        if snapshot.pane_buckets.len() > 5 {
            lines.push(format!("  ... {} more", snapshot.pane_buckets.len() - 5));
        }
    }
    lines.push(String::new());
    lines.push("Action Impact".into());
    if snapshot.action_impacts.is_empty() {
        lines.push("  none".into());
    } else {
        for impact in &snapshot.action_impacts {
            let provider = impact.provider.as_deref().unwrap_or("Unknown");
            lines.push(format!(
                "  {} {} {} {} {}",
                impact.outcome_ts_label, impact.pane_id, provider, impact.action, impact.outcome
            ));
            lines.push(format!(
                "    tokens before={} after={} delta={}",
                format_signed_i64(impact.before_token_growth),
                format_signed_i64(impact.after_token_growth),
                format_signed_i64(impact.token_growth_delta)
            ));
            lines.push(format!(
                "    cost before={} after={} delta={}",
                format_usd(impact.before_cost_delta_usd),
                format_usd(impact.after_cost_delta_usd),
                format_usd(impact.cost_delta_usd)
            ));
            lines.push(format!(
                "    cache {} -> {}",
                format_percent(impact.before_cache_ratio),
                format_percent(impact.after_cache_ratio)
            ));
        }
    }
    lines.push(String::new());
    lines.push("Data Completeness".into());
    lines.push(format!(
        "  pane buckets: {}",
        snapshot.data_completeness.pane_bucket_count
    ));
    lines.push(format!(
        "  token samples: {}",
        snapshot.data_completeness.token_sample_count
    ));
    lines.push(format!(
        "  cache ratios: {}/{} buckets",
        snapshot.data_completeness.cache_ratio_bucket_count,
        snapshot.data_completeness.pane_bucket_count
    ));
    lines.push(format!(
        "  cost deltas: {}/{} buckets",
        snapshot.data_completeness.cost_delta_bucket_count,
        snapshot.data_completeness.pane_bucket_count
    ));
    lines.push(format!(
        "  action impacts: {}",
        snapshot.data_completeness.action_impact_count
    ));
    if snapshot.data_completeness.panes.is_empty() {
        lines.push("  pane status: none".into());
    } else {
        lines.push("  pane status:".into());
        for pane in snapshot.data_completeness.panes.iter().take(8) {
            let coverage = pane
                .coverage_pct
                .map(|value| format!("{value:.1}%"))
                .unwrap_or_else(|| "n/a".into());
            let missing = pane
                .missing_polls
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".into());
            let elapsed = pane
                .elapsed_secs
                .map(|value| format!("{value}s"))
                .unwrap_or_else(|| "n/a".into());
            lines.push(format!(
                "    {} {} {} coverage {} missing {} elapsed {}",
                pane.pane_id, pane.provider, pane.status, coverage, missing, elapsed
            ));
        }
    }
    lines.push(String::new());
    lines.push("Action Ledger".into());
    if snapshot.actions.is_empty() {
        lines.push("  none".into());
    } else {
        for row in &snapshot.actions {
            let ignored = if snapshot.ignored_available {
                row.ignored.to_string()
            } else {
                "n/a".into()
            };
            lines.push(format!(
                "  {} emitted={} accepted={} rejected={} blocked={} completed={} failed={} archived={} snapshot={} hidden={} ignored={} avoided={} no_effect={} improved={}",
                row.action,
                row.emitted,
                row.accepted,
                row.rejected,
                row.blocked,
                row.completed,
                row.failed,
                row.archived,
                row.snapshot_written,
                row.hidden,
                ignored,
                row.avoided,
                row.no_effect,
                row.improved,
            ));
        }
    }
    if !snapshot.actions.is_empty() {
        let emitted: u64 = snapshot.actions.iter().map(|row| row.emitted).sum();
        let accepted: u64 = snapshot.actions.iter().map(|row| row.accepted).sum();
        let completed: u64 = snapshot.actions.iter().map(|row| row.completed).sum();
        let ignored: u64 = snapshot.actions.iter().map(|row| row.ignored).sum();
        lines.push(String::new());
        lines.push("Action Rates".into());
        lines.push(format!(
            "  accepted rate: {}",
            format_rate(accepted, emitted)
        ));
        lines.push(format!(
            "  completion rate: {}",
            format_rate(completed, accepted)
        ));
        let ignored_rate = if snapshot.ignored_available {
            format_rate(ignored, emitted)
        } else {
            "n/a".into()
        };
        lines.push(format!("  ignored rate: {ignored_rate}"));
    }
    if !snapshot.actions.is_empty() {
        lines.push(String::new());
        lines.push("Outcome Learning".into());
        for row in &snapshot.actions {
            if row.accepted == 0
                && row.improved == 0
                && row.avoided == 0
                && row.no_effect == 0
                && row.expired == 0
                && row.suppressed == 0
            {
                continue;
            }
            lines.push(format!(
                "  {}: accepted {}, improved {}, avoided {}, no_effect {}",
                row.action, row.accepted, row.improved, row.avoided, row.no_effect
            ));
        }
        lines.push(String::new());
        lines.push("Rule Tuning Candidates".into());
        let mut any_candidate = false;
        for row in &snapshot.actions {
            if row.no_effect == 0 && row.ignored == 0 && row.rejected == 0 {
                continue;
            }
            any_candidate = true;
            lines.push(format!(
                "  {}: no_effect {}, ignored {}, rejected {}",
                row.action, row.no_effect, row.ignored, row.rejected
            ));
        }
        if !any_candidate {
            lines.push("  none".into());
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
    lines.push("Cross-pane Correlations".into());
    if snapshot.anomaly_correlations.is_empty() {
        lines.push("  none".into());
    } else {
        for row in &snapshot.anomaly_correlations {
            lines.push(format!(
                "  {}  {}  {} panes",
                row.window_label, row.summary, row.pane_count
            ));
        }
    }
    lines.push(String::new());
    lines.push("Evidence".into());
    lines.push(
        "  source tables: audit_events, token_usage_samples, cost_usage_events (when present)"
            .into(),
    );
    lines.push(if snapshot.ignored_available {
        "  lifecycle: recommendation outcomes available".into()
    } else {
        "  lifecycle: audit-only, recommendation correlation unavailable".into()
    });
    lines
}

fn format_rate(numerator: u64, denominator: u64) -> String {
    if denominator == 0 {
        return "n/a".into();
    }
    format!("{:.1}%", numerator as f64 / denominator as f64 * 100.0)
}

fn format_next_best_action_lines(summary: &crate::store::NextBestActionSummary) -> Vec<String> {
    if !summary.data_available {
        return vec![format!(
            "  data unavailable · open ★p:{}",
            summary.open_proposal_count
        )];
    }
    let Some(action) = summary.action.as_ref() else {
        return vec![format!(
            "  no pending action · open ★p:{}",
            summary.open_proposal_count
        )];
    };
    let provider = action.provider.as_deref().unwrap_or("Unknown");
    let mut lines = vec![format!(
        "  {} {} {} {}: {}",
        action.pane_id, provider, action.severity, action.situation, action.reason_summary
    )];
    if let Some(command) = action.suggested_command.as_deref() {
        lines.push(format!("  run: {command}"));
    }
    lines.push(format!("  open ★p:{}", summary.open_proposal_count));
    lines
}

fn format_last_compact_payoff_line(payoff: &crate::store::LastCompactPayoffSummary) -> String {
    if payoff.status == "n/a" {
        return "  n/a (need >=6 samples and >=5% or 1K token delta)".into();
    }
    let age = payoff.age_label.as_deref().unwrap_or("age n/a");
    let token = match payoff.input_tokens_saved {
        Some(saved) if saved >= 0 => format!("saved ~{} input tokens", compact_number(saved)),
        Some(delta) => format!("regressed ~{} input tokens", compact_number(delta.abs())),
        None => "tokens n/a".into(),
    };
    let cache = match (
        payoff.before_cache_state.as_deref(),
        payoff.after_cache_state.as_deref(),
    ) {
        (Some(before), Some(after)) => format!("cache {before}->{after}"),
        _ => "cache n/a".into(),
    };
    let cost = payoff
        .cost_usd_saved
        .map(|saved| format!("${saved:.4}"))
        .unwrap_or_else(|| "$n/a".into());
    format!("  last /compact {age} · {} · {cache} · {cost}", token)
}

fn compact_number(value: i64) -> String {
    if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn format_cost_breakdown_rows(rows: &[crate::store::CostBreakdownRow]) -> String {
    if rows.is_empty() {
        return "none".into();
    }
    rows.iter()
        .take(5)
        .map(|row| format!("{} ${:.4}", row.label, row.cost_delta_usd))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn format_percent(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:.1}%", value * 100.0))
        .unwrap_or_else(|| "n/a".into())
}

fn format_signed_i64(value: Option<i64>) -> String {
    value
        .map(|value| {
            if value >= 0 {
                format!("+{value}")
            } else {
                value.to_string()
            }
        })
        .unwrap_or_else(|| "n/a".into())
}

fn format_usd(value: Option<f64>) -> String {
    value
        .map(|value| format!("${value:.4}"))
        .unwrap_or_else(|| "n/a".into())
}

pub fn empty_insights_snapshot(window: InsightsWindow) -> InsightsSnapshot {
    InsightsSnapshot {
        window,
        situations: Vec::new(),
        cache: CacheInsightSummary::default(),
        pane_buckets: Vec::new(),
        next_best_action: Default::default(),
        last_compact_payoff: Default::default(),
        cost_breakdown: Default::default(),
        anomaly_correlations: Vec::new(),
        action_impacts: Vec::new(),
        data_completeness: Default::default(),
        timeline: Vec::new(),
        actions: Vec::new(),
        ignored_available: false,
    }
}
