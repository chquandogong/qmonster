use crate::domain::identity::ResolvedIdentity;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::{IdleCause, SignalSet};
use crate::policy::gates::{PolicyGates, allow_provider_specific};

/// Phase F F-7c (v1.34.0): reset-aware advisories. When a quota
/// window is high AND its reset is close, suggest pausing until
/// reset (`wait_for_reset`). When ANY reset is very close,
/// suggest snapshotting before the reset rolls so handoff state
/// survives (`snapshot_before_reset`).
///
/// Both rules consume the `quota_*_resets_at` Unix timestamps
/// populated by F-5b (Claude sidefile) and F-6 (Codex App Server)
/// — Gemini panes have no resets_at surface today, so the rules
/// stay silent for them. Both rules gate on
/// `IdentityConfidence ≥ Medium` and suppress when the operator's
/// attention is on a permission/input prompt (don't pile advice
/// onto a paused pane).
///
/// Phase F F-7d (v1.35.0) lifted the four thresholds out of
/// hardcoded module constants into the `[reset]` config section
/// → `PolicyGates::reset_*` fields. Defaults (`wait_pressure=0.85`,
/// `wait_eta=30m`, `snapshot_pressure=0.50`, `snapshot_eta=5m`)
/// match the v1.34.0 constants exactly so an operator with no
/// `[reset]` section gets the original behavior.
pub fn eval_reset(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
    now_unix_seconds: u64,
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    if !allow_provider_specific(gates.identity_confidence) {
        return out;
    }
    if matches!(
        signals.idle_state,
        Some(IdleCause::InputWait) | Some(IdleCause::PermissionWait)
    ) {
        return out;
    }

    if let Some(rec) = recommend_wait_for_reset(
        id,
        "5h",
        signals.quota_5h_pressure.as_ref().map(|m| m.value),
        signals.quota_5h_resets_at.as_ref().map(|m| m.value),
        now_unix_seconds,
        gates,
    ) {
        out.push(rec);
    }
    if let Some(rec) = recommend_wait_for_reset(
        id,
        "weekly",
        signals.quota_weekly_pressure.as_ref().map(|m| m.value),
        signals.quota_weekly_resets_at.as_ref().map(|m| m.value),
        now_unix_seconds,
        gates,
    ) {
        out.push(rec);
    }
    if let Some(rec) = recommend_snapshot_before_reset(
        id,
        "5h",
        signals.quota_5h_pressure.as_ref().map(|m| m.value),
        signals.quota_5h_resets_at.as_ref().map(|m| m.value),
        now_unix_seconds,
        gates,
    ) {
        out.push(rec);
    }
    if let Some(rec) = recommend_snapshot_before_reset(
        id,
        "weekly",
        signals.quota_weekly_pressure.as_ref().map(|m| m.value),
        signals.quota_weekly_resets_at.as_ref().map(|m| m.value),
        now_unix_seconds,
        gates,
    ) {
        out.push(rec);
    }
    out
}

fn recommend_wait_for_reset(
    _id: &ResolvedIdentity,
    window_label: &'static str,
    pressure: Option<f32>,
    resets_at: Option<u64>,
    now_unix_seconds: u64,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let pressure = pressure?;
    let resets_at = resets_at?;
    if pressure < gates.reset_wait_pressure {
        return None;
    }
    if resets_at <= now_unix_seconds {
        return None;
    }
    let remaining = resets_at - now_unix_seconds;
    if remaining > gates.reset_wait_eta_secs {
        return None;
    }
    Some(Recommendation {
        action: match window_label {
            "5h" => "quota: pause until 5h window resets",
            _ => "quota: pause until weekly window resets",
        },
        reason: format!(
            "{} quota at {:.0}% (≥ {:.0}% wait threshold), resets in {} — pausing here keeps the next session's full window intact",
            window_label,
            pressure * 100.0,
            gates.reset_wait_pressure * 100.0,
            format_eta_short(remaining),
        ),
        severity: Severity::Concern,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: Some(format!(
            "stop submitting prompts until the {window_label} window resets; F-5b / F-6 surface the countdown on the pane card"
        )),
        profile: None,
    })
}

