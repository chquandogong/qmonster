use std::collections::{HashMap, HashSet};

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::ui::alerts::AlertView;
use crate::ui::{alerts, labels, panels, theme};

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
    labels: &[String],
    state: &mut ListState,
    current_label: &str,
) {
    let area = centered_rect(60, 60, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!("Choose Target · current {current_label}"))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));

    if labels.is_empty() {
        frame.render_widget(
            Paragraph::new("no tmux windows discovered")
                .style(Style::default().fg(theme::TEXT_DIM))
                .block(block),
            area,
        );
        return;
    }

    let selected = state.selected().unwrap_or(0).min(labels.len().saturating_sub(1));
    state.select(Some(selected));
    let items: Vec<ListItem<'_>> = labels.iter().map(|label| ListItem::new(label.as_str())).collect();

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
        area,
        state,
    );
}

pub fn render_help_modal(frame: &mut Frame<'_>) {
    let area = centered_rect(72, 70, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = vec![
        "Controls".to_string(),
        "Tab: switch focus between alerts and pane list".to_string(),
        "Up / Down or j / k: scroll the focused list".to_string(),
        "t: open tmux target picker (all windows or a session:window)".to_string(),
        "Enter: confirm target selection".to_string(),
        "s: write a runtime snapshot".to_string(),
        "r: refresh version drift check".to_string(),
        "c: clear system notices".to_string(),
        "Esc or ?: close this help".to_string(),
        "q: quit the TUI".to_string(),
        String::new(),
        "Source Labels".to_string(),
    ];
    lines.extend(labels::source_kind_legend().into_iter().map(str::to_string));
    lines.push(String::new());
    lines.push("State Labels".to_string());
    lines.extend(labels::signal_legend().into_iter().map(str::to_string));

    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title("Help")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::BORDER_ACTIVE)),
            ),
        area,
    );
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
        "{focus} · Tab switch · ↑/↓ scroll · t target · ? help · s snapshot · r refresh · c clear · q quit"
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
