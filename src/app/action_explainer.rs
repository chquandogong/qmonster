//! Phase D (v1.38) — Action Explainer pre-confirm modal state + view builders.
//!
//! Operators press p/d/y to act on a pending prompt-send proposal or to copy
//! an alert's suggested_command. Before this bundle, those keys executed
//! immediately. With v1.38's [ux] confirm_actions config, the keystroke can
//! instead open this modal so the operator sees what will happen, why, and
//! the audit chain — observe-first.
//!
//! Pure state. No rendering (Task 19) and no tui_loop wiring (Task 20).

use crate::app::config::ActionsMode;
use crate::app::event_loop::PaneReport;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    AcceptPromptSend { pane_idx: usize },
    RejectPromptSend { pane_idx: usize },
    CopyAlertCommand { alert_idx: usize },
}

#[derive(Debug, Default)]
pub struct ActionExplainModal {
    pending: Option<PendingAction>,
    view: Option<ActionExplainView>,
    seen_accept: bool,
    seen_reject: bool,
    seen_copy: bool,
}

#[derive(Debug, Clone)]
pub struct ActionExplainView {
    pub title: String,
    pub target_label: String,
    pub payload_label: String,
    pub payload_text: String,
    pub why: String,
    pub why_source: SourceKind,
    pub severity: Option<Severity>,
    pub audit_chain: String,
    pub mode_warning: Option<String>,
}

impl ActionExplainModal {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_open(&self) -> bool {
        self.view.is_some()
    }
    pub fn close(&mut self) {
        self.pending = None;
        self.view = None;
    }
    pub fn pending(&self) -> Option<&PendingAction> {
        self.pending.as_ref()
    }
    pub fn view(&self) -> Option<&ActionExplainView> {
        self.view.as_ref()
    }
    pub fn open(&mut self, action: PendingAction, view: ActionExplainView) {
        self.pending = Some(action);
        self.view = Some(view);
    }
    pub fn mark_seen(&mut self, action: &PendingAction) {
        match action {
            PendingAction::AcceptPromptSend { .. } => self.seen_accept = true,
            PendingAction::RejectPromptSend { .. } => self.seen_reject = true,
            PendingAction::CopyAlertCommand { .. } => self.seen_copy = true,
        }
    }
    pub fn already_seen(&self, action: &PendingAction) -> bool {
        match action {
            PendingAction::AcceptPromptSend { .. } => self.seen_accept,
            PendingAction::RejectPromptSend { .. } => self.seen_reject,
            PendingAction::CopyAlertCommand { .. } => self.seen_copy,
        }
    }
}

/// Returns `true` when the keystroke should open the Action Explainer
/// modal vs execute immediately. Centralizes the Always / FirstTime /
/// Never decision in one pure function. Spec §6.2.
///
/// `mode`: from `[ux] confirm_actions` config.
/// `already_seen`: per-kind seen flag from `ActionExplainModal::already_seen`.
/// `has_target`: true when there's something to confirm (proposal exists, or
/// alert has suggested_command). When false, fall through to the underlying
/// handler so it can emit its own "no proposal" / "nothing to copy" notice.
pub fn should_open_explainer(
    mode: crate::app::config::ConfirmActions,
    already_seen: bool,
    has_target: bool,
) -> bool {
    use crate::app::config::ConfirmActions;
    if !has_target {
        return false;
    }
    match mode {
        ConfirmActions::Always => true,
        ConfirmActions::FirstTime => !already_seen,
        ConfirmActions::Never => false,
    }
}

