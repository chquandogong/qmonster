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
    let rects = metrics_modal_rects(
        viewport,
        overlay.width_pct(),
        overlay.height_pct(),
        overlay.offset_x(),
        overlay.offset_y(),
    );
    let close = close_button_rect(rects.area);

    // 1. Close button always wins.
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && rect_contains(close, event.column, event.row)
    {
        overlay.close();
        return;
    }

    // 2. Drag start: Down(Left) on title row outside [x].
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && event.row == rects.area.y
        && event.column >= rects.area.x
        && event.column < rects.area.x + rects.area.width
        && !rect_contains(close, event.column, event.row)
    {
        overlay.begin_drag(crate::ui::metrics::DragAnchor {
            start_col: event.column,
            start_row: event.row,
            start_offset_x: overlay.offset_x(),
            start_offset_y: overlay.offset_y(),
        });
        return;
    }

    // 3. Drag update: Drag(Left) while anchored.
    if let MouseEventKind::Drag(MouseButton::Left) = event.kind
        && let Some(anchor) = overlay.drag_anchor()
    {
        let dx = (event.column as i32) - (anchor.start_col as i32);
        let dy = (event.row as i32) - (anchor.start_row as i32);
        let new_x = (anchor.start_offset_x as i32 + dx).clamp(i16::MIN as i32, i16::MAX as i32);
        let new_y = (anchor.start_offset_y as i32 + dy).clamp(i16::MIN as i32, i16::MAX as i32);
        overlay.set_offset(new_x as i16, new_y as i16);
        return;
    }

    // 4. Drag end: Up(Left) or any non-Drag mouse event clears anchor.
    if !matches!(event.kind, MouseEventKind::Drag(_)) {
        overlay.end_drag();
    }

    // 5. Wheel scroll (existing behavior, unchanged).
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
        let rects = metrics_modal_rects(
            viewport,
            o.width_pct(),
            o.height_pct(),
            o.offset_x(),
            o.offset_y(),
        );
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

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> crossterm::event::MouseEvent {
        crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::empty(),
        }
    }

    #[test]
    fn drag_down_on_title_row_sets_anchor() {
        let mut o = MetricsOverlay::new();
        o.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = metrics_modal_rects(
            viewport,
            o.width_pct(),
            o.height_pct(),
            o.offset_x(),
            o.offset_y(),
        );
        let title_row = rects.area.y;
        // pick a title-bar column that's not the [x] close button
        let title_col = rects.area.x + rects.area.width / 2;

        let event = mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            title_col,
            title_row,
        );
        handle_metrics_overlay_mouse(&mut o, viewport, 0, event);

        let anchor = o.drag_anchor().expect("title-row click must set anchor");
        assert_eq!(anchor.start_col, title_col);
        assert_eq!(anchor.start_row, title_row);
        assert_eq!(anchor.start_offset_x, 0);
        assert_eq!(anchor.start_offset_y, 0);
    }

    #[test]
    fn drag_event_updates_offset_relative_to_anchor() {
        let mut o = MetricsOverlay::new();
        o.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = metrics_modal_rects(
            viewport,
            o.width_pct(),
            o.height_pct(),
            o.offset_x(),
            o.offset_y(),
        );
        let title_row = rects.area.y;
        let title_col = rects.area.x + 5;

        handle_metrics_overlay_mouse(
            &mut o,
            viewport,
            0,
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                title_col,
                title_row,
            ),
        );
        handle_metrics_overlay_mouse(
            &mut o,
            viewport,
            0,
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                title_col + 7,
                title_row + 3,
            ),
        );

        assert_eq!(o.offset_x(), 7);
        assert_eq!(o.offset_y(), 3);
        assert!(o.drag_anchor().is_some(), "anchor stays during active drag");
    }

    #[test]
    fn up_event_clears_anchor() {
        let mut o = MetricsOverlay::new();
        o.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = metrics_modal_rects(
            viewport,
            o.width_pct(),
            o.height_pct(),
            o.offset_x(),
            o.offset_y(),
        );
        let title_row = rects.area.y;
        let title_col = rects.area.x + 5;

        handle_metrics_overlay_mouse(
            &mut o,
            viewport,
            0,
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                title_col,
                title_row,
            ),
        );
        handle_metrics_overlay_mouse(
            &mut o,
            viewport,
            0,
            mouse_event(
                MouseEventKind::Up(MouseButton::Left),
                title_col + 2,
                title_row,
            ),
        );
        assert!(o.drag_anchor().is_none());
    }

    #[test]
    fn drag_does_not_start_in_body() {
        let mut o = MetricsOverlay::new();
        o.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = metrics_modal_rects(
            viewport,
            o.width_pct(),
            o.height_pct(),
            o.offset_x(),
            o.offset_y(),
        );
        let body_col = rects.area.x + rects.area.width / 2;
        let body_row = rects.area.y + rects.area.height / 2;
        handle_metrics_overlay_mouse(
            &mut o,
            viewport,
            0,
            mouse_event(MouseEventKind::Down(MouseButton::Left), body_col, body_row),
        );
        assert!(o.drag_anchor().is_none());
    }

    #[test]
    fn drag_does_not_start_on_close_button_and_close_wins() {
        let mut o = MetricsOverlay::new();
        o.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = metrics_modal_rects(
            viewport,
            o.width_pct(),
            o.height_pct(),
            o.offset_x(),
            o.offset_y(),
        );
        let close = crate::ui::dashboard::close_button_rect(rects.area);
        handle_metrics_overlay_mouse(
            &mut o,
            viewport,
            0,
            mouse_event(MouseEventKind::Down(MouseButton::Left), close.x, close.y),
        );
        assert!(!o.is_open(), "close-button click closes the overlay");
        assert!(o.drag_anchor().is_none());
    }
}
