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

use std::collections::HashMap;

use crate::app::event_loop::PaneReport;
use crate::store::TokenSample;
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// v1.38 polish: direction of the most recent MEM delta for one pane.
/// Drives the trend arrow rendered in the metrics overlay's COST·MEM
/// combined row. Computed in `update_mem_observation` from per-poll
/// observations stashed in the tracker map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemTrend {
    Up,
    Down,
    Steady,
}

impl MemTrend {
    pub fn arrow(self) -> &'static str {
        match self {
            MemTrend::Up => "▲",
            MemTrend::Down => "▼",
            MemTrend::Steady => "─",
        }
    }
}

/// One pane's last-poll MEM values plus the trend computed against
/// the previous observation. The first call seeds values with `None`
/// trends; the second and subsequent calls replace stored values and
/// emit Up/Down/Steady based on the delta. Owned by `tui_loop` so the
/// renderer stays a pure function of state.
#[derive(Debug, Clone, Default)]
pub struct MemObservation {
    pub rss_mb: Option<f64>,
    pub agent_bytes: Option<u64>,
    pub rss_trend: Option<MemTrend>,
    pub agent_trend: Option<MemTrend>,
}

/// Update the per-pane MEM tracker with the current poll's RSS and
/// agent-bytes values. Compares against the previously-stored
/// observation, sets `rss_trend`/`agent_trend`, and replaces the
/// stored values for the next poll's comparison.
///
/// RSS uses a 1.0 MiB threshold for `Steady` so normal allocator
/// jitter doesn't flicker ▲/▼ between polls. Agent bytes are summed
/// integers from disk-stat, so exact equality is the right
/// `Steady` predicate.
///
/// First observation has no prior to compare against — both trends
/// are `None` and the renderer falls back to the dim `─` glyph.
pub fn update_mem_observation(
    map: &mut HashMap<String, MemObservation>,
    pane_id: &str,
    current_rss_mb: Option<f64>,
    current_agent_bytes: Option<u64>,
) {
    let prev = map.get(pane_id).cloned().unwrap_or_default();
    let rss_trend = match (prev.rss_mb, current_rss_mb) {
        (Some(p), Some(c)) if (c - p).abs() < 1.0 => Some(MemTrend::Steady),
        (Some(p), Some(c)) if c > p => Some(MemTrend::Up),
        (Some(p), Some(c)) if c < p => Some(MemTrend::Down),
        _ => None,
    };
    let agent_trend = match (prev.agent_bytes, current_agent_bytes) {
        (Some(p), Some(c)) if c == p => Some(MemTrend::Steady),
        (Some(p), Some(c)) if c > p => Some(MemTrend::Up),
        (Some(p), Some(c)) if c < p => Some(MemTrend::Down),
        _ => None,
    };
    map.insert(
        pane_id.to_string(),
        MemObservation {
            rss_mb: current_rss_mb,
            agent_bytes: current_agent_bytes,
            rss_trend,
            agent_trend,
        },
    );
}

pub type DragAnchor = crate::ui::modal_chrome::DragAnchor;

#[derive(Debug, Clone)]
pub struct MetricsOverlay {
    open: bool,
    scroll: u16,
    geometry: crate::ui::modal_chrome::ModalGeometry,
}

