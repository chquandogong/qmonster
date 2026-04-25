use std::fmt;

use crate::domain::identity::{Provider, Role};
use crate::domain::recommendation::Severity;

/// Audit event kinds recorded by the observe-first MVP. This list stays
/// stable for the v0.4.0 line; additions require an MDR entry.
///
/// v1.10.4 audit-vocab compile-time safety (closes Codex v1.10.2 §9 +
/// Gemini v1.10.2 #10 — compatible pair): the canonical string form of
/// each variant is exposed via `AuditEventKind::as_str`, with
/// `std::fmt::Display` and `AsRef<str>` both delegating. SQLite
/// serialization (`store::audit::kind_to_str`) now routes through this
/// method so there is a single source of truth for the audit-kind
/// vocabulary. Unit tests in this module lock the exact string value
/// of every variant so renaming a variant requires a deliberate test
/// update (catches accidental schema drift at compile time and at test
/// time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditEventKind {
    PaneIdentityResolved,
    PaneIdentityChanged,
    PaneBecameDead,
    PaneReappeared,
    AlertFired,
    RecommendationEmitted,
    StartupVersionSnapshot,
    VersionDriftDetected,
    SafetyOverrideRejected,
    ArchiveWritten,
    SnapshotWritten,
    RetentionSwept,
    VersionSnapshotError,
    /// v1.11.2 remediation (Gemini v1.11.0 must-fix #2 + Codex Q5):
    /// `PricingTable::load_from_toml` returned an error other than
    /// NotFound (malformed TOML, unknown provider, I/O failure). The
    /// TUI falls back to an empty pricing table so cost badges stay
    /// blank; this event is the durable breadcrumb that records the
    /// fallback, visible via SQLite query. Complements the ephemeral
    /// `eprintln!` kept for dev / non-TUI runs.
    PricingLoadFailed,
    ClaudeSettingsLoadFailed,
    AuditWriteFailed,
    /// Phase 5 P5-1 (v1.9.0): a policy rule emitted a
    /// `RequestedEffect::PromptSendProposed` and the proposal reached
    /// the UI surface. Metadata-only — the summary string carries the
    /// target pane and slash command; raw pane tail never flows through
    /// this event (audit-isolation rule).
    PromptSendProposed,
    /// Phase 5 P5-1 (v1.9.0): operator-confirmed a pending prompt-send
    /// proposal. No executor code path exists yet in P5-1 (this kind
    /// is a forward-declared audit contract); P5-2+ will record the
    /// acceptance the moment the operator confirmation runs.
    PromptSendAccepted,
    /// Phase 5 P5-1 (v1.9.0): operator rejected / dismissed a pending
    /// prompt-send proposal. Same forward-declaration note as
    /// `PromptSendAccepted`.
    PromptSendRejected,
    /// Phase 5 P5-3 (v1.10.0): `tmux send-keys` completed successfully
    /// after the operator confirmed AND `allow_auto_prompt_send = true`
    /// (both gates in the execution gate passed). Metadata only —
    /// summary carries target pane + slash command + confirmation verb;
    /// raw pane tail never flows through this event.
    PromptSendCompleted,
    /// Phase 5 P5-3 (v1.10.0): `tmux send-keys` call failed after the
    /// execution gate passed (both gates: operator confirmation +
    /// `allow_auto_prompt_send = true`). Summary carries the error
    /// string from the tmux invocation.
    PromptSendFailed,
    /// Phase 5 P5-3 (v1.10.0, semantic broadened in v1.10.1
    /// remediation): the execution gate refused an operator-triggered
    /// send. Two fire paths:
    ///
    ///   1. `observe_only` mode: operator pressed `p` but the mode
    ///      blocks acceptance itself. No `PromptSendAccepted` fires;
    ///      `PromptSendBlocked` alone records the attempt.
    ///   2. non-observe_only mode with `allow_auto_prompt_send=false`
    ///      (`AutoSendOff`): operator pressed `p` and acceptance is
    ///      real (`PromptSendAccepted` fires), but execution is
    ///      refused because the auto-send flag is off. A follow-up
    ///      `PromptSendBlocked` event closes the audit chain
    ///      (v1.10.1 remediation — Gemini v1.10.0 finding #3).
    ///
    /// Forensics can always distinguish "operator tried and was
    /// blocked" from "no action was taken at all"; the summary string
    /// names which of the two gate reasons fired.
    PromptSendBlocked,
    /// Operator-triggered provider runtime refresh, normally a read-only
    /// slash command such as `/status` sent to the selected pane.
    /// Metadata only; raw pane tails are not recorded.
    RuntimeRefreshRequested,
    RuntimeRefreshCompleted,
    RuntimeRefreshFailed,
    RuntimeRefreshBlocked,
}

