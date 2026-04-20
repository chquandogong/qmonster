use crate::app::config::{QmonsterConfig, SafetyOverride, apply_safety_override};
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::recommendation::Severity;
use crate::store::sink::EventSink;

/// Summary of one override-application pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OverrideStats {
    pub accepted: u32,
    pub rejected: u32,
    pub unknown: u32,
}

/// Apply a batch of `(key, value)` safety overrides and route rejections
/// and unknown keys to the audit sink as required by
/// `docs/ai/VALIDATION.md` §69-72 (r2 §4). Accepted overrides are silent.
///
/// `Rejected` becomes a `risk`-severity audit event (someone tried to
/// move a safety flag toward more permissive). `UnknownKey` becomes
/// `concern` — it's a configuration mistake but not an attempt to
/// bypass safety.
pub fn apply_override_with_audit(
    cfg: &mut QmonsterConfig,
    kvs: &[(&str, &str)],
    sink: &dyn EventSink,
) -> OverrideStats {
    let mut stats = OverrideStats::default();
    for (key, value) in kvs {
        match apply_safety_override(cfg, key, value) {
            SafetyOverride::Accepted => {
                stats.accepted += 1;
            }
            SafetyOverride::Rejected { reason } => {
                stats.rejected += 1;
                sink.record(AuditEvent {
                    kind: AuditEventKind::SafetyOverrideRejected,
                    pane_id: "n/a".into(),
                    severity: Severity::Risk,
                    summary: format!("rejected {key}={value}: {reason}"),
                    provider: None,
                    role: None,
                });
            }
            SafetyOverride::UnknownKey => {
                stats.unknown += 1;
                sink.record(AuditEvent {
                    kind: AuditEventKind::SafetyOverrideRejected,
                    pane_id: "n/a".into(),
                    severity: Severity::Concern,
                    summary: format!("unknown config key: {key}"),
                    provider: None,
                    role: None,
                });
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config::QmonsterConfig;
    use crate::domain::audit::AuditEventKind;
    use crate::store::sink::InMemorySink;

    #[test]
    fn accepted_override_produces_no_audit_event() {
        let mut cfg = QmonsterConfig::defaults();
        let sink = InMemorySink::new();
        let results = apply_override_with_audit(
            &mut cfg,
            &[("actions.mode", "observe_only")],
            &sink,
        );
        assert_eq!(results.accepted, 1);
        assert_eq!(results.rejected, 0);
        assert_eq!(sink.len(), 0);
    }

    #[test]
    fn rejected_override_writes_risk_audit_event() {
        let mut cfg = QmonsterConfig::defaults();
        let sink = InMemorySink::new();
        let results = apply_override_with_audit(
            &mut cfg,
            &[("actions.mode", "safe_auto")],
            &sink,
        );
        assert_eq!(results.rejected, 1);
        let events = sink.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::SafetyOverrideRejected);
        assert_eq!(
            events[0].severity,
            crate::domain::recommendation::Severity::Risk
        );
    }

    #[test]
    fn unknown_key_is_also_audit_logged_at_concern_severity() {
        let mut cfg = QmonsterConfig::defaults();
        let sink = InMemorySink::new();
        let results = apply_override_with_audit(
            &mut cfg,
            &[("nonexistent.key", "x")],
            &sink,
        );
        assert_eq!(results.unknown, 1);
        let events = sink.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::SafetyOverrideRejected);
        assert_eq!(
            events[0].severity,
            crate::domain::recommendation::Severity::Concern
        );
    }

    #[test]
    fn multiple_overrides_produce_multiple_events() {
        let mut cfg = QmonsterConfig::defaults();
        let sink = InMemorySink::new();
        let results = apply_override_with_audit(
            &mut cfg,
            &[
                ("actions.allow_auto_prompt_send", "true"),
                ("actions.allow_destructive_actions", "true"),
                ("logging.sensitivity", "forensic"),
            ],
            &sink,
        );
        assert_eq!(results.accepted, 1);
        assert_eq!(results.rejected, 2);
        assert_eq!(sink.len(), 2);
    }
}
