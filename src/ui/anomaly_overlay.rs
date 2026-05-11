//! Phase 7 v3 (v1.46.0): `n` overlay — recent anomaly events
//! visualization. Renders `AnomalyEventsRing` chronologically
//! (newest first) with `pane_id` column. Read-only; no actions.

use crate::ui::dashboard::close_button_rect;
use crate::ui::labels::ellipsize;
use crate::ui::modal_chrome::{DragAnchor, ModalGeometry};
use crate::ui::scroll_hint;
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
    filter: AnomalyFilter,
    evidence_expanded: bool,
}

/// v1.58.0: cycling filter for the n-overlay rows. `All` is the
/// no-filter starting point; `PromotedOnly` hides Concern/Medium-
/// confidence rows that didn't promote to a Recommendation; `HighOnly`
/// shows only High-confidence rows. Cycle order: All → Promoted →
/// High → All. Each cycle resets `scroll` to 0 because the displayed
/// row count has changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyFilter {
    All,
    PromotedOnly,
    HighOnly,
}

impl AnomalyFilter {
    pub fn cycle(self) -> Self {
        match self {
            Self::All => Self::PromotedOnly,
            Self::PromotedOnly => Self::HighOnly,
            Self::HighOnly => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::PromotedOnly => "promoted",
            Self::HighOnly => "high-conf",
        }
    }

    pub fn matches(self, e: &crate::domain::anomaly::AnomalyEvent) -> bool {
        match self {
            Self::All => true,
            Self::PromotedOnly => e.promoted,
            Self::HighOnly => {
                matches!(
                    e.confidence,
                    crate::domain::anomaly::AnomalyConfidence::High
                )
            }
        }
    }
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
            filter: AnomalyFilter::All,
            evidence_expanded: false,
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
        self.evidence_expanded = false;
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

    pub fn filter(&self) -> AnomalyFilter {
        self.filter
    }

    /// v1.58.0: cycle the row filter (All → Promoted → High → All).
    /// Resets scroll because the visible row count just changed.
    pub fn cycle_filter(&mut self) {
        self.filter = self.filter.cycle();
        self.scroll = 0;
    }

    pub fn evidence_expanded(&self) -> bool {
        self.evidence_expanded
    }

    pub fn toggle_evidence(&mut self) {
        if self.open {
            self.evidence_expanded = !self.evidence_expanded;
        }
    }

