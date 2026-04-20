use ratatui::prelude::*;

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::ui::{alerts, panels};

/// Top-level dashboard layout: top third is the alert queue (system
/// notices + per-pane recommendations), rest is a grid of per-pane
/// panels.
pub fn render_dashboard(
    frame: &mut Frame<'_>,
    notices: &[SystemNotice],
    reports: &[PaneReport],
) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    alerts::render_alerts(chunks[0], frame.buffer_mut(), notices, reports);

    if reports.is_empty() {
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Ratio(1, reports.len() as u32);
            reports.len()
        ])
        .split(chunks[1]);

    for (i, rep) in reports.iter().enumerate() {
        panels::render_pane_panel(columns[i], frame.buffer_mut(), rep);
    }
}
