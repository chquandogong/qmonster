use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::widgets::ListState;

use crate::app::dashboard_state::{
    DashboardSyncState, refresh_alert_state, sync_alert_selection, sync_dashboard_state,
    sync_pane_selection,
};
use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;

pub struct DashboardRuntimeState {
    pub reports: Vec<PaneReport>,
    pub notices: Vec<SystemNotice>,
    pub alert_state: ListState,
    pub pane_state: ListState,
    pub previous_alerts: HashSet<String>,
    pub fresh_alerts: HashSet<String>,
    pub alert_times: HashMap<String, String>,
    pub alert_hide_deadlines: HashMap<String, Instant>,
}

impl DashboardRuntimeState {
    pub fn new(notices: Vec<SystemNotice>, now: Instant) -> Self {
        let reports = Vec::new();
        let mut alert_state = ListState::default();
        let mut pane_state = ListState::default();
        let mut previous_alerts = HashSet::new();
        let mut fresh_alerts = HashSet::new();
        let mut alert_times = HashMap::new();
        let mut alert_hide_deadlines = HashMap::new();

        refresh_alert_state(
            &notices,
            &reports,
            &mut previous_alerts,
            &mut fresh_alerts,
            &mut alert_times,
            &mut alert_hide_deadlines,
        );
        sync_alert_selection(
            &mut alert_state,
            &notices,
            &reports,
            &alert_hide_deadlines,
            now,
        );
        sync_pane_selection(&mut pane_state, reports.len());

        Self {
            reports,
            notices,
            alert_state,
            pane_state,
            previous_alerts,
            fresh_alerts,
            alert_times,
            alert_hide_deadlines,
        }
    }

    pub fn resync(&mut self, now: Instant) {
        sync_dashboard_state(
            &self.notices,
            &self.reports,
            DashboardSyncState {
                alert_state: &mut self.alert_state,
                pane_state: &mut self.pane_state,
                previous_alerts: &mut self.previous_alerts,
                fresh_alerts: &mut self.fresh_alerts,
                alert_times: &mut self.alert_times,
                alert_hide_deadlines: &mut self.alert_hide_deadlines,
            },
            now,
        );
    }

    pub fn sync_alert_selection(&mut self, now: Instant) {
        sync_alert_selection(
            &mut self.alert_state,
            &self.notices,
            &self.reports,
            &self.alert_hide_deadlines,
            now,
        );
    }

    pub fn set_reports(&mut self, reports: Vec<PaneReport>) {
        self.reports = reports;
    }

    pub fn push_notice(&mut self, notice: SystemNotice, now: Instant) {
        self.notices.insert(0, notice);
        self.resync(now);
    }

    pub fn replace_notices(&mut self, notices: Vec<SystemNotice>, now: Instant) {
        self.notices = notices;
        self.resync(now);
    }

    pub fn clear_notices(&mut self, now: Instant) {
        self.notices.clear();
        self.resync(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;

    fn notice(title: &str) -> SystemNotice {
        SystemNotice {
            title: title.into(),
            body: "body".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
        }
    }

    #[test]
    fn initializes_alert_selection_from_startup_notices() {
        let state = DashboardRuntimeState::new(vec![notice("startup")], Instant::now());

        assert_eq!(state.notices.len(), 1);
        assert_eq!(state.alert_state.selected(), Some(0));
        assert_eq!(state.pane_state.selected(), None);
    }

    #[test]
    fn notice_mutators_keep_alert_selection_synced() {
        let now = Instant::now();
        let mut state = DashboardRuntimeState::new(Vec::new(), now);

        state.push_notice(notice("first"), now);
        assert_eq!(state.alert_state.selected(), Some(0));

        state.clear_notices(now);
        assert_eq!(state.alert_state.selected(), None);
    }
}
