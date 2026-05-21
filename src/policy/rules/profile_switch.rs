//! Phase F F-9 (dynamic profile switching): error-rate-aware
//! profile-switch recommender.
//!
//! Watches per-pane `error_hint` observations over a sliding window
//! (default 10 polls) and recommends switching the active provider
//! profile when the error rate exceeds `gates.profile_switch_error_rate`
//! (default 50%). The recommendation is advisory: it tells the
//! operator the pane is churning on errors and suggests handing off
//! to a more conservative profile (the existing `*-script-low-token`
//! aggressive variants, or a review-tier handoff).
//!
//! Pure function — the caller (event loop) owns the per-pane error
//! ring buffer. Mirrors the `recent_token_samples` pattern that
//! `policy::rules::cache::recommend_cache_drift_compact` uses for
//! cache-drift detection so a future "metrics-time-series" abstraction
//! can collapse both into one helper.
//!
//! Gated behind `gates.profile_switch_enabled`. Default off because
//! short bursts of errors are normal for experimental work; the
//! recommendation only adds value once the operator has decided
//! they want chronic-error nudges.

use crate::domain::identity::{Provider, Role};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::{IdleCause, SignalSet};
use crate::policy::gates::{PolicyGates, allow_provider_specific};

/// Evaluate the per-pane profile-switch recommendation. Returns at
/// most one `Recommendation`. Pure function — `recent_errors` is the
/// most-recent-first slice of `error_hint` observations the caller
/// collected from past polls (the rule itself does no IO).
///
/// Eligibility:
///   - `gates.profile_switch_enabled` is `true`
///   - `gates.identity_confidence` is `High` or `Medium`
///   - pane is `Role::Main` (the only role with main-pane profiles)
///   - pane is NOT in `IdleCause::InputWait` / `PermissionWait`
///   - `recent_errors.len() >= gates.profile_switch_window`
///   - error rate over the most recent `window` samples is
///     `>= gates.profile_switch_error_rate`
///   - `gates.quota_tight` is currently `false` — once the operator
///     is already on the aggressive variant, the recommender has
///     nothing more aggressive to suggest, so it stays silent rather
///     than emit a no-op nudge.
pub fn eval_profile_switch(
    id: &crate::domain::identity::ResolvedIdentity,
    signals: &SignalSet,
    recent_errors: &[bool],
    gates: &PolicyGates,
) -> Vec<Recommendation> {
    if !gates.profile_switch_enabled {
        return Vec::new();
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return Vec::new();
    }
    if id.identity.role != Role::Main {
        return Vec::new();
    }
    if matches!(
        signals.idle_state,
        Some(IdleCause::InputWait) | Some(IdleCause::PermissionWait)
    ) {
        return Vec::new();
    }
    if gates.quota_tight {
        // Operator is already on the aggressive `*-script-low-token`
        // variant — no more conservative profile to escalate to.
        return Vec::new();
    }
    let window = gates.profile_switch_window;
    if window == 0 || recent_errors.len() < window {
        return Vec::new();
    }
    // Use the freshest `window` samples; the caller keeps the buffer
    // in most-recent-first order so the head slice is the right window.
    let window_slice = &recent_errors[..window];
    let error_count = window_slice.iter().filter(|b| **b).count();
    let rate = error_count as f32 / window as f32;
    if rate < gates.profile_switch_error_rate {
        return Vec::new();
    }
    let Some((current_profile, suggested_profile)) =
        profile_targets_for_provider(id.identity.provider)
    else {
        return Vec::new();
    };
    let pane_id = &id.identity.pane_id;
    let threshold_pct = (gates.profile_switch_error_rate * 100.0).round() as u32;
    let observed_pct = (rate * 100.0).round() as u32;
    let reason = format!(
        "pane {pane_id} error_hint observed in {error_count}/{window} recent polls ({observed_pct}% ≥ {threshold_pct}% threshold) — consider switching from `{current_profile}` to `{suggested_profile}` to reduce churn",
    );
    let next_step = format!(
        "set `[token] quota_tight = true` in qmonster.toml (or restart with `--set token.quota_tight=true`) to switch this pane onto `{suggested_profile}`",
    );
    Vec::from([Recommendation {
        action: "profile-switch: elevated error rate",
        reason,
        severity: Severity::Concern,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: Some(next_step),
        profile: None,
    }])
}

