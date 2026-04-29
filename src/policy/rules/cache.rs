use crate::domain::identity::ResolvedIdentity;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::{IdleCause, SignalSet};
use crate::policy::gates::{PolicyGates, allow_provider_specific};

/// Phase F F-7 (v1.26.0): cache-aware advisories that turn F-4's
/// `cached_input_tokens` into actionable `/compact` decisions.
///
/// Two rules fire on the same data:
///   - `recommend_cache_hot_compact_warning` — Concern, fires when
///     cache is hot AND context still has headroom. Tells the
///     operator NOT to compact yet (compact resets cache).
///   - `recommend_compact_when_cache_cold` — Good, fires when cache
///     is cold AND context is filling. Tells the operator a good
///     moment to compact (cache won't be lost).
///
/// Phase F F-7b (v1.27.0): third rule using F-3 time series:
///   - `recommend_cache_drift_compact` — Concern, fires when the
///     cache hit ratio drops ≥ 30 pp over the recent sample window.
///
/// All rules gate on `IdentityConfidence ≥ Medium` (provider-specific
/// signal: cached_input_tokens is parsed per-provider) and suppress
/// when the operator's attention is on a more pressing prompt.
pub fn eval_cache(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
    recent_token_samples: &[crate::store::TokenSample],
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    if let Some(rec) = recommend_cache_hot_compact_warning(id, signals, gates) {
        out.push(rec);
    }
    if let Some(rec) = recommend_compact_when_cache_cold(id, signals, gates) {
        out.push(rec);
    }
    if let Some(rec) = recommend_cache_drift_compact(id, signals, gates, recent_token_samples) {
        out.push(rec);
    }
    out
}

/// Returns `cache_hit_ratio = cached / (input + cached)` when both
/// signals are present; None otherwise. The denominator is the
/// total prompt input across cached and non-cached, matching the
/// UI's `format_cache_hit_ratio` definition (Codex's welcome-panel
/// `Token usage: total=N input=N (+ N cached) output=N`).
fn cache_hit_ratio(signals: &SignalSet) -> Option<f64> {
    // Phase F F-5 (v1.30.0): prefer the pre-computed ratio when the
    // adapter populated it directly (Claude statusline `cache N%`)
    // — that surface ships the percentage but not the raw cached /
    // input counts, so the count-derived path below would return
    // None for those panes and silently suppress the cache rules.
    if let Some(direct) = signals.cache_hit_ratio.as_ref() {
        return Some(direct.value);
    }
    let cached = signals.cached_input_tokens.as_ref()?.value as f64;
    let input = signals
        .input_tokens
        .as_ref()
        .map(|m| m.value as f64)
        .unwrap_or(0.0);
    let total = input + cached;
    if total <= 0.0 {
        return None;
    }
    Some(cached / total)
}

/// Cache hit ratio from a single `TokenSample`. Returns None when
/// `cached_input_tokens` is None or the total is zero. Mirrors the
/// `cache_hit_ratio(signals)` semantics but operates on persisted
/// historical data (`recent_token_samples` from F-3).
fn cache_hit_ratio_from_sample(s: &crate::store::TokenSample) -> Option<f64> {
    let cached = s.cached_input_tokens? as f64;
    let input = s.input_tokens.unwrap_or(0) as f64;
    let total = input + cached;
    if total <= 0.0 {
        return None;
    }
    Some(cached / total)
}

fn recommend_cache_hot_compact_warning(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    let ratio = cache_hit_ratio(signals)?;
    if ratio <= gates.cache_hot_ratio {
        return None;
    }
    let ctx = signals.context_pressure.as_ref()?.value;
    if ctx >= gates.cache_hot_low_ctx {
        // Context is high; the operator may need to compact regardless
        // of cache state. The "wait" advice is for when ctx still has
        // headroom AND cache is hot.
        return None;
    }
    if matches!(
        signals.idle_state,
        Some(IdleCause::InputWait) | Some(IdleCause::PermissionWait)
    ) {
        return None;
    }

    let pct = ratio * 100.0;
    Some(Recommendation {
        action: "cache: avoid /compact while cache is hot",
        reason: format!(
            "cache hit ratio {:.1}% (> {:.0}% hot threshold) and context still has headroom ({:.0}% used) — running /compact resets cache and forces full prompt rebuild on the next turn",
            pct,
            gates.cache_hot_ratio * 100.0,
            ctx * 100.0,
        ),
        severity: Severity::Concern,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: Some(
            "let context fill further before /compact; compact when ctx >= 80% so the cache rebuild cost is amortized"
                .into(),
        ),
        profile: None,
    })
}