impl Default for MetricsOverlay {
    fn default() -> Self {
        Self {
            open: false,
            scroll: 0,
            geometry: crate::ui::modal_chrome::ModalGeometry::new(
                Self::DEFAULT_WIDTH_PCT,
                Self::DEFAULT_HEIGHT_PCT,
                Self::SIZE_MIN,
                Self::SIZE_MAX,
                Self::SIZE_STEP,
            ),
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
        self.geometry.end_drag();
        // offset_x / offset_y intentionally preserved (mirrors size persistence)
    }

    pub fn close(&mut self) {
        self.open = false;
        self.scroll = 0;
        self.geometry.end_drag();
        // offset_x / offset_y preserved for next open
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

    pub fn set_offset(&mut self, x: i16, y: i16) {
        self.geometry.set_offset(x, y);
    }

    pub fn begin_drag(&mut self, anchor: DragAnchor) {
        self.geometry.begin_drag(anchor);
    }

    pub fn end_drag(&mut self) {
        self.geometry.end_drag();
    }

    pub fn grow(&mut self) {
        self.geometry.grow();
    }

    pub fn shrink(&mut self) {
        self.geometry.shrink();
    }

    pub fn reset_size(&mut self) {
        self.geometry.reset();
    }
}

pub struct MetricsModalRects {
    pub area: Rect,
    pub banner: Rect,
    pub body: Rect,
    pub hint: Rect,
}

pub fn metrics_modal_rects(
    viewport: Rect,
    width_pct: u16,
    height_pct: u16,
    offset_x: i16,
    offset_y: i16,
) -> MetricsModalRects {
    let area =
        crate::ui::modal_chrome::modal_area(viewport, width_pct, height_pct, offset_x, offset_y);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // banner
            Constraint::Min(6),    // body
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

/// Bar kind controls the filled-cell color:
/// - `Pressure`: filled cells take a severity color derived from the
///   value via `severity_for_ratio` (CTX/5H/7D pressure metrics).
/// - `Neutral`: filled cells use `theme::TEXT_PRIMARY` (CACHE hit
///   ratio is informational, so coloring it red at low values would
///   be misleading).
#[derive(Clone, Copy, Debug)]
enum BarKind {
    Pressure,
    Neutral,
}

/// Mirror of `panels::context_metric_severity` — kept local so the
/// metrics overlay can derive a `Severity` from a 0..1 pressure ratio
/// without crossing the panels visibility boundary.
fn severity_for_ratio(value: f32) -> crate::domain::recommendation::Severity {
    use crate::domain::recommendation::Severity;
    if value >= 0.85 {
        Severity::Risk
    } else if value >= 0.75 {
        Severity::Warning
    } else if value >= 0.60 {
        Severity::Concern
    } else {
        Severity::Good
    }
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
/// per-pane cards, each card = card-divider line + 4 content rows
/// (`<left half> │ <right half>`). Card height matches the left
/// column's metric count exactly (CTX/5H/7D/CACHE), so there are no
/// blank rows below CACHE — the right column packs reset etas and
/// COST/MEM into combined rows to fit the same 4-row budget.
pub fn render_metrics_lines(
    _overlay: &MetricsOverlay,
    _target_label: &str,
    reports: &[PaneReport],
    body: Rect,
    mem_observations: &HashMap<String, MemObservation>,
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
    // Two halves separated by " │ " (3 chars). The left column has a
    // fixed maximum useful width (label 6 + 3 spaces + bar 24 + 2 gap
    // + pct 4 + small margin 2 = ~41); when the modal is wide enough
    // we cap the left half at that and hand the rest to the right
    // column so sparklines and combined rows breathe. Narrow modals
    // fall back to the even split.
    let separator_len: u16 = 3;
    let halves_budget = inner_width.saturating_sub(separator_len);
    let left_content_width: u16 = 6 + 3 + (BAR_MAX_CELLS as u16) + 2 + 4 + 2;
    let (left_half_width, right_half_width) = if halves_budget >= left_content_width {
        let l = left_content_width;
        (l, halves_budget - l)
    } else {
        let l = halves_budget / 2;
        (l, halves_budget - l)
    };

    for r in reports {
        out.push(card_divider_line(r, inner_width));
        out.extend(card_rows(
            r,
            left_half_width,
            right_half_width,
            mem_observations,
        ));
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
    mem_observations: &HashMap<String, MemObservation>,
) -> Vec<Line<'static>> {
    let bar_cells = bar_cells_for_left_half(left_half_width);
    let s = &report.signals;

    let left_rows: [Vec<Span<'static>>; 4] = [
        left_row(
            "CTX",
            s.context_pressure.as_ref().map(|m| m.value),
            bar_cells,
            BarKind::Pressure,
        ),
        quota_left_row(s, bar_cells, true),
        quota_left_row(s, bar_cells, false),
        left_row(
            "CACHE",
            s.cache_hit_ratio.as_ref().map(|m| m.value as f32),
            bar_cells,
            BarKind::Neutral,
        ),
    ];

    let right_rows = [
        right_combined_reset_row(s),
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
        right_cost_mem_combined_row(
            &report.recent_token_samples,
            s.cost_usd.as_ref().map(|m| m.value),
            s,
            right_half_width,
            mem_observations
                .get(&report.pane_id)
                .cloned()
                .unwrap_or_default(),
        ),
    ];

    let mut lines = Vec::with_capacity(left_rows.len() + 1);
    for (l, r) in left_rows.iter().zip(right_rows.iter()) {
        lines.push(card_row(l, r, left_half_width, right_half_width));
    }

    if !report.anomalies.is_empty() {
        let count = report.anomalies.len();
        let pieces: Vec<String> = report
            .anomalies
            .iter()
            .map(|a| format!("{}:{}", a.kind.label(), a.confidence.label()))
            .collect();
        let summary = format!("ANOMALIES {} {}", count, pieces.join(", "));
        lines.push(Line::from(Span::styled(
            summary,
            Style::default().fg(theme::severity_color(
                crate::domain::recommendation::Severity::Concern,
            )),
        )));
    }

    lines
}

fn card_row(
    left: &[Span<'static>],
    right: &str,
    left_half_width: u16,
    right_half_width: u16,
) -> Line<'static> {
    // Compose the left half from styled spans, then pad with raw
    // spaces out to `left_half_width` so the separator and right half
    // align identically across rows. The total emitted character
    // count is `left_half_width + 3 (' │ ') + right_half_width`,
    // i.e. exactly `inner_width` — preserving the no-overflow contract
    // from db9bda3.
    let left_width: usize = left.iter().map(|s| s.content.chars().count()).sum();
    let pad_amount = (left_half_width as usize).saturating_sub(left_width);
    let mut spans: Vec<Span<'static>> = left.to_vec();
    if pad_amount > 0 {
        spans.push(Span::raw(" ".repeat(pad_amount)));
    }
    spans.push(Span::raw(" │ "));
    spans.push(Span::raw(pad_to_width(right, right_half_width as usize)));
    Line::from(spans)
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

fn left_row(
    label: &str,
    value: Option<f32>,
    bar_cells: usize,
    kind: BarKind,
) -> Vec<Span<'static>> {
    // Prefix: "LABEL  " — LABEL left-padded to 6 chars + a 1-space
    // gap matches the prior single-space layout in the `String` body
    // ("{label:<6} {bar}  {pct:>4}"). The post-bar gap is 2 spaces;
    // both are emitted as plain `Span::raw` so widths are stable.
    let prefix = format!("{label:<6} ");
    let mut spans: Vec<Span<'static>> = vec![Span::raw(prefix)];
    match value {
        Some(v) => {
            let filled = ((v.clamp(0.0, 1.0) * bar_cells as f32).round() as usize).min(bar_cells);
            let unfilled = bar_cells - filled;
            let filled_color = match kind {
                BarKind::Pressure => theme::severity_color(severity_for_ratio(v)),
                BarKind::Neutral => theme::TEXT_PRIMARY,
            };
            if filled > 0 {
                spans.push(Span::styled(
                    "█".repeat(filled),
                    Style::default().fg(filled_color),
                ));
            }
            if unfilled > 0 {
                spans.push(Span::styled(
                    "░".repeat(unfilled),
                    Style::default().fg(theme::TEXT_DIM),
                ));
            }
            spans.push(Span::raw(format!("  {:>4}", pct(v))));
        }
        None => {
            spans.push(Span::styled(
                DASH.to_string(),
                Style::default().fg(theme::TEXT_DIM),
            ));
        }
    }
    spans
}

/// Combined reset-eta row: `5H reset ▸ <eta>  ·  7D reset ▸ <eta>`.
/// If only one eta is present, render just that one (no dangling
/// separator). If neither split eta is present, fall back to the
/// Gemini `RuntimeFactKind::ModelReset` runtime fact (provider-rendered
/// `<model> <time/remaining>` text). If nothing is available, render a
/// dim em-dash.
fn right_combined_reset_row(s: &crate::domain::signal::SignalSet) -> String {
    let part_5h = s
        .quota_5h_resets_at
        .as_ref()
        .map(|m| m.value)
        .and_then(crate::ui::panels::format_resets_eta)
        .map(|eta| format!("5H reset ▸ {eta}"));
    let part_7d = s
        .quota_weekly_resets_at
        .as_ref()
        .map(|m| m.value)
        .and_then(crate::ui::panels::format_resets_eta)
        .map(|eta| format!("7D reset ▸ {eta}"));
    match (part_5h, part_7d) {
        (Some(a), Some(b)) => format!("{a}  ·  {b}"),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => {
            // Gemini fallback: ModelReset runtime fact carries
            // provider-rendered "<model> <time/remaining>" text.
            for fact in &s.runtime_facts {
                if fact.kind == crate::domain::signal::RuntimeFactKind::ModelReset {
                    return format!("RESET ▸ {}", fact.value);
                }
            }
            DASH.to_string()
        }
    }
}

/// Pick the right quota label/value for the left column based on which
/// quota fields the provider populates:
/// - Claude/Codex (split): 5H in primary slot, 7D in secondary.
/// - Gemini (single): QUOTA in primary slot, blank in secondary.
/// - None populated: ─ em-dash row in both slots (preserves the
///   4-row card height regardless of provider shape).
fn quota_left_row(
    s: &crate::domain::signal::SignalSet,
    bar_cells: usize,
    primary: bool,
) -> Vec<Span<'static>> {
    let split_5h = s.quota_5h_pressure.as_ref().map(|m| m.value);
    let split_weekly = s.quota_weekly_pressure.as_ref().map(|m| m.value);
    let single = s.quota_pressure.as_ref().map(|m| m.value);

    if split_5h.is_some() || split_weekly.is_some() {
        if primary {
            left_row("5H", split_5h, bar_cells, BarKind::Pressure)
        } else {
            left_row("7D", split_weekly, bar_cells, BarKind::Pressure)
        }
    } else if single.is_some() {
        if primary {
            left_row("QUOTA", single, bar_cells, BarKind::Pressure)
        } else {
            left_row_placeholder()
        }
    } else if primary {
        left_row("5H", None, bar_cells, BarKind::Pressure)
    } else {
        left_row("7D", None, bar_cells, BarKind::Pressure)
    }
}

/// Empty placeholder row for the secondary quota slot when only single
/// quota is populated. `card_row` pads the left half to
/// `left_half_width`, so an empty span vector is sufficient — the
/// 4-row card height stays consistent across providers.
fn left_row_placeholder() -> Vec<Span<'static>> {
    Vec::new()
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

/// Combined COST + MEM + MEM-FILE row. COST renders sparkline, value
/// and trend arrow (sparkline width is reduced because the row is
/// shared). MEM renders RSS as `<n> MiB <arrow>` where `<arrow>` is
/// driven by the per-pane `MemObservation` tracker (▲/▼/─ from the
/// last-poll delta; ─ until the second observation arrives). MEM-FILE
/// renders the agent memory byte count followed by the same per-pane
/// trend arrow. Sub-items are separated by ` · `; a missing sub-item
/// also drops its preceding separator so there are no dangling dots.
/// If all three are missing the row falls back to a dim em-dash.
fn right_cost_mem_combined_row(
    samples: &[TokenSample],
    cost_current: Option<f64>,
    s: &crate::domain::signal::SignalSet,
    right_half_width: u16,
    obs: MemObservation,
) -> String {
    let rss_arrow = obs.rss_trend.map(|t| t.arrow()).unwrap_or("─");
    let agent_arrow = obs.agent_trend.map(|t| t.arrow()).unwrap_or("─");

    let mem_part = s
        .process_memory_mb
        .as_ref()
        .map(|m| format!("MEM {} MiB {}", m.value as i64, rss_arrow));
    let mem_file_part = s.agent_memory_bytes.as_ref().map(|m| {
        format!(
            "MEM-FILE {} {}",
            crate::ui::panels::format_agent_memory_bytes(m.value),
            agent_arrow
        )
    });

    let cost_part = cost_current.map(|curr| {
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
        // Suffix: " $0.42 ▲" — single spaces (tighter than today's
        // double-space) so the shared row stays compact.
        let label = "COST ";
        let suffix = format!(" ${curr:.2} {trend}");
        // Reserve room for the rest of the row (separator + MEM
        // sub-items) so the COST sparkline shares the budget.
        let mut tail_reserved = 0usize;
        if let Some(p) = &mem_part {
            tail_reserved += " · ".chars().count() + p.chars().count();
        }
        if let Some(p) = &mem_file_part {
            tail_reserved += " · ".chars().count() + p.chars().count();
        }
        let label_len = label.chars().count();
        let suffix_len = suffix.chars().count();
        // sparkline_cells subtracts label, suffix, and a 4-char
        // padding from right_half_width; bake the trailing MEM
        // budget into the suffix length so the helper subtracts it
        // too. Clamp to [4, 24] per spec.
        let cells_raw =
            sparkline_cells(right_half_width, label_len, suffix_len + tail_reserved) as i32;
        let cells = cells_raw.clamp(4, 24) as usize;

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
        format!("{label}{glyphs}{suffix}")
    });

    let mut parts: Vec<String> = Vec::with_capacity(3);
    if let Some(p) = cost_part {
        parts.push(p);
    }
    if let Some(p) = mem_part {
        parts.push(p);
    }
    if let Some(p) = mem_file_part {
        parts.push(p);
    }
    if parts.is_empty() {
        DASH.to_string()
    } else {
        parts.join(" · ")
    }
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

pub fn render_metrics_modal(
    frame: &mut Frame<'_>,
    overlay: &MetricsOverlay,
    target_label: &str,
    reports: &[PaneReport],
    mem_observations: &HashMap<String, MemObservation>,
) {
    let rects = metrics_modal_rects(
        frame.area(),
        overlay.width_pct(),
        overlay.height_pct(),
        overlay.offset_x(),
        overlay.offset_y(),
    );
    frame.render_widget(Clear, rects.area);

    let block = Block::default()
        .title(format!(
            "Metrics · target {target_label} · {} panes · m close",
            reports.len()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));

    let lines = render_metrics_lines(overlay, target_label, reports, rects.body, mem_observations);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((overlay.scroll(), 0))
            .block(block)
            .wrap(Wrap { trim: false }),
        rects.area,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
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
        let r = metrics_modal_rects(viewport, 95, 90, 0, 0);
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
        let r = metrics_modal_rects(viewport, 95, 90, 0, 0);
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
        rep.signals.process_memory_mb = Some(MetricValue::new(312.0, SourceKind::ProviderOfficial));
        rep.recent_token_samples = vec![
            token_sample(1, Some(80_000), Some(8_000)),
            token_sample(2, Some(84_000), Some(8_400)),
        ];

        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &[rep], body, &HashMap::new());
        let strs: Vec<String> = lines.iter().map(line_to_string).collect();
        let dump: String = strs.iter().map(|s| format!("{s}\n")).collect();

        // 1 banner + 1 divider + 4 content rows = 6 total lines.
        assert_eq!(
            lines.len(),
            6,
            "card should render exactly 4 content rows + 1 divider after the banner; got:\n{dump}"
        );

        // CTX bar + percent
        assert!(dump.contains("CTX"), "missing CTX label: {dump}");
        assert!(dump.contains("█"), "missing bar glyph: {dump}");
        assert!(dump.contains("56%"), "missing CTX pct: {dump}");
        // 5H bar + percent
        assert!(dump.contains("47%"), "missing 5H pct: {dump}");
        // CACHE bar + percent
        assert!(dump.contains("87%"), "missing CACHE pct: {dump}");

        // Locate the per-pane card content rows (skip banner + divider).
        let row1 = &strs[2]; // combined reset row
        let row4 = &strs[5]; // combined COST · MEM · MEM-FILE row

        // Row 1: combined reset etas — both 5H reset and 7D reset present.
        assert!(
            row1.contains("5H reset") && row1.contains("7D reset"),
            "row 1 should combine 5H+7D resets; got: {row1}"
        );
        assert!(row1.contains("▸"), "row 1 missing reset arrow: {row1}");

        // Tokens sparklines on rows 2 and 3.
        assert!(strs[3].contains("TOKENS in"), "missing TOKENS in: {dump}");
        assert!(strs[4].contains("TOKENS out"), "missing TOKENS out: {dump}");

        // Row 4: COST + MEM combined.
        assert!(
            row4.contains("COST") && row4.contains("MEM"),
            "row 4 should combine COST and MEM; got: {row4}"
        );
        assert!(row4.contains("$0.42"), "missing cost current: {row4}");
        assert!(row4.contains("312"), "missing MEM RSS value: {row4}");
    }

    #[test]
    fn pane_card_card_divider_starts_with_pane_label() {
        use ratatui::layout::Rect;
        let rep = report_with_pressure("claude:1:main", 0.40, None, None);
        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &[rep], body, &HashMap::new());
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
        let lines = render_metrics_lines(&overlay, "qmonster:0", &[rep], body, &HashMap::new());
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        // Em-dash should appear at least once for the missing rows.
        assert!(
            dump.contains("─"),
            "expected em-dash for missing rows: {dump}"
        );
    }

    #[test]
    fn right_column_combined_reset_row_drops_separator_when_one_missing() {
        use crate::domain::origin::SourceKind;
        use crate::domain::signal::MetricValue;
        use ratatui::layout::Rect;
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut rep = report_with_pressure("codex:1:review", 0.5, None, None);
        // Only 5H reset eta — no 7D.
        rep.signals.quota_5h_resets_at = Some(MetricValue::new(
            now_unix + 90 * 60,
            SourceKind::ProviderOfficial,
        ));

        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &[rep], body, &HashMap::new());
        let strs: Vec<String> = lines.iter().map(line_to_string).collect();
        let row1 = &strs[2];

        assert!(
            row1.contains("5H reset"),
            "row 1 should still show 5H reset: {row1}"
        );
        assert!(
            !row1.contains("7D reset"),
            "row 1 should not mention 7D reset when 7D eta is missing: {row1}"
        );
        assert!(
            !row1.contains("·"),
            "row 1 should drop the ` · ` separator when only one reset is present: {row1}"
        );
    }

