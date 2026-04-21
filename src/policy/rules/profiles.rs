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
/// added the aggressive variant `recommend_claude_script_low_token`
/// (gated by `quota_tight`) with a populated
/// `ProviderProfile.side_effects` list — Gemini G-6. P4-4 adds
/// `recommend_codex_default` — the first non-Claude baseline
/// profile. Provider gates inside each rule keep the bundles
/// mutually exclusive across providers (a Claude pane sees only
/// Claude profiles, a Codex pane sees only Codex profiles).
/// `recommend_claude_default` and `recommend_claude_script_low_token`
/// are also mutually exclusive *within* Claude: the default profile's
/// `if gates.quota_tight { return None; }` gate hands off to the
/// aggressive variant exactly when the operator opts into
/// quota-tight mode. The same pattern will be applied to Codex in
/// P4-5 (aggressive Codex variant).
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
    if let Some(rec) = recommend_codex_default(id, signals, gates) {
        out.push(rec);
    }
    if let Some(rec) = recommend_codex_script_low_token(id, signals, gates) {
        out.push(rec);
    }
    if let Some(rec) = recommend_gemini_default(id, signals, gates) {
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

/// `codex-default`: healthy-state baseline profile for a Codex main
/// pane. Pattern parity with `recommend_claude_default` — fires on
/// Codex main at ≥ Medium confidence when no active alerts / high
/// context pressure / quota-tight gate are present. Bundles three
/// levers drawn from the Codex settings surface
/// (VALIDATION.md:137-143): two `ProviderOfficial` (`web_search =
/// cached` — explicit default saves tokens by reusing cached web
/// results; `commit_attribution = ""` — empty string disables
/// marketing attribution per Codex config spec) plus one
/// `ProjectCanonical` (`tool_output_token_limit = 30000` — Qmonster
/// parity choice with Claude's `BASH_MAX_OUTPUT_LENGTH` bound; Codex
/// docs describe the key but don't mandate this value, so
/// ProjectCanonical is the honest authority label per Codex
/// v1.8.4-review finding #2). Exec flags and aggressive-scripted-
/// session levers belong in the Codex aggressive variant (P4-5, tbd).
fn recommend_codex_default(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if id.identity.provider != Provider::Codex {
        return None;
    }
    if id.identity.role != Role::Main {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    if gates.quota_tight {
        // Baseline only; the aggressive Codex variant (P4-5) will
        // own the quota_tight path.
        return None;
    }
    if signals.waiting_for_input
        || signals.permission_prompt
        || signals.log_storm
        || signals.error_hint
    {
        return None;
    }
    if let Some(mv) = signals.context_pressure.as_ref()
        && mv.value >= 0.75
    {
        return None;
    }

    let profile = codex_default_profile();
    // v1.8.6 remediation (Codex P4-4-confirm finding #1): after the
    // v1.8.5 authority relabel the bundle is 2 ProviderOfficial + 1
    // ProjectCanonical, not a uniform "3 ProviderOfficial". Honest
    // user-visible summary below counts each kind explicitly so the
    // operator sees the same authority split the renderer surfaces
    // per-lever.
    let provider_official_count = profile
        .levers
        .iter()
        .filter(|l| l.source_kind == SourceKind::ProviderOfficial)
        .count();
    let project_canonical_count = profile
        .levers
        .iter()
        .filter(|l| l.source_kind == SourceKind::ProjectCanonical)
        .count();
    let reason = format!(
        "profile `{}`: apply {} levers for a healthy-state baseline main-pane Codex session — {} ProviderOfficial + {} ProjectCanonical (see lever list below for per-lever citations)",
        profile.name,
        profile.levers.len(),
        provider_official_count,
        project_canonical_count,
    );
    let side_effects = profile.side_effects.clone();
    Some(Recommendation {
        action: "provider-profile: codex-default",
        reason,
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
        // Same multi-key-settings-edit justification as the Claude
        // baseline — applying a profile is not a single runnable
        // command. Structured `profile` below carries each lever's
        // key/value/citation for the renderer.
        suggested_command: None,
        side_effects,
        is_strong: false,
        next_step: None,
        profile: Some(profile),
    })
}

fn codex_default_profile() -> ProviderProfile {
    ProviderProfile {
        name: "codex-default",
        levers: vec![
            ProfileLever {
                key: "web_search",
                value: "cached",
                citation: "Codex docs — settings surface, web_search default (cached results reduce token usage vs live lookups)",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "tool_output_token_limit",
                // v1.8.5 remediation (Codex P4-4 finding #2): the KEY
                // is ProviderOfficial (Codex docs describe it), but
                // the VALUE 30000 is Qmonster's parity choice with
                // Claude's BASH_MAX_OUTPUT_LENGTH — Codex's sample
                // config shows 12000 as an example, not a canonical
                // default. Label the lever as ProjectCanonical so
                // the authority stays honest; the citation explains
                // the split.
                value: "30000",
                citation: "Codex docs describe the key (no canonical default — sample shows 12000 as example); Qmonster picks 30000 for cross-provider parity with Claude's BASH_MAX_OUTPUT_LENGTH bound",
                source_kind: SourceKind::ProjectCanonical,
            },
            ProfileLever {
                key: "commit_attribution",
                // v1.8.5 remediation (Codex P4-4 finding #1, risk):
                // Codex docs define `commit_attribution` as a STRING;
                // disabling the marketing attribution means an empty
                // string, NOT the literal "false" (which would parse
                // as the non-empty truthy string "false" and INCLUDE
                // it as attribution text in commits).
                value: "",
                citation: "Codex docs — settings surface, commit_attribution is a string; empty string disables marketing attribution in git commits",
                source_kind: SourceKind::ProviderOfficial,
            },
        ],
        // Healthy-state baseline: no operator-visible trade-offs.
        // The aggressive Codex variant `claude-script-low-token`
        // (P4-5) populates side_effects with its scripted-session
        // cost list; this baseline stays empty by design.
        side_effects: vec![],
        source_kind: SourceKind::ProjectCanonical,
    }
}

/// `codex-script-low-token`: aggressive Codex profile for headless /
/// scripted sessions with a tight token budget. Pattern parity with
/// `recommend_claude_script_low_token` from P4-3 — fires only under
/// operator-opted `quota_tight` mode + Codex main + IdentityConfidence
/// of Medium or higher + healthy signals. Mutually exclusive with
/// `recommend_codex_default` by design: `codex-default` has
/// `if gates.quota_tight { return None; }` and this aggressive
/// variant has `if !gates.quota_tight { return None; }`, same shape
/// as the Claude pair. Bundles aggressive Codex levers drawn from
/// VALIDATION.md:137-143 (`model_auto_compact_token_limit`,
/// `[features].apps`, `[apps._default].enabled`, plus four
/// `codex exec` flags) that were explicitly reserved away from
/// `codex-default` for this slice. Every lever has a 1:1 operator-
/// visible side_effect string (Gemini G-6 parity).
fn recommend_codex_script_low_token(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if id.identity.provider != Provider::Codex {
        return None;
    }
    if id.identity.role != Role::Main {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    if !gates.quota_tight {
        // Aggressive profile — opt-in only (safety-precedence
        // constraint; same rule as claude-script-low-token).
        return None;
    }
    if signals.waiting_for_input
        || signals.permission_prompt
        || signals.log_storm
        || signals.error_hint
    {
        // Don't pile onto a pane that's already blocking on operator
        // attention; the aggressive profile's multi-key edit would be
        // noise compared to the pressing alert.
        return None;
    }
    if let Some(mv) = signals.context_pressure.as_ref()
        && mv.value >= 0.75
    {
        // High context pressure is handled by the Phase-3 strong recs.
        return None;
    }

    let profile = codex_script_low_token_profile();
    // Reason summary is derived from lever source_kinds (same
    // pattern as codex-default after v1.8.6 remediation) so future
    // authority relabels auto-propagate.
    let provider_official_count = profile
        .levers
        .iter()
        .filter(|l| l.source_kind == SourceKind::ProviderOfficial)
        .count();
    let project_canonical_count = profile
        .levers
        .iter()
        .filter(|l| l.source_kind == SourceKind::ProjectCanonical)
        .count();
    let reason = format!(
        "profile `{}`: apply {} levers for a quota-tight scripted Codex session — {} ProviderOfficial + {} ProjectCanonical, with {} operator-visible side effects (see list below)",
        profile.name,
        profile.levers.len(),
        provider_official_count,
        project_canonical_count,
        profile.side_effects.len(),
    );
    let side_effects = profile.side_effects.clone();
    Some(Recommendation {
        action: "provider-profile: codex-script-low-token",
        reason,
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
        // Same multi-key-settings-edit justification as the other
        // profile recs: applying the bundle is not a single
        // runnable command. The structured `profile` below carries
        // every lever's key/value/citation + the full side_effects
        // list so operators see the trade-off cost BEFORE applying.
        suggested_command: None,
        side_effects,
        is_strong: false,
        next_step: None,
        profile: Some(profile),
    })
}

fn codex_script_low_token_profile() -> ProviderProfile {
    ProviderProfile {
        name: "codex-script-low-token",
        levers: vec![
            // model_auto_compact_token_limit: force earlier auto-
            // compaction on scripted sessions. The VALUE is Qmonster's
            // choice (no canonical Codex default), so label the lever
            // ProjectCanonical — same honesty pattern as v1.8.5's
            // tool_output_token_limit relabel.
            ProfileLever {
                key: "model_auto_compact_token_limit",
                value: "100000",
                citation: "Codex docs describe the key; Qmonster picks a conservative 100000 threshold for quota-tight scripted sessions so auto-compaction kicks in before tool_output_token_limit is exhausted",
                source_kind: SourceKind::ProjectCanonical,
            },
            // Disable the [features].apps surface entirely in scripted
            // sessions — the feature is documented and disabling it
            // is a supported configuration.
            ProfileLever {
                key: "[features].apps",
                value: "false",
                citation: "Codex docs — settings surface, [features].apps toggle (disabling removes the apps feature surface in the session)",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "[apps._default].enabled",
                value: "false",
                citation: "Codex docs — settings surface, [apps._default].enabled (disabling skips the default app in scripted sessions)",
                source_kind: SourceKind::ProviderOfficial,
            },
            // codex exec flags for scripted-session use:
            ProfileLever {
                key: "codex exec --json",
                value: "enabled",
                citation: "Codex docs — exec flags, --json (structured JSON output instead of human-readable formatting)",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "codex exec --sandbox",
                value: "read-only",
                citation: "Codex docs — exec flags, --sandbox read-only (filesystem and network writes blocked under the sandbox)",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "codex exec --ephemeral",
                value: "enabled",
                citation: "Codex docs — exec flags, --ephemeral (session state is not persisted across the scripted invocation)",
                source_kind: SourceKind::ProviderOfficial,
            },
            ProfileLever {
                key: "codex exec --color",
                value: "never",
                citation: "Codex docs — exec flags, --color never (ANSI color codes disabled; pipe-safe output)",
                source_kind: SourceKind::ProviderOfficial,
            },
        ],
        // Gemini G-6: every aggressive lever has an operator-visible
        // trade-off. 1:1 with the lever list so operators can scan
        // cost before applying.
        side_effects: vec![
            "model_auto_compact_token_limit = 100000 forces aggressive auto-compaction — earlier history loss than Codex's default threshold; state handoff MUST go through .mission/CURRENT_STATE.md or an MDR (aligns with Gemini G-5)".into(),
            "[features].apps = false removes the apps feature surface — any workflow that relies on apps fails until the flag is flipped back".into(),
            "[apps._default].enabled = false disables the default app — scripted sessions lose the auto-configured app entry point".into(),
            "codex exec --json replaces human-readable output with structured JSON — direct tail-reading operators and adapters must parse JSON instead".into(),
            "codex exec --sandbox read-only blocks filesystem and network writes — any code/tool that needs to write (compile artifacts, cache, logs) fails under the sandbox".into(),
            "codex exec --ephemeral drops session state on invocation end — no resume; every run starts fresh".into(),
            "codex exec --color never strips ANSI colors — pipe-safe output but the operator loses color-coded severity / type cues when tailing the session directly".into(),
        ],
        source_kind: SourceKind::ProjectCanonical,
    }
}

/// `gemini-default`: healthy-state baseline profile for a Gemini main
/// pane. Pattern parity with `recommend_claude_default` /
/// `recommend_codex_default` — fires on a healthy Gemini main pane at
/// IdentityConfidence of Medium or higher, `!quota_tight`, no active
/// alerts, and low context pressure. Bundles two levers from the
/// Gemini CLI surface, both labeled `ProjectCanonical`: Gemini's
/// documented config surface for explicit token-efficiency is
/// narrower than Claude Code's or Codex's, so we honestly flag VALUE
/// choices as Qmonster picks rather than overclaim ProviderOfficial
/// canonical defaults. The per-lever citation explains the
/// "documented key, Qmonster-chosen value" split on each row.
///
/// The broader VALIDATION.md:149-150 constraint ("Gemini profile
/// recommendations stay advisory; `save_memory` / Auto Memory is
/// not treated as a state store") is NOT encoded here as a lever —
/// it is already enforced by the separate
/// `recommend_mdr_over_auto_memory` rule shipped in P4-2
/// (`src/policy/rules/auto_memory.rs`), which fires on any provider
/// (including Gemini) under state-critical task types. Encoding the
/// same policy twice would be duplication.
fn recommend_gemini_default(
    id: &ResolvedIdentity,
    signals: &SignalSet,
    gates: &PolicyGates,
) -> Option<Recommendation> {
    if id.identity.provider != Provider::Gemini {
        return None;
    }
    if id.identity.role != Role::Main {
        return None;
    }
    if !allow_provider_specific(gates.identity_confidence) {
        return None;
    }
    if gates.quota_tight {
        // Baseline only; the aggressive Gemini variant (P4-7) will
        // own the quota_tight path.
        return None;
    }
    if signals.waiting_for_input
        || signals.permission_prompt
        || signals.log_storm
        || signals.error_hint
    {
        return None;
    }
    if let Some(mv) = signals.context_pressure.as_ref()
        && mv.value >= 0.75
    {
        return None;
    }

    let profile = gemini_default_profile();
    // Reason summary is derived from lever source_kinds at runtime
    // (same pattern as the Claude and Codex baselines after v1.8.6
    // remediation) so future authority relabels auto-propagate.
    let provider_official_count = profile
        .levers
        .iter()
        .filter(|l| l.source_kind == SourceKind::ProviderOfficial)
        .count();
    let project_canonical_count = profile
        .levers
        .iter()
        .filter(|l| l.source_kind == SourceKind::ProjectCanonical)
        .count();
    let reason = format!(
        "profile `{}`: apply {} levers for a healthy-state baseline main-pane Gemini session — {} ProviderOfficial + {} ProjectCanonical (see lever list below for per-lever citations)",
        profile.name,
        profile.levers.len(),
        provider_official_count,
        project_canonical_count,
    );
    let side_effects = profile.side_effects.clone();
    Some(Recommendation {
        action: "provider-profile: gemini-default",
        reason,
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
        // Same multi-key-settings-edit justification as the other
        // baselines. Gemini CLI applies config via ~/.gemini/
        // settings.json + flag, not a single runnable command.
        suggested_command: None,
        side_effects,
        is_strong: false,
        next_step: None,
        profile: Some(profile),
    })
}

fn gemini_default_profile() -> ProviderProfile {
    ProviderProfile {
        name: "gemini-default",
        levers: vec![
            // model: Qmonster's baseline choice is gemini-2.5-flash —
            // a lighter / cheaper model, appropriate for healthy
            // routine work (summarization, research, light coding).
            // Gemini CLI docs describe the `--model` flag but don't
            // mandate a canonical default value for all sessions,
            // so the authority is split: the KEY is ProviderOfficial
            // (documented), the VALUE is ProjectCanonical (Qmonster
            // pick). Same honesty pattern as codex-default's
            // tool_output_token_limit after v1.8.5.
            ProfileLever {
                key: "--model",
                value: "gemini-2.5-flash",
                citation: "Gemini CLI docs describe the --model flag (no canonical default for all sessions); Qmonster picks gemini-2.5-flash for healthy main-pane baseline to keep per-token cost low on routine work",
                source_kind: SourceKind::ProjectCanonical,
            },
            // Auto-approval stays explicitly OFF in the baseline.
            // Gemini's `--yolo` flag auto-approves agent actions,
            // which belongs to the aggressive variant (P4-7, tbd);
            // the safe default for an interactive healthy main pane
            // is NOT to set --yolo. We express that as a lever so
            // the render surfaces it to the operator — the authority
            // label is ProjectCanonical because "recommend NOT
            // setting a flag" is a Qmonster architectural choice
            // (the mission.yaml safety-precedence constraint), not
            // a Gemini-doc canonical value.
            ProfileLever {
                key: "--yolo",
                value: "unset",
                citation: "Gemini CLI docs describe --yolo as auto-approval for agent actions; Qmonster recommends KEEPING IT UNSET on a healthy interactive main-pane baseline per the safety-precedence constraint (aggressive variant in P4-7 will reserve the auto-approval path for quota_tight-opted sessions)",
                source_kind: SourceKind::ProjectCanonical,
            },
        ],
        // Healthy-state baseline: no operator-visible trade-offs.
        // The aggressive Gemini variant (P4-7) will populate
        // side_effects with its scripted-session cost list.
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

    // -----------------------------------------------------------------
    // Phase 4 P4-4 v1.8.4 — codex-default baseline profile
    // -----------------------------------------------------------------

    fn healthy_codex_main() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Codex,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    #[test]
    fn recommend_codex_default_fires_with_honest_authority_labels_on_healthy_codex_main() {
        // v1.8.5 remediation (Codex P4-4 findings): the codex-default
        // bundle is 2 ProviderOfficial levers (`web_search = cached`,
        // `commit_attribution = ""`) + 1 ProjectCanonical lever
        // (`tool_output_token_limit = 30000` — Qmonster parity choice).
        // This test fails if any of the three lever values or
        // source_kinds drifts away from that split, including the
        // specific regressions the Codex P4-4 review caught:
        //   - commit_attribution must be "" (empty string), NOT "false"
        //     which would parse as the truthy string "false" per
        //     Codex docs (risk finding #1).
        //   - tool_output_token_limit must be labeled ProjectCanonical
        //     because the value 30000 is Qmonster's parity choice,
        //     not a Codex-doc canonical default (warning finding #2).
        let id = healthy_codex_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_default());

        let rec = recs
            .iter()
            .find(|r| r.action == "provider-profile: codex-default")
            .expect("codex-default profile rec fires on healthy Codex main pane");

        assert_eq!(
            rec.source_kind,
            SourceKind::ProjectCanonical,
            "profile bundle NAME is our abstraction"
        );
        assert_eq!(
            rec.severity,
            Severity::Good,
            "healthy-state profile rec is a positive advisory, not an alert"
        );
        assert!(
            rec.reason.contains("codex-default"),
            "reason must name the profile: {}", rec.reason
        );
        assert!(
            rec.reason.contains("ProviderOfficial"),
            "reason must cite ProviderOfficial authority label for the levers that ARE ProviderOfficial: {}",
            rec.reason
        );
        // v1.8.6 remediation (Codex P4-4-confirm finding #1): the
        // user-visible reason summary must reflect the honest
        // authority split. "apply 3 ProviderOfficial levers" was
        // wrong after v1.8.5 relabel because the bundle is now 2
        // PO + 1 PC.
        assert!(
            rec.reason.contains("ProjectCanonical"),
            "reason must also cite ProjectCanonical (the tool_output_token_limit lever authority is Qmonster's parity choice, not a Codex-doc default — Codex P4-4-confirm finding #1 locks the honest split in the summary): {}",
            rec.reason
        );
        assert!(
            rec.suggested_command.is_none(),
            "profile rec has no single-surface runnable command (multi-key settings edit)"
        );

        let profile = rec
            .profile
            .as_ref()
            .expect("structured ProviderProfile must be attached to the rec");
        assert_eq!(profile.name, "codex-default");
        assert_eq!(profile.source_kind, SourceKind::ProjectCanonical);
        assert_eq!(
            profile.levers.len(),
            3,
            "codex-default bundles three levers (2 ProviderOfficial + 1 ProjectCanonical)"
        );

        // Every lever must carry a non-empty citation (universal).
        for lever in &profile.levers {
            assert!(
                !lever.citation.is_empty(),
                "every lever carries a non-empty citation: {:?}",
                lever,
            );
        }

        // Per-lever value + source_kind contract. Exact values
        // prevent silent drift (the commit_attribution = "false"
        // regression caught by Codex P4-4 review would fail here).
        let find_lever = |key: &str| -> &ProfileLever {
            profile
                .levers
                .iter()
                .find(|l| l.key == key)
                .unwrap_or_else(|| panic!("lever `{key}` must be present in codex-default"))
        };

        let web_search = find_lever("web_search");
        assert_eq!(web_search.value, "cached");
        assert_eq!(
            web_search.source_kind,
            SourceKind::ProviderOfficial,
            "web_search default is a Codex-doc fact"
        );

        let tool_limit = find_lever("tool_output_token_limit");
        assert_eq!(tool_limit.value, "30000");
        assert_eq!(
            tool_limit.source_kind,
            SourceKind::ProjectCanonical,
            "the value 30000 is Qmonster's parity choice with Claude BASH_MAX_OUTPUT_LENGTH; Codex docs describe the key but don't mandate this value (Codex P4-4 finding #2)"
        );

        let commit_attr = find_lever("commit_attribution");
        assert_eq!(
            commit_attr.value, "",
            "Codex docs: commit_attribution is a STRING; empty string disables marketing attribution. Literal \"false\" would parse as truthy and wrongly include 'false' as attribution text (Codex P4-4 finding #1, risk)"
        );
        assert_eq!(commit_attr.source_kind, SourceKind::ProviderOfficial);

        // Healthy baseline has no operator-visible trade-offs.
        assert!(
            profile.side_effects.is_empty(),
            "codex-default is a healthy-state baseline; no side_effects until the aggressive variant (P4-5)"
        );
    }

    #[test]
    fn recommend_codex_default_suppressed_on_non_codex_provider() {
        // Symmetric to
        // `recommend_claude_default_suppressed_on_non_claude_provider`:
        // the Codex profile rule must stay pure (no accidental firing
        // on Claude / Gemini / Qmonster panes). On a Claude main
        // pane, claude-default fires INSTEAD — both rules respect
        // their provider gate.
        let claude_main = healthy_claude_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&claude_main, &s, &gates_default());
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "provider-profile: codex-default"),
            "codex-default profile is Codex-only; Claude provider must not match"
        );
        // Sanity: claude-default still fires on the Claude pane.
        assert!(
            recs.iter()
                .any(|r| r.action == "provider-profile: claude-default"),
            "claude-default should still fire on a healthy Claude main pane"
        );
    }

    #[test]
    fn recommend_codex_default_suppressed_when_quota_tight_on() {
        // codex-default is the baseline; the aggressive Codex
        // variant (P4-5, tbd) will own the quota_tight path. This
        // test locks the gate so the aggressive variant, when
        // added, cleanly takes over without overlap — same pattern
        // as claude-default ↔ claude-script-low-token mutual
        // exclusion.
        let id = healthy_codex_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_quota_tight());
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "provider-profile: codex-default"),
            "codex-default must NOT fire under quota_tight; aggressive variant will own that path in P4-5"
        );
    }

    // -----------------------------------------------------------------
    // Phase 4 P4-5 v1.8.7 — codex-script-low-token aggressive profile
    // + Gemini G-6 side_effects parity for Codex
    // -----------------------------------------------------------------

    #[test]
    fn recommend_codex_script_low_token_fires_on_quota_tight_with_honest_authority_labels_and_populated_side_effects() {
        // Shape contract for the Codex aggressive profile — mirrors
        // the claude-script-low-token contract from P4-3 with
        // Codex-specific levers. Fails if:
        //   - the rule doesn't fire under quota_tight on a healthy
        //     Codex main pane,
        //   - the structured profile payload doesn't reach the rec,
        //   - lever count drifts from 7,
        //   - side_effects count drifts from 7 (1:1 invariant, G-6),
        //   - every lever's citation is empty,
        //   - the rec severity / source_kind aren't Good / ProjectCanonical.
        let id = healthy_codex_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_quota_tight());

        let rec = recs
            .iter()
            .find(|r| r.action == "provider-profile: codex-script-low-token")
            .expect("codex-script-low-token fires under quota_tight on a healthy Codex main pane");

        assert_eq!(rec.severity, Severity::Good, "positive advisory; stays below Notify gate");
        assert_eq!(rec.source_kind, SourceKind::ProjectCanonical);

        let profile = rec
            .profile
            .as_ref()
            .expect("structured profile payload must reach the rec");
        assert_eq!(profile.name, "codex-script-low-token");
        assert_eq!(profile.source_kind, SourceKind::ProjectCanonical);
        assert_eq!(
            profile.levers.len(),
            7,
            "bundles 7 aggressive Codex levers (model_auto_compact_token_limit + 2 feature toggles + 4 exec flags)"
        );
        for lever in &profile.levers {
            assert!(
                !lever.citation.is_empty(),
                "every lever carries a non-empty citation: {:?}",
                lever,
            );
        }
        // Gemini G-6: side_effects populated 1:1 with lever count.
        assert_eq!(
            profile.side_effects.len(),
            profile.levers.len(),
            "G-6: every aggressive lever has a 1:1 operator-visible side effect"
        );

        // Authority split: 1 ProjectCanonical (the chosen
        // model_auto_compact_token_limit value) + 6 ProviderOfficial.
        // Pre-computed counts match the reason summary's derivation.
        let po_count = profile
            .levers
            .iter()
            .filter(|l| l.source_kind == SourceKind::ProviderOfficial)
            .count();
        let pc_count = profile
            .levers
            .iter()
            .filter(|l| l.source_kind == SourceKind::ProjectCanonical)
            .count();
        assert_eq!(po_count, 6, "6 ProviderOfficial (2 feature toggles + 4 exec flags)");
        assert_eq!(pc_count, 1, "1 ProjectCanonical (model_auto_compact_token_limit value)");

        // Reason summary honesty (Codex P4-4 v1.8.6 pattern — count
        // each authority kind).
        assert!(rec.reason.contains("codex-script-low-token"));
        assert!(
            rec.reason.contains("ProviderOfficial"),
            "reason must cite ProviderOfficial authority label: {}",
            rec.reason
        );
        assert!(
            rec.reason.contains("ProjectCanonical"),
            "reason must cite ProjectCanonical authority label: {}",
            rec.reason
        );

        // Spot-check one high-risk trade-off reaches side_effects
        // with the expected language — regression would fire here
        // if the string ever drifts.
        assert!(
            profile
                .side_effects
                .iter()
                .any(|s| s.contains("auto-compaction")),
            "side_effects must mention the model_auto_compact_token_limit trade-off (aggressive auto-compaction)"
        );
        assert!(
            profile
                .side_effects
                .iter()
                .any(|s| s.contains("sandbox")),
            "side_effects must mention the --sandbox read-only filesystem-block trade-off"
        );
    }

    #[test]
    fn recommend_codex_script_low_token_suppressed_when_quota_tight_off() {
        // Aggressive profile is opt-in only. Without quota_tight,
        // the baseline codex-default fires instead. This test also
        // implicitly verifies mutual exclusion: codex-default itself
        // gates off on quota_tight, so the two Codex profiles never
        // co-exist in recs on a single pane (same pattern as the
        // Claude pair from P4-1 / P4-3).
        let id = healthy_codex_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_default());

        assert!(
            !recs
                .iter()
                .any(|r| r.action == "provider-profile: codex-script-low-token"),
            "aggressive profile must NOT fire without quota_tight (safety-precedence constraint)"
        );
        assert!(
            recs.iter()
                .any(|r| r.action == "provider-profile: codex-default"),
            "baseline codex-default fires instead when quota_tight is off"
        );
    }

    #[test]
    fn recommend_codex_script_low_token_suppressed_on_non_codex_provider() {
        // Provider gate: the Codex aggressive profile must never
        // fire on a Claude or Gemini pane, even under quota_tight.
        // Under quota_tight on a Claude pane, claude-script-low-token
        // fires instead — sanity check that symmetric provider
        // gating holds across the four profiles.
        let claude_main = healthy_claude_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&claude_main, &s, &gates_quota_tight());

        assert!(
            !recs
                .iter()
                .any(|r| r.action == "provider-profile: codex-script-low-token"),
            "codex-script-low-token is Codex-only; Claude provider must not match"
        );
        assert!(
            recs.iter()
                .any(|r| r.action == "provider-profile: claude-script-low-token"),
            "claude-script-low-token fires on a Claude pane under quota_tight"
        );
    }

    #[test]
    fn codex_default_and_codex_script_low_token_are_mutually_exclusive_via_quota_tight_gate() {
        // Explicit mutual-exclusion contract test (mirrors the
        // implicit claude pair in P4-3). For a single Codex main
        // pane, flipping quota_tight toggles EXACTLY one of the two
        // profile recs — never both, never neither (on a healthy
        // baseline pane).
        let id = healthy_codex_main();
        let s = SignalSet::default();

        // quota_tight off: only codex-default fires.
        let off = eval_profiles(&id, &s, &gates_default());
        let default_off = off
            .iter()
            .any(|r| r.action == "provider-profile: codex-default");
        let aggressive_off = off
            .iter()
            .any(|r| r.action == "provider-profile: codex-script-low-token");
        assert!(default_off, "codex-default fires when quota_tight is off");
        assert!(
            !aggressive_off,
            "codex-script-low-token must NOT fire when quota_tight is off"
        );

        // quota_tight on: only codex-script-low-token fires.
        let on = eval_profiles(&id, &s, &gates_quota_tight());
        let default_on = on
            .iter()
            .any(|r| r.action == "provider-profile: codex-default");
        let aggressive_on = on
            .iter()
            .any(|r| r.action == "provider-profile: codex-script-low-token");
        assert!(
            !default_on,
            "codex-default must NOT fire when quota_tight is on"
        );
        assert!(
            aggressive_on,
            "codex-script-low-token fires when quota_tight is on"
        );
    }

    // -----------------------------------------------------------------
    // Phase 4 P4-6 v1.8.8 — gemini-default baseline profile (first
    // non-Claude-non-Codex provider entry)
    // -----------------------------------------------------------------

    fn healthy_gemini_main() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Gemini,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    #[test]
    fn recommend_gemini_default_fires_with_honest_authority_labels_on_healthy_gemini_main() {
        // Gemini's documented config surface for explicit token-
        // efficiency is narrower than Claude Code's or Codex's, so
        // the baseline bundle is honestly scoped at 2 levers — both
        // labeled ProjectCanonical because each VALUE is a Qmonster
        // pick rather than a Gemini-doc canonical default. This
        // test locks the per-lever value + source_kind + the
        // ProjectCanonical-heavy authority split.
        let id = healthy_gemini_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_default());

        let rec = recs
            .iter()
            .find(|r| r.action == "provider-profile: gemini-default")
            .expect("gemini-default profile rec fires on healthy Gemini main pane");

        assert_eq!(rec.source_kind, SourceKind::ProjectCanonical);
        assert_eq!(rec.severity, Severity::Good);
        assert!(rec.reason.contains("gemini-default"));
        assert!(
            rec.reason.contains("ProjectCanonical"),
            "reason must cite ProjectCanonical authority label (both levers are PC): {}",
            rec.reason
        );
        assert!(
            rec.suggested_command.is_none(),
            "profile rec has no single-surface runnable command"
        );

        let profile = rec
            .profile
            .as_ref()
            .expect("structured ProviderProfile must be attached to the rec");
        assert_eq!(profile.name, "gemini-default");
        assert_eq!(profile.source_kind, SourceKind::ProjectCanonical);
        assert_eq!(
            profile.levers.len(),
            2,
            "gemini-default bundles 2 levers — Gemini's documented surface for token-efficiency is narrower than Claude / Codex, so the honest minimum is smaller"
        );
        for lever in &profile.levers {
            assert!(
                !lever.citation.is_empty(),
                "every lever carries a non-empty citation: {:?}",
                lever,
            );
        }

        // Per-lever value + source_kind contract.
        let find_lever = |key: &str| -> &ProfileLever {
            profile
                .levers
                .iter()
                .find(|l| l.key == key)
                .unwrap_or_else(|| panic!("lever `{key}` must be present in gemini-default"))
        };

        let model = find_lever("--model");
        assert_eq!(model.value, "gemini-2.5-flash");
        assert_eq!(
            model.source_kind,
            SourceKind::ProjectCanonical,
            "the VALUE gemini-2.5-flash is Qmonster's pick; Gemini CLI docs describe --model but don't mandate this value for all sessions"
        );

        let yolo = find_lever("--yolo");
        assert_eq!(yolo.value, "unset");
        assert_eq!(
            yolo.source_kind,
            SourceKind::ProjectCanonical,
            "\"recommend NOT setting --yolo\" is a Qmonster architectural choice (safety-precedence constraint), not a Gemini-doc canonical default"
        );

        // Authority split: 0 ProviderOfficial + 2 ProjectCanonical.
        let po_count = profile
            .levers
            .iter()
            .filter(|l| l.source_kind == SourceKind::ProviderOfficial)
            .count();
        let pc_count = profile
            .levers
            .iter()
            .filter(|l| l.source_kind == SourceKind::ProjectCanonical)
            .count();
        assert_eq!(po_count, 0, "no ProviderOfficial levers in gemini-default — every value is a Qmonster pick");
        assert_eq!(pc_count, 2);

        // Healthy baseline has no operator-visible trade-offs.
        assert!(
            profile.side_effects.is_empty(),
            "gemini-default is a healthy-state baseline; side_effects stays empty until the P4-7 aggressive variant"
        );
    }

    #[test]
    fn recommend_gemini_default_suppressed_on_non_gemini_provider() {
        // Provider gate: gemini-default must not fire on Claude or
        // Codex panes, even under healthy signals. Claude + Codex
        // baselines fire instead — sanity check that symmetric
        // provider gating holds across all five profile rules.
        let claude_main = healthy_claude_main();
        let s = SignalSet::default();
        let recs_claude = eval_profiles(&claude_main, &s, &gates_default());
        assert!(
            !recs_claude
                .iter()
                .any(|r| r.action == "provider-profile: gemini-default"),
            "gemini-default is Gemini-only; Claude pane must not match"
        );
        assert!(
            recs_claude
                .iter()
                .any(|r| r.action == "provider-profile: claude-default"),
            "claude-default fires instead on a healthy Claude main pane"
        );

        let codex_main = healthy_codex_main();
        let recs_codex = eval_profiles(&codex_main, &s, &gates_default());
        assert!(
            !recs_codex
                .iter()
                .any(|r| r.action == "provider-profile: gemini-default"),
            "gemini-default is Gemini-only; Codex pane must not match"
        );
        assert!(
            recs_codex
                .iter()
                .any(|r| r.action == "provider-profile: codex-default"),
            "codex-default fires instead on a healthy Codex main pane"
        );
    }

    #[test]
    fn recommend_gemini_default_suppressed_when_quota_tight_on() {
        // Baseline only — the aggressive Gemini variant (P4-7, tbd)
        // will own the quota_tight path. This test reserves the
        // gate so the aggressive variant, when added, cleanly takes
        // over without overlap (same pattern as Claude / Codex
        // baseline ↔ aggressive pairs).
        let id = healthy_gemini_main();
        let s = SignalSet::default();
        let recs = eval_profiles(&id, &s, &gates_quota_tight());
        assert!(
            !recs
                .iter()
                .any(|r| r.action == "provider-profile: gemini-default"),
            "gemini-default must NOT fire under quota_tight; aggressive variant will own that path in P4-7"
        );
    }
}