    /// v1.58.0: count rows surviving the current filter, used by the
    /// scroll-bound calculator.
    fn filtered_count(&self, ring: &crate::app::anomaly_events_ring::AnomalyEventsRing) -> usize {
        match self.view {
            AnomalyOverlayView::Ring => ring.iter().filter(|e| self.filter.matches(e)).count(),
            AnomalyOverlayView::History => self
                .history_cache
                .iter()
                .filter(|e| self.filter.matches(e))
                .count(),
        }
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        ring: &crate::app::anomaly_events_ring::AnomalyEventsRing,
    ) {
        let popup_area = anomaly_modal_area_for(area, self);
        frame.render_widget(Clear, popup_area);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(popup_area);
        let body_area = layout[0];
        let hint_area = layout[1];

        let filter_label = self.filter.label();
        let filtered_count = self.filtered_count(ring);
        let (title_base, total_count) = match self.view {
            AnomalyOverlayView::Ring => (
                format!(
                    " ANOMALY EVENTS (last {} \u{2014} newest first, filter: {}) [h: history] [f: cycle filter] [n to close] ",
                    ring.len(),
                    filter_label,
                ),
                filtered_count,
            ),
            AnomalyOverlayView::History => (
                format!(
                    " ANOMALY EVENTS [history view, last {}, filter: {}] [h: ring] [f: cycle filter] [n to close] ",
                    self.history_cache.len(),
                    filter_label,
                ),
                filtered_count,
            ),
        };
        let visible_rows_for_scroll = body_area.height.saturating_sub(2).saturating_sub(1).max(1);
        let max_scroll = anomaly_visible_scroll_bound(total_count, visible_rows_for_scroll);
        let scroll = self.scroll.min(max_scroll);
        let title = format!("{} ", title_base.trim_end());
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(theme::border_active()));
        let close = Paragraph::new("[x]").style(theme::modal_close_style());
        let view_toggle = match self.view {
            AnomalyOverlayView::Ring => "h history",
            AnomalyOverlayView::History => "h ring",
        };
        let evidence_toggle = if self.evidence_expanded {
            "e hide evidence"
        } else {
            "e evidence"
        };
        let hint = format!(
            "[ shrink · ] grow · = reset · {view_toggle} · f filter · {evidence_toggle} · {} · ↑/↓ scroll · n close · Esc/q/[x] close",
            scroll_hint::scroll_status_label(scroll, max_scroll)
        );

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
            frame.render_widget(body, body_area);
            frame.render_widget(close, close_button_rect(popup_area));
            frame.render_widget(
                Paragraph::new(hint).style(Style::default().fg(theme::text_dim())),
                hint_area,
            );
            return;
        }

        let inner = block.inner(body_area);
        frame.render_widget(block, body_area);
        frame.render_widget(close, close_button_rect(popup_area));
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(theme::text_dim())),
            hint_area,
        );

        let header = format!(
            "{:<8}  {:<6}  {:<22}  {:<6}  {:<8}  {}",
            "Time", "Pane", "Kind", "Conf", "Promoted", "Reason"
        );

        let visible_rows = inner.height.saturating_sub(1) as usize;
        let filter = self.filter;
        // v1.59.0: color rows by AnomalySignal severity. Text format
        // unchanged; ListItem now carries a styled Line with the row's
        // severity-color fg. Operators read severity at a glance from
        // the row tint instead of having to scan the Conf column.
        let evidence_expanded = self.evidence_expanded;
        let row_to_item = |e: &crate::domain::anomaly::AnomalyEvent| {
            let text = format_event_row(e, inner.width as usize);
            let style = Style::default().fg(theme::severity_color(e.severity));
            let mut lines = vec![Line::from(ratatui::text::Span::styled(text, style))];
            if evidence_expanded {
                lines.extend(format_event_evidence_lines(e, inner.width as usize));
            }
            ListItem::new(lines)
        };
        let items: Vec<ListItem> = match self.view {
            AnomalyOverlayView::Ring => ring
                .iter()
                .rev()
                .filter(|e| filter.matches(e))
                .skip(scroll as usize)
                .take(visible_rows)
                .map(row_to_item)
                .collect(),
            AnomalyOverlayView::History => self
                .history_cache
                .iter()
                .filter(|e| filter.matches(e))
                .skip(scroll as usize)
                .take(visible_rows)
                .map(row_to_item)
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

fn anomaly_visible_scroll_bound(total_count: usize, visible_rows: u16) -> u16 {
    total_count
        .saturating_sub(visible_rows.max(1) as usize)
        .min(u16::MAX as usize) as u16
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
    let reason_source = format_event_reason(e);
    let reason = ellipsize(&reason_source, reason_budget);
    format!("{row_prefix}{reason}")
}

fn format_event_reason(e: &crate::domain::anomaly::AnomalyEvent) -> String {
    match anomaly_basis_label(e.kind) {
        Some(label) if e.reason.is_empty() => label.to_string(),
        Some(label) => format!("{label}: {}", e.reason),
        None => e.reason.clone(),
    }
}

fn format_event_evidence_lines(
    e: &crate::domain::anomaly::AnomalyEvent,
    width: usize,
) -> Vec<Line<'static>> {
    e.evidence
        .iter()
        .take(3)
        .map(|evidence| {
            let text = format!(
                "  {} {} -> {} over {} samples [{}]",
                evidence.metric_name,
                evidence.before,
                evidence.after,
                evidence.sample_count,
                evidence.source_kind
            );
            Line::from(ellipsize(&text, width)).style(Style::default().fg(theme::text_dim()))
        })
        .collect()
}

