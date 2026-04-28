use crate::domain::identity::ResolvedIdentity;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::{IdleCause, SignalSet};
use crate::policy::gates::{PolicyGates, allow_provider_specific};

/// Phase F F-2 (v1.23.0): bloat advisory on the per-pane agent
/// memory file footprint. Pure function over
/// `(identity, signals, gates)` that fires a `Severity::Concern`
/// passive advisory when the summed bytes exceed a fixed threshold,
/// so the operator gets a context-management trigger before reaching
/// for `/compact` / `/clear` / `/memory`. The threshold is hard-coded
/// at `BLOAT_THRESHOLD_BYTES` (50_000) for v1; making it
/// operator-configurable is deferred to a later slice.
pub fn eval_agent_memory(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    if let Some(rec) = recommend_memory_bloat_advisory(id, signals, gates) {
        out.push(rec);
    }
    out
}

const BLOAT_THRESHOLD_BYTES: u64 = 50_000;

fn recommend_memory_bloat_advisory(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    // Provider-specific gate: agent_memory_bytes fill is keyed off
    // the per-provider candidate list, so a Low-confidence pane has
    // unreliable provider attribution and the rule would fire on
    // mis-classified files (e.g., a Codex pane resolved as Claude
    // would scan CLAUDE.md and attribute a bogus number).
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }

    let metric = signals.agent_memory_bytes.as_ref()?;
    if metric.value <= BLOAT_THRESHOLD_BYTES {
        return None;
    }

    // Suppress when the operator's attention is already on a more
    // pressing prompt — bloat is a context-management nudge, not an
    // alert. Mirrors the auto_memory rule's input-wait suppression.
    if matches!(
        signals.idle_state,
        Some(IdleCause::InputWait) | Some(IdleCause::PermissionWait)
    ) {
        return None;
    }

    let kb = (metric.value as f64) / 1024.0;
    Some(Recommendation {
        action: "agent-memory: trim before /compact",
        reason: format!(
            "agent memory files total {:.1} KB (> 50_000 bytes (~49 KiB) advisory threshold) — every prompt loads them; trimming reduces session prompt surface and lets cache rebuild on a smaller stable prefix",
            kb,
        ),
        severity: Severity::Concern,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: Some(
            "trim CLAUDE.md / AGENTS.md / GEMINI.md content into per-task .claude/skills/, ~/.codex/AGENTS.override.md, or .gemini/skills/ on-demand docs to reduce session prompt surface".into(),
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

    fn gates_low(conf: IdentityConfidence) -> PolicyGates {
        PolicyGates {
            identity_confidence: conf,
            ..PolicyGates::default()
        }
    }

    fn signals_with_bytes(bytes: u64) -> SignalSet {
        SignalSet {
            agent_memory_bytes: Some(MetricValue::new(bytes, SourceKind::Heuristic)),
            ..SignalSet::default()
        }
    }

    #[test]
    fn fires_when_bytes_exceed_threshold_with_concern_and_project_canonical() {
        let recs = eval_agent_memory(
            &id(Provider::Claude, IdentityConfidence::High),
            &signals_with_bytes(60_000),
            &gates_high(),
        );
        assert_eq!(recs.len(), 1);
        let rec = &recs[0];
        assert_eq!(rec.severity, Severity::Concern);
        assert_eq!(rec.source_kind, SourceKind::ProjectCanonical);
        assert!(rec.reason.contains("58.6 KB"));
        let step = rec.next_step.as_deref().unwrap();
        assert!(step.contains(".claude/skills"));
        assert!(step.contains("~/.codex/AGENTS.override.md"));
        assert!(step.contains(".gemini/skills"));
        assert!(!rec.is_strong);
        assert!(rec.suggested_command.is_none());
    }

    #[test]
    fn suppressed_at_or_below_threshold() {
        let at = eval_agent_memory(
            &id(Provider::Claude, IdentityConfidence::High),
            &signals_with_bytes(50_000),
            &gates_high(),
        );
        assert!(at.is_empty());
        let below = eval_agent_memory(
            &id(Provider::Claude, IdentityConfidence::High),
            &signals_with_bytes(49_999),
            &gates_high(),
        );
        assert!(below.is_empty());
    }

    #[test]
    fn suppressed_when_signal_is_none() {
        let recs = eval_agent_memory(
            &id(Provider::Claude, IdentityConfidence::High),
            &SignalSet::default(),
            &gates_high(),
        );
        assert!(recs.is_empty());
    }

    #[test]
    fn suppressed_on_low_confidence() {
        let recs = eval_agent_memory(
            &id(Provider::Claude, IdentityConfidence::Low),
            &signals_with_bytes(60_000),
            &gates_low(IdentityConfidence::Low),
        );
        assert!(recs.is_empty());
        let recs = eval_agent_memory(
            &id(Provider::Claude, IdentityConfidence::Unknown),
            &signals_with_bytes(60_000),
            &gates_low(IdentityConfidence::Unknown),
        );
        assert!(recs.is_empty());
    }

    #[test]
    fn suppressed_when_input_or_permission_wait_active() {
        let s_input = SignalSet {
            agent_memory_bytes: Some(MetricValue::new(60_000, SourceKind::Heuristic)),
            idle_state: Some(IdleCause::InputWait),
            ..SignalSet::default()
        };
        let recs = eval_agent_memory(
            &id(Provider::Claude, IdentityConfidence::High),
            &s_input,
            &gates_high(),
        );
        assert!(recs.is_empty());
        let s_perm = SignalSet {
            agent_memory_bytes: Some(MetricValue::new(60_000, SourceKind::Heuristic)),
            idle_state: Some(IdleCause::PermissionWait),
            ..SignalSet::default()
        };
        let recs = eval_agent_memory(
            &id(Provider::Claude, IdentityConfidence::High),
            &s_perm,
            &gates_high(),
        );
        assert!(recs.is_empty());
    }
}
