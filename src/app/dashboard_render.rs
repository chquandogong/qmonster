use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::Frame;
use ratatui::widgets::ListState;

use crate::app::config::QmonsterConfig;
use crate::app::event_loop::PaneReport;
use crate::app::keymap::FocusedPanel;
use crate::app::modal_state::ScrollModalState;
use crate::app::system_notice::SystemNotice;
use crate::app::target_picker::{
    TargetChoice, TargetPickerStage, target_picker_hint, target_picker_title,
};
use crate::ui::dashboard::{
    DashboardSplit, DashboardView, TargetPickerView, render_dashboard, render_git_modal,
    render_help_modal, render_target_picker,
};
use crate::ui::panels::PaneStateFlash;
use crate::ui::settings::{SettingsOverlay, render_settings_modal};

pub struct DashboardFrameView<'a> {
    pub alert_state: &'a mut ListState,
    pub pane_state: &'a mut ListState,
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub hidden_until: &'a HashMap<String, Instant>,
    pub state_flashes: &'a HashMap<String, PaneStateFlash>,
    pub now: Instant,
    pub target_label: &'a str,
    pub split: DashboardSplit,
    pub focus: FocusedPanel,
    pub target_picker_open: bool,
    pub target_picker_stage: TargetPickerStage,
    pub target_picker_session: Option<&'a str>,
    pub target_picker_state: &'a mut ListState,
    pub target_choices: &'a [TargetChoice],
    pub target_preview_title: &'a str,
    pub target_preview_lines: &'a [String],
    pub git_modal: &'a ScrollModalState,
    pub help_modal: &'a ScrollModalState,
    pub settings_overlay: &'a SettingsOverlay,
    pub config: &'a QmonsterConfig,
}

pub fn render_dashboard_frame(frame: &mut Frame<'_>, view: DashboardFrameView<'_>) {
    render_dashboard(
        frame,
        view.alert_state,
        view.pane_state,
        DashboardView {
            notices: view.notices,
            reports: view.reports,
            fresh_alerts: view.fresh_alerts,
            alert_times: view.alert_times,
            hidden_until: view.hidden_until,
            state_flashes: view.state_flashes,
            now: view.now,
            target_label: view.target_label,
            split: view.split,
            alerts_focused: !view.target_picker_open
                && !view.help_modal.is_open()
                && !view.settings_overlay.is_open()
                && view.focus == FocusedPanel::Alerts,
            panes_focused: !view.target_picker_open
                && !view.help_modal.is_open()
                && !view.settings_overlay.is_open()
                && view.focus == FocusedPanel::Panes,
        },
    );

    if view.target_picker_open {
        let labels: Vec<String> = view
            .target_choices
            .iter()
            .map(|choice| choice.label.clone())
            .collect();
        let picker_title =
            target_picker_title(view.target_picker_stage, view.target_picker_session);
        render_target_picker(
            frame,
            view.target_picker_state,
            TargetPickerView {
                title: &picker_title,
                hint: target_picker_hint(view.target_picker_stage),
                labels: &labels,
                preview_title: view.target_preview_title,
                preview_lines: view.target_preview_lines,
                current_label: view.target_label,
            },
        );
    }

    if view.git_modal.is_open() {
        render_git_modal(
            frame,
            view.git_modal.title(),
            view.git_modal.lines(),
            view.git_modal.scroll() as u16,
        );
    }

    if view.help_modal.is_open() {
        render_help_modal(frame, view.help_modal.scroll() as u16);
    }

    if view.settings_overlay.is_open() {
        render_settings_modal(frame, view.settings_overlay, view.config);
    }
}
