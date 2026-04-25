//! Slice 4: idle-state transition rule. Fires alerts when a pane
//! enters a non-None IdleCause from None, or transitions between
//! distinct causes. Same-cause repeats produce no new alert.

use crate::domain::identity::ResolvedIdentity;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::domain::signal::{IdleCause, SignalSet};

pub fn eval_idle_transition(
    id: &ResolvedIdentity,
    s: &SignalSet,
    last: Option<IdleCause>,
) -> Vec<Recommendation> {
    let Some(cause) = s.idle_state else {
        return vec![];
    };
    if last == Some(cause) {
        return vec![];
    }
    let provider = id.identity.provider;
    vec![Recommendation {
        action: "pane-state",
        reason: format!("{:?} pane state: {}", provider, cause_label(cause)),
        severity: severity_for(cause),
        source_kind: source_kind_for(cause),
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
    }]
}

fn cause_label(c: IdleCause) -> &'static str {
    match c {
        IdleCause::PermissionWait => "WAIT (approval)",
        IdleCause::InputWait => "WAIT (input)",
        IdleCause::LimitHit => "LIMIT",
        IdleCause::WorkComplete => "IDLE (done)",
        IdleCause::Stale => "IDLE (?)",
    }
}

fn severity_for(c: IdleCause) -> Severity {
    match c {
        IdleCause::PermissionWait | IdleCause::InputWait => Severity::Warning,
        IdleCause::LimitHit => Severity::Risk,
        IdleCause::WorkComplete | IdleCause::Stale => Severity::Concern,
    }
}

fn source_kind_for(c: IdleCause) -> SourceKind {
    match c {
        IdleCause::LimitHit => SourceKind::ProviderOfficial,
        IdleCause::Stale => SourceKind::Heuristic,
        _ => SourceKind::ProjectCanonical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role};

    fn id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity { provider: Provider::Claude, instance: 1, role: Role::Main, pane_id: "%1".into() },
            confidence: IdentityConfidence::High,
        }
    }

    #[test]
    fn none_to_some_input_wait_fires_warning() {
        let s = SignalSet { idle_state: Some(IdleCause::InputWait), ..SignalSet::default() };
        let recs = eval_idle_transition(&id(), &s, None);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].action, "pane-state");
        assert_eq!(recs[0].severity, Severity::Warning);
    }

    #[test]
    fn same_cause_repeat_fires_no_new_alert() {
        let s = SignalSet { idle_state: Some(IdleCause::WorkComplete), ..SignalSet::default() };
        let recs = eval_idle_transition(&id(), &s, Some(IdleCause::WorkComplete));
        assert!(recs.is_empty());
    }

    #[test]
    fn distinct_cause_transition_fires_new_alert() {
        let s = SignalSet { idle_state: Some(IdleCause::PermissionWait), ..SignalSet::default() };
        let recs = eval_idle_transition(&id(), &s, Some(IdleCause::InputWait));
        assert_eq!(recs.len(), 1);
    }

    #[test]
    fn limit_hit_fires_risk_severity() {
        let s = SignalSet { idle_state: Some(IdleCause::LimitHit), ..SignalSet::default() };
        let recs = eval_idle_transition(&id(), &s, None);
        assert_eq!(recs[0].severity, Severity::Risk);
        assert_eq!(recs[0].source_kind, SourceKind::ProviderOfficial);
    }

    #[test]
    fn work_complete_fires_concern_severity() {
        let s = SignalSet { idle_state: Some(IdleCause::WorkComplete), ..SignalSet::default() };
        let recs = eval_idle_transition(&id(), &s, None);
        assert_eq!(recs[0].severity, Severity::Concern);
    }

    #[test]
    fn stale_fires_concern_severity_with_heuristic_source() {
        let s = SignalSet { idle_state: Some(IdleCause::Stale), ..SignalSet::default() };
        let recs = eval_idle_transition(&id(), &s, None);
        assert_eq!(recs[0].severity, Severity::Concern);
        assert_eq!(recs[0].source_kind, SourceKind::Heuristic);
    }

    #[test]
    fn idle_state_none_fires_no_alert() {
        let s = SignalSet { idle_state: None, ..SignalSet::default() };
        let recs = eval_idle_transition(&id(), &s, Some(IdleCause::WorkComplete));
        assert!(recs.is_empty());
    }
}
