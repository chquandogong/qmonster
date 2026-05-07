use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::keymap::rect_contains;
use crate::ui::dashboard::close_button_rect;
use crate::ui::insights::InsightsOverlay;
use crate::ui::modal_chrome::{DragAnchor, title_row_contains};

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
        KeyCode::Char('[') => {
            overlay.shrink();
            InsightsOverlayAction::None
        }
        KeyCode::Char(']') => {
            overlay.grow();
            InsightsOverlayAction::None
        }
        KeyCode::Char('=') => {
            overlay.reset_geometry();
            InsightsOverlayAction::None
        }
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
    let area = crate::ui::insights::insights_modal_area_for(viewport, overlay);
    let close = close_button_rect(area);
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
            if rect_contains(close, event.column, event.row)
                || !rect_contains(area, event.column, event.row)
            {
                overlay.close();
            } else if title_row_contains(area, event.column, event.row) {
                overlay.begin_drag(DragAnchor {
                    start_col: event.column,
                    start_row: event.row,
                    start_offset_x: overlay.offset_x(),
                    start_offset_y: overlay.offset_y(),
                });
            }
            InsightsOverlayAction::None
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if let Some(anchor) = overlay.drag_anchor() {
                let dx = event.column as i32 - anchor.start_col as i32;
                let dy = event.row as i32 - anchor.start_row as i32;
                let next_x =
                    (anchor.start_offset_x as i32 + dx).clamp(i16::MIN as i32, i16::MAX as i32);
                let next_y =
                    (anchor.start_offset_y as i32 + dy).clamp(i16::MIN as i32, i16::MAX as i32);
                overlay.set_offset(next_x as i16, next_y as i16);
            }
            InsightsOverlayAction::None
        }
        MouseEventKind::Up(MouseButton::Left) => {
            overlay.end_drag();
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
    fn bracket_keys_resize_insights_overlay() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        let (w0, h0) = (overlay.width_pct(), overlay.height_pct());
        assert_eq!(
            handle_insights_overlay_key(&mut overlay, KeyCode::Char(']')),
            InsightsOverlayAction::None
        );
        assert!(overlay.width_pct() > w0);
        assert!(overlay.height_pct() > h0);
        assert_eq!(
            handle_insights_overlay_key(&mut overlay, KeyCode::Char('[')),
            InsightsOverlayAction::None
        );
        assert_eq!(
            handle_insights_overlay_key(&mut overlay, KeyCode::Char('=')),
            InsightsOverlayAction::None
        );
        assert_eq!(overlay.width_pct(), InsightsOverlay::DEFAULT_WIDTH_PCT);
        assert_eq!(overlay.height_pct(), InsightsOverlay::DEFAULT_HEIGHT_PCT);
    }

    #[test]
    fn title_drag_moves_insights_overlay_and_reset_clears_offset() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        let area = crate::ui::insights::insights_modal_area_for(viewport, &overlay);
        handle_insights_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), area.x + 2, area.y),
        );
        handle_insights_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                area.x + 7,
                area.y + 3,
            ),
        );
        assert_eq!(overlay.offset_x(), 5);
        assert_eq!(overlay.offset_y(), 3);
        assert_eq!(
            handle_insights_overlay_key(&mut overlay, KeyCode::Char('=')),
            InsightsOverlayAction::None
        );
        assert_eq!(overlay.offset_x(), 0);
        assert_eq!(overlay.offset_y(), 0);
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
