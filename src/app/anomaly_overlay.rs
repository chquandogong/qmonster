//! Phase 7 v3 (v1.46.0): keyboard handler for the `n` (anomaly
//! events) overlay. Mirrors the `m` overlay handler's contract
//! (returns `true` when the key is consumed).

use crate::app::keymap::rect_contains;
use crate::ui::anomaly_overlay::AnomalyOverlay;
use crate::ui::anomaly_overlay::anomaly_modal_area_for;
use crate::ui::dashboard::close_button_rect;
use crate::ui::modal_chrome::{DragAnchor, modal_body_rect, title_row_contains};
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

pub fn handle_anomaly_overlay_key(
    overlay: &mut AnomalyOverlay,
    ring_len: usize,
    key: KeyCode,
) -> bool {
    match key {
        KeyCode::Char('n') => {
            overlay.toggle();
            true
        }
        KeyCode::Char('[') if overlay.is_open() => {
            overlay.shrink();
            true
        }
        KeyCode::Char(']') if overlay.is_open() => {
            overlay.grow();
            true
        }
        KeyCode::Char('=') if overlay.is_open() => {
            overlay.reset_geometry();
            true
        }
        KeyCode::Esc | KeyCode::Char('q') if overlay.is_open() => {
            overlay.close();
            true
        }
        KeyCode::Down | KeyCode::Char('j') if overlay.is_open() => {
            overlay.scroll_down(overlay.scroll_bound(ring_len));
            true
        }
        KeyCode::Up | KeyCode::Char('k') if overlay.is_open() => {
            overlay.scroll_up();
            true
        }
        KeyCode::Char('f') if overlay.is_open() => {
            // v1.58.0: cycle the row filter (All → Promoted → High → All).
            overlay.cycle_filter();
            true
        }
        _ => false,
    }
}

