//! Phase C (v1.38) — Metrics Overlay state container.
//!
//! Pure-state struct with open/close/select APIs and clamping. No
//! rendering yet (Tasks 10-13 add layout, hottest banner, comparison
//! table, selected-pane detail, and Frame-based renderer). Tests
//! cover the state transitions in isolation so subsequent renderer
//! tasks can rely on a stable state contract.

use crate::app::event_loop::PaneReport;
use crate::store::TokenSample;
use crate::ui::dashboard::centered_rect;
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

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
    let beats = best.as_ref().map_or(true, |(v, _, _, _)| value > *v);
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

const BAR_CELLS: usize = 6;

fn render_pressure_bar(ratio: f32) -> String {
    let filled = ((ratio.clamp(0.0, 1.0) * BAR_CELLS as f32).round() as usize).min(BAR_CELLS);
    let mut s = String::with_capacity(BAR_CELLS);
    for i in 0..BAR_CELLS {
        s.push(if i < filled { '█' } else { '░' });
    }
    s
}

fn pct(v: f32) -> String {
    format!("{}%", (v * 100.0).round() as i32)
}

const DASH: &str = "─";

/// One row per pane in the comparison table. Columns: pane label, CTX
/// bar, CTX %, 5H bar, 5H %, 7D bar, 7D %, CACHE bar, CACHE %, COST.
/// Missing values render as em-dash dim. Spec §4.4.
pub fn comparison_row_line(report: &PaneReport) -> Line<'static> {
    let label = pane_label(report);
    let s = &report.signals;
    let mut spans = Vec::new();
    spans.push(Span::raw(format!("{label:<18} ")));

    push_pressure_cell(&mut spans, s.context_pressure.as_ref().map(|m| m.value));
    push_pressure_cell(&mut spans, s.quota_5h_pressure.as_ref().map(|m| m.value));
    push_pressure_cell(
        &mut spans,
        s.quota_weekly_pressure.as_ref().map(|m| m.value),
    );
    push_pressure_cell(
        &mut spans,
        s.cache_hit_ratio.as_ref().map(|m| m.value as f32),
    );
    push_cost_cell(&mut spans, s.cost_usd.as_ref().map(|m| m.value));

    Line::from(spans)
}

fn push_pressure_cell(spans: &mut Vec<Span<'static>>, value: Option<f32>) {
    match value {
        Some(v) => {
            spans.push(Span::raw(render_pressure_bar(v)));
            spans.push(Span::raw(format!(" {} ", pct(v))));
        }
        None => spans.push(Span::styled(
            format!("{:^11} ", DASH),
            Style::default().fg(theme::TEXT_DIM),
        )),
    }
}

fn push_cost_cell(spans: &mut Vec<Span<'static>>, value: Option<f64>) {
    match value {
        Some(v) => spans.push(Span::raw(format!("${v:.2} "))),
        None => spans.push(Span::styled(
            format!("{} ", DASH),
            Style::default().fg(theme::TEXT_DIM),
        )),
    }
}

const WINDOW_5H_SECS: f64 = 5.0 * 3600.0;
const WINDOW_7D_SECS: f64 = 7.0 * 86400.0;
const SPARK_GLYPHS: &str = "▁▂▃▄▅▆▇█";

/// Render the selected pane's detail block — token in/out sparklines,
/// cost trend, memory current values, and 5H/7D reset bars. Spec §4.5.
pub fn selected_detail_lines(report: &PaneReport) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // TOKENS in / out sparklines from recent_token_samples deltas
    if let Some(line) = token_sparkline_line("TOKENS in", &report.recent_token_samples, |s| {
        s.input_tokens
    }) {
        lines.push(line);
    }
    if let Some(line) = token_sparkline_line("TOKENS out", &report.recent_token_samples, |s| {
        s.output_tokens
    }) {
        lines.push(line);
    }
    // COST sparkline + current
    if let Some(line) = cost_sparkline_line(
        &report.recent_token_samples,
        report.signals.cost_usd.as_ref().map(|m| m.value),
    ) {
        lines.push(line);
    }
    // MEM RSS / MEM-FILE — current value
    if let Some(line) = mem_rss_line(&report.signals) {
        lines.push(line);
    }
    if let Some(line) = mem_file_line(&report.signals) {
        lines.push(line);
    }
    // 5H / 7D reset bars
    lines.push(reset_line(
        "5H reset",
        report.signals.quota_5h_resets_at.as_ref().map(|m| m.value),
        WINDOW_5H_SECS,
    ));
    lines.push(reset_line(
        "7D reset",
        report
            .signals
            .quota_weekly_resets_at
            .as_ref()
            .map(|m| m.value),
        WINDOW_7D_SECS,
    ));

    lines
}

