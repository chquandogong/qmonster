use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::audit::AuditEventKind;
use crate::ui::alerts::AlertView;
use crate::ui::{alerts, labels, panels, scroll_hint, theme};

const VERSION_BADGE_PADDING: u16 = 2;
const GIT_WIDTH_PERCENT: u16 = 72;
const GIT_HEIGHT_PERCENT: u16 = 68;
const HELP_WIDTH_PERCENT: u16 = 76;
const HELP_HEIGHT_PERCENT: u16 = 76;
const HELP_LABEL_WIDTH: usize = 18;
const DASHBOARD_FOOTER_HEIGHT: u16 = 2;
const DASHBOARD_SPLIT_HANDLE_HEIGHT: u16 = 1;
const DEFAULT_ALERTS_PERCENT: u16 = 36;
const MIN_ALERTS_PERCENT: u16 = 20;
const MAX_ALERTS_PERCENT: u16 = 80;
const RESIZE_STEP_PERCENT: u16 = 5;
const MIN_ALERTS_HEIGHT: u16 = 5;
const MIN_PANES_HEIGHT: u16 = 6;

/// Top-level dashboard layout: alerts on top, pane list below, and a
/// persistent control footer to keep navigation discoverable.
pub struct DashboardView<'a> {
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub anomaly_events_ring: &'a crate::app::anomaly_events_ring::AnomalyEventsRing,
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub hidden_until: &'a HashMap<String, Instant>,
    pub state_flashes: &'a HashMap<String, panels::PaneStateFlash>,
    pub now: Instant,
    pub now_unix_secs: u64,
    pub cost_budget_usd: f64,
    pub audit_recent_severity: Option<crate::domain::recommendation::Severity>,
    pub target_label: &'a str,
    pub split: DashboardSplit,
    pub alerts_focused: bool,
    pub panes_focused: bool,
    /// v1.51.0: divider renders an IME-active warning banner when true.
    /// Computed by the loop from `Context::ime_state.is_active(now)`.
    pub ime_active: bool,
    /// v1.59.0: optional case-insensitive filter applied to the
    /// Alerts list. None = no filter; Some("") = filter input mode
    /// active but empty. Plumbed straight through to AlertView.
    pub alert_filter: Option<&'a str>,
}

pub struct TargetPickerView<'a> {
    pub title: &'a str,
    pub hint: &'a str,
    pub labels: &'a [String],
    pub preview_title: &'a str,
    pub preview_lines: &'a [String],
    pub current_label: &'a str,
}

pub struct DashboardRects {
    pub now_strip: Rect,
    pub alerts: Rect,
    pub divider: Rect,
    pub panes: Rect,
    pub footer: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardSplit {
    alerts_percent: u16,
}

impl Default for DashboardSplit {
    fn default() -> Self {
        Self::new(DEFAULT_ALERTS_PERCENT)
    }
}

impl DashboardSplit {
    pub fn new(alerts_percent: u16) -> Self {
        Self {
            alerts_percent: alerts_percent.clamp(MIN_ALERTS_PERCENT, MAX_ALERTS_PERCENT),
        }
    }

    pub fn alerts_percent(self) -> u16 {
        self.alerts_percent
    }

    pub fn shrink_alerts(&mut self) {
        self.nudge_alerts(-(RESIZE_STEP_PERCENT as i16));
    }

    pub fn grow_alerts(&mut self) {
        self.nudge_alerts(RESIZE_STEP_PERCENT as i16);
    }

    pub fn cycle_alerts(&mut self) {
        let next = self.alerts_percent.saturating_add(RESIZE_STEP_PERCENT);
        self.alerts_percent = if next > MAX_ALERTS_PERCENT {
            MIN_ALERTS_PERCENT
        } else {
            next
        };
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn nudge_alerts(&mut self, delta: i16) {
        let next = (self.alerts_percent as i16 + delta)
            .clamp(MIN_ALERTS_PERCENT as i16, MAX_ALERTS_PERCENT as i16);
        self.alerts_percent = next as u16;
    }
}

pub struct TargetPickerRects {
    pub area: Rect,
    pub list: Rect,
    pub preview: Rect,
    pub hint: Rect,
}

pub struct HelpModalRects {
    pub area: Rect,
    pub body: Rect,
    pub hint: Rect,
}

pub struct GitModalRects {
    pub area: Rect,
    pub body: Rect,
    pub hint: Rect,
}

pub fn close_button_rect(area: Rect) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(4),
        area.y,
        3.min(area.width),
        1.min(area.height),
    )
}

pub fn render_dashboard(
    frame: &mut Frame<'_>,
    alert_state: &mut ListState,
    pane_state: &mut ListState,
    view: DashboardView<'_>,
) {
    let rects = dashboard_rects(frame.area(), view.split);

    render_now_strip(
        rects.now_strip,
        frame.buffer_mut(),
        now_strip_summary(
            view.reports,
            view.anomaly_events_ring,
            view.now_unix_secs,
            view.cost_budget_usd,
        ),
    );

    alerts::render_alerts(
        rects.alerts,
        frame.buffer_mut(),
        alert_state,
        AlertView {
            notices: view.notices,
            reports: view.reports,
            fresh_alerts: view.fresh_alerts,
            alert_times: view.alert_times,
            hidden_until: view.hidden_until,
            now: view.now,
            target_label: view.target_label,
            focused: view.alerts_focused,
            filter: view.alert_filter,
        },
    );

    render_split_divider(
        rects.divider,
        frame.buffer_mut(),
        view.split,
        view.ime_active,
    );

    panels::render_pane_list(
        rects.panes,
        frame.buffer_mut(),
        view.reports,
        pane_state,
        view.target_label,
        view.panes_focused,
        panels::PaneStateFlashView {
            now: view.now,
            state_flashes: view.state_flashes,
        },
    );

    // v1.39 Pending-action discoverability surface B: count panes
    // with a pending prompt-send proposal AND alerts with a runnable
    // suggested_command, and pass the counts (plus their highest
    // severity) into the footer so the always-visible chip surfaces
    // pending work peripherally.
    let (proposal_count, proposal_top_severity) = pending_proposal_summary(view.reports);
    let (copy_count, copy_top_severity) =
        alerts::copy_alert_count(view.notices, view.reports, view.hidden_until, view.now);
    render_footer(
        rects.footer,
        frame.buffer_mut(),
        view.alerts_focused,
        view.panes_focused,
        view.split,
        FooterCounters {
            proposal_count,
            proposal_top_severity,
            copy_count,
            copy_top_severity,
            audit_top_severity: view.audit_recent_severity,
        },
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NowStripSummary {
    text: String,
    severity: crate::domain::recommendation::Severity,
}

fn now_strip_summary(
    reports: &[PaneReport],
    anomaly_events_ring: &crate::app::anomaly_events_ring::AnomalyEventsRing,
    now_unix_secs: u64,
    cost_budget_usd: f64,
) -> NowStripSummary {
    let events: Vec<crate::domain::anomaly::AnomalyEvent> =
        anomaly_events_ring.iter().cloned().collect();
    now_strip_summary_for(reports, &events, now_unix_secs, cost_budget_usd)
}

fn now_strip_summary_for(
    reports: &[PaneReport],
    anomaly_events: &[crate::domain::anomaly::AnomalyEvent],
    now_unix_secs: u64,
    cost_budget_usd: f64,
) -> NowStripSummary {
    if let Some((report, idle_cause)) = reports
        .iter()
        .filter_map(|report| {
            report
                .idle_state
                .or(report.signals.idle_state)
                .map(|idle_cause| (report, idle_cause))
        })
        .find(|(_, idle_cause)| {
            matches!(
                idle_cause,
                crate::domain::signal::IdleCause::PermissionWait
                    | crate::domain::signal::IdleCause::InputWait
            )
        })
    {
        let (label, hint) = match idle_cause {
            crate::domain::signal::IdleCause::PermissionWait => ("WAIT APPROVAL", "Enter approve"),
            crate::domain::signal::IdleCause::InputWait => ("WAIT INPUT", "respond in pane"),
            _ => unreachable!("wait-state filter admits only permission or input waits"),
        };
        return NowStripSummary {
            text: format!("{} {label} — {hint} · q skip", report.pane_id),
            severity: crate::domain::recommendation::Severity::Risk,
        };
    }

    for report in reports {
        if let Some(rec) = report
            .recommendations
            .iter()
            .find(|rec| rec.severity >= crate::domain::recommendation::Severity::Risk)
        {
            let action = rec
                .suggested_command
                .as_deref()
                .map(|cmd| format!("p send {cmd}"))
                .unwrap_or_else(|| "p review".to_string());
            return NowStripSummary {
                text: format!(
                    "{} RISK: {} — {action} · d dismiss",
                    report.pane_id, rec.reason
                ),
                severity: rec.severity,
            };
        }
    }

    if let Some(summary) = quota_or_cost_now_summary(reports, cost_budget_usd) {
        return summary;
    }

    if let Some(event) = anomaly_events.iter().rev().find(|event| {
        event.promoted
            && event.timestamp <= now_unix_secs
            && now_unix_secs.saturating_sub(event.timestamp) <= 60
    }) {
        return NowStripSummary {
            text: format!(
                "{} ANOMALY {} ({}) — see Alerts",
                event.pane_id,
                event.kind.label(),
                event.confidence.label()
            ),
            severity: event.severity,
        };
    }

    let last_anomaly = anomaly_events
        .iter()
        .filter(|event| event.timestamp <= now_unix_secs)
        .map(|event| now_unix_secs.saturating_sub(event.timestamp))
        .min()
        .map(format_age);
    NowStripSummary {
        text: format!(
            "healthy · {} panes · last anomaly {}",
            reports.len(),
            last_anomaly.unwrap_or_else(|| "none".to_string())
        ),
        severity: crate::domain::recommendation::Severity::Good,
    }
}

fn quota_or_cost_now_summary(
    reports: &[PaneReport],
    cost_budget_usd: f64,
) -> Option<NowStripSummary> {
    let quota = reports
        .iter()
        .filter_map(|report| {
            report
                .signals
                .quota_5h_pressure
                .as_ref()
                .map(|metric| (report, metric.value))
        })
        .filter(|(_, pressure)| *pressure >= 0.85)
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if let Some((report, pressure)) = quota {
        let mut text = format!("{} QUOTA {:.0}%", report.pane_id, pressure * 100.0);
        if cost_is_budget_pressure(report, cost_budget_usd)
            && let Some(rate) = cost_burn_usd_per_hour(&report.recent_token_samples)
        {
            text.push_str(&format!(" / cost burn ${rate:.2}/hr"));
        }
        return Some(NowStripSummary {
            text,
            severity: crate::domain::recommendation::Severity::Warning,
        });
    }

    reports
        .iter()
        .find(|report| cost_is_budget_pressure(report, cost_budget_usd))
        .map(|report| {
            let rate = cost_burn_usd_per_hour(&report.recent_token_samples)
                .map(|rate| format!("${rate:.2}/hr"))
                .unwrap_or_else(|| "$?".to_string());
            NowStripSummary {
                text: format!("{} cost burn {rate}", report.pane_id),
                severity: crate::domain::recommendation::Severity::Warning,
            }
        })
}

fn cost_is_budget_pressure(report: &PaneReport, cost_budget_usd: f64) -> bool {
    if !cost_budget_usd.is_finite() || cost_budget_usd <= 0.0 {
        return false;
    }
    let threshold = cost_budget_usd * 0.8;
    report
        .signals
        .cost_usd
        .as_ref()
        .is_some_and(|metric| metric.value >= threshold)
        || report
            .recent_token_samples
            .iter()
            .filter_map(|sample| sample.cost_usd)
            .any(|cost| cost >= threshold)
}

fn cost_burn_usd_per_hour(samples: &[crate::store::TokenSample]) -> Option<f64> {
    let newest = samples.iter().find(|sample| sample.cost_usd.is_some())?;
    let oldest = samples
        .iter()
        .rev()
        .find(|sample| sample.cost_usd.is_some())?;
    let newest_cost = newest.cost_usd?;
    let oldest_cost = oldest.cost_usd?;
    let delta = newest_cost - oldest_cost;
    let elapsed_ms = newest.ts_unix_ms.saturating_sub(oldest.ts_unix_ms);
    if delta <= 0.0 || elapsed_ms <= 0 {
        return None;
    }
    Some(delta / (elapsed_ms as f64 / 3_600_000.0))
}

fn format_age(age_secs: u64) -> String {
    if age_secs < 60 {
        format!("{age_secs}s ago")
    } else if age_secs < 3_600 {
        format!("{}m ago", age_secs / 60)
    } else {
        format!("{}h ago", age_secs / 3_600)
    }
}

fn render_now_strip(area: Rect, buf: &mut Buffer, summary: NowStripSummary) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let style = Style::default()
        .fg(theme::severity_color(summary.severity))
        .bg(theme::badge_bg())
        .add_modifier(Modifier::BOLD);
    Paragraph::new(Line::from(vec![
        Span::styled(
            "Now · ",
            Style::default().fg(theme::text_dim()).bg(theme::badge_bg()),
        ),
        Span::styled(summary.text, style),
    ]))
    .render(area, buf);
}

/// v1.39 Pending-action discoverability surface B: walk the live
/// reports for any pane that carries a pending `PromptSendProposed`
/// effect via `first_prompt_send_proposal` (so the count matches the
/// `★p` chip on each card title) and surface the highest-severity
/// recommendation across those panes for the footer chip color.
fn pending_proposal_summary(
    reports: &[PaneReport],
) -> (usize, Option<crate::domain::recommendation::Severity>) {
    let mut count = 0usize;
    let mut top: Option<crate::domain::recommendation::Severity> = None;
    for r in reports {
        if crate::app::prompt_send_actions::first_prompt_send_proposal(r).is_some() {
            count += 1;
            if let Some(s) = r.recommendations.iter().map(|rec| rec.severity).max() {
                top = Some(top.map_or(s, |t| t.max(s)));
            }
        }
    }
    (count, top)
}

/// Footer chip counts threaded from `render_dashboard` into
/// `render_footer`. Severities come from the highest-severity
/// recommendation among the matching panes (★p) or alerts (★y), so
/// the chip colors track the underlying urgency rather than always
/// rendering in the dim default.
struct FooterCounters {
    proposal_count: usize,
    proposal_top_severity: Option<crate::domain::recommendation::Severity>,
    copy_count: usize,
    copy_top_severity: Option<crate::domain::recommendation::Severity>,
    audit_top_severity: Option<crate::domain::recommendation::Severity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterStatusChip {
    Proposal,
    Copy,
    Audit,
}

pub fn render_target_picker(
    frame: &mut Frame<'_>,
    state: &mut ListState,
    view: TargetPickerView<'_>,
) {
    let rects = target_picker_rects(frame.area());
    frame.render_widget(Clear, rects.area);

    let block = Block::default()
        .title(format!("{} · current {}", view.title, view.current_label))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::border_active()));

    if view.labels.is_empty() {
        frame.render_widget(
            Paragraph::new("no tmux windows discovered")
                .style(Style::default().fg(theme::text_dim()))
                .block(block),
            rects.list,
        );
        frame.render_widget(
            Paragraph::new(target_picker_hint_line(view.hint, 0, 0))
                .style(Style::default().fg(theme::text_dim()))
                .wrap(Wrap { trim: false }),
            rects.hint,
        );
        return;
    }

    let selected = state
        .selected()
        .unwrap_or(0)
        .min(view.labels.len().saturating_sub(1));
    state.select(Some(selected));
    let items: Vec<ListItem<'_>> = view
        .labels
        .iter()
        .map(|label| ListItem::new(label.as_str()))
        .collect();

    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(theme::text_primary())
                    .bg(theme::badge_bg())
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ "),
        rects.list,
        state,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(rects.list),
    );
    frame.render_widget(
        Paragraph::new(if view.preview_lines.is_empty() {
            "no pane preview available".to_string()
        } else {
            view.preview_lines.join("\n")
        })
        .style(Style::default().fg(theme::text_dim()))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(view.preview_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::border_idle())),
        ),
        rects.preview,
    );
    frame.render_widget(
        Paragraph::new(target_picker_hint_line(
            view.hint,
            selected,
            view.labels.len(),
        ))
        .style(Style::default().fg(theme::text_dim()))
        .wrap(Wrap { trim: false }),
        rects.hint,
    );
}

