//! Phase C (v1.38) — Metrics Overlay state container.
//!
//! v1.38 layout v2 (operator feedback): the comparison/detail split is
//! gone in favor of per-pane cards so all panes' metrics are visible
//! at once without ↑/↓ selection. Bars widen to 24 cells (scaled down
//! on narrow viewports), 5H/7D reset etas live at the top of the right
//! column as plain text (no progress bar), COST appears only once
//! (right column), and ↑/↓ now scroll the body.
//!
//! Pure-state struct with open/close + scroll APIs. Tests cover state
//! transitions in isolation so renderer and key-handler tests can rely
//! on a stable state contract.

use crate::app::event_loop::PaneReport;
use crate::store::TokenSample;
use crate::ui::dashboard::centered_rect;
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

#[derive(Debug, Clone)]
pub struct MetricsOverlay {
    open: bool,
    scroll: u16,
    width_pct: u16,
    height_pct: u16,
}

impl Default for MetricsOverlay {
    fn default() -> Self {
        Self {
            open: false,
            scroll: 0,
            width_pct: Self::DEFAULT_WIDTH_PCT,
            height_pct: Self::DEFAULT_HEIGHT_PCT,
        }
    }
}

impl MetricsOverlay {
    pub const SIZE_STEP: u16 = 5;
    pub const SIZE_MIN: u16 = 50;
    pub const SIZE_MAX: u16 = 99;
    pub const DEFAULT_WIDTH_PCT: u16 = 95;
    pub const DEFAULT_HEIGHT_PCT: u16 = 90;

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
        self.width_pct
    }

    pub fn height_pct(&self) -> u16 {
        self.height_pct
    }

    pub fn grow(&mut self) {
        self.width_pct = (self.width_pct + Self::SIZE_STEP).min(Self::SIZE_MAX);
        self.height_pct = (self.height_pct + Self::SIZE_STEP).min(Self::SIZE_MAX);
    }

    pub fn shrink(&mut self) {
        self.width_pct = self
            .width_pct
            .saturating_sub(Self::SIZE_STEP)
            .max(Self::SIZE_MIN);
        self.height_pct = self
            .height_pct
            .saturating_sub(Self::SIZE_STEP)
            .max(Self::SIZE_MIN);
    }

    pub fn reset_size(&mut self) {
        self.width_pct = Self::DEFAULT_WIDTH_PCT;
        self.height_pct = Self::DEFAULT_HEIGHT_PCT;
    }
}

pub struct MetricsModalRects {
    pub area: Rect,
    pub banner: Rect,
    pub body: Rect,
    pub hint: Rect,
}

pub fn metrics_modal_rects(viewport: Rect, width_pct: u16, height_pct: u16) -> MetricsModalRects {
    let area = centered_rect(width_pct, height_pct, viewport);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // banner
            Constraint::Min(6),    // body (per-pane cards, scrollable)
            Constraint::Length(1), // hint
        ])
        .split(area);
    MetricsModalRects {
        area,
        banner: chunks[0],
        body: chunks[1],
        hint: chunks[2],
    }
}

/// Returns the "Hottest: <pane> · <metric> {pct}% [<source>]" header
/// line. If no pane has any bounded pressure data, returns
/// "Hottest: —" with dim style. Spec §4.3.
pub fn hottest_banner_line(reports: &[PaneReport]) -> Line<'static> {
    let mut best: Option<(f32, String, &'static str, crate::domain::origin::SourceKind)> = None;

    for r in reports {
        let label = pane_label(r);
        if let Some(m) = &r.signals.context_pressure {
            consider(&mut best, m.value, &label, "CTX", m.source_kind);
        }
        if let Some(m) = &r.signals.quota_5h_pressure {
            consider(&mut best, m.value, &label, "5H", m.source_kind);
        }
        if let Some(m) = &r.signals.quota_weekly_pressure {
            consider(&mut best, m.value, &label, "7D", m.source_kind);
        }
    }

    match best {
        Some((value, label, metric, source)) => Line::from(format!(
            "Hottest: {label} · {metric} {pct}% [{src}]",
            pct = (value * 100.0).round() as i32,
            src = crate::ui::labels::source_kind_label(source),
        )),
        None => Line::from(Span::styled(
            "Hottest: —",
            Style::default().fg(theme::TEXT_DIM),
        )),
    }
}

