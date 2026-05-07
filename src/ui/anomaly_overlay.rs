//! Phase 7 v3 (v1.46.0): `n` overlay — recent anomaly events
//! visualization. Renders `AnomalyEventsRing` chronologically
//! (newest first) with `pane_id` column. Read-only; no actions.

use crate::ui::dashboard::close_button_rect;
use crate::ui::labels::ellipsize;
use crate::ui::modal_chrome::{DragAnchor, ModalGeometry};
use crate::ui::theme;
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
    geometry: ModalGeometry,
    view: AnomalyOverlayView,
    history_cache: Vec<crate::domain::anomaly::AnomalyEvent>,
}

impl Default for AnomalyOverlay {
    fn default() -> Self {
        Self {
            open: false,
            scroll: 0,
            geometry: ModalGeometry::new(
                Self::DEFAULT_WIDTH_PCT,
                Self::DEFAULT_HEIGHT_PCT,
                Self::SIZE_MIN,
                Self::SIZE_MAX,
                Self::SIZE_STEP,
            ),
            view: AnomalyOverlayView::Ring,
            history_cache: Vec::new(),
        }
    }
}

impl AnomalyOverlay {
    pub const SIZE_STEP: u16 = 5;
    pub const SIZE_MIN: u16 = 50;
    pub const SIZE_MAX: u16 = 99;
    pub const DEFAULT_WIDTH_PCT: u16 = 80;
    pub const DEFAULT_HEIGHT_PCT: u16 = 70;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.scroll = 0;
        self.geometry.end_drag();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.scroll = 0;
        self.geometry.end_drag();
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

    pub fn scroll_bound(&self, ring_len: usize) -> u16 {
        match self.view {
            AnomalyOverlayView::Ring => ring_len.saturating_sub(1) as u16,
            AnomalyOverlayView::History => self.history_cache.len().saturating_sub(1) as u16,
        }
    }

    pub fn width_pct(&self) -> u16 {
        self.geometry.width_pct()
    }

    pub fn height_pct(&self) -> u16 {
        self.geometry.height_pct()
    }

    pub fn offset_x(&self) -> i16 {
        self.geometry.offset_x()
    }

    pub fn offset_y(&self) -> i16 {
        self.geometry.offset_y()
    }

    pub fn drag_anchor(&self) -> Option<DragAnchor> {
        self.geometry.drag_anchor()
    }

    pub fn shrink(&mut self) {
        self.geometry.shrink();
    }

    pub fn grow(&mut self) {
        self.geometry.grow();
    }

    pub fn reset_geometry(&mut self) {
        self.geometry.reset();
    }

    pub fn begin_drag(&mut self, anchor: DragAnchor) {
        self.geometry.begin_drag(anchor);
    }

    pub fn set_offset(&mut self, x: i16, y: i16) {
        self.geometry.set_offset(x, y);
    }

    pub fn end_drag(&mut self) {
        self.geometry.end_drag();
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
        self.geometry.end_drag();
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
        let popup_area = anomaly_modal_area_for(area, self);
        frame.render_widget(Clear, popup_area);

        let (title, total_count) = match self.view {
            AnomalyOverlayView::Ring => (
                format!(
                    " ANOMALY EVENTS (last {} \u{2014} newest first) [h: history] [n to close] ",
                    ring.len()
                ),
                ring.len(),
            ),
            AnomalyOverlayView::History => (
                format!(
                    " ANOMALY EVENTS [history view, last {}] [h: ring] [n to close] ",
                    self.history_cache.len()
                ),
                self.history_cache.len(),
            ),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(theme::BORDER_ACTIVE));
        let close = Paragraph::new("[x]").style(theme::modal_close_style());

        if total_count == 0 {
            let body_lines = match self.view {
                AnomalyOverlayView::Ring => vec![
                    Line::from("No anomaly events recorded this session."),
                    Line::from(""),
                    Line::from(
                        "Anomalies are recorded once detection is enabled in Settings: [anomaly] enabled = true.",
                    ),
                ],
                AnomalyOverlayView::History => vec![
                    Line::from("No persisted anomaly events."),
                    Line::from(""),
                    Line::from(
                        "Enable [anomaly] in Settings to start recording. Old rows are pruned by [anomaly] retention_days (default 30).",
                    ),
                ],
            };
            let body = Paragraph::new(body_lines)
                .block(block)
                .wrap(Wrap { trim: false });
            frame.render_widget(body, popup_area);
            frame.render_widget(close, close_button_rect(popup_area));
            return;
        }

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);
        frame.render_widget(close, close_button_rect(popup_area));

