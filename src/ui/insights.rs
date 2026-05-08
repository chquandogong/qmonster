use std::time::{Duration, Instant};

use crate::insights_report::format_insights_report_lines;
use crate::store::InsightsSnapshot;
use crate::ui::dashboard::close_button_rect;
use crate::ui::modal_chrome::{DragAnchor, ModalGeometry};
use crate::ui::scroll_hint;
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// v1.60.0: snapshot is treated as stale once this many seconds have
/// passed since `loaded_at`. Operators get a `· stale` chip in the
/// title + a hint to press `r` for a fresh load.
pub const INSIGHTS_STALENESS_SECS: u64 = 300;

#[derive(Debug, Clone)]
pub struct InsightsOverlay {
    open: bool,
    scroll: u16,
    geometry: ModalGeometry,
    snapshot: Option<InsightsSnapshot>,
    error: Option<String>,
    refreshed_label: Option<String>,
    /// v1.60.0: monotonic timestamp of the last successful snapshot
    /// load. Used by the renderer to compute relative age + the
    /// `stale` chip when no fresh data has landed in
    /// `INSIGHTS_STALENESS_SECS`.
    loaded_at: Option<Instant>,
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
            loaded_at: None,
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
        self.loaded_at = None;
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
        self.set_snapshot_at(snapshot, refreshed_label, Instant::now());
    }

    pub fn set_snapshot_at(
        &mut self,
        snapshot: InsightsSnapshot,
        refreshed_label: String,
        loaded_at: Instant,
    ) {
        self.snapshot = Some(snapshot);
        self.error = None;
        self.refreshed_label = Some(refreshed_label);
        self.loaded_at = Some(loaded_at);
        self.scroll = 0;
    }

    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.snapshot = None;
        self.refreshed_label = None;
        self.loaded_at = None;
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

    pub fn loaded_at(&self) -> Option<Instant> {
        self.loaded_at
    }

    /// v1.60.0: returns true once the snapshot has aged past
    /// `INSIGHTS_STALENESS_SECS`. Returns false when no snapshot is
    /// loaded — the empty-state UX handles that case separately.
    pub fn is_stale(&self, now: Instant) -> bool {
        let Some(loaded_at) = self.loaded_at else {
            return false;
        };
        now.saturating_duration_since(loaded_at) >= Duration::from_secs(INSIGHTS_STALENESS_SECS)
    }

    /// v1.60.0: short relative-age label ("just now", "2m ago",
    /// "1h ago"). Returns None when there is no loaded snapshot.
    pub fn relative_age_label(&self, now: Instant) -> Option<String> {
        let loaded_at = self.loaded_at?;
        let secs = now.saturating_duration_since(loaded_at).as_secs();
        Some(format_relative_age(secs))
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

/// v1.60.0: render a short relative-age label suitable for the title
/// chip ("just now", "23s ago", "5m ago", "2h ago"). Stays purely
/// based on seconds so unit tests can pass any deterministic value.
pub fn format_relative_age(secs: u64) -> String {
    if secs < 5 {
        "just now".to_string()
    } else if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else {
        format!("{}d ago", secs / 86_400)
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

pub fn render_insights_modal(frame: &mut Frame<'_>, overlay: &InsightsOverlay, now: Instant) {
    let area = insights_modal_area_for(frame.area(), overlay);
    frame.render_widget(Clear, area);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    let body_area = layout[0];
    let hint_area = layout[1];
    let block = Block::default()
        .borders(Borders::ALL)
        .title_style(Style::default().add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(theme::border_active()));
    let inner = block.inner(body_area);
    let max_scroll = if overlay.is_loading() {
        0
    } else {
        overlay
            .line_count()
            .saturating_sub(inner.height as usize)
            .min(u16::MAX as usize) as u16
    };
    let scroll = overlay.scroll().min(max_scroll);
    let title = if overlay.is_loading() {
        " Token Insights · loading ".to_string()
    } else if let Some(age) = overlay.relative_age_label(now) {
        if overlay.is_stale(now) {
            format!(" Token Insights · {age} · stale (press r) ")
        } else {
            format!(" Token Insights · {age} ")
        }
    } else {
        " Token Insights ".to_string()
    };
    let block = block.title(title);
    frame.render_widget(block, body_area);
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(area),
    );
    let hint = format!(
        "[ shrink · ] grow · = reset · r refresh · ↑/↓ scroll · i close · Esc/q close · click [x] close · {}",
        scroll_hint::scroll_status_label(scroll, max_scroll)
    );
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(theme::text_dim())),
        hint_area,
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
                .fg(theme::border_active())
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
        .skip(scroll as usize)
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

    // v1.60.0 -----------------------------------------------------------

    #[test]
    fn relative_age_label_covers_each_bucket_boundary() {
        assert_eq!(format_relative_age(0), "just now");
        assert_eq!(format_relative_age(4), "just now");
        assert_eq!(format_relative_age(5), "5s ago");
        assert_eq!(format_relative_age(59), "59s ago");
        assert_eq!(format_relative_age(60), "1m ago");
        assert_eq!(format_relative_age(3_599), "59m ago");
        assert_eq!(format_relative_age(3_600), "1h ago");
        assert_eq!(format_relative_age(86_399), "23h ago");
        assert_eq!(format_relative_age(86_400), "1d ago");
    }

    #[test]
    fn is_stale_flips_after_threshold_and_stays_false_without_snapshot() {
        let mut overlay = InsightsOverlay::new();
        let t0 = Instant::now();
        // Empty overlay: never stale (the empty-state UX handles it).
        assert!(!overlay.is_stale(t0));

        let snapshot = empty_insights_snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 1,
        });
        overlay.set_snapshot_at(snapshot, "12:00:00".into(), t0);

        // Just-loaded: not stale.
        assert!(!overlay.is_stale(t0));
        assert!(!overlay.is_stale(t0 + std::time::Duration::from_secs(60)));

        // Past the staleness threshold: stale.
        let stale_at = t0 + std::time::Duration::from_secs(INSIGHTS_STALENESS_SECS);
        assert!(overlay.is_stale(stale_at));
        assert!(overlay.is_stale(stale_at + std::time::Duration::from_secs(60)));
    }

    #[test]
    fn relative_age_label_returns_none_without_loaded_snapshot() {
        let overlay = InsightsOverlay::new();
        assert!(overlay.relative_age_label(Instant::now()).is_none());
    }

    #[test]
    fn mark_loading_clears_loaded_at_so_age_disappears_under_spinner() {
        let mut overlay = InsightsOverlay::new();
        let t0 = Instant::now();
        let snapshot = empty_insights_snapshot(InsightsWindow {
            since_ms: 0,
            until_ms: 1,
        });
        overlay.set_snapshot_at(snapshot, "12:00:00".into(), t0);
        assert!(overlay.relative_age_label(t0).is_some());

        overlay.open();
        overlay.mark_loading(7);
        assert!(overlay.loaded_at().is_none());
        assert!(overlay.relative_age_label(t0).is_none());
    }

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

    /// v1.56.0 regression guard pinning the audit doc's "Situations
    /// always renders 'none'" perception fix. The insights overlay
    /// must surface populated Situations rows verbatim — situation
    /// label + emitted count — so future refactors of
    /// `format_insights_report_lines` cannot silently drop them.
    #[test]
    fn rendered_overlay_surfaces_populated_situations_section() {
        use crate::store::{InsightsSnapshot, SituationSummary};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let snapshot = InsightsSnapshot {
            window: InsightsWindow {
                since_ms: 0,
                until_ms: 3_600_000,
            },
            situations: vec![
                SituationSummary {
                    situation: "context_pressure_warning",
                    emitted: 7,
                },
                SituationSummary {
                    situation: "log_storm_advisory",
                    emitted: 2,
                },
            ],
            cache: Default::default(),
            timeline: vec![],
            actions: vec![],
            ignored_available: true,
        };

        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.set_snapshot(snapshot, "13:14:15".into());

        // 1) Pure builder: Situations section header + each row
        // formatted as `  <situation>: <emitted>` must be present.
        let line_dump = overlay.lines().join("\n");
        assert!(
            line_dump.contains("Situations"),
            "Situations header missing from overlay lines: {line_dump}"
        );
        assert!(
            line_dump.contains("context_pressure_warning: 7"),
            "first Situations row missing from overlay lines: {line_dump}"
        );
        assert!(
            line_dump.contains("log_storm_advisory: 2"),
            "second Situations row missing from overlay lines: {line_dump}"
        );

        // 2) End-to-end render: drive the overlay through ratatui's
        // TestBackend so we also catch a future regression that drops
        // the rows during the modal layout / paragraph rendering.
        let viewport = ratatui::layout::Rect::new(0, 0, 120, 40);
        let mut terminal =
            Terminal::new(TestBackend::new(viewport.width, viewport.height)).unwrap();
        terminal
            .draw(|frame| render_insights_modal(frame, &overlay, Instant::now()))
            .unwrap();
        let buf = terminal.backend().buffer();
        let buf_dump: String = buf
            .content
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect();
        assert!(
            buf_dump.contains("context_pressure_warning"),
            "rendered buffer missing context_pressure_warning row"
        );
        assert!(
            buf_dump.contains("log_storm_advisory"),
            "rendered buffer missing log_storm_advisory row"
        );
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
            .draw(|frame| render_insights_modal(frame, &overlay, Instant::now()))
            .unwrap();
        let cell = terminal
            .backend()
            .buffer()
            .cell((area.x, area.y))
            .expect("top-left border cell in bounds");

        assert_eq!(cell.fg, theme::border_active());
    }

    #[test]
    fn render_keeps_scroll_status_in_footer_not_title() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let viewport = ratatui::layout::Rect::new(0, 0, 160, 36);
        let mut terminal =
            Terminal::new(TestBackend::new(viewport.width, viewport.height)).unwrap();
        let mut overlay = InsightsOverlay::new();
        overlay.open();

        terminal
            .draw(|frame| render_insights_modal(frame, &overlay, Instant::now()))
            .unwrap();
        let rendered = buf_to_string(terminal.backend().buffer());
        let title_line = rendered
            .lines()
            .find(|line| line.contains("Token Insights"))
            .unwrap_or_else(|| panic!("insights title line missing:\n{rendered}"));

        assert!(
            !title_line.contains("scroll"),
            "insights title should identify the window only: {title_line}"
        );
        assert!(
            rendered.contains("scroll 0/0 · END"),
            "insights footer should carry scroll status:\n{rendered}"
        );
        assert!(
            rendered.contains("[ shrink · ] grow · = reset"),
            "insights footer should carry shared chrome controls:\n{rendered}"
        );
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
            .draw(|frame| render_insights_modal(frame, &overlay, Instant::now()))
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