fn consider(
    best: &mut Option<(f32, String, &'static str, crate::domain::origin::SourceKind)>,
    value: f32,
    label: &str,
    metric: &'static str,
    source: crate::domain::origin::SourceKind,
) {
    let beats = best.as_ref().is_none_or(|(v, _, _, _)| value > *v);
    if beats {
        *best = Some((value, label.to_string(), metric, source));
    }
}

fn pane_label(r: &PaneReport) -> String {
    let id = &r.identity.identity;
    let role = match id.role {
        crate::domain::identity::Role::Main => "main",
        crate::domain::identity::Role::Review => "review",
        crate::domain::identity::Role::Research => "research",
        crate::domain::identity::Role::Monitor => "monitor",
        crate::domain::identity::Role::Unknown => "?",
    };
    let provider = match id.provider {
        crate::domain::identity::Provider::Claude => "claude",
        crate::domain::identity::Provider::Codex => "codex",
        crate::domain::identity::Provider::Gemini => "gemini",
        crate::domain::identity::Provider::Qmonster => "qmonster",
        crate::domain::identity::Provider::Unknown => "?",
    };
    format!("{provider}:{}:{role}", id.instance)
}

const BAR_MAX_CELLS: usize = 24;
const BAR_MIN_CELLS: usize = 8;

/// Render a pressure bar with `cells` total width — `█` for filled,
/// `░` for unfilled. Severity color is applied by the caller via Span.
fn render_pressure_bar(ratio: f32, cells: usize) -> String {
    let filled = ((ratio.clamp(0.0, 1.0) * cells as f32).round() as usize).min(cells);
    let mut s = String::with_capacity(cells);
    for i in 0..cells {
        s.push(if i < filled { '█' } else { '░' });
    }
    s
}

fn pct(v: f32) -> String {
    format!("{}%", (v * 100.0).round() as i32)
}

const DASH: &str = "─";

const SPARK_GLYPHS: &str = "▁▂▃▄▅▆▇█";

/// Compute the bar width for the given left-half budget.
///
/// Layout per left row: `LABEL   <bar>  <pct>` where:
/// - LABEL column = 6 chars + 3 spaces = 9
/// - pct column   = 4 chars (e.g. "100%")
/// - gap before pct = 2 chars
///
/// So bar = `left_half_width - 9 - 2 - 4` clamped to [BAR_MIN_CELLS, BAR_MAX_CELLS].
fn bar_cells_for_left_half(left_half_width: u16) -> usize {
    let raw = (left_half_width as i32) - 9 - 2 - 4;
    raw.clamp(BAR_MIN_CELLS as i32, BAR_MAX_CELLS as i32) as usize
}

/// Compute the sparkline width for the right-half budget given a
/// label prefix length and a suffix (current + delta) reservation.
/// `right_half_width - label - suffix - padding` clamped to a sane
/// minimum so we always render *some* glyph row even at narrow widths.
fn sparkline_cells(right_half_width: u16, label_len: usize, suffix_len: usize) -> usize {
    let padding = 4; // gaps around sparkline (2 spaces before + 2 after)
    let raw = (right_half_width as i32) - (label_len as i32) - (suffix_len as i32) - padding;
    raw.clamp(4, 32) as usize
}

/// Pure helper that produces the modal body lines. Tested directly;
/// `render_metrics_modal` uses this to feed a `Paragraph`. Lays out
/// per-pane cards, each card = card-divider line + 6 content rows
/// (`<left half> │ <right half>`).
pub fn render_metrics_lines(
    _overlay: &MetricsOverlay,
    _target_label: &str,
    reports: &[PaneReport],
    body: Rect,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(hottest_banner_line(reports));

    if reports.is_empty() {
        return out;
    }

    let body_width = body.width.max(20);
    // Inner width = body width minus the surrounding modal border (2
    // chars). A `body` that came directly from `metrics_modal_rects`
    // already excludes the borders, but `render_metrics_lines` may be
    // called with the outer `area`, so subtract defensively.
    let inner_width = body_width.saturating_sub(2);
    // Two halves separated by " │ " (3 chars). Any odd remainder goes
    // to the right half so the sparkline keeps a touch more room.
    let separator_len: u16 = 3;
    let halves_budget = inner_width.saturating_sub(separator_len);
    let left_half_width = halves_budget / 2;
    let right_half_width = halves_budget - left_half_width;

    for r in reports {
        out.push(card_divider_line(r, inner_width));
        out.extend(card_rows(r, left_half_width, right_half_width));
    }

    out
}

fn card_divider_line(report: &PaneReport, inner_width: u16) -> Line<'static> {
    let label = pane_label(report);
    let prefix = format!("━ {label} · {pane_id} ", pane_id = report.pane_id);
    let prefix_len = prefix.chars().count();
    let trail = (inner_width as usize).saturating_sub(prefix_len);
    let mut s = prefix;
    for _ in 0..trail {
        s.push('━');
    }
    Line::from(Span::styled(s, Style::default().fg(theme::TEXT_DIM)))
}