/// Build the explainer view for `p` (accept). The `target` and `payload`
/// inputs already come from the existing `first_prompt_send_proposal`
/// helper at the caller. The audit-chain string and mode-warning are
/// derived per spec §6.3 from the live `actions.mode` and the
/// `allow_auto_prompt_send` flag — these mirror the three
/// `PromptSendGate` verdicts in `policy/gates.rs` (Execute /
/// AutoSendOff / Blocked).
pub fn build_accept_view(
    report: &PaneReport,
    slash: &str,
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
) -> ActionExplainView {
    let target_label = pane_label(report);
    let (audit_chain, mode_warning) = match mode {
        ActionsMode::ObserveOnly => (
            "observe_only \u{2192} PromptSendBlocked".to_string(),
            Some("observe_only  \u{26A0}  prompt sends are blocked".to_string()),
        ),
        _ if !allow_auto_prompt_send => (
            "AutoSendOff \u{2192} PromptSendAccepted + PromptSendBlocked".to_string(),
            Some("auto-send disabled  \u{26A0}  proposal will be marked blocked".to_string()),
        ),
        _ => (
            "Execute \u{2192} PromptSendAccepted \u{2192} PromptSendCompleted (or PromptSendFailed)"
                .to_string(),
            None,
        ),
    };
    let (why, why_source, severity) = first_proposal_reason(report);
    ActionExplainView {
        title: "Accept prompt-send proposal".to_string(),
        target_label,
        payload_label: "What to send".to_string(),
        payload_text: slash.to_string(),
        why,
        why_source,
        severity,
        audit_chain,
        mode_warning,
    }
}

pub fn build_reject_view(report: &PaneReport, slash: &str) -> ActionExplainView {
    let target_label = pane_label(report);
    let (why, why_source, severity) = first_proposal_reason(report);
    ActionExplainView {
        title: "Reject prompt-send proposal".to_string(),
        target_label,
        payload_label: "What to reject".to_string(),
        payload_text: slash.to_string(),
        why,
        why_source,
        severity,
        audit_chain: "PromptSendRejected".to_string(),
        mode_warning: None,
    }
}

pub fn build_copy_view(
    alert_title: &str,
    suggested_command: &str,
    severity: Option<Severity>,
    source_kind: SourceKind,
) -> ActionExplainView {
    ActionExplainView {
        title: "Copy alert command".to_string(),
        target_label: alert_title.to_string(),
        payload_label: "What to copy".to_string(),
        payload_text: suggested_command.to_string(),
        why: alert_title.to_string(),
        why_source: source_kind,
        severity,
        audit_chain: "(no audit event \u{2014} clipboard write only)".to_string(),
        mode_warning: None,
    }
}

/// Helper — derive a stable `provider:instance:role · pane_id` label for the
/// modal. Mirrors the `provider:instance:role` convention already used by
/// `src/ui/metrics.rs::pane_label`, with the pane_id appended after a
/// middle dot so the operator can see which tmux pane is the target.
fn pane_label(report: &PaneReport) -> String {
    use crate::domain::identity::{Provider, Role};
    let id = &report.identity.identity;
    let provider = match id.provider {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
        Provider::Gemini => "gemini",
        Provider::Qmonster => "qmonster",
        Provider::Unknown => "?",
    };
    let role = match id.role {
        Role::Main => "main",
        Role::Review => "review",
        Role::Research => "research",
        Role::Monitor => "monitor",
        Role::Unknown => "?",
    };
    format!(
        "{provider}:{}:{role} \u{B7} {}",
        id.instance, report.pane_id
    )
}