fn target_picker_hint_line(base: &str, selected: usize, total: usize) -> String {
    let max_scroll = total.saturating_sub(1).min(u16::MAX as usize) as u16;
    let scroll = selected.min(max_scroll as usize) as u16;
    scrollable_modal_hint(base, scroll, max_scroll)
}

pub fn render_help_modal(frame: &mut Frame<'_>, scroll: u16) {
    let rects = help_modal_rects(frame.area());
    let text_width = help_body_text_width(rects.body);
    let max_scroll = max_help_scroll(frame.area()).min(u16::MAX as usize) as u16;
    let scroll = scroll.min(max_scroll);
    frame.render_widget(Clear, rects.area);

    frame.render_widget(
        Paragraph::new(help_lines_for_width(text_width))
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title("Help")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::border_active())),
            ),
        rects.body,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(rects.body),
    );
    frame.render_widget(
        Paragraph::new(scrollable_modal_hint(
            "↑/↓ scroll · PgUp/PgDn jump · Home/End · 1..N jump section · click [x] close · Esc close",
            scroll,
            max_scroll,
        ))
        .style(Style::default().fg(theme::text_dim()))
        .wrap(Wrap { trim: false }),
        rects.hint,
    );
}

/// Render the Provider Setup modal (Phase G-1, v0.4.0). Read-only
/// guidance for wiring Claude / Codex / Gemini to expose token +
/// cache data and a recommended tmux bundle to Qmonster — never
/// writes provider config files.
pub struct ProviderSetupModalRects {
    pub area: Rect,
    pub tabs: Rect,
    pub body: Rect,
    pub hint: Rect,
}

pub fn provider_setup_modal_rects(viewport: Rect) -> ProviderSetupModalRects {
    let area = centered_rect(80, 76, viewport);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tabs row
            Constraint::Min(5),    // body
            Constraint::Length(2), // hint
        ])
        .split(area);
    ProviderSetupModalRects {
        area,
        tabs: chunks[0],
        body: chunks[1],
        hint: chunks[2],
    }
}

/// Map a left-click column on the tabs row to a tab index (0/1/2/3/4).
/// Uses the actual rendered tab layout — ratatui's `Tabs` widget
/// renders left-aligned ` Claude │ Codex │ Gemini │ agy │ Tmux ` inside the border,
/// not equal slots, so a naive width split would mis-route
/// clicks on a wide modal (later tab regions would map to the
/// always-Claude left third). Boundaries derived from the fixed
/// `Claude` (6) / `Codex` (5) / `Gemini` (6) / `agy` (3) / `Tmux` (4) label widths plus
/// 1-cell padding around each label and 1-cell `│` divider between
/// adjacent tabs. Returns None for columns outside the rendered tab
/// region (including the empty space to the right of `Tmux`) so
/// clicks on the unrendered remainder don't accidentally snap to
/// the last tab.
pub fn provider_setup_tab_index_at(tabs: Rect, column: u16) -> Option<usize> {
    if column < tabs.x {
        return None;
    }
    // Block::default().borders(ALL) eats the leftmost cell as a border.
    let inner_x = tabs.x.saturating_add(1);
    // Layout (positions relative to inner_x):
    //   0       : leading space
    //   1..=6   : "Claude"      (6 cells)
    //   7       : trailing space
    //   8       : "│" divider
    //   9       : leading space
    //   10..=14 : "Codex"       (5 cells)
    //   15      : trailing space
    //   16      : "│" divider
    //   17      : leading space
    //   18..=23 : "Gemini"      (6 cells)
    //   24      : trailing space
    //   25      : "│" divider
    //   26      : leading space
    //   27..=29 : "agy"         (3 cells)
    //   30      : trailing space
    //   31      : "│" divider
    //   32      : leading space
    //   33..=36 : "Tmux"        (4 cells)
    //   37      : trailing space
    let claude_end = inner_x.saturating_add(8); // first column belonging to Codex
    let codex_end = inner_x.saturating_add(16); // first column belonging to Gemini
    let gemini_end = inner_x.saturating_add(25); // first column belonging to agy
    let agy_end = inner_x.saturating_add(32); // first column belonging to Tmux
    let tmux_end = inner_x.saturating_add(38); // first column past "Tmux "
    if column >= tmux_end {
        return None;
    }
    if column < claude_end {
        Some(0)
    } else if column < codex_end {
        Some(1)
    } else if column < gemini_end {
        Some(2)
    } else if column < agy_end {
        Some(3)
    } else {
        Some(4)
    }
}

pub fn render_provider_setup_modal(
    frame: &mut Frame<'_>,
    overlay: &crate::ui::provider_setup::ProviderSetupOverlay,
) {
    use ratatui::widgets::Tabs;

    let rects = provider_setup_modal_rects(frame.area());
    frame.render_widget(Clear, rects.area);

    let home = crate::ui::provider_setup::detect_home();
    let claude = home
        .as_deref()
        .map(crate::ui::provider_setup::detect_claude_state)
        .unwrap_or(crate::ui::provider_setup::ClaudeState {
            statusline_script_present: false,
            statusline_size_bytes: 0,
            exports_cache_read: false,
            exports_cache_creation: false,
            exports_input_tokens: false,
            sidefile_export_present: false,
        });
    let codex = home
        .as_deref()
        .map(crate::ui::provider_setup::detect_codex_state)
        .unwrap_or(crate::ui::provider_setup::CodexState {
            config_present: false,
            app_server_running: false,
        });
    let gemini = home
        .as_deref()
        .map(crate::ui::provider_setup::detect_gemini_footer_state)
        .unwrap_or_default();

    let tab_titles: Vec<Line<'_>> = crate::ui::provider_setup::ProviderSetupTab::ALL
        .iter()
        .map(|t| Line::from(t.label()))
        .collect();
    let active_idx = overlay.tab.index();
    let tabs = Tabs::new(tab_titles)
        .select(active_idx)
        .block(
            Block::default()
                .title("Provider Setup")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::border_active())),
        )
        .highlight_style(
            Style::default()
                .fg(theme::text_primary())
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, rects.tabs);
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(rects.tabs),
    );

    let lines = crate::ui::provider_setup::render_tab_content(overlay, &claude, &codex, &gemini);
    let visible_lines = rects.body.height.saturating_sub(2) as usize;
    let max_scroll = lines
        .len()
        .saturating_sub(visible_lines)
        .min(u16::MAX as usize) as u16;
    let scroll = (overlay.scroll_offset as u16).min(max_scroll);
    let body_text: Vec<Line<'_>> = lines.iter().map(|l| Line::from(l.as_str())).collect();
    let body = Paragraph::new(body_text)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::border_active())),
        );
    frame.render_widget(body, rects.body);

    let hint = Paragraph::new(Line::from(scrollable_modal_hint(
        "[1]/[2]/[3]/[4]/[Tab]/[←→] tab · [y] copy current tab snippet · edit provider options in [S] Settings > Integrations · [↑↓ wheel j/k] scroll · click [x] / [q] / [Esc] close",
        scroll,
        max_scroll,
    )))
    .style(Style::default().fg(theme::text_dim()))
    .wrap(Wrap { trim: false });
    frame.render_widget(hint, rects.hint);
}

pub fn render_git_modal(frame: &mut Frame<'_>, title: &str, lines: &[String], scroll: u16) {
    let rects = git_modal_rects(frame.area());
    let max_scroll = max_git_scroll(frame.area(), lines.len()).min(u16::MAX as usize) as u16;
    let scroll = scroll.min(max_scroll);
    frame.render_widget(Clear, rects.area);

    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::border_active())),
            ),
        rects.body,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(rects.body),
    );
    frame.render_widget(
        Paragraph::new(scrollable_modal_hint(
            "↑/↓ scroll · PgUp/PgDn jump · Home/End · click [x] close · Esc close",
            scroll,
            max_scroll,
        ))
        .style(Style::default().fg(theme::text_dim()))
        .wrap(Wrap { trim: false }),
        rects.hint,
    );
}

fn scrollable_modal_hint(base: &str, scroll: u16, max_scroll: u16) -> String {
    scroll_hint::scrollable_hint(base, scroll, max_scroll)
}

