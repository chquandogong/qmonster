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
        if !recs.is_empty() {
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
}
