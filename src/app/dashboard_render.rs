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
    render_help_modal, render_provider_setup_modal, render_target_picker,
};
use crate::ui::panels::PaneStateFlash;
use crate::ui::pending_actions::{
    PendingActionsOverlay, PendingItem, render_pending_actions_modal,
};
use crate::ui::provider_setup::ProviderSetupOverlay;
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
    pub provider_setup_overlay: &'a ProviderSetupOverlay,
    pub metrics_overlay: &'a crate::ui::metrics::MetricsOverlay,
    pub anomaly_overlay: &'a crate::ui::anomaly_overlay::AnomalyOverlay,
    pub insights_overlay: &'a crate::ui::insights::InsightsOverlay,
    pub anomaly_events_ring: &'a crate::app::anomaly_events_ring::AnomalyEventsRing,
    pub mem_observations: &'a HashMap<String, crate::ui::metrics::MemObservation>,
    pub action_explainer: &'a crate::app::action_explainer::ActionExplainModal,
    pub pending_actions: &'a PendingActionsOverlay,
    pub pending_items: &'a [PendingItem],
    pub config: &'a QmonsterConfig,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct OverlayFocusFlags {
    target_picker_open: bool,
    git_modal_open: bool,
    help_modal_open: bool,
    settings_overlay_open: bool,
    provider_setup_overlay_open: bool,
    metrics_overlay_open: bool,
    anomaly_overlay_open: bool,
    insights_overlay_open: bool,
    action_explainer_open: bool,
    pending_actions_open: bool,
}

impl OverlayFocusFlags {
    fn from_view(view: &DashboardFrameView<'_>) -> Self {
        Self {
            target_picker_open: view.target_picker_open,
            git_modal_open: view.git_modal.is_open(),
            help_modal_open: view.help_modal.is_open(),
            settings_overlay_open: view.settings_overlay.is_open(),
            provider_setup_overlay_open: view.provider_setup_overlay.is_open(),
            metrics_overlay_open: view.metrics_overlay.is_open(),
            anomaly_overlay_open: view.anomaly_overlay.is_open(),
            insights_overlay_open: view.insights_overlay.is_open(),
            action_explainer_open: view.action_explainer.is_open(),
            pending_actions_open: view.pending_actions.is_open(),
        }
    }
}

fn overlay_owns_keyboard(flags: OverlayFocusFlags) -> bool {
    flags.target_picker_open
        || flags.git_modal_open
        || flags.help_modal_open
        || flags.settings_overlay_open
        || flags.provider_setup_overlay_open
        || flags.metrics_overlay_open
        || flags.anomaly_overlay_open
        || flags.insights_overlay_open
        || flags.action_explainer_open
        || flags.pending_actions_open
}

pub fn render_dashboard_frame(frame: &mut Frame<'_>, view: DashboardFrameView<'_>) {
    let overlay_owns_keyboard = overlay_owns_keyboard(OverlayFocusFlags::from_view(&view));
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
            alerts_focused: !overlay_owns_keyboard && view.focus == FocusedPanel::Alerts,
            panes_focused: !overlay_owns_keyboard && view.focus == FocusedPanel::Panes,
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

    if view.provider_setup_overlay.is_open() {
        render_provider_setup_modal(frame, view.provider_setup_overlay);
    }

    if view.metrics_overlay.is_open() {
        crate::ui::metrics::render_metrics_modal(
            frame,
            view.metrics_overlay,
            view.target_label,
            view.reports,
            view.mem_observations,
        );
    }

    if view.anomaly_overlay.is_open() {
        view.anomaly_overlay
            .render(frame, frame.area(), view.anomaly_events_ring);
    }

    if view.insights_overlay.is_open() {
        crate::ui::insights::render_insights_modal(frame, view.insights_overlay);
    }

    if let Some(view) = view.action_explainer.view() {
        crate::ui::action_explainer::render_action_explainer_modal(frame, view);
    }

    // v1.39 surface C: Pending Actions overlay. Rendered last so it
    // sits on top of every other overlay; tui_loop only opens it when
    // none of the higher-priority modals (action explainer / target /
    // help / etc.) are taking keystrokes.
    if view.pending_actions.is_open() {
        render_pending_actions_modal(
            frame,
            crate::ui::pending_actions::PendingActionsRenderCtx {
                overlay: view.pending_actions,
                items: view.pending_items,
                reports: view.reports,
                mode: view.config.actions.mode,
                allow_auto_prompt_send: view.config.actions.allow_auto_prompt_send,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{OverlayFocusFlags, overlay_owns_keyboard};

    #[test]
    fn overlay_focus_blocks_dashboard_for_each_modal_owner() {
        let cases = [
            OverlayFocusFlags {
                target_picker_open: true,
                ..OverlayFocusFlags::default()
            },
            OverlayFocusFlags {
                git_modal_open: true,
                ..OverlayFocusFlags::default()
            },
            OverlayFocusFlags {
                help_modal_open: true,
                ..OverlayFocusFlags::default()
            },
            OverlayFocusFlags {
                settings_overlay_open: true,
                ..OverlayFocusFlags::default()
            },
            OverlayFocusFlags {
                provider_setup_overlay_open: true,
                ..OverlayFocusFlags::default()
            },
            OverlayFocusFlags {
                metrics_overlay_open: true,
                ..OverlayFocusFlags::default()
            },
            OverlayFocusFlags {
                anomaly_overlay_open: true,
                ..OverlayFocusFlags::default()
            },
            OverlayFocusFlags {
                insights_overlay_open: true,
                ..OverlayFocusFlags::default()
            },
            OverlayFocusFlags {
                action_explainer_open: true,
                ..OverlayFocusFlags::default()
            },
            OverlayFocusFlags {
                pending_actions_open: true,
                ..OverlayFocusFlags::default()
            },
        ];

        for flags in cases {
            assert!(
                overlay_owns_keyboard(flags),
                "open overlay should force footer focus: overlay; flags={flags:?}"
            );
        }
    }

    #[test]
    fn overlay_focus_does_not_block_dashboard_when_all_closed() {
        assert!(!overlay_owns_keyboard(OverlayFocusFlags::default()));
    }
}
