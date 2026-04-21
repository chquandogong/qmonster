use crate::domain::identity::{Provider, ResolvedIdentity, Role};
use crate::domain::origin::SourceKind;
use crate::domain::profile::{ProfileLever, ProviderProfile};
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::SignalSet;
use crate::policy::gates::{PolicyGates, allow_provider_specific};

/// Phase 4 provider-profile recommender. Each rule is a pure function
/// over `(identity, signals, gates)` and emits a `Recommendation` that
/// carries a named `ProviderProfile` bundle. Profile NAMES are
/// `ProjectCanonical`; levers inside are `ProviderOfficial` with
/// explicit citations. P4-1 ships `recommend_claude_default` only;
/// G-5 (auto-memory guide) and G-6 (side_effects on high-compression
/// profiles) land in follow-up slices P4-2 and P4-3.
pub fn eval_profiles(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    if let Some(rec) = recommend_claude_default(id, signals, gates) {
        out.push(rec);
    }
    out
}

/// `claude-default`: healthy-state baseline profile for a Claude main
/// pane. Fires only when identity is Claude main at ≥ Medium
/// confidence AND no active alerts / high context pressure / quota-
/// tight gate are present. Levers are copied from Claude Code docs
/// and labeled `ProviderOfficial` with per-lever citations. The
/// recommendation has no single-surface runnable command (applying a
/// profile is a multi-step settings edit), so `suggested_command` is
/// left `None` with a justification here; Phase 5 may revisit via a
/// manual prompt-send helper.
fn recommend_claude_default(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if id.identity.provider != Provider::Claude {
        return None;
    }
    if id.identity.role != Role::Main {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    if gates.quota_tight {
        // claude-default is the HEALTHY-state baseline; quota-tight
        // mode belongs to an aggressive-variant profile shipped in a
        // later Phase 4 slice.
        return None;
    }
    if signals.waiting_for_input
        || signals.permission_prompt
        || signals.log_storm
        || signals.error_hint
    {
        // Any active alert signal means the pane is NOT in a healthy
        // resting state; the profile rec would be noise.
        return None;
    }
    // High context pressure is handled by the Phase-3 strong recs,
    // not by baseline-profile tuning.
    if let Some(mv) = signals.context_pressure.as_ref()
        && mv.value >= 0.75
    {
        return None;
    }

    let profile = claude_default_profile();
    let reason = format!(
        "profile `{}`: apply {} ProviderOfficial levers for a healthy-state baseline main-pane session (see lever list below — each lever carries its own citation)",
        profile.name,
        profile.levers.len(),
    );
    let side_effects = profile.side_effects.clone();
    Some(Recommendation {
        action: "provider-profile: claude-default",
        reason,
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
        // No single runnable command: applying a profile is a multi-
        // key settings edit across ~/.claude/settings.json and env.
        // The structured `profile` payload below carries the three
        // lever keys/values/citations the UI renders — do NOT fold
        // those into suggested_command (Codex v1.8.1 finding #1).
        suggested_command: None,
        side_effects,
        is_strong: false,
        next_step: None,
        // v1.8.1 remediation: thread the structured ProviderProfile
        // through to the renderer so the ProjectCanonical bundle vs
        // ProviderOfficial lever authority split is visible end-to-
        // end (Codex Phase-4 P4-1 finding #1 closed).
        profile: Some(profile),
    })
}

fn claude_default_profile() -> ProviderProfile {
    ProviderProfile {
        name: "claude-default",
        levers: vec![
            ProfileLever {
                key: "CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS",
                value: "25000",
                citation: "Claude Code docs — environment variables, file-read budget",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "BASH_MAX_OUTPUT_LENGTH",
                value: "30000",
                citation: "Claude Code docs — environment variables, bash output cap",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "includeGitInstructions",
                value: "false",
                citation: "Claude Code docs — settings.json, reduces boilerplate on tight sessions",
                source_kind: SourceKind::ProviderOfficial,
            },
        ],
        // G-6 (side_effects population on high-compression profiles)
        // lands in a later Phase 4 slice; `claude-default` is a
        // healthy-state baseline with no expected side effects.
        side_effects: vec![],
        source_kind: SourceKind::ProjectCanonical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity};

    fn healthy_claude_main() -> ResolvedIdentity {
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
            identity_confidence: IdentityConfidence::High,
        }
    }

    #[test]
    fn recommend_claude_default_fires_with_provider_official_levers_on_healthy_claude_main() {
        let id = healthy_claude_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_default());

        let rec = recs
            .iter()
            .find(|r| r.action == "provider-profile: claude-default")
            .expect("claude-default profile rec fires on healthy Claude main pane");

        // Profile NAME is the project's abstraction — ProjectCanonical.
        assert_eq!(
            rec.source_kind,
            SourceKind::ProjectCanonical,
            "profile bundle NAME is our abstraction; individual levers keep ProviderOfficial inside"
        );
        // Severity is Good — a positive advisory that must NOT trigger
        // the Notify gate (which fires only for >= Warning).
        assert_eq!(
            rec.severity,
            Severity::Good,
            "healthy-state profile rec is a positive advisory, not an alert"
        );
        // Reason mentions the profile name AND cites ProviderOfficial authority.
        assert!(
            rec.reason.contains("claude-default"),
            "reason must name the profile: {}", rec.reason
        );
        assert!(
            rec.reason.contains("ProviderOfficial"),
            "reason must cite ProviderOfficial authority label: {}", rec.reason
        );
        // No single runnable command — applying a profile is multi-
        // key settings editing; justified None.
        assert!(
            rec.suggested_command.is_none(),
            "profile rec has no single-surface runnable command"
        );
    }

    #[test]
    fn recommend_claude_default_attaches_structured_profile_with_three_provider_official_levers() {
        // Codex v1.8.1 (P4-1 finding #1 closed): the structured
        // ProviderProfile bundle must reach the Recommendation payload
        // so the renderer can surface lever key/value/citation/source_kind.
        // This test fails if recommend_claude_default ever drops the
        // structured profile on the floor (the regression that shipped
        // in v1.8.0).
        let id = healthy_claude_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_default());
        let rec = recs
            .iter()
            .find(|r| r.action == "provider-profile: claude-default")
            .expect("claude-default rec fires");

        let profile = rec
            .profile
            .as_ref()
            .expect("structured ProviderProfile must be attached to the rec; Codex v1.8.1 fix");
        assert_eq!(profile.name, "claude-default");
        assert_eq!(
            profile.source_kind,
            SourceKind::ProjectCanonical,
            "profile bundle NAME is our abstraction"
        );
        assert_eq!(
            profile.levers.len(),
            3,
            "claude-default bundles exactly three ProviderOfficial levers"
        );
        for lever in &profile.levers {
            assert_eq!(
                lever.source_kind,
                SourceKind::ProviderOfficial,
                "every lever inside the bundle is ProviderOfficial"
            );
            assert!(
                !lever.citation.is_empty(),
                "every lever carries a non-empty citation (a ProviderOfficial claim without a citation is Heuristic)"
            );
        }
        // Spot-check the exact lever keys so a silent re-ordering or
        // value change is caught.
        let keys: Vec<&str> = profile.levers.iter().map(|l| l.key).collect();
        assert!(keys.contains(&"CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS"));
        assert!(keys.contains(&"BASH_MAX_OUTPUT_LENGTH"));
        assert!(keys.contains(&"includeGitInstructions"));
    }

    #[test]
    fn recommend_claude_default_suppressed_on_non_claude_provider() {
        // The profile is Claude-specific; Codex / Gemini panes must not
        // match. This locks the provider gate at the rule level and
        // ensures the rule stays pure (no accidental global firing).
        let id = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Codex,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        };
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_default());
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "provider-profile: claude-default"),
            "claude-default profile is Claude-only; Codex provider must not match"
        );
    }
}
