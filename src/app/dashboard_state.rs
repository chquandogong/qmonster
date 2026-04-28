use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use chrono::Local;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Margin;
use ratatui::widgets::ListState;

use crate::app::event_loop::PaneReport;
use crate::app::keymap::{
    FocusedPanel, ScrollDir, list_row_at, move_selection, page_selection, rect_contains,
    select_first, select_last,
};
use crate::app::system_notice::SystemNotice;
use crate::domain::recommendation::Severity;
use crate::domain::signal::IdleCause;
use crate::ui::dashboard::{
    DashboardSplit, dashboard_rects, dashboard_split_from_row, version_badge_rect,
};

#[derive(Debug, Clone)]
pub struct AlertMouseClick {
    key: String,
    at: Instant,
}

const ALERT_DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(450);

pub struct DashboardSyncState<'a> {
    pub alert_state: &'a mut ListState,
    pub pane_state: &'a mut ListState,
    pub previous_alerts: &'a mut HashSet<String>,
    pub fresh_alerts: &'a mut HashSet<String>,
    pub alert_times: &'a mut HashMap<String, String>,
    pub alert_hide_deadlines: &'a mut HashMap<String, Instant>,
}

pub struct DashboardSelectionKeyView<'a> {
    pub focus: FocusedPanel,
    pub alert_state: &'a mut ListState,
    pub pane_state: &'a mut ListState,
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub alert_hide_deadlines: &'a mut HashMap<String, Instant>,
    pub now: Instant,
}

pub struct DashboardMouseView<'a> {
    pub focus: &'a mut FocusedPanel,
    pub split: &'a mut DashboardSplit,
    pub split_dragging: &'a mut bool,
    pub alert_state: &'a mut ListState,
    pub pane_state: &'a mut ListState,
    pub last_alert_click: &'a mut Option<AlertMouseClick>,
    pub alert_hide_deadlines: &'a mut HashMap<String, Instant>,
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub target_label: &'a str,
    pub now: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardMouseAction {
    None,
    OpenGitModal,
}

pub fn handle_dashboard_selection_key(view: DashboardSelectionKeyView<'_>, key: KeyCode) -> bool {
    match key {
        KeyCode::Up | KeyCode::Char('k') => {
            match view.focus {
                FocusedPanel::Alerts => {
                    let total = crate::ui::alerts::alert_count(
                        view.notices,
                        view.reports,
                        view.alert_hide_deadlines,
                        view.now,
                    );
                    move_selection(view.alert_state, total, -1);
                }
                FocusedPanel::Panes => {
                    move_selection(view.pane_state, view.reports.len(), -1);
                }
            }
            true
        }
        KeyCode::Down | KeyCode::Char('j') => {
            match view.focus {
                FocusedPanel::Alerts => {
                    let total = crate::ui::alerts::alert_count(
                        view.notices,
                        view.reports,
                        view.alert_hide_deadlines,
                        view.now,
                    );
                    move_selection(view.alert_state, total, 1);
                }
                FocusedPanel::Panes => {
                    move_selection(view.pane_state, view.reports.len(), 1);
                }
            }
            true
        }
        KeyCode::PageUp => {
            match view.focus {
                FocusedPanel::Alerts => {
                    let total = crate::ui::alerts::alert_count(
                        view.notices,
                        view.reports,
                        view.alert_hide_deadlines,
                        view.now,
                    );
                    page_selection(view.alert_state, total, 6, ScrollDir::Up);
                }
                FocusedPanel::Panes => {
                    page_selection(view.pane_state, view.reports.len(), 3, ScrollDir::Up);
                }
            }
            true
        }
        KeyCode::PageDown => {
            match view.focus {
                FocusedPanel::Alerts => {
                    let total = crate::ui::alerts::alert_count(
                        view.notices,
                        view.reports,
                        view.alert_hide_deadlines,
                        view.now,
                    );
                    page_selection(view.alert_state, total, 6, ScrollDir::Down);
                }
                FocusedPanel::Panes => {
                    page_selection(view.pane_state, view.reports.len(), 3, ScrollDir::Down);
                }
            }
            true
        }
        KeyCode::Home => {
            match view.focus {
                FocusedPanel::Alerts => {
                    let total = crate::ui::alerts::alert_count(
                        view.notices,
                        view.reports,
                        view.alert_hide_deadlines,
                        view.now,
                    );
                    select_first(view.alert_state, total);
                }
                FocusedPanel::Panes => select_first(view.pane_state, view.reports.len()),
            }
            true
        }
        KeyCode::End => {
            match view.focus {
                FocusedPanel::Alerts => {
                    let total = crate::ui::alerts::alert_count(
                        view.notices,
                        view.reports,
                        view.alert_hide_deadlines,
                        view.now,
                    );
                    select_last(view.alert_state, total);
                }
                FocusedPanel::Panes => select_last(view.pane_state, view.reports.len()),
            }
            true
        }
        KeyCode::Enter | KeyCode::Char(' ') if view.focus == FocusedPanel::Alerts => {
            toggle_selected_alert_hide(
                view.alert_hide_deadlines,
                view.alert_state,
                view.notices,
                view.reports,
                view.now,
            );
            sync_alert_selection(
                view.alert_state,
                view.notices,
                view.reports,
                view.alert_hide_deadlines,
                view.now,
            );
            true
        }
        _ => false,
    }
}

