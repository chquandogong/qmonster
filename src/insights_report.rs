use anyhow::{Context as _, Result};

use crate::store::InsightsSnapshot;

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
    lines.push(format!(
        "  hot/cold/drift: {}/{}/{}",
        snapshot.cache.hot_count, snapshot.cache.cold_count, snapshot.cache.drift_count
    ));
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
    lines.push(if snapshot.ignored_available {
        "  lifecycle: recommendation outcomes available".into()
    } else {
        "  lifecycle: audit-only, recommendation correlation unavailable".into()
    });
    lines
}
