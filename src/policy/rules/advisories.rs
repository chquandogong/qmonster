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

    if let Some(rec) = context_pressure_critical(id, signals, gates) {
        out.push(rec);
        if gates.quota_tight {
            out.push(aggressive_context_pressure_critical());
        }
    } else if let Some(rec) = context_pressure_warning(id, signals, gates) {
        out.push(rec);
        if gates.quota_tight {
            out.push(aggressive_context_pressure_warning());
        }
    }

    if let Some(rec) = verbose_review(id, signals, gates) {
        out.push(rec);
        if gates.quota_tight {
            out.push(aggressive_verbose_review());
        }
    }

    if let Some(rec) = quota_tight_nudge(id, signals, gates) {
        out.push(rec);
    }

    if let Some(rec) = repeated_cache_suggest(id, signals, gates) {
        out.push(rec);
        if gates.quota_tight {
            out.push(aggressive_repeated_cache_suggest());
        }
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

fn context_pressure_warning(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let v = signals.context_pressure.as_ref()?.value;
    if !(0.75..0.85).contains(&v) {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "context-pressure: checkpoint",
        reason: "context warming — checkpoint first, archive large results, only then consider /compact".into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Estimated,
        suggested_command: None,
        side_effects: vec![],
    })
}

fn aggressive_context_pressure_warning() -> Recommendation {
    Recommendation {
        action: "aggressive: terse profile + archive",
        reason: "quota-tight: apply terse output profile and archive anything >500 chars".into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    }
}

fn context_pressure_critical(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let v = signals.context_pressure.as_ref()?.value;
    if v < 0.85 {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "context-pressure: act now",
        reason: "context near critical — checkpoint + archive now; /compact after".into(),
        severity: Severity::Risk,
        source_kind: SourceKind::Estimated,
        suggested_command: None,
        side_effects: vec![],
    })
}

fn aggressive_context_pressure_critical() -> Recommendation {
    Recommendation {
        action: "aggressive: clamp output, archive all",
        reason: "quota-tight critical: clamp max-output tokens and archive all non-trivial panes".into(),
        severity: Severity::Risk,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    }
}

fn verbose_review(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if !matches!(id.identity.role, Role::Review) {
        return None;
    }
    if !signals.verbose_answer {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "verbose-review: terse profile",
        reason: "review pane is verbose — consider Caveman / claude-token-efficient terse profile".into(),
        severity: Severity::Concern,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    })
}

fn aggressive_verbose_review() -> Recommendation {
    Recommendation {
        action: "aggressive: strip attribution",
        reason: "quota-tight: drop attribution footer and preamble on review output".into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    }
}

fn quota_tight_nudge(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if gates.quota_tight {
        return None; // no nudge if already enabled
    }
    let v = signals.context_pressure.as_ref()?.value;
    if v < 0.90 {
        return None;
    }
    // Not provider-flavored — do NOT check allow_provider_specific.
    Some(Recommendation {
        action: "quota-tight: consider enabling",
        reason: "sustained context pressure — consider enabling `quota_tight` in config to unlock aggressive token-saver recommendations".into(),
        severity: Severity::Concern,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    })
}

fn repeated_cache_suggest(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if !signals.repeated_output {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "repeated-output: result-hash cache",
        reason: "repeated output — consider a result-hash cache (token-optimizer-mcp)".into(),
        severity: Severity::Concern,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    })
}

fn aggressive_repeated_cache_suggest() -> Recommendation {
    Recommendation {
        action: "aggressive: dedupe + hash",
        reason: "quota-tight: enable per-pane result-hash dedupe".into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role};
    use crate::domain::signal::MetricValue;
    use crate::domain::origin::SourceKind as SK;

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

    fn pressure(v: f32) -> SignalSet {
        SignalSet {
            context_pressure: Some(MetricValue::new(v, SK::Estimated)),
            ..SignalSet::default()
        }
    }

    #[test]
    fn context_pressure_warning_at_0_75() {
        let id = id_high(Role::Main);
        let s = pressure(0.78);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(recs.iter().any(|r| r.action == "context-pressure: checkpoint"));
        assert!(!recs.iter().any(|r| r.action == "context-pressure: act now"));
    }

    #[test]
    fn context_pressure_critical_at_0_85() {
        let id = id_high(Role::Main);
        let s = pressure(0.88);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(recs.iter().any(|r| r.action == "context-pressure: act now"));
        assert!(!recs.iter().any(|r| r.action == "context-pressure: checkpoint"));
    }

    #[test]
    fn context_pressure_suppressed_on_low_identity_confidence() {
        let id = id_low(Role::Main);
        let s = pressure(0.92);
        let gates = PolicyGates { quota_tight: false, identity_confidence: IdentityConfidence::Low };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(!recs.iter().any(|r| r.action.starts_with("context-pressure")),
            "Codex #2: context_pressure_* must respect the gate");
    }

    #[test]
    fn verbose_review_requires_review_role() {
        let s = SignalSet { verbose_answer: true, ..SignalSet::default() };
        let rev = id_high(Role::Review);
        let main = id_high(Role::Main);

        let recs_rev = eval_advisories(&rev, &s, &gates_default());
        assert!(recs_rev.iter().any(|r| r.action == "verbose-review: terse profile"));

        let recs_main = eval_advisories(&main, &s, &gates_default());
        assert!(!recs_main.iter().any(|r| r.action == "verbose-review: terse profile"),
            "verbose_review must NOT fire on role=Main");
    }

    #[test]
    fn quota_tight_nudge_fires_only_when_gate_off_and_pressure_high() {
        let id = id_high(Role::Main);
        let s = pressure(0.92);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(recs.iter().any(|r| r.action == "quota-tight: consider enabling"));
    }

    #[test]
    fn quota_tight_nudge_never_fires_when_gate_on() {
        let id = id_high(Role::Main);
        let s = pressure(0.92);
        let gates = PolicyGates { quota_tight: true, identity_confidence: IdentityConfidence::High };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(!recs.iter().any(|r| r.action == "quota-tight: consider enabling"));
    }

    #[test]
    fn quota_tight_nudge_fires_regardless_of_identity_confidence() {
        let id = id_low(Role::Main);
        let s = pressure(0.92);
        let gates = PolicyGates { quota_tight: false, identity_confidence: IdentityConfidence::Low };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(recs.iter().any(|r| r.action == "quota-tight: consider enabling"),
            "quota_tight_nudge is Qmonster-config-level, not provider-flavored");
    }

    #[test]
    fn repeated_cache_suggest_fires_on_repeated_output() {
        let id = id_high(Role::Main);
        let s = SignalSet { repeated_output: true, ..SignalSet::default() };
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(recs.iter().any(|r| r.action == "repeated-output: result-hash cache"));
    }
}
