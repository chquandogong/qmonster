use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::widgets::ListState;

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;

pub struct AlertCommandCopyView<'a> {
    pub alert_state: &'a ListState,
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub hidden_until: &'a HashMap<String, Instant>,
    pub now: Instant,
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| e.to_string())
}

pub fn copy_selected_alert_command_to_clipboard(view: AlertCommandCopyView<'_>) -> SystemNotice {
    copy_selected_alert_command(view, copy_text_to_clipboard)
}

pub fn copy_selected_alert_command<F>(view: AlertCommandCopyView<'_>, copy_text: F) -> SystemNotice
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let command = crate::ui::alerts::selected_alert_suggested_command(
        view.alert_state,
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    match command {
        Some(cmd) => match copy_text(&cmd) {
            Ok(()) => SystemNotice {
                title: "command copied".into(),
                body: format!("`{cmd}`"),
                severity: Severity::Good,
                source_kind: SourceKind::ProjectCanonical,
            },
            Err(e) => SystemNotice {
                title: "clipboard unavailable".into(),
                body: format!("could not copy command: {e}"),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
            },
        },
        None => SystemNotice {
            title: "no command selected".into(),
            body: "select an alert with a run command before pressing y".into(),
            severity: Severity::Concern,
            source_kind: SourceKind::ProjectCanonical,
        },
    }
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
    use std::cell::RefCell;

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
            recommendations: recs,
            effects: vec![],
            dead: false,
            current_path: "/repo".into(),
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
        }
    }

    fn recommendation_with_command(command: Option<&str>) -> Recommendation {
        Recommendation {
            action: "run command",
            reason: "operator can copy".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: command.map(str::to_string),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        }
    }

    fn view<'a>(
        state: &'a ListState,
        notices: &'a [SystemNotice],
        reports: &'a [PaneReport],
        fresh: &'a HashSet<String>,
        times: &'a HashMap<String, String>,
        hidden: &'a HashMap<String, Instant>,
    ) -> AlertCommandCopyView<'a> {
        AlertCommandCopyView {
            alert_state: state,
            notices,
            reports,
            fresh_alerts: fresh,
            alert_times: times,
            hidden_until: hidden,
            now: Instant::now(),
        }
    }

    #[test]
    fn copy_selected_alert_command_copies_run_command() {
        let mut state = ListState::default();
        state.select(Some(0));
        let reports = vec![base_report(vec![recommendation_with_command(Some(
            "cargo test",
        ))])];
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();
        let copied = RefCell::new(String::new());

        let notice = copy_selected_alert_command(
            view(&state, &[], &reports, &fresh, &times, &hidden),
            |text| {
                copied.replace(text.to_string());
                Ok(())
            },
        );

        assert_eq!(notice.title, "command copied");
        assert_eq!(copied.into_inner(), "cargo test");
    }

    #[test]
    fn copy_selected_alert_command_reports_clipboard_failure() {
        let mut state = ListState::default();
        state.select(Some(0));
        let reports = vec![base_report(vec![recommendation_with_command(Some(
            "cargo test",
        ))])];
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();

        let notice = copy_selected_alert_command(
            view(&state, &[], &reports, &fresh, &times, &hidden),
            |_| Err("not available".into()),
        );

        assert_eq!(notice.title, "clipboard unavailable");
        assert_eq!(notice.severity, Severity::Warning);
    }

    #[test]
    fn copy_selected_alert_command_reports_missing_command() {
        let mut state = ListState::default();
        state.select(Some(0));
        let reports = vec![base_report(vec![recommendation_with_command(None)])];
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();

        let notice = copy_selected_alert_command(
            view(&state, &[], &reports, &fresh, &times, &hidden),
            |_| Ok(()),
        );

        assert_eq!(notice.title, "no command selected");
        assert_eq!(notice.severity, Severity::Concern);
    }
}
