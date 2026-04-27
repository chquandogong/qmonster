use crate::domain::identity::ResolvedIdentity;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::{IdleCause, SignalSet, TaskType};
use crate::policy::gates::{PolicyGates, allow_provider_specific};

/// Phase 4 G-5 auto-memory guide. Pure function over
/// `(identity, signals, gates)` that emits a gentle advisory when
/// state-critical work is detected — the operator should route
/// handoff content into `.mission/CURRENT_STATE.md` or a new
/// `.mission/decisions/MDR-XXX.md` rather than the provider's
/// `save_memory` / auto-memory surface (mission.yaml constraint:
/// "Auto memory ... never the primary state store").
pub fn eval_auto_memory(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    if let Some(rec) = recommend_mdr_over_auto_memory(id, signals, gates) {
        out.push(rec);
    }
    out
}

/// G-5 rule: fire when the pane's observed task type is Review or
/// SessionResume — both classes of work that produce handoff-grade
/// state (decisions, context restore) that belongs in the project
/// ledger, not in a provider's machine-local memory store. The
/// recommendation is ProjectCanonical (this is a Qmonster
/// architectural principle, not a provider-doc fact) and stays
/// Severity::Concern — passive advisory, does NOT trigger the
/// `>= Warning` Notify gate.
fn recommend_mdr_over_auto_memory(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    // Respect the provider-specific gate even though the guidance
    // itself is ProjectCanonical: task_type inference comes from the
    // per-provider adapter, so a Low-confidence pane has unreliable
    // task_type and the rule would fire on noise.
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }

    // Fire only on the two task types that are canonical examples of
    // state-critical work. Unknown / CodeExploration / LogTriage /
    // Summary / Automation produce other kinds of output but do NOT
    // generate handoff-grade state that gets lost if stored only in
    // auto-memory.
    let triggers_state_critical = matches!(
        signals.task_type,
        TaskType::Review | TaskType::SessionResume
    );
    if !triggers_state_critical {
        return None;
    }

    // Suppress when the operator's attention is already elsewhere —
    // a permission prompt or input wait is the more pressing signal,
    // and the memory guide would be noise.
    if matches!(
        signals.idle_state,
        Some(IdleCause::InputWait) | Some(IdleCause::PermissionWait)
    ) {
        return None;
    }

    Some(Recommendation {
        action: "auto-memory: route to MDR / CURRENT_STATE",
        reason: format!(
            "state-critical task detected ({:?}): record handoff content in the project ledger, not in provider auto-memory",
            signals.task_type,
        ),
        severity: Severity::Concern,
        source_kind: SourceKind::ProjectCanonical,
        // Not a single runnable command — this is an operator routing
        // decision. The concrete next-step prose below points at the
        // two canonical write targets (CURRENT_STATE.md + a new MDR).
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: Some(
            "record in `.mission/CURRENT_STATE.md` or create a new `.mission/decisions/MDR-XXX.md`; avoid provider save_memory / auto-memory for state-critical content"
                .into(),
        ),
        profile: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};

    fn id_high() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    fn gates_default() -> PolicyGates {
        PolicyGates {
            quota_tight: false,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
            quota_5h_warning_pct: 0.75,
            quota_5h_critical_pct: 0.85,
            quota_weekly_warning_pct: 0.75,
            quota_weekly_critical_pct: 0.85,
            cross_window_findings: false,
            identity_drift_findings: false,
        }
    }

    #[test]
    fn fires_on_review_task_with_mdr_next_step_and_project_canonical_source() {
        // G-5 trigger condition #1: TaskType::Review — the reviewer
        // pane produces decisions + judgments + conclusions that
        // belong in an MDR or CURRENT_STATE.md, never in
        // machine-local auto-memory. The rec must carry the
        // operator-facing routing hint in `next_step` and be labeled
        // ProjectCanonical (Qmonster's own architectural rule, not a
        // provider doc fact).
        let s = SignalSet {
            task_type: TaskType::Review,
            ..SignalSet::default()
        };
        let recs = eval_auto_memory(&id_high(), &s, &gates_default());
        let rec = recs
            .iter()
            .find(|r| r.action == "auto-memory: route to MDR / CURRENT_STATE")
            .expect("auto-memory guide rec must fire on Review task");

        assert_eq!(
            rec.severity,
            Severity::Concern,
            "passive advisory; does NOT trigger Notify (< Warning)"
        );
        assert_eq!(
            rec.source_kind,
            SourceKind::ProjectCanonical,
            "G-5 is a Qmonster architectural rule, not a provider-doc fact"
        );
        let step = rec
            .next_step
            .as_deref()
            .expect("G-5 rec must carry a `next_step` with the canonical write targets");
        assert!(
            step.contains("CURRENT_STATE.md"),
            "next_step must point at CURRENT_STATE.md: {step}"
        );
        assert!(
            step.contains("MDR"),
            "next_step must point at an MDR record: {step}"
        );
        assert!(
            step.contains("save_memory") || step.contains("auto-memory"),
            "next_step must explicitly discourage provider auto-memory: {step}"
        );
        assert!(
            rec.suggested_command.is_none(),
            "operator routing decision; no single runnable command"
        );
    }

    #[test]
    fn fires_on_session_resume_task() {
        // G-5 trigger condition #2: TaskType::SessionResume — the
        // operator is reconstructing a prior session's state, which
        // is exactly the moment where routing handoff info into the
        // project ledger matters most.
        let s = SignalSet {
            task_type: TaskType::SessionResume,
            ..SignalSet::default()
        };
        let recs = eval_auto_memory(&id_high(), &s, &gates_default());
        assert!(
            recs.iter()
                .any(|r| r.action == "auto-memory: route to MDR / CURRENT_STATE"),
            "auto-memory guide rec must fire on SessionResume task"
        );
    }

    #[test]
    fn suppressed_on_non_state_critical_task_types() {
        // The rule must stay narrow. CodeExploration / LogTriage /
        // Summary / Automation / Unknown all produce output, but
        // none of it is the kind of handoff state that gets lost if
        // stored only in auto-memory. Firing on every task type
        // would turn a useful advisory into background noise.
        for task in [
            TaskType::Unknown,
            TaskType::CodeExploration,
            TaskType::LogTriage,
            TaskType::Summary,
            TaskType::Automation,
        ] {
            let s = SignalSet {
                task_type: task,
                ..SignalSet::default()
            };
            let recs = eval_auto_memory(&id_high(), &s, &gates_default());
            assert!(
                !recs
                    .iter()
                    .any(|r| r.action == "auto-memory: route to MDR / CURRENT_STATE"),
                "auto-memory guide must NOT fire on non-state-critical task {:?}",
                task
            );
        }
    }

    #[test]
    fn suppressed_on_low_identity_confidence() {
        // task_type comes from the per-provider adapter. On a
        // Low-confidence pane that inference is unreliable, so the
        // rule would fire on noise. Respect the KD-007 gate.
        let id = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::Low,
        };
        let gates = PolicyGates {
            quota_tight: false,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::Low,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
            quota_5h_warning_pct: 0.75,
            quota_5h_critical_pct: 0.85,
            quota_weekly_warning_pct: 0.75,
            quota_weekly_critical_pct: 0.85,
            cross_window_findings: false,
            identity_drift_findings: false,
        };
        let s = SignalSet {
            task_type: TaskType::Review,
            ..SignalSet::default()
        };
        let recs = eval_auto_memory(&id, &s, &gates);
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "auto-memory: route to MDR / CURRENT_STATE"),
            "Low-confidence pane must suppress the G-5 rec (unreliable task_type)"
        );
    }

    #[test]
    fn suppressed_when_operator_attention_is_on_a_prompt_or_input_wait() {
        // Don't pile onto a pane that's already blocking on operator
        // attention — the auto-memory guide would be noise compared
        // to the input-wait / permission-prompt alert.
        let s_input = SignalSet {
            task_type: TaskType::Review,
            idle_state: Some(IdleCause::InputWait),
            ..SignalSet::default()
        };
        assert!(
            eval_auto_memory(&id_high(), &s_input, &gates_default()).is_empty(),
            "InputWait takes priority over G-5"
        );
        let s_perm = SignalSet {
            task_type: TaskType::SessionResume,
            idle_state: Some(IdleCause::PermissionWait),
            ..SignalSet::default()
        };
        assert!(
            eval_auto_memory(&id_high(), &s_perm, &gates_default()).is_empty(),
            "PermissionWait takes priority over G-5"
        );
    }
}