fn token_sparkline_line(
    label: &'static str,
    samples: &[TokenSample],
    field: impl Fn(&TokenSample) -> Option<u64>,
) -> Option<Line<'static>> {
    if samples.len() < 2 {
        return None;
    }
    // Persisted callers store newest-first (`recent_samples` ORDER BY
    // ts_unix_ms DESC), so sort ascending for delta computation.
    let mut sorted: Vec<&TokenSample> = samples.iter().collect();
    sorted.sort_by_key(|s| s.ts_unix_ms);
    let values: Vec<u64> = sorted.iter().filter_map(|s| field(s)).collect();
    if values.len() < 2 {
        return None;
    }
    let deltas: Vec<u64> = values
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]))
        .collect();
    let max_delta = *deltas.iter().max().unwrap_or(&0);
    let glyph_count = SPARK_GLYPHS.chars().count();
    let spark: String = if max_delta == 0 {
        SPARK_GLYPHS
            .chars()
            .next()
            .unwrap()
            .to_string()
            .repeat(deltas.len())
    } else {
        deltas
            .iter()
            .map(|d| {
                let idx =
                    ((*d as f64 / max_delta as f64) * (glyph_count - 1) as f64).round() as usize;
                SPARK_GLYPHS.chars().nth(idx).unwrap_or('▁')
            })
            .collect()
    };
    let current = *values.last().unwrap();
    let last_delta = *deltas.last().unwrap_or(&0);
    Some(Line::from(format!(
        "{label:<11} {spark}  {current_fmt}  Δ+{delta_fmt}",
        current_fmt = crate::ui::labels::format_count_with_suffix(current),
        delta_fmt = crate::ui::labels::format_count_with_suffix(last_delta),
    )))
}

fn cost_sparkline_line(samples: &[TokenSample], current: Option<f64>) -> Option<Line<'static>> {
    let curr = current?;
    let mut sorted: Vec<&TokenSample> = samples.iter().collect();
    sorted.sort_by_key(|s| s.ts_unix_ms);
    let values: Vec<f64> = sorted.iter().filter_map(|s| s.cost_usd).collect();
    let glyph_count = SPARK_GLYPHS.chars().count();
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
    let spark: String = if values.len() < 2 {
        String::from(SPARK_GLYPHS.chars().next().unwrap())
    } else {
        let max = values.iter().cloned().fold(0.0_f64, f64::max);
        if max == 0.0 {
            SPARK_GLYPHS
                .chars()
                .next()
                .unwrap()
                .to_string()
                .repeat(values.len())
        } else {
            values
                .iter()
                .map(|v| {
                    let idx = ((v / max) * (glyph_count - 1) as f64).round() as usize;
                    SPARK_GLYPHS.chars().nth(idx).unwrap_or('▁')
                })
                .collect()
        }
    };
    Some(Line::from(format!(
        "COST $       {spark}  ${curr:.2}  {trend}"
    )))
}

fn mem_rss_line(s: &crate::domain::signal::SignalSet) -> Option<Line<'static>> {
    let m = s.process_memory_mb.as_ref()?;
    // TODO(v1.38.x): wire delta-vs-previous-poll source to render trend arrow + delta.
    // Spec §4.5 calls for `▲ +3 MiB` style; today we render value only with a
    // dim placeholder arrow ─ since no delta source exists yet.
    Some(Line::from(format!(
        "{:<11} {} MiB  ─",
        "MEM RSS", m.value as i64
    )))
}

fn mem_file_line(s: &crate::domain::signal::SignalSet) -> Option<Line<'static>> {
    let m = s.agent_memory_bytes.as_ref()?;
    let fmt = crate::ui::panels::format_agent_memory_bytes(m.value);
    // TODO(v1.38.x): same — delta arrow placeholder.
    Some(Line::from(format!("{:<11} {fmt}  ─", "MEM-FILE")))
}

fn reset_line(label: &'static str, eta_unix: Option<u64>, window_secs: f64) -> Line<'static> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match eta_unix {
        Some(eta) => {
            // `format_resets_eta` filters past timestamps and 14-day
            // sentinels; if it rejects, render em-dash. Otherwise
            // compute window-elapsed bar from the same `now`.
            match crate::ui::panels::format_resets_eta(eta) {
                Some(eta_fmt) => {
                    let remaining = eta.saturating_sub(now) as f64;
                    let bar_ratio = ((window_secs - remaining) / window_secs).clamp(0.0, 1.0);
                    let bar = render_pressure_bar(bar_ratio as f32);
                    Line::from(format!(
                        "{label:<11} ▸ {eta_fmt}  {bar} {pct}%",
                        pct = (bar_ratio * 100.0).round() as i32
                    ))
                }
                None => Line::from(Span::styled(
                    format!("{label:<11} ▸ —"),
                    Style::default().fg(theme::TEXT_DIM),
                )),
            }
        }
        None => Line::from(Span::styled(
            format!("{label:<11} ▸ —"),
            Style::default().fg(theme::TEXT_DIM),
        )),
    }
}

