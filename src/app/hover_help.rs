use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::layout::{Margin, Rect};
use ratatui::widgets::ListState;

use crate::app::config::HoverHelpTrigger;
use crate::app::event_loop::PaneReport;
use crate::app::keymap::rect_contains;
use crate::app::system_notice::SystemNotice;
use crate::ui::dashboard::{DashboardSplit, dashboard_rects};
use crate::ui::help_glossary::HelpTopic;

const HOVER_HELP_LABEL_ZONE_WIDTH: u16 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoverHelpHover {
    pub topic: HelpTopic,
    pub column: u16,
    pub row: u16,
    pub updated_at: Instant,
}

#[derive(Debug, Default)]
pub struct HoverHelpState {
    hover: Option<HoverHelpHover>,
}

impl HoverHelpState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hover(&self) -> Option<HoverHelpHover> {
        self.hover
    }

    pub fn set_hover(&mut self, topic: HelpTopic, column: u16, row: u16, now: Instant) {
        self.hover = Some(HoverHelpHover {
            topic,
            column,
            row,
            updated_at: now,
        });
    }

    pub fn clear_hover(&mut self) {
        self.hover = None;
    }
}

pub struct DashboardHoverView<'a> {
    pub split: DashboardSplit,
    pub hover_help_trigger: HoverHelpTrigger,
    pub alert_state: &'a ListState,
    pub pane_state: &'a ListState,
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub hidden_until: &'a HashMap<String, Instant>,
    pub now: Instant,
    pub target_label: &'a str,
}

fn hover_trigger_accepts_column(trigger: HoverHelpTrigger, inner: Rect, column: u16) -> bool {
    match trigger {
        HoverHelpTrigger::Row => true,
        HoverHelpTrigger::Label => {
            let label_width = inner.width.min(HOVER_HELP_LABEL_ZONE_WIDTH);
            column < inner.x.saturating_add(label_width)
        }
    }
}

pub fn dashboard_hover_topic(
    viewport: Rect,
    column: u16,
    row: u16,
    view: DashboardHoverView<'_>,
) -> Option<HelpTopic> {
    let rects = dashboard_rects(viewport, view.split);
    if rect_contains(rects.alerts, column, row) {
        let inner = rects.alerts.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        if !rect_contains(inner, column, row) {
            return None;
        }
        if !hover_trigger_accepts_column(view.hover_help_trigger, inner, column) {
            return None;
        }
        if row == inner.y {
            return Some(HelpTopic::AlertBulkHide);
        }
        return crate::ui::alerts::alert_help_topic_at_row(
            view.alert_state,
            crate::ui::alerts::AlertView {
                notices: view.notices,
                reports: view.reports,
                fresh_alerts: view.fresh_alerts,
                alert_times: view.alert_times,
                hidden_until: view.hidden_until,
                now: view.now,
                target_label: view.target_label,
                focused: true,
            },
            inner.width.saturating_sub(3) as usize,
            row.saturating_sub(inner.y.saturating_add(1)),
        );
    }

    if rect_contains(rects.panes, column, row) {
        let inner = rects.panes.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        if !rect_contains(inner, column, row) {
            return None;
        }
        if !hover_trigger_accepts_column(view.hover_help_trigger, inner, column) {
            return None;
        }
        return crate::ui::panels::pane_help_topic_at_row(
            view.reports,
            view.pane_state,
            row.saturating_sub(inner.y),
            rects.panes.width.saturating_sub(4),
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_state_tracks_and_clears_topic() {
        let now = Instant::now();
        let mut state = HoverHelpState::new();
        state.set_hover(HelpTopic::PaneCommand, 5, 6, now);

        assert_eq!(
            state.hover().map(|hover| hover.topic),
            Some(HelpTopic::PaneCommand)
        );

        state.clear_hover();
        assert!(state.hover().is_none());
    }

    #[test]
    fn label_trigger_limits_dashboard_hover_to_front_label_zone() {
        let viewport = Rect::new(0, 0, 120, 40);
        let split = DashboardSplit::default();
        let rects = dashboard_rects(viewport, split);
        let inner = rects.alerts.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let alert_state = ListState::default();
        let pane_state = ListState::default();
        let notices = Vec::new();
        let reports = Vec::new();
        let fresh_alerts = HashSet::new();
        let alert_times = HashMap::new();
        let hidden_until = HashMap::new();
        let now = Instant::now();

        let view = |hover_help_trigger| DashboardHoverView {
            split,
            hover_help_trigger,
            alert_state: &alert_state,
            pane_state: &pane_state,
            notices: &notices,
            reports: &reports,
            fresh_alerts: &fresh_alerts,
            alert_times: &alert_times,
            hidden_until: &hidden_until,
            now,
            target_label: "main",
        };

        assert_eq!(
            dashboard_hover_topic(
                viewport,
                inner.x.saturating_add(2),
                inner.y,
                view(HoverHelpTrigger::Label)
            ),
            Some(HelpTopic::AlertBulkHide)
        );
        assert_eq!(
            dashboard_hover_topic(
                viewport,
                inner.x.saturating_add(50),
                inner.y,
                view(HoverHelpTrigger::Label)
            ),
            None
        );
        assert_eq!(
            dashboard_hover_topic(
                viewport,
                inner.x.saturating_add(50),
                inner.y,
                view(HoverHelpTrigger::Row)
            ),
            Some(HelpTopic::AlertBulkHide)
        );
    }
}