/// Map a provider to its (current healthy-state baseline, suggested
/// aggressive variant) profile names. Returns `None` for providers
/// that have no profile pair today (`Qmonster`, `Unknown`).
fn profile_targets_for_provider(provider: Provider) -> Option<(&'static str, &'static str)> {
    match provider {
        Provider::Claude => Some(("claude-default", "claude-script-low-token")),
        Provider::Codex => Some(("codex-default", "codex-script-low-token")),
        Provider::Gemini => Some(("gemini-default", "gemini-script-low-token")),
        Provider::Antigravity => None,
        Provider::Qmonster | Provider::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, ResolvedIdentity};

    fn mk_id(provider: Provider, role: Role, pane_id: &str) -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider,
                instance: 1,
                role,
                pane_id: pane_id.into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    fn gates_enabled(window: usize, threshold: f32) -> PolicyGates {
        PolicyGates {
            identity_confidence: IdentityConfidence::High,
            profile_switch_enabled: true,
            profile_switch_window: window,
            profile_switch_error_rate: threshold,
            ..PolicyGates::default()
        }
    }

    #[test]
    fn fires_when_error_rate_meets_threshold_for_claude_main() {
        let id = mk_id(Provider::Claude, Role::Main, "%1");
        let signals = SignalSet::default();
        // 6/10 errors = 60% >= 50% default threshold.
        let history: Vec<bool> = vec![
            true, true, true, false, true, true, true, false, false, false,
        ];
        let recs = eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.5));
        assert_eq!(recs.len(), 1, "exactly one recommendation");
        let rec = &recs[0];
        assert_eq!(rec.severity, Severity::Concern);
        assert_eq!(rec.source_kind, SourceKind::ProjectCanonical);
        assert!(
            rec.reason.contains("claude-default") && rec.reason.contains("claude-script-low-token"),
            "reason names both profiles: {:?}",
            rec.reason
        );
        assert!(
            rec.reason.contains("60%") && rec.reason.contains("50%"),
            "reason interpolates observed rate and threshold: {:?}",
            rec.reason
        );
        assert!(rec.next_step.as_deref().unwrap().contains("quota_tight"));
    }

    #[test]
    fn does_not_fire_when_error_rate_below_threshold() {
        let id = mk_id(Provider::Codex, Role::Main, "%1");
        let signals = SignalSet::default();
        // 4/10 = 40% < 50% threshold.
        let history: Vec<bool> = vec![
            true, true, false, false, true, true, false, false, false, false,
        ];
        assert!(eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.5)).is_empty());
    }

    #[test]
    fn does_not_fire_when_window_not_full() {
        let id = mk_id(Provider::Codex, Role::Main, "%1");
        let signals = SignalSet::default();
        // 5/5 = 100% errors but window is configured at 10 polls.
        let history: Vec<bool> = vec![true, true, true, true, true];
        assert!(
            eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.5)).is_empty(),
            "window must be full before the rule fires"
        );
    }

    #[test]
    fn does_not_fire_when_gate_disabled() {
        let id = mk_id(Provider::Claude, Role::Main, "%1");
        let signals = SignalSet::default();
        let history: Vec<bool> = vec![true; 10];
        let gates = PolicyGates {
            identity_confidence: IdentityConfidence::High,
            profile_switch_enabled: false,
            profile_switch_window: 10,
            profile_switch_error_rate: 0.5,
            ..PolicyGates::default()
        };
        assert!(eval_profile_switch(&id, &signals, &history, &gates).is_empty());
    }

    #[test]
    fn does_not_fire_when_low_confidence() {
        let mut id = mk_id(Provider::Claude, Role::Main, "%1");
        id.confidence = IdentityConfidence::Low;
        let signals = SignalSet::default();
        let history: Vec<bool> = vec![true; 10];
        // Build gates with low identity_confidence so the
        // allow_provider_specific check fails.
        let gates = PolicyGates {
            identity_confidence: IdentityConfidence::Low,
            profile_switch_enabled: true,
            profile_switch_window: 10,
            profile_switch_error_rate: 0.5,
            ..PolicyGates::default()
        };
        assert!(eval_profile_switch(&id, &signals, &history, &gates).is_empty());
    }

    #[test]
    fn does_not_fire_when_quota_tight_already_on() {
        // Operator already on the aggressive variant — there is no
        // more conservative profile to escalate to.
        let id = mk_id(Provider::Gemini, Role::Main, "%1");
        let signals = SignalSet::default();
        let history: Vec<bool> = vec![true; 10];
        let gates = PolicyGates {
            identity_confidence: IdentityConfidence::High,
            profile_switch_enabled: true,
            profile_switch_window: 10,
            profile_switch_error_rate: 0.5,
            quota_tight: true,
            ..PolicyGates::default()
        };
        assert!(eval_profile_switch(&id, &signals, &history, &gates).is_empty());
    }

    #[test]
    fn does_not_fire_when_pane_is_input_waiting() {
        let id = mk_id(Provider::Claude, Role::Main, "%1");
        let signals = SignalSet {
            idle_state: Some(IdleCause::InputWait),
            ..SignalSet::default()
        };
        let history: Vec<bool> = vec![true; 10];
        assert!(eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.5)).is_empty());
    }

    #[test]
    fn does_not_fire_when_pane_is_permission_waiting() {
        let id = mk_id(Provider::Claude, Role::Main, "%1");
        let signals = SignalSet {
            idle_state: Some(IdleCause::PermissionWait),
            ..SignalSet::default()
        };
        let history: Vec<bool> = vec![true; 10];
        assert!(eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.5)).is_empty());
    }

    #[test]
    fn does_not_fire_on_review_role() {
        let id = mk_id(Provider::Codex, Role::Review, "%1");
        let signals = SignalSet::default();
        let history: Vec<bool> = vec![true; 10];
        assert!(
            eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.5)).is_empty(),
            "main-pane profile pairs are not the review-tier rec target"
        );
    }

    #[test]
    fn does_not_fire_on_qmonster_provider() {
        let id = mk_id(Provider::Qmonster, Role::Main, "%1");
        let signals = SignalSet::default();
        let history: Vec<bool> = vec![true; 10];
        assert!(
            eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.5)).is_empty(),
            "no profile pair exists for the Qmonster monitor pane"
        );
    }

    #[test]
    fn fires_at_exact_threshold_boundary() {
        // 5/10 = 50.0%; the rule uses `>=` so the boundary fires.
        let id = mk_id(Provider::Codex, Role::Main, "%1");
        let signals = SignalSet::default();
        let history: Vec<bool> = vec![
            true, true, true, true, true, false, false, false, false, false,
        ];
        let recs = eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.5));
        assert_eq!(recs.len(), 1);
    }

    #[test]
    fn uses_only_freshest_window_samples() {
        // History has 12 samples; rule uses the 10 most recent. Older
        // samples beyond the window must not influence the decision.
        let id = mk_id(Provider::Claude, Role::Main, "%1");
        let signals = SignalSet::default();
        // Most-recent 10 are all `false` (rate = 0%); the two older
        // entries are `true` but should be ignored at window=10.
        let mut history: Vec<bool> = vec![false; 10];
        history.extend([true, true]);
        assert!(
            eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.5)).is_empty(),
            "samples older than the window must not contribute"
        );
    }

    #[test]
    fn custom_threshold_can_quiet_chronically_noisy_pane() {
        // Operator on a noisy workload sets threshold to 0.8; a 7/10
        // error rate (70%) must NOT fire.
        let id = mk_id(Provider::Claude, Role::Main, "%1");
        let signals = SignalSet::default();
        let history: Vec<bool> = vec![
            true, true, true, true, true, true, true, false, false, false,
        ];
        assert!(
            eval_profile_switch(&id, &signals, &history, &gates_enabled(10, 0.8)).is_empty(),
            "70% < 80% threshold must stay quiet"
        );
        // Bump above threshold (8/10 = 80%) and the rule fires.
        let history2: Vec<bool> =
            vec![true, true, true, true, true, true, true, true, false, false];
        assert_eq!(
            eval_profile_switch(&id, &signals, &history2, &gates_enabled(10, 0.8)).len(),
            1
        );
    }

    #[test]
    fn empty_history_never_fires() {
        let id = mk_id(Provider::Claude, Role::Main, "%1");
        let signals = SignalSet::default();
        assert!(eval_profile_switch(&id, &signals, &[], &gates_enabled(10, 0.5)).is_empty());
    }
}