pub fn handle_dashboard_mouse(
    viewport: ratatui::layout::Rect,
    event: MouseEvent,
    view: DashboardMouseView<'_>,
) -> DashboardMouseAction {
    let rects = dashboard_rects(viewport, *view.split);
    match event.kind {
        MouseEventKind::ScrollUp => {
            if rect_contains(rects.alerts, event.column, event.row) {
                *view.focus = FocusedPanel::Alerts;
                *view.last_alert_click = None;
                let total = crate::ui::alerts::alert_count(
                    view.notices,
                    view.reports,
                    view.alert_hide_deadlines,
                    view.now,
                );
                move_selection(view.alert_state, total, -1);
            } else if rect_contains(rects.panes, event.column, event.row) {
                *view.focus = FocusedPanel::Panes;
                move_selection(view.pane_state, view.reports.len(), -1);
            }
            DashboardMouseAction::None
        }
        MouseEventKind::ScrollDown => {
            if rect_contains(rects.alerts, event.column, event.row) {
                *view.focus = FocusedPanel::Alerts;
                let total = crate::ui::alerts::alert_count(
                    view.notices,
                    view.reports,
                    view.alert_hide_deadlines,
                    view.now,
                );
                move_selection(view.alert_state, total, 1);
            } else if rect_contains(rects.panes, event.column, event.row) {
                *view.focus = FocusedPanel::Panes;
                *view.last_alert_click = None;
                move_selection(view.pane_state, view.reports.len(), 1);
            }
            DashboardMouseAction::None
        }
        MouseEventKind::Down(MouseButton::Left) => {
            *view.split_dragging = false;
            if rect_contains(rects.divider, event.column, event.row) {
                *view.split = dashboard_split_from_row(viewport, event.row);
                *view.split_dragging = true;
                *view.last_alert_click = None;
                return DashboardMouseAction::None;
            }
            if rect_contains(version_badge_rect(rects.footer), event.column, event.row) {
                return DashboardMouseAction::OpenGitModal;
            }
            if let Some(row) = list_row_at(rects.alerts, event) {
                *view.focus = FocusedPanel::Alerts;
                let alerts_inner = rects.alerts.inner(Margin {
                    vertical: 1,
                    horizontal: 1,
                });
                if row == 0 {
                    *view.last_alert_click = None;
                    if let Some(severity) = crate::ui::alerts::bulk_hide_severity_at_column(
                        crate::ui::alerts::AlertView {
                            notices: view.notices,
                            reports: view.reports,
                            fresh_alerts: view.fresh_alerts,
                            alert_times: view.alert_times,
                            hidden_until: view.alert_hide_deadlines,
                            now: view.now,
                            target_label: view.target_label,
                            focused: true,
                        },
                        event.column.saturating_sub(alerts_inner.x),
                    ) {
                        toggle_alert_severity_hide(
                            view.alert_hide_deadlines,
                            view.notices,
                            view.reports,
                            view.now,
                            severity,
                        );
                        sync_alert_selection(
                            view.alert_state,
                            view.notices,
                            view.reports,
                            view.alert_hide_deadlines,
                            view.now,
                        );
                    }
                } else if let Some(hit) = crate::ui::alerts::alert_hit_at_row(
                    view.alert_state,
                    crate::ui::alerts::AlertView {
                        notices: view.notices,
                        reports: view.reports,
                        fresh_alerts: view.fresh_alerts,
                        alert_times: view.alert_times,
                        hidden_until: view.alert_hide_deadlines,
                        now: view.now,
                        target_label: view.target_label,
                        focused: true,
                    },
                    alerts_inner.width.saturating_sub(3) as usize,
                    row.saturating_sub(1),
                ) {
                    view.alert_state.select(Some(hit.index));
                    if hit.dismiss {
                        *view.last_alert_click = None;
                        toggle_selected_alert_hide(
                            view.alert_hide_deadlines,
                            view.alert_state,
                            view.notices,
                            view.reports,
                            view.now,
                        );
                        sync_alert_selection(
                            view.alert_state,
                            view.notices,
                            view.reports,
                            view.alert_hide_deadlines,
                            view.now,
                        );
                    } else if let Some(key) = alert_key_at_index(
                        view.notices,
                        view.reports,
                        view.alert_hide_deadlines,
                        view.now,
                        hit.index,
                    ) && register_alert_double_click(
                        view.last_alert_click,
                        &key,
                        view.now,
                    ) {
                        toggle_selected_alert_hide(
                            view.alert_hide_deadlines,
                            view.alert_state,
                            view.notices,
                            view.reports,
                            view.now,
                        );
                        sync_alert_selection(
                            view.alert_state,
                            view.notices,
                            view.reports,
                            view.alert_hide_deadlines,
                            view.now,
                        );
                    }
                }
            } else if let Some(row) = list_row_at(rects.panes, event) {
                *view.focus = FocusedPanel::Panes;
                *view.last_alert_click = None;
                if let Some(idx) =
                    crate::ui::panels::pane_index_at_row(view.reports, view.pane_state, row)
                {
                    view.pane_state.select(Some(idx));
                }
            } else {
                *view.last_alert_click = None;
            }
            DashboardMouseAction::None
        }
        MouseEventKind::Drag(MouseButton::Left) if *view.split_dragging => {
            *view.split = dashboard_split_from_row(viewport, event.row);
            *view.last_alert_click = None;
            DashboardMouseAction::None
        }
        MouseEventKind::Up(MouseButton::Left) if *view.split_dragging => {
            *view.split = dashboard_split_from_row(viewport, event.row);
            *view.split_dragging = false;
            *view.last_alert_click = None;
            DashboardMouseAction::None
        }
        _ => DashboardMouseAction::None,
    }
}

