use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::Rect;

use crate::app::bootstrap::Context;
use crate::app::clipboard_actions::{
    AlertCommandCopyView, copy_selected_alert_command_to_clipboard,
};
use crate::app::dashboard_render::{DashboardFrameView, render_dashboard_frame};
use crate::app::dashboard_runtime::DashboardRuntimeState;
use crate::app::dashboard_state::{
    AlertMouseClick, DashboardMouseAction, DashboardMouseView, DashboardSelectionKeyView,
    handle_dashboard_mouse, handle_dashboard_selection_key,
};
use crate::app::git_info::capture_repo_panel;
use crate::app::keymap::{FocusedPanel, toggle_focus};
use crate::app::modal_state::{
    ScrollModalState, handle_scroll_modal_key, handle_scroll_modal_mouse,
};
use crate::app::operator_actions::{version_refresh_notices, write_operator_snapshot};
use crate::app::polling_tick::{PollTickState, handle_poll_tick};
use crate::app::prompt_send_actions::handle_prompt_send_action;
use crate::app::provider_setup_overlay::{
    copy_active_tab_snippet, handle_provider_setup_overlay_key, handle_provider_setup_overlay_mouse,
};
use crate::app::runtime_refresh::handle_runtime_refresh_action;
use crate::app::settings_overlay::{handle_settings_overlay_key, handle_settings_overlay_mouse};
use crate::app::system_notice::SystemNotice;
use crate::app::target_picker::{
    TargetPickerAction, TargetPickerRuntimeState, handle_target_picker_key,
    handle_target_picker_mouse, open_target_picker, target_label, target_switched_notice,
};
use crate::app::terminal_session::{enter_terminal_session, leave_terminal_session};
use crate::app::version_drift::{VersionSnapshot, capture_versions};
use crate::domain::signal::IdleCause;
use crate::notify::desktop::NotifyBackend;
use crate::store::SnapshotWriter;
use crate::tmux::polling::PaneSource;
use crate::ui::dashboard::{DashboardSplit, close_button_rect, git_modal_rects, help_modal_rects};