fn card_rows(
    report: &PaneReport,
    left_half_width: u16,
    right_half_width: u16,
) -> Vec<Line<'static>> {
    let bar_cells = bar_cells_for_left_half(left_half_width);
    let s = &report.signals;

    let left_rows = [
        left_row(
            "CTX",
            s.context_pressure.as_ref().map(|m| m.value),
            bar_cells,
        ),
        left_row(
            "5H",
            s.quota_5h_pressure.as_ref().map(|m| m.value),
            bar_cells,
        ),
        left_row(
            "7D",
            s.quota_weekly_pressure.as_ref().map(|m| m.value),
            bar_cells,
        ),
        left_row(
            "CACHE",
            s.cache_hit_ratio.as_ref().map(|m| m.value as f32),
            bar_cells,
        ),
        String::new(), // row 5 left blank (left col only has 4 metrics)
        String::new(), // row 6 left blank
    ];

    let right_rows = [
        right_reset_row("5H reset", s.quota_5h_resets_at.as_ref().map(|m| m.value)),
        right_reset_row(
            "7D reset",
            s.quota_weekly_resets_at.as_ref().map(|m| m.value),
        ),
        right_tokens_row(
            "TOKENS in",
            &report.recent_token_samples,
            |s| s.input_tokens,
            right_half_width,
        ),
        right_tokens_row(
            "TOKENS out",
            &report.recent_token_samples,
            |s| s.output_tokens,
            right_half_width,
        ),
        right_cost_row(
            &report.recent_token_samples,
            s.cost_usd.as_ref().map(|m| m.value),
            right_half_width,
        ),
        right_mem_row(s),
    ];

    let mut lines = Vec::with_capacity(6);
    for (l, r) in left_rows.iter().zip(right_rows.iter()) {
        lines.push(card_row(l, r, left_half_width, right_half_width));
    }
    lines
}

fn card_row(left: &str, right: &str, left_half_width: u16, right_half_width: u16) -> Line<'static> {
    let left_padded = pad_to_width(left, left_half_width as usize);
    let right_padded = pad_to_width(right, right_half_width as usize);
    Line::from(format!("  {left_padded} │ {right_padded}"))
}

/// Pad a string with spaces on the right to exactly `width` columns.
/// Truncation is by character count; the caller owns ensuring
/// individual cells are single-column glyphs (bar `█`/`░`, sparkline
/// glyphs, ASCII text).
fn pad_to_width(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        // truncate to width chars
        s.chars().take(width).collect()
    } else {
        let mut out = String::with_capacity(s.len() + (width - len));
        out.push_str(s);
        for _ in 0..(width - len) {
            out.push(' ');
        }
        out
    }
}