fn recommend_compact_when_cache_cold(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    let ratio = cache_hit_ratio(signals)?;
    if ratio >= gates.cache_cold_ratio {
        return None;
    }
    let ctx = signals.context_pressure.as_ref()?.value;
    if ctx <= gates.cache_cold_high_ctx {
        // Cache is cold but context isn't filling yet — no urgency
        // to compact. Operator can keep working.
        return None;
    }
    if matches!(
        signals.idle_state,
        Some(IdleCause::InputWait) | Some(IdleCause::PermissionWait)
    ) {
        return None;
    }

    let pct = ratio * 100.0;
    Some(Recommendation {
        action: "cache: /compact is safe — cache is cold",
        reason: format!(
            "cache hit ratio {:.1}% (< {:.0}% cold threshold) and context filling ({:.0}% used) — /compact would not lose meaningful cache; cache rebuild cost is already paid on every turn",
            pct,
            gates.cache_cold_ratio * 100.0,
            ctx * 100.0,
        ),
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: Some("/compact".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: Some(
            "snapshot first via 's' key to preserve handoff state, then run /compact".into(),
        ),
        profile: None,
    })
}

fn recommend_cache_drift_compact(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
    recent_token_samples: &[crate::store::TokenSample],
) -> Option<Recommendation> {
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    if matches!(
        signals.idle_state,
        Some(IdleCause::InputWait) | Some(IdleCause::PermissionWait)
    ) {
        return None;
    }
    if recent_token_samples.len() < gates.cache_drift_min_samples {
        return None;
    }
    let current = cache_hit_ratio(signals)?;
    // recent_token_samples is newest-first (DESC). Use the oldest
    // sample in the window as the baseline.
    let oldest = recent_token_samples.last()?;
    let baseline = cache_hit_ratio_from_sample(oldest)?;
    let drop = baseline - current;
    if drop < gates.cache_drift_drop {
        return None;
    }
    let baseline_pct = baseline * 100.0;
    let current_pct = current * 100.0;
    let drop_pp = drop * 100.0;
    Some(Recommendation {
        action: "cache: drift detected — /compact will let cache rebuild",
        reason: format!(
            "cache hit ratio dropped {:.1}pp (from {:.1}% to {:.1}%) over the last {} samples, meeting the {:.1}pp drift threshold with min {} samples — context drifted; /compact will reset on a smaller stable surface and let cache rebuild quickly on the next turn",
            drop_pp,
            baseline_pct,
            current_pct,
            recent_token_samples.len(),
            gates.cache_drift_drop * 100.0,
            gates.cache_drift_min_samples,
        ),
        severity: Severity::Concern,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: Some("/compact".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: Some(
            "snapshot first via 's' key to preserve handoff state, then run /compact to rebuild cache on the trimmed surface"
                .into(),
        ),
        profile: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::signal::{MetricValue, SignalSet};
    use crate::policy::gates::PolicyGates;

    fn id(provider: Provider, conf: IdentityConfidence) -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: conf,
        }
    }

    fn gates_high() -> PolicyGates {
        PolicyGates {
            identity_confidence: IdentityConfidence::High,
            ..PolicyGates::default()
        }
    }

    fn signals_with(input: u64, cached: u64, ctx: f32) -> SignalSet {
        SignalSet {
            input_tokens: Some(MetricValue::new(input, SourceKind::ProviderOfficial)),
            cached_input_tokens: Some(MetricValue::new(cached, SourceKind::ProviderOfficial)),
            context_pressure: Some(MetricValue::new(ctx, SourceKind::ProviderOfficial)),
            ..SignalSet::default()
        }
    }

    fn signals_with_cache_pct(ratio: f64, ctx: f32) -> SignalSet {
        SignalSet {
            cache_hit_ratio: Some(MetricValue::new(ratio, SourceKind::ProviderOfficial)),
            context_pressure: Some(MetricValue::new(ctx, SourceKind::ProviderOfficial)),
            ..SignalSet::default()
        }
    }

    #[test]
    fn cache_hot_warning_fires_from_direct_cache_hit_ratio_field() {
        // Phase F F-5 (v1.30.0): when an adapter populates
        // SignalSet.cache_hit_ratio directly (Claude statusline's
        // pre-computed `cache N%`), the cache rules must use that
        // value — not silently suppress because the count-derived
        // path returns None for the missing input/cached pair.
        let recs = eval_cache(
            &id(Provider::Claude, IdentityConfidence::High),
            &signals_with_cache_pct(0.83, 0.50),
            &gates_high(),
            &[],
        );
        assert!(
            recs.iter()
                .any(|r| r.action == "cache: avoid /compact while cache is hot"),
            "Claude statusline cache 83% must surface the hot-cache warning"
        );
    }

    #[test]
    fn cache_hot_warning_fires_when_ratio_above_threshold_and_ctx_below_threshold() {
        // ratio = 1_000_000 / (200_000 + 1_000_000) ≈ 83.3%; ctx = 50%
        let recs = eval_cache(
            &id(Provider::Codex, IdentityConfidence::High),
            &signals_with(200_000, 1_000_000, 0.50),
            &gates_high(),
            &[],
        );
        // Both rules consider the same data; only the hot rule should
        // fire here (ratio > 0.6 AND ctx < 0.7).
        assert_eq!(recs.len(), 1, "expected only hot warning; got: {recs:#?}");
        let rec = &recs[0];
        assert_eq!(rec.severity, Severity::Concern);
        assert_eq!(rec.source_kind, SourceKind::ProjectCanonical);
        assert!(rec.reason.contains("83.3%"));
        assert!(rec.next_step.as_deref().unwrap().contains("80%"));
    }

    #[test]
    fn cache_cold_compact_fires_when_ratio_below_threshold_and_ctx_above_threshold() {
        // ratio = 100_000 / (1_000_000 + 100_000) ≈ 9.1%; ctx = 75%
        let recs = eval_cache(
            &id(Provider::Codex, IdentityConfidence::High),
            &signals_with(1_000_000, 100_000, 0.75),
            &gates_high(),
            &[],
        );
        assert_eq!(recs.len(), 1);
        let rec = &recs[0];
        assert_eq!(rec.severity, Severity::Good);
        assert_eq!(rec.source_kind, SourceKind::ProjectCanonical);
        assert!(rec.reason.contains("9.1%"));
        assert_eq!(rec.suggested_command.as_deref(), Some("/compact"));
        assert!(rec.next_step.as_deref().unwrap().contains("snapshot"));
    }

    #[test]
    fn cache_hot_warning_suppressed_when_ctx_at_or_above_threshold() {
        // ratio = 83.3% (hot), ctx = 70% (== LOW_CTX_THRESHOLD)
        let recs = eval_cache(
            &id(Provider::Codex, IdentityConfidence::High),
            &signals_with(200_000, 1_000_000, 0.70),
            &gates_high(),
            &[],
        );
        assert!(recs.is_empty());
    }

    #[test]
    fn cache_cold_compact_suppressed_when_ctx_at_or_below_threshold() {
        // ratio = 9.1% (cold), ctx = 60% (== HIGH_CTX_THRESHOLD)
        let recs = eval_cache(
            &id(Provider::Codex, IdentityConfidence::High),
            &signals_with(1_000_000, 100_000, 0.60),
            &gates_high(),
            &[],
        );
        assert!(recs.is_empty());
    }

    #[test]
    fn neither_rule_fires_in_the_intermediate_band() {
        // ratio = 45% (between cold 30% and hot 60%); ctx 50%
        let recs = eval_cache(
            &id(Provider::Codex, IdentityConfidence::High),
            &signals_with(550_000, 450_000, 0.50),
            &gates_high(),
            &[],
        );
        assert!(recs.is_empty());
    }

    #[test]
    fn both_rules_suppressed_when_cached_input_tokens_is_none() {
        let s = SignalSet {
            input_tokens: Some(MetricValue::new(1000, SourceKind::ProviderOfficial)),
            cached_input_tokens: None,
            context_pressure: Some(MetricValue::new(0.50, SourceKind::ProviderOfficial)),
            ..SignalSet::default()
        };
        let recs = eval_cache(
            &id(Provider::Codex, IdentityConfidence::High),
            &s,
            &gates_high(),
            &[],
        );
        assert!(recs.is_empty());
    }

    #[test]
    fn both_rules_suppressed_on_low_or_unknown_confidence() {
        for conf in [IdentityConfidence::Low, IdentityConfidence::Unknown] {
            let recs = eval_cache(
                &id(Provider::Codex, conf),
                &signals_with(200_000, 1_000_000, 0.50),
                &PolicyGates {
                    identity_confidence: conf,
                    ..PolicyGates::default()
                },
                &[],
            );
            assert!(recs.is_empty(), "expected empty for {conf:?}");
        }
    }

    #[test]
    fn both_rules_suppressed_when_input_or_permission_wait_active() {
        for cause in [IdleCause::InputWait, IdleCause::PermissionWait] {
            let mut s = signals_with(200_000, 1_000_000, 0.50);
            s.idle_state = Some(cause);
            let recs = eval_cache(
                &id(Provider::Codex, IdentityConfidence::High),
                &s,
                &gates_high(),
                &[],
            );
            assert!(recs.is_empty(), "expected empty for {cause:?}");
        }
    }

    #[test]
    fn cache_drift_fires_when_ratio_drops_30pp_or_more_over_4_samples() {
        use crate::domain::identity::Provider as P;
        use crate::store::TokenSample;
        // current ratio ~ 30% (300_000 / (700_000 + 300_000))
        let signals = signals_with(700_000, 300_000, 0.50);
        // 4 samples DESC; oldest has ratio 80% (800_000 / (200_000 + 800_000))
        let samples = vec![
            // newest first; ratio ~ 30%, 35%, 50%, 80%
            TokenSample {
                ts_unix_ms: 4000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(700_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(300_000),
            },
            TokenSample {
                ts_unix_ms: 3000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(650_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(350_000),
            },
            TokenSample {
                ts_unix_ms: 2000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(500_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(500_000),
            },
            TokenSample {
                ts_unix_ms: 1000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(200_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(800_000),
            },
        ];
        let recs = eval_cache(
            &id(P::Codex, IdentityConfidence::High),
            &signals,
            &gates_high(),
            &samples,
        );
        let drift = recs
            .iter()
            .find(|r| r.action.starts_with("cache: drift detected"))
            .expect("drift rule should fire");
        assert_eq!(drift.severity, Severity::Concern);
        assert_eq!(drift.suggested_command.as_deref(), Some("/compact"));
        // Drop = 80% - 30% = 50 pp
        assert!(drift.reason.contains("50.0pp"));
        assert!(drift.reason.contains("80.0%"));
        assert!(drift.reason.contains("30.0%"));
    }

    #[test]
    fn cache_drift_suppressed_when_drop_below_30pp() {
        use crate::domain::identity::Provider as P;
        use crate::store::TokenSample;
        // current 50%, oldest 70% → drop = 20pp (< 30pp)
        let signals = signals_with(500_000, 500_000, 0.50);
        let samples = vec![
            TokenSample {
                ts_unix_ms: 4000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(500_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(500_000),
            },
            TokenSample {
                ts_unix_ms: 3000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(450_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(550_000),
            },
            TokenSample {
                ts_unix_ms: 2000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(400_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(600_000),
            },
            TokenSample {
                ts_unix_ms: 1000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(300_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(700_000),
            },
        ];
        let recs = eval_cache(
            &id(P::Codex, IdentityConfidence::High),
            &signals,
            &gates_high(),
            &samples,
        );
        assert!(
            !recs
                .iter()
                .any(|r| r.action.starts_with("cache: drift detected")),
            "drift rule should not fire for 20pp drop"
        );
    }

    #[test]
    fn cache_drift_suppressed_when_fewer_than_4_samples() {
        use crate::domain::identity::Provider as P;
        use crate::store::TokenSample;
        let signals = signals_with(700_000, 300_000, 0.50);
        // Only 3 samples — below DRIFT_MIN_SAMPLES
        let samples = vec![
            TokenSample {
                ts_unix_ms: 3000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(700_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(300_000),
            },
            TokenSample {
                ts_unix_ms: 2000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(500_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(500_000),
            },
            TokenSample {
                ts_unix_ms: 1000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(200_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(800_000),
            },
        ];
        let recs = eval_cache(
            &id(P::Codex, IdentityConfidence::High),
            &signals,
            &gates_high(),
            &samples,
        );
        assert!(
            !recs
                .iter()
                .any(|r| r.action.starts_with("cache: drift detected"))
        );
    }

    #[test]
    fn cache_rules_honor_custom_thresholds_from_gates() {
        use crate::domain::identity::Provider;
        // With a stricter hot_ratio threshold (0.4 instead of 0.6), a
        // ratio of 0.50 should trip the hot warning even though it's
        // below the default 0.6 threshold.
        let mut gates = gates_high();
        gates.cache_hot_ratio = 0.40;

        // ratio = 500_000 / (500_000 + 500_000) = 50%; ctx = 50%
        let signals = signals_with(500_000, 500_000, 0.50);
        let recs = eval_cache(
            &id(Provider::Codex, IdentityConfidence::High),
            &signals,
            &gates,
            &[],
        );
        let hot = recs
            .iter()
            .find(|r| r.action.starts_with("cache: avoid /compact"))
            .expect("with cache_hot_ratio=0.40, ratio=0.50 should fire");
        assert_eq!(hot.severity, Severity::Concern);
        assert!(
            hot.reason.contains("40%") || hot.reason.contains("40.0%"),
            "reason should reference the configured 40% threshold; got: {}",
            hot.reason
        );
    }

    #[test]
    fn cache_drift_honors_custom_thresholds_from_gates() {
        use crate::domain::identity::Provider as P;
        use crate::store::TokenSample;
        let mut gates = gates_high();
        gates.cache_drift_drop = 0.20;
        gates.cache_drift_min_samples = 3;

        // current 55%, oldest 80% -> drop = 25pp. This is below the
        // default 30pp threshold and below the default 4-sample minimum,
        // so it only fires when both custom gates are honored.
        let signals = signals_with(450_000, 550_000, 0.50);
        let samples = vec![
            TokenSample {
                ts_unix_ms: 3000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(450_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(550_000),
            },
            TokenSample {
                ts_unix_ms: 2000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(350_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(650_000),
            },
            TokenSample {
                ts_unix_ms: 1000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(200_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(800_000),
            },
        ];
        let recs = eval_cache(
            &id(P::Codex, IdentityConfidence::High),
            &signals,
            &gates,
            &samples,
        );
        let drift = recs
            .iter()
            .find(|r| r.action.starts_with("cache: drift detected"))
            .expect("custom drift threshold and min-sample gates should fire");
        assert!(drift.reason.contains("25.0pp"));
        assert!(drift.reason.contains("20.0pp drift threshold"));
        assert!(drift.reason.contains("min 3 samples"));
    }

    #[test]
    fn cache_drift_suppressed_on_low_confidence() {
        use crate::domain::identity::Provider as P;
        use crate::store::TokenSample;
        let signals = signals_with(700_000, 300_000, 0.50);
        let samples = vec![
            TokenSample {
                ts_unix_ms: 4000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(700_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(300_000),
            },
            TokenSample {
                ts_unix_ms: 3000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(650_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(350_000),
            },
            TokenSample {
                ts_unix_ms: 2000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(500_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(500_000),
            },
            TokenSample {
                ts_unix_ms: 1000,
                pane_id: "%1".into(),
                provider: P::Codex,
                input_tokens: Some(200_000),
                output_tokens: None,
                cost_usd: None,
                cached_input_tokens: Some(800_000),
            },
        ];
        let gates_low = PolicyGates {
            identity_confidence: IdentityConfidence::Low,
            ..PolicyGates::default()
        };
        let recs = eval_cache(
            &id(P::Codex, IdentityConfidence::Low),
            &signals,
            &gates_low,
            &samples,
        );
        assert!(recs.is_empty());
    }
}
