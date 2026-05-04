//! Phase C (v1.38) — Metrics Overlay key + mouse dispatchers.
//!
//! v1.38 layout v2: selection state is gone — all panes' metrics live
//! inline in per-pane cards. ↑/↓/j/k now scroll the body, not select
//! a pane. Mouse wheel still scrolls; `[x]` click and `m`/`Esc`/`q`
//! still close.

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::keymap::rect_contains;
use crate::ui::dashboard::close_button_rect;
use crate::ui::metrics::{MetricsOverlay, metrics_modal_rects};

/// Soft cap on the scroll counter. Ratatui's `Paragraph::scroll`
/// self-clips visible output, so the cap exists purely to keep
/// `MetricsOverlay::scroll` from drifting unboundedly when users
/// hold ↓ or wheel past the actual content. 200 lines comfortably
/// exceeds the modal body even on extreme pane counts.
const SCROLL_MAX: u16 = 200;

pub fn handle_metrics_overlay_key(
    overlay: &mut MetricsOverlay,
    _reports_len: usize,
    code: KeyCode,
) -> bool {
    if !overlay.is_open() {
        return false;
    }
    match code {
        KeyCode::Char('[') => overlay.shrink(),
        KeyCode::Char(']') => overlay.grow(),
        KeyCode::Char('=') => overlay.reset_size(),
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('m') => overlay.close(),
        KeyCode::Up | KeyCode::Char('k') => overlay.scroll_up(),
        KeyCode::Down | KeyCode::Char('j') => overlay.scroll_down(SCROLL_MAX),
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
    let rects = metrics_modal_rects(viewport, overlay.width_pct(), overlay.height_pct());
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && rect_contains(close_button_rect(rects.area), event.column, event.row)
    {
        overlay.close();
        return;
    }
    match event.kind {
        MouseEventKind::ScrollUp => overlay.scroll_up(),
        MouseEventKind::ScrollDown => overlay.scroll_down(SCROLL_MAX),
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
    fn arrows_scroll_body() {
        let mut o = MetricsOverlay::new();
        o.open();
        handle_metrics_overlay_key(&mut o, 3, KeyCode::Down);
        handle_metrics_overlay_key(&mut o, 3, KeyCode::Down);
        assert_eq!(o.scroll(), 2);
        handle_metrics_overlay_key(&mut o, 3, KeyCode::Up);
        assert_eq!(o.scroll(), 1);
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
        let rects = metrics_modal_rects(viewport, o.width_pct(), o.height_pct());
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

    #[test]
    fn bracket_keys_resize_overlay() {
        let mut o = MetricsOverlay::new();
        o.open();
        let (w0, h0) = (o.width_pct(), o.height_pct());
        // ']' grows
        assert!(handle_metrics_overlay_key(&mut o, 0, KeyCode::Char(']')));
        assert!(o.width_pct() >= w0);
        assert!(o.height_pct() >= h0);
        // '[' shrinks past the original
        let (w1, h1) = (o.width_pct(), o.height_pct());
        assert!(handle_metrics_overlay_key(&mut o, 0, KeyCode::Char('[')));
        assert!(o.width_pct() <= w1);
        assert!(o.height_pct() <= h1);
        // '=' resets to defaults
        assert!(handle_metrics_overlay_key(&mut o, 0, KeyCode::Char('=')));
        assert_eq!(o.width_pct(), MetricsOverlay::DEFAULT_WIDTH_PCT);
        assert_eq!(o.height_pct(), MetricsOverlay::DEFAULT_HEIGHT_PCT);
    }
}
