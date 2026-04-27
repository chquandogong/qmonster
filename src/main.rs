use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::Rect;

use qmonster::app::bootstrap::Context;
use qmonster::app::clipboard_actions::{
    AlertCommandCopyView, copy_selected_alert_command_to_clipboard,
};
use qmonster::app::dashboard_render::{DashboardFrameView, render_dashboard_frame};
use qmonster::app::dashboard_runtime::DashboardRuntimeState;
use qmonster::app::dashboard_state::{
    AlertMouseClick, DashboardMouseAction, DashboardMouseView, DashboardSelectionKeyView,
    handle_dashboard_mouse, handle_dashboard_selection_key,
};
use qmonster::app::event_loop::run_once;
use qmonster::app::git_info::capture_repo_panel;
use qmonster::app::keymap::{FocusedPanel, toggle_focus};
use qmonster::app::modal_state::{
    ScrollModalState, handle_scroll_modal_key, handle_scroll_modal_mouse,
};
use qmonster::app::once_report::print_once_reports;
use qmonster::app::operator_actions::{version_refresh_notices, write_operator_snapshot};
use qmonster::app::polling_tick::{PollTickState, handle_poll_tick};
use qmonster::app::prompt_send_actions::handle_prompt_send_action;
use qmonster::app::runtime_refresh::handle_runtime_refresh_action;
use qmonster::app::settings_overlay::{handle_settings_overlay_key, handle_settings_overlay_mouse};
use qmonster::app::startup::{StartupOptions, build_startup_runtime};
use qmonster::app::system_notice::SystemNotice;
use qmonster::app::target_picker::{
    TargetChoice, TargetPickerAction, TargetPickerController, TargetPickerStage,
    handle_target_picker_key, handle_target_picker_mouse, initial_target, open_target_picker,
    target_label, target_switched_notice,
};
use qmonster::app::terminal_session::{enter_terminal_session, leave_terminal_session};
use qmonster::app::version_drift::{VersionSnapshot, capture_versions};
use qmonster::domain::signal::IdleCause;
use qmonster::store::SnapshotWriter;
use qmonster::tmux::polling::PaneSource;
use qmonster::ui::dashboard::{
    DashboardSplit, close_button_rect, git_modal_rects, help_modal_rects,
};

#[derive(Debug, Parser)]
#[command(name = "qmonster", about = "Observe-first TUI for multi-CLI tmux work")]
struct Cli {
    /// Path to a TOML config file.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Safer-only config overrides as key=value (e.g. `actions.mode=observe_only`).
    #[arg(long, value_name = "KEY=VALUE")]
    set: Vec<String>,

    /// Override the storage root (defaults to ~/.qmonster/ or $QMONSTER_ROOT).
    #[arg(long, value_name = "PATH")]
    root: Option<PathBuf>,

    /// Run one iteration and exit (for smoke tests and scripted checks).
    #[arg(long)]
    once: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let env_root = std::env::var("QMONSTER_ROOT").ok();
    let runtime = build_startup_runtime(StartupOptions {
        config_path: cli.config.as_deref(),
        root: cli.root.as_deref(),
        set: &cli.set,
        env_root: env_root.as_deref(),
    })?;
    let qmonster::app::startup::StartupRuntime {
        mut ctx,
        paths,
        root_source,
        versions,
        startup_notices,
        snapshot_writer,
    } = runtime;

    if cli.once {
        println!(
            "qmonster paths: {} (source: {:?})",
            paths.root().display(),
            root_source
        );
        println!("qmonster versions captured:");
        for (k, v) in &versions.tools {
            println!("  {k}: {v}");
        }
        if !startup_notices.is_empty() {
            println!();
            println!("startup notices:");
            for n in &startup_notices {
                println!("  [{}] {}", n.severity.letter(), n.body);
            }
        }
        println!();
        let reports = run_once(&mut ctx, Instant::now())?;
        print_once_reports(&reports, &ctx.config);
        return Ok(());
    }

    run_tui(&mut ctx, versions, snapshot_writer, startup_notices)
}

// Phase 5 P5-3 second gate types (`PromptSendGate` + `check_send_gate`)
// were moved to `qmonster::policy::gates` in v1.10.1 remediation
// (Gemini v1.10.0 finding #1 closed). The TUI keystroke handler below
// imports them via the `use` statement at the top of this file.

