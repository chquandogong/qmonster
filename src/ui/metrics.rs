//! Phase C (v1.38) — Metrics Overlay state container.
//!
//! Pure-state struct with open/close/select APIs and clamping. No
//! rendering yet (Tasks 10-13 add layout, hottest banner, comparison
//! table, selected-pane detail, and Frame-based renderer). Tests
//! cover the state transitions in isolation so subsequent renderer
//! tasks can rely on a stable state contract.

use crate::app::event_loop::PaneReport;
use crate::ui::dashboard::centered_rect;
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

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
