use crate::domain::identity::ResolvedIdentity;
use crate::domain::recommendation::{Recommendation, RequestedEffect};
use crate::domain::signal::SignalSet;
use crate::policy::rules::eval_alerts;

#[derive(Debug, Default, Clone, Copy)]
pub struct Engine;

#[derive(Debug, Clone)]
pub struct EvalOutput {
    pub recommendations: Vec<Recommendation>,
    pub effects: Vec<RequestedEffect>,
}

impl Engine {
    pub fn evaluate(&self, id: &ResolvedIdentity, signals: &SignalSet) -> EvalOutput {
        let recs = eval_alerts(id, signals);
        let mut effects = Vec::new();
        if !recs.is_empty() {
            // Any alert at all means we want to notify. Archive is
            // reserved for Phase 2; we never request sensitive effects.
            effects.push(RequestedEffect::Notify);
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

    #[test]
    fn engine_runs_alert_rules() {
        let s = SignalSet {
            waiting_for_input: true,
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s);
        assert!(!out.recommendations.is_empty());
    }

    #[test]
    fn engine_produces_notify_effect_for_input_wait() {
        let s = SignalSet {
            waiting_for_input: true,
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s);
        assert!(out.effects.contains(&crate::domain::recommendation::RequestedEffect::Notify));
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
        let out = eng.evaluate(&id(IdentityConfidence::High), &s);
        assert!(
            !out.effects
                .contains(&crate::domain::recommendation::RequestedEffect::SensitiveNotImplemented)
        );
    }
}