pub fn handle_anomaly_overlay_mouse(
    overlay: &mut AnomalyOverlay,
    viewport: Rect,
    ring_len: usize,
    event: MouseEvent,
) -> bool {
    if !overlay.is_open() {
        return false;
    }
    let area = anomaly_modal_area_for(viewport, overlay);
    let close = close_button_rect(area);
    let body = modal_body_rect(area);
    match event.kind {
        MouseEventKind::ScrollDown => {
            overlay.end_drag();
            if rect_contains(body, event.column, event.row) {
                overlay.scroll_down(overlay.scroll_bound(ring_len));
            }
            true
        }
        MouseEventKind::ScrollUp => {
            overlay.end_drag();
            if rect_contains(body, event.column, event.row) {
                overlay.scroll_up();
            }
            true
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if rect_contains(close, event.column, event.row) {
                overlay.close();
            } else if title_row_contains(area, event.column, event.row) {
                overlay.begin_drag(DragAnchor {
                    start_col: event.column,
                    start_row: event.row,
                    start_offset_x: overlay.offset_x(),
                    start_offset_y: overlay.offset_y(),
                });
            } else {
                overlay.end_drag();
            }
            true
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
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            overlay.end_drag();
            true
        }
        _ => {
            overlay.end_drag();
            true
        }
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

    fn drag_overlay_by(
        overlay: &mut AnomalyOverlay,
        viewport: Rect,
        delta_x: u16,
        delta_y: u16,
    ) -> Rect {
        let area = crate::ui::anomaly_overlay::anomaly_modal_area_for(viewport, overlay);
        handle_anomaly_overlay_mouse(
            overlay,
            viewport,
            5,
            mouse(MouseEventKind::Down(MouseButton::Left), area.x + 2, area.y),
        );
        handle_anomaly_overlay_mouse(
            overlay,
            viewport,
            5,
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                area.x + 2 + delta_x,
                area.y + delta_y,
            ),
        );
        crate::ui::anomaly_overlay::anomaly_modal_area_for(viewport, overlay)
    }

    #[test]
    fn n_toggles_open_close() {
        let mut o = AnomalyOverlay::new();
        assert!(handle_anomaly_overlay_key(&mut o, 0, KeyCode::Char('n')));
        assert!(o.is_open());
        assert!(handle_anomaly_overlay_key(&mut o, 0, KeyCode::Char('n')));
        assert!(!o.is_open());
    }

    #[test]
    fn esc_closes_when_open_no_op_when_closed() {
        let mut o = AnomalyOverlay::new();
        assert!(!handle_anomaly_overlay_key(&mut o, 0, KeyCode::Esc));
        assert!(!o.is_open());
        o.open();
        assert!(handle_anomaly_overlay_key(&mut o, 0, KeyCode::Esc));
        assert!(!o.is_open());
    }

    #[test]
    fn q_closes_when_open() {
        let mut o = AnomalyOverlay::new();
        o.open();
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('q')));
        assert!(!o.is_open());
    }

    #[test]
    fn bracket_keys_resize_anomaly_overlay() {
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let (w0, h0) = (overlay.width_pct(), overlay.height_pct());
        assert!(handle_anomaly_overlay_key(
            &mut overlay,
            0,
            KeyCode::Char(']')
        ));
        assert!(overlay.width_pct() > w0);
        assert!(overlay.height_pct() > h0);
        assert!(handle_anomaly_overlay_key(
            &mut overlay,
            0,
            KeyCode::Char('[')
        ));
        assert!(handle_anomaly_overlay_key(
            &mut overlay,
            0,
            KeyCode::Char('=')
        ));
        assert_eq!(overlay.width_pct(), AnomalyOverlay::DEFAULT_WIDTH_PCT);
        assert_eq!(overlay.height_pct(), AnomalyOverlay::DEFAULT_HEIGHT_PCT);
    }

    #[test]
    fn title_drag_moves_anomaly_overlay_and_reset_clears_offset() {
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 100, 40);
        let area = crate::ui::anomaly_overlay::anomaly_modal_area_for(viewport, &overlay);
        let down = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 2,
            row: area.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let drag = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: area.x + 8,
            row: area.y + 4,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            down
        ));
        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            drag
        ));
        assert_eq!(overlay.offset_x(), 6);
        assert_eq!(overlay.offset_y(), 4);
        assert!(handle_anomaly_overlay_key(
            &mut overlay,
            0,
            KeyCode::Char('=')
        ));
        assert_eq!(overlay.offset_x(), 0);
        assert_eq!(overlay.offset_y(), 0);
    }

    #[test]
    fn down_and_j_scroll_within_bounds() {
        let mut o = AnomalyOverlay::new();
        o.open();
        assert!(handle_anomaly_overlay_key(&mut o, 3, KeyCode::Down));
        assert_eq!(o.scroll(), 1);
        assert!(handle_anomaly_overlay_key(&mut o, 3, KeyCode::Char('j')));
        assert_eq!(o.scroll(), 2);
        // ring_len=3 means max scroll index = 2
        assert!(handle_anomaly_overlay_key(&mut o, 3, KeyCode::Down));
        assert_eq!(o.scroll(), 2);
    }

    #[test]
    fn history_view_scroll_uses_history_cache_len_not_ring_len() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;

        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        overlay.toggle_view(vec![
            AnomalyEvent {
                timestamp: 1,
                pane_id: "%1".to_string(),
                kind: AnomalyKind::CostSlope,
                confidence: AnomalyConfidence::High,
                severity: Severity::Warning,
                promoted: true,
                reason: "one".to_string(),
            },
            AnomalyEvent {
                timestamp: 2,
                pane_id: "%2".to_string(),
                kind: AnomalyKind::CostSlope,
                confidence: AnomalyConfidence::High,
                severity: Severity::Warning,
                promoted: true,
                reason: "two".to_string(),
            },
            AnomalyEvent {
                timestamp: 3,
                pane_id: "%3".to_string(),
                kind: AnomalyKind::CostSlope,
                confidence: AnomalyConfidence::High,
                severity: Severity::Warning,
                promoted: true,
                reason: "three".to_string(),
            },
            AnomalyEvent {
                timestamp: 4,
                pane_id: "%4".to_string(),
                kind: AnomalyKind::CostSlope,
                confidence: AnomalyConfidence::High,
                severity: Severity::Warning,
                promoted: true,
                reason: "four".to_string(),
            },
            AnomalyEvent {
                timestamp: 5,
                pane_id: "%5".to_string(),
                kind: AnomalyKind::CostSlope,
                confidence: AnomalyConfidence::High,
                severity: Severity::Warning,
                promoted: true,
                reason: "five".to_string(),
            },
        ]);

        assert!(handle_anomaly_overlay_key(&mut overlay, 1, KeyCode::Down));
        assert_eq!(overlay.scroll(), 1);
        assert!(handle_anomaly_overlay_key(
            &mut overlay,
            1,
            KeyCode::Char('j')
        ));
        assert_eq!(overlay.scroll(), 2);

        let viewport = Rect::new(0, 0, 80, 24);
        let area = crate::ui::anomaly_overlay::anomaly_modal_area_for(viewport, &overlay);
        let body = crate::ui::modal_chrome::modal_body_rect(area);
        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            1,
            mouse(MouseEventKind::ScrollDown, body.x, body.y),
        ));
        assert_eq!(overlay.scroll(), 3);
    }

    #[test]
    fn up_and_k_scroll_saturate_at_zero() {
        let mut o = AnomalyOverlay::new();
        o.open();
        o.scroll_down(5);
        o.scroll_down(5);
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Up));
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('k')));
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('k')));
        assert_eq!(o.scroll(), 0);
    }

    #[test]
    fn keys_ignored_when_closed() {
        let mut o = AnomalyOverlay::new();
        assert!(!handle_anomaly_overlay_key(&mut o, 5, KeyCode::Down));
        assert!(!handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('q')));
    }

    /// v1.58.0: `f` cycles the row filter and resets scroll.
    #[test]
    fn f_cycles_anomaly_row_filter_and_resets_scroll() {
        use crate::ui::anomaly_overlay::AnomalyFilter;
        let mut o = AnomalyOverlay::new();
        o.open();
        o.scroll_down(5);
        assert_eq!(o.filter(), AnomalyFilter::All);
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('f')));
        assert_eq!(o.filter(), AnomalyFilter::PromotedOnly);
        assert_eq!(o.scroll(), 0, "cycling filter resets scroll");
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('f')));
        assert_eq!(o.filter(), AnomalyFilter::HighOnly);
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('f')));
        assert_eq!(o.filter(), AnomalyFilter::All);
    }

    /// v1.58.0: filter when overlay is closed must not be consumed.
    #[test]
    fn f_does_not_cycle_filter_when_overlay_is_closed() {
        let mut o = AnomalyOverlay::new();
        assert!(!handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('f')));
    }

    /// v1.58.0: AnomalyFilter::matches gates rows correctly.
    #[test]
    fn anomaly_filter_matches_per_variant() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        use crate::ui::anomaly_overlay::AnomalyFilter;

        let high_promoted = AnomalyEvent {
            timestamp: 1,
            pane_id: "%1".into(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "h".into(),
        };
        let medium_promoted = AnomalyEvent {
            confidence: AnomalyConfidence::Medium,
            ..high_promoted.clone()
        };
        let medium_unpromoted = AnomalyEvent {
            promoted: false,
            ..medium_promoted.clone()
        };

        assert!(AnomalyFilter::All.matches(&high_promoted));
        assert!(AnomalyFilter::All.matches(&medium_unpromoted));

        assert!(AnomalyFilter::PromotedOnly.matches(&high_promoted));
        assert!(AnomalyFilter::PromotedOnly.matches(&medium_promoted));
        assert!(!AnomalyFilter::PromotedOnly.matches(&medium_unpromoted));

        assert!(AnomalyFilter::HighOnly.matches(&high_promoted));
        assert!(!AnomalyFilter::HighOnly.matches(&medium_promoted));
        assert!(!AnomalyFilter::HighOnly.matches(&medium_unpromoted));
    }

    #[test]
    fn mouse_wheel_down_scrolls() {
        use crossterm::event::{MouseEvent, MouseEventKind};
        use ratatui::layout::Rect;
        let mut o = AnomalyOverlay::new();
        o.open();
        let viewport = Rect::new(0, 0, 80, 24);
        let event = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
        assert_eq!(o.scroll(), 1);
    }

    #[test]
    fn mouse_wheel_up_scrolls() {
        let mut o = AnomalyOverlay::new();
        o.open();
        o.scroll_down(5);
        o.scroll_down(5);
        let viewport = Rect::new(0, 0, 80, 24);
        let event = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 10,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
        assert_eq!(o.scroll(), 1);
    }

    #[test]
    fn mouse_click_outside_viewport_is_swallowed_without_closing() {
        let mut o = AnomalyOverlay::new();
        o.open();
        let viewport = Rect::new(10, 5, 20, 10);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
        assert!(o.is_open());
    }

    #[test]
    fn mouse_click_inside_viewport_is_consumed() {
        let mut o = AnomalyOverlay::new();
        o.open();
        let viewport = Rect::new(10, 5, 20, 10);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 15,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
        assert!(o.is_open());
    }

    #[test]
    fn mouse_click_on_close_button_closes() {
        let mut o = AnomalyOverlay::new();
        o.open();
        let viewport = Rect::new(0, 0, 100, 40);
        let area = crate::ui::anomaly_overlay::anomaly_modal_area(viewport);
        let close = crate::ui::dashboard::close_button_rect(area);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: close.x,
            row: close.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
        assert!(!o.is_open());
    }

    #[test]
    fn mouse_left_on_close_button_closes_overlay_after_move() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let area = drag_overlay_by(&mut overlay, viewport, 5, 3);
        let close = crate::ui::dashboard::close_button_rect(area);

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(MouseEventKind::Down(MouseButton::Left), close.x, close.y),
        ));

        assert!(!overlay.is_open());
    }

    #[test]
    fn mouse_left_inside_moved_body_keeps_overlay_open() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let area = drag_overlay_by(&mut overlay, viewport, 5, 3);

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                area.x.saturating_add(2),
                area.y.saturating_add(2),
            ),
        ));

        assert!(overlay.is_open());
        assert!(overlay.drag_anchor().is_none());
    }

    #[test]
    fn mouse_left_outside_overlay_is_swallowed_without_closing() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut overlay = AnomalyOverlay::new();
        overlay.open();

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
        ));

        assert!(overlay.is_open());
        assert!(overlay.drag_anchor().is_none());
    }

    #[test]
    fn mouse_scroll_inside_body_clears_drag_anchor_before_scrolling() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let area = crate::ui::anomaly_overlay::anomaly_modal_area_for(viewport, &overlay);
        let body = crate::ui::modal_chrome::modal_body_rect(area);

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(MouseEventKind::Down(MouseButton::Left), area.x + 2, area.y),
        ));
        assert!(overlay.drag_anchor().is_some());

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(MouseEventKind::ScrollDown, body.x, body.y),
        ));
        assert!(overlay.drag_anchor().is_none());
        assert_eq!(overlay.scroll(), 1);

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                area.x + 10,
                area.y + 4
            ),
        ));
        assert_eq!(overlay.offset_x(), 0);
        assert_eq!(overlay.offset_y(), 0);
    }

    #[test]
    fn mouse_scroll_outside_body_clears_drag_anchor_without_scrolling() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let area = crate::ui::anomaly_overlay::anomaly_modal_area_for(viewport, &overlay);

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(MouseEventKind::Down(MouseButton::Left), area.x + 2, area.y),
        ));
        assert!(overlay.drag_anchor().is_some());

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(MouseEventKind::ScrollDown, 0, 0),
        ));

        assert!(overlay.drag_anchor().is_none());
        assert_eq!(overlay.scroll(), 0);
        assert!(overlay.is_open());
    }

    #[test]
    fn mouse_moved_clears_drag_anchor_before_later_drag() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let area = crate::ui::anomaly_overlay::anomaly_modal_area_for(viewport, &overlay);

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(MouseEventKind::Down(MouseButton::Left), area.x + 2, area.y),
        ));
        assert!(overlay.drag_anchor().is_some());

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(MouseEventKind::Moved, area.x + 4, area.y + 1),
        ));
        assert!(overlay.drag_anchor().is_none());

        assert!(handle_anomaly_overlay_mouse(
            &mut overlay,
            viewport,
            5,
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                area.x + 10,
                area.y + 4
            ),
        ));
        assert_eq!(overlay.offset_x(), 0);
        assert_eq!(overlay.offset_y(), 0);
    }

    #[test]
    fn close_reopen_preserves_geometry_but_clears_drag() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let default_width = overlay.width_pct();
        let default_height = overlay.height_pct();
        assert!(handle_anomaly_overlay_key(
            &mut overlay,
            0,
            KeyCode::Char(']')
        ));
        let resized_width = overlay.width_pct();
        let resized_height = overlay.height_pct();
        assert!(resized_width > default_width);
        assert!(resized_height > default_height);
        drag_overlay_by(&mut overlay, viewport, 5, 3);
        assert_eq!(overlay.offset_x(), 5);
        assert_eq!(overlay.offset_y(), 3);
        assert!(overlay.drag_anchor().is_some());

        overlay.scroll_down(10);
        overlay.toggle_view(Vec::new());
        overlay.close();
        overlay.open();

        assert_eq!(overlay.offset_x(), 5);
        assert_eq!(overlay.offset_y(), 3);
        assert_eq!(overlay.width_pct(), resized_width);
        assert_eq!(overlay.height_pct(), resized_height);
        assert!(overlay.drag_anchor().is_none());
        assert_eq!(overlay.scroll(), 0);
        assert_eq!(
            overlay.view(),
            crate::ui::anomaly_overlay::AnomalyOverlayView::Ring
        );
        assert!(overlay.history_cache().is_empty());
    }

    #[test]
    fn mouse_no_op_when_closed() {
        let mut o = AnomalyOverlay::new();
        let viewport = Rect::new(0, 0, 80, 24);
        let event = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(!handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
    }
}