/// Helper — find the highest-severity recommendation whose
/// `suggested_command` is a slash-style command and surface its reason /
/// source / severity. The exact linkage from a `PromptSendProposed`
/// effect back to its originating `Recommendation` lives inside the
/// policy engine and would require a wider refactor to expose here; for
/// the explainer the "highest-severity slash recommendation on this
/// pane" heuristic is acceptable — it gives the operator the most
/// salient `[SourceKind]` reason for the modal, and falls back to a
/// neutral default when nothing matches.
fn first_proposal_reason(report: &PaneReport) -> (String, SourceKind, Option<Severity>) {
    let candidate = report
        .recommendations
        .iter()
        .filter(|r| {
            r.suggested_command
                .as_deref()
                .is_some_and(|c| c.starts_with('/'))
        })
        .max_by_key(|r| r.severity);
    match candidate {
        Some(r) => (r.reason.clone(), r.source_kind, Some(r.severity)),
        None => (
            "operator-initiated review".to_string(),
            SourceKind::ProjectCanonical,
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::origin::SourceKind;

    #[test]
    fn open_sets_pending_and_view() {
        let mut m = ActionExplainModal::new();
        assert!(!m.is_open());
        m.open(
            PendingAction::AcceptPromptSend { pane_idx: 0 },
            sample_view(),
        );
        assert!(m.is_open());
        assert_eq!(
            m.pending(),
            Some(&PendingAction::AcceptPromptSend { pane_idx: 0 })
        );
        assert!(m.view().is_some());
    }

    #[test]
    fn close_clears_state() {
        let mut m = ActionExplainModal::new();
        m.open(
            PendingAction::AcceptPromptSend { pane_idx: 0 },
            sample_view(),
        );
        m.close();
        assert!(!m.is_open());
        assert!(m.pending().is_none());
        assert!(m.view().is_none());
    }

    #[test]
    fn first_time_seen_tracking_is_per_kind() {
        let mut m = ActionExplainModal::new();
        let accept = PendingAction::AcceptPromptSend { pane_idx: 0 };
        let reject = PendingAction::RejectPromptSend { pane_idx: 0 };
        let copy = PendingAction::CopyAlertCommand { alert_idx: 0 };
        assert!(!m.already_seen(&accept));
        assert!(!m.already_seen(&reject));
        assert!(!m.already_seen(&copy));
        m.mark_seen(&accept);
        assert!(m.already_seen(&accept));
        assert!(!m.already_seen(&reject));
        assert!(!m.already_seen(&copy));
        m.mark_seen(&copy);
        assert!(m.already_seen(&copy));
        assert!(!m.already_seen(&reject));
    }

    #[test]
    fn should_open_explainer_predicate_covers_all_modes() {
        use crate::app::config::ConfirmActions;
        // (mode, already_seen, has_target) -> expected open?
        assert!(should_open_explainer(ConfirmActions::Always, false, true));
        assert!(should_open_explainer(ConfirmActions::Always, true, true));
        assert!(should_open_explainer(
            ConfirmActions::FirstTime,
            false,
            true
        ));
        assert!(!should_open_explainer(
            ConfirmActions::FirstTime,
            true,
            true
        ));
        assert!(!should_open_explainer(ConfirmActions::Never, false, true));
        assert!(!should_open_explainer(ConfirmActions::Never, true, true));
        assert!(
            !should_open_explainer(ConfirmActions::Always, false, false),
            "no target => skip modal"
        );
    }

    #[test]
    fn build_accept_view_includes_observe_only_warning() {
        let rep = sample_pane_report();
        let v = build_accept_view(&rep, "/compact", ActionsMode::ObserveOnly, true);
        assert!(v.audit_chain.contains("observe_only"));
        assert!(v.audit_chain.contains("PromptSendBlocked"));
        let warning = v.mode_warning.expect("observe_only must surface a warning");
        assert!(warning.contains("blocked"));
    }

    #[test]
    fn build_copy_view_payload_text_equals_command() {
        let v = build_copy_view(
            "alert title",
            "/clear",
            Some(Severity::Concern),
            SourceKind::ProjectCanonical,
        );
        assert_eq!(v.payload_text, "/clear");
        assert_eq!(v.payload_label, "What to copy");
        assert!(v.audit_chain.contains("clipboard"));
    }

    fn sample_view() -> ActionExplainView {
        ActionExplainView {
            title: "Accept prompt-send proposal".into(),
            target_label: "codex:1:review \u{B7} %57".into(),
            payload_label: "What to send".into(),
            payload_text: "/compact".into(),
            why: "test".into(),
            why_source: SourceKind::ProjectCanonical,
            severity: Some(Severity::Concern),
            audit_chain: "Execute \u{2192} ...".into(),
            mode_warning: None,
        }
    }

    fn sample_pane_report() -> PaneReport {
        use crate::domain::identity::{
            IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
        };
        use crate::domain::signal::SignalSet;
        PaneReport {
            pane_id: "%57".into(),
            session_name: "qmonster".into(),
            window_index: "0".into(),
            provider: Provider::Codex,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Codex,
                    instance: 1,
                    role: Role::Review,
                    pane_id: "%57".into(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations: Vec::new(),
            effects: Vec::new(),
            dead: false,
            current_path: String::new(),
            current_command: String::new(),
            cross_pane_findings: Vec::new(),
            idle_state: None,
            idle_state_entered_at: None,
            recent_token_samples: Vec::new(),
        }
    }
}
