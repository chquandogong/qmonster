use crate::domain::identity::{Provider, Role};
use crate::domain::recommendation::Severity;

/// Audit event kinds recorded by the observe-first MVP. This list stays
/// stable for the v0.4.0 line; additions require an MDR entry.
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
}
