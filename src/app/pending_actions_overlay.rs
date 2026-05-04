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
}