fn anomaly_basis_label(kind: crate::domain::anomaly::AnomalyKind) -> Option<&'static str> {
    use crate::domain::anomaly::AnomalyKind;
    match kind {
        AnomalyKind::ErrorBurst | AnomalyKind::MemoryGrowth => Some("heuristic"),
        AnomalyKind::SubagentSideEffect => Some("correlation"),
        _ => None,
    }
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

        assert_eq!(cell.fg, theme::border_active());
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
            evidence: Vec::new(),
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
            evidence: Vec::new(),
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
            evidence: Vec::new(),
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
            evidence: Vec::new(),
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
            evidence: Vec::new(),
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
            evidence: Vec::new(),
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
            evidence: Vec::new(),
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
            evidence: Vec::new(),
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

    #[test]
    fn render_ring_view_labels_heuristic_anomaly_basis() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let mut ring = AnomalyEventsRing::new();
        ring.push(AnomalyEvent {
            timestamp: 1_700_000_000,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::ErrorBurst,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "error_rate: 0.10 \u{2192} 0.80".to_string(),
            evidence: Vec::new(),
        });

        terminal
            .draw(|f| overlay.render(f, f.area(), &ring))
            .unwrap();
        let rendered = buf_to_string(terminal.backend().buffer());

        assert!(
            rendered.contains("heuristic"),
            "noisy anomaly kinds should label their evidence basis: {rendered}"
        );
    }

    #[test]
    fn render_ring_view_keeps_scroll_status_in_footer_not_title() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let viewport = Rect::new(0, 0, 160, 30);
        let mut terminal =
            Terminal::new(TestBackend::new(viewport.width, viewport.height)).unwrap();
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
            evidence: Vec::new(),
        });

        terminal
            .draw(|f| overlay.render(f, f.area(), &ring))
            .unwrap();
        let rendered = buf_to_string(terminal.backend().buffer());
        let title_line = rendered
            .lines()
            .find(|line| line.contains("ANOMALY EVENTS"))
            .unwrap_or_else(|| panic!("anomaly title line missing:\n{rendered}"));

        assert!(
            !title_line.contains("scroll"),
            "anomaly title should identify the window only: {title_line}"
        );
        assert!(
            rendered.contains("scroll 0/0 · END"),
            "anomaly footer should carry scroll status:\n{rendered}"
        );
        assert!(
            rendered.contains("[ shrink · ] grow · = reset"),
            "anomaly footer should carry shared chrome controls:\n{rendered}"
        );
    }

    #[test]
    fn render_ring_view_reports_end_at_last_full_page() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let viewport = Rect::new(0, 0, 160, 24);
        let mut terminal =
            Terminal::new(TestBackend::new(viewport.width, viewport.height)).unwrap();
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let popup = anomaly_modal_area_for(viewport, &overlay);
        let visible_rows = popup.height.saturating_sub(2).saturating_sub(1).max(1) as usize;
        let total = 104usize;
        let last_full_page_scroll = total.saturating_sub(visible_rows) as u16;
        for _ in 0..last_full_page_scroll {
            overlay.scroll_down(u16::MAX);
        }

        let mut ring = AnomalyEventsRing::new();
        for i in 0..total {
            ring.push(AnomalyEvent {
                timestamp: 1_700_000_000 + i as u64,
                pane_id: format!("%{i}"),
                kind: AnomalyKind::CostSlope,
                confidence: AnomalyConfidence::High,
                severity: Severity::Warning,
                promoted: true,
                reason: "paged row".to_string(),
                evidence: Vec::new(),
            });
        }

        terminal
            .draw(|f| overlay.render(f, f.area(), &ring))
            .unwrap();
        let rendered = buf_to_string(terminal.backend().buffer());
        assert!(
            rendered.contains("END"),
            "last full anomaly page should render END, not more: {rendered}"
        );
        assert!(
            !rendered.contains("more"),
            "last full anomaly page should not advertise more rows: {rendered}"
        );
    }

    #[test]
    fn evidence_rows_render_only_when_expanded() {
        use crate::app::anomaly_events_ring::AnomalyEventsRing;
        use crate::domain::anomaly::{
            AnomalyConfidence, AnomalyEvent, AnomalyEvidence, AnomalyKind,
        };
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut ring = AnomalyEventsRing::new();
        ring.push(AnomalyEvent {
            timestamp: 1_700_000_000,
            pane_id: "%1".into(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "cost rose".into(),
            evidence: vec![AnomalyEvidence {
                metric_name: "cost_usd",
                before: "$0.12".into(),
                after: "$0.34".into(),
                sample_count: 6,
                source_kind: SourceKind::Estimated,
            }],
        });
        let viewport = Rect::new(0, 0, 120, 30);
        let mut overlay = AnomalyOverlay::new();
        overlay.open();
        let mut terminal =
            Terminal::new(TestBackend::new(viewport.width, viewport.height)).unwrap();
        terminal
            .draw(|frame| overlay.render(frame, viewport, &ring))
            .unwrap();
        let collapsed = buf_to_string(terminal.backend().buffer());
        assert!(!collapsed.contains("cost_usd"));

        overlay.toggle_evidence();
        terminal
            .draw(|frame| overlay.render(frame, viewport, &ring))
            .unwrap();
        let expanded = buf_to_string(terminal.backend().buffer());
        assert!(expanded.contains("cost_usd"));
        assert!(expanded.contains("$0.12 -> $0.34"));
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
