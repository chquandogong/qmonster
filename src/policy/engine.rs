use crate::domain::identity::ResolvedIdentity;
use crate::domain::recommendation::{Recommendation, RequestedEffect};
use crate::domain::signal::{IdleCause, SignalSet};
use crate::policy::gates::PolicyGates;
use crate::policy::rules::eval_alerts;

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
        gates: &PolicyGates,
        last_idle_state: Option<IdleCause>,
        recent_token_samples: &[crate::store::TokenSample],
    ) -> EvalOutput {
        let mut recs = eval_alerts(id, signals);
        recs.extend(crate::policy::rules::advisories::eval_advisories(
            id, signals, gates,
        ));
        recs.extend(crate::policy::rules::profiles::eval_profiles(
            id, signals, gates,
        ));
        recs.extend(crate::policy::rules::auto_memory::eval_auto_memory(
            id, signals, gates,
        ));
        recs.extend(crate::policy::rules::agent_memory::eval_agent_memory(
            id, signals, gates,
        ));
        recs.extend(crate::policy::rules::cache::eval_cache(
            id,
            signals,
            gates,
            recent_token_samples,
        ));
        recs.extend(crate::policy::rules::idle::eval_idle_transition(
            id,
            signals,
            last_idle_state,
        ));
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
        // Phase 5 P5-2 (v1.9.2): every strong recommendation whose
        // `suggested_command` names an in-pane slash-command (e.g.
        // `/compact`) graduates from a copy-pasteable hint into a
        // structured `PromptSendProposed` proposal. The proposal rides
        // alongside the rec in `effects`; the UI layer pairs the two
        // so an operator can confirm (P5-2 audit) or dismiss the send
        // without scrolling to the strong-rec slot. Actual tmux
        // send-keys execution is P5-3 — the proposal stays inert at
        // the dispatch layer (see `app::event_loop::deliver_effects`).
        for rec in &recs {
            if rec.is_strong
                && let Some(cmd) = rec.suggested_command.as_ref()
                && cmd.starts_with('/')
            {
                let pane_id = id.identity.pane_id.clone();
                let proposal_id = format!("{pane_id}:{cmd}");
                effects.push(RequestedEffect::PromptSendProposed {
                    target_pane_id: pane_id,
                    slash_command: cmd.clone(),
                    proposal_id,
                });
            }
        }
        EvalOutput {
            recommendations: recs,
            effects,
        }
    }

    pub fn evaluate_cross_pane(
        &self,
        panes: &[PaneView<'_>],
        gates: &PolicyGates,
    ) -> Vec<crate::domain::recommendation::CrossPaneFinding> {
        crate::policy::rules::concurrent::eval_concurrent(panes, gates)
    }
}

