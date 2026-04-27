use crate::domain::identity::{ResolvedIdentity, Role};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::{RuntimeFact, RuntimeFactKind, SignalSet};
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

    if let Some(rec) = quota_pressure_critical(id, signals, gates) {
        out.push(rec);
    } else if let Some(rec) = quota_pressure_warning(id, signals, gates) {
        out.push(rec);
    }

    if let Some(rec) = cost_pressure_critical(id, signals, gates) {
        out.push(rec);
    } else if let Some(rec) = cost_pressure_warning(id, signals, gates) {
        out.push(rec);
    }

    if let Some(rec) = security_posture_advisory(id, signals, gates) {
        out.push(rec);
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
        suggested_command: Some("tmux capture-pane -pS -2000 > ~/.qmonster/archive/$(date +%F)-<pane_id>.log".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
    })
}

fn aggressive_log_storm() -> Recommendation {
    Recommendation {
        action: "aggressive: drop non-essential ingress",
        reason: "quota-tight: suppress low-value ingress lines; keep only error/warn markers"
            .into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Heuristic,
        suggested_command: Some(
            "# edit config/qmonster.toml: [logging] sensitivity = \"minimal\"".into(),
        ),
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
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
    let triggers_fired = matches!(
        signals.task_type,
        crate::domain::signal::TaskType::CodeExploration
    ) || signals.verbose_answer
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
        suggested_command: None, // workflow advice; no single command captures "graph navigation"
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
    })
}

fn context_pressure_warning(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let v = signals.context_pressure.as_ref()?.value;
    if !(gates.context_warning_pct..gates.context_critical_pct).contains(&v) {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "context-pressure: checkpoint",
        reason:
            "context warming — checkpoint first, archive large results, only then consider /compact"
                .into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Estimated,
        suggested_command: Some("/compact".into()),
        side_effects: vec![],
        is_strong: true,
        next_step: Some("press 's' to snapshot first, then archive large results".into()),
        profile: None,
    })
}

fn aggressive_context_pressure_warning() -> Recommendation {
    Recommendation {
        action: "aggressive: terse profile + archive",
        reason: "quota-tight: apply terse output profile and archive anything >500 chars".into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Heuristic,
        suggested_command: Some("# edit config/qmonster.toml: [token] strategy = \"terse\"".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
    }
}

fn context_pressure_critical(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let v = signals.context_pressure.as_ref()?.value;
    if v < gates.context_critical_pct {
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
        suggested_command: Some("/compact".into()),
        side_effects: vec![],
        is_strong: true,
        next_step: Some("press 's' to snapshot + archive now, before running /compact".into()),
        profile: None,
    })
}

fn aggressive_context_pressure_critical() -> Recommendation {
    Recommendation {
        action: "aggressive: clamp output, archive all",
        reason: "quota-tight critical: clamp max-output tokens and archive all non-trivial panes".into(),
        severity: Severity::Risk,
        source_kind: SourceKind::Heuristic,
        suggested_command: Some("# edit config/qmonster.toml: [token] strategy = \"terse\" + [logging] sensitivity = \"minimal\"".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
    }
}

/// quota_pressure gradient advisory (post-v1.15.10). Built on the
/// quota_pressure metric shipped in v1.15.8 and the v1.15.10 honesty
/// fix that limits LimitHit to the validated quota column. Operators
/// should be aware before quota hits 100% so they can pace work or
/// switch model; rate-limited quotas reset on the provider's schedule
/// and cannot be addressed by `/compact` (unlike context_pressure).
fn quota_pressure_warning(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let v = signals.quota_pressure.as_ref()?.value;
    if !(gates.quota_warning_pct..gates.quota_critical_pct).contains(&v) {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "quota-pressure: pace",
        reason:
            "provider quota approaching limit — pace work and prepare to checkpoint before LimitHit"
                .into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Estimated,
        // No single runnable command: rate-limited quotas reset on the
        // provider's schedule, not by a slash command. Operator picks
        // between waiting, switching account/model, or pausing the pane.
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: Some(
            "press 's' to snapshot now, then pace prompts or switch to a less rate-limited model"
                .into(),
        ),
        profile: None,
    })
}

fn quota_pressure_critical(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let v = signals.quota_pressure.as_ref()?.value;
    if v < gates.quota_critical_pct {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "quota-pressure: act now",
        reason: "provider quota near critical — checkpoint and stop new prompts before LimitHit"
            .into(),
        severity: Severity::Risk,
        source_kind: SourceKind::Estimated,
        // Same constraint as the warning variant: no single runnable
        // command resolves a rate-limit; operator must change behaviour.
        suggested_command: None,
        side_effects: vec![],
        is_strong: true,
        next_step: Some(
            "press 's' to snapshot + archive, then stop or switch account/model before quota hits 100%"
                .into(),
        ),
        profile: None,
    })
}

