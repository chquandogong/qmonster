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
}