impl AuditEventKind {
    /// Canonical string form used for audit-log serialization (SQLite
    /// row value) and for operator-facing documentation (UI help
    /// overlay, Phase-5 mission narrative). Single source of truth —
    /// `store::audit::kind_to_str` delegates here, and adding a new
    /// variant without extending this match is a compile-time error.
    pub const fn as_str(&self) -> &'static str {
        match self {
            AuditEventKind::PaneIdentityResolved => "PaneIdentityResolved",
            AuditEventKind::PaneIdentityChanged => "PaneIdentityChanged",
            AuditEventKind::PaneBecameDead => "PaneBecameDead",
            AuditEventKind::PaneReappeared => "PaneReappeared",
            AuditEventKind::AlertFired => "AlertFired",
            AuditEventKind::RecommendationEmitted => "RecommendationEmitted",
            AuditEventKind::StartupVersionSnapshot => "StartupVersionSnapshot",
            AuditEventKind::VersionDriftDetected => "VersionDriftDetected",
            AuditEventKind::SafetyOverrideRejected => "SafetyOverrideRejected",
            AuditEventKind::ArchiveWritten => "ArchiveWritten",
            AuditEventKind::SnapshotWritten => "SnapshotWritten",
            AuditEventKind::RetentionSwept => "RetentionSwept",
            AuditEventKind::VersionSnapshotError => "VersionSnapshotError",
            AuditEventKind::PricingLoadFailed => "PricingLoadFailed",
            AuditEventKind::ClaudeSettingsLoadFailed => "ClaudeSettingsLoadFailed",
            AuditEventKind::AuditWriteFailed => "AuditWriteFailed",
            AuditEventKind::PromptSendProposed => "PromptSendProposed",
            AuditEventKind::PromptSendAccepted => "PromptSendAccepted",
            AuditEventKind::PromptSendRejected => "PromptSendRejected",
            AuditEventKind::PromptSendCompleted => "PromptSendCompleted",
            AuditEventKind::PromptSendFailed => "PromptSendFailed",
            AuditEventKind::PromptSendBlocked => "PromptSendBlocked",
            AuditEventKind::RuntimeRefreshRequested => "RuntimeRefreshRequested",
            AuditEventKind::RuntimeRefreshCompleted => "RuntimeRefreshCompleted",
            AuditEventKind::RuntimeRefreshFailed => "RuntimeRefreshFailed",
            AuditEventKind::RuntimeRefreshBlocked => "RuntimeRefreshBlocked",
        }
    }
}

