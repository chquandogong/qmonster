//! Phase 7 v3 (v1.46.0): `n` overlay — recent anomaly events
//! visualization. Renders `AnomalyEventsRing` chronologically
//! (newest first) with `pane_id` column. Read-only; no actions.

use ratatui::Frame;
use ratatui::layout::Rect;

#[derive(Debug, Default, Clone)]
pub struct AnomalyOverlay {
    open: bool,
    scroll: u16,
}

impl AnomalyOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.scroll = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.scroll = 0;
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    pub fn scroll_down(&mut self, max: u16) {
        if self.scroll < max {
            self.scroll += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Render is added in Task 9; for Task 7 we leave a stub that
    /// the wiring task can call without panicking.
    pub fn render(
        &self,
        _frame: &mut Frame,
        _area: Rect,
        _ring: &crate::app::anomaly_events_ring::AnomalyEventsRing,
    ) {
        // implemented in Task 9
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_starts_closed() {
        let o = AnomalyOverlay::new();
        assert!(!o.is_open());
        assert_eq!(o.scroll(), 0);
    }

    #[test]
    fn open_resets_scroll_to_zero() {
        let mut o = AnomalyOverlay::new();
        o.open();
        assert!(o.is_open());
        o.scroll_down(50);
        assert!(o.scroll() > 0);
        o.close();
        o.open();
        assert_eq!(o.scroll(), 0);
    }

    #[test]
    fn toggle_flips_state() {
        let mut o = AnomalyOverlay::new();
        o.toggle();
        assert!(o.is_open());
        o.toggle();
        assert!(!o.is_open());
    }

    #[test]
    fn scroll_down_clamps_at_max() {
        let mut o = AnomalyOverlay::new();
        o.open();
        for _ in 0..10 {
            o.scroll_down(3);
        }
        assert_eq!(o.scroll(), 3);
    }

    #[test]
    fn scroll_up_saturates_at_zero() {
        let mut o = AnomalyOverlay::new();
        o.open();
        o.scroll_up();
        assert_eq!(o.scroll(), 0);
    }
}
