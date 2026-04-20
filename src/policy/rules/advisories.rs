use crate::domain::identity::{ResolvedIdentity, Role};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::SignalSet;
use crate::policy::gates::{PolicyGates, allow_provider_specific};

/// Phase 3A advisory rules. Each rule is a pure function over
/// `(identity, signals, gates)`. Provider-flavored rules respect the
/// IdentityConfidence gate (KD-007). `quota_tight` unlocks aggressive
/// variants on some rules; the `quota_tight_nudge` rule is the only
/// non-provider-flavored advisory.
pub fn eval_advisories(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Vec<Recommendation> {
    let mut out = Vec::new();

    if let Some(rec) = log_storm_advisory(id, signals, gates) {
        out.push(rec);
        if gates.quota_tight {
            out.push(aggressive_log_storm());
        }
    }

    if let Some(rec) = code_exploration(id, signals, gates) {
        out.push(rec);
    }

    out
}

fn log_storm_advisory(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if !signals.log_storm {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "log-storm: ingress filter + summary",
        reason: "heavy ingress — use RTK-style ingress filter and produce a context-mode summary after archive".into(),
        severity: Severity::Concern,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    })
}

fn aggressive_log_storm() -> Recommendation {
    Recommendation {
        action: "aggressive: drop non-essential ingress",
        reason: "quota-tight: suppress low-value ingress lines; keep only error/warn markers".into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    }
}

fn code_exploration(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if !matches!(id.identity.role, Role::Main | Role::Review) {
        return None;
    }
    let triggers_fired = matches!(signals.task_type, crate::domain::signal::TaskType::CodeExploration)
        || signals.verbose_answer
        || signals.output_chars >= 1500;
    if !triggers_fired {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "code-exploration: graph/symbol",
        reason: "prefer graph/symbol navigation (Token Savior / code-review-graph); avoid full-file re-reads; delegate deep scans to the research pane".into(),
        severity: Severity::Concern,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role};

    fn id_high(role: Role) -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity { provider: Provider::Claude, instance: 1, role, pane_id: "%1".into() },
            confidence: IdentityConfidence::High,
        }
    }

    fn id_low(role: Role) -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity { provider: Provider::Claude, instance: 1, role, pane_id: "%1".into() },
            confidence: IdentityConfidence::Low,
        }
    }

    fn gates_default() -> PolicyGates {
        PolicyGates { quota_tight: false, identity_confidence: IdentityConfidence::High }
    }

    #[test]
    fn log_storm_advisory_fires_with_heuristic_source_kind() {
        let id = id_high(Role::Main);
        let s = SignalSet { log_storm: true, ..SignalSet::default() };
        let recs = eval_advisories(&id, &s, &gates_default());
        let adv = recs
            .iter()
            .find(|r| r.action == "log-storm: ingress filter + summary")
            .expect("log_storm_advisory must fire");
        assert_eq!(adv.source_kind, SourceKind::Heuristic);
        assert_eq!(adv.severity, Severity::Concern);
    }

    #[test]
    fn code_exploration_fires_on_verbose_main_role() {
        let id = id_high(Role::Main);
        let s = SignalSet { verbose_answer: true, ..SignalSet::default() };
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(recs.iter().any(|r| r.action == "code-exploration: graph/symbol"));
    }

    #[test]
    fn code_exploration_suppressed_on_low_identity_confidence() {
        let id = id_low(Role::Main);
        let s = SignalSet { verbose_answer: true, ..SignalSet::default() };
        let gates = PolicyGates { quota_tight: false, identity_confidence: IdentityConfidence::Low };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(!recs.iter().any(|r| r.action == "code-exploration: graph/symbol"));
    }
}