fn recommend_snapshot_before_reset(
    _id: &ResolvedIdentity,
    window_label: &'static str,
    pressure: Option<f32>,
    resets_at: Option<u64>,
    now_unix_seconds: u64,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let pressure = pressure?;
    let resets_at = resets_at?;
    if pressure < gates.reset_snapshot_pressure {
        return None;
    }
    if resets_at <= now_unix_seconds {
        return None;
    }
    let remaining = resets_at - now_unix_seconds;
    if remaining > gates.reset_snapshot_eta_secs {
        return None;
    }
    Some(Recommendation {
        action: match window_label {
            "5h" => "snapshot before 5h window resets",
            _ => "snapshot before weekly window resets",
        },
        reason: format!(
            "{} quota resets in {} — write a snapshot now so handoff state (open files, current task) survives the reset",
            window_label,
            format_eta_short(remaining),
        ),
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: Some("press 's' to write a runtime snapshot to ~/.qmonster/snapshots/".into()),
        profile: None,
    })
}

/// Compact countdown formatter local to this module — kept small
/// because the rule reasons embed it inline. Mirrors
/// `panels.rs::format_resets_eta`'s shape so the badge and the
/// recommendation reason agree.
fn format_eta_short(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours >= 1 {
        format!("{hours}h{minutes:02}m")
    } else if minutes >= 1 {
        format!("{minutes}m")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};
    use crate::domain::signal::MetricValue;

    fn id(provider: Provider) -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    fn gates_high() -> PolicyGates {
        PolicyGates {
            identity_confidence: IdentityConfidence::High,
            ..PolicyGates::default()
        }
    }

    fn signals_with(
        quota_5h_pct: Option<f32>,
        quota_5h_eta_secs: Option<u64>,
        now: u64,
    ) -> SignalSet {
        SignalSet {
            quota_5h_pressure: quota_5h_pct
                .map(|v| MetricValue::new(v, SourceKind::ProviderOfficial)),
            quota_5h_resets_at: quota_5h_eta_secs
                .map(|secs| MetricValue::new(now + secs, SourceKind::ProviderOfficial)),
            ..SignalSet::default()
        }
    }

    #[test]
    fn wait_for_reset_fires_when_pressure_high_and_eta_close() {
        let s = signals_with(Some(0.92), Some(15 * 60), 100);
        let recs = eval_reset(&id(Provider::Claude), &s, &gates_high(), 100);
        assert!(
            recs.iter()
                .any(|r| r.action == "quota: pause until 5h window resets"),
            "wait_for_reset should fire at 92% with 15m to go"
        );
    }

    #[test]
    fn wait_for_reset_skips_when_pressure_below_threshold() {
        let s = signals_with(Some(0.50), Some(10 * 60), 100);
        let recs = eval_reset(&id(Provider::Claude), &s, &gates_high(), 100);
        assert!(
            recs.iter()
                .all(|r| r.action != "quota: pause until 5h window resets"),
            "low pressure should suppress wait_for_reset"
        );
    }

    #[test]
    fn wait_for_reset_skips_when_eta_too_far() {
        let s = signals_with(Some(0.92), Some(2 * 3600), 100);
        let recs = eval_reset(&id(Provider::Claude), &s, &gates_high(), 100);
        assert!(
            recs.iter()
                .all(|r| r.action != "quota: pause until 5h window resets"),
            "far ETA should suppress wait_for_reset"
        );
    }

    #[test]
    fn snapshot_before_reset_fires_when_eta_imminent_and_pressure_meaningful() {
        let s = signals_with(Some(0.60), Some(3 * 60), 100);
        let recs = eval_reset(&id(Provider::Claude), &s, &gates_high(), 100);
        assert!(
            recs.iter()
                .any(|r| r.action == "snapshot before 5h window resets"),
            "snapshot_before_reset should fire at 60% with 3m to go"
        );
    }

    #[test]
    fn snapshot_before_reset_skips_when_pressure_negligible() {
        // No reason to snapshot for handoff if there's no real
        // session state to preserve (operator just started).
        let s = signals_with(Some(0.10), Some(2 * 60), 100);
        let recs = eval_reset(&id(Provider::Claude), &s, &gates_high(), 100);
        assert!(
            recs.iter()
                .all(|r| r.action != "snapshot before 5h window resets"),
            "trivial pressure should suppress snapshot_before_reset"
        );
    }

    #[test]
    fn both_rules_can_co_fire_at_high_pressure_and_imminent_reset() {
        let s = signals_with(Some(0.95), Some(2 * 60), 100);
        let recs = eval_reset(&id(Provider::Claude), &s, &gates_high(), 100);
        let actions: Vec<&'static str> = recs.iter().map(|r| r.action).collect();
        assert!(actions.contains(&"quota: pause until 5h window resets"));
        assert!(actions.contains(&"snapshot before 5h window resets"));
    }

    #[test]
    fn both_windows_evaluate_independently() {
        let now = 100u64;
        let s = SignalSet {
            quota_5h_pressure: Some(MetricValue::new(0.92, SourceKind::ProviderOfficial)),
            quota_5h_resets_at: Some(MetricValue::new(
                now + 15 * 60,
                SourceKind::ProviderOfficial,
            )),
            quota_weekly_pressure: Some(MetricValue::new(0.92, SourceKind::ProviderOfficial)),
            quota_weekly_resets_at: Some(MetricValue::new(
                now + 20 * 60,
                SourceKind::ProviderOfficial,
            )),
            ..SignalSet::default()
        };
        let recs = eval_reset(&id(Provider::Claude), &s, &gates_high(), now);
        let actions: Vec<&'static str> = recs.iter().map(|r| r.action).collect();
        assert!(actions.contains(&"quota: pause until 5h window resets"));
        assert!(actions.contains(&"quota: pause until weekly window resets"));
    }

    #[test]
    fn rules_skip_when_already_in_input_or_permission_wait() {
        let mut s = signals_with(Some(0.95), Some(2 * 60), 100);
        s.idle_state = Some(IdleCause::InputWait);
        let recs = eval_reset(&id(Provider::Claude), &s, &gates_high(), 100);
        assert!(
            recs.is_empty(),
            "input/permission wait suppresses reset advisories — operator already paused"
        );
    }

    #[test]
    fn rules_skip_when_identity_confidence_low() {
        let s = signals_with(Some(0.95), Some(2 * 60), 100);
        let gates = PolicyGates {
            identity_confidence: IdentityConfidence::Low,
            ..PolicyGates::default()
        };
        let recs = eval_reset(&id(Provider::Claude), &s, &gates, 100);
        assert!(recs.is_empty());
    }

    #[test]
    fn custom_wait_pressure_threshold_from_gates_lets_lower_pressure_fire() {
        // Phase F F-7d (v1.35.0): operator can lower the wait
        // threshold via [reset] config so pause-advice fires
        // earlier than the 0.85 default.
        let s = signals_with(Some(0.65), Some(15 * 60), 100);
        let strict_gates = gates_high();
        let recs_default = eval_reset(&id(Provider::Claude), &s, &strict_gates, 100);
        assert!(
            recs_default
                .iter()
                .all(|r| r.action != "quota: pause until 5h window resets"),
            "default 0.85 threshold should suppress at 65% pressure"
        );

        let lenient = PolicyGates {
            reset_wait_pressure: 0.50,
            ..gates_high()
        };
        let recs_lenient = eval_reset(&id(Provider::Claude), &s, &lenient, 100);
        assert!(
            recs_lenient
                .iter()
                .any(|r| r.action == "quota: pause until 5h window resets"),
            "lowered 0.50 threshold should let 65% pressure fire wait_for_reset"
        );
    }

    #[test]
    fn custom_snapshot_eta_threshold_from_gates_widens_imminent_window() {
        // Lift the snapshot eta from 5m to 15m so a 10m countdown
        // qualifies as imminent for an operator who wants more
        // lead-time before reset.
        let s = signals_with(Some(0.70), Some(10 * 60), 100);
        let recs_default = eval_reset(&id(Provider::Claude), &s, &gates_high(), 100);
        assert!(
            recs_default
                .iter()
                .all(|r| r.action != "snapshot before 5h window resets"),
            "default 5m snapshot eta should not fire at 10m"
        );

        let widened = PolicyGates {
            reset_snapshot_eta_secs: 15 * 60,
            ..gates_high()
        };
        let recs_widened = eval_reset(&id(Provider::Claude), &s, &widened, 100);
        assert!(
            recs_widened
                .iter()
                .any(|r| r.action == "snapshot before 5h window resets"),
            "widened 15m snapshot eta should fire at 10m"
        );
    }

    #[test]
    fn rules_skip_when_resets_at_already_past() {
        let now = 1_000_000_u64;
        let s = SignalSet {
            quota_5h_pressure: Some(MetricValue::new(0.95, SourceKind::ProviderOfficial)),
            quota_5h_resets_at: Some(MetricValue::new(now - 60, SourceKind::ProviderOfficial)),
            ..SignalSet::default()
        };
        let recs = eval_reset(&id(Provider::Claude), &s, &gates_high(), now);
        assert!(
            recs.is_empty(),
            "stale resets_at must not produce a negative-eta countdown"
        );
    }
}