fn left_row(label: &str, value: Option<f32>, bar_cells: usize) -> String {
    match value {
        Some(v) => {
            let bar = render_pressure_bar(v, bar_cells);
            // Layout: "LABEL{2-space gap}{bar}  {pct}"
            // LABEL is left-padded to 6 chars so CTX/5H/7D/CACHE align.
            format!("{label:<6} {bar}  {pct:>4}", pct = pct(v))
        }
        None => format!("{label:<6} {DASH}"),
    }
}

fn right_reset_row(label: &'static str, eta_unix: Option<u64>) -> String {
    match eta_unix.and_then(crate::ui::panels::format_resets_eta) {
        Some(eta_fmt) => format!("{label:<11} ▸ {eta_fmt}"),
        None => format!("{label:<11} ▸ {DASH}"),
    }
}

fn right_tokens_row(
    label: &'static str,
    samples: &[TokenSample],
    field: impl Fn(&TokenSample) -> Option<u64>,
    right_half_width: u16,
) -> String {
    if samples.len() < 2 {
        return format!("{label:<11} {DASH}");
    }
    let mut sorted: Vec<&TokenSample> = samples.iter().collect();
    sorted.sort_by_key(|s| s.ts_unix_ms);
    let values: Vec<u64> = sorted.iter().filter_map(|s| field(s)).collect();
    if values.len() < 2 {
        return format!("{label:<11} {DASH}");
    }
    let deltas: Vec<u64> = values
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]))
        .collect();
    let max_delta = *deltas.iter().max().unwrap_or(&0);
    let glyph_count = SPARK_GLYPHS.chars().count();
    let current = *values.last().unwrap();
    let last_delta = *deltas.last().unwrap_or(&0);
    let current_fmt = crate::ui::labels::format_count_with_suffix(current);
    let delta_fmt = crate::ui::labels::format_count_with_suffix(last_delta);
    // Suffix: "  {current}  Δ+{delta}" — measured before sparkline so
    // the sparkline scales to fill the remaining budget.
    let suffix = format!("  {current_fmt}  Δ+{delta_fmt}");
    let label_field = format!("{label:<11} ");
    let cells = sparkline_cells(
        right_half_width,
        label_field.chars().count(),
        suffix.chars().count(),
    );
    let glyphs: String = if max_delta == 0 {
        SPARK_GLYPHS
            .chars()
            .next()
            .unwrap()
            .to_string()
            .repeat(cells)
    } else {
        sample_to_cells(&deltas, cells, |d| {
            let max = max_delta as f64;
            ((*d as f64 / max) * (glyph_count - 1) as f64).round() as usize
        })
    };
    format!("{label_field}{glyphs}{suffix}")
}

fn right_cost_row(samples: &[TokenSample], current: Option<f64>, right_half_width: u16) -> String {
    let curr = match current {
        Some(c) => c,
        None => return format!("{:<11} {}", "COST", DASH),
    };
    let mut sorted: Vec<&TokenSample> = samples.iter().collect();
    sorted.sort_by_key(|s| s.ts_unix_ms);
    let values: Vec<f64> = sorted.iter().filter_map(|s| s.cost_usd).collect();
    let trend = if values.len() < 2 {
        '─'
    } else {
        let last = values[values.len() - 1];
        let prev = values[values.len() - 2];
        if last > prev {
            '▲'
        } else if last < prev {
            '▼'
        } else {
            '─'
        }
    };
    let label_field = format!("{:<11} ", "COST");
    let suffix = format!("  ${curr:.2}  {trend}");
    let cells = sparkline_cells(
        right_half_width,
        label_field.chars().count(),
        suffix.chars().count(),
    );
    let glyph_count = SPARK_GLYPHS.chars().count();
    let glyphs: String = if values.len() < 2 {
        SPARK_GLYPHS
            .chars()
            .next()
            .unwrap()
            .to_string()
            .repeat(cells)
    } else {
        let max = values.iter().cloned().fold(0.0_f64, f64::max);
        if max == 0.0 {
            SPARK_GLYPHS
                .chars()
                .next()
                .unwrap()
                .to_string()
                .repeat(cells)
        } else {
            sample_to_cells(&values, cells, |v| {
                ((*v / max) * (glyph_count - 1) as f64).round() as usize
            })
        }
    };
    format!("{label_field}{glyphs}{suffix}")
}

