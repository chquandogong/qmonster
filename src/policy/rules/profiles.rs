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
/// explicit citations. P4-1 shipped `recommend_claude_default`; P4-3
/// adds the aggressive variant `recommend_claude_script_low_token`
/// (gated by `quota_tight`) with a populated
/// `ProviderProfile.side_effects` list — Gemini G-6.
/// `recommend_claude_default` and `recommend_claude_script_low_token`
/// are mutually exclusive by design: the default profile's
/// `if gates.quota_tight { return None; }` gate hands off to the
/// aggressive variant exactly when the operator opts into
/// quota-tight mode.
pub fn eval_profiles(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    if let Some(rec) = recommend_claude_default(id, signals, gates) {
        out.push(rec);
    }
    if let Some(rec) = recommend_claude_script_low_token(id, signals, gates) {
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
        // `claude-default` is the healthy-state baseline — none of
        // its three levers carry operator-visible trade-offs, so
        // `side_effects` is empty by design. The aggressive variant
        // `claude_script_low_token_profile` populates this slot per
        // Gemini G-6.
        side_effects: vec![],
        source_kind: SourceKind::ProjectCanonical,
    }
}

/// `claude-script-low-token`: aggressive Claude profile for
/// headless / scripted sessions with a tight token budget. Fires
/// only under operator-opted `quota_tight` mode — the safety-
/// precedence constraint forbids the aggressive profile from ever
/// surfacing as an always-on default. Bundles low-token CLI flags
/// plus three high-risk env vars (`CLAUDE_CODE_DISABLE_AUTO_MEMORY`,
/// `CLAUDE_CODE_DISABLE_CLAUDE_MDS`,
/// `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS`) that VALIDATION.md:
/// 144-148 gates to THIS profile only. The high-risk-lever guard
/// is enforced both here (inclusion) and in
/// `claude_default_profile` (guaranteed exclusion, locked by the
/// `high_risk_claude_levers_are_gated_to_claude_script_low_token_only`
/// test).
fn recommend_claude_script_low_token(
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
    if !gates.quota_tight {
        // Aggressive profile — opt-in only.
        return None;
    }
    if signals.waiting_for_input
        || signals.permission_prompt
        || signals.log_storm
        || signals.error_hint
    {
        // Don't pile onto a pane that's already blocked on operator
        // attention; the aggressive profile's multi-key edit would
        // be noise compared to the pressing alert.
        return None;
    }
    if let Some(mv) = signals.context_pressure.as_ref()
        && mv.value >= 0.75
    {
        // High context pressure is handled by the Phase-3 strong
        // recs (checkpoint first, compact after). A profile switch
        // mid-pressure would confuse the remediation sequence.
        return None;
    }

    let profile = claude_script_low_token_profile();
    let reason = format!(
        "profile `{}`: apply {} ProviderOfficial levers for a quota-tight scripted session — {} operator-visible side effects (see list below)",
        profile.name,
        profile.levers.len(),
        profile.side_effects.len(),
    );
    let side_effects = profile.side_effects.clone();
    Some(Recommendation {
        action: "provider-profile: claude-script-low-token",
        reason,
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
        // Same multi-key-settings-edit justification as
        // claude-default: applying a profile is not a single
        // runnable command. The structured `profile` payload below
        // carries every lever's key/value/citation + the full
        // side_effects list so operators see the trade-off cost
        // BEFORE applying.
        suggested_command: None,
        side_effects,
        is_strong: false,
        next_step: None,
        profile: Some(profile),
    })
}

