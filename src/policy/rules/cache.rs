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
/// Both gate on `IdentityConfidence ≥ Medium` (provider-specific
/// signal: cached_input_tokens is parsed per-provider) and suppress
/// when the operator's attention is on a more pressing prompt.
pub fn eval_cache(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    if let Some(rec) = recommend_cache_hot_compact_warning(id, signals, gates) {
        out.push(rec);
    }
    if let Some(rec) = recommend_compact_when_cache_cold(id, signals, gates) {
        out.push(rec);
    }
    out
}

const HOT_RATIO_THRESHOLD: f64 = 0.6;
const COLD_RATIO_THRESHOLD: f64 = 0.3;
const LOW_CTX_THRESHOLD: f32 = 0.7;
const HIGH_CTX_THRESHOLD: f32 = 0.6;

/// Returns `cache_hit_ratio = cached / (input + cached)` when both
/// signals are present; None otherwise. The denominator is the
/// total prompt input across cached and non-cached, matching the
/// UI's `format_cache_hit_ratio` definition (Codex's welcome-panel
/// `Token usage: total=N input=N (+ N cached) output=N`).
fn cache_hit_ratio(signals: &SignalSet) -> Option<f64> {
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

fn recommend_cache_hot_compact_warning(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    let ratio = cache_hit_ratio(signals)?;
    if ratio <= HOT_RATIO_THRESHOLD {
        return None;
    }
    let ctx = signals.context_pressure.as_ref()?.value;
    if ctx >= LOW_CTX_THRESHOLD {
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
            HOT_RATIO_THRESHOLD * 100.0,
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
    if ratio >= COLD_RATIO_THRESHOLD {
        return None;
    }
    let ctx = signals.context_pressure.as_ref()?.value;
    if ctx <= HIGH_CTX_THRESHOLD {
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
            COLD_RATIO_THRESHOLD * 100.0,
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

    #[test]
    fn cache_hot_warning_fires_when_ratio_above_threshold_and_ctx_below_threshold() {
        // ratio = 1_000_000 / (200_000 + 1_000_000) ≈ 83.3%; ctx = 50%
        let recs = eval_cache(
            &id(Provider::Codex, IdentityConfidence::High),
            &signals_with(200_000, 1_000_000, 0.50),
            &gates_high(),
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
            );
            assert!(recs.is_empty(), "expected empty for {cause:?}");
        }
    }
}
