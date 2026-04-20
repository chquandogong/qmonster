use crate::domain::identity::ResolvedIdentity;
use crate::domain::recommendation::{Recommendation, RequestedEffect};
use crate::domain::signal::SignalSet;
use crate::policy::rules::eval_alerts;
use crate::policy::gates::PolicyGates;

#[derive(Debug, Default, Clone, Copy)]
pub struct Engine;

#[derive(Debug, Clone)]
pub struct EvalOutput {
    pub recommendations: Vec<Recommendation>,
    pub effects: Vec<RequestedEffect>,
}

impl Engine {
    pub fn evaluate(
        &self,
        id: &ResolvedIdentity,
        signals: &SignalSet,
        _gates: &PolicyGates,
    ) -> EvalOutput {
        let recs = eval_alerts(id, signals);
        let mut effects = Vec::new();
        // Notify fires only when at least one rec is urgent (Warning or Risk).
        // Concern-severity passive advisories stay in-UI (Codex #3 fix; r1
        // plan "Alert-first" principle).
        use crate::domain::recommendation::Severity;
        let any_urgent = recs.iter().any(|r| r.severity >= Severity::Warning);
        if any_urgent {
            effects.push(RequestedEffect::Notify);
        }
        // Phase 2: log storms trigger a runtime-local archive write so
        // the raw tail survives even though the screen only keeps a
        // preview. The allow-list gate in app::EffectRunner still
        // decides whether it actually runs.
        if signals.log_storm {
            effects.push(RequestedEffect::ArchiveLocal);
        }
        EvalOutput {
            recommendations: recs,
            effects,
        }
    }

    pub fn evaluate_cross_pane(
        &self,
        panes: &[PaneView<'_>],
    ) -> Vec<crate::domain::recommendation::CrossPaneFinding> {
        crate::policy::rules::concurrent::eval_concurrent(panes)
    }
}

/// Read-only view over one pane's current state, used by cross-pane
/// rules. Built upstream by `app::event_loop` from the per-pane
/// report; never constructed inside `policy/`.
#[derive(Debug, Clone, Copy)]
pub struct PaneView<'a> {
    pub identity: &'a ResolvedIdentity,
    pub signals: &'a SignalSet,
    pub current_path: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role};
    use crate::domain::signal::SignalSet;

    fn id(conf: IdentityConfidence) -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: conf,
        }
    }

    fn gates() -> PolicyGates {
        PolicyGates {
            quota_tight: false,
            identity_confidence: IdentityConfidence::High,
        }
    }

    #[test]
    fn engine_runs_alert_rules() {
        let s = SignalSet {
            waiting_for_input: true,
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates());
        assert!(!out.recommendations.is_empty());
    }

    #[test]
    fn engine_produces_notify_effect_for_input_wait() {
        let s = SignalSet {
            waiting_for_input: true,
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates());
        assert!(out.effects.contains(&crate::domain::recommendation::RequestedEffect::Notify));
    }

    #[test]
    fn log_storm_also_requests_archive_local() {
        let s = SignalSet {
            log_storm: true,
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates());
        assert!(
            out.effects
                .contains(&crate::domain::recommendation::RequestedEffect::ArchiveLocal)
        );
    }

    #[test]
    fn non_storm_signal_does_not_request_archive() {
        let s = SignalSet {
            waiting_for_input: true,
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates());
        assert!(
            !out.effects
                .contains(&crate::domain::recommendation::RequestedEffect::ArchiveLocal)
        );
    }

    #[test]
    fn engine_does_not_emit_sensitive_effects() {
        // The allow-list in recommend_only must reject SensitiveNotImplemented.
        let s = SignalSet {
            log_storm: true,
            subagent_hint: true,
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates());
        assert!(
            !out.effects
                .contains(&crate::domain::recommendation::RequestedEffect::SensitiveNotImplemented)
        );
    }

    #[test]
    fn notify_effect_fires_only_for_warning_or_higher() {
        let s = SignalSet {
            waiting_for_input: true, // produces a Warning-severity rec
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates());
        assert!(
            out.effects.contains(&crate::domain::recommendation::RequestedEffect::Notify),
            "Warning-severity rec must still trigger Notify"
        );
    }

    #[test]
    fn notify_effect_absent_when_only_concern_severity_recs() {
        // repeated_output is Concern-severity in alerts.rs.
        let s = SignalSet {
            repeated_output: true,
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates());
        assert!(
            !out.effects.contains(&crate::domain::recommendation::RequestedEffect::Notify),
            "Concern-severity recs must NOT trigger Notify (Codex #3)"
        );
        assert!(
            !out.recommendations.is_empty(),
            "sanity: repeated_output rec still exists in the list"
        );
    }

    #[test]
    fn evaluate_cross_pane_returns_empty_for_zero_panes() {
        let eng = Engine;
        let views: Vec<PaneView<'_>> = vec![];
        assert!(eng.evaluate_cross_pane(&views).is_empty());
    }

    #[test]
    fn evaluate_cross_pane_returns_empty_for_one_pane() {
        let identity = id(IdentityConfidence::High);
        let signals = SignalSet::default();
        let eng = Engine;
        let views = vec![PaneView {
            identity: &identity,
            signals: &signals,
            current_path: "/repo",
        }];
        assert!(eng.evaluate_cross_pane(&views).is_empty());
    }

    #[test]
    fn evaluate_cross_pane_uses_concurrent_rule() {
        use crate::domain::identity::{PaneIdentity, Provider, Role};
        let id_a = ResolvedIdentity {
            identity: PaneIdentity { provider: Provider::Claude, instance: 1, role: Role::Main, pane_id: "%1".into() },
            confidence: IdentityConfidence::High,
        };
        let id_b = ResolvedIdentity {
            identity: PaneIdentity { provider: Provider::Claude, instance: 2, role: Role::Main, pane_id: "%2".into() },
            confidence: IdentityConfidence::High,
        };
        let s = SignalSet { output_chars: 800, ..SignalSet::default() };
        let views = vec![
            PaneView { identity: &id_a, signals: &s, current_path: "/repo" },
            PaneView { identity: &id_b, signals: &s, current_path: "/repo" },
        ];
        let findings = Engine.evaluate_cross_pane(&views);
        assert_eq!(findings.len(), 1);
    }
}
