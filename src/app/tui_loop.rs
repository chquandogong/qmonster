use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEventKind};
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
use crate::app::hover_help::{
    DashboardHoverView, HoverHelpState, dashboard_forced_hover_topic, dashboard_hover_topic,
};
use crate::app::keymap::{FocusedPanel, toggle_focus};
use crate::app::modal_state::{
    ScrollModalState, handle_scroll_modal_key, handle_scroll_modal_mouse,
};
use crate::app::operator_actions::{version_refresh_notices, write_operator_snapshot};
use crate::app::polling_tick::{PollTickState, handle_poll_tick};
use crate::app::provider_setup_overlay::{
    copy_active_tab_snippet, handle_provider_setup_overlay_key, handle_provider_setup_overlay_mouse,
};
use crate::app::runtime_refresh::handle_runtime_refresh_action;
use crate::app::settings_overlay::{
    handle_settings_overlay_key_with_viewport, handle_settings_overlay_mouse,
};
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

    // v1.58.0: apply the configured theme variant before any render so
    // theme:: accessors return the right palette from the first frame.
    crate::ui::theme::set_theme_mode(ctx.config.ux.theme.into());

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
    let mut action_explainer = crate::app::action_explainer::ActionExplainModal::new();
    let mut hover_help = HoverHelpState::new();
    // Restored v3.1.4: decorative effects overlay (Q hotkey, prompt-send
    // celebration, idle screensaver). Inert until a trigger fires.
    let mut fx_overlay = crate::app::fx_overlay::FxOverlay::new();
    let mut last_user_activity = startup_now;

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
                    // Phase H (v1.42.0): auto-snapshot notices arrive
                    // newest-first so prepending preserves temporal order.
                    for auto_notice in outcome.auto_notices.into_iter().rev() {
                        dashboard.notices.insert(0, auto_notice);
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
                let alert_filter_snapshot = dashboard.alert_filter().map(|s| s.to_string());
                let audit_recent_severity = ctx.sqlite_db_path.as_deref().and_then(|path| {
                    crate::store::recent_audit_max_severity(path, 15 * 60)
                        .ok()
                        .flatten()
                });
                dashboard.audit_recent_severity = audit_recent_severity;
                // Restored v3.1.4: idle screensaver auto-trigger +
                // per-frame scene step. Pure gating helper keeps this
                // unit-testable; `step` advances the active effect's
                // particles / banner / streams.
                {
                    let term_size = terminal.size()?;
                    if crate::app::fx_overlay::should_auto_open_screensaver(
                        &ctx.config.fx,
                        &fx_overlay,
                        last_user_activity,
                        now,
                    ) {
                        fx_overlay.open(
                            &ctx.config.fx,
                            crate::app::fx_overlay::FxTrigger::Screensaver,
                            now,
                            term_size.width,
                            term_size.height,
                        );
                    }
                    if fx_overlay.is_open() {
                        fx_overlay.step(now, term_size.width, term_size.height);
                    }
                }
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
                            audit_recent_severity,
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
                            anomaly_events_ring: &ctx.anomaly_events_ring,
                            action_explainer: &action_explainer,
                            hover_help: &hover_help,
                            config: &ctx.config,
                            ime_active: ctx.ime_state.is_active(now),
                            alert_filter: alert_filter_snapshot.as_deref(),
                            fx_overlay: &fx_overlay,
                            fx_text: ctx.config.fx.text.as_str(),
                        },
                    );
                })?;

                // Restored v3.1.4: tighten poll cadence to ~30 FPS while
                // the fx overlay is open so banner / confetti / matrix
                // animate smoothly; closed → keep the 100ms cadence so
                // the quiet path stays cheap.
                let poll_ms = if fx_overlay.is_open() {
                    crate::app::fx_state::FX_FRAME_INTERVAL_MS
                } else {
                    100
                };
                if event::poll(Duration::from_millis(poll_ms))? {
                    match event::read()? {
                        Event::Key(k) if k.kind == KeyEventKind::Press => {
                            // Restored v3.1.4: any keypress refreshes the
                            // screensaver idle clock; while the fx overlay
                            // is open a keypress dismisses it and is
                            // consumed so the operator regains control on
                            // a single press.
                            last_user_activity = Instant::now();
                            if fx_overlay.is_open() {
                                fx_overlay.dismiss();
                                continue;
                            }
                            // v1.51.0: feed Char keystrokes into the heuristic
                            // IME indicator BEFORE per-overlay dispatch so it
                            // observes keys consumed by any modal as well as
                            // the dashboard. Bell on inactive→active edges
                            // only — the user accepted the heuristic limit
                            // that the very first non-ASCII key is what lights
                            // the indicator (terminals don't expose IME state).
                            if let KeyCode::Char(c) = k.code
                                && matches!(
                                    ctx.ime_state.observe(c, Instant::now()),
                                    crate::app::ime_state::ImeObservation::NonAsciiSet {
                                        transitioned_on: true
                                    }
                                )
                            {
                                ring_terminal_bell();
                            }

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
                                let viewport = Rect::new(0, 0, size.width, size.height);
                                let max_scroll = crate::ui::dashboard::max_help_scroll(viewport);
                                // v1.60.0: digit keys jump to the
                                // matching help section. `1` always
                                // lands on Controls; later digits map
                                // to Hover Help / Source Labels /
                                // State Labels in document order.
                                if let KeyCode::Char(c) = k.code
                                    && let Some(digit) = c.to_digit(10)
                                    && digit >= 1
                                {
                                    let sections =
                                        crate::ui::dashboard::help_section_line_indices(viewport);
                                    if let Some(target) = sections.get((digit - 1) as usize) {
                                        help_modal.set_scroll(*target, max_scroll);
                                        continue;
                                    }
                                }
                                handle_scroll_modal_key(
                                    &mut help_modal,
                                    k.code,
                                    max_scroll,
                                    Some(KeyCode::Char('?')),
                                );
                                continue;
                            }

                            if target_picker.open {
                                if crate::app::target_picker::target_picker_entry_key_closes(
                                    &target_picker,
                                    k.code,
                                ) {
                                    target_picker.open = false;
                                    continue;
                                }
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
                                if crate::app::settings_overlay::settings_entry_key_closes(
                                    &settings_overlay,
                                    k.code,
                                ) {
                                    settings_overlay.close();
                                    continue;
                                }
                                let config_path = ctx.config_path.clone();
                                let size = terminal.size()?;
                                handle_settings_overlay_key_with_viewport(
                                    &mut settings_overlay,
                                    &mut ctx.config,
                                    config_path.as_deref(),
                                    Rect::new(0, 0, size.width, size.height),
                                    k.code,
                                );
                                continue;
                            }

                            if provider_setup_overlay.is_open() {
                                if crate::app::provider_setup_overlay::provider_setup_entry_key_closes(
                                    &provider_setup_overlay,
                                    k.code,
                                ) {
                                    provider_setup_overlay.close();
                                    continue;
                                }
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

                            if action_explainer.is_open() {
                                let now = Instant::now();
                                match k.code {
                                    KeyCode::Enter => {
                                        if let Some(action) = action_explainer.pending().cloned() {
                                            action_explainer.mark_seen(&action);
                                            // Restored v3.1.4: capture the accept edge
                                            // before confirm so celebration fires only
                                            // on an AcceptPromptSend.
                                            let was_accept = matches!(
                                                action,
                                                crate::app::action_explainer::PendingAction::AcceptPromptSend { .. }
                                            );
                                            let notice = confirm_pending_action(
                                                &action,
                                                &ctx.source,
                                                &*ctx.sink,
                                                &dashboard.reports,
                                                ctx.config.actions.mode,
                                                ctx.config.actions.allow_auto_prompt_send,
                                            );
                                            action_explainer.close();
                                            dashboard.push_notice(notice, now);
                                            if was_accept
                                                && ctx.config.fx.celebration_enabled
                                                && ctx.config.fx.enabled
                                            {
                                                let term_size = terminal.size()?;
                                                fx_overlay.open(
                                                    &ctx.config.fx,
                                                    crate::app::fx_overlay::FxTrigger::Celebration,
                                                    now,
                                                    term_size.width,
                                                    term_size.height,
                                                );
                                            }
                                        }
                                        continue;
                                    }
                                    KeyCode::Esc | KeyCode::Char('q') => {
                                        action_explainer.close();
                                        continue;
                                    }
                                    KeyCode::Char(c) => {
                                        if matches_originating_key(action_explainer.pending(), c) {
                                            action_explainer.close();
                                        }
                                        continue;
                                    }
                                    _ => continue,
                                }
                            }

                            // v1.59.0: Alerts filter input mode. Symmetric
                            // to the v1.58.0 Settings filter — `/` opens
                            // it (handled below in the main match), and
                            // while it's active typed chars narrow the
                            // alert list and Esc/Backspace/Enter route
                            // here instead of the dashboard selection
                            // handler. Other keys (Up/Down/etc.) fall
                            // through so navigation still works under a
                            // frozen filter.
                            if focus == FocusedPanel::Alerts && dashboard.alert_filter().is_some() {
                                match k.code {
                                    KeyCode::Esc => {
                                        dashboard.cancel_alert_filter();
                                        continue;
                                    }
                                    KeyCode::Enter => {
                                        dashboard.confirm_alert_filter();
                                        continue;
                                    }
                                    KeyCode::Backspace => {
                                        dashboard.alert_filter_backspace();
                                        continue;
                                    }
                                    KeyCode::Char('/') => {
                                        dashboard.cancel_alert_filter();
                                        dashboard.start_alert_filter();
                                        continue;
                                    }
                                    KeyCode::Char(c) if !c.is_control() => {
                                        dashboard.alert_filter_type_char(c);
                                        continue;
                                    }
                                    _ => {}
                                }
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
                                // v1.59.0: `/` starts the Alerts filter
                                // when Alerts is focused; otherwise keep
                                // the legacy split-cycle binding so Panes
                                // focus still has a layout shortcut.
                                KeyCode::Char('/') if focus == FocusedPanel::Alerts => {
                                    dashboard.start_alert_filter();
                                }
                                KeyCode::Char('/') => dashboard_split.cycle_alerts(),
                                KeyCode::Char('=') => dashboard_split.reset(),
                                KeyCode::Char('?') => {
                                    help_modal.open("", Vec::new());
                                }
                                KeyCode::Char('H') => {
                                    ctx.config.ux.hover_help = !ctx.config.ux.hover_help;
                                    if !ctx.config.ux.hover_help {
                                        hover_help.clear_hover();
                                    }
                                    dashboard.push_notice(
                                        SystemNotice {
                                            title: "hover help toggled".into(),
                                            body: format!(
                                                "floating help is now {}",
                                                if ctx.config.ux.hover_help {
                                                    "on"
                                                } else {
                                                    "off"
                                                }
                                            ),
                                            severity: crate::domain::recommendation::Severity::Good,
                                            source_kind:
                                                crate::domain::origin::SourceKind::ProjectCanonical,
                                        },
                                        Instant::now(),
                                    );
                                }
                                KeyCode::Char('L') => {
                                    ctx.config.ux.help_language =
                                        ctx.config.ux.help_language.toggle();
                                    dashboard.push_notice(
                                        SystemNotice {
                                            title: "hover help language".into(),
                                            body: format!(
                                                "floating help language is now {}",
                                                ctx.config.ux.help_language.as_str()
                                            ),
                                            severity: crate::domain::recommendation::Severity::Good,
                                            source_kind:
                                                crate::domain::origin::SourceKind::ProjectCanonical,
                                        },
                                        Instant::now(),
                                    );
                                }
                                KeyCode::Char('S') => settings_overlay.open(),
                                // v3.1.2: the legacy `K` footer-keys hover binding was
                                // dropped (operator shortcut cleanup). It duplicated `?`
                                // (full help modal) and the still-clickable `[keys]`
                                // footer badge, and was absent from the `?` help list.
                                KeyCode::Char('P') => {
                                    provider_setup_overlay.sync_from_config(&ctx.config);
                                    provider_setup_overlay.open();
                                }
                                // Restored v3.1.4: `Q` opens the decorative fx
                                // overlay (manual hotkey trigger). Gated so an
                                // overlay owning the keyboard receives Q as text.
                                KeyCode::Char('Q')
                                    if dashboard_fx_hotkey_allowed(k.code, &ctx.config, false) =>
                                {
                                    let term_size = terminal.size()?;
                                    fx_overlay.open(
                                        &ctx.config.fx,
                                        crate::app::fx_overlay::FxTrigger::Hotkey,
                                        now,
                                        term_size.width,
                                        term_size.height,
                                    );
                                }
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
                                    use crate::app::action_explainer::{
                                        PendingAction, build_copy_view,
                                    };
                                    let now = Instant::now();
                                    if dashboard.alert_state.selected().is_none() {
                                        continue;
                                    }
                                    let suggested =
                                        crate::app::clipboard_actions::selected_alert_suggested_command(
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
                                    // v1.38 Bug B fix: snapshot the (title, command) at
                                    // modal-open time so a polling reorder between modal
                                    // open and Enter cannot drift the clipboard payload.
                                    let snapshot = suggested.as_ref().map(|(title, cmd, _, _)| {
                                        PendingAction::CopyAlertCommand {
                                            command: cmd.clone(),
                                            alert_title: title.clone(),
                                        }
                                    });
                                    let should_open =
                                        crate::app::action_explainer::should_open_explainer(
                                            ctx.config.ux.confirm_actions,
                                            snapshot
                                                .as_ref()
                                                .is_some_and(|s| action_explainer.already_seen(s)),
                                            suggested.is_some(),
                                        );
                                    if should_open {
                                        if let (Some((title, cmd, sev, source)), Some(action)) =
                                            (suggested, snapshot)
                                        {
                                            let view =
                                                build_copy_view(&title, &cmd, Some(sev), source);
                                            action_explainer.open(action, view);
                                        }
                                    } else {
                                        let view = AlertCommandCopyView {
                                            alert_state: &dashboard.alert_state,
                                            notices: &dashboard.notices,
                                            reports: &dashboard.reports,
                                            fresh_alerts: &dashboard.fresh_alerts,
                                            alert_times: &dashboard.alert_times,
                                            hidden_until: &dashboard.alert_hide_deadlines,
                                            now,
                                        };
                                        let notice = copy_selected_alert_command_to_clipboard(view);
                                        dashboard.push_notice(notice, now);
                                    }
                                }
                                KeyCode::Char('p') | KeyCode::Char('d') => {
                                    use crate::app::action_explainer::{
                                        PendingAction, build_accept_view, build_reject_view,
                                    };

                                    let accepting = k.code == KeyCode::Char('p');
                                    let pane_idx = match dashboard.pane_state.selected() {
                                        Some(i) => i,
                                        None => continue,
                                    };
                                    let report = match dashboard.reports.get(pane_idx) {
                                        Some(r) => r,
                                        None => continue,
                                    };
                                    let proposal =
                                        crate::app::prompt_send_actions::first_prompt_send_proposal_full(
                                            report,
                                        );

                                    // v1.38 Bug B fix: snapshot identifying fields at
                                    // modal-open time. Confirm validates the proposal
                                    // still exists rather than re-querying by index,
                                    // so a polling reorder cannot drift the action.
                                    let action = proposal.as_ref().map(|(target, slash, pid)| {
                                        if accepting {
                                            PendingAction::AcceptPromptSend {
                                                target_pane_id: target.clone(),
                                                slash_command: slash.clone(),
                                                proposal_id: pid.clone(),
                                            }
                                        } else {
                                            PendingAction::RejectPromptSend {
                                                target_pane_id: target.clone(),
                                                slash_command: slash.clone(),
                                                proposal_id: pid.clone(),
                                            }
                                        }
                                    });

                                    let should_open =
                                        crate::app::action_explainer::should_open_explainer(
                                            ctx.config.ux.confirm_actions,
                                            action
                                                .as_ref()
                                                .is_some_and(|a| action_explainer.already_seen(a)),
                                            proposal.is_some(),
                                        );

                                    if should_open {
                                        if let (Some((_target, slash, _pid)), Some(action)) =
                                            (proposal, action)
                                        {
                                            let view = if accepting {
                                                build_accept_view(
                                                    report,
                                                    &slash,
                                                    ctx.config.actions.mode,
                                                    ctx.config.actions.allow_auto_prompt_send,
                                                )
                                            } else {
                                                build_reject_view(report, &slash)
                                            };
                                            action_explainer.open(action, view);
                                        }
                                    } else {
                                        let notice =
                                            crate::app::prompt_send_actions::handle_prompt_send_action(
                                            &ctx.source,
                                            &*ctx.sink,
                                            &dashboard.reports,
                                            Some(pane_idx),
                                            accepting,
                                            ctx.config.actions.mode,
                                            ctx.config.actions.allow_auto_prompt_send,
                                        );
                                        dashboard.push_notice(notice, Instant::now());
                                        // Restored v3.1.4: celebration confetti on a
                                        // direct (no-explainer) prompt-send accept.
                                        if accepting
                                            && ctx.config.fx.celebration_enabled
                                            && ctx.config.fx.enabled
                                        {
                                            let term_size = terminal.size()?;
                                            fx_overlay.open(
                                                &ctx.config.fx,
                                                crate::app::fx_overlay::FxTrigger::Celebration,
                                                Instant::now(),
                                                term_size.width,
                                                term_size.height,
                                            );
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        Event::Mouse(m) => {
                            let size = terminal.size()?;
                            let viewport = Rect::new(0, 0, size.width, size.height);
                            let now = Instant::now();
                            // Restored v3.1.4: any mouse event refreshes the
                            // screensaver idle clock; a click dismisses the fx
                            // overlay and is consumed.
                            last_user_activity = now;
                            if fx_overlay.is_open() && matches!(m.kind, MouseEventKind::Down(_)) {
                                fx_overlay.dismiss();
                                continue;
                            }
                            let overlay_mouse_owner = settings_overlay.is_open()
                                || provider_setup_overlay.is_open()
                                || action_explainer.is_open()
                                || git_modal.is_open()
                                || help_modal.is_open()
                                || target_picker.open;
                            if overlay_mouse_owner {
                                hover_help.clear_hover();
                            }

                            if settings_overlay.is_open() {
                                dashboard_split_dragging = false;
                                handle_settings_overlay_mouse(
                                    &mut settings_overlay,
                                    &mut ctx.config,
                                    viewport,
                                    m,
                                );
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

                            // v1.38 Bug A fix: while the Action Explainer modal is
                            // open, swallow all mouse events so they don't leak
                            // through to the dashboard (selection / divider drag).
                            // A left-click on the `[x]` rect closes the modal —
                            // matches the hint shown in the renderer footer.
                            if action_explainer.is_open() {
                                dashboard_split_dragging = false;
                                crate::app::action_explainer::handle_action_explainer_mouse(
                                    &mut action_explainer,
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

                            if matches!(
                                m.kind,
                                MouseEventKind::Moved
                                    | MouseEventKind::Down(_)
                                    | MouseEventKind::Drag(_)
                                    | MouseEventKind::ScrollUp
                                    | MouseEventKind::ScrollDown
                            ) {
                                if let Some(topic) = dashboard_forced_hover_topic(
                                    viewport,
                                    m.column,
                                    m.row,
                                    dashboard_split,
                                ) {
                                    hover_help.set_hover(topic, m.column, m.row, now);
                                } else if ctx.config.ux.hover_help {
                                    if let Some(topic) = dashboard_hover_topic(
                                        viewport,
                                        m.column,
                                        m.row,
                                        DashboardHoverView {
                                            split: dashboard_split,
                                            hover_help_trigger: ctx.config.ux.hover_help_trigger,
                                            alerts_focused: focus == FocusedPanel::Alerts,
                                            panes_focused: focus == FocusedPanel::Panes,
                                            alert_state: &dashboard.alert_state,
                                            pane_state: &dashboard.pane_state,
                                            notices: &dashboard.notices,
                                            reports: &dashboard.reports,
                                            fresh_alerts: &dashboard.fresh_alerts,
                                            alert_times: &dashboard.alert_times,
                                            hidden_until: &dashboard.alert_hide_deadlines,
                                            now,
                                            target_label: &target,
                                            audit_recent_severity: dashboard.audit_recent_severity,
                                        },
                                    ) {
                                        hover_help.set_hover(topic, m.column, m.row, now);
                                    } else {
                                        hover_help.clear_hover();
                                    }
                                } else {
                                    hover_help.clear_hover();
                                }
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
                                    audit_recent_severity: dashboard.audit_recent_severity,
                                },
                            );
                            match action {
                                DashboardMouseAction::OpenGitModal => {
                                    let panel = capture_repo_panel();
                                    git_modal.open(panel.title, panel.lines);
                                }
                                DashboardMouseAction::None => {}
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

/// v1.38 Phase D Task 20: when the Action Explainer modal is open and
/// the operator presses the same key that opened it (`p` for accept,
/// `d` for reject, `y` for copy), close the modal without executing —
/// the originating key acts as a toggle/cancel rather than re-opening
/// the same action.
/// v1.51.0: emit a single ASCII BEL (0x07) so terminals that honour the
/// audible-bell setting chirp once on the inactive→active IME edge.
/// Writes directly to `stdout` because ratatui's backend doesn't expose
/// a "raw byte" path; the BEL is non-printing and won't disturb the
/// rendered frame. Errors are intentionally swallowed — failing to ring
/// is never worth crashing the loop over.
fn ring_terminal_bell() {
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x07");
    let _ = out.flush();
}

/// Restored v3.1.4: gate for the `Q` decorative-fx hotkey. Fires only
/// when no overlay owns the keyboard and the operator has `[fx]`
/// hotkey + effects enabled, so an overlay in edit/input mode receives
/// `Q` as text instead of opening the overlay.
fn dashboard_fx_hotkey_allowed(
    code: KeyCode,
    config: &crate::app::config::QmonsterConfig,
    overlay_owns_keyboard: bool,
) -> bool {
    matches!(code, KeyCode::Char('Q'))
        && !overlay_owns_keyboard
        && config.fx.hotkey_enabled
        && config.fx.enabled
}

fn matches_originating_key(
    pending: Option<&crate::app::action_explainer::PendingAction>,
    c: char,
) -> bool {
    use crate::app::action_explainer::PendingAction;
    matches!(
        (pending, c),
        (Some(PendingAction::AcceptPromptSend { .. }), 'p')
            | (Some(PendingAction::RejectPromptSend { .. }), 'd')
            | (Some(PendingAction::CopyAlertCommand { .. }), 'y')
    )
}

/// v1.38 Bug B fix: at Enter time, build a `SystemNotice` from the
/// snapshotted `PendingAction` rather than re-querying the live
/// `pane_state` / `alert_state`. For accept/reject this validates the
/// proposal_id still matches a live `PromptSendProposed` effect on
/// the same pane and surfaces a "proposal vanished" notice if not.
/// For copy this writes the snapshotted command to the clipboard
/// directly so polling can't drift the payload.
fn confirm_pending_action<P: crate::tmux::polling::PaneSource>(
    action: &crate::app::action_explainer::PendingAction,
    source: &P,
    sink: &dyn crate::store::EventSink,
    reports: &[crate::app::event_loop::PaneReport],
    mode: crate::app::config::ActionsMode,
    allow_auto_prompt_send: bool,
) -> SystemNotice {
    use crate::app::action_explainer::{PendingAction, resolve_accept_target};
    match action {
        PendingAction::AcceptPromptSend {
            target_pane_id,
            slash_command,
            proposal_id,
        }
        | PendingAction::RejectPromptSend {
            target_pane_id,
            slash_command,
            proposal_id,
        } => {
            let accepting = matches!(action, PendingAction::AcceptPromptSend { .. });
            // v1.38 explainer fix: `resolve_accept_target` is consulted only
            // for vanished-proposal detection. Dispatch always uses the
            // snapshot fields directly via
            // `handle_prompt_send_action_for_proposal`, bypassing
            // `first_prompt_send_proposal`'s lex-first re-selection so a
            // newly polled lower-id proposal can't drift the executed
            // command away from the modal-shown one.
            match resolve_accept_target(action, reports) {
                Some(_idx) => {
                    crate::app::prompt_send_actions::handle_prompt_send_action_for_proposal(
                        source,
                        sink,
                        mode,
                        allow_auto_prompt_send,
                        target_pane_id,
                        slash_command,
                        proposal_id,
                        accepting,
                    )
                }
                None => SystemNotice {
                    title: "proposal vanished".into(),
                    body: format!(
                        "{target_pane_id} \u{2192} `{slash_command}` (proposal_id `{proposal_id}` no longer present \u{2014} another agent or a poll cycle dropped it)"
                    ),
                    severity: crate::domain::recommendation::Severity::Warning,
                    source_kind: crate::domain::origin::SourceKind::ProjectCanonical,
                },
            }
        }
        PendingAction::CopyAlertCommand { command, .. } => {
            match crate::app::clipboard_actions::copy_text_to_clipboard(command) {
                Ok(()) => SystemNotice {
                    title: "command copied".into(),
                    body: format!("`{command}`"),
                    severity: crate::domain::recommendation::Severity::Good,
                    source_kind: crate::domain::origin::SourceKind::ProjectCanonical,
                },
                Err(e) => SystemNotice {
                    title: "clipboard unavailable".into(),
                    body: format!("could not copy command: {e}"),
                    severity: crate::domain::recommendation::Severity::Warning,
                    source_kind: crate::domain::origin::SourceKind::ProjectCanonical,
                },
            }
        }
    }
}

#[cfg(test)]
mod fx_hotkey_tests {
    use super::dashboard_fx_hotkey_allowed;
    use crate::app::config::QmonsterConfig;
    use crossterm::event::KeyCode;

    #[test]
    fn fx_hotkey_gated_by_enabled_flags_and_overlay_focus() {
        let mut config = QmonsterConfig::defaults();
        config.fx.enabled = true;
        config.fx.hotkey_enabled = true;
        assert!(
            dashboard_fx_hotkey_allowed(KeyCode::Char('Q'), &config, false),
            "Q opens fx when enabled and no overlay owns the keyboard"
        );
        assert!(
            !dashboard_fx_hotkey_allowed(KeyCode::Char('Q'), &config, true),
            "an overlay owning the keyboard must receive Q as text, not open fx"
        );

        config.fx.hotkey_enabled = false;
        assert!(!dashboard_fx_hotkey_allowed(
            KeyCode::Char('Q'),
            &config,
            false
        ));

        config.fx.hotkey_enabled = true;
        config.fx.enabled = false;
        assert!(!dashboard_fx_hotkey_allowed(
            KeyCode::Char('Q'),
            &config,
            false
        ));

        config.fx.enabled = true;
        assert!(
            !dashboard_fx_hotkey_allowed(KeyCode::Char('x'), &config, false),
            "a non-Q key never opens fx"
        );
    }
}