/// v1.15.14: cost_pressure gradient advisories. Mirrors the
/// quota_pressure pattern but for the operator-visible USD spend
/// per session. Cost has no provider-emitted "100% threshold" to
/// trigger LimitHit on, so the gradient warnings are the only
/// surface for cost awareness today.
///
/// v1.15.16: thresholds now live in `[cost]` config (per-provider
/// overrides supported) and are resolved into `PolicyGates` upstream
/// by `app::event_loop`. The defaults match the v1.15.14 baseline
/// when the operator does not override; per-provider overrides let
/// Claude / Gemini panes use thresholds appropriate to their pricing
/// tier (see `app::config::CostConfig` for the recommended defaults).
///
/// `cost_usd` is currently populated only on Codex panes (Claude
/// has no input-token source on tail; Gemini's status table does
/// not expose cost). When a future provider populates `cost_usd`,
/// these rules fire automatically — no per-provider gating change.
fn cost_pressure_warning(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let v = signals.cost_usd.as_ref()?.value;
    if !(gates.cost_warning_usd..gates.cost_critical_usd).contains(&v) {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "cost-pressure: pace",
        reason:
            "session cost is climbing — pace prompts and consider archiving large outputs"
                .into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Estimated,
        // No single runnable command reduces accumulated session
        // cost. Operator picks between pacing prompts, switching to
        // a cheaper model, or ending the session.
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: Some(
            "consider switching to a cheaper model, archiving long outputs, or wrapping up the session"
                .into(),
        ),
        profile: None,
    })
}

fn cost_pressure_critical(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    let v = signals.cost_usd.as_ref()?.value;
    if v < gates.cost_critical_usd {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    Some(Recommendation {
        action: "cost-pressure: act now",
        reason: "session cost crossed the critical threshold — pause new prompts and decide whether to continue"
            .into(),
        severity: Severity::Risk,
        source_kind: SourceKind::Estimated,
        suggested_command: None,
        side_effects: vec![],
        is_strong: true,
        next_step: Some(
            "press 's' to snapshot + archive, then pause or switch model before cost climbs further"
                .into(),
        ),
        profile: None,
    })
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
        reason: "review pane is verbose — consider Caveman / claude-token-efficient terse profile"
            .into(),
        severity: Severity::Concern,
        source_kind: SourceKind::Heuristic,
        suggested_command: Some("# edit config/qmonster.toml: [review] style = \"terse\"".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
    })
}

fn aggressive_verbose_review() -> Recommendation {
    Recommendation {
        action: "aggressive: strip attribution",
        reason: "quota-tight: drop attribution footer and preamble on review output".into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Heuristic,
        suggested_command: Some(
            "# edit ~/.claude/settings.json: \"attribution\": { \"commit\": false }".into(),
        ),
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
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
        suggested_command: Some("# set quota_tight = true under [token] in config/qmonster.toml".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
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
        suggested_command: None, // install/config step varies by agent stack
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
    })
}