pub fn max_help_scroll(viewport: Rect) -> usize {
    let rects = help_modal_rects(viewport);
    let visible_lines = rects.body.height.saturating_sub(2) as usize;
    help_lines_for_width(help_body_text_width(rects.body))
        .len()
        .saturating_sub(visible_lines)
}

pub fn max_git_scroll(viewport: Rect, line_count: usize) -> usize {
    let rects = git_modal_rects(viewport);
    let visible_lines = rects.body.height.saturating_sub(2) as usize;
    line_count.saturating_sub(visible_lines)
}

pub fn dashboard_rects(area: Rect, split: DashboardSplit) -> DashboardRects {
    let (footer_height, divider_height, body_height) = dashboard_layout_heights(area);
    let now_strip_height = if body_height > MIN_ALERTS_HEIGHT + MIN_PANES_HEIGHT {
        1
    } else {
        0
    };
    let list_body_height = body_height.saturating_sub(now_strip_height);
    let alerts_height = alerts_height_for(list_body_height, split);
    let panes_height = list_body_height.saturating_sub(alerts_height);
    let now_strip = Rect::new(area.x, area.y, area.width, now_strip_height);
    let alerts = Rect::new(
        area.x,
        now_strip.y.saturating_add(now_strip.height),
        area.width,
        alerts_height,
    );
    let divider = Rect::new(
        area.x,
        alerts.y.saturating_add(alerts.height),
        area.width,
        divider_height,
    );
    let panes = Rect::new(
        area.x,
        divider.y.saturating_add(divider.height),
        area.width,
        panes_height,
    );
    let footer = Rect::new(
        area.x,
        area.y
            .saturating_add(area.height)
            .saturating_sub(footer_height),
        area.width,
        footer_height,
    );
    DashboardRects {
        now_strip,
        alerts,
        divider,
        panes,
        footer,
    }
}

pub fn dashboard_split_from_row(area: Rect, row: u16) -> DashboardSplit {
    let (_, _, body_height) = dashboard_layout_heights(area);
    if body_height == 0 {
        return DashboardSplit::default();
    }
    let relative_row = row.saturating_sub(area.y).min(body_height);
    let percent =
        ((relative_row as u32 * 100 + body_height as u32 / 2) / body_height as u32) as u16;
    DashboardSplit::new(percent)
}

fn dashboard_layout_heights(area: Rect) -> (u16, u16, u16) {
    let footer_height = DASHBOARD_FOOTER_HEIGHT.min(area.height);
    let content_height = area.height.saturating_sub(footer_height);
    let divider_height =
        if content_height >= MIN_ALERTS_HEIGHT + MIN_PANES_HEIGHT + DASHBOARD_SPLIT_HANDLE_HEIGHT {
            DASHBOARD_SPLIT_HANDLE_HEIGHT
        } else {
            0
        };
    let body_height = content_height.saturating_sub(divider_height);
    (footer_height, divider_height, body_height)
}

fn alerts_height_for(body_height: u16, split: DashboardSplit) -> u16 {
    if body_height == 0 {
        return 0;
    }
    let desired = ((body_height as u32 * split.alerts_percent() as u32 + 50) / 100) as u16;
    let min_alerts = MIN_ALERTS_HEIGHT.min(body_height);
    let min_panes = MIN_PANES_HEIGHT.min(body_height.saturating_sub(min_alerts));
    let max_alerts = body_height.saturating_sub(min_panes);
    desired.clamp(min_alerts, max_alerts)
}

pub fn target_picker_rects(viewport: Rect) -> TargetPickerRects {
    let area = centered_rect(76, 72, viewport);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(2)])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(chunks[0]);
    TargetPickerRects {
        area,
        list: body[0],
        preview: body[1],
        hint: chunks[1],
    }
}

pub fn help_modal_rects(viewport: Rect) -> HelpModalRects {
    let area = centered_rect(HELP_WIDTH_PERCENT, HELP_HEIGHT_PERCENT, viewport);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(2)])
        .split(area);
    HelpModalRects {
        area,
        body: chunks[0],
        hint: chunks[1],
    }
}

pub fn git_modal_rects(viewport: Rect) -> GitModalRects {
    let area = centered_rect(GIT_WIDTH_PERCENT, GIT_HEIGHT_PERCENT, viewport);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(2)])
        .split(area);
    GitModalRects {
        area,
        body: chunks[0],
        hint: chunks[1],
    }
}

pub fn version_badge_label() -> String {
    // v1.10.6: show the git-based version (populated by `build.rs`)
    // instead of `CARGO_PKG_VERSION`. The package version in
    // `Cargo.toml` is rarely bumped per commit and would mislead
    // operators about which code their binary actually carries.
    // `build.rs` embeds `git describe --tags --always --dirty` so
    // tagged builds show the tag, untagged builds show the short
    // SHA, and a dirty working tree gets a `-dirty` suffix.
    env!("QMONSTER_GIT_VERSION").to_string()
}

pub fn version_badge_rect(area: Rect) -> Rect {
    let label = version_badge_label();
    let width = (label.chars().count() as u16).saturating_add(VERSION_BADGE_PADDING);
    Rect::new(
        area.x + area.width.saturating_sub(width),
        area.y + area.height.saturating_sub(1),
        width.min(area.width),
        1,
    )
}

pub fn footer_keys_badge_label() -> &'static str {
    "keys"
}

pub fn footer_keys_badge_rect(area: Rect) -> Rect {
    let width = (footer_keys_badge_label().chars().count() as u16).saturating_add(4);
    Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1),
        width.min(area.width),
        1,
    )
}

fn render_split_divider(area: Rect, buf: &mut Buffer, split: DashboardSplit, ime_active: bool) {
    if area.height == 0 {
        return;
    }
    if ime_active {
        // v1.51.0: replace the normal drag-resize hint with a high-
        // visibility banner when the heuristic IME indicator says the
        // operator is typing non-ASCII. Helps Korean/CJK operators
        // notice they hit a command key while in IME mode (those
        // keystrokes get composed away rather than dispatched).
        Paragraph::new(split_divider_ime_banner_text())
            .style(
                Style::default()
                    .fg(theme::severity_color(
                        crate::domain::recommendation::Severity::Warning,
                    ))
                    .bg(theme::badge_bg())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .alignment(Alignment::Center)
            .render(area, buf);
        return;
    }
    Paragraph::new(format!(
        "drag resize alerts/panes · alerts {}% · [ ] resize · / cycle · = reset",
        split.alerts_percent()
    ))
    .style(Style::default().fg(theme::text_dim()).bg(theme::badge_bg()))
    .alignment(Alignment::Center)
    .render(area, buf);
}

pub(crate) fn split_divider_ime_banner_text() -> String {
    "  \u{26A0} HANGUL/IME ACTIVE — press 영문/English key to disable  \u{26A0}".to_string()
}

fn render_footer(
    area: Rect,
    buf: &mut Buffer,
    alerts_focused: bool,
    panes_focused: bool,
    split: DashboardSplit,
    counters: FooterCounters,
) {
    let focus = footer_focus_label(alerts_focused, panes_focused);
    let badge = version_badge_rect(area);
    let keys_badge = footer_keys_badge_rect(area);
    let text_x = keys_badge
        .x
        .saturating_add(keys_badge.width)
        .saturating_add(1);
    let text_right = badge.x.saturating_sub(1);
    let text_width = text_right.saturating_sub(text_x);
    let text_area = Rect::new(text_x, area.y, text_width, area.height);
    Paragraph::new(footer_keys_badge_label())
        .style(
            theme::label_style()
                .fg(theme::text_primary())
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .render(keys_badge, buf);
    Paragraph::new(footer_lines(focus, split, &counters))
        .style(Style::default().fg(theme::text_dim()))
        .wrap(Wrap { trim: false })
        .render(text_area, buf);
    Paragraph::new(version_badge_label())
        .style(
            theme::label_style()
                .fg(theme::text_primary())
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .render(badge, buf);
}

pub fn footer_focus_label(alerts_focused: bool, panes_focused: bool) -> &'static str {
    if alerts_focused {
        "focus: alerts"
    } else if panes_focused {
        "focus: panes"
    } else {
        "focus: overlay"
    }
}

/// Inputs for `footer_status_chip_at`. Bundled into a struct so the
/// function does not accumulate positional arguments as the footer gains
/// new chips (each chip needs a count + label, plus the cursor position to
/// hit-test against). All fields are immediate values from the renderer's
/// current frame; the struct is not stored.
pub struct FooterChipQuery<'a> {
    pub area: Rect,
    pub focus: &'a str,
    pub split: DashboardSplit,
    pub proposal_count: usize,
    pub copy_count: usize,
    pub audit_top_severity: Option<crate::domain::recommendation::Severity>,
    pub column: u16,
    pub row: u16,
}

pub fn footer_status_chip_at(q: FooterChipQuery<'_>) -> Option<FooterStatusChip> {
    if q.row != q.area.y || q.area.height == 0 || q.area.width == 0 {
        return None;
    }

    let keys_badge = footer_keys_badge_rect(q.area);
    let version_badge = version_badge_rect(q.area);
    let text_x = keys_badge
        .x
        .saturating_add(keys_badge.width)
        .saturating_add(1);
    let text_right = version_badge.x.saturating_sub(1);
    if q.column < text_x || q.column >= text_right {
        return None;
    }

    let head = footer_status_head(q.focus, q.split);
    let p_label = format!("\u{2605}p:{}", q.proposal_count);
    let y_label = format!("\u{2605}y:{}", q.copy_count);
    let a_letter = q.audit_top_severity.map(|s| s.letter()).unwrap_or("0");
    let a_label = format!("\u{2605}a:{a_letter}");
    let p_start = text_x.saturating_add(text_cells(&head));
    let y_start = p_start
        .saturating_add(text_cells(&p_label))
        .saturating_add(text_cells(" \u{00b7} "));
    let a_start = y_start
        .saturating_add(text_cells(&y_label))
        .saturating_add(text_cells(" \u{00b7} "));

    if column_in_text(q.column, p_start, &p_label) {
        Some(FooterStatusChip::Proposal)
    } else if column_in_text(q.column, y_start, &y_label) {
        Some(FooterStatusChip::Copy)
    } else if column_in_text(q.column, a_start, &a_label) {
        Some(FooterStatusChip::Audit)
    } else {
        None
    }
}

fn footer_status_head(focus: &str, split: DashboardSplit) -> String {
    format!(
        "{focus} \u{00b7} split {}% \u{00b7} ",
        split.alerts_percent()
    )
}

fn column_in_text(column: u16, start: u16, text: &str) -> bool {
    let end = start.saturating_add(text_cells(text));
    column >= start && column < end
}

fn text_cells(text: &str) -> u16 {
    text.chars().count().min(u16::MAX as usize) as u16
}

/// v1.39 Pending-action discoverability surface B: build the footer
/// as styled `Line`s so the always-visible `★p:N · ★y:M` counter
/// chip can render in severity color when N>0 / dim on 0. The first
/// row carries status; the second row aligns the compact key cluster
/// with the left `keys` chip and right version badge.
fn footer_lines(
    focus: &str,
    split: DashboardSplit,
    counters: &FooterCounters,
) -> Vec<Line<'static>> {
    let head = footer_status_head(focus, split);
    let p_chip = footer_chip_span(
        "\u{2605}p",
        counters.proposal_count,
        counters.proposal_top_severity,
    );
    let y_chip = footer_chip_span("\u{2605}y", counters.copy_count, counters.copy_top_severity);
    let a_chip = footer_audit_chip_span(counters.audit_top_severity);
    let status_spans = vec![
        Span::raw(head),
        p_chip,
        Span::raw(" \u{00b7} "),
        y_chip,
        Span::raw(" \u{00b7} "),
        a_chip,
    ];
    vec![
        Line::from(status_spans),
        Line::from(footer_compact_keys_text()),
    ]
}

fn footer_audit_chip_span(
    top_severity: Option<crate::domain::recommendation::Severity>,
) -> Span<'static> {
    let label = top_severity
        .map(|severity| severity.letter())
        .unwrap_or("0");
    let style = match top_severity {
        Some(severity) if severity >= crate::domain::recommendation::Severity::Warning => {
            Style::default()
                .fg(theme::severity_color(severity))
                .add_modifier(Modifier::BOLD)
        }
        _ => Style::default().fg(theme::text_dim()),
    };
    Span::styled(format!("\u{2605}a:{label}"), style)
}

