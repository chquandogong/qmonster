use crate::domain::identity::ResolvedIdentity;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::SignalSet;

/// Phase-1 alert rules. Pure function over signals. Each rule attaches
/// a `SourceKind` so the UI can surface authority honestly.
pub fn eval_alerts(_id: &ResolvedIdentity, s: &SignalSet) -> Vec<Recommendation> {
    let mut out = Vec::new();

    if s.waiting_for_input {
        out.push(Recommendation {
            action: "notify-input-wait",
            reason: "pane appears to be waiting for user input".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
        });
    }

    if s.permission_prompt {
        out.push(Recommendation {
            action: "notify-permission-wait",
            reason: "pane appears to require an approval".into(),
            severity: Severity::Risk,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
        });
    }

    if s.log_storm {
        out.push(Recommendation {
            action: "archive-preview-suggested",
            reason: "log storm pattern: consider keeping preview on screen and archiving the raw tail".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
        });
    }

    if s.repeated_output {
        out.push(Recommendation {
            action: "repeated-output-cache",
            reason: "identical output seen in recent polls; consider result caching".into(),
            severity: Severity::Concern,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
        });
    }

    if s.verbose_answer {
        out.push(Recommendation {
            action: "verbose-output",
            reason: "long/boilerplate output detected; terse profile may help".into(),
            severity: Severity::Concern,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
        });
    }

    if s.error_hint && !s.log_storm {
        out.push(Recommendation {
            action: "error-detected",
            reason: "error/trace-like text detected in pane tail".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
        });
    }

    if s.subagent_hint {
        out.push(Recommendation {
            action: "subagent-detected",
            reason: "a subagent was launched; token consumption may be delayed or missing in main stats".into(),
            severity: Severity::Concern,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role};
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
    fn input_wait_fires_high_severity() {
        let s = SignalSet {
            waiting_for_input: true,
            ..SignalSet::default()
        };
        let recs = eval_alerts(&id(), &s);
        assert!(recs.iter().any(|r| r.action == "notify-input-wait"));
    }

    #[test]
    fn permission_prompt_fires_high_severity() {
        let s = SignalSet {
            permission_prompt: true,
            ..SignalSet::default()
        };
        let recs = eval_alerts(&id(), &s);
        assert!(recs.iter().any(|r| r.action == "notify-permission-wait"));
    }

    #[test]
    fn log_storm_fires_warning_with_heuristic_source() {
        let s = SignalSet {
            log_storm: true,
            ..SignalSet::default()
        };
        let recs = eval_alerts(&id(), &s);
        let rec = recs
            .iter()
            .find(|r| r.action == "archive-preview-suggested")
            .expect("log storm recommendation");
        assert_eq!(rec.source_kind, SourceKind::Heuristic);
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
}
