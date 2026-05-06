//! Phase v1.39 — key + mouse dispatchers for the Pending Actions
//! overlay. The overlay state and renderer live in
//! `ui::pending_actions`; this module wires keystrokes to the
//! overlay so `tui_loop` can stay slim.
//!
//! Pure dispatch — when the operator presses Enter, we return
//! `EnterSelected(idx)` so the caller can look up the underlying
//! `PendingItem`, drive `pane_state`/`alert_state` selection, and
//! open the Action Explainer modal.

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::ui::dashboard::close_button_rect;
use crate::ui::pending_actions::{PendingActionsOverlay, pending_actions_modal_area};

/// Outcome of a key press while the overlay is open. `None` = the
/// dispatcher consumed the event but no action is needed; `Closed`
/// = caller must mark the overlay closed (it's already closed in
/// the overlay state by the time this returns); `EnterSelected(idx)`
/// = caller must look up the item at `idx` and trigger the jump +
/// Action Explainer flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingActionsKeyOutcome {
    None,
    Closed,
    EnterSelected(usize),
}

pub fn handle_pending_actions_overlay_key(
    overlay: &mut PendingActionsOverlay,
    items_len: usize,
    code: KeyCode,
) -> PendingActionsKeyOutcome {
    if !overlay.is_open() {
        return PendingActionsKeyOutcome::None;
    }
    match code {
        KeyCode::Char('a') | KeyCode::Char('q') | KeyCode::Esc => {
            overlay.close();
            PendingActionsKeyOutcome::Closed
        }
        KeyCode::Up | KeyCode::Char('k') => {
            overlay.select_prev(items_len);
            PendingActionsKeyOutcome::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            overlay.select_next(items_len);
            PendingActionsKeyOutcome::None
        }
        KeyCode::Enter => {
            if items_len == 0 {
                PendingActionsKeyOutcome::None
            } else {
                let idx = overlay.selected().min(items_len.saturating_sub(1));
                PendingActionsKeyOutcome::EnterSelected(idx)
            }
        }
        _ => PendingActionsKeyOutcome::None,
    }
}

/// Process a mouse event while the overlay is open. Currently a
/// left-click on the `[x]` close button closes the modal; every
/// other mouse event is swallowed so it cannot leak through to the
/// dashboard. Returns `true` if the event closed the overlay.
pub fn handle_pending_actions_overlay_mouse(
    overlay: &mut PendingActionsOverlay,
    viewport: Rect,
    event: MouseEvent,
) -> bool {
    if !overlay.is_open() {
        return false;
    }
    let close_rect = close_button_rect(pending_actions_modal_area(viewport));
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && crate::app::keymap::rect_contains(close_rect, event.column, event.row)
    {
        overlay.close();
        return true;
    }
    false
}

use crate::app::action_explainer::{
    ActionExplainView, PendingAction, build_accept_view, build_copy_view,
};
use crate::app::config::ActionsMode;
use crate::app::event_loop::PaneReport;
use crate::ui::pending_actions::PendingItem;

/// Build the `AcceptPromptSend` action for a proposal item, or `None`
/// if the item is not a proposal. Used by the bulk `p` dispatch in
/// `tui_loop`.
pub fn accept_action_for(item: &PendingItem) -> Option<PendingAction> {
    match item {
        PendingItem::Proposal {
            target_pane_id,
            slash_command,
            proposal_id,
            ..
        } => Some(PendingAction::AcceptPromptSend {
            target_pane_id: target_pane_id.clone(),
            slash_command: slash_command.clone(),
            proposal_id: proposal_id.clone(),
        }),
        PendingItem::Copy { .. } => None,
    }
}

/// Build the `RejectPromptSend` action for a proposal item, or `None`
/// for non-proposal items. Used by the bulk `d` dispatch when the
/// item is a proposal (alerts use the hide path instead).
pub fn reject_action_for(item: &PendingItem) -> Option<PendingAction> {
    match item {
        PendingItem::Proposal {
            target_pane_id,
            slash_command,
            proposal_id,
            ..
        } => Some(PendingAction::RejectPromptSend {
            target_pane_id: target_pane_id.clone(),
            slash_command: slash_command.clone(),
            proposal_id: proposal_id.clone(),
        }),
        PendingItem::Copy { .. } => None,
    }
}

/// Build the `CopyAlertCommand` action for an alert item, or `None`
/// for proposals. Used by the bulk `y` dispatch.
pub fn copy_action_for(item: &PendingItem) -> Option<PendingAction> {
    match item {
        PendingItem::Copy {
            command,
            alert_title,
            ..
        } => Some(PendingAction::CopyAlertCommand {
            command: command.clone(),
            alert_title: alert_title.clone(),
        }),
        PendingItem::Proposal { .. } => None,
    }
}

