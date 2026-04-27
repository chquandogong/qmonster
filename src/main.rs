use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use ratatui::widgets::ListState;

use qmonster::app::bootstrap::Context;
use qmonster::app::clipboard_actions::{
    AlertCommandCopyView, copy_selected_alert_command_to_clipboard,
};
use qmonster::app::config::{QmonsterConfig, load_from_path};
use qmonster::app::dashboard_render::{DashboardFrameView, render_dashboard_frame};
use qmonster::app::dashboard_state::{
    AlertMouseClick, DashboardMouseAction, DashboardMouseView, DashboardSelectionKeyView,
    DashboardSyncState, handle_dashboard_mouse, handle_dashboard_selection_key,
    refresh_alert_state, sync_alert_selection, sync_dashboard_state, sync_pane_selection,
    update_pane_state_flashes,
};
use qmonster::app::event_loop::{PaneReport, run_once, run_once_with_target};
use qmonster::app::git_info::capture_repo_panel;
use qmonster::app::keymap::{FocusedPanel, toggle_focus};
use qmonster::app::modal_state::{
    ScrollModalState, handle_scroll_modal_key, handle_scroll_modal_mouse,
};
use qmonster::app::once_report::print_once_reports;
use qmonster::app::operator_actions::{version_refresh_notices, write_operator_snapshot};
use qmonster::app::path_resolution::{default_config_path, pick_root};
use qmonster::app::prompt_send_actions::handle_prompt_send_action;
use qmonster::app::runtime_refresh::handle_runtime_refresh_action;
use qmonster::app::safety_audit::apply_override_with_audit;
use qmonster::app::settings_overlay::{handle_settings_overlay_key, handle_settings_overlay_mouse};
use qmonster::app::system_notice::{
    SystemNotice, record_startup_snapshot, route_polling_failure, route_polling_recovered,
    route_version_drift,
};
use qmonster::app::target_picker::{
    TargetChoice, TargetPickerAction, TargetPickerController, TargetPickerStage,
    handle_target_picker_key, handle_target_picker_mouse, initial_target, open_target_picker,
    target_label, target_switched_notice,
};
use qmonster::app::version_drift::{
    StartupLoad, VersionSnapshot, capture_versions, load_startup_snapshot,
};
use qmonster::domain::audit::{AuditEvent, AuditEventKind};
use qmonster::domain::origin::SourceKind;
use qmonster::domain::recommendation::Severity;
use qmonster::domain::signal::IdleCause;
use qmonster::notify::desktop::DesktopNotifier;
use qmonster::policy::claude_settings::{ClaudeSettings, ClaudeSettingsError};
use qmonster::policy::pricing::PricingTable;
use qmonster::store::{
    ArchiveWriter, EventSink, InMemorySink, SnapshotWriter, SqliteAuditSink, sweep,
};
use qmonster::tmux::polling::{PaneSource, PollingSource};
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
    let default_config_path = default_config_path(cli.root.as_deref(), env_root.as_deref());
    let loaded_config_path = cli.config.clone().or_else(|| {
        if default_config_path.exists() {
            Some(default_config_path.clone())
        } else {
            None
        }
    });
    let config = match loaded_config_path.as_ref() {
        Some(path) => load_from_path(path).with_context(|| format!("loading {path:?}"))?,
        None => QmonsterConfig::defaults(),
    };
    let writable_config_path = cli
        .config
        .clone()
        .unwrap_or_else(|| default_config_path.clone());
    let mut pairs: Vec<(String, String)> = Vec::new();
    for kv in &cli.set {
        let Some((k, v)) = kv.split_once('=') else {
            anyhow::bail!("--set expects key=value, got {kv}");
        };
        pairs.push((k.trim().into(), v.trim().into()));
    }

    let resolved = pick_root(cli.root.as_deref(), env_root.as_deref(), &config);
    let paths = resolved.clone().into_paths();
    paths.ensure().context("ensure ~/.qmonster layout")?;

    // Phase-2: open durable audit sink; fall back to in-memory if the
    // DB can't open (disk full, permission issues, etc.) so the TUI
    // never silently abandons observe-first behaviour.
    let sink: Box<dyn EventSink> = match SqliteAuditSink::open(&paths.sqlite_path()) {
        Ok(db) => Box::new(db),
        Err(e) => {
            eprintln!(
                "qmonster: falling back to in-memory audit sink ({e}); events \
                 will not survive restart this session"
            );
            Box::new(InMemorySink::new())
        }
    };

    let source = PollingSource::new(config.tmux.capture_lines);
    let notifier = DesktopNotifier;
    let archive = ArchiveWriter::new(paths.clone(), config.logging.big_output_chars);

    let pricing_path = paths.pricing_path();
    let pricing = match PricingTable::load_from_toml(&pricing_path) {
        Ok(table) => table,
        Err(qmonster::policy::pricing::PricingError::Io(io_err))
            if io_err.kind() == std::io::ErrorKind::NotFound =>
        {
            // absent by default — silent fallback is the documented behaviour
            PricingTable::empty()
        }
        Err(e) => {
            // v1.11.2 remediation (Gemini v1.11.0 must-fix #2 + Codex
            // Q5): record a durable breadcrumb via the audit sink so
            // the fallback is visible via SQLite query and survives
            // the TUI alternate-screen cycle that swallows stderr.
            // Keep the ephemeral eprintln for dev / non-TUI runs.
            sink.record(AuditEvent {
                kind: AuditEventKind::PricingLoadFailed,
                pane_id: "n/a".into(),
                severity: Severity::Warning,
                summary: format!("pricing load failed at {}: {e}", pricing_path.display()),
                provider: None,
                role: None,
            });
            eprintln!(
                "qmonster: failed to load pricing table at {}: {e}; cost badges disabled this session",
                pricing_path.display()
            );
            PricingTable::empty()
        }
    };

    let claude_settings = match ClaudeSettings::default_path() {
        Some(path) => match ClaudeSettings::load_from_path(&path) {
            Ok(s) => s,
            Err(ClaudeSettingsError::Io(io)) if io.kind() == std::io::ErrorKind::NotFound => {
                ClaudeSettings::empty()
            }
            Err(e) => {
                sink.record(qmonster::domain::audit::AuditEvent {
                    kind: qmonster::domain::audit::AuditEventKind::ClaudeSettingsLoadFailed,
                    pane_id: "n/a".into(),
                    severity: qmonster::domain::recommendation::Severity::Warning,
                    summary: format!("claude settings load failed at {}: {}", path.display(), e),
                    provider: None,
                    role: None,
                });
                eprintln!(
                    "qmonster: failed to load claude settings at {}: {e}; claude model badge disabled this session",
                    path.display()
                );
                ClaudeSettings::empty()
            }
        },
        None => ClaudeSettings::empty(),
    };

    let mut ctx = Context::new(config, source, notifier, sink)
        .with_archive(archive)
        .with_pricing(pricing)
        .with_claude_settings(claude_settings);
    ctx = ctx.with_config_path(writable_config_path);

    if !pairs.is_empty() {
        let refs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let stats = apply_override_with_audit(&mut ctx.config, &refs, &*ctx.sink);
        if stats.rejected + stats.unknown > 0 {
            eprintln!(
                "qmonster: {} override(s) rejected, {} unknown key(s); see audit log",
                stats.rejected, stats.unknown
            );
        }
    }

    // Retention sweep (startup-only in Phase 2; Phase 3 may schedule it).
    match sweep(&paths, ctx.config.logging.retention_days) {
        Ok(report) => {
            if report.files_removed > 0 {
                ctx.sink.record(AuditEvent {
                    kind: AuditEventKind::RetentionSwept,
                    pane_id: "n/a".into(),
                    severity: Severity::Safe,
                    summary: format!(
                        "retention: removed {} file(s), {} byte(s); kept {}",
                        report.files_removed, report.bytes_removed, report.files_kept
                    ),
                    provider: None,
                    role: None,
                });
            }
        }
        Err(e) => eprintln!("qmonster: retention sweep failed: {e}"),
    }

    // Load previous version snapshot with error surfacing (Codex Phase-2 #1):
    // a corrupted file is audit-logged AND preserved (may_save_fresh = false).
    let startup = load_startup_snapshot(&*ctx.sink, &paths.versions_path());
    let may_save_fresh = startup.may_save_fresh();
    let fresh = capture_versions();
    let mut startup_notices: Vec<SystemNotice> = Vec::new();
    match &startup {
        StartupLoad::Previous(prev) => {
            startup_notices = route_version_drift(prev, &fresh, &*ctx.sink);
        }
        StartupLoad::Fresh => {}
        StartupLoad::Corrupted(_) => {
            startup_notices.push(SystemNotice {
                title: "versions.json corrupted".into(),
                body: format!(
                    "{} left in place for inspection; drift detection skipped this session",
                    paths.versions_path().display()
                ),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
            });
        }
    }
    record_startup_snapshot(&*ctx.sink, &fresh);
    if may_save_fresh && let Err(e) = fresh.save_to(&paths.versions_path()) {
        eprintln!("qmonster: could not persist version snapshot: {e}");
    }

    let snapshot_writer = SnapshotWriter::new(paths.clone());

    if cli.once {
        println!(
            "qmonster paths: {} (source: {:?})",
            paths.root().display(),
            resolved.source
        );
        println!("qmonster versions captured:");
        for (k, v) in &fresh.tools {
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

    run_tui(&mut ctx, fresh, snapshot_writer, startup_notices)
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
    let mut stdout = io::stdout();
    enable_raw_mode().context("enable raw mode")?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let poll = ctx.config.tmux.poll_interval();
    let mut last_reports: Vec<PaneReport> = Vec::new();
    let mut notices: Vec<SystemNotice> = startup_notices;
    let mut last_poll = Instant::now() - poll;
    let mut last_poll_error: Option<String> = None;
    let mut selected_target = initial_target(&ctx.source);
    let mut focus = FocusedPanel::Alerts;
    let mut dashboard_split = DashboardSplit::default();
    let mut dashboard_split_dragging = false;
    let mut alert_state = ListState::default();
    let mut pane_state = ListState::default();
    let mut target_picker_open = false;
    let mut target_picker_stage = TargetPickerStage::Session;
    let mut target_picker_session: Option<String> = None;
    let mut target_picker_state = ListState::default();
    let mut target_choices: Vec<TargetChoice> = Vec::new();
    let mut target_preview_title = "Panes".to_string();
    let mut target_preview_lines: Vec<String> = Vec::new();
    let mut git_modal = ScrollModalState::default();
    let mut help_modal = ScrollModalState::default();
    let mut settings_overlay = qmonster::ui::settings::SettingsOverlay::new();
    let mut previous_alerts: HashSet<String> = HashSet::new();
    let mut fresh_alerts: HashSet<String> = HashSet::new();
    let mut alert_times: HashMap<String, String> = HashMap::new();
    let mut alert_hide_deadlines: HashMap<String, Instant> = HashMap::new();
    let mut last_alert_click: Option<AlertMouseClick> = None;
    let mut last_pane_idle_states: HashMap<String, Option<IdleCause>> = HashMap::new();
    let mut pane_state_flashes: HashMap<String, qmonster::ui::panels::PaneStateFlash> =
        HashMap::new();
    let mut runtime_refresh_offsets: HashMap<String, usize> = HashMap::new();
    refresh_alert_state(
        &notices,
        &last_reports,
        &mut previous_alerts,
        &mut fresh_alerts,
        &mut alert_times,
        &mut alert_hide_deadlines,
    );
    sync_alert_selection(
        &mut alert_state,
        &notices,
        &last_reports,
        &alert_hide_deadlines,
        Instant::now(),
    );
    sync_pane_selection(&mut pane_state, 0);

    let result = {
        let mut run_loop = || -> anyhow::Result<()> {
            loop {
                let now = Instant::now();
                if now.saturating_duration_since(last_poll) >= poll {
                    last_poll = now;
                    match run_once_with_target(ctx, now, selected_target.as_ref()) {
                        Ok(reports) => {
                            if let Some(notice) = route_polling_recovered(&mut last_poll_error) {
                                notices.insert(0, notice);
                            }
                            update_pane_state_flashes(
                                &reports,
                                &mut last_pane_idle_states,
                                &mut pane_state_flashes,
                                now,
                            );
                            last_reports = reports;
                            sync_dashboard_state(
                                &notices,
                                &last_reports,
                                DashboardSyncState {
                                    alert_state: &mut alert_state,
                                    pane_state: &mut pane_state,
                                    previous_alerts: &mut previous_alerts,
                                    fresh_alerts: &mut fresh_alerts,
                                    alert_times: &mut alert_times,
                                    alert_hide_deadlines: &mut alert_hide_deadlines,
                                },
                                now,
                            );
                        }
                        Err(e) => {
                            if let Some(notice) =
                                route_polling_failure(&mut last_poll_error, e.to_string())
                            {
                                notices.insert(0, notice);
                                sync_dashboard_state(
                                    &notices,
                                    &last_reports,
                                    DashboardSyncState {
                                        alert_state: &mut alert_state,
                                        pane_state: &mut pane_state,
                                        previous_alerts: &mut previous_alerts,
                                        fresh_alerts: &mut fresh_alerts,
                                        alert_times: &mut alert_times,
                                        alert_hide_deadlines: &mut alert_hide_deadlines,
                                    },
                                    now,
                                );
                            }
                        }
                    }
                }

                pane_state_flashes.retain(|_, flash| flash.is_active(now));
                sync_alert_selection(
                    &mut alert_state,
                    &notices,
                    &last_reports,
                    &alert_hide_deadlines,
                    now,
                );
                let target = target_label(selected_target.as_ref());
                terminal.draw(|frame| {
                    render_dashboard_frame(
                        frame,
                        DashboardFrameView {
                            alert_state: &mut alert_state,
                            pane_state: &mut pane_state,
                            notices: &notices,
                            reports: &last_reports,
                            fresh_alerts: &fresh_alerts,
                            alert_times: &alert_times,
                            hidden_until: &alert_hide_deadlines,
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
                                    notices.insert(0, target_switched_notice(&label));
                                    sync_dashboard_state(
                                        &notices,
                                        &last_reports,
                                        DashboardSyncState {
                                            alert_state: &mut alert_state,
                                            pane_state: &mut pane_state,
                                            previous_alerts: &mut previous_alerts,
                                            fresh_alerts: &mut fresh_alerts,
                                            alert_times: &mut alert_times,
                                            alert_hide_deadlines: &mut alert_hide_deadlines,
                                        },
                                        now,
                                    );
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
                                    alert_state: &mut alert_state,
                                    pane_state: &mut pane_state,
                                    notices: &notices,
                                    reports: &last_reports,
                                    alert_hide_deadlines: &mut alert_hide_deadlines,
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
                                        notices = new_notices;
                                        sync_dashboard_state(
                                            &notices,
                                            &last_reports,
                                            DashboardSyncState {
                                                alert_state: &mut alert_state,
                                                pane_state: &mut pane_state,
                                                previous_alerts: &mut previous_alerts,
                                                fresh_alerts: &mut fresh_alerts,
                                                alert_times: &mut alert_times,
                                                alert_hide_deadlines: &mut alert_hide_deadlines,
                                            },
                                            Instant::now(),
                                        );
                                    }
                                    versions = fresh;
                                }
                                KeyCode::Char('s') => {
                                    let notice = write_operator_snapshot(
                                        &snapshot_writer,
                                        &*ctx.sink,
                                        &last_reports,
                                        &notices,
                                    );
                                    notices.insert(0, notice);
                                    sync_dashboard_state(
                                        &notices,
                                        &last_reports,
                                        DashboardSyncState {
                                            alert_state: &mut alert_state,
                                            pane_state: &mut pane_state,
                                            previous_alerts: &mut previous_alerts,
                                            fresh_alerts: &mut fresh_alerts,
                                            alert_times: &mut alert_times,
                                            alert_hide_deadlines: &mut alert_hide_deadlines,
                                        },
                                        Instant::now(),
                                    );
                                }
                                KeyCode::Char('c') => {
                                    notices.clear();
                                    sync_dashboard_state(
                                        &notices,
                                        &last_reports,
                                        DashboardSyncState {
                                            alert_state: &mut alert_state,
                                            pane_state: &mut pane_state,
                                            previous_alerts: &mut previous_alerts,
                                            fresh_alerts: &mut fresh_alerts,
                                            alert_times: &mut alert_times,
                                            alert_hide_deadlines: &mut alert_hide_deadlines,
                                        },
                                        Instant::now(),
                                    );
                                }
                                KeyCode::Char('u') if focus == FocusedPanel::Panes => {
                                    let selected =
                                        pane_state.selected().and_then(|i| last_reports.get(i));
                                    let outcome = handle_runtime_refresh_action(
                                        &ctx.source,
                                        &*ctx.sink,
                                        selected,
                                        ctx.config.actions.mode,
                                        ctx.config.tmux.capture_lines,
                                        &mut runtime_refresh_offsets,
                                        &mut ctx.runtime_refresh_tail_overlays,
                                    );
                                    notices.insert(0, outcome.notice);
                                    if outcome.force_poll {
                                        last_poll = Instant::now() - poll;
                                    }
                                    sync_dashboard_state(
                                        &notices,
                                        &last_reports,
                                        DashboardSyncState {
                                            alert_state: &mut alert_state,
                                            pane_state: &mut pane_state,
                                            previous_alerts: &mut previous_alerts,
                                            fresh_alerts: &mut fresh_alerts,
                                            alert_times: &mut alert_times,
                                            alert_hide_deadlines: &mut alert_hide_deadlines,
                                        },
                                        Instant::now(),
                                    );
                                }
                                KeyCode::Char('y') if focus == FocusedPanel::Alerts => {
                                    let now = Instant::now();
                                    let notice = copy_selected_alert_command_to_clipboard(
                                        AlertCommandCopyView {
                                            alert_state: &alert_state,
                                            notices: &notices,
                                            reports: &last_reports,
                                            fresh_alerts: &fresh_alerts,
                                            alert_times: &alert_times,
                                            hidden_until: &alert_hide_deadlines,
                                            now,
                                        },
                                    );
                                    notices.insert(0, notice);
                                    sync_dashboard_state(
                                        &notices,
                                        &last_reports,
                                        DashboardSyncState {
                                            alert_state: &mut alert_state,
                                            pane_state: &mut pane_state,
                                            previous_alerts: &mut previous_alerts,
                                            fresh_alerts: &mut fresh_alerts,
                                            alert_times: &mut alert_times,
                                            alert_hide_deadlines: &mut alert_hide_deadlines,
                                        },
                                        now,
                                    );
                                }
                                KeyCode::Char('p') | KeyCode::Char('d') => {
                                    let notice = handle_prompt_send_action(
                                        &ctx.source,
                                        &*ctx.sink,
                                        &last_reports,
                                        pane_state.selected(),
                                        k.code == KeyCode::Char('p'),
                                        ctx.config.actions.mode,
                                        ctx.config.actions.allow_auto_prompt_send,
                                    );
                                    notices.insert(0, notice);
                                    sync_dashboard_state(
                                        &notices,
                                        &last_reports,
                                        DashboardSyncState {
                                            alert_state: &mut alert_state,
                                            pane_state: &mut pane_state,
                                            previous_alerts: &mut previous_alerts,
                                            fresh_alerts: &mut fresh_alerts,
                                            alert_times: &mut alert_times,
                                            alert_hide_deadlines: &mut alert_hide_deadlines,
                                        },
                                        Instant::now(),
                                    );
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
                                    notices.insert(0, target_switched_notice(&label));
                                    sync_dashboard_state(
                                        &notices,
                                        &last_reports,
                                        DashboardSyncState {
                                            alert_state: &mut alert_state,
                                            pane_state: &mut pane_state,
                                            previous_alerts: &mut previous_alerts,
                                            fresh_alerts: &mut fresh_alerts,
                                            alert_times: &mut alert_times,
                                            alert_hide_deadlines: &mut alert_hide_deadlines,
                                        },
                                        now,
                                    );
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
                                    alert_state: &mut alert_state,
                                    pane_state: &mut pane_state,
                                    last_alert_click: &mut last_alert_click,
                                    alert_hide_deadlines: &mut alert_hide_deadlines,
                                    notices: &notices,
                                    reports: &last_reports,
                                    fresh_alerts: &fresh_alerts,
                                    alert_times: &alert_times,
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

    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    result
}