/// Read-only view over one pane's current state, used by cross-pane
/// rules. Built upstream by `app::event_loop` from the per-pane
/// report; never constructed inside `policy/`.
///
/// `window_label` is the operator-visible `session:window` string the
/// pane lives in (Phase D D1 v1.17.0). An empty string means "window
/// unknown" — the cross-window rule treats empty labels as the same
/// implicit group, so the same-window concurrent-work rule still fires
/// on legacy tests that omit the field.
#[derive(Debug, Clone, Copy)]
pub struct PaneView<'a> {
    pub identity: &'a ResolvedIdentity,
    pub signals: &'a SignalSet,
    pub current_path: &'a str,
    pub window_label: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
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
            identity_confidence: IdentityConfidence::High,
            ..PolicyGates::default()
        }
    }

    #[test]
    fn engine_runs_alert_rules() {
        // idle::eval_idle_transition fires a Warning rec on None→InputWait.
        let s = SignalSet {
            idle_state: Some(crate::domain::signal::IdleCause::InputWait),
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates(), None, &[]);
        assert!(!out.recommendations.is_empty());
    }

    #[test]
    fn engine_produces_notify_effect_for_input_wait() {
        // idle::eval_idle_transition fires Warning on None→InputWait;
        // engine promotes Warning+ recs to the Notify effect.
        let s = SignalSet {
            idle_state: Some(crate::domain::signal::IdleCause::InputWait),
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates(), None, &[]);
        assert!(
            out.effects
                .contains(&crate::domain::recommendation::RequestedEffect::Notify)
        );
    }

    #[test]
    fn strong_rec_with_slash_suggested_command_graduates_to_prompt_send_proposal() {
        // P5-2 producer contract: context_pressure_warning fires as a
        // strong rec with `suggested_command = Some("/compact")` on
        // context_pressure >= 0.75 && < 0.85. Engine MUST emit a
        // matching `PromptSendProposed` effect targeting the source
        // pane, carrying the same slash command.
        let s = SignalSet {
            context_pressure: Some(crate::domain::signal::MetricValue {
                value: 0.80,
                source_kind: crate::domain::origin::SourceKind::Estimated,
                confidence: None,
                provider: None,
            }),
            ..SignalSet::default()
        };
        let id = id(IdentityConfidence::High);
        let out = Engine.evaluate(&id, &s, &gates(), None, &[]);

        let strong_rec = out
            .recommendations
            .iter()
            .find(|r| r.is_strong)
            .expect("context_pressure_warning is_strong rec must fire at 0.80");
        assert_eq!(strong_rec.suggested_command.as_deref(), Some("/compact"));

        let proposal = out
            .effects
            .iter()
            .find_map(|e| match e {
                crate::domain::recommendation::RequestedEffect::PromptSendProposed {
                    target_pane_id,
                    slash_command,
                    proposal_id,
                } => Some((
                    target_pane_id.as_str(),
                    slash_command.as_str(),
                    proposal_id.as_str(),
                )),
                _ => None,
            })
            .expect("strong rec with slash suggested_command must produce a PromptSendProposed");
        assert_eq!(
            (proposal.0, proposal.1),
            ("%1", "/compact"),
            "proposal must target the source pane and carry the strong rec's slash command verbatim"
        );
        // P5-3: proposal_id is "{pane_id}:{slash_command}" — stable key.
        assert_eq!(
            proposal.2, "%1:/compact",
            "proposal_id must be stable '{{pane_id}}:{{slash_command}}' format"
        );
    }

    #[test]
    fn non_strong_rec_with_slash_suggested_command_does_not_graduate() {
        // Any rec emitting `/…` as a copy-pasteable hint but NOT marked
        // is_strong stays in the UI hint channel only. Only the
        // strong/checkpoint-class recs graduate to proposals.
        // idle::eval_idle_transition fires `pane-state` at Warning severity
        // with is_strong=false — no proposal should appear.
        let s = SignalSet {
            idle_state: Some(crate::domain::signal::IdleCause::InputWait),
            ..SignalSet::default()
        };
        let out = Engine.evaluate(&id(IdentityConfidence::High), &s, &gates(), None, &[]);
        let any_proposal = out.effects.iter().any(|e| {
            matches!(
                e,
                crate::domain::recommendation::RequestedEffect::PromptSendProposed { .. }
            )
        });
        assert!(
            !any_proposal,
            "non-strong recs must not produce prompt-send proposals"
        );
    }

    #[test]
    fn healthy_pane_produces_no_prompt_send_proposal() {
        // Negative baseline: a healthy pane (Severity::Good profile
        // rec only) has no strong rec and therefore no proposal.
        let s = SignalSet::default();
        let out = Engine.evaluate(&id(IdentityConfidence::High), &s, &gates(), None, &[]);
        let any_proposal = out.effects.iter().any(|e| {
            matches!(
                e,
                crate::domain::recommendation::RequestedEffect::PromptSendProposed { .. }
            )
        });
        assert!(
            !any_proposal,
            "healthy pane must not emit prompt-send proposals"
        );
    }

    #[test]
    fn log_storm_also_requests_archive_local() {
        let s = SignalSet {
            log_storm: true,
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates(), None, &[]);
        assert!(
            out.effects
                .contains(&crate::domain::recommendation::RequestedEffect::ArchiveLocal)
        );
    }

    #[test]
    fn non_storm_signal_does_not_request_archive() {
        let s = SignalSet {
            idle_state: Some(IdleCause::InputWait),
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates(), None, &[]);
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
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates(), None, &[]);
        assert!(
            !out.effects
                .contains(&crate::domain::recommendation::RequestedEffect::SensitiveNotImplemented)
        );
    }

    #[test]
    fn notify_effect_fires_only_for_warning_or_higher() {
        // idle::eval_idle_transition fires Warning on None→InputWait, which
        // triggers the Notify effect gate (Warning+ threshold).
        let s = SignalSet {
            idle_state: Some(crate::domain::signal::IdleCause::InputWait),
            ..SignalSet::default()
        };
        let eng = Engine;
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates(), None, &[]);
        assert!(
            out.effects
                .contains(&crate::domain::recommendation::RequestedEffect::Notify),
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
        let out = eng.evaluate(&id(IdentityConfidence::High), &s, &gates(), None, &[]);
        assert!(
            !out.effects
                .contains(&crate::domain::recommendation::RequestedEffect::Notify),
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
        assert!(eng.evaluate_cross_pane(&views, &gates()).is_empty());
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
            window_label: "",
        }];
        assert!(eng.evaluate_cross_pane(&views, &gates()).is_empty());
    }

    #[test]
    fn evaluate_cross_pane_uses_concurrent_rule() {
        use crate::domain::identity::{PaneIdentity, Provider, Role};
        let id_a = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        };
        let id_b = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 2,
                role: Role::Main,
                pane_id: "%2".into(),
            },
            confidence: IdentityConfidence::High,
        };
        let s = SignalSet {
            output_chars: 800,
            git_branch: Some(crate::domain::signal::MetricValue::new(
                "main".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            ..SignalSet::default()
        };
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
        ];
        let findings = Engine.evaluate_cross_pane(&views, &gates());
        assert_eq!(findings.len(), 1);
    }
}