/// Pure helper that produces the modal body lines. Tested directly;
/// `render_metrics_modal` uses this to feed a `Paragraph`.
pub fn render_metrics_lines(
    overlay: &MetricsOverlay,
    _target_label: &str,
    reports: &[PaneReport],
    _body: Rect,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(hottest_banner_line(reports));
    out.push(Line::from(""));
    if !reports.is_empty() {
        out.push(comparison_header_line());
        for (i, r) in reports.iter().enumerate() {
            out.push(comparison_row_line_with_cursor(r, i == overlay.selected()));
        }
        out.push(separator_line());
        if let Some(sel) = reports.get(overlay.selected()) {
            out.push(selected_header_line(sel));
            out.extend(selected_detail_lines(sel));
        }
    }
    out
}

fn comparison_header_line() -> Line<'static> {
    Line::from(format!(
        "{:<18} {:<11} {:<11} {:<11} {:<11} {}",
        "Pane", "CTX", "5H", "7D", "CACHE", "COST"
    ))
}

fn comparison_row_line_with_cursor(report: &PaneReport, selected: bool) -> Line<'static> {
    let prefix = if selected { "▶ " } else { "  " };
    let mut line = comparison_row_line(report);
    line.spans.insert(0, Span::raw(prefix));
    line
}

fn separator_line() -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(60),
        Style::default().fg(theme::TEXT_DIM),
    ))
}

fn selected_header_line(report: &PaneReport) -> Line<'static> {
    Line::from(format!("Selected: {}", pane_label(report)))
}

pub fn render_metrics_modal(
    frame: &mut Frame<'_>,
    overlay: &MetricsOverlay,
    target_label: &str,
    reports: &[PaneReport],
) {
    let rects = metrics_modal_rects(frame.area());
    frame.render_widget(Clear, rects.area);

    let block = Block::default()
        .title(format!(
            "Metrics · target {target_label} · {} panes · m again to close",
            reports.len()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));

    let lines = render_metrics_lines(overlay, target_label, reports, rects.area);
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
        Paragraph::new("↑/↓ select pane · m close · Esc close · click [x] close")
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
    fn comparison_row_renders_em_dash_for_missing() {
        let row = comparison_row_line(&report_with_pressure("g:1:r", 0.3, None, None));
        let txt = line_to_string(&row);
        assert!(
            txt.contains("─"),
            "expected em-dash for missing 5h, got: {txt}"
        );
    }

    #[test]
    fn selected_detail_renders_token_sparklines_when_samples_present() {
        let mut rep = base_test_report("codex:1:review");
        rep.recent_token_samples = vec![
            token_sample(1, Some(1_000_000), Some(20_000)),
            token_sample(2, Some(1_500_000), Some(20_300)),
        ];
        let lines = selected_detail_lines(&rep);
        assert!(
            lines
                .iter()
                .any(|l| line_to_string(l).contains("TOKENS in"))
        );
        assert!(
            lines
                .iter()
                .any(|l| line_to_string(l).contains("TOKENS out"))
        );
    }

    #[test]
    fn selected_detail_renders_reset_bar_when_eta_present() {
        use crate::domain::origin::SourceKind;
        use crate::domain::signal::MetricValue;
        let mut rep = base_test_report("codex:1:review");
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        rep.signals.quota_5h_resets_at = Some(MetricValue::new(
            now_unix + 13 * 60,
            SourceKind::ProviderOfficial,
        ));
        let lines = selected_detail_lines(&rep);
        assert!(
            lines.iter().any(|l| {
                let t = line_to_string(l);
                t.contains("5H reset") && t.contains("█")
            }),
            "expected 5H reset bar; got: {:?}",
            lines.iter().map(line_to_string).collect::<Vec<_>>()
        );
    }

    #[test]
    fn selected_detail_em_dash_when_no_eta() {
        let rep = base_test_report("g:1:r");
        let lines = selected_detail_lines(&rep);
        assert!(
            lines
                .iter()
                .any(|l| line_to_string(l).contains("5H reset    ▸ —")),
            "expected literal `5H reset    ▸ —` prefix; got: {:?}",
            lines.iter().map(line_to_string).collect::<Vec<_>>()
        );
    }

    #[test]
    fn render_metrics_lines_includes_banner_comparison_and_detail() {
        use ratatui::layout::Rect;
        let panes = vec![
            report_with_pressure("claude:1:main", 0.40, None, None),
            report_with_pressure("codex:1:review", 0.70, Some(0.85), None),
        ];
        let mut overlay = MetricsOverlay::new();
        overlay.open();
        overlay.select_at(1, panes.len());
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &panes, body);
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(dump.contains("Hottest:"), "missing banner; got:\n{dump}");
        assert!(
            dump.contains("claude:1:main"),
            "missing comparison row; got:\n{dump}"
        );
        assert!(
            dump.contains("codex:1:review"),
            "missing comparison row; got:\n{dump}"
        );
        assert!(
            dump.contains("5H reset"),
            "missing selected detail reset row; got:\n{dump}"
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
