use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::audit::AuditEventKind;
use crate::ui::alerts::AlertView;
use crate::ui::{alerts, labels, panels, theme};

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
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub hidden_until: &'a HashMap<String, Instant>,
    pub state_flashes: &'a HashMap<String, panels::PaneStateFlash>,
    pub now: Instant,
    pub target_label: &'a str,
    pub split: DashboardSplit,
    pub alerts_focused: bool,
    pub panes_focused: bool,
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
        },
    );

    render_split_divider(rects.divider, frame.buffer_mut(), view.split);

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

    render_footer(
        rects.footer,
        frame.buffer_mut(),
        view.alerts_focused,
        view.panes_focused,
        view.split,
    );
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
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));

    if view.labels.is_empty() {
        frame.render_widget(
            Paragraph::new("no tmux windows discovered")
                .style(Style::default().fg(theme::TEXT_DIM))
                .block(block),
            rects.list,
        );
        frame.render_widget(
            Paragraph::new(view.hint)
                .style(Style::default().fg(theme::TEXT_DIM))
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
                    .fg(theme::TEXT_PRIMARY)
                    .bg(theme::BADGE_BG)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ "),
        rects.list,
        state,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        close_button_rect(rects.list),
    );
    frame.render_widget(
        Paragraph::new(if view.preview_lines.is_empty() {
            "no pane preview available".to_string()
        } else {
            view.preview_lines.join("\n")
        })
        .style(Style::default().fg(theme::TEXT_DIM))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(view.preview_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::BORDER_IDLE)),
        ),
        rects.preview,
    );
    frame.render_widget(
        Paragraph::new(view.hint)
            .style(Style::default().fg(theme::TEXT_DIM))
            .wrap(Wrap { trim: false }),
        rects.hint,
    );
}

pub fn render_help_modal(frame: &mut Frame<'_>, scroll: u16) {
    let rects = help_modal_rects(frame.area());
    let text_width = help_body_text_width(rects.body);
    frame.render_widget(Clear, rects.area);

    frame.render_widget(
        Paragraph::new(help_lines_for_width(text_width))
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title("Help")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::BORDER_ACTIVE)),
            ),
        rects.body,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        close_button_rect(rects.body),
    );
    frame.render_widget(
        Paragraph::new("↑/↓ scroll · PgUp/PgDn jump · Home/End · click [x] close · Esc close")
            .style(Style::default().fg(theme::TEXT_DIM))
            .wrap(Wrap { trim: false }),
        rects.hint,
    );
}

pub fn render_git_modal(frame: &mut Frame<'_>, title: &str, lines: &[String], scroll: u16) {
    let rects = git_modal_rects(frame.area());
    frame.render_widget(Clear, rects.area);

    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::BORDER_ACTIVE)),
            ),
        rects.body,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        close_button_rect(rects.body),
    );
    frame.render_widget(
        Paragraph::new("↑/↓ scroll · PgUp/PgDn jump · Home/End · click [x] close · Esc close")
            .style(Style::default().fg(theme::TEXT_DIM))
            .wrap(Wrap { trim: false }),
        rects.hint,
    );
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
    let alerts_height = alerts_height_for(body_height, split);
    let panes_height = body_height.saturating_sub(alerts_height);
    let alerts = Rect::new(area.x, area.y, area.width, alerts_height);
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

fn render_split_divider(area: Rect, buf: &mut Buffer, split: DashboardSplit) {
    if area.height == 0 {
        return;
    }
    Paragraph::new(format!(
        "drag resize alerts/panes · alerts {}% · [/] resize · = reset",
        split.alerts_percent()
    ))
    .style(Style::default().fg(theme::TEXT_DIM).bg(theme::BADGE_BG))
    .alignment(Alignment::Center)
    .render(area, buf);
}

