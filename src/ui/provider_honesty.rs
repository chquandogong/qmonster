use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::SignalSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CacheMetricStatus {
    Value { ratio: f64, source_kind: SourceKind },
    Pending,
    Unsupported,
    Hidden,
}

pub(crate) fn cache_hit_ratio_from_counts(
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
) -> Option<f64> {
    let cached = cached_input_tokens?;
    let input = input_tokens.unwrap_or(0);
    let total = input.saturating_add(cached);
    if total == 0 {
        return None;
    }
    Some(cached as f64 / total as f64)
}

pub(crate) fn cache_hit_ratio_with_source(s: &SignalSet) -> Option<(f64, SourceKind)> {
    if let Some(direct) = s.cache_hit_ratio.as_ref() {
        return Some((direct.value, direct.source_kind));
    }
    let ratio = cache_hit_ratio_from_counts(
        s.input_tokens.as_ref().map(|m| m.value),
        s.cached_input_tokens.as_ref().map(|m| m.value),
    )?;
    let source = s
        .cached_input_tokens
        .as_ref()
        .map(|m| m.source_kind)
        .unwrap_or(SourceKind::Heuristic);
    Some((ratio, source))
}

pub(crate) fn cache_metric_status(s: &SignalSet, provider: Provider) -> CacheMetricStatus {
    if let Some((ratio, source_kind)) = cache_hit_ratio_with_source(s) {
        return CacheMetricStatus::Value { ratio, source_kind };
    }

    match provider {
        Provider::Claude | Provider::Codex => CacheMetricStatus::Pending,
        Provider::Gemini => {
            if s.input_tokens.is_some() || s.output_tokens.is_some() {
                CacheMetricStatus::Unsupported
            } else {
                CacheMetricStatus::Pending
            }
        }
        Provider::Antigravity | Provider::Qmonster | Provider::Unknown => CacheMetricStatus::Hidden,
    }
}

pub(crate) fn format_ratio_pct(ratio: f64) -> String {
    format!("{:.1}%", ratio * 100.0)
}

/// v1.58.0: provider-honesty for the COST chip.
///
/// Mirrors `CacheMetricStatus` but specialised for `cost_usd`. Cost is
/// derived as `(input_tokens × in_rate + output_tokens × out_rate) /
/// 1M` whenever `pricing.lookup(provider, model)` returns Some and
/// both token counts are present (see `adapters/codex.rs:118-133`,
/// `adapters/gemini.rs` cost activation, etc). When `cost_usd` is
/// `None` the previous UI silently dropped the chip — operators on
/// Gemini panes especially couldn't tell whether their pane was just
/// quiet or whether they had forgotten to curate `pricing.toml` for
/// the model the pane was running.
///
/// Rules:
/// - Always Value when `cost_usd` is `Some`.
/// - Otherwise:
///   - Qmonster / Unknown → `Hidden` (monitor/ambiguous panes have
///     no meaningful cost).
///   - Any other provider with at least some token activity (input
///     or output tokens) → `Pending` — the pricing table is the
///     missing input; encourage the operator to add it.
///   - No token activity yet → `Hidden` — the pane is fresh; showing
///     a cost placeholder would be noise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CostMetricStatus {
    Value { usd: f64, source_kind: SourceKind },
    Pending,
    Hidden,
}

pub(crate) fn cost_metric_status(s: &SignalSet, provider: Provider) -> CostMetricStatus {
    if let Some(metric) = s.cost_usd.as_ref() {
        return CostMetricStatus::Value {
            usd: metric.value,
            source_kind: metric.source_kind,
        };
    }
    match provider {
        Provider::Antigravity | Provider::Qmonster | Provider::Unknown => CostMetricStatus::Hidden,
        Provider::Claude | Provider::Codex | Provider::Gemini => {
            if s.input_tokens.is_some() || s.output_tokens.is_some() {
                CostMetricStatus::Pending
            } else {
                CostMetricStatus::Hidden
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::MetricValue;

    #[test]
    fn cache_hit_ratio_uses_cached_over_input_plus_cached() {
        assert_eq!(cache_hit_ratio_from_counts(Some(200), Some(800)), Some(0.8));
    }

    #[test]
    fn gemini_with_stats_but_no_cache_reads_is_structurally_unsupported() {
        let signals = SignalSet {
            input_tokens: Some(MetricValue::new(100_u64, SourceKind::ProviderOfficial)),
            output_tokens: Some(MetricValue::new(10_u64, SourceKind::ProviderOfficial)),
            ..SignalSet::default()
        };

        assert_eq!(
            cache_metric_status(&signals, Provider::Gemini),
            CacheMetricStatus::Unsupported
        );
    }

    #[test]
    fn gemini_without_stats_is_pending_not_structurally_denied() {
        assert_eq!(
            cache_metric_status(&SignalSet::default(), Provider::Gemini),
            CacheMetricStatus::Pending
        );
    }
}