pub fn sync_pane_selection(state: &mut ListState, pane_count: usize) {
    match pane_count {
        0 => state.select(None),
        count => {
            let selected = state.selected().unwrap_or(0).min(count.saturating_sub(1));
            state.select(Some(selected));
        }
    }
}

pub fn sync_alert_selection(
    state: &mut ListState,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    alert_hide_deadlines: &HashMap<String, Instant>,
    now: Instant,
) {
    let count = crate::ui::alerts::alert_count(notices, reports, alert_hide_deadlines, now);
    match count {
        0 => state.select(None),
        total => {
            let selected = state.selected().unwrap_or(0).min(total.saturating_sub(1));
            state.select(Some(selected));
        }
    }
}

pub fn toggle_selected_alert_hide(
    alert_hide_deadlines: &mut HashMap<String, Instant>,
    alert_state: &ListState,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    now: Instant,
) {
    let Some(selected_idx) = alert_state.selected() else {
        return;
    };
    let keys = crate::ui::alerts::visible_alert_keys(notices, reports, alert_hide_deadlines, now);
    let Some(key) = keys.get(selected_idx) else {
        return;
    };
    if alert_hide_deadlines.remove(key).is_none() {
        alert_hide_deadlines.insert(key.clone(), now + crate::ui::alerts::ALERT_AUTO_HIDE_DELAY);
    }
}