fn run_tui<P, N>(
    ctx: &mut Context<P, N>,
    mut versions: VersionSnapshot,
    snapshot_writer: SnapshotWriter,
    startup_notices: Vec<SystemNotice>,
) -> anyhow::Result<()>
where
    P: PaneSource,
    N: qmonster::notify::desktop::NotifyBackend,
{
    let mut terminal = enter_terminal_session()?;

    let poll = ctx.config.tmux.poll_interval();
    let startup_now = Instant::now();
    let mut dashboard = DashboardRuntimeState::new(startup_notices, startup_now);
    let mut last_poll = startup_now - poll;
    let mut last_poll_error: Option<String> = None;
    let mut selected_target = initial_target(&ctx.source);
    let mut focus = FocusedPanel::Alerts;
    let mut dashboard_split = DashboardSplit::default();
    let mut dashboard_split_dragging = false;
    let mut target_picker_open = false;
    let mut target_picker_stage = TargetPickerStage::Session;
    let mut target_picker_session: Option<String> = None;
    let mut target_picker_state = ratatui::widgets::ListState::default();
    let mut target_choices: Vec<TargetChoice> = Vec::new();
    let mut target_preview_title = "Panes".to_string();
    let mut target_preview_lines: Vec<String> = Vec::new();
    let mut git_modal = ScrollModalState::default();
    let mut help_modal = ScrollModalState::default();
    let mut settings_overlay = qmonster::ui::settings::SettingsOverlay::new();
    let mut last_alert_click: Option<AlertMouseClick> = None;
    let mut last_pane_idle_states: HashMap<String, Option<IdleCause>> = HashMap::new();
    let mut pane_state_flashes: HashMap<String, qmonster::ui::panels::PaneStateFlash> =
        HashMap::new();
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
                        selected_target.as_ref(),
                        PollTickState {
                            last_poll_error: &mut last_poll_error,
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
                let target = target_label(selected_target.as_ref());
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
                            target_picker_open,
                            target_picker_stage,
                            target_picker_session: target_picker_session.as_deref(),
                            target_picker_state: &mut target_picker_state,
                            target_choices: &target_choices,
                            target_preview_title: &target_preview_title,
                            target_preview_lines: &target_preview_lines,
                            git_modal: &git_modal,
                            help_modal: &help_modal,
                            settings_overlay: &settings_overlay,
                            config: &ctx.config,
                        },
                    );
                })?;

                if event::poll(Duration::from_millis(100))? {
                    match event::read()? {
                        Event::Key(k) if k.kind == KeyEventKind::Press => {
                            if git_modal.is_open() {
                                let size = terminal.size()?;
                                let max_scroll = qmonster::ui::dashboard::max_git_scroll(
                                    Rect::new(0, 0, size.width, size.height),
                                    git_modal.line_count(),
                                );
                                handle_scroll_modal_key(&mut git_modal, k.code, max_scroll, None);
                                continue;
                            }

                            if help_modal.is_open() {
                                let size = terminal.size()?;
                                let max_scroll = qmonster::ui::dashboard::max_help_scroll(
                                    Rect::new(0, 0, size.width, size.height),
                                );
                                handle_scroll_modal_key(
                                    &mut help_modal,
                                    k.code,
                                    max_scroll,
                                    Some(KeyCode::Char('?')),
                                );
                                continue;
                            }

                            if target_picker_open {
                                let action = handle_target_picker_key(
                                    &ctx.source,
                                    TargetPickerController {
                                        open: &mut target_picker_open,
                                        stage: &mut target_picker_stage,
                                        session: &mut target_picker_session,
                                        state: &mut target_picker_state,
                                        choices: &mut target_choices,
                                        preview_title: &mut target_preview_title,
                                        preview_lines: &mut target_preview_lines,
                                        selected_target: &mut selected_target,
                                    },
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

                            let now = Instant::now();
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
                                KeyCode::Char('t') => {
                                    open_target_picker(
                                        &ctx.source,
                                        TargetPickerController {
                                            open: &mut target_picker_open,
                                            stage: &mut target_picker_stage,
                                            session: &mut target_picker_session,
                                            state: &mut target_picker_state,
                                            choices: &mut target_choices,
                                            preview_title: &mut target_preview_title,
                                            preview_lines: &mut target_preview_lines,
                                            selected_target: &mut selected_target,
                                        },
                                    );
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
                                KeyCode::Char('c') => {
                                    dashboard.clear_notices(Instant::now());
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

                            if git_modal.is_open() {
                                dashboard_split_dragging = false;
                                let rects = git_modal_rects(viewport);
                                let max_scroll = qmonster::ui::dashboard::max_git_scroll(
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
                                let max_scroll = qmonster::ui::dashboard::max_help_scroll(viewport);
                                handle_scroll_modal_mouse(
                                    &mut help_modal,
                                    m,
                                    rects.body,
                                    close_button_rect(rects.body),
                                    max_scroll,
                                );
                                continue;
                            }

                            if target_picker_open {
                                dashboard_split_dragging = false;
                                let action = handle_target_picker_mouse(
                                    &ctx.source,
                                    TargetPickerController {
                                        open: &mut target_picker_open,
                                        stage: &mut target_picker_stage,
                                        session: &mut target_picker_session,
                                        state: &mut target_picker_state,
                                        choices: &mut target_choices,
                                        preview_title: &mut target_preview_title,
                                        preview_lines: &mut target_preview_lines,
                                        selected_target: &mut selected_target,
                                    },
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
