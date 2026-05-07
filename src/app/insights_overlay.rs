use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::ui::insights::InsightsOverlay;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsightsOverlayAction {
    None,
    Refresh,
}

pub fn handle_insights_overlay_key(
    overlay: &mut InsightsOverlay,
    key: KeyCode,
) -> InsightsOverlayAction {
    if !overlay.is_open() {
        return InsightsOverlayAction::None;
    }
    match key {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('i') => {
            overlay.close();
            InsightsOverlayAction::None
        }
        KeyCode::Char('r') => InsightsOverlayAction::Refresh,
        KeyCode::Up | KeyCode::Char('k') => {
            overlay.scroll_up();
            InsightsOverlayAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            overlay.scroll_down(overlay.line_count().saturating_sub(1) as u16);
            InsightsOverlayAction::None
        }
        _ => InsightsOverlayAction::None,
    }
}

pub fn handle_insights_overlay_mouse(
    overlay: &mut InsightsOverlay,
    viewport: Rect,
    event: MouseEvent,
) -> InsightsOverlayAction {
    if !overlay.is_open() {
        return InsightsOverlayAction::None;
    }
    match event.kind {
        MouseEventKind::ScrollDown => {
            overlay.scroll_down(overlay.line_count().saturating_sub(1) as u16);
            InsightsOverlayAction::None
        }
        MouseEventKind::ScrollUp => {
            overlay.scroll_up();
            InsightsOverlayAction::None
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let area = crate::ui::insights::insights_modal_area(viewport);
            if crate::app::keymap::rect_contains(
                crate::ui::dashboard::close_button_rect(area),
                event.column,
                event.row,
            ) || !crate::app::keymap::rect_contains(area, event.column, event.row)
            {
                overlay.close();
            }
            InsightsOverlayAction::None
        }
        _ => InsightsOverlayAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEvent};

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn i_closes_open_overlay() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        assert_eq!(
            handle_insights_overlay_key(&mut overlay, KeyCode::Char('i')),
            InsightsOverlayAction::None
        );
        assert!(!overlay.is_open());
    }

    #[test]
    fn r_requests_refresh() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        assert_eq!(
            handle_insights_overlay_key(&mut overlay, KeyCode::Char('r')),
            InsightsOverlayAction::Refresh
        );
        assert!(overlay.is_open());
    }

    #[test]
    fn mouse_left_on_close_button_closes_overlay() {
        let viewport = Rect::new(0, 0, 100, 40);
        let area = crate::ui::insights::insights_modal_area(viewport);
        let close = crate::ui::dashboard::close_button_rect(area);
        let mut overlay = InsightsOverlay::new();
        overlay.open();

        handle_insights_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), close.x, close.y),
        );

        assert!(!overlay.is_open());
    }

    #[test]
    fn mouse_left_inside_body_keeps_overlay_open() {
        let viewport = Rect::new(0, 0, 100, 40);
        let area = crate::ui::insights::insights_modal_area(viewport);
        let mut overlay = InsightsOverlay::new();
        overlay.open();

        handle_insights_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                area.x.saturating_add(2),
                area.y.saturating_add(2),
            ),
        );

        assert!(overlay.is_open());
    }
}