/// Build a single `★p:N` / `★y:M` chip styled by the highest
/// severity among the matching panes/alerts. Renders dim when the
/// count is 0 so an empty queue doesn't pull the operator's eye.
fn footer_chip_span(
    label: &str,
    count: usize,
    top_severity: Option<crate::domain::recommendation::Severity>,
) -> Span<'static> {
    let style = if count == 0 {
        Style::default().fg(theme::text_dim())
    } else {
        let color = top_severity
            .map(theme::severity_color)
            .unwrap_or(theme::text_primary());
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    };
    Span::styled(format!("{label}:{count}"), style)
}

/// Static key-cluster string. Reused by `footer_lines` (after the
/// counter chip slot) and by `footer_text` (the legacy unit-tested
/// shape that pins `p accept` / `d dismiss` placement).
#[cfg(test)]
pub(crate) fn footer_keys_text() -> &'static str {
    "[ ] resize \u{00b7} / cycle \u{00b7} = reset \u{00b7} wheel scroll \u{00b7} click select \u{00b7} click severity bulk hide \u{00b7} click version git \u{00b7} \u{2191}/\u{2193} item \u{00b7} PgUp/PgDn page \u{00b7} Home/End \u{00b7} Tab switch \u{00b7} t target \u{00b7} u runtime \u{00b7} s snapshot \u{00b7} y copy \u{00b7} c clear \u{00b7} p accept \u{00b7} d dismiss \u{00b7} S settings \u{00b7} P provider-setup \u{00b7} H hover-help \u{00b7} L help-lang \u{00b7} n anomalies \u{00b7} a actions \u{00b7} i insights \u{00b7} ? help \u{00b7} q quit"
}

fn footer_compact_keys_text() -> &'static str {
    "\u{2191}/\u{2193} select \u{00b7} Enter/Space action \u{00b7} Tab focus \u{00b7} S settings \u{00b7} K keys \u{00b7} ? help \u{00b7} q quit"
}

/// Pure footer-line builder. Extracted from `render_footer` in v1.10.2
/// so the list of advertised keybindings can be unit-tested without
/// spinning up a buffer. The `focus` argument is the prefix (e.g.
/// `"focus: alerts"`) decided by the caller.
///
/// v1.39 update: includes the `★p:0 · ★y:0` placeholder slot so the
/// keybinding placement test still pins the order with the counter
/// chip in front of the key cluster (the live render uses
/// `footer_lines` with real counts and severity-colored spans).
#[cfg(test)]
fn footer_text(focus: &str, split: DashboardSplit) -> String {
    format!(
        "{focus} \u{00b7} split {}% \u{00b7} \u{2605}p:0 \u{00b7} \u{2605}y:0 \u{00b7} \u{2605}a:0\n{}",
        split.alerts_percent(),
        footer_compact_keys_text(),
    )
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup[1])[1]
}

#[cfg(test)]
fn help_lines() -> Vec<Line<'static>> {
    help_lines_for_width(usize::MAX)
}

fn help_lines_for_width(total_width: usize) -> Vec<Line<'static>> {
    // Help rows are wrapped before they reach Paragraph so every
    // continuation line keeps the same hanging indent under the
    // value column instead of re-flowing flush-left at the modal
    // border. That applies to ordinary detail rows and the `p`
    // bullet continuations alike.
    let accepted = AuditEventKind::PromptSendAccepted;
    let completed = AuditEventKind::PromptSendCompleted;
    let failed = AuditEventKind::PromptSendFailed;
    let blocked = AuditEventKind::PromptSendBlocked;
    let rejected = AuditEventKind::PromptSendRejected;

    let mut lines = vec![section_line("Controls")];
    for (label, value) in [
        (
            "Mouse wheel",
            "scroll the list, modal, settings read-only body, or settings fields under the pointer",
        ),
        (
            "Mouse left",
            "select the clicked alert, pane, target, tab, or settings field",
        ),
        ("Mouse double", "toggle hide on the clicked alert"),
        (
            "Mouse drag",
            "drag the divider between Alerts and Panes to resize them; drag a large overlay title row to move the modal (a/i); drag the a overlay's list/explainer separator to resize the split",
        ),
        (
            "Severity chip",
            "click a bulk chip in Alerts to toggle auto-hide for that severity",
        ),
        (
            "Version badge",
            "click the bottom-right version to open Git status",
        ),
        (
            "Modal [x]",
            "every overlay (Help / Settings / Provider Setup / Pending Actions / Token Insights / Git / Action Explainer / Target picker) has an [x] click target in the top-right corner",
        ),
        (
            "Overlay chrome",
            "scrollable overlays use title for identity and footer/hint for controls plus scroll x/y · more/END; large overlays (a/i) use [/] resize, = reset geometry, title drag move, body-wheel/↑/↓ scroll, same entry key/Esc/q/[x] close; S edit mode and short confirmation modals are exceptions",
        ),
        ("Tab", "switch focus between alerts and pane list"),
        ("Up / Down", "move one item in the focused list"),
        ("j / k", "alternate list scroll keys"),
        ("PgUp / PgDn", "scroll one page in the focused list"),
        ("Home / End", "jump to the first or last item"),
        (
            "[ and ]",
            "shrink or grow Alerts (Panes uses remaining height); when a large overlay is open (a/i), resize that modal in 5% steps",
        ),
        (
            "/",
            "Alerts focused: open the alert filter input (Esc clears, Enter exits input keeping filter, Backspace edits, / restarts; case-insensitive substring against title/headline/details/suggested-command); Panes focused: cycle the Alerts/Panes split by one resize step",
        ),
        (
            "=",
            "reset the Alerts/Panes split; when a large overlay is open (a/i), also reset modal geometry to its default size + position",
        ),
        (
            "t",
            "open tmux target picker (session -> window); hint shows scroll x/y · more/END for the list; press t again, click [x], or Esc to close",
        ),
        (
            "a",
            "open Pending Actions overlay (live explainer + multi-select bulk dispatch + size/ratio adjustability): lists every pane with a pending p/d proposal AND every alert with a y-copyable command; hint shows scroll x/y · more/END; Space=toggle selection, P/Y/A=group toggle, c=clear; p/d/y dispatch selected items bypassing confirm_actions; [/] resize modal, ,/. resize list pane, = reset all geometry; drag title row to move, drag separator to resize ratio; a again or Esc/q/[x] close",
        ),
        (
            "i",
            "open the resizable Token Insights overlay for the configured insight ledger window; [/] resize, = reset geometry, title drag move, r refreshes while open, body-wheel/Up/Down scroll, i again or Esc/q/[x] close",
        ),
        ("Space", "toggle multi-select on cursor item (a overlay)"),
        (
            "P / Y / A",
            "toggle proposal / alert / all group selection (a overlay)",
        ),
        (
            "c (a overlay)",
            "clear multi-select set, cursor stays (a overlay)",
        ),
        (
            "Enter",
            "advance session selection or confirm window target",
        ),
        (
            "Left / Backspace",
            "return from window list to session list",
        ),
        ("s", "write a runtime snapshot"),
        ("r", "refresh version drift check"),
        (
            "u",
            "request provider runtime status for the selected pane via its read-only slash command",
        ),
        (
            "H",
            "toggle floating hover help for Alerts and Panes; by default it opens near the front label area, and [ux] hover_help_trigger can restore full-row hover",
        ),
        (
            "L",
            "toggle floating help language between ko and en; the setting is also editable as [ux] help_language in Settings > Parameters",
        ),
        (
            "y",
            "copy the selected alert run command to the clipboard; [ux] confirm_actions controls Action Explainer modal: always = every time, first_time = once per kind per session, never = skipped",
        ),
        ("c", "clear system notices"),
    ] {
        lines.extend(help_wrapped_detail_lines(label, value, total_width));
    }

    lines.extend(help_wrapped_detail_lines(
        "p",
        "accept pending prompt-send proposal (P5-3 safer-actuation); [ux] confirm_actions controls Action Explainer modal: always = every time, first_time = once per kind per session, never = skipped (Enter confirms, Esc / p / [x] click cancels)",
        total_width,
    ));
    lines.extend(help_wrapped_bullet_lines(
        &format!(
            "Execute (allow_auto_prompt_send=true, non-observe_only): {accepted} → {completed} or {failed}"
        ),
        total_width,
    ));
    lines.extend(help_wrapped_bullet_lines(
        &format!(
            "AutoSendOff (allow_auto_prompt_send=false, non-observe_only): {accepted} + {blocked}"
        ),
        total_width,
    ));
    lines.extend(help_wrapped_bullet_lines(
        &format!("observe_only: {blocked} alone (no {accepted})"),
        total_width,
    ));

    lines.extend(help_wrapped_detail_lines(
        "d",
        &format!("dismiss pending prompt-send proposal (audit: {rejected}; every actuation mode); [ux] confirm_actions controls Action Explainer modal: always = every time, first_time = once per kind per session, never = skipped"),
        total_width,
    ));

    lines.extend(help_wrapped_detail_lines(
        "S",
        "open the settings overlay (Thresholds / Integrations / Parameters / Rules / Badges; includes [insights], [anomaly], [reset], and [provider_setup] surfaces); Thresholds/Integrations use ↑/↓/wheel for selection, read-only tabs use ↑/↓/j/k/wheel/PgUp/PgDn/Home/End for body scroll; press 'w' inside to write back to the loaded TOML; press S again (when not editing), Esc, q, or click [x] to close",
        total_width,
    ));

    lines.extend(help_wrapped_detail_lines(
        "P",
        "open the Provider Setup overlay (read-only) — recommended Claude statusline.sh / Codex /statusline + /status / Gemini ui.footer.* / tmux launcher snippets with detected current state; 1/2/3/4, Tab, or Left/Right switch tabs; integration options are edited in S Settings > Integrations; press P again, Esc, q, or click [x] to close",
        total_width,
    ));

    for (label, value) in [("Esc / ?", "close this help"), ("q", "quit the TUI")] {
        lines.extend(help_wrapped_detail_lines(label, value, total_width));
    }

    lines.push(Line::raw(""));
    lines.push(section_line("Hover Help"));
    for (label, value) in [
        (
            "Scope",
            "Alerts and Panes rows expose floating explanations for headers, dismiss, summary, details, copy hints, pane state, path, cmd, status, signals, metrics, token/cache rows, runtime facts, and recommendations/profile payloads; label trigger opens only near the front row label, row trigger restores full-row hover; explicit overlays hide dashboard hover help",
        ),
        (
            "Small terminal",
            "if a floating card would clip or the terminal is narrow, the same help opens as a bottom drawer and wraps the content instead of cutting text",
        ),
        (
            "Settings",
            "S > Parameters shows a Selected parameter help block for the selected field with TOML key, current/default value, meaning, accepted values, shortcut, and save note; ux.hover_help_trigger accepts label or row",
        ),
        (
            "Keys",
            "H toggles hover help; L switches ko/en; inside S > Parameters the same keys edit ux.hover_help and ux.help_language until w writes the loaded TOML",
        ),
    ] {
        lines.extend(help_wrapped_detail_lines(label, value, total_width));
    }

    lines.push(Line::raw(""));
    lines.push(section_line("Source Labels"));
    lines.extend(
        labels::source_kind_legend()
            .into_iter()
            .map(split_once_space)
            .flat_map(|(label, value)| help_wrapped_detail_lines(label, value, total_width)),
    );

    lines.push(Line::raw(""));
    lines.push(section_line("State Labels"));
    lines.extend(
        labels::signal_legend()
            .into_iter()
            .map(split_once_colon)
            .flat_map(|(label, value)| help_wrapped_detail_lines(label, value, total_width)),
    );
    lines
}

fn section_line(title: &str) -> Line<'static> {
    Line::styled(
        title.to_string(),
        Style::default()
            .fg(theme::text_primary())
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )
}

/// 2-space indent applied to every detail row under a help section
/// header so the rows visibly nest under "Controls" / "Source Labels"
/// / "State Labels" instead of sitting flush against the modal's
/// left border (v1.10.6 UX polish).
const HELP_DETAIL_INDENT: &str = "  ";

