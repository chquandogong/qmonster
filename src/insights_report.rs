use std::path::Path;

use anyhow::{Context as _, Result};

use crate::app::config::{QmonsterConfig, load_with_local_override};
use crate::app::path_resolution::{RootSource, default_config_path, pick_root};
use crate::app::safety_audit::apply_override_with_audit;
use crate::store::{InMemorySink, InsightsSnapshot, QmonsterPaths};

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
) -> anyhow::Result<(QmonsterPaths, RootSource)> {
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
    Ok((paths, source))
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
            let ignored = if snapshot.ignored_available {
                row.ignored.to_string()
            } else {
                "n/a".into()
            };
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
                ignored,
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
