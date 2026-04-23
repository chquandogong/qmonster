use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::ui::alerts::AlertView;
use crate::ui::{alerts, labels, panels, theme};

const VERSION_BADGE_PADDING: u16 = 2;
const GIT_WIDTH_PERCENT: u16 = 72;
const GIT_HEIGHT_PERCENT: u16 = 68;
const HELP_WIDTH_PERCENT: u16 = 76;
const HELP_HEIGHT_PERCENT: u16 = 76;
const HELP_LABEL_WIDTH: usize = 18;

/// Top-level dashboard layout: alerts on top, pane list below, and a
/// persistent control footer to keep navigation discoverable.
pub struct DashboardView<'a> {
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub hidden_until: &'a HashMap<String, Instant>,
    pub now: Instant,
    pub target_label: &'a str,
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
    pub panes: Rect,
    pub footer: Rect,
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
    let rects = dashboard_rects(frame.area());

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

    panels::render_pane_list(
        rects.panes,
        frame.buffer_mut(),
        view.reports,
        pane_state,
        view.target_label,
        view.panes_focused,
    );

    render_footer(
        rects.footer,
        frame.buffer_mut(),
        view.alerts_focused,
        view.panes_focused,
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
    frame.render_widget(Clear, rects.area);

    frame.render_widget(
        Paragraph::new(help_lines())
            .wrap(Wrap { trim: false })
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
    help_lines().len().saturating_sub(visible_lines)
}

pub fn max_git_scroll(viewport: Rect, line_count: usize) -> usize {
    let rects = git_modal_rects(viewport);
    let visible_lines = rects.body.height.saturating_sub(2) as usize;
    line_count.saturating_sub(visible_lines)
}

pub fn dashboard_rects(area: Rect) -> DashboardRects {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(36),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(area);
    DashboardRects {
        alerts: chunks[0],
        panes: chunks[1],
        footer: chunks[2],
    }
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
    format!("v{}", env!("CARGO_PKG_VERSION"))
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

fn render_footer(area: Rect, buf: &mut Buffer, alerts_focused: bool, panes_focused: bool) {
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
    Paragraph::new(footer_text(focus))
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
fn footer_text(focus: &str) -> String {
    format!(
        "{focus} · wheel scroll · click select · click severity bulk hide · click version git · ↑/↓ item · PgUp/PgDn page · Home/End · Tab switch · t target · p accept · d dismiss · ? help · q quit"
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

fn help_lines() -> Vec<Line<'static>> {
    let mut lines = vec![section_line("Controls")];
    lines.extend(
        [
            ("Mouse wheel", "scroll the list or modal under the pointer"),
            ("Mouse left", "select the clicked alert, pane, or target"),
            ("Mouse double", "toggle hide on the clicked alert"),
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
            ("c", "clear system notices"),
            (
                "p",
                "accept the pending prompt-send proposal on the selected pane (P5-3 safer-actuation). Audit chain depends on the actuation mode: Execute (allow_auto_prompt_send=true, non-observe_only) fires PromptSendAccepted → PromptSendCompleted or PromptSendFailed; AutoSendOff (allow_auto_prompt_send=false, non-observe_only) fires PromptSendAccepted + PromptSendBlocked; observe_only fires PromptSendBlocked alone (no PromptSendAccepted)",
            ),
            (
                "d",
                "dismiss the pending prompt-send proposal on the selected pane (audit: PromptSendRejected; available in every actuation mode)",
            ),
            ("Esc / ?", "close this help"),
            ("q", "quit the TUI"),
        ]
        .into_iter()
        .map(|(label, value)| help_detail_line(label, value)),
    );

    lines.push(Line::raw(""));
    lines.push(section_line("Source Labels"));
    lines.extend(
        labels::source_kind_legend()
            .into_iter()
            .map(split_once_space)
            .map(|(label, value)| help_detail_line(label, value)),
    );

    lines.push(Line::raw(""));
    lines.push(section_line("State Labels"));
    lines.extend(
        labels::signal_legend()
            .into_iter()
            .map(split_once_colon)
            .map(|(label, value)| help_detail_line(label, value)),
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

fn help_detail_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
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
        assert!(text.starts_with("Tab"));
        assert!(text.contains(": switch focus"));
    }

    #[test]
    fn help_scroll_budget_grows_when_viewport_shrinks() {
        let tall = max_help_scroll(Rect::new(0, 0, 120, 48));
        let short = max_help_scroll(Rect::new(0, 0, 120, 16));
        assert!(short >= tall);
    }

    #[test]
    fn version_badge_hugs_bottom_right_edge() {
        let area = Rect::new(4, 6, 40, 2);
        let badge = version_badge_rect(area);
        assert_eq!(badge.y, area.y + area.height - 1);
        assert_eq!(badge.x + badge.width, area.x + area.width);
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
        let text = footer_text("focus: alerts");
        assert!(
            text.contains("p accept"),
            "footer must advertise `p accept`: {text}"
        );
        assert!(
            text.contains("d dismiss"),
            "footer must advertise `d dismiss`: {text}"
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
        let d_pos = text
            .find("d dismiss")
            .expect("footer must carry `d dismiss`");
        let help_pos = text
            .find("? help")
            .expect("footer must keep the `? help` anchor");
        assert!(
            target_pos < p_pos,
            "`p accept` must come after `t target` (actuation keys adjacent to target selection)"
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
        let p_entry = entry_for("p").expect("help overlay must carry a `p` entry");
        let d_entry = entry_for("d").expect("help overlay must carry a `d` entry");
        assert!(
            p_entry.contains("PromptSendAccepted"),
            "the `p` entry must name the PromptSendAccepted audit kind so operators can map the key to the audit log. got: {p_entry}"
        );
        assert!(
            p_entry.contains("PromptSendBlocked"),
            "the `p` entry must mention PromptSendBlocked so the observe_only / auto-send-off branches are discoverable. got: {p_entry}"
        );
        assert!(
            d_entry.contains("PromptSendRejected"),
            "the `d` entry must name the PromptSendRejected audit kind. got: {d_entry}"
        );
        // v1.10.3 tightening (Codex v1.10.2 finding #1): the `p` help
        // row MUST describe all three audit outcomes distinctly so
        // the AutoSendOff branch is not confused with the observe_only
        // branch. AutoSendOff is a two-event chain
        // (PromptSendAccepted + PromptSendBlocked); observe_only fires
        // PromptSendBlocked alone. The old copy collapsed them.
        assert!(
            p_entry.contains("AutoSendOff"),
            "the `p` entry must name the AutoSendOff path explicitly so operators see that it fires TWO audit events (Accepted + Blocked). got: {p_entry}"
        );
        assert!(
            p_entry.contains("observe_only"),
            "the `p` entry must name the observe_only path explicitly so operators see it fires PromptSendBlocked ALONE (no Accepted). got: {p_entry}"
        );
        // Both terminal outcomes on the Execute path must be
        // enumerated so operators know the audit log will carry one
        // of them per successful confirmation.
        assert!(
            p_entry.contains("PromptSendCompleted"),
            "the `p` entry must name PromptSendCompleted as the success terminal outcome on Execute. got: {p_entry}"
        );
        assert!(
            p_entry.contains("PromptSendFailed"),
            "the `p` entry must name PromptSendFailed as the failure terminal outcome on Execute. got: {p_entry}"
        );
    }
}
