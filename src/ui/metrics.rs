//! Phase C (v1.38) — Metrics Overlay state container.
//!
//! Pure-state struct with open/close/select APIs and clamping. No
//! rendering yet (Tasks 10-13 add layout, hottest banner, comparison
//! table, selected-pane detail, and Frame-based renderer). Tests
//! cover the state transitions in isolation so subsequent renderer
//! tasks can rely on a stable state contract.

use crate::ui::dashboard::centered_rect;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[derive(Debug, Default, Clone)]
pub struct MetricsOverlay {
    open: bool,
    selected: usize,
    scroll: u16,
}

impl MetricsOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self) {
        self.open = true;
        self.scroll = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.scroll = 0;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    pub fn select_at(&mut self, idx: usize, total: usize) {
        if total == 0 {
            self.selected = 0;
            return;
        }
        self.selected = idx.min(total - 1);
    }

    pub fn select_next(&mut self, total: usize) {
        if total == 0 {
            return;
        }
        self.selected = self.selected.saturating_add(1).min(total - 1);
    }

    pub fn select_prev(&mut self, total: usize) {
        if total == 0 {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self, max: u16) {
        self.scroll = self.scroll.saturating_add(1).min(max);
    }
}

pub struct MetricsModalRects {
    pub area: Rect,
    pub banner: Rect,
    pub comparison: Rect,
    pub detail: Rect,
    pub hint: Rect,
}

pub fn metrics_modal_rects(viewport: Rect) -> MetricsModalRects {
    let area = centered_rect(88, 80, viewport);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // banner
            Constraint::Min(6),    // comparison
            Constraint::Length(8), // detail
            Constraint::Length(1), // hint
        ])
        .split(area);
    MetricsModalRects {
        area,
        banner: chunks[0],
        comparison: chunks[1],
        detail: chunks[2],
        hint: chunks[3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close_round_trip() {
        let mut o = MetricsOverlay::new();
        assert!(!o.is_open());
        o.open();
        assert!(o.is_open());
        o.close();
        assert!(!o.is_open());
    }

    #[test]
    fn select_clamps_within_bounds() {
        let mut o = MetricsOverlay::new();
        o.select_at(99, 3);
        assert_eq!(o.selected(), 2);
        o.select_prev(3);
        assert_eq!(o.selected(), 1);
        o.select_next(3);
        assert_eq!(o.selected(), 2);
        o.select_next(3); // saturates at top
        assert_eq!(o.selected(), 2);
    }

    #[test]
    fn select_with_zero_panes_stays_at_zero() {
        let mut o = MetricsOverlay::new();
        o.select_next(0);
        o.select_prev(0);
        o.select_at(99, 0);
        assert_eq!(o.selected(), 0);
    }

    #[test]
    fn metrics_modal_rects_centered_88_by_80() {
        use ratatui::layout::Rect;
        let viewport = Rect::new(0, 0, 100, 50);
        let r = metrics_modal_rects(viewport);
        assert_eq!(r.area.width, 88);
        assert_eq!(r.area.height, 40);
        assert_eq!(r.area.x, 6);
        assert_eq!(r.area.y, 5);
    }

    #[test]
    fn metrics_modal_rects_partition_sums_to_body() {
        use ratatui::layout::Rect;
        let viewport = Rect::new(0, 0, 120, 40);
        let r = metrics_modal_rects(viewport);
        let bottom = r.banner.height + r.comparison.height + r.detail.height + r.hint.height;
        assert_eq!(bottom, r.area.height);
        assert_eq!(r.banner.y, r.area.y);
        assert_eq!(r.hint.y + r.hint.height, r.area.y + r.area.height);
    }
}
