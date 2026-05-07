use crate::insights_report::format_insights_report_lines;
use crate::store::InsightsSnapshot;
use crate::ui::dashboard::close_button_rect;
use crate::ui::modal_chrome::{DragAnchor, ModalGeometry};
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

#[derive(Debug, Clone)]
pub struct InsightsOverlay {
    open: bool,
    scroll: u16,
    geometry: ModalGeometry,
    snapshot: Option<InsightsSnapshot>,
    error: Option<String>,
    refreshed_label: Option<String>,
    loading: bool,
    spinner_phase: u8,
    pending_request_id: u64,
}

impl Default for InsightsOverlay {
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
            snapshot: None,
            error: None,
            refreshed_label: None,
            loading: false,
            spinner_phase: 0,
            pending_request_id: 0,
        }
    }
}

impl InsightsOverlay {
    pub const SIZE_STEP: u16 = 5;
    pub const SIZE_MIN: u16 = 50;
    pub const SIZE_MAX: u16 = 99;
    pub const DEFAULT_WIDTH_PCT: u16 = 86;
    pub const DEFAULT_HEIGHT_PCT: u16 = 78;

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
        self.loading = false;
        self.geometry.end_drag();
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn pending_request_id(&self) -> u64 {
        self.pending_request_id
    }

    pub fn mark_loading(&mut self, request_id: u64) {
        self.loading = true;
        self.spinner_phase = 0;
        self.pending_request_id = request_id;
        self.snapshot = None;
        self.error = None;
        self.refreshed_label = None;
        self.scroll = 0;
    }

    pub fn spinner_glyph(&self) -> char {
        SPINNER_FRAMES[(self.spinner_phase as usize) % SPINNER_FRAMES.len()]
    }

    pub fn advance_spinner(&mut self) {
        if !self.loading {
            return;
        }
        self.spinner_phase = (self.spinner_phase + 1) % SPINNER_FRAMES.len() as u8;
    }

    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self, max: u16) {
        self.scroll = self.scroll.saturating_add(1).min(max);
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

    pub fn set_snapshot(&mut self, snapshot: InsightsSnapshot, refreshed_label: String) {
        self.snapshot = Some(snapshot);
        self.error = None;
        self.refreshed_label = Some(refreshed_label);
        self.scroll = 0;
    }

    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.snapshot = None;
        self.refreshed_label = None;
        self.scroll = 0;
    }

    pub fn set_snapshot_for(
        &mut self,
        request_id: u64,
        result: Result<InsightsSnapshot, String>,
        refreshed_label: String,
    ) {
        if !self.open || !self.loading || self.pending_request_id != request_id {
            return;
        }
        self.loading = false;
        match result {
            Ok(snapshot) => self.set_snapshot(snapshot, refreshed_label),
            Err(error) => self.set_error(error),
        }
    }

    pub fn line_count(&self) -> usize {
        if self.loading {
            return 0;
        }
        self.lines().len()
    }

    fn lines(&self) -> Vec<String> {
        if let Some(error) = self.error.as_ref() {
            return vec![
                "Token Insights".into(),
                String::new(),
                format!("error: {error}"),
            ];
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            return vec![
                "Token Insights".into(),
                String::new(),
                "No insights snapshot loaded.".into(),
                "Press r to refresh.".into(),
            ];
        };
        let mut lines = format_insights_report_lines(snapshot);
        if let Some(label) = self.refreshed_label.as_ref() {
            lines.insert(1, format!("refreshed: {label}"));
        }
        lines
    }
}

pub fn insights_modal_area(viewport: ratatui::layout::Rect) -> ratatui::layout::Rect {
    crate::ui::modal_chrome::modal_area(
        viewport,
        InsightsOverlay::DEFAULT_WIDTH_PCT,
        InsightsOverlay::DEFAULT_HEIGHT_PCT,
        0,
        0,
    )
}

pub fn insights_modal_area_for(
    viewport: ratatui::layout::Rect,
    overlay: &InsightsOverlay,
) -> ratatui::layout::Rect {
    crate::ui::modal_chrome::modal_area(
        viewport,
        overlay.width_pct(),
        overlay.height_pct(),
        overlay.offset_x(),
        overlay.offset_y(),
    )
}