fn security_posture_advisory(
    _id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if !gates.security_posture_advisories {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }

    let risky: Vec<&RuntimeFact> = signals
        .runtime_facts
        .iter()
        .filter(|fact| is_permissive_runtime_fact(fact))
        .collect();
    if risky.is_empty() {
        return None;
    }

    let summary = risky
        .iter()
        .map(|fact| {
            format!(
                "{}={} [{}]",
                runtime_fact_kind_label(fact.kind),
                fact.value,
                fact.source_kind
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    Some(Recommendation {
        action: "security-posture: review permissive runtime",
        reason: format!(
            "permissive runtime posture observed: {summary}; keep it only when intentional"
        ),
        severity: Severity::Concern,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: None, // provider-specific toggle; no single safe command spans all CLIs
        side_effects: vec![],
        is_strong: false,
        next_step: Some(
            "review YOLO / bypass / sandbox settings; disable permissive mode unless this pane is intentionally trusted"
                .into(),
        ),
        profile: None,
    })
}

fn is_permissive_runtime_fact(fact: &RuntimeFact) -> bool {
    let value = fact.value.to_ascii_lowercase();
    match fact.kind {
        RuntimeFactKind::PermissionMode => {
            value.contains("bypass") || value.contains("full access")
        }
        RuntimeFactKind::AutoMode => value.contains("yolo"),
        RuntimeFactKind::Sandbox => {
            value.contains("danger-full-access") || value.contains("no sandbox")
        }
        _ => false,
    }
}

fn runtime_fact_kind_label(kind: RuntimeFactKind) -> &'static str {
    match kind {
        RuntimeFactKind::PermissionMode => "permission",
        RuntimeFactKind::AutoMode => "mode",
        RuntimeFactKind::Sandbox => "sandbox",
        RuntimeFactKind::AllowedDirectory => "dir",
        RuntimeFactKind::AgentConfig => "agents",
        RuntimeFactKind::LoadedTool => "tool",
        RuntimeFactKind::LoadedSkill => "skill",
        RuntimeFactKind::LoadedPlugin => "plugin",
        RuntimeFactKind::RestrictedTool => "restricted-tool",
    }
}

fn aggressive_repeated_cache_suggest() -> Recommendation {
    Recommendation {
        action: "aggressive: dedupe + hash",
        reason: "quota-tight: enable per-pane result-hash dedupe".into(),
        severity: Severity::Warning,
        source_kind: SourceKind::Heuristic,
        suggested_command: None, // config varies by agent stack
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind as SK;
    use crate::domain::signal::{MetricValue, RuntimeFact, RuntimeFactKind};

    fn id_high(role: Role) -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    fn id_low(role: Role) -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::Low,
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
        }
    }

    fn gates_security_posture() -> PolicyGates {
        PolicyGates {
            security_posture_advisories: true,
            ..gates_default()
        }
    }

    #[test]
    fn log_storm_advisory_fires_with_heuristic_source_kind() {
        let id = id_high(Role::Main);
        let s = SignalSet {
            log_storm: true,
            ..SignalSet::default()
        };
        let recs = eval_advisories(&id, &s, &gates_default());
        let adv = recs
            .iter()
            .find(|r| r.action == "log-storm: ingress filter + summary")
            .expect("log_storm_advisory must fire");
        assert_eq!(adv.source_kind, SourceKind::Heuristic);
        assert_eq!(adv.severity, Severity::Concern);
    }

    #[test]
    fn security_posture_advisory_is_opt_in_only() {
        let id = id_high(Role::Main);
        let s = SignalSet {
            runtime_facts: vec![RuntimeFact::new(
                RuntimeFactKind::AutoMode,
                "YOLO mode",
                SourceKind::ProviderOfficial,
            )],
            ..SignalSet::default()
        };

        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "security-posture: review permissive runtime"),
            "security posture advisory must stay badge-only until explicitly enabled"
        );
    }

    #[test]
    fn security_posture_advisory_fires_on_permissive_runtime_facts_when_enabled() {
        let id = id_high(Role::Main);
        let s = SignalSet {
            runtime_facts: vec![
                RuntimeFact::new(
                    RuntimeFactKind::PermissionMode,
                    "Full Access",
                    SourceKind::ProviderOfficial,
                ),
                RuntimeFact::new(
                    RuntimeFactKind::AutoMode,
                    "YOLO Ctrl+Y",
                    SourceKind::Heuristic,
                ),
                RuntimeFact::new(
                    RuntimeFactKind::Sandbox,
                    "danger-full-access",
                    SourceKind::ProviderOfficial,
                ),
            ],
            ..SignalSet::default()
        };

        let recs = eval_advisories(&id, &s, &gates_security_posture());
        let adv = recs
            .iter()
            .find(|r| r.action == "security-posture: review permissive runtime")
            .expect("opt-in security posture advisory must fire");
        assert_eq!(adv.severity, Severity::Concern);
        assert_eq!(adv.source_kind, SourceKind::ProjectCanonical);
        assert!(adv.suggested_command.is_none());
        assert!(
            adv.reason
                .contains("permission=Full Access [ProviderOfficial]")
        );
        assert!(adv.reason.contains("mode=YOLO Ctrl+Y [Heuristic]"));
        assert!(
            adv.reason
                .contains("sandbox=danger-full-access [ProviderOfficial]")
        );
        assert!(
            adv.next_step
                .as_deref()
                .unwrap_or_default()
                .contains("disable permissive mode")
        );
    }

    #[test]
    fn security_posture_advisory_suppressed_on_low_identity_confidence() {
        let id = id_low(Role::Main);
        let s = SignalSet {
            runtime_facts: vec![RuntimeFact::new(
                RuntimeFactKind::Sandbox,
                "no sandbox",
                SourceKind::ProviderOfficial,
            )],
            ..SignalSet::default()
        };

        let gates = PolicyGates {
            identity_confidence: IdentityConfidence::Low,
            ..gates_security_posture()
        };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "security-posture: review permissive runtime")
        );
    }

    #[test]
    fn security_posture_advisory_ignores_non_permissive_runtime_facts() {
        let id = id_high(Role::Main);
        let s = SignalSet {
            runtime_facts: vec![
                RuntimeFact::new(
                    RuntimeFactKind::PermissionMode,
                    "Default",
                    SourceKind::ProviderOfficial,
                ),
                RuntimeFact::new(
                    RuntimeFactKind::Sandbox,
                    "workspace-write",
                    SourceKind::ProviderOfficial,
                ),
            ],
            ..SignalSet::default()
        };

        let recs = eval_advisories(&id, &s, &gates_security_posture());
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "security-posture: review permissive runtime")
        );
    }

    #[test]
    fn code_exploration_fires_on_verbose_main_role() {
        let id = id_high(Role::Main);
        let s = SignalSet {
            verbose_answer: true,
            ..SignalSet::default()
        };
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(
            recs.iter()
                .any(|r| r.action == "code-exploration: graph/symbol")
        );
    }

    #[test]
    fn code_exploration_suppressed_on_low_identity_confidence() {
        let id = id_low(Role::Main);
        let s = SignalSet {
            verbose_answer: true,
            ..SignalSet::default()
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
        };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "code-exploration: graph/symbol")
        );
    }

    fn pressure(v: f32) -> SignalSet {
        SignalSet {
            context_pressure: Some(MetricValue::new(v, SK::Estimated)),
            ..SignalSet::default()
        }
    }

    fn quota(v: f32) -> SignalSet {
        SignalSet {
            quota_pressure: Some(MetricValue::new(v, SK::ProviderOfficial)),
            ..SignalSet::default()
        }
    }

    #[test]
    fn quota_pressure_warning_at_0_75() {
        let id = id_high(Role::Main);
        let s = quota(0.78);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(recs.iter().any(|r| r.action == "quota-pressure: pace"));
        assert!(!recs.iter().any(|r| r.action == "quota-pressure: act now"));
        let warn = recs
            .iter()
            .find(|r| r.action == "quota-pressure: pace")
            .unwrap();
        assert_eq!(warn.severity, Severity::Warning);
        assert_eq!(
            warn.source_kind,
            SourceKind::Estimated,
            "the quota metric is ProviderOfficial, but the 75% advisory threshold is a Qmonster estimate"
        );
    }

    #[test]
    fn quota_pressure_critical_at_0_85() {
        let id = id_high(Role::Main);
        let s = quota(0.88);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(recs.iter().any(|r| r.action == "quota-pressure: act now"));
        assert!(!recs.iter().any(|r| r.action == "quota-pressure: pace"));
        let crit = recs
            .iter()
            .find(|r| r.action == "quota-pressure: act now")
            .unwrap();
        assert_eq!(crit.severity, Severity::Risk);
        assert_eq!(
            crit.source_kind,
            SourceKind::Estimated,
            "the quota metric is ProviderOfficial, but the 85% advisory threshold is a Qmonster estimate"
        );
    }

    #[test]
    fn quota_pressure_below_threshold_does_not_fire() {
        let id = id_high(Role::Main);
        let s = quota(0.50);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(!recs.iter().any(|r| r.action.starts_with("quota-pressure")));
    }

    #[test]
    fn quota_pressure_absent_does_not_fire() {
        let id = id_high(Role::Main);
        let s = SignalSet::default();
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(!recs.iter().any(|r| r.action.starts_with("quota-pressure")));
    }

    #[test]
    fn quota_pressure_suppressed_on_low_identity_confidence() {
        let id = id_low(Role::Main);
        let s = quota(0.92);
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
        };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(
            !recs.iter().any(|r| r.action.starts_with("quota-pressure")),
            "quota_pressure_* must respect the IdentityConfidence gate"
        );
    }

    #[test]
    fn context_pressure_warning_at_0_75() {
        let id = id_high(Role::Main);
        let s = pressure(0.78);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(
            recs.iter()
                .any(|r| r.action == "context-pressure: checkpoint")
        );
        assert!(!recs.iter().any(|r| r.action == "context-pressure: act now"));
    }

    #[test]
    fn context_pressure_critical_at_0_85() {
        let id = id_high(Role::Main);
        let s = pressure(0.88);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(recs.iter().any(|r| r.action == "context-pressure: act now"));
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "context-pressure: checkpoint")
        );
    }

    #[test]
    fn context_pressure_suppressed_on_low_identity_confidence() {
        let id = id_low(Role::Main);
        let s = pressure(0.92);
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
        };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(
            !recs
                .iter()
                .any(|r| r.action.starts_with("context-pressure")),
            "Codex #2: context_pressure_* must respect the gate"
        );
    }

    #[test]
    fn verbose_review_requires_review_role() {
        let s = SignalSet {
            verbose_answer: true,
            ..SignalSet::default()
        };
        let rev = id_high(Role::Review);
        let main = id_high(Role::Main);

        let recs_rev = eval_advisories(&rev, &s, &gates_default());
        assert!(
            recs_rev
                .iter()
                .any(|r| r.action == "verbose-review: terse profile")
        );

        let recs_main = eval_advisories(&main, &s, &gates_default());
        assert!(
            !recs_main
                .iter()
                .any(|r| r.action == "verbose-review: terse profile"),
            "verbose_review must NOT fire on role=Main"
        );
    }

    #[test]
    fn quota_tight_nudge_fires_only_when_gate_off_and_pressure_high() {
        let id = id_high(Role::Main);
        let s = pressure(0.92);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(
            recs.iter()
                .any(|r| r.action == "quota-tight: consider enabling")
        );
    }

    #[test]
    fn quota_tight_nudge_never_fires_when_gate_on() {
        let id = id_high(Role::Main);
        let s = pressure(0.92);
        let gates = PolicyGates {
            quota_tight: true,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "quota-tight: consider enabling")
        );
    }

    #[test]
    fn quota_tight_nudge_fires_regardless_of_identity_confidence() {
        let id = id_low(Role::Main);
        let s = pressure(0.92);
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
        };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(
            recs.iter()
                .any(|r| r.action == "quota-tight: consider enabling"),
            "quota_tight_nudge is Qmonster-config-level, not provider-flavored"
        );
    }

    #[test]
    fn repeated_cache_suggest_fires_on_repeated_output() {
        let id = id_high(Role::Main);
        let s = SignalSet {
            repeated_output: true,
            ..SignalSet::default()
        };
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(
            recs.iter()
                .any(|r| r.action == "repeated-output: result-hash cache")
        );
    }

    #[test]
    fn aggressive_variants_fire_only_when_quota_tight_gate_open() {
        let id = id_high(Role::Review);
        let s = SignalSet {
            log_storm: true,
            verbose_answer: true,
            repeated_output: true,
            context_pressure: Some(MetricValue::new(0.92, SK::Estimated)),
            ..SignalSet::default()
        };
        let gates = PolicyGates {
            quota_tight: true,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let recs = eval_advisories(&id, &s, &gates);
        let aggressive_actions: Vec<&str> = recs
            .iter()
            .map(|r| r.action)
            .filter(|a| a.starts_with("aggressive:"))
            .collect();
        assert!(aggressive_actions.contains(&"aggressive: drop non-essential ingress"));
        assert!(aggressive_actions.contains(&"aggressive: clamp output, archive all"));
        assert!(aggressive_actions.contains(&"aggressive: strip attribution"));
        assert!(aggressive_actions.contains(&"aggressive: dedupe + hash"));
        // Note: "aggressive: terse profile + archive" (for C-warning) does
        // NOT fire here because C-critical supersedes C-warning.
    }

    #[test]
    fn aggressive_context_pressure_warning_fires_at_moderate_pressure() {
        let id = id_high(Role::Main);
        let s = SignalSet {
            context_pressure: Some(MetricValue::new(0.80, SK::Estimated)),
            ..SignalSet::default()
        };
        let gates = PolicyGates {
            quota_tight: true,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let recs = eval_advisories(&id, &s, &gates);
        let actions: Vec<&str> = recs.iter().map(|r| r.action).collect();
        assert!(actions.contains(&"context-pressure: checkpoint"));
        assert!(
            actions.contains(&"aggressive: terse profile + archive"),
            "C-warning aggressive must fire when pressure in [0.75, 0.85) and gate is on"
        );
    }

    #[test]
    fn aggressive_variant_never_fires_when_gate_off() {
        let id = id_high(Role::Review);
        let s = SignalSet {
            log_storm: true,
            verbose_answer: true,
            repeated_output: true,
            context_pressure: Some(MetricValue::new(0.92, SK::Estimated)),
            ..SignalSet::default()
        };
        let gates = PolicyGates {
            quota_tight: false,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let recs = eval_advisories(&id, &s, &gates);
        let any_aggressive = recs.iter().any(|r| r.action.starts_with("aggressive:"));
        assert!(
            !any_aggressive,
            "S3: aggressive variants never fire when gate is off"
        );
    }

    #[test]
    fn log_storm_advisory_carries_tmux_capture_suggested_command() {
        let id = id_high(Role::Main);
        let s = SignalSet {
            log_storm: true,
            ..SignalSet::default()
        };
        let recs = eval_advisories(&id, &s, &gates_default());
        let adv = recs
            .iter()
            .find(|r| r.action == "log-storm: ingress filter + summary")
            .expect("log_storm_advisory fires");
        let cmd = adv
            .suggested_command
            .as_deref()
            .expect("suggested_command present");
        assert!(cmd.contains("tmux capture-pane"), "got: {cmd}");
        assert!(cmd.contains("~/.qmonster/archive/"), "got: {cmd}");
    }

    #[test]
    fn context_pressure_warning_carries_runnable_compact_with_snapshot_next_step() {
        let id = id_high(Role::Main);
        let s = pressure(0.80);
        let recs = eval_advisories(&id, &s, &gates_default());
        let adv = recs
            .iter()
            .find(|r| r.action == "context-pressure: checkpoint")
            .expect("C-warning fires");
        // Codex v1.7.3 finding #1: suggested_command must be runnable on
        // a single surface. "/compact" is a pure in-pane slash command.
        assert_eq!(
            adv.suggested_command.as_deref(),
            Some("/compact"),
            "suggested_command must be runnable in-pane; mixed-mode prose belongs in next_step"
        );
        // Codex v1.7.3 finding #2: contract must lock ordering — snapshot
        // step precedes the run step. Structurally enforced by putting
        // prose in next_step (preamble) and command in suggested_command
        // (execution).
        let step = adv
            .next_step
            .as_deref()
            .expect("strong rec must carry snapshot next_step");
        assert!(
            step.contains("snapshot"),
            "next_step must describe the snapshot precondition. got: {step}"
        );
        assert!(
            step.contains("'s'") || step.contains("snapshot"),
            "next_step should hint the TUI key `s` or the snapshot action. got: {step}"
        );
    }

    #[test]
    fn context_pressure_critical_carries_runnable_compact_with_snapshot_next_step() {
        let id = id_high(Role::Main);
        let s = pressure(0.92);
        let recs = eval_advisories(&id, &s, &gates_default());
        let adv = recs
            .iter()
            .find(|r| r.action == "context-pressure: act now")
            .expect("C-critical fires");
        assert_eq!(
            adv.suggested_command.as_deref(),
            Some("/compact"),
            "suggested_command must be runnable in-pane; mixed-mode prose belongs in next_step"
        );
        let step = adv
            .next_step
            .as_deref()
            .expect("strong rec must carry snapshot next_step");
        assert!(
            step.contains("snapshot"),
            "next_step must describe the snapshot precondition (strong rec). got: {step}"
        );
    }

    #[test]
    fn all_provider_flavored_advisories_suppressed_on_low_confidence() {
        let id = id_low(Role::Review);
        let s = SignalSet {
            log_storm: true,
            verbose_answer: true,
            repeated_output: true,
            context_pressure: Some(MetricValue::new(0.92, SK::Estimated)),
            ..SignalSet::default()
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
        };
        let recs = eval_advisories(&id, &s, &gates);
        let actions: Vec<&str> = recs.iter().map(|r| r.action).collect();
        // quota_tight_nudge fires — not provider-flavored.
        assert!(actions.contains(&"quota-tight: consider enabling"));
        // All provider-flavored advisories suppressed.
        assert!(!actions.contains(&"log-storm: ingress filter + summary"));
        assert!(!actions.contains(&"code-exploration: graph/symbol"));
        assert!(!actions.contains(&"context-pressure: act now"));
        assert!(!actions.contains(&"context-pressure: checkpoint"));
        assert!(!actions.contains(&"verbose-review: terse profile"));
        assert!(!actions.contains(&"repeated-output: result-hash cache"));
    }

    #[test]
    fn aggressive_verbose_review_suggests_attribution_edit() {
        let id = id_high(Role::Review);
        let s = SignalSet {
            verbose_answer: true,
            ..SignalSet::default()
        };
        let gates = PolicyGates {
            quota_tight: true,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let recs = eval_advisories(&id, &s, &gates);
        let adv = recs
            .iter()
            .find(|r| r.action == "aggressive: strip attribution")
            .expect("aggressive_verbose_review fires under quota_tight");
        let cmd = adv
            .suggested_command
            .as_deref()
            .expect("populated in v1.7.1");
        assert!(cmd.contains("attribution"), "got: {cmd}");
    }

    fn cost(v: f64) -> SignalSet {
        SignalSet {
            cost_usd: Some(MetricValue::new(v, SK::Estimated)),
            ..SignalSet::default()
        }
    }

    #[test]
    fn cost_pressure_warning_at_5_usd() {
        // v1.15.14: when accumulated session cost reaches the warning
        // threshold (default $5), surface a Warning-severity advisory.
        // Source is Estimated because the threshold is a Qmonster pick
        // (cost_usd itself is also Estimated — derived from token
        // counts × pricing).
        let id = id_high(Role::Main);
        let s = cost(7.50);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(recs.iter().any(|r| r.action == "cost-pressure: pace"));
        assert!(!recs.iter().any(|r| r.action == "cost-pressure: act now"));
        let warn = recs
            .iter()
            .find(|r| r.action == "cost-pressure: pace")
            .unwrap();
        assert_eq!(warn.severity, Severity::Warning);
        assert_eq!(warn.source_kind, SourceKind::Estimated);
        assert!(
            warn.suggested_command.is_none(),
            "session cost cannot be reduced by a slash command"
        );
    }

    #[test]
    fn cost_pressure_critical_at_20_usd() {
        let id = id_high(Role::Main);
        let s = cost(25.00);
        let recs = eval_advisories(&id, &s, &gates_default());
        let crit = recs
            .iter()
            .find(|r| r.action == "cost-pressure: act now")
            .expect("cost_pressure_critical must fire at $25");
        assert_eq!(crit.severity, Severity::Risk);
        assert!(
            crit.is_strong,
            "critical cost-pressure surfaces in CHECKPOINT slot"
        );
        assert!(crit.suggested_command.is_none());
        // Below-warning bound (cost-pressure: pace) must NOT also fire
        // since the critical rule consumes the same metric.
        assert!(
            !recs.iter().any(|r| r.action == "cost-pressure: pace"),
            "critical and warning are mutually exclusive on the same metric"
        );
    }

    #[test]
    fn cost_pressure_below_threshold_does_not_fire() {
        let id = id_high(Role::Main);
        let s = cost(2.50);
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(!recs.iter().any(|r| r.action.starts_with("cost-pressure")));
    }

    #[test]
    fn cost_pressure_absent_does_not_fire() {
        let id = id_high(Role::Main);
        let s = SignalSet::default();
        let recs = eval_advisories(&id, &s, &gates_default());
        assert!(!recs.iter().any(|r| r.action.starts_with("cost-pressure")));
    }

    #[test]
    fn cost_pressure_suppressed_on_low_identity_confidence() {
        let id = id_low(Role::Main);
        let s = cost(50.00);
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
        };
        let recs = eval_advisories(&id, &s, &gates);
        assert!(
            !recs.iter().any(|r| r.action.starts_with("cost-pressure")),
            "cost_pressure_* must respect the IdentityConfidence gate"
        );
    }

    #[test]
    fn cost_pressure_thresholds_track_per_provider_overrides() {
        // v1.15.16: when CostConfig has different thresholds for
        // different providers (e.g. Claude $10 / $30 vs Codex
        // $5 / $20), a $25 cost on a Claude pane stays in Warning
        // territory while the same $25 on a Codex pane is already
        // critical. The advisory rule reads thresholds from gates;
        // the gates are built per-pane upstream.
        let id = id_high(Role::Main);
        let s = cost(25.00);

        // Codex-style thresholds → $25 is past critical ($20).
        let codex_gates = PolicyGates {
            quota_tight: false,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let codex_recs = eval_advisories(&id, &s, &codex_gates);
        assert!(
            codex_recs
                .iter()
                .any(|r| r.action == "cost-pressure: act now"),
            "Codex threshold ($20) → $25 must trigger critical"
        );

        // Claude-style thresholds → $25 is between warning ($10) and
        // critical ($30); only Warning fires.
        let claude_gates = PolicyGates {
            quota_tight: false,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 10.0,
            cost_critical_usd: 30.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let claude_recs = eval_advisories(&id, &s, &claude_gates);
        assert!(
            claude_recs
                .iter()
                .any(|r| r.action == "cost-pressure: pace"),
            "Claude threshold ($10..$30) → $25 must trigger warning, not critical"
        );
        assert!(
            !claude_recs
                .iter()
                .any(|r| r.action == "cost-pressure: act now"),
            "Claude threshold ($30 critical) → $25 must NOT trigger critical"
        );

        // Gemini-style thresholds → $25 is past critical ($10).
        let gemini_gates = PolicyGates {
            quota_tight: false,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 3.0,
            cost_critical_usd: 10.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let gemini_recs = eval_advisories(&id, &s, &gemini_gates);
        assert!(
            gemini_recs
                .iter()
                .any(|r| r.action == "cost-pressure: act now"),
            "Gemini threshold ($10 critical) → $25 must trigger critical"
        );
    }

    #[test]
    fn context_pressure_thresholds_track_per_provider_overrides() {
        // v1.15.17: per-provider context_pressure thresholds. A
        // value that is "warning" under default 0.75/0.85 is "critical"
        // under tighter 0.60/0.75 (Gemini-leaning), and "below threshold"
        // under looser 0.85/0.95 (a hypothetical Claude override).
        let id = id_high(Role::Main);
        let s = pressure(0.78);

        // Default 0.75/0.85 → 0.78 is in [warning, critical) → Warning only.
        let default_gates = PolicyGates {
            quota_tight: false,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let default_recs = eval_advisories(&id, &s, &default_gates);
        assert!(
            default_recs
                .iter()
                .any(|r| r.action == "context-pressure: checkpoint"),
            "default 0.75/0.85 thresholds → 0.78 must trigger warning"
        );
        assert!(
            !default_recs
                .iter()
                .any(|r| r.action == "context-pressure: act now"),
            "default 0.75/0.85 thresholds → 0.78 must NOT trigger critical"
        );

        // Tighter 0.60/0.75 → 0.78 is past critical (0.75) → Critical fires.
        let tight_gates = PolicyGates {
            context_warning_pct: 0.60,
            context_critical_pct: 0.75,
            ..default_gates
        };
        let tight_recs = eval_advisories(&id, &s, &tight_gates);
        assert!(
            tight_recs
                .iter()
                .any(|r| r.action == "context-pressure: act now"),
            "tight 0.60/0.75 thresholds → 0.78 must trigger critical"
        );

        // Looser 0.85/0.95 → 0.78 is below warning (0.85) → no advisory.
        let loose_gates = PolicyGates {
            context_warning_pct: 0.85,
            context_critical_pct: 0.95,
            ..default_gates
        };
        let loose_recs = eval_advisories(&id, &s, &loose_gates);
        assert!(
            !loose_recs
                .iter()
                .any(|r| r.action.starts_with("context-pressure")),
            "loose 0.85/0.95 thresholds → 0.78 must NOT fire any context advisory"
        );
    }

    #[test]
    fn quota_pressure_thresholds_track_per_provider_overrides() {
        // v1.15.17: same per-provider override pattern for quota_pressure.
        // Today only Gemini surfaces a populated quota_pressure metric,
        // but the gating is uniform — any provider whose pane reports
        // quota_pressure inherits its [quota.<provider>] override.
        let id = id_high(Role::Main);
        let s = quota(0.78);

        // Default 0.75/0.85 → 0.78 is in [warning, critical) → Warning only.
        let default_gates = PolicyGates {
            quota_tight: false,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        };
        let default_recs = eval_advisories(&id, &s, &default_gates);
        assert!(
            default_recs
                .iter()
                .any(|r| r.action == "quota-pressure: pace"),
            "default 0.75/0.85 thresholds → 0.78 must trigger warning"
        );
        assert!(
            !default_recs
                .iter()
                .any(|r| r.action == "quota-pressure: act now"),
            "default 0.75/0.85 thresholds → 0.78 must NOT trigger critical"
        );

        // Tighter 0.60/0.75 → 0.78 is past critical (0.75) → Critical fires.
        let tight_gates = PolicyGates {
            quota_warning_pct: 0.60,
            quota_critical_pct: 0.75,
            ..default_gates
        };
        let tight_recs = eval_advisories(&id, &s, &tight_gates);
        assert!(
            tight_recs
                .iter()
                .any(|r| r.action == "quota-pressure: act now"),
            "tight 0.60/0.75 thresholds → 0.78 must trigger critical"
        );

        // Looser 0.85/0.95 → 0.78 is below warning (0.85) → no advisory.
        let loose_gates = PolicyGates {
            quota_warning_pct: 0.85,
            quota_critical_pct: 0.95,
            ..default_gates
        };
        let loose_recs = eval_advisories(&id, &s, &loose_gates);
        assert!(
            !loose_recs
                .iter()
                .any(|r| r.action.starts_with("quota-pressure")),
            "loose 0.85/0.95 thresholds → 0.78 must NOT fire any quota advisory"
        );
    }
}