    #[test]
    fn right_column_combined_cost_mem_row_drops_separator_when_only_cost() {
        use crate::domain::origin::SourceKind;
        use crate::domain::signal::MetricValue;
        use ratatui::layout::Rect;
        let mut rep = report_with_pressure("codex:1:review", 0.5, None, None);
        // Only COST present — no MEM RSS, no MEM-FILE.
        rep.signals.cost_usd = Some(MetricValue::new(0.42, SourceKind::ProviderOfficial));
        rep.recent_token_samples = vec![
            token_sample(1, Some(1_000), Some(100)),
            token_sample(2, Some(1_200), Some(120)),
        ];

        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &[rep], body, &HashMap::new());
        let strs: Vec<String> = lines.iter().map(line_to_string).collect();
        let row4 = &strs[5];

        assert!(row4.contains("COST"), "row 4 should show COST: {row4}");
        assert!(
            row4.contains("$0.42"),
            "row 4 should show cost value: {row4}"
        );
        assert!(
            !row4.contains("MEM"),
            "row 4 should not mention MEM/MEM-FILE when both are missing: {row4}"
        );
        assert!(
            !row4.contains("·"),
            "row 4 should drop the ` · ` separator when only COST is present: {row4}"
        );
    }

    #[test]
    fn render_metrics_lines_handles_empty_reports() {
        use ratatui::layout::Rect;
        let panes: Vec<crate::app::event_loop::PaneReport> = Vec::new();
        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &panes, body, &HashMap::new());
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
        let lines = render_metrics_lines(&overlay, "qmonster:0", &panes, body, &HashMap::new());
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
    fn card_row_does_not_overflow_inner_width() {
        use ratatui::layout::Rect;
        let body = Rect::new(0, 0, 100, 30);
        let mut rep = base_test_report("test:1:main");
        rep.signals.context_pressure = Some(crate::domain::signal::MetricValue::new(
            0.5,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            body,
            &HashMap::new(),
        );
        let inner_width = (body.width - 2) as usize;
        for (i, line) in lines.iter().enumerate() {
            let len: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(
                len <= inner_width,
                "line {i} has {len} chars > inner_width {inner_width}: {:?}",
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn pressure_bar_filled_cells_use_severity_color() {
        // Build a fixture with CTX = 0.95 (Risk threshold). Render a
        // single pane card. Find the CTX line. Assert that at least
        // one Span in that line has fg == severity_color(Risk).
        use ratatui::layout::Rect;
        let body = Rect::new(0, 0, 100, 30);
        let mut rep = base_test_report("test:1:main");
        rep.signals.context_pressure = Some(crate::domain::signal::MetricValue::new(
            0.95,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            body,
            &HashMap::new(),
        );
        let ctx_line = &lines[2]; // line 2 is CTX (0=banner, 1=divider)
        let risk_color = theme::severity_color(crate::domain::recommendation::Severity::Risk);
        let any_risk_fg = ctx_line
            .spans
            .iter()
            .any(|s| s.style.fg == Some(risk_color));
        assert!(
            any_risk_fg,
            "CTX bar at 95% should have at least one span with Risk fg; got: {:?}",
            ctx_line
                .spans
                .iter()
                .map(|s| (s.content.as_ref(), s.style.fg))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn pressure_bar_unfilled_cells_use_dim_color() {
        use ratatui::layout::Rect;
        let body = Rect::new(0, 0, 100, 30);
        let mut rep = base_test_report("test:1:main");
        rep.signals.context_pressure = Some(crate::domain::signal::MetricValue::new(
            0.10,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            body,
            &HashMap::new(),
        );
        let ctx_line = &lines[2];
        let any_dim = ctx_line
            .spans
            .iter()
            .any(|s| s.style.fg == Some(theme::TEXT_DIM) && s.content.contains('░'));
        assert!(
            any_dim,
            "CTX bar at 10% should have a span with dim fg on '░' chars; got: {:?}",
            ctx_line
                .spans
                .iter()
                .map(|s| (s.content.as_ref(), s.style.fg))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn cache_bar_uses_neutral_not_severity_color() {
        use ratatui::layout::Rect;
        let body = Rect::new(0, 0, 100, 30);
        let mut rep = base_test_report("test:1:main");
        rep.signals.cache_hit_ratio = Some(crate::domain::signal::MetricValue::new(
            0.30,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            body,
            &HashMap::new(),
        );
        // CACHE is row 3 of the card; lines[2..6] are CTX/5H/7D/CACHE.
        let cache_line = &lines[5];
        let neutral = theme::TEXT_PRIMARY;
        let has_neutral_filled = cache_line
            .spans
            .iter()
            .any(|s| s.style.fg == Some(neutral) && s.content.contains('█'));
        assert!(
            has_neutral_filled,
            "CACHE bar should have neutral-colored filled cells, not severity; got: {:?}",
            cache_line
                .spans
                .iter()
                .map(|s| (s.content.as_ref(), s.style.fg))
                .collect::<Vec<_>>()
        );
        // Also assert it does NOT use severity Risk color even though
        // 30% would normally fall below the Risk threshold anyway —
        // belt-and-suspenders check that BarKind::Neutral routes
        // around `severity_for_ratio` entirely.
        let risk = theme::severity_color(crate::domain::recommendation::Severity::Risk);
        let has_risk = cache_line.spans.iter().any(|s| s.style.fg == Some(risk));
        assert!(!has_risk, "CACHE bar must not use severity color");
    }

    #[test]
    fn render_metrics_lines_includes_banner_and_per_pane_cards() {
        use crate::domain::origin::SourceKind;
        use crate::domain::signal::MetricValue;
        use ratatui::layout::Rect;
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut codex = report_with_pressure("codex:1:review", 0.70, Some(0.85), None);
        codex.signals.quota_5h_resets_at = Some(MetricValue::new(
            now_unix + 90 * 60,
            SourceKind::ProviderOfficial,
        ));
        let panes = vec![
            report_with_pressure("claude:1:main", 0.40, None, None),
            codex,
        ];
        let overlay = MetricsOverlay::new();
        let body = Rect::new(0, 0, 120, 30);
        let lines = render_metrics_lines(&overlay, "qmonster:0", &panes, body, &HashMap::new());
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

    #[test]
    fn update_mem_observation_first_call_has_no_trend() {
        let mut map: HashMap<String, MemObservation> = HashMap::new();
        update_mem_observation(&mut map, "%1", Some(100.0), Some(40_000));
        let obs = map.get("%1").unwrap();
        assert!(
            obs.rss_trend.is_none(),
            "first observation has no prev to compare"
        );
        assert!(obs.agent_trend.is_none());
        assert_eq!(obs.rss_mb, Some(100.0));
        assert_eq!(obs.agent_bytes, Some(40_000));
    }

    #[test]
    fn update_mem_observation_second_call_computes_trend() {
        let mut map: HashMap<String, MemObservation> = HashMap::new();
        update_mem_observation(&mut map, "%1", Some(100.0), Some(40_000));
        update_mem_observation(&mut map, "%1", Some(110.0), Some(40_000));
        let obs = map.get("%1").unwrap();
        assert_eq!(obs.rss_trend, Some(MemTrend::Up));
        assert_eq!(obs.agent_trend, Some(MemTrend::Steady));
    }

    #[test]
    fn update_mem_observation_steady_threshold_for_rss() {
        let mut map: HashMap<String, MemObservation> = HashMap::new();
        update_mem_observation(&mut map, "%1", Some(100.0), None);
        // delta of 0.5 MiB is below the 1.0 MiB jitter threshold
        update_mem_observation(&mut map, "%1", Some(100.5), None);
        let obs = map.get("%1").unwrap();
        assert_eq!(obs.rss_trend, Some(MemTrend::Steady));
    }

    #[test]
    fn update_mem_observation_detects_down_trend() {
        let mut map: HashMap<String, MemObservation> = HashMap::new();
        update_mem_observation(&mut map, "%1", Some(200.0), Some(80_000));
        update_mem_observation(&mut map, "%1", Some(150.0), Some(70_000));
        let obs = map.get("%1").unwrap();
        assert_eq!(obs.rss_trend, Some(MemTrend::Down));
        assert_eq!(obs.agent_trend, Some(MemTrend::Down));
    }

    #[test]
    fn mem_trend_arrow_glyphs() {
        assert_eq!(MemTrend::Up.arrow(), "▲");
        assert_eq!(MemTrend::Down.arrow(), "▼");
        assert_eq!(MemTrend::Steady.arrow(), "─");
    }

    #[test]
    fn render_metrics_lines_uses_real_mem_trend_when_available() {
        // Build a fixture pane with current RSS = 110 and a tracker
        // that has a previous observation of RSS = 100. Render and
        // assert the COST·MEM row contains "▲" for RSS.
        use ratatui::layout::Rect;
        let mut rep = base_test_report("test:1:main");
        rep.signals.process_memory_mb = Some(crate::domain::signal::MetricValue::new(
            110.0,
            crate::domain::origin::SourceKind::Heuristic,
        ));
        let mut map: HashMap<String, MemObservation> = HashMap::new();
        map.insert(
            rep.pane_id.clone(),
            MemObservation {
                rss_mb: Some(100.0),
                agent_bytes: None,
                rss_trend: Some(MemTrend::Up),
                agent_trend: None,
            },
        );
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            Rect::new(0, 0, 120, 30),
            &map,
        );
        // CACHE row's right side carries the COST·MEM combined row
        // (banner=0, divider=1, content rows 2..6 → COST·MEM = 5).
        let cost_mem_line = &lines[5];
        let dump: String = cost_mem_line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            dump.contains("▲"),
            "expected ▲ for RSS up trend; got: {dump}"
        );
    }

    #[test]
    fn render_metrics_lines_falls_back_to_dash_without_observation() {
        // No prior MemObservation for this pane → trend is None →
        // renderer falls back to the dim em-dash glyph.
        use ratatui::layout::Rect;
        let mut rep = base_test_report("test:1:main");
        rep.signals.process_memory_mb = Some(crate::domain::signal::MetricValue::new(
            312.0,
            crate::domain::origin::SourceKind::Heuristic,
        ));
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            Rect::new(0, 0, 120, 30),
            &HashMap::new(),
        );
        let cost_mem_line = &lines[5];
        let dump: String = cost_mem_line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            dump.contains("MEM 312 MiB ─"),
            "expected RSS line ending in '─' (no prior trend); got: {dump}"
        );
        // Belt-and-suspenders: no real arrow should appear without a
        // prior observation.
        assert!(
            !dump.contains("▲") && !dump.contains("▼"),
            "expected no real arrow without observation; got: {dump}"
        );
    }

    #[test]
    fn codex_pane_split_resets_at_renders_in_metrics_overlay() {
        use ratatui::layout::Rect;
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut rep = base_test_report("codex:1:review");
        // Simulate F-6 App Server populating split resets_at:
        rep.signals.quota_5h_resets_at = Some(crate::domain::signal::MetricValue::new(
            now + 2 * 3600 + 13 * 60, // 2h13m from now
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        rep.signals.quota_weekly_resets_at = Some(crate::domain::signal::MetricValue::new(
            now + 4 * 86400 + 6 * 3600, // 4d 6h from now
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));

        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            Rect::new(0, 0, 120, 30),
            &HashMap::new(),
        );
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(
            dump.contains("5H reset"),
            "Codex with quota_5h_resets_at must show 5H reset; got:\n{dump}"
        );
        assert!(
            dump.contains("7D reset"),
            "Codex with quota_weekly_resets_at must show 7D reset"
        );
        assert!(dump.contains("2h13m"), "5H eta should format to 2h13m");
        assert!(
            dump.contains("4d 6h"),
            "7D eta should format to 4d 6h (>=24h day format)"
        );
    }

    #[test]
    fn codex_pane_without_resets_at_falls_back_to_dash() {
        use ratatui::layout::Rect;
        let rep = base_test_report("codex:1:review");
        // No quota_*_resets_at populated (codex_app_server OFF scenario)
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            Rect::new(0, 0, 120, 30),
            &HashMap::new(),
        );
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        // Right column row 1 should be em-dash, not "5H reset"/"7D reset"/"RESET ▸"
        assert!(
            !dump.contains("5H reset"),
            "no resets_at should mean no 5H reset row"
        );
        assert!(
            !dump.contains("7D reset"),
            "no resets_at should mean no 7D reset row"
        );
        assert!(
            !dump.contains("RESET ▸"),
            "no Codex ModelReset (only Gemini emits it)"
        );
        // The reset slot should be em-dash
        assert!(
            dump.contains("─"),
            "reset slot should be em-dash without resets_at"
        );
    }

    #[test]
    fn gemini_pane_shows_quota_in_metrics_overlay() {
        use ratatui::layout::Rect;
        let mut rep = base_test_report("gemini:1:research");
        // Gemini populates the SINGLE quota_pressure field, not split.
        rep.signals.quota_pressure = Some(crate::domain::signal::MetricValue::new(
            0.47,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            Rect::new(0, 0, 120, 30),
            &HashMap::new(),
        );
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(
            dump.contains("QUOTA"),
            "Gemini pane should show QUOTA bar; got:\n{dump}"
        );
        assert!(dump.contains("47%"), "QUOTA bar should render 47%");
        // 5H / 7D labels should NOT appear since split is not populated.
        assert!(
            !dump.contains("5H "),
            "5H label should not appear when only single quota is present:\n{dump}"
        );
        assert!(
            !dump.contains("7D "),
            "7D label should not appear when only single quota is present:\n{dump}"
        );
    }

    #[test]
    fn gemini_pane_uses_model_reset_runtime_fact_in_overlay() {
        use ratatui::layout::Rect;
        let mut rep = base_test_report("gemini:1:research");
        rep.signals
            .runtime_facts
            .push(crate::domain::signal::RuntimeFact::new(
                crate::domain::signal::RuntimeFactKind::ModelReset,
                "Pro 14:35 (1h22m)",
                crate::domain::origin::SourceKind::ProviderOfficial,
            ));
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            Rect::new(0, 0, 120, 30),
            &HashMap::new(),
        );
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(
            dump.contains("RESET"),
            "Gemini reset row should fallback to ModelReset runtime fact; got:\n{dump}"
        );
        assert!(
            dump.contains("Pro 14:35"),
            "RESET row should carry provider-rendered model+time; got:\n{dump}"
        );
    }

    #[test]
    fn claude_codex_split_quota_still_renders_5h_and_7d() {
        use ratatui::layout::Rect;
        let mut rep = base_test_report("codex:1:review");
        rep.signals.quota_5h_pressure = Some(crate::domain::signal::MetricValue::new(
            0.92,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        rep.signals.quota_weekly_pressure = Some(crate::domain::signal::MetricValue::new(
            0.35,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&rep),
            Rect::new(0, 0, 120, 30),
            &HashMap::new(),
        );
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(dump.contains("5H "), "split-quota provider must show 5H");
        assert!(dump.contains("7D "), "split-quota provider must show 7D");
        assert!(dump.contains("92%"));
        assert!(dump.contains("35%"));
        // QUOTA single-label should NOT appear when split is populated.
        assert!(
            !dump.contains("QUOTA "),
            "QUOTA single label should not appear when split is populated:\n{dump}"
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
            anomalies: vec![],
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

    #[test]
    fn metrics_modal_rects_zero_offset_matches_centered() {
        let viewport = Rect::new(0, 0, 200, 80);
        let with_offset = metrics_modal_rects(viewport, 95, 90, 0, 0);
        let baseline = crate::ui::dashboard::centered_rect(95, 90, viewport);
        assert_eq!(with_offset.area, baseline);
    }

    #[test]
    fn metrics_modal_rects_positive_offset_moves_right_down() {
        let viewport = Rect::new(0, 0, 200, 80);
        let r0 = metrics_modal_rects(viewport, 95, 90, 0, 0);
        let r1 = metrics_modal_rects(viewport, 95, 90, 5, 3);
        assert_eq!(r1.area.x, r0.area.x + 5);
        assert_eq!(r1.area.y, r0.area.y + 3);
    }

    #[test]
    fn metrics_modal_rects_negative_offset_moves_left_up() {
        let viewport = Rect::new(0, 0, 200, 80);
        let r0 = metrics_modal_rects(viewport, 95, 90, 0, 0);
        let r1 = metrics_modal_rects(viewport, 95, 90, -4, -2);
        assert_eq!(r1.area.x, r0.area.x.saturating_sub(4));
        assert_eq!(r1.area.y, r0.area.y.saturating_sub(2));
    }

    #[test]
    fn metrics_modal_rects_right_clamp_keeps_4_cells_visible() {
        // Non-origin viewport so a viewport-origin bug cannot hide the clamp.
        let viewport = Rect::new(20, 5, 200, 80);
        let r = metrics_modal_rects(viewport, 50, 50, i16::MAX, 0);
        let visible_right = (viewport.x + viewport.width).saturating_sub(r.area.x);
        assert!(
            visible_right >= 4,
            "right clamp must leave >= 4 cells inside viewport (got {visible_right})"
        );
    }

    #[test]
    fn metrics_modal_rects_left_clamp_snaps_to_viewport_x() {
        // Non-origin viewport so the snap-to-zero shortcut can't hide a bug.
        // The modal cannot extend past viewport.x (left is a hard bound —
        // Ratatui u16-Rect cannot represent negative coordinates).
        let viewport = Rect::new(20, 5, 200, 80);
        let r = metrics_modal_rects(viewport, 50, 50, i16::MIN, 0);
        assert_eq!(
            r.area.x, viewport.x,
            "modal must not extend past viewport.x (left is hard bound)"
        );
    }

    #[test]
    fn metrics_modal_rects_bottom_clamp_keeps_1_row_visible() {
        // Non-origin viewport so a viewport-origin bug cannot hide the clamp.
        let viewport = Rect::new(20, 5, 200, 80);
        let r = metrics_modal_rects(viewport, 50, 50, 0, i16::MAX);
        let visible_height = (viewport.y + viewport.height).saturating_sub(r.area.y);
        assert!(
            visible_height >= 1,
            "bottom clamp must leave >= 1 row inside viewport (got {visible_height})"
        );
    }

    #[test]
    fn metrics_modal_rects_top_clamp_snaps_to_viewport_y() {
        // Non-origin viewport so the snap-to-zero shortcut can't hide a bug.
        // The modal cannot extend past viewport.y (top is a hard bound —
        // Ratatui u16-Rect cannot represent negative coordinates).
        let viewport = Rect::new(20, 5, 200, 80);
        let r = metrics_modal_rects(viewport, 50, 50, 0, i16::MIN);
        assert_eq!(
            r.area.y, viewport.y,
            "modal must not extend past viewport.y (top is hard bound)"
        );
    }

    #[test]
    fn reset_size_also_zeros_offset() {
        let mut o = MetricsOverlay::new();
        o.set_offset(7, 4);
        assert_eq!(o.offset_x(), 7);
        assert_eq!(o.offset_y(), 4);
        o.reset_size();
        assert_eq!(o.offset_x(), 0);
        assert_eq!(o.offset_y(), 0);
        assert_eq!(o.width_pct(), MetricsOverlay::DEFAULT_WIDTH_PCT);
        assert_eq!(o.height_pct(), MetricsOverlay::DEFAULT_HEIGHT_PCT);
    }

    #[test]
    fn close_preserves_offset_and_size() {
        let mut o = MetricsOverlay::new();
        o.open();
        o.set_offset(3, -2);
        o.grow();
        let (w, h, ox, oy) = (o.width_pct(), o.height_pct(), o.offset_x(), o.offset_y());
        o.close();
        o.open();
        assert_eq!(o.width_pct(), w);
        assert_eq!(o.height_pct(), h);
        assert_eq!(o.offset_x(), ox);
        assert_eq!(o.offset_y(), oy);
    }

    #[test]
    fn pane_card_includes_anomalies_row_when_present() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind, AnomalySignal};
        use crate::domain::recommendation::Severity;
        use ratatui::layout::Rect;

        let mut pane = base_test_report("claude:1:main");
        pane.anomalies = vec![AnomalySignal {
            kind: AnomalyKind::IdentityChurn,
            confidence: AnomalyConfidence::Medium,
            severity: Severity::Concern,
            evidence: vec![],
            window_polls: 10,
            detected_at: 0,
        }];
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&pane),
            Rect::new(0, 0, 120, 30),
            &HashMap::new(),
        );
        let rendered: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(
            rendered.contains("ANOMALIES 1"),
            "ANOMALIES count missing: {rendered}"
        );
        assert!(
            rendered.contains("IdentityChurn:medium"),
            "kind:conf token missing: {rendered}"
        );
    }

    #[test]
    fn pane_card_omits_anomalies_row_when_empty() {
        use ratatui::layout::Rect;

        let pane = base_test_report("claude:1:main");
        // anomalies is already vec![] from base_test_report
        let lines = render_metrics_lines(
            &MetricsOverlay::default(),
            "tgt",
            std::slice::from_ref(&pane),
            Rect::new(0, 0, 120, 30),
            &HashMap::new(),
        );
        let rendered: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(
            !rendered.contains("ANOMALIES"),
            "ANOMALIES row should be absent when anomalies is empty: {rendered}"
        );
    }
}
