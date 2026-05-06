//! Phase v1.40 — key + mouse dispatchers for the Pending Actions
//! overlay. The overlay state and renderer live in
//! `ui::pending_actions`; this module wires keystrokes to the
//! overlay so `tui_loop` can stay slim.

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::ui::dashboard::close_button_rect;
use crate::ui::pending_actions::{PendingActionsOverlay, PendingItem, pending_actions_modal_area};

/// Outcome of a key press while the overlay is open.
///
/// - `None`         — event consumed, no further action needed.
/// - `Closed`       — overlay was closed (already reflected in overlay state).
/// - `AcceptItems`  — operator confirmed accept for these item indices (Task 10/12).
/// - `ClearItems`   — operator confirmed clear/reject for these item indices (Task 10/12).
/// - `CopyItem`     — operator confirmed copy for this item index (Task 10/12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingActionsOutcome {
    None,
    Closed,
    AcceptItems(Vec<usize>),
    ClearItems(Vec<usize>),
    CopyItem(usize),
}

pub fn handle_pending_actions_overlay_key(
    overlay: &mut PendingActionsOverlay,
    items: &[PendingItem],
    code: KeyCode,
) -> PendingActionsOutcome {
    if !overlay.is_open() {
        return PendingActionsOutcome::None;
    }
    match code {
        KeyCode::Char('a') | KeyCode::Char('q') | KeyCode::Esc => {
            overlay.close();
            PendingActionsOutcome::Closed
        }
        KeyCode::Up | KeyCode::Char('k') => {
            overlay.select_prev(items.len());
            PendingActionsOutcome::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            overlay.select_next(items.len());
            PendingActionsOutcome::None
        }
        KeyCode::Char(' ') => {
            if let Some(item) = items.get(overlay.selected()) {
                overlay.toggle_multi(item);
            }
            PendingActionsOutcome::None
        }
        KeyCode::Char('P') => {
            overlay.toggle_group_proposals(items);
            PendingActionsOutcome::None
        }
        KeyCode::Char('Y') => {
            overlay.toggle_group_alerts(items);
            PendingActionsOutcome::None
        }
        KeyCode::Char('A') => {
            overlay.toggle_group_all(items);
            PendingActionsOutcome::None
        }
        KeyCode::Char('c') => {
            overlay.clear_multi();
            PendingActionsOutcome::None
        }
        KeyCode::Char('p') | KeyCode::Char('d') | KeyCode::Char('y') | KeyCode::Enter => {
            // Dispatch keys filled in by Task 10.
            PendingActionsOutcome::None
        }
        _ => PendingActionsOutcome::None,
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

    fn make_three_proposals() -> Vec<crate::ui::pending_actions::PendingItem> {
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::PendingItem;
        (0..3)
            .map(|i| PendingItem::Proposal {
                pane_idx: i,
                pane_label: format!("pane {i}"),
                slash_command: "/compact".into(),
                severity: None,
                source: SourceKind::ProjectCanonical,
                proposal_id: format!("%{i}:/compact"),
                target_pane_id: format!("%{i}"),
            })
            .collect()
    }

    #[test]
    fn closed_overlay_swallows_keys() {
        let mut overlay = PendingActionsOverlay::new();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Enter);
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert!(!overlay.is_open());
    }

    #[test]
    fn a_key_closes_overlay() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char('a'));
        assert_eq!(outcome, PendingActionsOutcome::Closed);
        assert!(!overlay.is_open());
    }

    #[test]
    fn q_and_esc_close_overlay() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Esc);
        assert_eq!(outcome, PendingActionsOutcome::Closed);
        assert!(!overlay.is_open());

        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char('q'));
        assert_eq!(outcome, PendingActionsOutcome::Closed);
        assert!(!overlay.is_open());
    }

    #[test]
    fn arrow_keys_navigate() {
        let items = make_three_proposals();
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Down);
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert_eq!(overlay.selected(), 1);
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Up);
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert_eq!(overlay.selected(), 0);
    }

    #[test]
    fn jk_navigates_like_arrows() {
        let items = make_three_proposals();
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('j'));
        handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('j'));
        assert_eq!(overlay.selected(), 2);
        handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('k'));
        assert_eq!(overlay.selected(), 1);
    }

    #[test]
    fn enter_is_silently_swallowed() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Enter);
        assert_eq!(outcome, PendingActionsOutcome::None);
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

    #[test]
    fn space_toggles_multi_on_cursor_item() {
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::{PendingItem, pending_item_key};
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char(' '),
        );
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert!(overlay.multi_contains(&pending_item_key(&item)));
    }

    #[test]
    fn shift_p_toggles_proposal_group() {
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
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char('P'),
        );
        assert_eq!(overlay.multi_len(), 1);
        handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char('P'),
        );
        assert_eq!(overlay.multi_len(), 0);
    }

    #[test]
    fn c_clears_multi_only_keeps_cursor() {
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
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.set_selected(0);
        handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char(' '),
        );
        assert_eq!(overlay.multi_len(), 1);
        handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char('c'),
        );
        assert_eq!(overlay.multi_len(), 0);
        assert_eq!(overlay.selected(), 0, "c does not move the cursor");
    }
}