/// Hanging-indent prefix used on continuation bullets (v1.10.8 —
/// operator asked for deeper bullet indent). 20 spaces place the
/// `·` marker at column 20 (same column as the `:` in the summary
/// row of a detail entry — HELP_DETAIL_INDENT (2) + HELP_LABEL_WIDTH
/// (18) = 20), so the bullet text starts at column 22 and hangs
/// visibly under the value text of the `p` summary row. Distinct
/// from HELP_DETAIL_INDENT so the reader can tell a bullet belongs
/// to the entry immediately above it rather than being a new
/// top-level row. The `help_continuation_prefix_hangs_bullet_under_
/// value_column` test locks this alignment against drift.
const HELP_CONTINUATION_PREFIX: &str = "                    · ";

fn help_body_text_width(body: Rect) -> usize {
    body.width.saturating_sub(2) as usize
}

/// v1.60.0: line index of the start of every help-modal section, in
/// document order. Used by the section-jump keys in `tui_loop` so
/// `1..=N` scrolls the modal to the top of section N. Returns at
/// least one entry (Controls at index 0); empty Vec only if the help
/// body is empty.
pub fn help_section_line_indices(viewport: Rect) -> Vec<usize> {
    let rects = help_modal_rects(viewport);
    let lines = help_lines_for_width(help_body_text_width(rects.body));
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if line_is_section_header(line) {
            out.push(idx);
        }
    }
    out
}

fn line_is_section_header(line: &Line<'_>) -> bool {
    // `section_line` builds the header via `Line::styled(title, style)`,
    // which applies the style to the Line itself (`line.style`). Fall
    // back to the first span's style for older builds.
    let line_style = line
        .style
        .add_modifier
        .contains(Modifier::BOLD | Modifier::UNDERLINED);
    let span_style = line
        .spans
        .first()
        .map(|s| {
            s.style
                .add_modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED)
        })
        .unwrap_or(false);
    line_style || span_style
}

fn help_value_column() -> usize {
    HELP_DETAIL_INDENT.chars().count() + HELP_LABEL_WIDTH + ": ".chars().count()
}

fn help_value_continuation_prefix() -> String {
    " ".repeat(help_value_column())
}

fn help_wrapped_detail_lines(label: &str, value: &str, total_width: usize) -> Vec<Line<'static>> {
    let content_width = total_width.saturating_sub(help_value_column()).max(8);
    let wrapped = help_wrap_text(value, content_width);
    let mut out = Vec::with_capacity(wrapped.len());
    for (idx, chunk) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            out.push(help_detail_line(label, &chunk));
        } else {
            out.push(help_value_continuation_line(&chunk));
        }
    }
    out
}

fn help_wrapped_bullet_lines(text: &str, total_width: usize) -> Vec<Line<'static>> {
    let content_width = total_width
        .saturating_sub(HELP_CONTINUATION_PREFIX.chars().count())
        .max(8);
    let wrapped = help_wrap_text(text, content_width);
    let mut out = Vec::with_capacity(wrapped.len());
    for (idx, chunk) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            out.push(help_continuation_bullet(&chunk));
        } else {
            out.push(help_value_continuation_line(&chunk));
        }
    }
    out
}

fn help_continuation_bullet(text: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            HELP_CONTINUATION_PREFIX,
            Style::default().fg(theme::text_dim()),
        ),
        Span::styled(text.to_string(), Style::default().fg(theme::text_dim())),
    ])
}

fn help_value_continuation_line(text: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            help_value_continuation_prefix(),
            Style::default().fg(theme::text_dim()),
        ),
        Span::styled(text.to_string(), Style::default().fg(theme::text_dim())),
    ])
}

fn help_detail_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw(HELP_DETAIL_INDENT),
        Span::styled(
            format!("{label:<HELP_LABEL_WIDTH$}"),
            Style::default()
                .fg(theme::text_primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": ", Style::default().fg(theme::text_dim())),
        Span::styled(value.to_string(), Style::default().fg(theme::text_dim())),
    ])
}

fn split_once_space(line: &'static str) -> (&'static str, &'static str) {
    line.split_once(' ').unwrap_or((line, ""))
}

fn split_once_colon(line: &'static str) -> (&'static str, &'static str) {
    line.split_once(": ").unwrap_or((line, ""))
}

