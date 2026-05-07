//! Phase 7 v3 (v1.46.0): `n` overlay — recent anomaly events
//! visualization. Renders `AnomalyEventsRing` chronologically
//! (newest first) with `pane_id` column. Read-only; no actions.

use crate::ui::dashboard::centered_rect;
use crate::ui::labels::ellipsize;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyOverlayView {
    #[default]
    Ring,
    History,
}

#[derive(Debug, Clone)]
pub struct AnomalyOverlay {
    open: bool,
    scroll: u16,
    view: AnomalyOverlayView,
    history_cache: Vec<crate::domain::anomaly::AnomalyEvent>,
}

impl Default for AnomalyOverlay {
    fn default() -> Self {
        Self {
            open: false,
            scroll: 0,
            view: AnomalyOverlayView::Ring,
            history_cache: Vec::new(),
        }
    }
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
        self.view = AnomalyOverlayView::Ring;
        self.history_cache.clear();
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

    pub fn view(&self) -> AnomalyOverlayView {
        self.view
    }

    /// Toggle between Ring and History views. When called with a
    /// non-empty `history_cache`, switch to History; otherwise switch
    /// to Ring (and clear the cache). The dispatch layer
    /// (`tui_loop.rs`) is responsible for fetching the cache from
    /// the persistence sink and passing it in.
    pub fn toggle_view(&mut self, history_cache: Vec<crate::domain::anomaly::AnomalyEvent>) {
        match self.view {
            AnomalyOverlayView::Ring => {
                self.view = AnomalyOverlayView::History;
                self.history_cache = history_cache;
                self.scroll = 0;
            }
            AnomalyOverlayView::History => {
                self.view = AnomalyOverlayView::Ring;
                self.history_cache.clear();
                self.scroll = 0;
            }
        }
    }

    pub fn history_cache(&self) -> &[crate::domain::anomaly::AnomalyEvent] {
        &self.history_cache
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        ring: &crate::app::anomaly_events_ring::AnomalyEventsRing,
    ) {
        let popup_area = centered_rect(80, 70, area);
        frame.render_widget(Clear, popup_area);

        let title = format!(
            " ANOMALY EVENTS (last {} \u{2014} newest first) [n to close] ",
            ring.len()
        );
        let block = Block::default().borders(Borders::ALL).title(title);

        if ring.is_empty() {
            let body = Paragraph::new(vec![
                Line::from("No anomaly events recorded this session."),
                Line::from(""),
                Line::from(
                    "Anomalies are recorded once detection is enabled in Settings: [anomaly] enabled = true.",
                ),
            ])
            .block(block)
            .wrap(Wrap { trim: false });
            frame.render_widget(body, popup_area);
            return;
        }

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let header = format!(
            "{:<8}  {:<6}  {:<22}  {:<6}  {:<8}  {}",
            "Time", "Pane", "Kind", "Conf", "Promoted", "Reason"
        );

        let visible_rows = inner.height.saturating_sub(1) as usize;
        let items: Vec<ListItem> = ring
            .iter()
            .rev()
            .skip(self.scroll() as usize)
            .take(visible_rows)
            .map(|e| ListItem::new(format_event_row(e, inner.width as usize)))
            .collect();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        frame.render_widget(
            Paragraph::new(header).style(Style::default().add_modifier(Modifier::BOLD)),
            layout[0],
        );
        frame.render_widget(List::new(items), layout[1]);
    }
}

fn format_event_row(e: &crate::domain::anomaly::AnomalyEvent, width: usize) -> String {
    let time = format_hhmmss(e.timestamp);
    let pane = ellipsize(&e.pane_id, 6);
    let kind = e.kind.label();
    let conf = e.confidence.label();
    let promoted = if e.promoted { "yes" } else { "no" };
    let row_prefix = format!(
        "{:<8}  {:<6}  {:<22}  {:<6}  {:<8}  ",
        time, pane, kind, conf, promoted
    );
    let prefix_len = row_prefix.chars().count();
    if prefix_len >= width {
        // Row prefix alone already fills or exceeds available width;
        // truncate the whole row to fit.
        return ellipsize(&row_prefix, width);
    }
    let reason_budget = width - prefix_len;
    let reason = ellipsize(&e.reason, reason_budget);
    format!("{row_prefix}{reason}")
}