/// Build an `ActionExplainView` for the right-pane live panel. For
/// proposals this is the accept-flavored view (the "primary" action a
/// `p` keypress would do); the operator can still press `d` to reject
/// without changing what the panel shows. Returns `None` when the
/// caller should render the empty-state placeholder.
pub fn build_explainer_view_for_item(
    item: &PendingItem,
    reports: &[PaneReport],
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
) -> Option<ActionExplainView> {
    match item {
        PendingItem::Proposal {
            pane_idx,
            slash_command,
            ..
        } => {
            let report = reports.get(*pane_idx)?;
            Some(build_accept_view(
                report,
                slash_command,
                mode,
                allow_auto_prompt_send,
            ))
        }
        PendingItem::Copy {
            command,
            alert_title,
            severity,
            source,
            ..
        } => Some(build_copy_view(
            alert_title,
            command,
            Some(*severity),
            *source,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_overlay_swallows_keys() {
        let mut overlay = PendingActionsOverlay::new();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Enter);
        assert_eq!(outcome, PendingActionsKeyOutcome::None);
        assert!(!overlay.is_open());
    }

    #[test]
    fn a_key_closes_overlay() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Char('a'));
        assert_eq!(outcome, PendingActionsKeyOutcome::Closed);
        assert!(!overlay.is_open());
    }

    #[test]
    fn q_and_esc_close_overlay() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Esc);
        assert_eq!(outcome, PendingActionsKeyOutcome::Closed);
        assert!(!overlay.is_open());

        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Char('q'));
        assert_eq!(outcome, PendingActionsKeyOutcome::Closed);
        assert!(!overlay.is_open());
    }

    #[test]
    fn arrow_keys_navigate() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Down);
        assert_eq!(outcome, PendingActionsKeyOutcome::None);
        assert_eq!(overlay.selected(), 1);
        let outcome = handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Up);
        assert_eq!(outcome, PendingActionsKeyOutcome::None);
        assert_eq!(overlay.selected(), 0);
    }

    #[test]
    fn jk_navigates_like_arrows() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Char('j'));
        handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Char('j'));
        assert_eq!(overlay.selected(), 2);
        handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Char('k'));
        assert_eq!(overlay.selected(), 1);
    }

    #[test]
    fn enter_returns_selected_idx() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Down);
        let outcome = handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Enter);
        assert_eq!(outcome, PendingActionsKeyOutcome::EnterSelected(1));
        assert!(
            overlay.is_open(),
            "Enter must NOT close the overlay; caller closes it after dispatch"
        );
    }

    #[test]
    fn enter_with_zero_items_is_no_op() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, 0, KeyCode::Enter);
        assert_eq!(outcome, PendingActionsKeyOutcome::None);
        assert!(overlay.is_open());
    }

    #[test]
    fn accept_action_for_proposal_returns_accept_variant() {
        use crate::app::action_explainer::PendingAction;
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::PendingItem;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let action = accept_action_for(&item).expect("proposal must produce an Accept action");
        match action {
            PendingAction::AcceptPromptSend {
                target_pane_id,
                slash_command,
                proposal_id,
            } => {
                assert_eq!(target_pane_id, "%1");
                assert_eq!(slash_command, "/compact");
                assert_eq!(proposal_id, "%1:/compact");
            }
            _ => panic!("expected AcceptPromptSend"),
        }
    }

    #[test]
    fn accept_action_for_alert_returns_none() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        use crate::ui::pending_actions::PendingItem;
        let item = PendingItem::Copy {
            alert_idx: 0,
            command: "/clear".into(),
            alert_title: "ctx".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        assert!(accept_action_for(&item).is_none());
    }

    #[test]
    fn reject_action_for_proposal_returns_reject_variant() {
        use crate::app::action_explainer::PendingAction;
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::PendingItem;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let action = reject_action_for(&item).expect("proposal must produce a Reject action");
        assert!(matches!(action, PendingAction::RejectPromptSend { .. }));
    }

    #[test]
    fn copy_action_for_alert_returns_copy_variant() {
        use crate::app::action_explainer::PendingAction;
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        use crate::ui::pending_actions::PendingItem;
        let item = PendingItem::Copy {
            alert_idx: 0,
            command: "/clear".into(),
            alert_title: "ctx".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        let action = copy_action_for(&item).expect("alert must produce a Copy action");
        match action {
            PendingAction::CopyAlertCommand {
                command,
                alert_title,
            } => {
                assert_eq!(command, "/clear");
                assert_eq!(alert_title, "ctx");
            }
            _ => panic!("expected CopyAlertCommand"),
        }
    }
}
