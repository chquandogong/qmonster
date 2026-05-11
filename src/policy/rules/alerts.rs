use crate::domain::identity::ResolvedIdentity;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::SignalSet;

/// Phase-1 alert rules. Pure function over signals. Each rule attaches
/// a `SourceKind` so the UI can surface authority honestly.
///
/// v2.2.0 dedup: log_storm was firing twice — once here (Warning alert)
/// and once via `advisories::log_storm_advisory` (Concern + quota_tight
/// aggressive variant + same archive suggested_command). The advisory is
/// strictly richer (carries the aggressive escalation), so the alert
/// path was removed. The Notify effect still fires whenever the
/// advisory's Concern severity escalates to Warning under quota_tight.
/// `ArchiveLocal` effect is unchanged.
pub fn eval_alerts(_id: &ResolvedIdentity, s: &SignalSet) -> Vec<Recommendation> {
    let mut out = Vec::new();

    if s.verbose_answer {
        out.push(Recommendation {
            action: "verbose-output",
            reason: "long/boilerplate output detected; terse profile may help".into(),
            severity: Severity::Concern,
            source_kind: SourceKind::Heuristic,
            suggested_command: None, // observation: action is a config change (see advisory)
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        });
    }

    // v2.2.0: the `!s.log_storm` guard used to prevent double-firing
    // alongside the log_storm alert. With the alert path removed, the
    // guard is no longer needed — the error_hint alert and the log_storm
    // advisory are distinct surfaces (Warning vs Concern, different
    // actions) and can co-exist when both signals are present.
    if s.error_hint {
        out.push(Recommendation {
            action: "error-detected",
            reason: "error/trace-like text detected in pane tail".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None, // error text varies by provider; no universal command
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        });
    }

    if s.subagent_hint {
        out.push(Recommendation {
            action: "subagent-detected",
            reason:
                "a subagent was launched; token consumption may be delayed or missing in main stats"
                    .into(),
            severity: Severity::Concern,
            source_kind: SourceKind::Heuristic,
            suggested_command: None, // informational; no action required
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::SignalSet;

    fn id() -> ResolvedIdentity {
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

    #[test]
    fn log_storm_alert_path_removed_in_v2_2_0() {
        // v2.2.0 dedup: log_storm now fires only via the advisories rule
        // (`log-storm: ingress filter + summary`), not the alerts rule.
        // The ArchiveLocal effect still fires from `engine::evaluate`.
        let s = SignalSet {
            log_storm: true,
            ..SignalSet::default()
        };
        let recs = eval_alerts(&id(), &s);
        assert!(
            !recs.iter().any(|r| r.action == "archive-preview-suggested"),
            "log_storm alert was removed in favor of the richer advisory; \
             this test pins the dedup so a future refactor cannot resurrect \
             the duplicate fire without flipping this assertion"
        );
    }

    #[test]
    fn subagent_hint_fires_concern() {
        let s = SignalSet {
            subagent_hint: true,
            ..SignalSet::default()
        };
        let recs = eval_alerts(&id(), &s);
        assert!(recs.iter().any(|r| r.action == "subagent-detected"));
    }

    #[test]
    fn clean_signal_set_fires_no_alerts() {
        let s = SignalSet::default();
        let recs = eval_alerts(&id(), &s);
        assert!(recs.is_empty());
    }

    #[test]
    fn verbose_answer_fires_concern_severity() {
        let s = SignalSet {
            verbose_answer: true,
            ..SignalSet::default()
        };
        let recs = eval_alerts(&id(), &s);
        let rec = recs
            .iter()
            .find(|r| r.action == "verbose-output")
            .expect("verbose_answer must fire a recommendation");
        assert_eq!(rec.severity, Severity::Concern);
        assert_eq!(rec.source_kind, SourceKind::Heuristic);
    }

    #[test]
    fn error_hint_fires_warning_when_not_log_storm() {
        let s = SignalSet {
            error_hint: true,
            log_storm: false,
            ..SignalSet::default()
        };
        let recs = eval_alerts(&id(), &s);
        let rec = recs
            .iter()
            .find(|r| r.action == "error-detected")
            .expect("error_hint without log_storm must fire");
        assert_eq!(rec.severity, Severity::Warning);
    }
}