fn render_footer(
    area: Rect,
    buf: &mut Buffer,
    alerts_focused: bool,
    panes_focused: bool,
    split: DashboardSplit,
) {
    let focus = if alerts_focused {
        "focus: alerts"
    } else if panes_focused {
        "focus: panes"
    } else {
        "focus: overlay"
    };
    let badge = version_badge_rect(area);
    let text_width = area.width.saturating_sub(badge.width).saturating_sub(1);
    let text_area = Rect::new(area.x, area.y, text_width, area.height);
    Paragraph::new(footer_text(focus, split))
        .style(Style::default().fg(theme::TEXT_DIM))
        .wrap(Wrap { trim: false })
        .render(text_area, buf);
    Paragraph::new(version_badge_label())
        .style(
            theme::label_style()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .render(badge, buf);
}

/// Pure footer-line builder. Extracted from `render_footer` in v1.10.2
/// so the list of advertised keybindings can be unit-tested without
/// spinning up a buffer. The `focus` argument is the prefix (e.g.
/// `"focus: alerts"`) decided by the caller.
fn footer_text(focus: &str, split: DashboardSplit) -> String {
    format!(
        "{focus} · split {}% · [/] resize · = reset · wheel scroll · click select · click severity bulk hide · click version git · ↑/↓ item · PgUp/PgDn page · Home/End · Tab switch · t target · u runtime · p accept · d dismiss · ? help · q quit",
        split.alerts_percent()
    )
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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
        ("Mouse wheel", "scroll the list or modal under the pointer"),
        ("Mouse left", "select the clicked alert, pane, or target"),
        ("Mouse double", "toggle hide on the clicked alert"),
        (
            "Mouse drag",
            "drag the divider between Alerts and Panes to resize them",
        ),
        (
            "Severity chip",
            "click a bulk chip in Alerts to toggle auto-hide for that severity",
        ),
        (
            "Version badge",
            "click the bottom-right version to open Git status",
        ),
        ("Tab", "switch focus between alerts and pane list"),
        ("Up / Down", "move one item in the focused list"),
        ("j / k", "alternate list scroll keys"),
        ("PgUp / PgDn", "scroll one page in the focused list"),
        ("Home / End", "jump to the first or last item"),
        (
            "[ / ]",
            "shrink or grow Alerts; Panes use the remaining height",
        ),
        ("=", "reset the Alerts/Panes split"),
        ("t", "open tmux target picker (session -> window)"),
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
        ("c", "clear system notices"),
    ] {
        lines.extend(help_wrapped_detail_lines(label, value, total_width));
    }

    lines.extend(help_wrapped_detail_lines(
        "p",
        "accept pending prompt-send proposal (P5-3 safer-actuation)",
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
        &format!("dismiss pending prompt-send proposal (audit: {rejected}; every actuation mode)"),
        total_width,
    ));

    for (label, value) in [("Esc / ?", "close this help"), ("q", "quit the TUI")] {
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
            .fg(theme::TEXT_PRIMARY)
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
            Style::default().fg(theme::TEXT_DIM),
        ),
        Span::styled(text.to_string(), Style::default().fg(theme::TEXT_DIM)),
    ])
}

fn help_value_continuation_line(text: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            help_value_continuation_prefix(),
            Style::default().fg(theme::TEXT_DIM),
        ),
        Span::styled(text.to_string(), Style::default().fg(theme::TEXT_DIM)),
    ])
}

fn help_detail_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw(HELP_DETAIL_INDENT),
        Span::styled(
            format!("{label:<HELP_LABEL_WIDTH$}"),
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": ", Style::default().fg(theme::TEXT_DIM)),
        Span::styled(value.to_string(), Style::default().fg(theme::TEXT_DIM)),
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
    fn version_badge_hugs_bottom_right_edge() {
        let area = Rect::new(4, 6, 40, 2);
        let badge = version_badge_rect(area);
        assert_eq!(badge.y, area.y + area.height - 1);
        assert_eq!(badge.x + badge.width, area.x + area.width);
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
    fn footer_text_advertises_prompt_send_keys() {
        // v1.10.2 polish (Codex v1.9.2 / v1.10.0 follow-up): the
        // global footer must advertise `p` (accept) and `d` (dismiss)
        // alongside the other single-letter keys so operators notice
        // the P5-3 actuation surface without having to open the help
        // overlay. v1.10.3 tightening (Codex v1.10.2 §8): also pin the
        // ordering — the two actuation keys should sit between
        // `t target` and `? help` so they stay adjacent to target
        // selection and immediately before the generic help/quit tail.
        let text = footer_text("focus: alerts", DashboardSplit::default());
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
            text.contains("split 36%"),
            "footer must show current dashboard split: {text}"
        );
        assert!(
            text.contains("[/] resize"),
            "footer must advertise split resize keys: {text}"
        );
        assert!(
            text.contains("= reset"),
            "footer must advertise split reset key: {text}"
        );
        // Sanity: existing anchors still present.
        assert!(text.starts_with("focus: alerts"));
        assert!(text.contains("? help"));
        assert!(text.contains("q quit"));
        // Placement contract: t target → p accept → d dismiss → ? help.
        let target_pos = text
            .find("t target")
            .expect("footer must keep the `t target` anchor");
        let p_pos = text.find("p accept").expect("footer must carry `p accept`");
        let u_pos = text
            .find("u runtime")
            .expect("footer must carry `u runtime`");
        let d_pos = text
            .find("d dismiss")
            .expect("footer must carry `d dismiss`");
        let help_pos = text
            .find("? help")
            .expect("footer must keep the `? help` anchor");
        assert!(
            target_pos < u_pos,
            "`u runtime` must come after `t target` (provider refresh near target selection)"
        );
        assert!(
            u_pos < p_pos,
            "`u runtime` must precede prompt-send actuation keys"
        );
        assert!(
            p_pos < d_pos,
            "`p accept` must precede `d dismiss` (alphabetical / accept-before-dismiss)"
        );
        assert!(
            d_pos < help_pos,
            "actuation keys must precede `? help` (generic tail)"
        );
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
        let resize_keys = entry_for("[ / ]").expect("help must document resize keys");
        let reset_key = entry_for("=").expect("help must document split reset");

        assert!(drag.contains("divider"), "got: {drag}");
        assert!(
            resize_keys.contains("shrink or grow Alerts"),
            "got: {resize_keys}"
        );
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
}
