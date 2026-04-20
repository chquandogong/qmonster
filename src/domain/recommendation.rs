use crate::domain::origin::SourceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Safe,
    Good,
    Concern,
    Warning,
    Risk,
}

impl Severity {
    pub fn letter(self) -> &'static str {
        match self {
            Severity::Safe => "S",
            Severity::Good => "G",
            Severity::Concern => "C",
            Severity::Warning => "W",
            Severity::Risk => "R",
        }
    }
}

/// Advisory recommendation surfaced to the UI and (optionally) as an
/// audit event. Pure data — produced by `policy/` and consumed by
/// `app/` and `ui/`.
#[derive(Debug, Clone)]
pub struct Recommendation {
    pub action: &'static str,
    pub reason: String,
    pub severity: Severity,
    pub source_kind: SourceKind,
    pub suggested_command: Option<String>,
    pub side_effects: Vec<String>,
    /// G-7: if true, this recommendation is rendered in a dedicated
    /// "CHECKPOINT" slot above per-pane alerts in the alert queue and
    /// `--once` output.
    pub is_strong: bool,
}

/// Effects the policy engine wants `app::EffectRunner` to consider.
/// The runner decides which actually fire based on `actions.mode` and
/// the allow-list. Ordering is authority-sensitive: sensitive effects
/// compare as "greater" so the gate can reject the top of the range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RequestedEffect {
    Notify,
    ArchiveLocal,
    /// Reserved — no Phase 1 code path creates this; placeholder for the
    /// future allow-list gate. Never executed in `recommend_only`.
    SensitiveNotImplemented,
}

/// Cross-pane advisory. Emitted by `policy::Engine::evaluate_cross_pane`
/// when a rule observes overlap/concurrency across two or more panes.
/// Rendered in the alert queue alongside `SystemNotice`.
#[derive(Debug, Clone)]
pub struct CrossPaneFinding {
    pub kind: CrossPaneKind,
    pub anchor_pane_id: String,
    pub other_pane_ids: Vec<String>,
    pub reason: String,
    pub severity: Severity,
    pub source_kind: SourceKind,
    pub suggested_command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossPaneKind {
    /// Gemini G-11: two or more panes producing output in the same
    /// working directory — risk of divergent edits.
    ConcurrentMutatingWork,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::origin::SourceKind;

    #[test]
    fn severity_ordering_safe_lt_risk() {
        assert!(Severity::Safe < Severity::Good);
        assert!(Severity::Good < Severity::Concern);
        assert!(Severity::Concern < Severity::Warning);
        assert!(Severity::Warning < Severity::Risk);
    }

    #[test]
    fn recommendation_carries_source_kind_and_reason() {
        let r = Recommendation {
            action: "raw archive + preview",
            reason: "log storm detected".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
        };
        assert_eq!(r.severity, Severity::Warning);
        assert_eq!(r.source_kind, SourceKind::Heuristic);
    }

    #[test]
    fn requested_effect_ordering_preserves_allow_list_intent() {
        // Ordering: notify < archive < sensitive actions. The allow-list
        // gate in app::EffectRunner reads this ordering.
        assert!(RequestedEffect::Notify < RequestedEffect::ArchiveLocal);
        assert!(RequestedEffect::ArchiveLocal < RequestedEffect::SensitiveNotImplemented);
    }

    #[test]
    fn cross_pane_finding_carries_anchor_and_others() {
        let f = CrossPaneFinding {
            kind: CrossPaneKind::ConcurrentMutatingWork,
            anchor_pane_id: "%1".into(),
            other_pane_ids: vec!["%2".into(), "%3".into()],
            reason: "concurrent work on /repo".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: None,
        };
        assert_eq!(f.kind, CrossPaneKind::ConcurrentMutatingWork);
        assert_eq!(f.anchor_pane_id, "%1");
        assert_eq!(f.other_pane_ids.len(), 2);
        assert_eq!(f.severity, Severity::Warning);
    }
}