/// Resample a series to exactly `cells` points, mapping each to a
/// SPARK_GLYPHS index via `idx_for(value)`. When `series.len() > cells`
/// it picks evenly-spaced indices; when shorter, it stretches the
/// existing points.
fn sample_to_cells<T>(series: &[T], cells: usize, idx_for: impl Fn(&T) -> usize) -> String {
    let glyph_count = SPARK_GLYPHS.chars().count();
    if cells == 0 || series.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(cells);
    for i in 0..cells {
        let pos = if cells == 1 {
            series.len() - 1
        } else {
            (i * (series.len() - 1)) / (cells - 1)
        };
        let raw_idx = idx_for(&series[pos]).min(glyph_count - 1);
        out.push(SPARK_GLYPHS.chars().nth(raw_idx).unwrap_or('▁'));
    }
    out
}

fn right_mem_row(s: &crate::domain::signal::SignalSet) -> String {
    let rss = s
        .process_memory_mb
        .as_ref()
        .map(|m| format!("{} MiB ─", m.value as i64));
    let agent = s
        .agent_memory_bytes
        .as_ref()
        .map(|m| crate::ui::panels::format_agent_memory_bytes(m.value));

    match (rss, agent) {
        (Some(rss), Some(af)) => format!("MEM   {rss}   MEM-FILE {af}"),
        (Some(rss), None) => format!("MEM   {rss}   MEM-FILE {DASH}"),
        (None, Some(af)) => format!("MEM   {DASH}   MEM-FILE {af}"),
        (None, None) => format!("MEM   {DASH}"),
    }
}