pub fn toggle_alert_severity_hide(
    alert_hide_deadlines: &mut HashMap<String, Instant>,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    now: Instant,
    severity: Severity,
) {
    let keys = crate::ui::alerts::actionable_alert_keys_for_severity(
        notices,
        reports,
        alert_hide_deadlines,
        now,
        severity,
    );
    if keys.is_empty() {
        return;
    }
    let all_pending = keys.iter().all(|key| {
        alert_hide_deadlines
            .get(key)
            .is_some_and(|deadline| *deadline > now)
    });
    if all_pending {
        for key in keys {
            alert_hide_deadlines.remove(&key);
        }
        return;
    }
    for key in keys {
        alert_hide_deadlines.insert(key, now + crate::ui::alerts::ALERT_AUTO_HIDE_DELAY);
    }
}

pub fn alert_key_at_index(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    alert_hide_deadlines: &HashMap<String, Instant>,
    now: Instant,
    idx: usize,
) -> Option<String> {
    crate::ui::alerts::visible_alert_keys(notices, reports, alert_hide_deadlines, now)
        .get(idx)
        .cloned()
}

pub fn register_alert_double_click(
    last_click: &mut Option<AlertMouseClick>,
    key: &str,
    now: Instant,
) -> bool {
    if last_click.as_ref().is_some_and(|previous| {
        previous.key == key
            && now.saturating_duration_since(previous.at) <= ALERT_DOUBLE_CLICK_WINDOW
    }) {
        *last_click = None;
        return true;
    }

    *last_click = Some(AlertMouseClick {
        key: key.to_string(),
        at: now,
    });
    false
}

pub fn sync_dashboard_state(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    state: DashboardSyncState<'_>,
    now: Instant,
) {
    sync_pane_selection(state.pane_state, reports.len());
    refresh_alert_state(
        notices,
        reports,
        state.previous_alerts,
        state.fresh_alerts,
        state.alert_times,
        state.alert_hide_deadlines,
    );
    sync_alert_selection(
        state.alert_state,
        notices,
        reports,
        state.alert_hide_deadlines,
        now,
    );
}

pub fn update_pane_state_flashes(
    reports: &[PaneReport],
    last_states: &mut HashMap<String, Option<IdleCause>>,
    flashes: &mut HashMap<String, crate::ui::panels::PaneStateFlash>,
    now: Instant,
) {
    let current_panes: HashSet<&str> = reports
        .iter()
        .map(|report| report.pane_id.as_str())
        .collect();
    last_states.retain(|pane_id, _| current_panes.contains(pane_id.as_str()));
    flashes
        .retain(|pane_id, flash| current_panes.contains(pane_id.as_str()) && flash.is_active(now));

    for report in reports {
        let current = report.idle_state;
        match last_states.insert(report.pane_id.clone(), current) {
            Some(previous) if previous != current => {
                flashes.insert(
                    report.pane_id.clone(),
                    crate::ui::panels::PaneStateFlash::new(current, now),
                );
            }
            _ => {}
        }
    }
}