        let header = format!(
            "{:<8}  {:<6}  {:<22}  {:<6}  {:<8}  {}",
            "Time", "Pane", "Kind", "Conf", "Promoted", "Reason"
        );

        let visible_rows = inner.height.saturating_sub(1) as usize;
        let items: Vec<ListItem> = match self.view {
            AnomalyOverlayView::Ring => ring
                .iter()
                .rev()
                .skip(self.scroll as usize)
                .take(visible_rows)
                .map(|e| ListItem::new(format_event_row(e, inner.width as usize)))
                .collect(),
            AnomalyOverlayView::History => self
                .history_cache
                .iter()
                .skip(self.scroll as usize)
                .take(visible_rows)
                .map(|e| ListItem::new(format_event_row(e, inner.width as usize)))
                .collect(),
        };

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

pub fn anomaly_modal_area(viewport: Rect) -> Rect {
    crate::ui::modal_chrome::modal_area(
        viewport,
        AnomalyOverlay::DEFAULT_WIDTH_PCT,
        AnomalyOverlay::DEFAULT_HEIGHT_PCT,
        0,
        0,
    )
}

pub fn anomaly_modal_area_for(viewport: Rect, overlay: &AnomalyOverlay) -> Rect {
    crate::ui::modal_chrome::modal_area(
        viewport,
        overlay.width_pct(),
        overlay.height_pct(),
        overlay.offset_x(),
        overlay.offset_y(),
    )
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
    fn render_uses_active_border_style() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let viewport = Rect::new(0, 0, 100, 40);
        let mut terminal =
            Terminal::new(TestBackend::new(viewport.width, viewport.height)).unwrap();
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let area = anomaly_modal_area_for(viewport, &overlay);
        let ring = AnomalyEventsRing::new();

        terminal
            .draw(|frame| overlay.render(frame, frame.area(), &ring))
            .unwrap();
        let cell = terminal
            .backend()
            .buffer()
            .cell((area.x, area.y))
            .expect("top-left border cell in bounds");

        assert_eq!(cell.fg, theme::BORDER_ACTIVE);
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
    fn toggle_view_clears_drag_anchor() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;

        let mut o = AnomalyOverlay::new();
        o.open();
        o.begin_drag(DragAnchor {
            start_col: 10,
            start_row: 5,
            start_offset_x: 2,
            start_offset_y: 3,
        });
        assert!(o.drag_anchor().is_some());

        o.toggle_view(vec![AnomalyEvent {
            timestamp: 1,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "x".to_string(),
        }]);

        assert!(o.drag_anchor().is_none());
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

    #[test]
    fn render_history_view_shows_history_title_indicator() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        overlay.toggle_view(vec![AnomalyEvent {
            timestamp: 1_700_000_000,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "from disk".to_string(),
        }]);
        let ring = AnomalyEventsRing::new();
        terminal
            .draw(|f| overlay.render(f, f.area(), &ring))
            .unwrap();
        let rendered = buf_to_string(terminal.backend().buffer());
        assert!(
            rendered.contains("history view"),
            "title missing 'history view': {rendered}"
        );
        assert!(
            rendered.contains("[h: ring]"),
            "ring-toggle hint missing: {rendered}"
        );
        assert!(
            rendered.contains("from disk"),
            "history row content missing"
        );
    }

    #[test]
    fn render_history_view_empty_shows_help_hint() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        overlay.toggle_view(Vec::new());
        let ring = AnomalyEventsRing::new();
        terminal
            .draw(|f| overlay.render(f, f.area(), &ring))
            .unwrap();
        let rendered = buf_to_string(terminal.backend().buffer());
        assert!(
            rendered.contains("No persisted anomaly events"),
            "history empty body missing"
        );
        assert!(
            rendered.contains("retention_days"),
            "history empty hint missing retention reference"
        );
    }

    #[test]
    fn render_ring_view_still_uses_h_history_hint() {
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
            reason: "ring view".to_string(),
        });
        terminal
            .draw(|f| overlay.render(f, f.area(), &ring))
            .unwrap();
        let rendered = buf_to_string(terminal.backend().buffer());
        assert!(
            rendered.contains("[h: history]"),
            "ring view title should advertise history toggle"
        );
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