pub fn render_metrics_modal(
    frame: &mut Frame<'_>,
    overlay: &MetricsOverlay,
    target_label: &str,
    reports: &[PaneReport],
) {
    let rects = metrics_modal_rects(frame.area(), overlay.width_pct(), overlay.height_pct());
    frame.render_widget(Clear, rects.area);

    let block = Block::default()
        .title(format!(
            "Metrics · target {target_label} · {} panes · m close",
            reports.len()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));

    let lines = render_metrics_lines(overlay, target_label, reports, rects.body);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((overlay.scroll(), 0))
            .block(block)
            .wrap(Wrap { trim: false }),
        rects.area,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        crate::ui::dashboard::close_button_rect(rects.area),
    );
    frame.render_widget(
        Paragraph::new(
            "[ shrink · ] grow · = reset · ↑/↓ scroll · m close · Esc close · click [x] close",
        )
        .style(Style::default().fg(theme::TEXT_DIM)),
        rects.hint,
    );
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
    fn scroll_saturates_at_zero_and_max() {
        let mut o = MetricsOverlay::new();
        o.scroll_up();
        assert_eq!(o.scroll(), 0);
        o.scroll_down(2);
        o.scroll_down(2);
        o.scroll_down(2); // capped at 2
        assert_eq!(o.scroll(), 2);
        o.scroll_up();
        assert_eq!(o.scroll(), 1);
    }

    #[test]
    fn metrics_modal_rects_centered_95_by_90() {
        use ratatui::layout::Rect;
        let viewport = Rect::new(0, 0, 100, 50);
        let r = metrics_modal_rects(viewport, 95, 90);
        assert_eq!(r.area.width, 95);
        assert_eq!(r.area.height, 45);
        // area is centered: x ≈ (100-95)/2 = 2..3, y ≈ (50-45)/2 = 2..3
        // (ratatui's percentage solver may distribute rounding either way).
        assert!((2..=3).contains(&r.area.x), "x out of band: {}", r.area.x);
        assert!((2..=3).contains(&r.area.y), "y out of band: {}", r.area.y);
    }

    #[test]
    fn metrics_modal_rects_partition_sums_to_area() {
        use ratatui::layout::Rect;
        let viewport = Rect::new(0, 0, 120, 40);
        let r = metrics_modal_rects(viewport, 95, 90);
        let bottom = r.banner.height + r.body.height + r.hint.height;
        assert_eq!(bottom, r.area.height);
        assert_eq!(r.banner.y, r.area.y);
        assert_eq!(r.hint.y + r.hint.height, r.area.y + r.area.height);
    }

    #[test]
    fn grow_clamps_to_max() {
        let mut o = MetricsOverlay::new();
        // start at 95/90
        assert_eq!(o.width_pct(), 95);
        assert_eq!(o.height_pct(), 90);
        o.grow();
        // 95+5=100 → clamped to 99; 90+5=95
        assert_eq!(o.width_pct(), 99);
        assert_eq!(o.height_pct(), 95);
        o.grow();
        // 99 (already at max) stays; 95+5=100 → clamped to 99
        assert_eq!(o.width_pct(), 99);
        assert_eq!(o.height_pct(), 99);
        o.grow();
        // both saturated at 99
        assert_eq!(o.width_pct(), 99);
        assert_eq!(o.height_pct(), 99);
    }

    #[test]
    fn shrink_clamps_to_min() {
        let mut o = MetricsOverlay::new();
        for _ in 0..50 {
            o.shrink();
        }
        assert_eq!(o.width_pct(), MetricsOverlay::SIZE_MIN);
        assert_eq!(o.height_pct(), MetricsOverlay::SIZE_MIN);
    }

    #[test]
    fn reset_size_returns_to_defaults() {
        let mut o = MetricsOverlay::new();
        o.shrink();
        o.shrink();
        o.grow();
        // size has drifted; reset returns to 95/90
        o.reset_size();
        assert_eq!(o.width_pct(), 95);
        assert_eq!(o.height_pct(), 90);
    }

    #[test]
    fn open_does_not_reset_size() {
        let mut o = MetricsOverlay::new();
        o.grow();
        let w = o.width_pct();
        let h = o.height_pct();
        o.open();
        o.close();
        o.open();
        assert_eq!(o.width_pct(), w);
        assert_eq!(o.height_pct(), h);
    }

    #[test]
    fn hottest_banner_picks_max_pressure() {
        let panes = vec![
            report_with_pressure("claude:1:main", 0.56, None, None),
            report_with_pressure("codex:1:review", 0.71, Some(0.92), None),
            report_with_pressure("gemini:1:research", 0.31, None, None),
        ];
        let line = hottest_banner_line(&panes);
        let txt = line_to_string(&line);
        assert!(txt.contains("codex:1:review"), "got: {txt}");
        assert!(txt.contains("92%"), "got: {txt}");
    }

    #[test]
    fn hottest_banner_dim_when_no_pressure_data() {
        let panes = vec![report_no_pressure("claude:1:main")];
        let line = hottest_banner_line(&panes);
        let txt = line_to_string(&line);
        assert!(txt.contains("—"), "got: {txt}");
    }

    #[test]
    fn pane_card_includes_left_bars_and_right_timeseries() {
        use crate::domain::origin::SourceKind;
        use crate::domain::signal::MetricValue;
        use ratatui::layout::Rect;
        let mut rep = report_with_pressure("codex:1:review", 0.56, Some(0.47), None);
        rep.signals.cache_hit_ratio = Some(MetricValue::new(0.87, SourceKind::ProviderOfficial));
        rep.signals.cost_usd = Some(MetricValue::new(0.42, SourceKind::ProviderOfficial));
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        rep.signals.quota_5h_resets_at = Some(MetricValue::new(
            now_unix + 2 * 3600 + 13 * 60,
            SourceKind::ProviderOfficial,
        ));
        rep.signals.quota_weekly_resets_at = Some(MetricValue::new(
            now_unix + 4 * 24 * 3600 + 6 * 3600,
            SourceKind::ProviderOfficial,
        ));
        rep.recent_token_samples = vec![
            token_sample(1, Some(80_000), Some(8_000)),
            token_sample(2, Some(84_000), Some(8_400)),
        ];

        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &[rep], body);
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        // CTX bar + percent
        assert!(dump.contains("CTX"), "missing CTX label: {dump}");
        assert!(dump.contains("█"), "missing bar glyph: {dump}");
        assert!(dump.contains("56%"), "missing CTX pct: {dump}");
        // 5H bar + percent
        assert!(dump.contains("47%"), "missing 5H pct: {dump}");
        // CACHE bar + percent
        assert!(dump.contains("87%"), "missing CACHE pct: {dump}");
        // Right column: 5H reset eta text (no progress bar)
        assert!(dump.contains("5H reset"), "missing 5H reset row: {dump}");
        assert!(dump.contains("▸"), "missing reset arrow: {dump}");
        // Tokens sparklines
        assert!(dump.contains("TOKENS in"), "missing TOKENS in: {dump}");
        assert!(dump.contains("TOKENS out"), "missing TOKENS out: {dump}");
        // COST in right column
        assert!(dump.contains("COST"), "missing COST: {dump}");
        assert!(dump.contains("$0.42"), "missing cost current: {dump}");
    }

    #[test]
    fn pane_card_card_divider_starts_with_pane_label() {
        use ratatui::layout::Rect;
        let rep = report_with_pressure("claude:1:main", 0.40, None, None);
        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &[rep], body);
        // Find the divider: it begins with `━ claude:1:main`
        let divider = lines
            .iter()
            .find(|l| line_to_string(l).starts_with("━ claude:1:main"));
        assert!(
            divider.is_some(),
            "expected divider starting with `━ claude:1:main`, got:\n{}",
            lines
                .iter()
                .map(line_to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn bar_width_scales_to_left_half_budget() {
        // Narrow: 80-col viewport -> body width ~76, inner ~74,
        //   per-half ~35, bar 35-9-2-4 = 20 cells.
        // Wide: 200-col viewport -> bar saturates at BAR_MAX_CELLS (24).
        let narrow = bar_cells_for_left_half(35);
        let wide = bar_cells_for_left_half(80);
        assert!(
            narrow < BAR_MAX_CELLS,
            "narrow should be below max: {narrow}"
        );
        assert!(
            narrow >= BAR_MIN_CELLS,
            "narrow should be above min: {narrow}"
        );
        assert_eq!(wide, BAR_MAX_CELLS, "wide should saturate at max: {wide}");
        // Below min still floors to BAR_MIN_CELLS.
        let tiny = bar_cells_for_left_half(8);
        assert_eq!(tiny, BAR_MIN_CELLS);
    }

    #[test]
    fn em_dash_for_missing_left_metrics() {
        use ratatui::layout::Rect;
        // Pane with only CTX populated; 5H, 7D, CACHE absent → em-dash.
        let rep = report_with_pressure("g:1:r", 0.3, None, None);
        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &[rep], body);
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        // Em-dash should appear at least once for the missing rows.
        assert!(
            dump.contains("─"),
            "expected em-dash for missing rows: {dump}"
        );
    }

    #[test]
    fn render_metrics_lines_handles_empty_reports() {
        use ratatui::layout::Rect;
        let panes: Vec<crate::app::event_loop::PaneReport> = Vec::new();
        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &panes, body);
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(
            dump.contains("Hottest:") && dump.contains("—"),
            "expected dim banner with em-dash"
        );
    }

    #[test]
    fn pane_cards_pack_without_blank_lines_between() {
        use ratatui::layout::Rect;
        let panes = vec![
            report_with_pressure("claude:1:main", 0.40, None, None),
            report_with_pressure("codex:1:review", 0.70, Some(0.85), None),
            report_with_pressure("gemini:1:research", 0.21, None, None),
        ];
        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &panes, body);
        // Find any pair of consecutive purely-empty lines.
        let strs: Vec<String> = lines.iter().map(line_to_string).collect();
        for w in strs.windows(2) {
            assert!(
                !(w[0].trim().is_empty() && w[1].trim().is_empty()),
                "found two consecutive blank lines:\n{}",
                strs.join("\n")
            );
        }
        // There should be exactly 3 card dividers (one per pane).
        let divider_count = strs.iter().filter(|l| l.starts_with("━ ")).count();
        assert_eq!(
            divider_count,
            3,
            "expected one divider per pane; got:\n{}",
            strs.join("\n")
        );
    }

    #[test]
    fn render_metrics_lines_includes_banner_and_per_pane_cards() {
        use ratatui::layout::Rect;
        let panes = vec![
            report_with_pressure("claude:1:main", 0.40, None, None),
            report_with_pressure("codex:1:review", 0.70, Some(0.85), None),
        ];
        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &panes, body);
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(dump.contains("Hottest:"), "missing banner; got:\n{dump}");
        assert!(
            dump.contains("claude:1:main"),
            "missing claude pane card; got:\n{dump}"
        );
        assert!(
            dump.contains("codex:1:review"),
            "missing codex pane card; got:\n{dump}"
        );
        assert!(
            dump.contains("5H reset"),
            "missing 5H reset row; got:\n{dump}"
        );
        // No "Selected:" header in v2 layout.
        assert!(
            !dump.contains("Selected:"),
            "v2 layout should drop Selected header; got:\n{dump}"
        );
    }

    fn token_sample(ts: i64, in_t: Option<u64>, out_t: Option<u64>) -> crate::store::TokenSample {
        crate::store::TokenSample {
            ts_unix_ms: ts,
            pane_id: "%test".into(),
            provider: crate::domain::identity::Provider::Codex,
            input_tokens: in_t,
            output_tokens: out_t,
            cost_usd: None,
            cached_input_tokens: None,
        }
    }

    fn report_with_pressure(
        label: &str,
        ctx: f32,
        q5h: Option<f32>,
        q7d: Option<f32>,
    ) -> crate::app::event_loop::PaneReport {
        let mut rep = base_test_report(label);
        rep.signals.context_pressure = Some(metric_value(ctx));
        rep.signals.quota_5h_pressure = q5h.map(metric_value);
        rep.signals.quota_weekly_pressure = q7d.map(metric_value);
        rep
    }

    fn report_no_pressure(label: &str) -> crate::app::event_loop::PaneReport {
        base_test_report(label)
    }

    fn base_test_report(label: &str) -> crate::app::event_loop::PaneReport {
        // Parse "<provider>:<instance>:<role>" so the rendered pane
        // label matches the literal passed by the test author.
        let mut parts = label.split(':');
        let provider_str = parts.next().unwrap_or("claude");
        let instance: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        let role_str = parts.next().unwrap_or("main");

        let provider = match provider_str {
            "claude" => crate::domain::identity::Provider::Claude,
            "codex" => crate::domain::identity::Provider::Codex,
            "gemini" => crate::domain::identity::Provider::Gemini,
            "qmonster" => crate::domain::identity::Provider::Qmonster,
            _ => crate::domain::identity::Provider::Unknown,
        };
        let role = match role_str {
            "main" => crate::domain::identity::Role::Main,
            "review" => crate::domain::identity::Role::Review,
            "research" => crate::domain::identity::Role::Research,
            "monitor" => crate::domain::identity::Role::Monitor,
            "r" => crate::domain::identity::Role::Review,
            _ => crate::domain::identity::Role::Unknown,
        };

        crate::app::event_loop::PaneReport {
            pane_id: "%1".into(),
            session_name: provider_str.to_string(),
            window_index: instance.to_string(),
            provider,
            identity: crate::domain::identity::ResolvedIdentity {
                identity: crate::domain::identity::PaneIdentity {
                    provider,
                    instance,
                    role,
                    pane_id: "%1".into(),
                },
                confidence: crate::domain::identity::IdentityConfidence::High,
            },
            signals: crate::domain::signal::SignalSet::default(),
            recommendations: vec![],
            effects: vec![],
            dead: false,
            current_path: "/repo".into(),
            current_command: provider_str.to_string(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
            recent_token_samples: Vec::new(),
        }
    }

    fn metric_value(v: f32) -> crate::domain::signal::MetricValue<f32> {
        crate::domain::signal::MetricValue::new(
            v,
            crate::domain::origin::SourceKind::ProviderOfficial,
        )
    }

    fn line_to_string(line: &ratatui::text::Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }
}