pub fn render_insights_modal(frame: &mut Frame<'_>, overlay: &InsightsOverlay) {
    let area = insights_modal_area_for(frame.area(), overlay);
    frame.render_widget(Clear, area);
    let title = if overlay.is_loading() {
        " Token Insights — loading [i/Esc/q close] "
    } else {
        " Token Insights [i/Esc/q close] [r refresh] "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(area),
    );

    if overlay.is_loading() {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);
        let line = format!("Aggregating insights {}", overlay.spinner_glyph());
        let paragraph = Paragraph::new(line).alignment(Alignment::Center).style(
            Style::default()
                .fg(theme::BORDER_ACTIVE)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_widget(paragraph, rows[1]);
        return;
    }

    let lines = overlay.lines();
    if lines.len() <= 4 && overlay.snapshot.is_none() {
        let paragraph = Paragraph::new(lines.join("\n")).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
        return;
    }

    let items: Vec<ListItem> = lines
        .into_iter()
        .skip(overlay.scroll as usize)
        .take(inner.height as usize)
        .map(ListItem::new)
        .collect();
    frame.render_widget(List::new(items), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights_report::empty_insights_snapshot;
    use crate::store::InsightsWindow;

    #[test]
    fn overlay_open_resets_scroll() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.scroll_down(10);
        assert_eq!(overlay.scroll(), 1);
        overlay.close();
        overlay.open();
        assert_eq!(overlay.scroll(), 0);
    }

    #[test]
    fn snapshot_lines_include_action_ledger() {
        let mut overlay = InsightsOverlay::new();
        overlay.set_snapshot(
            empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            }),
            "12:00:00".into(),
        );

        let joined = overlay.lines().join("\n");
        assert!(joined.contains("Action Ledger"));
        assert!(joined.contains("refreshed: 12:00:00"));
    }

    #[test]
    fn mark_loading_sets_loading_flag_and_pending_id() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        assert!(!overlay.is_loading());
        overlay.mark_loading(7);
        assert!(overlay.is_loading());
        assert_eq!(overlay.pending_request_id(), 7);
    }

    #[test]
    fn mark_loading_clears_snapshot_and_error() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.set_snapshot(
            empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            }),
            "12:00:00".into(),
        );
        overlay.mark_loading(1);
        let joined = overlay.lines().join("\n");
        assert!(!joined.contains("Action Ledger"));
        assert!(!joined.contains("12:00:00"));
    }

    #[test]
    fn render_uses_active_border_style() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let viewport = ratatui::layout::Rect::new(0, 0, 100, 40);
        let mut terminal =
            Terminal::new(TestBackend::new(viewport.width, viewport.height)).unwrap();
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        let area = insights_modal_area_for(viewport, &overlay);

        terminal
            .draw(|frame| render_insights_modal(frame, &overlay))
            .unwrap();
        let cell = terminal
            .backend()
            .buffer()
            .cell((area.x, area.y))
            .expect("top-left border cell in bounds");

        assert_eq!(cell.fg, theme::BORDER_ACTIVE);
    }

    #[test]
    fn advance_spinner_cycles_through_braille_phases() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(1);
        assert_eq!(overlay.spinner_glyph(), '⠋');
        overlay.advance_spinner();
        assert_eq!(overlay.spinner_glyph(), '⠙');
        for _ in 0..9 {
            overlay.advance_spinner();
        }
        assert_eq!(overlay.spinner_glyph(), '⠋');
    }

    #[test]
    fn advance_spinner_is_no_op_when_not_loading() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        let before = overlay.spinner_glyph();
        overlay.advance_spinner();
        assert_eq!(overlay.spinner_glyph(), before);
    }

    #[test]
    fn set_snapshot_for_drops_stale_request_id() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(2);
        overlay.set_snapshot_for(
            1,
            Ok(empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            })),
            "stale".into(),
        );
        assert!(overlay.is_loading());
        assert_eq!(overlay.pending_request_id(), 2);
    }

    #[test]
    fn set_snapshot_for_accepts_matching_request_id_with_ok() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(2);
        overlay.set_snapshot_for(
            2,
            Ok(empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            })),
            "12:00:00".into(),
        );
        assert!(!overlay.is_loading());
        let joined = overlay.lines().join("\n");
        assert!(joined.contains("refreshed: 12:00:00"));
    }

    #[test]
    fn set_snapshot_for_accepts_matching_request_id_with_err() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(3);
        overlay.set_snapshot_for(3, Err("boom".into()), "n/a".into());
        assert!(!overlay.is_loading());
        let joined = overlay.lines().join("\n");
        assert!(joined.contains("error: boom"));
    }

    #[test]
    fn close_while_loading_drops_late_outcome() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(4);
        overlay.close();
        overlay.set_snapshot_for(
            4,
            Ok(empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            })),
            "12:00:00".into(),
        );
        overlay.open();
        let joined = overlay.lines().join("\n");
        assert!(!joined.contains("refreshed: 12:00:00"));
        assert!(!joined.contains("Action Ledger"));
    }

    #[test]
    fn line_count_is_zero_while_loading() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(1);
        assert_eq!(overlay.line_count(), 0);
    }

    #[test]
    fn loading_state_renders_centered_spinner_and_text() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let viewport = ratatui::layout::Rect::new(0, 0, 100, 40);
        let mut terminal =
            Terminal::new(TestBackend::new(viewport.width, viewport.height)).unwrap();
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(1);

        terminal
            .draw(|frame| render_insights_modal(frame, &overlay))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut joined = String::new();
        for y in 0..viewport.height {
            for x in 0..viewport.width {
                if let Some(cell) = buffer.cell((x, y)) {
                    joined.push_str(cell.symbol());
                }
            }
            joined.push('\n');
        }
        assert!(joined.contains("Aggregating insights"), "buffer:\n{joined}");
        assert!(joined.contains('⠋'), "buffer:\n{joined}");
        assert!(!joined.contains("Action Ledger"), "buffer:\n{joined}");
        assert!(!joined.contains("CACHE TREND"), "buffer:\n{joined}");
    }
}