fn format_hhmmss(ts: u64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "??:??:??".to_string())
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

    #[test]
    fn render_empty_state_includes_help_hint() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let ring = AnomalyEventsRing::new();
        terminal
            .draw(|f| overlay.render(f, f.area(), &ring))
            .unwrap();
        let buf = terminal.backend().buffer();
        let rendered = buf_to_string(buf);
        assert!(
            rendered.contains("No anomaly events recorded this session."),
            "empty state body missing: {rendered}"
        );
        assert!(
            rendered.contains("[anomaly] enabled = true"),
            "help hint missing: {rendered}"
        );
    }

    #[test]
    fn render_populated_shows_columns_and_at_least_one_row() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let mut ring = AnomalyEventsRing::new();
        ring.push(AnomalyEvent {
            timestamp: 1_700_000_000,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "cost_usd: 0.10 \u{2192} 25.40".to_string(),
        });
        terminal
            .draw(|f| overlay.render(f, f.area(), &ring))
            .unwrap();
        let rendered = buf_to_string(terminal.backend().buffer());
        assert!(
            rendered.contains("Time"),
            "header 'Time' missing: {rendered}"
        );
        assert!(rendered.contains("Kind"), "header 'Kind' missing");
        assert!(rendered.contains("Promoted"), "header 'Promoted' missing");
        assert!(rendered.contains("CostSlope"), "kind value missing");
        assert!(rendered.contains("yes"), "promoted=yes missing");
    }

    #[test]
    fn render_truncates_long_reason() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(60, 12); // narrow on purpose
        let mut terminal = Terminal::new(backend).unwrap();
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let mut ring = AnomalyEventsRing::new();
        let long_reason = "A".repeat(200);
        ring.push(AnomalyEvent {
            timestamp: 1_700_000_000,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: long_reason.clone(),
        });
        terminal
            .draw(|f| overlay.render(f, f.area(), &ring))
            .unwrap();
        let rendered = buf_to_string(terminal.backend().buffer());
        // No row should literally contain the full 200-char reason
        assert!(
            !rendered.contains(&long_reason),
            "reason was not truncated to fit"
        );
        // But a truncation marker should appear somewhere on a CostSlope row
        assert!(rendered.contains('\u{2026}'), "truncation ellipsis missing");
    }

    #[test]
    fn overlay_starts_in_ring_view() {
        let o = AnomalyOverlay::new();
        assert_eq!(o.view(), AnomalyOverlayView::Ring);
        assert!(o.history_cache().is_empty());
    }

    #[test]
    fn toggle_view_to_history_with_cache() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        let mut o = AnomalyOverlay::new();
        o.open();
        o.scroll_down(5);
        let event = AnomalyEvent {
            timestamp: 1,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "x".to_string(),
        };
        o.toggle_view(vec![event.clone()]);
        assert_eq!(o.view(), AnomalyOverlayView::History);
        assert_eq!(o.history_cache().len(), 1);
        assert_eq!(o.scroll(), 0); // toggle resets scroll
    }

    #[test]
    fn toggle_view_back_to_ring_clears_cache() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        let mut o = AnomalyOverlay::new();
        o.open();
        let event = AnomalyEvent {
            timestamp: 1,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "x".to_string(),
        };
        o.toggle_view(vec![event]);
        assert_eq!(o.view(), AnomalyOverlayView::History);
        o.toggle_view(Vec::new());
        assert_eq!(o.view(), AnomalyOverlayView::Ring);
        assert!(o.history_cache().is_empty());
    }

    #[test]
    fn close_resets_view_and_clears_cache() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        let mut o = AnomalyOverlay::new();
        o.open();
        o.toggle_view(vec![AnomalyEvent {
            timestamp: 1,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "x".to_string(),
        }]);
        o.close();
        assert!(!o.is_open());
        assert_eq!(o.view(), AnomalyOverlayView::Ring);
        assert!(o.history_cache().is_empty());
    }

    fn buf_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }
}