// Phase 5 P5-3 second gate types (`PromptSendGate` + `check_send_gate`)
// were moved to `crate::policy::gates` in v1.10.1 remediation
// (Gemini v1.10.0 finding #1 closed). The TUI keystroke handler below
// imports them through helper modules.
pub fn run_tui<P, N>(
    ctx: &mut Context<P, N>,
    mut versions: VersionSnapshot,
    snapshot_writer: SnapshotWriter,
    startup_notices: Vec<SystemNotice>,
) -> anyhow::Result<()>
where
    P: PaneSource,
    N: NotifyBackend,
{
    let mut terminal = enter_terminal_session()?;

    let poll = ctx.config.tmux.poll_interval();
    let startup_now = Instant::now();
    let mut dashboard = DashboardRuntimeState::new(startup_notices, startup_now);
    let mut last_poll = startup_now - poll;
    let mut last_tmux_source_error: Option<String> = None;
    let mut target_picker = TargetPickerRuntimeState::new(&ctx.source);
    let mut focus = FocusedPanel::Alerts;
    let mut dashboard_split = DashboardSplit::default();
    let mut dashboard_split_dragging = false;
    let mut git_modal = ScrollModalState::default();
    let mut help_modal = ScrollModalState::default();
    let mut settings_overlay = crate::ui::settings::SettingsOverlay::new();
    let mut provider_setup_overlay =
        crate::ui::provider_setup::ProviderSetupOverlay::from_config(&ctx.config);
    let mut last_alert_click: Option<AlertMouseClick> = None;
    let mut last_pane_idle_states: HashMap<String, Option<IdleCause>> = HashMap::new();
    let mut pane_state_flashes: HashMap<String, crate::ui::panels::PaneStateFlash> = HashMap::new();
    let mut runtime_refresh_offsets: HashMap<String, usize> = HashMap::new();

    let result = {
        let mut run_loop = || -> anyhow::Result<()> {
            loop {
                let now = Instant::now();
                if now.saturating_duration_since(last_poll) >= poll {
                    last_poll = now;
                    let outcome = handle_poll_tick(
                        ctx,
                        now,
                        target_picker.selected_target.as_ref(),
                        PollTickState {
                            last_source_error: &mut last_tmux_source_error,
                            last_pane_idle_states: &mut last_pane_idle_states,
                            pane_state_flashes: &mut pane_state_flashes,
                        },
                    );
                    if let Some(notice) = outcome.notice {
                        dashboard.notices.insert(0, notice);
                    }
                    if let Some(reports) = outcome.reports {
                        dashboard.set_reports(reports);
                    }
                    if outcome.resync_dashboard {
                        dashboard.resync(now);
                    }
                }

                pane_state_flashes.retain(|_, flash| flash.is_active(now));
                dashboard.sync_alert_selection(now);
                let target = target_label(target_picker.selected_target.as_ref());
                terminal.draw(|frame| {
                    render_dashboard_frame(
                        frame,
                        DashboardFrameView {
                            alert_state: &mut dashboard.alert_state,
                            pane_state: &mut dashboard.pane_state,
                            notices: &dashboard.notices,
                            reports: &dashboard.reports,
                            fresh_alerts: &dashboard.fresh_alerts,
                            alert_times: &dashboard.alert_times,
                            hidden_until: &dashboard.alert_hide_deadlines,
                            state_flashes: &pane_state_flashes,
                            now,
                            target_label: &target,
                            split: dashboard_split,
                            focus,
                            target_picker_open: target_picker.open,
                            target_picker_stage: target_picker.stage,
                            target_picker_session: target_picker.session.as_deref(),
                            target_picker_state: &mut target_picker.state,
                            target_choices: &target_picker.choices,
                            target_preview_title: &target_picker.preview_title,
                            target_preview_lines: &target_picker.preview_lines,
                            git_modal: &git_modal,
                            help_modal: &help_modal,
                            settings_overlay: &settings_overlay,
                            provider_setup_overlay: &provider_setup_overlay,
                            config: &ctx.config,
                        },
                    );
                })?;

                if event::poll(Duration::from_millis(100))? {
                    match event::read()? {
                        Event::Key(k) if k.kind == KeyEventKind::Press => {
                            if git_modal.is_open() {
                                let size = terminal.size()?;
                                let max_scroll = crate::ui::dashboard::max_git_scroll(
                                    Rect::new(0, 0, size.width, size.height),
                                    git_modal.line_count(),
                                );
                                handle_scroll_modal_key(&mut git_modal, k.code, max_scroll, None);
                                continue;
                            }

                            if help_modal.is_open() {
                                let size = terminal.size()?;
                                let max_scroll = crate::ui::dashboard::max_help_scroll(Rect::new(
                                    0,
                                    0,
                                    size.width,
                                    size.height,
                                ));
                                handle_scroll_modal_key(
                                    &mut help_modal,
                                    k.code,
                                    max_scroll,
                                    Some(KeyCode::Char('?')),
                                );
                                continue;
                            }

                            if target_picker.open {
                                let action = handle_target_picker_key(
                                    &ctx.source,
                                    target_picker.controller(),
                                    k.code,
                                );
                                if let TargetPickerAction::TargetSwitched(label) = action {
                                    let now = Instant::now();
                                    dashboard.push_notice(target_switched_notice(&label), now);
                                    last_poll = now - poll;
                                }
                                continue;
                            }

                            if settings_overlay.is_open() {
                                let config_path = ctx.config_path.clone();
                                handle_settings_overlay_key(
                                    &mut settings_overlay,
                                    &mut ctx.config,
                                    config_path.as_deref(),
                                    k.code,
                                );
                                continue;
                            }

                            if provider_setup_overlay.is_open() {
                                if k.code == KeyCode::Char('y') {
                                    let notice = copy_active_tab_snippet(
                                        &provider_setup_overlay,
                                        crate::app::clipboard_actions::copy_text_to_clipboard,
                                    );
                                    dashboard.push_notice(notice, Instant::now());
                                    continue;
                                }
                                handle_provider_setup_overlay_key(
                                    &mut provider_setup_overlay,
                                    k.code,
                                );
                                continue;
                            }

                            let now = Instant::now();
                            if matches!(k.code, KeyCode::Char('c') | KeyCode::Char('C')) {
                                dashboard.clear_notices(now);
                                continue;
                            }

                            if handle_dashboard_selection_key(
                                DashboardSelectionKeyView {
                                    focus,
                                    alert_state: &mut dashboard.alert_state,
                                    pane_state: &mut dashboard.pane_state,
                                    notices: &dashboard.notices,
                                    reports: &dashboard.reports,
                                    alert_hide_deadlines: &mut dashboard.alert_hide_deadlines,
                                    now,
                                },
                                k.code,
                            ) {
                                continue;
                            }

                            match k.code {
                                KeyCode::Char('q') | KeyCode::Esc => break,
                                KeyCode::Tab => focus = toggle_focus(focus),
                                KeyCode::Char('[') => dashboard_split.shrink_alerts(),
                                KeyCode::Char(']') => dashboard_split.grow_alerts(),
                                KeyCode::Char('/') => dashboard_split.cycle_alerts(),
                                KeyCode::Char('=') => dashboard_split.reset(),
                                KeyCode::Char('?') => {
                                    help_modal.open("", Vec::new());
                                }
                                KeyCode::Char('S') => settings_overlay.open(),
                                KeyCode::Char('P') => provider_setup_overlay.open(),
                                KeyCode::Char('t') => {
                                    open_target_picker(&ctx.source, target_picker.controller());
                                }
                                KeyCode::Char('r') => {
                                    let fresh = capture_versions();
                                    let new_notices =
                                        version_refresh_notices(&versions, &fresh, &*ctx.sink);
                                    if !new_notices.is_empty() {
                                        dashboard.replace_notices(new_notices, Instant::now());
                                    }
                                    versions = fresh;
                                }
                                KeyCode::Char('s') => {
                                    let notice = write_operator_snapshot(
                                        &snapshot_writer,
                                        &*ctx.sink,
                                        &dashboard.reports,
                                        &dashboard.notices,
                                    );
                                    dashboard.push_notice(notice, Instant::now());
                                }
                                KeyCode::Char('u') if focus == FocusedPanel::Panes => {
                                    let selected = dashboard
                                        .pane_state
                                        .selected()
                                        .and_then(|i| dashboard.reports.get(i));
                                    let outcome = handle_runtime_refresh_action(
                                        &ctx.source,
                                        &*ctx.sink,
                                        selected,
                                        ctx.config.actions.mode,
                                        ctx.config.tmux.capture_lines,
                                        &mut runtime_refresh_offsets,
                                        &mut ctx.runtime_refresh_tail_overlays,
                                    );
                                    if outcome.force_poll {
                                        last_poll = Instant::now() - poll;
                                    }
                                    dashboard.push_notice(outcome.notice, Instant::now());
                                }
                                KeyCode::Char('y') if focus == FocusedPanel::Alerts => {
                                    let now = Instant::now();
                                    let notice = copy_selected_alert_command_to_clipboard(
                                        AlertCommandCopyView {
                                            alert_state: &dashboard.alert_state,
                                            notices: &dashboard.notices,
                                            reports: &dashboard.reports,
                                            fresh_alerts: &dashboard.fresh_alerts,
                                            alert_times: &dashboard.alert_times,
                                            hidden_until: &dashboard.alert_hide_deadlines,
                                            now,
                                        },
                                    );
                                    dashboard.push_notice(notice, now);
                                }
                                KeyCode::Char('p') | KeyCode::Char('d') => {
                                    let notice = handle_prompt_send_action(
                                        &ctx.source,
                                        &*ctx.sink,
                                        &dashboard.reports,
                                        dashboard.pane_state.selected(),
                                        k.code == KeyCode::Char('p'),
                                        ctx.config.actions.mode,
                                        ctx.config.actions.allow_auto_prompt_send,
                                    );
                                    dashboard.push_notice(notice, Instant::now());
                                }
                                _ => {}
                            }
                        }
                        Event::Mouse(m) => {
                            let size = terminal.size()?;
                            let viewport = Rect::new(0, 0, size.width, size.height);
                            let now = Instant::now();

                            if settings_overlay.is_open() {
                                dashboard_split_dragging = false;
                                handle_settings_overlay_mouse(&mut settings_overlay, viewport, m);
                                continue;
                            }

                            if provider_setup_overlay.is_open() {
                                dashboard_split_dragging = false;
                                handle_provider_setup_overlay_mouse(
                                    &mut provider_setup_overlay,
                                    viewport,
                                    m,
                                );
                                continue;
                            }

                            if git_modal.is_open() {
                                dashboard_split_dragging = false;
                                let rects = git_modal_rects(viewport);
                                let max_scroll = crate::ui::dashboard::max_git_scroll(
                                    viewport,
                                    git_modal.line_count(),
                                );
                                handle_scroll_modal_mouse(
                                    &mut git_modal,
                                    m,
                                    rects.body,
                                    close_button_rect(rects.body),
                                    max_scroll,
                                );
                                continue;
                            }

                            if help_modal.is_open() {
                                dashboard_split_dragging = false;
                                let rects = help_modal_rects(viewport);
                                let max_scroll = crate::ui::dashboard::max_help_scroll(viewport);
                                handle_scroll_modal_mouse(
                                    &mut help_modal,
                                    m,
                                    rects.body,
                                    close_button_rect(rects.body),
                                    max_scroll,
                                );
                                continue;
                            }

                            if target_picker.open {
                                dashboard_split_dragging = false;
                                let action = handle_target_picker_mouse(
                                    &ctx.source,
                                    target_picker.controller(),
                                    viewport,
                                    m,
                                );
                                if let TargetPickerAction::TargetSwitched(label) = action {
                                    dashboard.push_notice(target_switched_notice(&label), now);
                                    last_poll = now - poll;
                                }
                                continue;
                            }

                            let action = handle_dashboard_mouse(
                                viewport,
                                m,
                                DashboardMouseView {
                                    focus: &mut focus,
                                    split: &mut dashboard_split,
                                    split_dragging: &mut dashboard_split_dragging,
                                    alert_state: &mut dashboard.alert_state,
                                    pane_state: &mut dashboard.pane_state,
                                    last_alert_click: &mut last_alert_click,
                                    alert_hide_deadlines: &mut dashboard.alert_hide_deadlines,
                                    notices: &dashboard.notices,
                                    reports: &dashboard.reports,
                                    fresh_alerts: &dashboard.fresh_alerts,
                                    alert_times: &dashboard.alert_times,
                                    target_label: &target,
                                    now,
                                },
                            );
                            if action == DashboardMouseAction::OpenGitModal {
                                let panel = capture_repo_panel();
                                git_modal.open(panel.title, panel.lines);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        };
        run_loop()
    };

    leave_terminal_session();
    result
}
