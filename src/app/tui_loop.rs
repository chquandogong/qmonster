use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::bootstrap::Context;
use crate::app::clipboard_actions::{
    AlertCommandCopyView, copy_selected_alert_command_to_clipboard_with_ledger,
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
use crate::app::pending_actions_overlay::{
    PendingActionsOutcome, accept_action_for, copy_action_for, handle_pending_actions_overlay_key,
    handle_pending_actions_overlay_mouse, reject_action_for,
};
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
    let mut metrics_overlay = crate::ui::metrics::MetricsOverlay::new();
    let mut anomaly_overlay = crate::ui::anomaly_overlay::AnomalyOverlay::new();
    let mut insights_overlay = crate::ui::insights::InsightsOverlay::new();
    let mut action_explainer = crate::app::action_explainer::ActionExplainModal::new();
    // v1.39 surface C / v1.40 redesign: Pending Actions overlay (a key).
    // Split list+live-explainer modal listing every pane with a pending
    // prompt-send proposal AND every alert with a suggested_command, with
    // severity color coding. Multi-select (Space / P / Y / A / c) +
    // p/d/y dispatch in-place; Enter is silently swallowed.
    let mut pending_actions = crate::ui::pending_actions::PendingActionsOverlay::new();
    let mut hover_help = HoverHelpState::new();
    // v1.53.0: decorative effects overlay (Q hotkey, p-accept celebration,
    // idle screensaver). Inert until Q / celebration / screensaver fires.
    let mut fx_overlay = crate::app::fx_overlay::FxOverlay::new();
    let mut last_user_activity = startup_now;

    // Phase F F-6 (v1.32.0): spawn `codex app-server` once at TUI
    // startup when the operator opted in via the [provider_setup]
    // section. Spawn failures surface as a SystemNotice but never
    // abort startup — the rest of Qmonster works fine without
    // app-server-derived rate limits.
    if ctx.config.provider_setup.codex_app_server && ctx.codex_app_server.is_none() {
        let client_version = env!("CARGO_PKG_VERSION");
        match crate::adapters::codex_app_server::CodexAppServer::spawn("qmonster", client_version) {
            Ok(server) => {
                ctx.codex_app_server = Some(server);
                dashboard.push_notice(
                    crate::app::system_notice::SystemNotice {
                        title: "Codex App Server started".into(),
                        body: "F-6 rate-limit polling active".into(),
                        severity: crate::domain::recommendation::Severity::Good,
                        source_kind: crate::domain::origin::SourceKind::ProjectCanonical,
                    },
                    Instant::now(),
                );
            }
            Err(e) => {
                dashboard.push_notice(
                    crate::app::system_notice::SystemNotice {
                        title: "Codex App Server failed to start".into(),
                        body: format!("rate limits unavailable — {e}"),
                        severity: crate::domain::recommendation::Severity::Warning,
                        source_kind: crate::domain::origin::SourceKind::ProjectCanonical,
                    },
                    Instant::now(),
                );
            }
        }
    }

    let mut last_alert_click: Option<AlertMouseClick> = None;
    let mut last_pane_idle_states: HashMap<String, Option<IdleCause>> = HashMap::new();
    let mut pane_state_flashes: HashMap<String, crate::ui::panels::PaneStateFlash> = HashMap::new();
    let mut runtime_refresh_offsets: HashMap<String, usize> = HashMap::new();
    // v1.38 polish: per-pane MEM tracker. Updated once per poll cycle
    // (not per render), so the metrics overlay's COST·MEM combined
    // row can drive ▲/▼/─ trend arrows from the actual delta between
    // successive observations rather than the placeholder dash.
    let mut mem_observations: HashMap<String, crate::ui::metrics::MemObservation> = HashMap::new();
    // v1.60.0: per-pane CTX/quota/cache pressure tracker. Updated
    // once per poll cycle, parallel to the MEM tracker, so the
    // metrics overlay's left-column bars can render ▲/▼/─ trend
    // arrows next to the percentage and operators can see at a
    // glance whether pressure is climbing.
    let mut pressure_observations: HashMap<String, crate::ui::metrics::PressureObservation> =
        HashMap::new();

    let result = {
        let mut run_loop = || -> anyhow::Result<()> {
            loop {
                let now = Instant::now();
                // v1.55.0: skip the tmux poll tick while the fx overlay
                // is active so the 50-200ms tmux capture / parse pass
                // doesn't visibly stutter the 60 FPS animation. The
                // tick resumes the moment the overlay closes; one
                // skipped tick at most == 2s of stale pane data, which
                // is acceptable for a deliberately decorative moment.
                if now.saturating_duration_since(last_poll) >= poll && !fx_overlay.is_open() {
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
                    // v1.38 polish: refresh the per-pane MEM tracker
                    // each poll so the metrics overlay's COST·MEM row
                    // can render real ▲/▼/─ trend arrows derived from
                    // last-poll deltas instead of the placeholder dash.
                    for r in &dashboard.reports {
                        crate::ui::metrics::update_mem_observation(
                            &mut mem_observations,
                            &r.pane_id,
                            r.signals.process_memory_mb.as_ref().map(|m| m.value),
                            r.signals.agent_memory_bytes.as_ref().map(|m| m.value),
                        );
                        // v1.60.0: refresh the parallel pressure
                        // tracker so the metrics overlay can render
                        // CTX/5H/7D/CACHE trend arrows derived from
                        // last-poll deltas.
                        let cache_ratio =
                            r.signals.cache_hit_ratio.as_ref().map(|m| m.value as f32);
                        crate::ui::metrics::update_pressure_observation(
                            &mut pressure_observations,
                            &r.pane_id,
                            r.signals.context_pressure.as_ref().map(|m| m.value),
                            r.signals.quota_5h_pressure.as_ref().map(|m| m.value),
                            r.signals.quota_weekly_pressure.as_ref().map(|m| m.value),
                            cache_ratio,
                        );
                    }
                }

                pane_state_flashes.retain(|_, flash| flash.is_active(now));
                dashboard.sync_alert_selection(now);
                let target = target_label(target_picker.selected_target.as_ref());
                // v1.39 surface C: pre-build the pending-actions items
                // each frame so the overlay (when open) and the future
                // `a`-key handler always see the same snapshot of
                // actionable items as render and so the operator's
                // selection cursor stays aligned with the rendered list.
                let pending_items = crate::ui::pending_actions::collect_pending_items(
                    &dashboard.reports,
                    &dashboard.notices,
                    &dashboard.fresh_alerts,
                    &dashboard.alert_times,
                    &dashboard.alert_hide_deadlines,
                    now,
                );
                // v1.40 Task 12 §5.10: auto-prune stale multi-select keys
                // every frame so a proposal accepted last tick cannot
                // linger in `multi_selected` and re-dispatch.
                pending_actions.prune_to(&pending_items);
                // v1.53.0: idle screensaver auto-trigger + per-frame
                // scene step. Pure helper so the gating conditions
                // stay unit-testable; the `step` call advances the
                // active effect's particles / banner / streams.
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
                let alert_filter_snapshot = dashboard.alert_filter().map(|s| s.to_string());
                let audit_recent_severity = ctx.insights_db_path.as_deref().and_then(|path| {
                    crate::store::recent_audit_max_severity(path, 15 * 60)
                        .ok()
                        .flatten()
                });
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
                            metrics_overlay: &metrics_overlay,
                            anomaly_overlay: &anomaly_overlay,
                            insights_overlay: &insights_overlay,
                            anomaly_events_ring: &ctx.anomaly_events_ring,
                            mem_observations: &mem_observations,
                            pressure_observations: &pressure_observations,
                            action_explainer: &action_explainer,
                            pending_actions: &pending_actions,
                            pending_items: &pending_items,
                            hover_help: &hover_help,
                            config: &ctx.config,
                            ime_active: ctx.ime_state.is_active(now),
                            fx_overlay: &fx_overlay,
                            fx_text: ctx.config.fx.text.as_str(),
                            alert_filter: alert_filter_snapshot.as_deref(),
                        },
                    );
                })?;

                while let Ok(outcome) = ctx.insights_load_rx.try_recv() {
                    let label = chrono::Local::now().format("%H:%M:%S").to_string();
                    insights_overlay.set_snapshot_for(outcome.request_id, outcome.result, label);
                }
                if insights_overlay.is_loading() {
                    insights_overlay.advance_spinner();
                }

                // v1.53.0: tighten poll cadence to ~30 FPS while the fx
                // overlay is active so banner / confetti / matrix animation
                // looks smooth. Closed → keep the existing 100ms cadence so
                // the quiet path stays cheap.
                let poll_ms = if fx_overlay.is_open() {
                    crate::app::fx_state::FX_FRAME_INTERVAL_MS
                } else {
                    100
                };
                if event::poll(Duration::from_millis(poll_ms))? {
                    match event::read()? {
                        Event::Key(k) if k.kind == KeyEventKind::Press => {
                            // v1.53.0: any keypress refreshes the screensaver
                            // idle clock so legitimate operator input keeps
                            // the saver suppressed.
                            last_user_activity = Instant::now();
                            // v1.53.0: any key dismisses the fx overlay and
                            // is consumed (not forwarded to per-overlay
                            // dispatch) so the operator regains control on
                            // a single press.
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

                            if metrics_overlay.is_open() {
                                crate::app::metrics_overlay::handle_metrics_overlay_key(
                                    &mut metrics_overlay,
                                    dashboard.reports.len(),
                                    k.code,
                                );
                                continue;
                            }

                            if anomaly_overlay.is_open() {
                                if matches!(k.code, KeyCode::Char('h')) {
                                    match anomaly_overlay.view() {
                                        crate::ui::anomaly_overlay::AnomalyOverlayView::Ring => {
                                            let events = ctx
                                                .anomaly_sink
                                                .as_ref()
                                                .map(|sink| sink.fetch_recent_anomaly_events(200))
                                                .unwrap_or_default();
                                            anomaly_overlay.toggle_view(events);
                                        }
                                        crate::ui::anomaly_overlay::AnomalyOverlayView::History => {
                                            anomaly_overlay.toggle_view(Vec::new());
                                        }
                                    }
                                    continue;
                                }
                                let _ = crate::app::anomaly_overlay::handle_anomaly_overlay_key(
                                    &mut anomaly_overlay,
                                    ctx.anomaly_events_ring.len(),
                                    k.code,
                                );
                                continue;
                            }

                            if insights_overlay.is_open() {
                                let action =
                                    crate::app::insights_overlay::handle_insights_overlay_key(
                                        &mut insights_overlay,
                                        k.code,
                                    );
                                if action
                                    == crate::app::insights_overlay::InsightsOverlayAction::Refresh
                                {
                                    refresh_insights_overlay(&mut insights_overlay, ctx);
                                    // v1.60.0: confirm the refresh kick to the
                                    // operator. The async load takes a
                                    // beat to land — without this nudge a
                                    // stale chip + an `r` press feels
                                    // ignored.
                                    dashboard.push_notice(
                                        SystemNotice {
                                            title: "insights refresh requested".into(),
                                            body: "aggregating fresh token insights …".into(),
                                            severity: crate::domain::recommendation::Severity::Good,
                                            source_kind:
                                                crate::domain::origin::SourceKind::ProjectCanonical,
                                        },
                                        Instant::now(),
                                    );
                                }
                                continue;
                            }

                            if pending_actions.is_open() {
                                // Re-collect items at key-handle time so
                                // a poll between draw and key delivery
                                // can't drift the index.
                                let now = Instant::now();
                                let items = crate::ui::pending_actions::collect_pending_items(
                                    &dashboard.reports,
                                    &dashboard.notices,
                                    &dashboard.fresh_alerts,
                                    &dashboard.alert_times,
                                    &dashboard.alert_hide_deadlines,
                                    now,
                                );
                                pending_actions.prune_to(&items);
                                // v1.40 post-release fix: thread viewport
                                // through the key handler so the `,`/`.`
                                // arms can compute the current effective
                                // list width (needed to step in the right
                                // direction from the auto-formula baseline).
                                let size = terminal.size()?;
                                let viewport = Rect::new(0, 0, size.width, size.height);
                                let outcome = handle_pending_actions_overlay_key(
                                    &mut pending_actions,
                                    &items,
                                    viewport,
                                    k.code,
                                );
                                match outcome {
                                    PendingActionsOutcome::None | PendingActionsOutcome::Closed => {
                                    }
                                    PendingActionsOutcome::AcceptItems(idxs) => {
                                        dispatch_bulk_accept(
                                            &idxs,
                                            &items,
                                            &mut pending_actions,
                                            &mut dashboard,
                                            &ctx.source,
                                            &*ctx.sink,
                                            ctx.recommendation_lifecycle_sink.as_ref(),
                                            ctx.config.actions.mode,
                                            ctx.config.actions.allow_auto_prompt_send,
                                            Instant::now(),
                                        );
                                    }
                                    PendingActionsOutcome::ClearItems(idxs) => {
                                        let hidden_before = dashboard.alert_hide_deadlines.clone();
                                        dispatch_bulk_clear(
                                            &idxs,
                                            &items,
                                            &mut pending_actions,
                                            &mut dashboard,
                                            &ctx.source,
                                            &*ctx.sink,
                                            ctx.recommendation_lifecycle_sink.as_ref(),
                                            ctx.config.actions.mode,
                                            ctx.config.actions.allow_auto_prompt_send,
                                            Instant::now(),
                                        );
                                        let hidden_keys = newly_hidden_alert_keys(
                                            &hidden_before,
                                            &dashboard.alert_hide_deadlines,
                                            Instant::now(),
                                        );
                                        crate::app::insights_lifecycle::record_hidden_alert_outcomes(
                                            ctx.recommendation_lifecycle_sink.as_ref(),
                                            &dashboard.reports,
                                            hidden_keys,
                                        );
                                    }
                                    PendingActionsOutcome::CopyItem(idx) => {
                                        dispatch_bulk_copy(
                                            idx,
                                            &items,
                                            &mut pending_actions,
                                            &mut dashboard,
                                            &ctx.source,
                                            &*ctx.sink,
                                            ctx.recommendation_lifecycle_sink.as_ref(),
                                            ctx.config.actions.mode,
                                            ctx.config.actions.allow_auto_prompt_send,
                                            Instant::now(),
                                        );
                                    }
                                }
                                continue;
                            }

                            if action_explainer.is_open() {
                                let now = Instant::now();
                                match k.code {
                                    KeyCode::Enter => {
                                        if let Some(action) = action_explainer.pending().cloned() {
                                            action_explainer.mark_seen(&action);
                                            // v1.53.0: capture accept-edge before
                                            // confirm_pending_action so celebration
                                            // fires only on AcceptPromptSend.
                                            let was_accept = matches!(
                                                action,
                                                crate::app::action_explainer::PendingAction::AcceptPromptSend { .. }
                                            );
                                            let notice = confirm_pending_action(
                                                &action,
                                                &ctx.source,
                                                &*ctx.sink,
                                                ctx.recommendation_lifecycle_sink.as_ref(),
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

                            let hidden_before =
                                if matches!(k.code, KeyCode::Enter | KeyCode::Char(' '))
                                    && focus == FocusedPanel::Alerts
                                {
                                    Some(dashboard.alert_hide_deadlines.clone())
                                } else {
                                    None
                                };
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
                                if let Some(hidden_before) = hidden_before {
                                    let hidden_keys = newly_hidden_alert_keys(
                                        &hidden_before,
                                        &dashboard.alert_hide_deadlines,
                                        now,
                                    );
                                    crate::app::insights_lifecycle::record_hidden_alert_outcomes(
                                        ctx.recommendation_lifecycle_sink.as_ref(),
                                        &dashboard.reports,
                                        hidden_keys,
                                    );
                                }
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
                                KeyCode::Char('K') => {
                                    let size = terminal.size()?;
                                    let viewport = Rect::new(0, 0, size.width, size.height);
                                    let rects = crate::ui::dashboard::dashboard_rects(
                                        viewport,
                                        dashboard_split,
                                    );
                                    let badge =
                                        crate::ui::dashboard::footer_keys_badge_rect(rects.footer);
                                    hover_help.set_hover(
                                        crate::ui::help_glossary::HelpTopic::DashboardFooter,
                                        badge.x,
                                        badge.y,
                                        now,
                                    );
                                }
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
                                KeyCode::Char('P') => {
                                    provider_setup_overlay.sync_from_config(&ctx.config);
                                    provider_setup_overlay.open();
                                }
                                KeyCode::Char('m') => {
                                    if metrics_overlay.is_open() {
                                        metrics_overlay.close();
                                    } else {
                                        metrics_overlay.open();
                                    }
                                }
                                KeyCode::Char('n') => {
                                    if anomaly_overlay.is_open() {
                                        anomaly_overlay.close();
                                    } else {
                                        anomaly_overlay.open();
                                    }
                                }
                                KeyCode::Char('i') => {
                                    insights_overlay.open();
                                    refresh_insights_overlay(&mut insights_overlay, ctx);
                                }
                                KeyCode::Char('a') => {
                                    // v1.39 surface C: open the Pending
                                    // Actions overlay. Toggle on `a`
                                    // again so the operator can dismiss
                                    // without reaching for Esc.
                                    //
                                    // v1.41 P1: on the FIRST open per
                                    // session, also push a SystemNotice
                                    // warning about the confirm_actions
                                    // bypass (UI_MANUAL §8.7). Fires
                                    // once per Qmonster process.
                                    if pending_actions.is_open() {
                                        pending_actions.close();
                                    } else {
                                        if !pending_actions.seen_first_open() {
                                            dashboard.push_notice(
                                                crate::app::system_notice::SystemNotice {
                                                    title: "a overlay: confirm_actions bypass"
                                                        .into(),
                                                    body: "p/d/y inside the Pending Actions overlay dispatch immediately, ignoring `[ux] confirm_actions`. The right-pane live explainer is the confirmation. (UI_MANUAL §8.7 — fired once per session.)".into(),
                                                    severity:
                                                        crate::domain::recommendation::Severity::Concern,
                                                    source_kind:
                                                        crate::domain::origin::SourceKind::ProjectCanonical,
                                                },
                                                Instant::now(),
                                            );
                                            pending_actions.mark_first_open_seen();
                                        }
                                        pending_actions.open();
                                    }
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
                                    if notice.title == "snapshot saved" {
                                        crate::app::insights_lifecycle::record_operator_snapshot_outcomes(
                                            ctx.recommendation_lifecycle_sink.as_ref(),
                                            &dashboard.reports,
                                            format!("operator snapshot {}", notice.body),
                                        );
                                    }
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
                                        // v2.2.0 (P1-3): route through the
                                        // ledger-aware helper so the operator's
                                        // `y` copy lands as a
                                        // `RecommendationOutcome::Copied` row
                                        // when the alert is recommendation-class.
                                        let now_unix_ms = crate::app::event_loop::current_unix_ms();
                                        let notice =
                                            copy_selected_alert_command_to_clipboard_with_ledger(
                                                view,
                                                ctx.recommendation_lifecycle_sink.as_ref(),
                                                now_unix_ms,
                                            );
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
                                            crate::app::prompt_send_actions::handle_prompt_send_action_with_lifecycle(
                                            &ctx.source,
                                            &*ctx.sink,
                                            ctx.recommendation_lifecycle_sink.as_ref(),
                                            &dashboard.reports,
                                            Some(pane_idx),
                                            accepting,
                                            ctx.config.actions.mode,
                                            ctx.config.actions.allow_auto_prompt_send,
                                        );
                                        dashboard.push_notice(notice, Instant::now());
                                        // v1.53.0: celebration confetti on
                                        // a successful pending-action accept.
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
                            // v1.53.0: refresh idle clock on any mouse
                            // event; dismiss fx overlay on a click.
                            last_user_activity = now;
                            if fx_overlay.is_open() && matches!(m.kind, MouseEventKind::Down(_)) {
                                fx_overlay.dismiss();
                                continue;
                            }

                            let overlay_mouse_owner = settings_overlay.is_open()
                                || provider_setup_overlay.is_open()
                                || metrics_overlay.is_open()
                                || anomaly_overlay.is_open()
                                || insights_overlay.is_open()
                                || pending_actions.is_open()
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

                            if metrics_overlay.is_open() {
                                dashboard_split_dragging = false;
                                crate::app::metrics_overlay::handle_metrics_overlay_mouse(
                                    &mut metrics_overlay,
                                    viewport,
                                    dashboard.reports.len(),
                                    m,
                                );
                                continue;
                            }

                            if anomaly_overlay.is_open() {
                                dashboard_split_dragging = false;
                                crate::app::anomaly_overlay::handle_anomaly_overlay_mouse(
                                    &mut anomaly_overlay,
                                    viewport,
                                    ctx.anomaly_events_ring.len(),
                                    m,
                                );
                                continue;
                            }

                            if insights_overlay.is_open() {
                                dashboard_split_dragging = false;
                                crate::app::insights_overlay::handle_insights_overlay_mouse(
                                    &mut insights_overlay,
                                    viewport,
                                    m,
                                );
                                continue;
                            }

                            if pending_actions.is_open() {
                                dashboard_split_dragging = false;
                                let outcome = handle_pending_actions_overlay_mouse(
                                    &mut pending_actions,
                                    viewport,
                                    &pending_items,
                                    m,
                                );
                                match outcome {
                                    PendingActionsOutcome::None | PendingActionsOutcome::Closed => {
                                    }
                                    PendingActionsOutcome::AcceptItems(_)
                                    | PendingActionsOutcome::ClearItems(_)
                                    | PendingActionsOutcome::CopyItem(_) => {
                                        // Mouse never produces dispatch outcomes today; future-proof
                                        // by debug-asserting if the contract drifts.
                                        debug_assert!(
                                            false,
                                            "mouse outcomes should not include dispatch variants"
                                        );
                                    }
                                }
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
                                            alert_state: &dashboard.alert_state,
                                            pane_state: &dashboard.pane_state,
                                            notices: &dashboard.notices,
                                            reports: &dashboard.reports,
                                            fresh_alerts: &dashboard.fresh_alerts,
                                            alert_times: &dashboard.alert_times,
                                            hidden_until: &dashboard.alert_hide_deadlines,
                                            now,
                                            target_label: &target,
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

                            let hidden_before = dashboard.alert_hide_deadlines.clone();
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
                            let hidden_keys = newly_hidden_alert_keys(
                                &hidden_before,
                                &dashboard.alert_hide_deadlines,
                                now,
                            );
                            crate::app::insights_lifecycle::record_hidden_alert_outcomes(
                                ctx.recommendation_lifecycle_sink.as_ref(),
                                &dashboard.reports,
                                hidden_keys,
                            );
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

fn newly_hidden_alert_keys(
    before: &HashMap<String, Instant>,
    after: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<String> {
    after
        .iter()
        .filter(|(key, deadline)| {
            **deadline > now
                && before
                    .get(*key)
                    .is_none_or(|old_deadline| *old_deadline <= now)
        })
        .map(|(key, _)| key.clone())
        .collect()
}

fn refresh_insights_overlay<P, N>(
    overlay: &mut crate::ui::insights::InsightsOverlay,
    ctx: &mut Context<P, N>,
) where
    P: PaneSource,
    N: NotifyBackend,
{
    let Some(path) = ctx.insights_db_path.clone() else {
        overlay.set_error("insights database path is unavailable in this runtime");
        return;
    };
    let now_ms = crate::app::event_loop::current_unix_ms();
    let window_ms = i64::try_from(u128::from(ctx.config.insights.default_window_secs) * 1000)
        .unwrap_or(i64::MAX);
    let window = crate::store::InsightsWindow {
        since_ms: now_ms.saturating_sub(window_ms),
        until_ms: now_ms,
    };
    ctx.next_insights_request_id = ctx.next_insights_request_id.wrapping_add(1);
    let request_id = ctx.next_insights_request_id;
    overlay.mark_loading(request_id);
    crate::app::insights_load::spawn_insights_load(
        path,
        window,
        ctx.config.insights.ignored_ttl_secs,
        ctx.insights_load_tx.clone(),
        request_id,
    );
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
    lifecycle_sink: Option<&crate::store::SqliteRecommendationLifecycleSink>,
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
                    crate::app::prompt_send_actions::handle_prompt_send_action_for_proposal_with_lifecycle(
                        source,
                        sink,
                        lifecycle_sink,
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

/// v1.40 surface C: bulk accept dispatcher for the Pending Actions
/// overlay's `p` path. Routes every selected proposal through the same
/// `confirm_pending_action` hardening as the dashboard direct keys,
/// then drops only the dispatched keys from `multi_selected` (per spec
/// §5.10 — surviving keys stay selected so the operator can retry next
/// tick).
///
/// v1.40.1 review fix (Critical 2): notices are pushed via
/// `dashboard.push_notice(notice, now)` so they land at the top of the
/// queue with `fresh_alerts` / `alert_times` updated and the alert
/// selection resynced — same hardening the single-item Action
/// Explainer path uses. Previously `notices.push(notice)` appended at
/// the bottom and skipped the resync, so bulk notices never got "NEW"
/// badges and the list selection could drift.
#[allow(clippy::too_many_arguments)]
fn dispatch_bulk_accept<P: crate::tmux::polling::PaneSource>(
    idxs: &[usize],
    items: &[crate::ui::pending_actions::PendingItem],
    overlay: &mut crate::ui::pending_actions::PendingActionsOverlay,
    dashboard: &mut crate::app::dashboard_runtime::DashboardRuntimeState,
    source: &P,
    sink: &dyn crate::store::EventSink,
    lifecycle_sink: Option<&crate::store::SqliteRecommendationLifecycleSink>,
    mode: crate::app::config::ActionsMode,
    allow_auto_prompt_send: bool,
    now: std::time::Instant,
) {
    use crate::ui::pending_actions::pending_item_key;

    let mut dispatched_keys: Vec<String> = Vec::new();
    for &idx in idxs {
        let Some(item) = items.get(idx) else { continue };
        let Some(action) = accept_action_for(item) else {
            continue;
        };
        let notice = confirm_pending_action(
            &action,
            source,
            sink,
            lifecycle_sink,
            &dashboard.reports,
            mode,
            allow_auto_prompt_send,
        );
        dashboard.push_notice(notice, now);
        dispatched_keys.push(pending_item_key(item));
    }
    overlay.retain_multi(|k| !dispatched_keys.iter().any(|dk| dk == k));
}

/// v1.40 surface C: bulk clear dispatcher for the `d` path. Proposal
/// items take the reject path (same hardening as direct `d`); copy
/// items take the alert-hide path so a `d`-on-alert dismisses the
/// alert via `alert_hide_deadlines` rather than rejecting a proposal.
/// Only dispatched keys are dropped from `multi_selected`.
///
/// v1.40.1 review fix (Critical 1): `alert_key_at_index` resolves
/// against `visible_alert_keys`, which is computed from the live
/// `alert_hide_deadlines` map. Hiding one alert in iteration N would
/// shift the indices for iteration N+1, so bulk-clearing two or more
/// alerts hid only the first plus a wrong subsequent one. Fix:
/// resolve all alert keys upfront against the original snapshot in
/// Phase 1, then apply per-item dispatch (rejects + hides) in
/// subsequent phases. This also addresses Important 3 — alerts that
/// vanished between selection and dispatch no longer claim a
/// `dispatched_keys` slot, so they stay in the multi-select set and
/// the operator can retry next tick.
#[allow(clippy::too_many_arguments)]
fn dispatch_bulk_clear<P: crate::tmux::polling::PaneSource>(
    idxs: &[usize],
    items: &[crate::ui::pending_actions::PendingItem],
    overlay: &mut crate::ui::pending_actions::PendingActionsOverlay,
    dashboard: &mut crate::app::dashboard_runtime::DashboardRuntimeState,
    source: &P,
    sink: &dyn crate::store::EventSink,
    lifecycle_sink: Option<&crate::store::SqliteRecommendationLifecycleSink>,
    mode: crate::app::config::ActionsMode,
    allow_auto_prompt_send: bool,
    now: std::time::Instant,
) {
    use crate::ui::pending_actions::{PendingItem, pending_item_key};

    // Phase 1: resolve all dispatch targets up front against the
    // original snapshot. We MUST do this before any mutation because
    // `alert_key_at_index` resolves against `visible_alert_keys`,
    // which is computed from the live `alert_hide_deadlines` map.
    // Hiding one alert in iteration N would shift the index for
    // iteration N+1.
    struct PreparedReject {
        item_key: String,
        action: crate::app::action_explainer::PendingAction,
    }
    struct PreparedHide {
        item_key: String,
        alert_key: String,
    }

    let mut reject_targets: Vec<PreparedReject> = Vec::new();
    let mut hide_targets: Vec<PreparedHide> = Vec::new();
    for &idx in idxs {
        let Some(item) = items.get(idx) else { continue };
        match item {
            PendingItem::Proposal { .. } => {
                if let Some(action) = reject_action_for(item) {
                    reject_targets.push(PreparedReject {
                        item_key: pending_item_key(item),
                        action,
                    });
                }
            }
            PendingItem::Copy { alert_idx, .. } => {
                if let Some(alert_key) = crate::app::dashboard_state::alert_key_at_index(
                    &dashboard.notices,
                    &dashboard.reports,
                    &dashboard.alert_hide_deadlines,
                    now,
                    *alert_idx,
                ) {
                    hide_targets.push(PreparedHide {
                        item_key: pending_item_key(item),
                        alert_key,
                    });
                }
                // else: alert vanished between selection and dispatch
                // (review Important 3 — do NOT add to dispatched_keys
                // so the entry stays in multi-select for retry).
            }
        }
    }

    let mut dispatched_keys: Vec<String> = Vec::new();

    // Phase 2: apply rejects (per-item, may emit "vanished" notices
    // via confirm_pending_action). Each push_notice resyncs the
    // dashboard so subsequent calls observe the new state.
    for prep in reject_targets {
        let notice = confirm_pending_action(
            &prep.action,
            source,
            sink,
            lifecycle_sink,
            &dashboard.reports,
            mode,
            allow_auto_prompt_send,
        );
        dashboard.push_notice(notice, now);
        dispatched_keys.push(prep.item_key);
    }

    // Phase 3: apply alert hides (deadlines map only — no notice).
    // The keys were captured against the pre-dispatch snapshot, so
    // they are stable regardless of any resyncs done in Phase 2.
    let any_hides = !hide_targets.is_empty();
    for prep in hide_targets {
        dashboard.alert_hide_deadlines.insert(
            prep.alert_key,
            now + crate::ui::alerts::ALERT_AUTO_HIDE_DELAY,
        );
        dispatched_keys.push(prep.item_key);
    }

    // Phase 4: if we mutated alert_hide_deadlines without going
    // through push_notice, run a final resync so fresh_alerts /
    // alert_times / alert_state pick up the hidden entries.
    if any_hides {
        dashboard.resync(now);
    }

    overlay.retain_multi(|k| !dispatched_keys.iter().any(|dk| dk == k));
}

/// v1.40 surface C: bulk copy dispatcher for the `y` path. Always
/// targets a single alert (the first selected `Copy` item, per
/// `dispatch_copy` in `pending_actions_overlay`). Routes through
/// `confirm_pending_action` so the clipboard write reuses the
/// snapshotted command (no drift).
///
/// v1.40.1 review fix (Critical 2): notice routed through
/// `dashboard.push_notice(notice, now)` for ordering / NEW badge
/// parity with the dashboard direct path.
#[allow(clippy::too_many_arguments)]
fn dispatch_bulk_copy<P: crate::tmux::polling::PaneSource>(
    idx: usize,
    items: &[crate::ui::pending_actions::PendingItem],
    overlay: &mut crate::ui::pending_actions::PendingActionsOverlay,
    dashboard: &mut crate::app::dashboard_runtime::DashboardRuntimeState,
    source: &P,
    sink: &dyn crate::store::EventSink,
    lifecycle_sink: Option<&crate::store::SqliteRecommendationLifecycleSink>,
    mode: crate::app::config::ActionsMode,
    allow_auto_prompt_send: bool,
    now: std::time::Instant,
) {
    use crate::ui::pending_actions::pending_item_key;

    let Some(item) = items.get(idx) else { return };
    let Some(action) = copy_action_for(item) else {
        return;
    };
    let notice = confirm_pending_action(
        &action,
        source,
        sink,
        lifecycle_sink,
        &dashboard.reports,
        mode,
        allow_auto_prompt_send,
    );
    dashboard.push_notice(notice, now);
    let key = pending_item_key(item);
    overlay.retain_multi(|k| k != &key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::dashboard_runtime::DashboardRuntimeState;
    use crate::app::system_notice::SystemNotice;
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;
    use crate::store::NoopSink;
    use crate::tmux::polling::{PaneSource, PollingError};
    use crate::tmux::types::{RawPaneSnapshot, WindowTarget};
    use crate::ui::pending_actions::{PendingActionsOverlay, PendingItem};
    use crossterm::event::KeyCode;
    use std::time::Instant;

    /// Minimal `PaneSource` that returns empty data for every query —
    /// enough to satisfy the type bound in `dispatch_bulk_clear`. The
    /// alert-hide path under test does NOT invoke any send_keys /
    /// list_panes calls, so this never fires in practice.
    struct StubSource;

    impl PaneSource for StubSource {
        fn list_panes(
            &self,
            _target: Option<&WindowTarget>,
        ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
            Ok(vec![])
        }

        fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
            Ok(None)
        }

        fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
            Ok(vec![])
        }

        fn capture_tail(&self, _pane_id: &str, _lines: usize) -> Result<String, PollingError> {
            Ok(String::new())
        }

        fn send_keys(&self, _pane_id: &str, _text: &str) -> Result<(), PollingError> {
            Ok(())
        }
    }

    fn alertable_notice(title: &str, body: &str) -> SystemNotice {
        SystemNotice {
            title: title.into(),
            body: body.into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
        }
    }

    #[test]
    fn fx_hotkey_is_disabled_while_an_overlay_owns_keyboard() {
        let mut config = crate::app::config::QmonsterConfig::defaults();
        config.fx.enabled = true;
        config.fx.hotkey_enabled = true;

        assert!(
            dashboard_fx_hotkey_allowed(KeyCode::Char('Q'), &config, false),
            "dashboard Q should open fx when no overlay owns input"
        );
        assert!(
            !dashboard_fx_hotkey_allowed(KeyCode::Char('Q'), &config, true),
            "overlay edit/input modes must receive Q as text instead of opening fx"
        );
    }

    /// v1.40.1 review fix Critical 1 regression: bulk-clearing two
    /// alerts must hide BOTH alert keys. Before the fix,
    /// `alert_key_at_index` resolved against the live
    /// `alert_hide_deadlines` map and the second iteration's index
    /// shifted after the first hide — so the second hide either
    /// targeted the wrong alert or missed entirely. The fix snapshots
    /// the keys upfront against the original state and applies the
    /// hides afterward.
    #[test]
    fn dispatch_bulk_clear_two_alerts_hides_both() {
        let now = Instant::now();
        // Build a dashboard with 2 system notices. Both end up as
        // alerts in `visible_alert_keys`.
        let mut dashboard = DashboardRuntimeState::new(
            vec![
                alertable_notice("ctx-a", "/clear"),
                alertable_notice("ctx-b", "/compact"),
            ],
            now,
        );

        // Sanity check: both alerts visible at indexes 0 and 1.
        let visible = crate::ui::alerts::visible_alert_keys(
            &dashboard.notices,
            &dashboard.reports,
            &dashboard.alert_hide_deadlines,
            now,
        );
        assert_eq!(visible.len(), 2, "test setup must produce 2 visible alerts",);
        let key_at_0 = visible[0].clone();
        let key_at_1 = visible[1].clone();
        assert_ne!(key_at_0, key_at_1);

        // Build PendingItem::Copy entries that point at indexes 0
        // and 1 of the live alert list.
        let items = vec![
            PendingItem::Copy {
                alert_idx: 0,
                command: "/clear".into(),
                alert_title: "ctx-a".into(),
                severity: Severity::Warning,
                source: SourceKind::Estimated,
                pane_idx: None,
            },
            PendingItem::Copy {
                alert_idx: 1,
                command: "/compact".into(),
                alert_title: "ctx-b".into(),
                severity: Severity::Warning,
                source: SourceKind::Estimated,
                pane_idx: None,
            },
        ];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.toggle_group_all(&items);
        assert_eq!(overlay.multi_len(), 2);

        let source = StubSource;
        let sink = NoopSink;

        dispatch_bulk_clear(
            &[0, 1],
            &items,
            &mut overlay,
            &mut dashboard,
            &source,
            &sink,
            None,
            crate::app::config::ActionsMode::ObserveOnly,
            true,
            now,
        );

        // Both alert keys must be present in alert_hide_deadlines —
        // catching the index-shift bug where only one (or one + a
        // wrong key) ended up hidden.
        assert_eq!(
            dashboard.alert_hide_deadlines.len(),
            2,
            "both alert keys must be hidden (catches index-shift bug)",
        );
        assert!(
            dashboard.alert_hide_deadlines.contains_key(&key_at_0),
            "first alert key (index 0 in original snapshot) must be hidden",
        );
        assert!(
            dashboard.alert_hide_deadlines.contains_key(&key_at_1),
            "second alert key (index 1 in original snapshot) must be hidden",
        );
        // All dispatched keys removed from multi-select.
        assert_eq!(overlay.multi_len(), 0);
    }

    /// v1.40.1 review fix Important 3: an alert that vanishes
    /// between selection and dispatch must NOT claim a
    /// `dispatched_keys` slot. The pending-item key stays in the
    /// multi-select set so the operator can retry next tick once
    /// the alert reappears.
    #[test]
    fn dispatch_bulk_clear_skips_vanished_alert_index() {
        let now = Instant::now();
        // Dashboard has 0 notices — every alert_idx resolves to None.
        let mut dashboard = DashboardRuntimeState::new(Vec::new(), now);

        let item = PendingItem::Copy {
            alert_idx: 7, // out of range vs the empty alert list
            command: "/clear".into(),
            alert_title: "ctx-vanished".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        let items = vec![item.clone()];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.toggle_multi(&item);
        assert_eq!(overlay.multi_len(), 1);

        dispatch_bulk_clear(
            &[0],
            &items,
            &mut overlay,
            &mut dashboard,
            &StubSource,
            &NoopSink,
            None,
            crate::app::config::ActionsMode::ObserveOnly,
            true,
            now,
        );

        assert!(
            dashboard.alert_hide_deadlines.is_empty(),
            "vanished alert must NOT be added to alert_hide_deadlines",
        );
        assert_eq!(
            overlay.multi_len(),
            1,
            "vanished alert key must stay in multi-select for retry",
        );
    }
}
