use std::collections::{HashMap, HashSet};

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::ui::alerts::AlertView;
use crate::ui::{alerts, labels, panels, theme};

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
    pub target_label: &'a str,
    pub alerts_focused: bool,
    pub panes_focused: bool,
}

pub fn render_dashboard(
    frame: &mut Frame<'_>,
    alert_state: &mut ListState,
    pane_state: &mut ListState,
    view: DashboardView<'_>,
) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(36),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(area);

    alerts::render_alerts(
        chunks[0],
        frame.buffer_mut(),
        alert_state,
        AlertView {
            notices: view.notices,
            reports: view.reports,
            fresh_alerts: view.fresh_alerts,
            alert_times: view.alert_times,
            target_label: view.target_label,
            focused: view.alerts_focused,
        },
    );

    panels::render_pane_list(
        chunks[1],
        frame.buffer_mut(),
        view.reports,
        pane_state,
        view.target_label,
        view.panes_focused,
    );

    render_footer(
        chunks[2],
        frame.buffer_mut(),
        view.alerts_focused,
        view.panes_focused,
    );
}

pub fn render_target_picker(
    frame: &mut Frame<'_>,
    title: &str,
    hint: &str,
    labels: &[String],
    state: &mut ListState,
    current_label: &str,
) {
    let area = centered_rect(60, 60, frame.area());
    frame.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(2)])
        .split(area);

    let block = Block::default()
        .title(format!("{title} · current {current_label}"))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));

    if labels.is_empty() {
        frame.render_widget(
            Paragraph::new("no tmux windows discovered")
                .style(Style::default().fg(theme::TEXT_DIM))
                .block(block),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(hint)
                .style(Style::default().fg(theme::TEXT_DIM))
                .wrap(Wrap { trim: false }),
            chunks[1],
        );
        return;
    }

    let selected = state
        .selected()
        .unwrap_or(0)
        .min(labels.len().saturating_sub(1));
    state.select(Some(selected));
    let items: Vec<ListItem<'_>> = labels
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
        chunks[0],
        state,
    );
    frame.render_widget(
        Paragraph::new(hint)
            .style(Style::default().fg(theme::TEXT_DIM))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

pub fn render_help_modal(frame: &mut Frame<'_>, scroll: u16) {
    let area = centered_rect(HELP_WIDTH_PERCENT, HELP_HEIGHT_PERCENT, frame.area());
    frame.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(2)])
        .split(area);

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
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new("↑/↓ scroll · PgUp/PgDn jump · Home/End · Esc close")
            .style(Style::default().fg(theme::TEXT_DIM))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

pub fn max_help_scroll(viewport: Rect) -> usize {
    let area = centered_rect(HELP_WIDTH_PERCENT, HELP_HEIGHT_PERCENT, viewport);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(2)])
        .split(area);
    let visible_lines = chunks[0].height.saturating_sub(2) as usize;
    help_lines().len().saturating_sub(visible_lines)
}

fn render_footer(area: Rect, buf: &mut Buffer, alerts_focused: bool, panes_focused: bool) {
    let focus = if alerts_focused {
        "focus: alerts"
    } else if panes_focused {
        "focus: panes"
    } else {
        "focus: overlay"
    };
    Paragraph::new(format!(
        "{focus} · ↑/↓ item · PgUp/PgDn page · Home/End · Tab switch · t target · ? help · q quit"
    ))
    .style(Style::default().fg(theme::TEXT_DIM))
    .wrap(Wrap { trim: false })
    .render(area, buf);
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
}
