use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use chrono::Local;
use ratatui::widgets::ListState;

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::recommendation::Severity;
use crate::domain::signal::IdleCause;

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
        }
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