pub fn refresh_alert_state(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    previous_alerts: &mut HashSet<String>,
    fresh_alerts: &mut HashSet<String>,
    alert_times: &mut HashMap<String, String>,
    alert_hide_deadlines: &mut HashMap<String, Instant>,
) {
    let current = crate::ui::alerts::alert_fingerprints(notices, reports);
    let timestamp = Local::now().format("%H:%M:%S").to_string();
    let disappeared: Vec<String> = previous_alerts.difference(&current).cloned().collect();
    for key in disappeared {
        alert_times.remove(&key);
    }
    alert_hide_deadlines.retain(|key, _| current.contains(key));

    *fresh_alerts = current.difference(previous_alerts).cloned().collect();
    for key in fresh_alerts.iter() {
        alert_times.insert(key.clone(), timestamp.clone());
    }
    *previous_alerts = current;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Recommendation;
    use crate::domain::signal::SignalSet;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    fn base_report(recs: Vec<Recommendation>) -> PaneReport {
        PaneReport {
            pane_id: "%1".into(),
            session_name: "qwork".into(),
            window_index: "1".into(),
            provider: Provider::Claude,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Claude,
                    instance: 1,
                    role: Role::Main,
                    pane_id: "%1".into(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            effects: vec![],
            dead: false,
            recommendations: recs,
            current_path: "/repo".into(),
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
            recent_token_samples: Vec::new(), // F-3: test fixture; production fetches via event_loop
        }
    }

    #[test]
    fn dashboard_selection_key_moves_pane_selection() {
        let reports = vec![base_report(vec![]), base_report(vec![])];
        let mut alert_state = ListState::default();
        let mut pane_state = ListState::default();
        let mut hidden = HashMap::new();
        pane_state.select(Some(0));

        let handled = handle_dashboard_selection_key(
            DashboardSelectionKeyView {
                focus: FocusedPanel::Panes,
                alert_state: &mut alert_state,
                pane_state: &mut pane_state,
                notices: &[],
                reports: &reports,
                alert_hide_deadlines: &mut hidden,
                now: Instant::now(),
            },
            KeyCode::Down,
        );

        assert!(handled);
        assert_eq!(pane_state.selected(), Some(1));
    }

    #[test]
    fn dashboard_selection_key_toggles_selected_alert_hide() {
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "waiting".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let reports = vec![base_report(vec![rec])];
        let mut alert_state = ListState::default();
        let mut pane_state = ListState::default();
        let mut hidden = HashMap::new();
        alert_state.select(Some(0));

        let handled = handle_dashboard_selection_key(
            DashboardSelectionKeyView {
                focus: FocusedPanel::Alerts,
                alert_state: &mut alert_state,
                pane_state: &mut pane_state,
                notices: &[],
                reports: &reports,
                alert_hide_deadlines: &mut hidden,
                now: Instant::now(),
            },
            KeyCode::Enter,
        );

        assert!(handled);
        assert!(!hidden.is_empty());
    }

    #[test]
    fn dashboard_mouse_pane_click_selects_pane() {
        let reports = vec![base_report(vec![]), base_report(vec![])];
        let mut focus = FocusedPanel::Alerts;
        let mut split = DashboardSplit::default();
        let mut dragging = false;
        let mut alert_state = ListState::default();
        let mut pane_state = ListState::default();
        let mut last_click = None;
        let mut hidden = HashMap::new();
        let fresh = HashSet::new();
        let times = HashMap::new();
        pane_state.select(Some(1));
        let viewport = Rect::new(0, 0, 100, 40);
        let rects = dashboard_rects(viewport, split);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rects.panes.x + 1,
            row: rects.panes.y + 1,
            modifiers: KeyModifiers::empty(),
        };

        let action = handle_dashboard_mouse(
            viewport,
            event,
            DashboardMouseView {
                focus: &mut focus,
                split: &mut split,
                split_dragging: &mut dragging,
                alert_state: &mut alert_state,
                pane_state: &mut pane_state,
                last_alert_click: &mut last_click,
                alert_hide_deadlines: &mut hidden,
                notices: &[],
                reports: &reports,
                fresh_alerts: &fresh,
                alert_times: &times,
                target_label: "all sessions",
                now: Instant::now(),
            },
        );

        assert_eq!(action, DashboardMouseAction::None);
        assert_eq!(focus, FocusedPanel::Panes);
        assert_eq!(pane_state.selected(), Some(0));
    }

    #[test]
    fn dashboard_mouse_divider_drag_starts_resize() {
        let reports = vec![base_report(vec![])];
        let mut focus = FocusedPanel::Alerts;
        let mut split = DashboardSplit::default();
        let mut dragging = false;
        let mut alert_state = ListState::default();
        let mut pane_state = ListState::default();
        let mut last_click = Some(AlertMouseClick {
            key: "alert".into(),
            at: Instant::now(),
        });
        let mut hidden = HashMap::new();
        let fresh = HashSet::new();
        let times = HashMap::new();
        let viewport = Rect::new(0, 0, 100, 40);
        let rects = dashboard_rects(viewport, split);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rects.divider.x,
            row: rects.divider.y,
            modifiers: KeyModifiers::empty(),
        };

        let action = handle_dashboard_mouse(
            viewport,
            event,
            DashboardMouseView {
                focus: &mut focus,
                split: &mut split,
                split_dragging: &mut dragging,
                alert_state: &mut alert_state,
                pane_state: &mut pane_state,
                last_alert_click: &mut last_click,
                alert_hide_deadlines: &mut hidden,
                notices: &[],
                reports: &reports,
                fresh_alerts: &fresh,
                alert_times: &times,
                target_label: "all sessions",
                now: Instant::now(),
            },
        );

        assert_eq!(action, DashboardMouseAction::None);
        assert!(dragging);
        assert!(last_click.is_none());
    }

    #[test]
    fn update_pane_state_flashes_tracks_idle_and_active_transitions() {
        let now = Instant::now();
        let mut last_states = HashMap::new();
        let mut flashes = HashMap::new();
        let mut report = base_report(vec![]);

        update_pane_state_flashes(&[report.clone()], &mut last_states, &mut flashes, now);
        assert!(
            flashes.is_empty(),
            "initial observation should establish baseline without flashing"
        );

        report.idle_state = Some(IdleCause::InputWait);
        update_pane_state_flashes(
            &[report.clone()],
            &mut last_states,
            &mut flashes,
            now + Duration::from_millis(10),
        );
        assert_eq!(
            flashes.get("%1").map(|flash| flash.state),
            Some(Some(IdleCause::InputWait))
        );

        report.idle_state = None;
        update_pane_state_flashes(
            &[report],
            &mut last_states,
            &mut flashes,
            now + Duration::from_millis(20),
        );
        assert_eq!(flashes.get("%1").map(|flash| flash.state), Some(None));
    }

    #[test]
    fn register_alert_double_click_requires_same_key_within_window() {
        let now = Instant::now();
        let mut tracker = None;

        assert!(!register_alert_double_click(&mut tracker, "a", now));
        assert!(register_alert_double_click(
            &mut tracker,
            "a",
            now + Duration::from_millis(200)
        ));
        assert!(tracker.is_none());
    }

    #[test]
    fn register_alert_double_click_ignores_stale_or_different_clicks() {
        let now = Instant::now();
        let mut tracker = None;

        assert!(!register_alert_double_click(&mut tracker, "a", now));
        assert!(!register_alert_double_click(
            &mut tracker,
            "b",
            now + Duration::from_millis(200)
        ));
        assert!(!register_alert_double_click(
            &mut tracker,
            "b",
            now + Duration::from_millis(700)
        ));
    }

    #[test]
    fn toggle_alert_severity_hide_only_targets_actionable_alerts() {
        let notice = SystemNotice {
            title: "snapshot saved".into(),
            body: "/tmp/x".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
        };
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "waiting".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let reports = vec![base_report(vec![rec.clone()])];
        let now = Instant::now();
        let mut hidden = HashMap::new();

        toggle_alert_severity_hide(&mut hidden, &[notice], &reports, now, Severity::Good);

        let actionable = crate::ui::alerts::actionable_alert_keys_for_severity(
            &[],
            &reports,
            &HashMap::new(),
            now,
            Severity::Good,
        );
        assert_eq!(hidden.len(), actionable.len());
        assert!(actionable.iter().all(|key| hidden.contains_key(key)));
    }
}