fn help_wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut out = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if word.chars().count() > width {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            out.extend(help_split_long_word(word, width));
            continue;
        }

        if current.is_empty() {
            current.push_str(word);
            continue;
        }

        let next_len = current.chars().count() + 1 + word.chars().count();
        if next_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }

    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn help_split_long_word(word: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut chunk = String::new();
    for ch in word.chars() {
        chunk.push(ch);
        if chunk.chars().count() == width {
            out.push(std::mem::take(&mut chunk));
        }
    }
    if !chunk.is_empty() {
        out.push(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: Line<'static>) -> String {
        line.spans
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>()
    }

    #[test]
    fn help_detail_line_aligns_label_and_description() {
        let text = line_text(help_detail_line("Tab", "switch focus"));
        // v1.10.6: detail rows carry a 2-space indent so they nest
        // visibly under section headers in the help overlay. Label
        // now starts at column HELP_DETAIL_INDENT.len().
        assert!(
            text.starts_with("  Tab"),
            "detail row must start with the 2-space indent followed by the label. got: {text:?}"
        );
        assert!(text.contains(": switch focus"));
    }

    #[test]
    fn help_continuation_prefix_hangs_bullet_under_value_column() {
        // v1.10.8 bullet-indent invariant. Summary row layout:
        //   cols 0-1:   HELP_DETAIL_INDENT (2 spaces)
        //   cols 2-19:  padded label (HELP_LABEL_WIDTH = 18)
        //   col  20:    `:`
        //   col  21:    ` ` (separator)
        //   col  22+:   value text (the "value column")
        // Continuation bullet must place its `·` at col 20 (same col
        // as the `:` of the summary row) so the bullet text then
        // starts at col 22 — visually hanging directly under the
        // value of the `p` entry.
        //
        // Therefore the full prefix (leading whitespace + `·` + ` `)
        // consumes exactly value_column characters before the
        // bullet text begins.
        let value_column = help_value_column();
        let prefix_chars = HELP_CONTINUATION_PREFIX.chars().count();
        assert_eq!(
            prefix_chars, value_column,
            "HELP_CONTINUATION_PREFIX must reach exactly the value column so bullet text starts aligned. got prefix_chars={prefix_chars}, value_column={value_column}"
        );
        // The character at col value_column - 2 (= col 20, same col
        // as the `:` of the summary row) must be the bullet itself —
        // not whitespace — so the visual alignment is intentional
        // rather than accidental.
        let bullet_col = value_column - 2;
        let bullet_char = HELP_CONTINUATION_PREFIX
            .chars()
            .nth(bullet_col)
            .expect("prefix must reach the bullet column");
        assert_eq!(
            bullet_char, '·',
            "bullet marker must sit at column {bullet_col} (aligned with the `:` of the summary row)"
        );
    }

    #[test]
    fn help_detail_line_indent_is_stable_across_rows() {
        // Lock the indent so a future refactor can't silently change
        // the column alignment. Section headers stay flush left (no
        // indent) by construction — section_line returns Line::styled
        // with just the title, which has no leading space.
        for (label, value) in [
            ("t", "open target picker"),
            ("s", "snapshot"),
            ("Mouse wheel", "scroll"),
        ] {
            let text = line_text(help_detail_line(label, value));
            assert!(
                text.starts_with("  "),
                "every detail row starts with the 2-space indent. got: {text:?}"
            );
        }
    }

    #[test]
    fn wrapped_help_detail_lines_keep_continuations_under_value_column() {
        let lines: Vec<String> = help_wrapped_detail_lines(
            "Severity chip",
            "click a bulk chip in Alerts to toggle auto-hide for that severity",
            48,
        )
        .into_iter()
        .map(line_text)
        .collect();
        let continuation_prefix = help_value_continuation_prefix();
        assert!(lines.len() > 1, "expected wrapped help row: {lines:?}");
        assert!(
            lines[1].starts_with(&continuation_prefix),
            "wrapped continuation must align with the value column. got: {lines:?}"
        );
    }

    #[test]
    fn wrapped_help_bullets_keep_continuations_under_value_column() {
        let lines: Vec<String> = help_wrapped_bullet_lines(
            "Execute (allow_auto_prompt_send=true, non-observe_only): PromptSendAccepted → PromptSendCompleted or PromptSendFailed",
            56,
        )
        .into_iter()
        .map(line_text)
        .collect();
        let continuation_prefix = help_value_continuation_prefix();
        assert!(lines.len() > 1, "expected wrapped help bullet: {lines:?}");
        assert!(
            lines[0].starts_with(HELP_CONTINUATION_PREFIX),
            "first bullet line must keep the bullet prefix. got: {lines:?}"
        );
        assert!(
            lines[1].starts_with(&continuation_prefix),
            "wrapped bullet continuation must align with the value column. got: {lines:?}"
        );
    }

    #[test]
    fn version_badge_label_comes_from_build_embedded_git_version() {
        // v1.10.6: the footer label is no longer the Cargo package
        // version; `build.rs` resolves `git describe --tags --always
        // --dirty` and embeds it via QMONSTER_GIT_VERSION. Assert
        // that the runtime label equals that env var (set at build
        // time) and that it is non-empty — even in a git-less build
        // the fallback `v{pkg}-nogit` string is non-empty.
        let label = version_badge_label();
        assert!(!label.is_empty(), "version badge label must not be empty");
        assert_eq!(
            label,
            env!("QMONSTER_GIT_VERSION"),
            "footer version must come from QMONSTER_GIT_VERSION (set by build.rs), not CARGO_PKG_VERSION"
        );
    }

    #[test]
    fn help_scroll_budget_grows_when_viewport_shrinks() {
        let tall = max_help_scroll(Rect::new(0, 0, 120, 48));
        let short = max_help_scroll(Rect::new(0, 0, 120, 16));
        assert!(short >= tall);
    }

    #[test]
    fn help_scroll_budget_grows_when_viewport_narrows() {
        let wide = max_help_scroll(Rect::new(0, 0, 120, 24));
        let narrow = max_help_scroll(Rect::new(0, 0, 80, 24));
        assert!(narrow >= wide);
    }

    #[test]
    fn help_modal_documents_hover_help_and_settings_context() {
        let text = help_lines()
            .into_iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            text.contains("Hover Help"),
            "help modal must include a dedicated hover-help section: {text}"
        );
        assert!(
            text.contains("bottom drawer"),
            "help modal must explain the small-terminal hover help fallback: {text}"
        );
        assert!(
            text.contains("Selected parameter help"),
            "help modal must point operators to the Settings parameter explainer: {text}"
        );
        assert!(
            text.contains("H") && text.contains("L"),
            "help modal must mention hover help toggle and language keys: {text}"
        );
    }

    #[test]
    fn help_section_line_indices_returns_at_least_four_sections_in_order() {
        let viewport = Rect::new(0, 0, 120, 60);
        let indices = help_section_line_indices(viewport);
        assert!(
            indices.len() >= 4,
            "expected at least 4 sections (Controls / Hover Help / Source Labels / State Labels); got {}",
            indices.len()
        );
        // Indices must be strictly increasing — sections are emitted
        // in document order.
        for window in indices.windows(2) {
            assert!(
                window[0] < window[1],
                "section indices must be strictly increasing: {indices:?}"
            );
        }
        // First section ('Controls') is the first line of the help body.
        assert_eq!(
            indices.first().copied(),
            Some(0),
            "first section should sit at line 0"
        );
    }

    #[test]
    fn help_section_line_indices_targets_correspond_to_section_headers() {
        let viewport = Rect::new(0, 0, 120, 60);
        let indices = help_section_line_indices(viewport);
        let rects = help_modal_rects(viewport);
        let lines = help_lines_for_width(help_body_text_width(rects.body));
        let expected_titles = ["Controls", "Hover Help", "Source Labels", "State Labels"];
        for (idx, expected) in indices.iter().zip(expected_titles.iter()) {
            let actual = lines[*idx]
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>();
            assert!(
                actual.contains(expected),
                "expected section at line {idx} to contain {expected:?}; got {actual:?}"
            );
        }
    }

    #[test]
    fn scrollable_modal_hint_reports_progress_and_end() {
        assert_eq!(
            scrollable_modal_hint("close", 0, 3),
            "close · scroll 0/3 · more"
        );
        assert_eq!(
            scrollable_modal_hint("close", 3, 3),
            "close · scroll 3/3 · END"
        );
    }

    #[test]
    fn target_picker_hint_reports_list_progress_and_end() {
        assert_eq!(
            target_picker_hint_line("close", 1, 4),
            "close · scroll 1/3 · more"
        );
        assert_eq!(
            target_picker_hint_line("close", 3, 4),
            "close · scroll 3/3 · END"
        );
        assert_eq!(
            target_picker_hint_line("close", 9, 4),
            "close · scroll 3/3 · END"
        );
        assert_eq!(
            target_picker_hint_line("close", 0, 0),
            "close · scroll 0/0 · END"
        );
    }

    #[test]
    fn version_badge_hugs_bottom_right_edge() {
        let area = Rect::new(4, 6, 40, 2);
        let badge = version_badge_rect(area);
        assert_eq!(badge.y, area.y + area.height - 1);
        assert_eq!(badge.x + badge.width, area.x + area.width);
    }

    #[test]
    fn footer_keys_badge_hugs_bottom_left_edge() {
        let area = Rect::new(4, 6, 40, 2);
        let badge = footer_keys_badge_rect(area);
        assert_eq!(badge.x, area.x);
        assert_eq!(badge.y, area.y + area.height - 1);
        assert!(badge.width >= footer_keys_badge_label().chars().count() as u16);
    }

    #[test]
    fn split_divider_renders_normal_resize_hint_when_ime_inactive() {
        // v1.51.0: regression guard — divider must keep showing the
        // existing "drag resize alerts/panes …" hint when IME is off.
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        render_split_divider(area, &mut buf, DashboardSplit::default(), false);
        let dump: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(
            dump.contains("drag resize alerts/panes"),
            "normal divider text missing: {dump}"
        );
        assert!(
            !dump.contains("HANGUL/IME ACTIVE"),
            "IME banner should not render when inactive: {dump}"
        );
    }

    #[test]
    fn split_divider_renders_ime_warning_when_active() {
        // v1.51.0: when the heuristic IME indicator is active, the
        // divider replaces the resize hint with a HANGUL/IME ACTIVE
        // banner so a Korean/CJK operator notices their command keys
        // are being composed away by the OS-level IME.
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        render_split_divider(area, &mut buf, DashboardSplit::default(), true);
        let dump: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(
            dump.contains("HANGUL/IME ACTIVE"),
            "IME banner missing: {dump}"
        );
        assert!(
            !dump.contains("drag resize alerts/panes"),
            "normal divider text should be suppressed when IME is active: {dump}"
        );
    }

    #[test]
    fn dashboard_rects_include_draggable_divider_between_lists() {
        let area = Rect::new(0, 0, 120, 32);
        let rects = dashboard_rects(area, DashboardSplit::default());

        assert_eq!(rects.footer.height, DASHBOARD_FOOTER_HEIGHT);
        assert_eq!(rects.divider.height, DASHBOARD_SPLIT_HANDLE_HEIGHT);
        assert_eq!(rects.divider.y, rects.alerts.y + rects.alerts.height);
        assert_eq!(rects.panes.y, rects.divider.y + rects.divider.height);
        assert_eq!(rects.footer.y + rects.footer.height, area.y + area.height);
        assert!(rects.alerts.height >= MIN_ALERTS_HEIGHT);
        assert!(rects.panes.height >= MIN_PANES_HEIGHT);
    }

    #[test]
    fn dashboard_split_from_row_maps_drag_position_to_percent() {
        let area = Rect::new(0, 0, 120, 42);
        let high_alerts = dashboard_split_from_row(area, 30);
        let low_alerts = dashboard_split_from_row(area, 8);

        assert!(high_alerts.alerts_percent() > low_alerts.alerts_percent());
        assert_eq!(
            dashboard_split_from_row(area, 0).alerts_percent(),
            MIN_ALERTS_PERCENT
        );
        assert_eq!(
            dashboard_split_from_row(area, 100).alerts_percent(),
            MAX_ALERTS_PERCENT
        );
    }

    #[test]
    fn dashboard_split_keys_step_and_clamp() {
        let mut split = DashboardSplit::new(DEFAULT_ALERTS_PERCENT);
        split.grow_alerts();
        assert_eq!(
            split.alerts_percent(),
            DEFAULT_ALERTS_PERCENT + RESIZE_STEP_PERCENT
        );
        split.shrink_alerts();
        assert_eq!(split.alerts_percent(), DEFAULT_ALERTS_PERCENT);

        for _ in 0..20 {
            split.shrink_alerts();
        }
        assert_eq!(split.alerts_percent(), MIN_ALERTS_PERCENT);

        split.reset();
        assert_eq!(split, DashboardSplit::default());
    }

    #[test]
    fn dashboard_split_slash_cycle_advances_and_wraps() {
        let mut split = DashboardSplit::new(DEFAULT_ALERTS_PERCENT);
        split.cycle_alerts();
        assert_eq!(
            split.alerts_percent(),
            DEFAULT_ALERTS_PERCENT + RESIZE_STEP_PERCENT
        );

        let mut split = DashboardSplit::new(MAX_ALERTS_PERCENT);
        split.cycle_alerts();
        assert_eq!(split.alerts_percent(), MIN_ALERTS_PERCENT);
    }

    #[test]
    fn footer_text_advertises_prompt_send_keys() {
        // v1.10.2 polish (Codex v1.9.2 / v1.10.0 follow-up): the
        // global footer must advertise `p` (accept) and `d` (dismiss)
        // alongside the other single-letter keys so operators notice
        // the P5-3 actuation surface without having to open the help
        // overlay. v1.10.3 tightening (Codex v1.10.2 §8): also pin the
        // ordering — the two actuation keys should sit between
        // `t target` and `? help` so they stay adjacent to target
        // selection and immediately before the generic help/quit tail.
        let text = footer_keys_text();
        assert!(
            text.contains("p accept"),
            "footer must advertise `p accept`: {text}"
        );
        assert!(
            text.contains("d dismiss"),
            "footer must advertise `d dismiss`: {text}"
        );
        assert!(
            text.contains("u runtime"),
            "footer must advertise `u runtime`: {text}"
        );
        assert!(
            text.contains("y copy"),
            "footer must advertise `y copy`: {text}"
        );
        assert!(
            text.contains("c clear"),
            "footer must advertise `c clear`: {text}"
        );
        assert!(
            text.contains("[ ] resize"),
            "footer must advertise split resize keys: {text}"
        );
        assert!(
            text.contains("/ cycle"),
            "footer must advertise slash split cycle key: {text}"
        );
        assert!(
            text.contains("= reset"),
            "footer must advertise split reset key: {text}"
        );
        // v1.15.18: settings overlay key.
        assert!(
            text.contains("S settings"),
            "footer must advertise the `S settings` overlay key: {text}"
        );
        // v1.29.0 G-1: provider setup overlay key.
        assert!(
            text.contains("P provider-setup"),
            "footer must advertise the `P provider-setup` overlay key: {text}"
        );
        assert!(
            text.contains("H hover-help"),
            "footer must advertise the hover-help toggle key: {text}"
        );
        assert!(
            text.contains("L help-lang"),
            "footer must advertise the hover-help language key: {text}"
        );
        assert!(
            text.contains("s snapshot"),
            "footer must advertise the `s snapshot` key: {text}"
        );
        assert!(
            text.contains("n anomalies"),
            "footer must advertise the `n anomalies` overlay key: {text}"
        );
        // Sanity: existing anchors still present.
        assert!(text.contains("? help"));
        assert!(text.contains("q quit"));
        // Placement contract: t target → y copy → c clear → p accept → d dismiss → S settings → P provider-setup → H/L hover help → ? help.
        let target_pos = text
            .find("t target")
            .expect("footer must keep the `t target` anchor");
        let p_pos = text.find("p accept").expect("footer must carry `p accept`");
        let u_pos = text
            .find("u runtime")
            .expect("footer must carry `u runtime`");
        let y_pos = text.find("y copy").expect("footer must carry `y copy`");
        let c_pos = text.find("c clear").expect("footer must carry `c clear`");
        let d_pos = text
            .find("d dismiss")
            .expect("footer must carry `d dismiss`");
        let s_pos = text
            .find("S settings")
            .expect("footer must carry `S settings`");
        let p_provider_pos = text
            .find("P provider-setup")
            .expect("footer must carry `P provider-setup`");
        let hover_pos = text
            .find("H hover-help")
            .expect("footer must carry `H hover-help`");
        let lang_pos = text
            .find("L help-lang")
            .expect("footer must carry `L help-lang`");
        let anomalies_pos = text
            .find("n anomalies")
            .expect("footer must carry `n anomalies`");
        let actions_pos = text
            .find("a actions")
            .expect("footer must carry `a actions`");
        let insights_pos = text
            .find("i insights")
            .expect("footer must carry `i insights`");
        let help_pos = text
            .find("? help")
            .expect("footer must keep the `? help` anchor");
        assert!(
            target_pos < u_pos,
            "`u runtime` must come after `t target` (provider refresh near target selection)"
        );
        assert!(
            u_pos < p_pos,
            "`u runtime` must precede command copy + prompt-send actuation keys"
        );
        assert!(
            y_pos > u_pos && y_pos < c_pos,
            "`y copy` must sit between runtime refresh and notice-clear"
        );
        assert!(
            c_pos < p_pos,
            "`c clear` must precede prompt-send actuation keys"
        );
        assert!(
            p_pos < d_pos,
            "`p accept` must precede `d dismiss` (alphabetical / accept-before-dismiss)"
        );
        assert!(
            d_pos < s_pos,
            "`S settings` must follow the prompt-send actuation pair"
        );
        assert!(
            s_pos < p_provider_pos,
            "`P provider-setup` must follow `S settings` (overlay-opener pair)"
        );
        assert!(
            p_provider_pos < hover_pos && hover_pos < lang_pos,
            "hover help keys must follow provider setup"
        );
        assert!(
            lang_pos < anomalies_pos,
            "hover help keys must precede the operational overlay cluster"
        );
        assert!(
            anomalies_pos < actions_pos && actions_pos < insights_pos,
            "overlay cluster must stay n anomalies -> a actions -> i insights"
        );
        assert!(
            insights_pos < help_pos,
            "overlay-opener keys must precede `? help` (generic tail)"
        );
    }

    #[test]
    fn compact_footer_keeps_detailed_keys_out_of_main_row() {
        let text = footer_text("focus: alerts", DashboardSplit::default());

        assert!(
            text.contains("keys"),
            "compact footer should expose the keys badge: {text}"
        );
        assert!(
            text.contains("K keys"),
            "compact footer should advertise the key legend shortcut: {text}"
        );
        assert!(
            text.contains("? help"),
            "compact footer should keep the full help entry: {text}"
        );
        assert!(
            !text.contains("click severity bulk hide"),
            "long key reference belongs in the keys hover legend, not the main footer: {text}"
        );
    }

    #[test]
    fn compact_footer_uses_status_row_plus_key_row() {
        let counters = FooterCounters {
            proposal_count: 0,
            proposal_top_severity: None,
            copy_count: 0,
            copy_top_severity: None,
            audit_top_severity: None,
        };
        let lines = footer_lines("focus: alerts", DashboardSplit::default(), &counters);

        assert_eq!(
            lines.len(),
            2,
            "compact footer should keep a balanced 2-row footprint"
        );
        let status_row = footer_line_text(&lines[0]);
        let key_row = footer_line_text(&lines[1]);

        assert!(status_row.contains("focus: alerts"));
        assert!(status_row.contains("split 36%"));
        assert!(status_row.contains("\u{2605}p:0"));
        assert!(status_row.contains("\u{2605}y:0"));
        assert!(status_row.contains("\u{2605}a:0"));
        assert!(
            !status_row.contains("K keys"),
            "key hints belong on the second row: {status_row}"
        );
        assert!(key_row.contains("K keys"));
        assert!(
            !key_row.contains("focus:"),
            "status text belongs on the first row: {key_row}"
        );
    }

    fn pending_actions_pane_report(
        with_proposal: bool,
        recommendations: Vec<crate::domain::recommendation::Recommendation>,
    ) -> PaneReport {
        use crate::domain::identity::{
            IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
        };
        use crate::domain::recommendation::RequestedEffect;
        use crate::domain::signal::SignalSet;
        let pane_id = "%1".to_string();
        let mut effects = Vec::new();
        if with_proposal {
            effects.push(RequestedEffect::PromptSendProposed {
                target_pane_id: pane_id.clone(),
                slash_command: "/compact".into(),
                proposal_id: "pid-1".into(),
            });
        }
        PaneReport {
            pane_id: pane_id.clone(),
            session_name: "qwork".into(),
            window_index: "1".into(),
            provider: Provider::Claude,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Claude,
                    instance: 1,
                    role: Role::Main,
                    pane_id: pane_id.clone(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations,
            effects,
            dead: false,
            current_path: "/repo".into(),
            worktree_role: None,
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
            recent_token_samples: Vec::new(),
            anomalies: vec![],
        }
    }

    fn footer_line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn footer_shows_pending_proposal_counter_when_present() {
        // v1.39 surface B: footer must surface a `★p:N` chip whose
        // count tracks panes carrying a pending PromptSendProposed
        // effect. Exercises the live span path used by render_footer.
        let rep_with = pending_actions_pane_report(true, vec![]);
        let rep_without = pending_actions_pane_report(false, vec![]);
        let reports = vec![rep_with, rep_without];
        let (count, top) = pending_proposal_summary(&reports);
        assert_eq!(count, 1, "exactly one pane carries a proposal");
        assert!(top.is_none(), "no recommendations => no severity color");

        let counters = FooterCounters {
            proposal_count: count,
            proposal_top_severity: top,
            copy_count: 0,
            copy_top_severity: None,
            audit_top_severity: None,
        };
        let lines = footer_lines("focus: alerts", DashboardSplit::default(), &counters);
        let dump = footer_line_text(&lines[0]);
        assert!(
            dump.contains("\u{2605}p:1"),
            "footer must count pending proposals; got: {dump}"
        );
    }

    #[test]
    fn footer_shows_copyable_alert_counter_when_present() {
        // v1.39 surface B: footer `★y:N` chip tracks alerts whose
        // suggested_command is Some — the same set the operator can
        // `y`-copy.
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::{Recommendation, Severity};
        let rec = Recommendation {
            action: "context-pressure",
            reason: "near limit".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/clear".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
        };
        let rep = pending_actions_pane_report(false, vec![rec]);
        let reports = vec![rep];
        let (count, top) =
            alerts::copy_alert_count(&[], &reports, &HashMap::new(), std::time::Instant::now());
        assert_eq!(count, 1);
        assert_eq!(top, Some(crate::domain::recommendation::Severity::Warning));

        let counters = FooterCounters {
            proposal_count: 0,
            proposal_top_severity: None,
            copy_count: count,
            copy_top_severity: top,
            audit_top_severity: None,
        };
        let lines = footer_lines("focus: alerts", DashboardSplit::default(), &counters);
        let dump = footer_line_text(&lines[0]);
        assert!(
            dump.contains("\u{2605}y:1"),
            "footer must count copyable alerts; got: {dump}"
        );
    }

    #[test]
    fn now_strip_prioritizes_wait_state() {
        use crate::domain::signal::IdleCause;
        let mut report = pending_actions_pane_report(false, vec![]);
        report.idle_state = Some(IdleCause::PermissionWait);

        let summary = now_strip_summary_for(&[report], &[], 1_700_000_000, 200.0);

        assert_eq!(summary.text, "%1 WAIT APPROVAL — Enter approve · q skip");
        assert_eq!(
            summary.severity,
            crate::domain::recommendation::Severity::Risk
        );
    }

    #[test]
    fn now_strip_labels_input_wait_distinctly() {
        use crate::domain::signal::IdleCause;
        let mut report = pending_actions_pane_report(false, vec![]);
        report.idle_state = Some(IdleCause::InputWait);

        let summary = now_strip_summary_for(&[report], &[], 1_700_000_000, 200.0);

        assert_eq!(summary.text, "%1 WAIT INPUT — respond in pane · q skip");
        assert_eq!(
            summary.severity,
            crate::domain::recommendation::Severity::Risk
        );
    }

    #[test]
    fn now_strip_prioritizes_risk_recommendation_after_waits() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::{Recommendation, Severity};
        let rec = Recommendation {
            action: "checkpoint",
            reason: "large context pressure".into(),
            severity: Severity::Risk,
            source_kind: SourceKind::Heuristic,
            suggested_command: Some("/compact".into()),
            side_effects: vec![],
            is_strong: true,
            next_step: None,
        };
        let report = pending_actions_pane_report(false, vec![rec]);

        let summary = now_strip_summary_for(&[report], &[], 1_700_000_000, 200.0);

        assert_eq!(
            summary.text,
            "%1 RISK: large context pressure — p send /compact · d dismiss"
        );
        assert_eq!(summary.severity, Severity::Risk);
    }

    #[test]
    fn now_strip_prioritizes_quota_pressure_after_risk() {
        use crate::domain::origin::SourceKind;
        use crate::domain::signal::MetricValue;
        let mut report = pending_actions_pane_report(false, vec![]);
        report.signals.quota_5h_pressure =
            Some(MetricValue::new(0.88, SourceKind::ProviderOfficial));

        let summary = now_strip_summary_for(&[report], &[], 1_700_000_000, 200.0);

        assert_eq!(summary.text, "%1 QUOTA 88%");
        assert_eq!(
            summary.severity,
            crate::domain::recommendation::Severity::Warning
        );
    }

    #[test]
    fn now_strip_prioritizes_recent_promoted_anomaly_after_quota() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        let report = pending_actions_pane_report(false, vec![]);
        let event = AnomalyEvent {
            timestamp: 1_700_000_000 - 30,
            pane_id: "%1".into(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "cost_usd rose".into(),
            evidence: Vec::new(),
        };

        let summary = now_strip_summary_for(&[report], &[event], 1_700_000_000, 200.0);

        assert_eq!(summary.text, "%1 ANOMALY CostSlope (high) — see Alerts");
        assert_eq!(summary.severity, Severity::Warning);
    }

    #[test]
    fn now_strip_healthy_reports_pane_count_and_last_anomaly_age() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
        use crate::domain::recommendation::Severity;
        let reports = vec![
            pending_actions_pane_report(false, vec![]),
            pending_actions_pane_report(false, vec![]),
        ];
        let event = AnomalyEvent {
            timestamp: 1_700_000_000 - 125,
            pane_id: "%1".into(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::Medium,
            severity: Severity::Concern,
            promoted: false,
            reason: "cost_usd rose".into(),
            evidence: Vec::new(),
        };

        let summary = now_strip_summary_for(&reports, &[event], 1_700_000_000, 200.0);

        assert_eq!(summary.text, "healthy · 2 panes · last anomaly 2m ago");
        assert_eq!(summary.severity, Severity::Good);
    }

    #[test]
    fn now_strip_empty_reports_is_healthy() {
        let summary = now_strip_summary_for(&[], &[], 1_700_000_000, 200.0);

        assert_eq!(summary.text, "healthy · 0 panes · last anomaly none");
        assert_eq!(
            summary.severity,
            crate::domain::recommendation::Severity::Good
        );
    }

    #[test]
    fn footer_zero_counts_render_dim() {
        // v1.39 surface B: when nothing is pending, the chip still
        // renders with a `:0` count (dim styling) so the footer slot
        // doesn't shift between empty/non-empty states. The styling
        // itself is the dim TEXT_DIM color (asserted via the span
        // style; presence of `:0` proves the chip is in the line).
        let counters = FooterCounters {
            proposal_count: 0,
            proposal_top_severity: None,
            copy_count: 0,
            copy_top_severity: None,
            audit_top_severity: None,
        };
        let lines = footer_lines("focus: alerts", DashboardSplit::default(), &counters);
        let dump = footer_line_text(&lines[0]);
        assert!(
            dump.contains("\u{2605}p:0"),
            "footer must show ★p:0 when no proposals; got: {dump}"
        );
        assert!(
            dump.contains("\u{2605}y:0"),
            "footer must show ★y:0 when no copyables; got: {dump}"
        );
        assert!(
            dump.contains("\u{2605}a:0"),
            "footer must show ★a:0 when no recent audit severity; got: {dump}"
        );
        // Style: each chip is a Span with TEXT_DIM fg when count is 0.
        let p_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "\u{2605}p:0")
            .expect("★p:0 span must be present");
        assert_eq!(p_span.style.fg, Some(theme::text_dim()));
        let y_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "\u{2605}y:0")
            .expect("★y:0 span must be present");
        assert_eq!(y_span.style.fg, Some(theme::text_dim()));
        let a_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "\u{2605}a:0")
            .expect("★a:0 span must be present");
        assert_eq!(a_span.style.fg, Some(theme::text_dim()));
    }

    #[test]
    fn footer_nonzero_counts_render_severity_color() {
        // Lock the contract: ★p:N (N>0) renders in severity color
        // when the pane has a recommendation, falling back to
        // TEXT_PRIMARY when no severity is available.
        use crate::domain::recommendation::Severity;
        let counters = FooterCounters {
            proposal_count: 2,
            proposal_top_severity: Some(Severity::Risk),
            copy_count: 3,
            copy_top_severity: Some(Severity::Warning),
            audit_top_severity: Some(Severity::Warning),
        };
        let lines = footer_lines("focus: alerts", DashboardSplit::default(), &counters);
        let p_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "\u{2605}p:2")
            .expect("★p:2 span must be present");
        assert_eq!(p_span.style.fg, Some(theme::severity_color(Severity::Risk)));
        let y_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "\u{2605}y:3")
            .expect("★y:3 span must be present");
        assert_eq!(
            y_span.style.fg,
            Some(theme::severity_color(Severity::Warning))
        );
        let a_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "\u{2605}a:W")
            .expect("★a:W span must be present");
        assert_eq!(
            a_span.style.fg,
            Some(theme::severity_color(Severity::Warning))
        );
    }

    #[test]
    fn footer_audit_concern_renders_dim() {
        use crate::domain::recommendation::Severity;
        let counters = FooterCounters {
            proposal_count: 0,
            proposal_top_severity: None,
            copy_count: 0,
            copy_top_severity: None,
            audit_top_severity: Some(Severity::Concern),
        };
        let lines = footer_lines("focus: alerts", DashboardSplit::default(), &counters);
        let a_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "\u{2605}a:C")
            .expect("★a:C span must be present");
        assert_eq!(a_span.style.fg, Some(theme::text_dim()));
    }

    #[test]
    fn help_overlay_documents_dashboard_resize_controls() {
        let lines: Vec<String> = help_lines().into_iter().map(line_text).collect();
        let entry_for = |key: &str| -> Option<String> {
            lines.iter().find_map(|line| {
                let (label, value) = line.split_once(':')?;
                if label.trim() == key {
                    Some(value.trim().to_string())
                } else {
                    None
                }
            })
        };

        let drag = entry_for("Mouse drag").expect("help must document divider drag");
        let resize_keys = entry_for("[ and ]").expect("help must document resize keys");
        let slash_key = entry_for("/").expect("help must document slash cycle key");
        let reset_key = entry_for("=").expect("help must document split reset");

        assert!(drag.contains("divider"), "got: {drag}");
        assert!(
            resize_keys.contains("shrink or grow Alerts"),
            "got: {resize_keys}"
        );
        assert!(slash_key.contains("cycle"), "got: {slash_key}");
        assert!(reset_key.contains("reset"), "got: {reset_key}");
    }

    #[test]
    fn help_overlay_documents_p_and_d_prompt_send_actions() {
        // v1.10.2 polish: the `?` help overlay must describe both
        // the accept (`p`) and dismiss (`d`) paths, including the
        // P5-3 audit-event chain, so the operator can learn what
        // pressing each key will record without having to read the
        // mission ledger. Assertions look for the P5-3 kind names so
        // a renamed event kind in the future will surface here first.
        //
        // `help_detail_line` left-pads the label to DETAIL_LABEL_WIDTH,
        // so the rendered format is `"p   …   : accept …"` — we parse
        // each line on the first `:` and match the trimmed label.
        let lines: Vec<String> = help_lines().into_iter().map(line_text).collect();
        let entry_for = |key: &str| -> Option<String> {
            lines.iter().find_map(|line| {
                let (label, value) = line.split_once(':')?;
                if label.trim() == key {
                    Some(value.trim().to_string())
                } else {
                    None
                }
            })
        };
        // v1.10.7: the `p` entry is now multi-line — a short summary
        // row + three indented bullet continuations (one per
        // actuation-mode audit chain). The entry_for helper still
        // finds the summary row, but the audit-kind tokens now live
        // in the bullet lines, so we assert against the full joined
        // help text rather than a single row. `d` stays single-line.
        let p_entry = entry_for("p").expect("help overlay must carry a `p` summary row");
        let d_entry = entry_for("d").expect("help overlay must carry a `d` entry");
        let y_entry = entry_for("y").expect("help overlay must carry a `y` copy entry");
        let c_entry = entry_for("c").expect("help overlay must carry a `c` clear entry");
        assert!(
            c_entry.contains("clear") && c_entry.contains("system notices"),
            "the `c` entry must describe system-notice clear behavior. got: {c_entry}"
        );
        assert!(
            y_entry.contains("clipboard") && y_entry.contains("run command"),
            "the `y` entry must describe suggested-command clipboard copy. got: {y_entry}"
        );
        assert!(
            p_entry.contains("accept"),
            "the `p` summary row must describe accept semantics. got: {p_entry}"
        );

        let joined = lines.join("\n");

        // Audit-kind name assertions pulled from the AuditEventKind
        // enum so a variant rename auto-propagates into the help
        // text (v1.10.5 linkage) AND this test continues to pass.
        let accepted = AuditEventKind::PromptSendAccepted.as_str();
        let rejected = AuditEventKind::PromptSendRejected.as_str();
        let blocked = AuditEventKind::PromptSendBlocked.as_str();
        let completed = AuditEventKind::PromptSendCompleted.as_str();
        let failed = AuditEventKind::PromptSendFailed.as_str();
        assert!(
            joined.contains(accepted),
            "help overlay must name AuditEventKind::PromptSendAccepted ({accepted:?})"
        );
        assert!(
            joined.contains(blocked),
            "help overlay must mention AuditEventKind::PromptSendBlocked ({blocked:?})"
        );
        assert!(
            d_entry.contains(rejected),
            "the `d` entry must name AuditEventKind::PromptSendRejected ({rejected:?}). got: {d_entry}"
        );
        // v1.10.3 tightening + v1.10.7 bullet split: the help text
        // MUST describe all three audit outcomes distinctly so the
        // AutoSendOff branch is not confused with observe_only.
        // AutoSendOff is a two-event chain (Accepted + Blocked);
        // observe_only fires Blocked alone. The branch labels
        // (`AutoSendOff`, `observe_only`) are domain concepts named
        // by the spec rather than enum variant names.
        assert!(
            joined.contains("AutoSendOff"),
            "help overlay must name the AutoSendOff path so operators see that it fires TWO audit events ({accepted} + {blocked})"
        );
        assert!(
            joined.contains("observe_only"),
            "help overlay must name the observe_only path so operators see it fires {blocked} ALONE (no {accepted})"
        );
        // Both terminal outcomes on the Execute path must be
        // enumerated so operators know the audit log will carry one
        // of them per successful confirmation.
        assert!(
            joined.contains(completed),
            "help overlay must name AuditEventKind::PromptSendCompleted ({completed:?}) as the success terminal outcome on Execute"
        );
        assert!(
            joined.contains(failed),
            "help overlay must name AuditEventKind::PromptSendFailed ({failed:?}) as the failure terminal outcome on Execute"
        );

        // v1.10.7 bullet-indent contract: every continuation bullet
        // must start with HELP_CONTINUATION_PREFIX so the audit-
        // chain elaborations stay visually nested under the `p`
        // summary row even when the Paragraph does not wrap them.
        let bullet_count = lines
            .iter()
            .filter(|l| l.starts_with(HELP_CONTINUATION_PREFIX))
            .count();
        assert!(
            bullet_count >= 3,
            "the `p` summary row must be followed by at least 3 bullet-indented continuation lines (one per audit chain). got bullet_count = {bullet_count}. joined:\n{joined}"
        );
    }

    #[test]
    fn help_lists_a_pending_actions_overlay_key() {
        // v1.39: the Pending Actions overlay key was added so
        // operators can dispatch from anywhere without navigating to
        // the right pane/alert. Lock the description so future row
        // reorderings can't drop it.
        let lines: Vec<String> = help_lines().into_iter().map(line_text).collect();
        let dump = lines.join("\n");
        assert!(
            dump.contains("Pending Actions overlay"),
            "Help should list a -> Pending Actions overlay; got:\n{dump}"
        );
        assert!(
            dump.contains("a again or Esc/q/[x] close"),
            "Help a row should mention close affordances; got:\n{dump}"
        );
    }

    #[test]
    fn help_documents_action_explainer_for_pdy() {
        // v1.38: p / d / y now route through the Action Explainer
        // modal when `[ux] confirm_actions != never`. Help must call
        // out both the modal and the config key so operators can
        // toggle the gate.
        let lines: Vec<String> = help_lines().into_iter().map(line_text).collect();
        let dump = lines.join("\n");
        assert!(
            dump.contains("Action Explainer modal"),
            "Help should mention Action Explainer for p/d/y; got:\n{dump}"
        );
        assert!(
            dump.contains("[ux] confirm_actions"),
            "Help should reference [ux] confirm_actions"
        );
    }

    #[test]
    fn help_documents_modal_x_close() {
        // v1.38: every overlay carries an `[x]` mouse close target
        // in the top-right corner. Document it once in the Mouse
        // section so operators know the affordance is universal.
        let lines: Vec<String> = help_lines().into_iter().map(line_text).collect();
        let dump = lines.join("\n");
        assert!(
            dump.contains("Modal [x]") || dump.contains("[x] click target"),
            "Help should mention [x] click on modals; got:\n{dump}"
        );
        assert!(
            dump.contains("Token Insights"),
            "Help modal [x] row should include the i Token Insights overlay; got:\n{dump}"
        );
    }

    #[test]
    fn help_documents_overlay_chrome_contract() {
        let lines: Vec<String> = help_lines().into_iter().map(line_text).collect();
        let dump = lines.join("\n");
        let entry_for = |key: &str| -> String {
            lines
                .iter()
                .find_map(|line| {
                    let (label, value) = line.split_once(':')?;
                    if label.trim() == key {
                        Some(value.trim().to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("help row {key:?} missing from:\n{dump}"))
        };

        let overlay = entry_for("Overlay chrome");
        let insights = entry_for("i");

        assert!(
            overlay.contains("a/i")
                && overlay.contains("[/] resize")
                && overlay.contains("= reset geometry")
                && overlay.contains("title drag move")
                && overlay.contains("body-wheel"),
            "Help Overlay chrome row should scope the shared contract to a/i; got: {overlay}\nfull help:\n{dump}"
        );
        assert!(
            overlay.contains("exceptions"),
            "Help Overlay chrome row should mention exceptions briefly; got: {overlay}\nfull help:\n{dump}"
        );
        assert!(
            insights.contains("resizable Token Insights")
                && insights.contains("[/] resize")
                && insights.contains("= reset geometry")
                && insights.contains("title drag move")
                && insights.contains("body-wheel")
                && insights.contains("r refreshes"),
            "Help i row should document the resizable Token Insights contract; got: {insights}\nfull help:\n{dump}"
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn help_documents_toggle_close_on_S_P_t() {
        // v1.38 Tasks 6/7/8: pressing the opener key a second time
        // closes the matching overlay. Help rows for S / P / t must
        // mention the press-again-to-close affordance.
        let lines: Vec<String> = help_lines().into_iter().map(line_text).collect();
        let dump = lines.join("\n");
        assert!(
            dump.contains("press S again"),
            "Help S row should mention press-S-again close; got:\n{dump}"
        );
        assert!(
            dump.contains("press P again"),
            "Help P row should mention press-P-again close; got:\n{dump}"
        );
        assert!(
            dump.contains("press t again"),
            "Help t row should mention press-t-again close; got:\n{dump}"
        );
    }

    #[test]
    fn provider_setup_modal_renders_without_panic() {
        // Phase G-1 Task 2 (v0.4.0) smoke test: drive the new modal
        // through ratatui's `TestBackend` to lock in that the layout
        // constraints + tab + body + hint composition stays
        // panic-free across viewport sizes used in the integration
        // tests, and that the operator-facing strings ("Provider
        // Setup" title and the active tab label "Claude") actually
        // reach the buffer rather than being clipped or styled
        // away.
        use crate::ui::provider_setup::ProviderSetupOverlay;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut overlay = ProviderSetupOverlay::new();
        overlay.open();
        terminal
            .draw(|f| {
                super::render_provider_setup_modal(f, &overlay);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let dump: String = buffer
            .content
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(
            dump.contains("Provider Setup"),
            "modal must render its title in the visible buffer"
        );
        assert!(
            dump.contains("Claude"),
            "default tab must render the Claude label"
        );
        assert!(
            dump.contains("[x]"),
            "modal must render an [x] close button so mouse operators can dismiss it"
        );
    }

    #[test]
    fn provider_setup_tab_index_at_uses_label_aware_boundaries() {
        // Tabs rendered as ` Claude │ Codex │ Gemini │ agy │ Tmux ` left-aligned
        // inside a single-cell border — the function maps each click
        // column to whichever label region it lands on, NOT to a
        // naive equal-width partition.
        let tabs = Rect::new(10, 0, 54, 3);
        // inner_x = 11
        // claude_end = 19  (first column belonging to Codex region)
        // codex_end = 27   (first column belonging to Gemini region)
        // gemini_end = 36  (first column belonging to agy region)
        // agy_end = 43     (first column belonging to Tmux region)
        // tmux_end = 49    (first column past " Tmux ")
        //
        // Claude region: columns 10..=18 (border + leading space + "Claude" + trailing space)
        assert_eq!(provider_setup_tab_index_at(tabs, 10), Some(0));
        assert_eq!(provider_setup_tab_index_at(tabs, 12), Some(0)); // first 'C' of "Claude"
        assert_eq!(provider_setup_tab_index_at(tabs, 18), Some(0));
        // Codex region: columns 19..=26 (divider + leading space + "Codex" + trailing space)
        assert_eq!(provider_setup_tab_index_at(tabs, 19), Some(1));
        assert_eq!(provider_setup_tab_index_at(tabs, 22), Some(1)); // 'd' of "Codex"
        assert_eq!(provider_setup_tab_index_at(tabs, 26), Some(1));
        // Gemini region: columns 27..=35 (divider + leading space + "Gemini" + trailing space)
        assert_eq!(provider_setup_tab_index_at(tabs, 27), Some(2));
        assert_eq!(provider_setup_tab_index_at(tabs, 30), Some(2)); // 'm' of "Gemini"
        assert_eq!(provider_setup_tab_index_at(tabs, 35), Some(2));
        // agy region: columns 36..=42 (divider + leading space + "agy" + trailing space)
        assert_eq!(provider_setup_tab_index_at(tabs, 36), Some(3));
        assert_eq!(provider_setup_tab_index_at(tabs, 38), Some(3)); // 'y' of "agy"
        assert_eq!(provider_setup_tab_index_at(tabs, 42), Some(3));
        // Tmux region: columns 43..=48 (divider + leading space + "Tmux" + trailing space)
        assert_eq!(provider_setup_tab_index_at(tabs, 43), Some(4));
        assert_eq!(provider_setup_tab_index_at(tabs, 46), Some(4)); // 'm' of "Tmux"
        assert_eq!(provider_setup_tab_index_at(tabs, 48), Some(4));
        // Past the rendered tabs (within the wide tabs rect): None
        assert_eq!(provider_setup_tab_index_at(tabs, 49), None);
        assert_eq!(provider_setup_tab_index_at(tabs, 55), None);
        // Out of the tabs rect entirely: None
        assert_eq!(provider_setup_tab_index_at(tabs, 9), None);
    }
}