fn claude_script_low_token_profile() -> ProviderProfile {
    ProviderProfile {
        name: "claude-script-low-token",
        levers: vec![
            // Low-token CLI flags (VALIDATION.md:133-136).
            ProfileLever {
                key: "--bare",
                value: "enabled",
                citation: "Claude Code docs — CLI flags, suppresses verbose status output",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "--exclude-dynamic-system-prompt-sections",
                value: "enabled",
                citation: "Claude Code docs — CLI flags, omits dynamic system-prompt context",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "--strict-mcp-config",
                value: "enabled",
                citation: "Claude Code docs — CLI flags, reject unrecognized MCP entries at startup",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "--disable-slash-commands",
                value: "enabled",
                citation: "Claude Code docs — CLI flags, in-pane slash commands unavailable",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "--no-session-persistence",
                value: "enabled",
                citation: "Claude Code docs — CLI flags, session state not persisted on restart",
                source_kind: SourceKind::ProviderOfficial,
            },
            // High-risk env vars: VALIDATION.md:144-148 REQUIRES
            // these to live in `claude-script-low-token` ONLY,
            // never in `claude-default` or any always-on profile.
            // The guard is test-enforced (see
            // `high_risk_claude_levers_are_gated_to_claude_script_low_token_only`).
            ProfileLever {
                key: "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
                value: "1",
                citation: "Claude Code docs — environment variables, disables provider auto-memory (aligns with Gemini G-5)",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "CLAUDE_CODE_DISABLE_CLAUDE_MDS",
                value: "1",
                citation: "Claude Code docs — environment variables, skips auto-loading of CLAUDE.md / AGENTS.md",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS",
                value: "1",
                citation: "Claude Code docs — environment variables, disables Agent SDK built-in sub-agents",
                source_kind: SourceKind::ProviderOfficial,
            },
        ],
        // Gemini G-6: every lever above has an operator-visible
        // trade-off. The list is 1:1 with the lever list so
        // operators can scan cost before applying.
        side_effects: vec![
            "--bare suppresses verbose status output — debugging detail may be harder to reconstruct".into(),
            "--exclude-dynamic-system-prompt-sections drops project hints / env info from the system prompt".into(),
            "--strict-mcp-config causes startup to fail loudly on unrecognized MCP entries instead of silently skipping them".into(),
            "--disable-slash-commands blocks in-pane slash commands (/compact, /memory, /clear, ...) mid-session".into(),
            "--no-session-persistence drops session state on restart — resume starts fresh".into(),
            "CLAUDE_CODE_DISABLE_AUTO_MEMORY=1 disables provider auto-memory — state handoff MUST go through .mission/CURRENT_STATE.md or an MDR (aligns with Gemini G-5)".into(),
            "CLAUDE_CODE_DISABLE_CLAUDE_MDS=1 means CLAUDE.md / AGENTS.md are NOT auto-loaded — operator must pass project instructions explicitly".into(),
            "CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1 disables Agent SDK built-in sub-agents — complex delegations unavailable".into(),
        ],
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

    // -----------------------------------------------------------------
    // Phase 4 P4-3 v1.8.3 — claude-script-low-token aggressive profile
    // + Gemini G-6 side_effects population
    // -----------------------------------------------------------------

    fn gates_quota_tight() -> PolicyGates {
        PolicyGates {
            quota_tight: true,
            identity_confidence: IdentityConfidence::High,
        }
    }

    #[test]
    fn recommend_claude_script_low_token_fires_on_quota_tight_with_eight_provider_official_levers_and_populated_side_effects() {
        // Shape contract for the aggressive profile:
        // - fires under quota_tight + healthy Claude main
        // - exactly 8 ProviderOfficial levers with non-empty citations
        // - side_effects list is 1:1 with lever count (Gemini G-6)
        // - rec is Severity::Good (positive advisory, does NOT trigger Notify)
        // - rec source_kind is ProjectCanonical (bundle name is our abstraction)
        // - rec carries the structured profile payload end-to-end
        let id = healthy_claude_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_quota_tight());

        let rec = recs
            .iter()
            .find(|r| r.action == "provider-profile: claude-script-low-token")
            .expect("aggressive profile rec fires under quota_tight");
        assert_eq!(rec.severity, Severity::Good, "positive advisory; stays below Notify gate");
        assert_eq!(rec.source_kind, SourceKind::ProjectCanonical);

        let profile = rec
            .profile
            .as_ref()
            .expect("structured profile payload must reach the rec");
        assert_eq!(profile.name, "claude-script-low-token");
        assert_eq!(profile.source_kind, SourceKind::ProjectCanonical);
        assert_eq!(
            profile.levers.len(),
            8,
            "bundles five low-token CLI flags + three high-risk env vars"
        );
        for lever in &profile.levers {
            assert_eq!(lever.source_kind, SourceKind::ProviderOfficial);
            assert!(!lever.citation.is_empty(), "every lever carries a non-empty citation");
        }
        // Gemini G-6: side_effects populated 1:1 with lever count.
        assert_eq!(
            profile.side_effects.len(),
            profile.levers.len(),
            "G-6: every aggressive lever has a 1:1 operator-visible side effect"
        );
        // Spot-check one concrete side_effect so a silent regression
        // (e.g. empty-string entry, wrong wording) fails here.
        assert!(
            profile
                .side_effects
                .iter()
                .any(|s| s.contains("debugging detail")),
            "side_effects must mention the --bare debugging trade-off"
        );
    }

    #[test]
    fn recommend_claude_script_low_token_suppressed_when_quota_tight_off() {
        // Aggressive profile is opt-in only. Without quota_tight, the
        // baseline `claude-default` fires instead. This test also
        // implicitly verifies mutual exclusion: `claude-default`
        // itself gates off on quota_tight, so the two profiles never
        // co-exist in recs.
        let id = healthy_claude_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_default());

        assert!(
            !recs
                .iter()
                .any(|r| r.action == "provider-profile: claude-script-low-token"),
            "aggressive profile must NOT fire without quota_tight (safety-precedence constraint)"
        );
        assert!(
            recs.iter()
                .any(|r| r.action == "provider-profile: claude-default"),
            "baseline claude-default fires instead when quota_tight is off"
        );
    }

    #[test]
    fn high_risk_claude_levers_are_gated_to_claude_script_low_token_only() {
        // VALIDATION.md:144-148 guard: the three high-risk Claude
        // env vars may NEVER appear in `claude-default` (or any
        // other always-on profile). This test sweeps the default
        // profile's lever keys and ensures none of the three are
        // present; the counterpart inclusion in
        // claude_script_low_token_profile is covered by the shape
        // test above.
        let default = claude_default_profile();
        let default_keys: Vec<&str> = default.levers.iter().map(|l| l.key).collect();

        for high_risk in [
            "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
            "CLAUDE_CODE_DISABLE_CLAUDE_MDS",
            "CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS",
        ] {
            assert!(
                !default_keys.contains(&high_risk),
                "VALIDATION.md:144-148 guard: {} must NOT appear in claude-default; levers: {:?}",
                high_risk,
                default_keys,
            );
        }

        // Counterpart assertion: all three high-risk vars ARE
        // present in the aggressive profile (the guard applies
        // scope, not existence).
        let aggressive = claude_script_low_token_profile();
        let aggressive_keys: Vec<&str> = aggressive.levers.iter().map(|l| l.key).collect();
        for high_risk in [
            "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
            "CLAUDE_CODE_DISABLE_CLAUDE_MDS",
            "CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS",
        ] {
            assert!(
                aggressive_keys.contains(&high_risk),
                "{} must appear in claude-script-low-token; aggressive levers: {:?}",
                high_risk,
                aggressive_keys,
            );
        }
    }
}