impl AsRef<str> for AuditEventKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for AuditEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Structured audit record. The writer API must only accept this type —
/// raw pane tails are never allowed in (r2 type-level separation rule).
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub kind: AuditEventKind,
    pub pane_id: String,
    pub severity: Severity,
    pub summary: String,
    pub provider: Option<Provider>,
    pub role: Option<Role>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{PaneIdentity, Provider, Role};
    use crate::domain::recommendation::Severity;

    fn any_identity() -> PaneIdentity {
        PaneIdentity {
            provider: Provider::Claude,
            instance: 1,
            role: Role::Main,
            pane_id: "%1".into(),
        }
    }

    #[test]
    fn event_stores_structured_metadata_only() {
        let e = AuditEvent {
            kind: AuditEventKind::PaneIdentityResolved,
            pane_id: any_identity().pane_id,
            severity: Severity::Safe,
            summary: "claude:1:main".into(),
            provider: Some(Provider::Claude),
            role: Some(Role::Main),
        };
        // No field accepts raw tail or bytes — enforced by the struct itself.
        assert_eq!(e.kind, AuditEventKind::PaneIdentityResolved);
    }

    #[test]
    fn version_drift_is_a_distinct_event_kind() {
        // Gemini G-3 / version-drift detector: when a CLI version changes,
        // the audit log records the transition as a named event, not as
        // raw text bleeding in from anywhere else.
        let e = AuditEvent {
            kind: AuditEventKind::VersionDriftDetected,
            pane_id: "n/a".into(),
            severity: Severity::Warning,
            summary: "claude-cli: 1.0 -> 1.1".into(),
            provider: Some(Provider::Claude),
            role: None,
        };
        assert_eq!(e.kind, AuditEventKind::VersionDriftDetected);
        assert_eq!(e.severity, Severity::Warning);
    }

    #[test]
    fn prompt_send_audit_kinds_are_distinct_and_carry_only_metadata() {
        // P5-1 audit contract: three dedicated kinds cover the
        // proposal → accepted / rejected lifecycle. Every field is
        // structured metadata — summary holds target + slash command
        // text, never raw pane bytes. The writer still rejects raw
        // input by virtue of the struct signature (no bytes field).
        let proposed = AuditEvent {
            kind: AuditEventKind::PromptSendProposed,
            pane_id: "%1".into(),
            severity: Severity::Concern,
            summary: "%1 /compact (pending operator confirmation)".into(),
            provider: Some(Provider::Claude),
            role: Some(Role::Main),
        };
        let accepted = AuditEvent {
            kind: AuditEventKind::PromptSendAccepted,
            pane_id: "%1".into(),
            severity: Severity::Warning,
            summary: "%1 /compact (operator-confirmed)".into(),
            provider: Some(Provider::Claude),
            role: Some(Role::Main),
        };
        let rejected = AuditEvent {
            kind: AuditEventKind::PromptSendRejected,
            pane_id: "%1".into(),
            severity: Severity::Safe,
            summary: "%1 /compact (operator-dismissed)".into(),
            provider: Some(Provider::Claude),
            role: Some(Role::Main),
        };
        assert_ne!(proposed.kind, accepted.kind);
        assert_ne!(accepted.kind, rejected.kind);
        assert_ne!(proposed.kind, rejected.kind);
        // Sanity: all three are usable Copy values (this mirrors the
        // pattern used throughout the domain and guarantees we did not
        // accidentally break Copy by reshuffling the enum).
        let _ = proposed.kind;
        let _ = accepted.kind;
        let _ = rejected.kind;
    }

    #[test]
    fn audit_event_kind_as_str_contract_locks_every_variant_string() {
        // v1.10.4 audit-vocab compile-time safety: lock the canonical
        // string form of every AuditEventKind. This contract is what
        // `store::audit::kind_to_str` serializes into SQLite and what
        // the UI help overlay shows to operators. Renaming a variant
        // without updating both this test AND the as_str match arm
        // fails at compile time (exhaustive match) + at test time
        // (explicit string expectation).
        let cases: &[(AuditEventKind, &str)] = &[
            (AuditEventKind::PaneIdentityResolved, "PaneIdentityResolved"),
            (AuditEventKind::PaneIdentityChanged, "PaneIdentityChanged"),
            (AuditEventKind::PaneBecameDead, "PaneBecameDead"),
            (AuditEventKind::PaneReappeared, "PaneReappeared"),
            (AuditEventKind::AlertFired, "AlertFired"),
            (
                AuditEventKind::RecommendationEmitted,
                "RecommendationEmitted",
            ),
            (
                AuditEventKind::StartupVersionSnapshot,
                "StartupVersionSnapshot",
            ),
            (AuditEventKind::VersionDriftDetected, "VersionDriftDetected"),
            (
                AuditEventKind::SafetyOverrideRejected,
                "SafetyOverrideRejected",
            ),
            (AuditEventKind::ArchiveWritten, "ArchiveWritten"),
            (AuditEventKind::SnapshotWritten, "SnapshotWritten"),
            (AuditEventKind::RetentionSwept, "RetentionSwept"),
            (AuditEventKind::VersionSnapshotError, "VersionSnapshotError"),
            (AuditEventKind::PricingLoadFailed, "PricingLoadFailed"),
            (
                AuditEventKind::ClaudeSettingsLoadFailed,
                "ClaudeSettingsLoadFailed",
            ),
            (AuditEventKind::AuditWriteFailed, "AuditWriteFailed"),
            (AuditEventKind::PromptSendProposed, "PromptSendProposed"),
            (AuditEventKind::PromptSendAccepted, "PromptSendAccepted"),
            (AuditEventKind::PromptSendRejected, "PromptSendRejected"),
            (AuditEventKind::PromptSendCompleted, "PromptSendCompleted"),
            (AuditEventKind::PromptSendFailed, "PromptSendFailed"),
            (AuditEventKind::PromptSendBlocked, "PromptSendBlocked"),
            (
                AuditEventKind::RuntimeRefreshRequested,
                "RuntimeRefreshRequested",
            ),
            (
                AuditEventKind::RuntimeRefreshCompleted,
                "RuntimeRefreshCompleted",
            ),
            (AuditEventKind::RuntimeRefreshFailed, "RuntimeRefreshFailed"),
            (
                AuditEventKind::RuntimeRefreshBlocked,
                "RuntimeRefreshBlocked",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(
                kind.as_str(),
                *expected,
                "{kind:?}.as_str() must stringify as {expected:?}"
            );
            // Display, AsRef<str>, and as_str must all agree — one
            // canonical representation, three ergonomic entry points.
            assert_eq!(format!("{kind}"), *expected, "Display for {kind:?}");
            let r: &str = kind.as_ref();
            assert_eq!(r, *expected, "AsRef<str> for {kind:?}");
            // v1.10.5 remediation (Codex v1.10.4 optional TODO #2):
            // lock `{:?}` ↔ Display parity explicitly. Today the
            // derived Debug output equals the Display output
            // because variant names happen to match `as_str`
            // literals; this assertion catches a future variant
            // being renamed (or `as_str` being rewritten) that
            // breaks the parity silently.
            assert_eq!(
                format!("{kind:?}"),
                *expected,
                "Debug for {kind:?} must equal Display/as_str for audit-log forensic clarity"
            );
        }
    }

    #[test]
    fn p5_3_audit_kinds_are_distinct_and_copy() {
        // P5-3 contract: three new terminal-outcome kinds cover
        // Completed / Failed / Blocked. Must be distinct, Copy, and
        // carry only structured metadata (summary string, no bytes).
        let completed = AuditEvent {
            kind: AuditEventKind::PromptSendCompleted,
            pane_id: "%1".into(),
            severity: Severity::Safe,
            summary: "%1 /compact (sent; operator-confirmed)".into(),
            provider: Some(Provider::Claude),
            role: Some(Role::Main),
        };
        let failed = AuditEvent {
            kind: AuditEventKind::PromptSendFailed,
            pane_id: "%1".into(),
            severity: Severity::Warning,
            summary: "%1 /compact (send failed: tmux error)".into(),
            provider: Some(Provider::Claude),
            role: Some(Role::Main),
        };
        let blocked = AuditEvent {
            kind: AuditEventKind::PromptSendBlocked,
            pane_id: "%1".into(),
            severity: Severity::Warning,
            summary: "%1 /compact (blocked; observe_only mode)".into(),
            provider: Some(Provider::Claude),
            role: Some(Role::Main),
        };
        assert_ne!(completed.kind, failed.kind);
        assert_ne!(failed.kind, blocked.kind);
        assert_ne!(completed.kind, blocked.kind);
        // These must be different from the existing P5-1 kinds.
        assert_ne!(completed.kind, AuditEventKind::PromptSendAccepted);
        assert_ne!(blocked.kind, AuditEventKind::PromptSendRejected);
        // Copy usage (no move required).
        let _c = completed.kind;
        let _f = failed.kind;
        let _b = blocked.kind;
    }
}
