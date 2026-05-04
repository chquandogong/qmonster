//! Phase C (v1.38) — Metrics Overlay key + mouse dispatchers.

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::keymap::rect_contains;
use crate::ui::dashboard::close_button_rect;
use crate::ui::metrics::{MetricsOverlay, metrics_modal_rects};

pub fn handle_metrics_overlay_key(
    overlay: &mut MetricsOverlay,
    reports_len: usize,
    code: KeyCode,
) -> bool {
    if !overlay.is_open() {
        return false;
    }
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('m') => overlay.close(),
        KeyCode::Up | KeyCode::Char('k') => overlay.select_prev(reports_len),
        KeyCode::Down | KeyCode::Char('j') => overlay.select_next(reports_len),
        _ => {}
    }
    true
}

pub fn handle_metrics_overlay_mouse(
    overlay: &mut MetricsOverlay,
    viewport: Rect,
    _reports_len: usize,
    event: MouseEvent,
) {
    if !overlay.is_open() {
        return;
    }
    let rects = metrics_modal_rects(viewport);
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && rect_contains(close_button_rect(rects.area), event.column, event.row)
    {
        overlay.close();
        return;
    }
    match event.kind {
        MouseEventKind::ScrollUp => overlay.scroll_up(),
        // Soft cap on internal scroll state. Ratatui's `Paragraph::scroll`
        // self-clips visible output, so the cap exists purely to keep
        // `MetricsOverlay::scroll` from drifting unboundedly when users
        // wheel past the actual content. 200 lines comfortably exceeds
        // the modal body even on extreme pane counts.
        MouseEventKind::ScrollDown => overlay.scroll_down(200),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::metrics::MetricsOverlay;

    #[test]
    fn m_key_closes_open_overlay() {
        let mut o = MetricsOverlay::new();
        o.open();
        assert!(handle_metrics_overlay_key(&mut o, 0, KeyCode::Char('m')));
        assert!(!o.is_open());
    }

    #[test]
    fn arrows_move_selection() {
        let mut o = MetricsOverlay::new();
        o.open();
        handle_metrics_overlay_key(&mut o, 3, KeyCode::Down);
        assert_eq!(o.selected(), 1);
        handle_metrics_overlay_key(&mut o, 3, KeyCode::Up);
        assert_eq!(o.selected(), 0);
    }

    #[test]
    fn key_handler_returns_false_when_closed() {
        let mut o = MetricsOverlay::new();
        // not opened
        assert!(!handle_metrics_overlay_key(&mut o, 0, KeyCode::Char('m')));
    }

    #[test]
    fn x_click_closes_overlay() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut o = MetricsOverlay::new();
        o.open();
        let viewport = Rect::new(0, 0, 100, 50);
        let rects = metrics_modal_rects(viewport);
        let close_rect = close_button_rect(rects.area);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: close_rect.x,
            row: close_rect.y,
            modifiers: KeyModifiers::empty(),
        };
        handle_metrics_overlay_mouse(&mut o, viewport, 0, event);
        assert!(!o.is_open());
    }
}
