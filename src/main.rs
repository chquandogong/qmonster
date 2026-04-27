use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use ratatui::widgets::ListState;

use qmonster::app::bootstrap::Context;
use qmonster::app::config::{ActionsMode, QmonsterConfig, load_from_path};
use qmonster::app::dashboard_state::{
    AlertMouseClick, DashboardSyncState, alert_key_at_index, refresh_alert_state,
    register_alert_double_click, sync_alert_selection, sync_dashboard_state, sync_pane_selection,
    toggle_alert_severity_hide, toggle_selected_alert_hide, update_pane_state_flashes,
};
use qmonster::app::event_loop::{PaneReport, run_once, run_once_with_target};
use qmonster::app::git_info::capture_repo_panel;
use qmonster::app::keymap::{
    FocusedPanel, ScrollDir, list_row_at, move_selection, page_selection, rect_contains,
    select_first, select_last, toggle_focus,
};
use qmonster::app::modal_state::{
    ScrollModalState, handle_scroll_modal_key, handle_scroll_modal_mouse,
};
use qmonster::app::operator_actions::{version_refresh_notices, write_operator_snapshot};
use qmonster::app::path_resolution::pick_root;
use qmonster::app::runtime_refresh::{
    runtime_refresh_command_label, runtime_refresh_commands, runtime_refresh_completion_label,
    runtime_refresh_dispatch_commands, runtime_refresh_notice_body, runtime_refresh_provider_label,
    runtime_refresh_request_label, runtime_refresh_sends_one_command_at_a_time,
    runtime_refresh_uses_active_safe_only, send_runtime_refresh_commands,
};
use qmonster::app::safety_audit::apply_override_with_audit;
use qmonster::app::settings_overlay::{handle_settings_overlay_key, handle_settings_overlay_mouse};
use qmonster::app::system_notice::{
    SystemNotice, record_startup_snapshot, route_polling_failure, route_polling_recovered,
    route_version_drift,
};
use qmonster::app::target_picker::{
    TargetChoice, TargetPickerOutcome, TargetPickerStage, apply_target_choice,
    refresh_target_choices, refresh_target_preview, target_choice_index_at_row, target_label,
    target_picker_hint, target_picker_title, target_switched_notice,
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
use qmonster::policy::gates::{PromptSendGate, check_send_gate};
use qmonster::policy::pricing::PricingTable;
use qmonster::store::{
    ArchiveWriter, EventSink, InMemorySink, QmonsterPaths, SnapshotWriter, SqliteAuditSink, sweep,
};
use qmonster::tmux::polling::{PaneSource, PollingSource};
use qmonster::tmux::types::WindowTarget;
use qmonster::ui::dashboard::{
    DashboardSplit, DashboardView, TargetPickerView, close_button_rect, dashboard_rects,
    dashboard_split_from_row, git_modal_rects, help_modal_rects, render_dashboard,
    target_picker_rects, version_badge_rect,
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
        print_reports(&reports, &ctx.config);
        return Ok(());
    }

    run_tui(&mut ctx, fresh, snapshot_writer, startup_notices)
}

fn default_config_path(cli_root: Option<&Path>, env_root: Option<&str>) -> PathBuf {
    let root = if let Some(env) = env_root
        && !env.is_empty()
    {
        PathBuf::from(env)
    } else if let Some(cli) = cli_root {
        cli.to_path_buf()
    } else {
        QmonsterPaths::default_root().root().to_path_buf()
    };
    QmonsterPaths::at(root).config_path()
}

fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| e.to_string())
}

fn print_reports(reports: &[PaneReport], config: &qmonster::app::config::QmonsterConfig) {
    // 1. Cross-pane findings.
    for rep in reports {
        for f in &rep.cross_pane_findings {
            println!(
                "[{}] [{}] CROSS-PANE: {} (anchor: {}, others: {})",
                f.severity.letter(),
                qmonster::ui::labels::source_kind_label(f.source_kind),
                f.reason,
                f.anchor_pane_id,
                f.other_pane_ids.join(", "),
            );
        }
    }
    // 2. Strong recommendations (G-7 checkpoint UX).
    for rep in reports {
        for rec in rep.recommendations.iter().filter(|r| r.is_strong) {
            println!(
                "{}",
                qmonster::ui::alerts::format_strong_rec_body(rec, &rep.pane_id)
            );
        }
    }
    // 2b. Pending prompt-send proposals (P5-2 v1.9.2). Emitted
    // alongside strong recs so the operator sees the structured
    // proposal immediately below the CHECKPOINT line that generated
    // it. `--once` mode has no accept keystroke (non-interactive),
    // but we still reflect the `EffectRunner::permit` gate state so
    // `observe_only` prints an honest "send disabled" form instead of
    // advertising keys that would be ignored by the interactive TUI.
    let runner = qmonster::app::effects::EffectRunner::new(config);
    for rep in reports {
        for effect in &rep.effects {
            if let qmonster::domain::recommendation::RequestedEffect::PromptSendProposed {
                target_pane_id,
                slash_command,
                ..
            } = effect
            {
                let accept_gated = runner.permit(effect);
                println!(
                    "{}",
                    qmonster::ui::alerts::format_prompt_send_proposal(
                        target_pane_id,
                        slash_command,
                        accept_gated,
                    )
                );
            }
        }
    }
    // 3. Per-pane summaries with non-strong recommendations.
    for r in reports {
        println!(
            "{}:{} {} {:?}:{}:{:?} confidence={:?} dead={}",
            r.session_name,
            r.window_index,
            r.pane_id,
            r.identity.identity.provider,
            r.identity.identity.instance,
            r.identity.identity.role,
            r.identity.confidence,
            r.dead
        );
        println!("  path: {}", r.current_path);
        println!("  cmd: {}", r.current_command);
        let chips = qmonster::ui::panels::signal_chips(&r.signals);
        if !chips.is_empty() {
            println!("  state: {}", chips.join(" | "));
        }
        let metrics = qmonster::ui::panels::metric_row(&r.signals);
        if !metrics.is_empty() {
            println!("  metrics: {metrics}");
        }
        let runtime = qmonster::ui::panels::runtime_row(&r.signals);
        if !runtime.is_empty() {
            println!("  runtime: {runtime}");
        }
        if !r.effects.is_empty() {
            let names: Vec<String> = r.effects.iter().map(|e| format!("{e:?}")).collect();
            println!("  effects: {}", names.join(" "));
        }
        for rec in r.recommendations.iter().filter(|rec| !rec.is_strong) {
            println!(
                "  {}",
                qmonster::ui::alerts::format_recommendation_body(rec, &r.pane_id)
            );
            // v1.8.1: surface the structured ProviderProfile payload so
            // lever key/value/citation/SourceKind are visible in --once
            // (Codex P4-1 finding #1 closed).
            for line in qmonster::ui::panels::format_profile_lines(rec) {
                println!("    {line}");
            }
        }
    }
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

    let result = (|| -> anyhow::Result<()> {
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
                render_dashboard(
                    frame,
                    &mut alert_state,
                    &mut pane_state,
                    DashboardView {
                        notices: &notices,
                        reports: &last_reports,
                        fresh_alerts: &fresh_alerts,
                        alert_times: &alert_times,
                        hidden_until: &alert_hide_deadlines,
                        state_flashes: &pane_state_flashes,
                        now,
                        target_label: &target,
                        split: dashboard_split,
                        alerts_focused: !target_picker_open
                            && !help_modal.is_open()
                            && !settings_overlay.is_open()
                            && focus == FocusedPanel::Alerts,
                        panes_focused: !target_picker_open
                            && !help_modal.is_open()
                            && !settings_overlay.is_open()
                            && focus == FocusedPanel::Panes,
                    },
                );
                if target_picker_open {
                    let labels: Vec<String> = target_choices
                        .iter()
                        .map(|choice| choice.label.clone())
                        .collect();
                    let picker_title =
                        target_picker_title(target_picker_stage, target_picker_session.as_deref());
                    qmonster::ui::dashboard::render_target_picker(
                        frame,
                        &mut target_picker_state,
                        TargetPickerView {
                            title: &picker_title,
                            hint: target_picker_hint(target_picker_stage),
                            labels: &labels,
                            preview_title: &target_preview_title,
                            preview_lines: &target_preview_lines,
                            current_label: &target,
                        },
                    );
                }
                if git_modal.is_open() {
                    qmonster::ui::dashboard::render_git_modal(
                        frame,
                        git_modal.title(),
                        git_modal.lines(),
                        git_modal.scroll() as u16,
                    );
                }
                if help_modal.is_open() {
                    qmonster::ui::dashboard::render_help_modal(frame, help_modal.scroll() as u16);
                }
                if settings_overlay.is_open() {
                    qmonster::ui::settings::render_settings_modal(
                        frame,
                        &settings_overlay,
                        &ctx.config,
                    );
                }
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
                            let max_scroll = qmonster::ui::dashboard::max_help_scroll(Rect::new(
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

                        if target_picker_open {
                            match k.code {
                                KeyCode::Esc | KeyCode::Char('t') => target_picker_open = false,
                                KeyCode::Left | KeyCode::Backspace => {
                                    if target_picker_stage == TargetPickerStage::Window {
                                        target_picker_stage = TargetPickerStage::Session;
                                        target_picker_session = None;
                                        refresh_target_choices(
                                            &ctx.source,
                                            target_picker_stage,
                                            target_picker_session.as_deref(),
                                            &mut target_choices,
                                            &mut target_picker_state,
                                            selected_target.as_ref(),
                                        );
                                        refresh_target_preview(
                                            &ctx.source,
                                            &target_choices,
                                            &target_picker_state,
                                            &mut target_preview_title,
                                            &mut target_preview_lines,
                                        );
                                    }
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    move_selection(
                                        &mut target_picker_state,
                                        target_choices.len(),
                                        -1,
                                    );
                                    refresh_target_preview(
                                        &ctx.source,
                                        &target_choices,
                                        &target_picker_state,
                                        &mut target_preview_title,
                                        &mut target_preview_lines,
                                    );
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    move_selection(
                                        &mut target_picker_state,
                                        target_choices.len(),
                                        1,
                                    );
                                    refresh_target_preview(
                                        &ctx.source,
                                        &target_choices,
                                        &target_picker_state,
                                        &mut target_preview_title,
                                        &mut target_preview_lines,
                                    );
                                }
                                KeyCode::PageUp => {
                                    page_selection(
                                        &mut target_picker_state,
                                        target_choices.len(),
                                        6,
                                        ScrollDir::Up,
                                    );
                                    refresh_target_preview(
                                        &ctx.source,
                                        &target_choices,
                                        &target_picker_state,
                                        &mut target_preview_title,
                                        &mut target_preview_lines,
                                    );
                                }
                                KeyCode::PageDown => {
                                    page_selection(
                                        &mut target_picker_state,
                                        target_choices.len(),
                                        6,
                                        ScrollDir::Down,
                                    );
                                    refresh_target_preview(
                                        &ctx.source,
                                        &target_choices,
                                        &target_picker_state,
                                        &mut target_preview_title,
                                        &mut target_preview_lines,
                                    );
                                }
                                KeyCode::Home => {
                                    select_first(&mut target_picker_state, target_choices.len());
                                    refresh_target_preview(
                                        &ctx.source,
                                        &target_choices,
                                        &target_picker_state,
                                        &mut target_preview_title,
                                        &mut target_preview_lines,
                                    );
                                }
                                KeyCode::End => {
                                    select_last(&mut target_picker_state, target_choices.len());
                                    refresh_target_preview(
                                        &ctx.source,
                                        &target_choices,
                                        &target_picker_state,
                                        &mut target_preview_title,
                                        &mut target_preview_lines,
                                    );
                                }
                                KeyCode::Enter => {
                                    match apply_target_choice(
                                        target_picker_stage,
                                        target_picker_session.as_deref(),
                                        &target_choices,
                                        &target_picker_state,
                                        &mut selected_target,
                                    ) {
                                        Some(TargetPickerOutcome::AdvanceToWindows(
                                            session_name,
                                        )) => {
                                            target_picker_stage = TargetPickerStage::Window;
                                            target_picker_session = Some(session_name);
                                            refresh_target_choices(
                                                &ctx.source,
                                                target_picker_stage,
                                                target_picker_session.as_deref(),
                                                &mut target_choices,
                                                &mut target_picker_state,
                                                selected_target.as_ref(),
                                            );
                                            refresh_target_preview(
                                                &ctx.source,
                                                &target_choices,
                                                &target_picker_state,
                                                &mut target_preview_title,
                                                &mut target_preview_lines,
                                            );
                                        }
                                        Some(TargetPickerOutcome::Close(label)) => {
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
                                                Instant::now(),
                                            );
                                            target_picker_open = false;
                                            last_poll = Instant::now() - poll;
                                        }
                                        None => {}
                                    }
                                }
                                _ => {}
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
                            KeyCode::Up | KeyCode::Char('k') => match focus {
                                FocusedPanel::Alerts => move_selection(
                                    &mut alert_state,
                                    qmonster::ui::alerts::alert_count(
                                        &notices,
                                        &last_reports,
                                        &alert_hide_deadlines,
                                        Instant::now(),
                                    ),
                                    -1,
                                ),
                                FocusedPanel::Panes => {
                                    move_selection(&mut pane_state, last_reports.len(), -1);
                                }
                            },
                            KeyCode::Down | KeyCode::Char('j') => match focus {
                                FocusedPanel::Alerts => move_selection(
                                    &mut alert_state,
                                    qmonster::ui::alerts::alert_count(
                                        &notices,
                                        &last_reports,
                                        &alert_hide_deadlines,
                                        Instant::now(),
                                    ),
                                    1,
                                ),
                                FocusedPanel::Panes => {
                                    move_selection(&mut pane_state, last_reports.len(), 1);
                                }
                            },
                            KeyCode::PageUp => match focus {
                                FocusedPanel::Alerts => page_selection(
                                    &mut alert_state,
                                    qmonster::ui::alerts::alert_count(
                                        &notices,
                                        &last_reports,
                                        &alert_hide_deadlines,
                                        Instant::now(),
                                    ),
                                    6,
                                    ScrollDir::Up,
                                ),
                                FocusedPanel::Panes => {
                                    page_selection(
                                        &mut pane_state,
                                        last_reports.len(),
                                        3,
                                        ScrollDir::Up,
                                    );
                                }
                            },
                            KeyCode::PageDown => match focus {
                                FocusedPanel::Alerts => page_selection(
                                    &mut alert_state,
                                    qmonster::ui::alerts::alert_count(
                                        &notices,
                                        &last_reports,
                                        &alert_hide_deadlines,
                                        Instant::now(),
                                    ),
                                    6,
                                    ScrollDir::Down,
                                ),
                                FocusedPanel::Panes => {
                                    page_selection(
                                        &mut pane_state,
                                        last_reports.len(),
                                        3,
                                        ScrollDir::Down,
                                    );
                                }
                            },
                            KeyCode::Home => match focus {
                                FocusedPanel::Alerts => select_first(
                                    &mut alert_state,
                                    qmonster::ui::alerts::alert_count(
                                        &notices,
                                        &last_reports,
                                        &alert_hide_deadlines,
                                        Instant::now(),
                                    ),
                                ),
                                FocusedPanel::Panes => {
                                    select_first(&mut pane_state, last_reports.len())
                                }
                            },
                            KeyCode::End => match focus {
                                FocusedPanel::Alerts => select_last(
                                    &mut alert_state,
                                    qmonster::ui::alerts::alert_count(
                                        &notices,
                                        &last_reports,
                                        &alert_hide_deadlines,
                                        Instant::now(),
                                    ),
                                ),
                                FocusedPanel::Panes => {
                                    select_last(&mut pane_state, last_reports.len())
                                }
                            },
                            KeyCode::Enter | KeyCode::Char(' ')
                                if focus == FocusedPanel::Alerts =>
                            {
                                toggle_selected_alert_hide(
                                    &mut alert_hide_deadlines,
                                    &alert_state,
                                    &notices,
                                    &last_reports,
                                    Instant::now(),
                                );
                                sync_alert_selection(
                                    &mut alert_state,
                                    &notices,
                                    &last_reports,
                                    &alert_hide_deadlines,
                                    Instant::now(),
                                );
                            }
                            KeyCode::Char('t') => {
                                target_picker_stage = TargetPickerStage::Session;
                                target_picker_session = None;
                                refresh_target_choices(
                                    &ctx.source,
                                    target_picker_stage,
                                    target_picker_session.as_deref(),
                                    &mut target_choices,
                                    &mut target_picker_state,
                                    selected_target.as_ref(),
                                );
                                refresh_target_preview(
                                    &ctx.source,
                                    &target_choices,
                                    &target_picker_state,
                                    &mut target_preview_title,
                                    &mut target_preview_lines,
                                );
                                target_picker_open = true;
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
                                match selected {
                                    None => notices.insert(
                                        0,
                                        SystemNotice {
                                            title: "no pane selected".into(),
                                            body: "select a provider pane before requesting runtime refresh".into(),
                                            severity: Severity::Concern,
                                            source_kind: SourceKind::ProjectCanonical,
                                        },
                                    ),
                                    Some(report) => {
                                        let pane_id = report.pane_id.clone();
                                        let provider = report.identity.identity.provider;
                                        let active_only = runtime_refresh_uses_active_safe_only(
                                            report.idle_state,
                                        );
                                        let available_commands =
                                            runtime_refresh_commands(provider, report.idle_state);
                                        if available_commands.is_empty() {
                                            notices.insert(
                                                0,
                                                SystemNotice {
                                                    title: "runtime refresh unavailable".into(),
                                                    body: format!(
                                                        "{} has no known read-only runtime slash command",
                                                        runtime_refresh_provider_label(provider)
                                                    ),
                                                    severity: Severity::Concern,
                                                    source_kind: SourceKind::ProjectCanonical,
                                                },
                                            );
                                        } else if matches!(
                                            ctx.config.actions.mode,
                                            ActionsMode::ObserveOnly
                                        ) {
                                            let command_label =
                                                runtime_refresh_command_label(available_commands);
                                            ctx.sink.record(AuditEvent {
                                                kind: AuditEventKind::RuntimeRefreshBlocked,
                                                pane_id: pane_id.clone(),
                                                severity: Severity::Warning,
                                                summary: format!(
                                                    "{pane_id} {command_label} (blocked; observe_only mode)"
                                                ),
                                                provider: Some(provider),
                                                role: Some(report.identity.identity.role),
                                            });
                                            notices.insert(
                                                0,
                                                SystemNotice {
                                                    title: "runtime refresh blocked".into(),
                                                    body: format!(
                                                        "{pane_id} → `{command_label}` blocked by observe_only mode"
                                                    ),
                                                    severity: Severity::Warning,
                                                    source_kind: SourceKind::ProjectCanonical,
                                                },
                                            );
                                        } else {
                                            let commands = runtime_refresh_dispatch_commands(
                                                provider,
                                                report.idle_state,
                                                &pane_id,
                                                &mut runtime_refresh_offsets,
                                            );
                                            let command_label =
                                                runtime_refresh_command_label(&commands);
                                            let one_at_a_time =
                                                runtime_refresh_sends_one_command_at_a_time(
                                                    provider,
                                                    report.idle_state,
                                                );
                                            ctx.sink.record(AuditEvent {
                                                kind: AuditEventKind::RuntimeRefreshRequested,
                                                pane_id: pane_id.clone(),
                                                severity: Severity::Concern,
                                                summary: format!(
                                                    "{pane_id} {command_label} ({})",
                                                    runtime_refresh_request_label(
                                                        active_only,
                                                        one_at_a_time,
                                                    )
                                                ),
                                                provider: Some(provider),
                                                role: Some(report.identity.identity.role),
                                            });
                                            let send_outcome = send_runtime_refresh_commands(
                                                &ctx.source,
                                                &pane_id,
                                                provider,
                                                report.idle_state,
                                                &commands,
                                                ctx.config.tmux.capture_lines,
                                                &mut ctx.runtime_refresh_tail_overlays,
                                            );
                                            match send_outcome.failed {
                                                None => {
                                                    ctx.sink.record(AuditEvent {
                                                        kind: AuditEventKind::RuntimeRefreshCompleted,
                                                        pane_id: pane_id.clone(),
                                                        severity: Severity::Safe,
                                                        summary: format!(
                                                            "{pane_id} {command_label} ({})",
                                                            runtime_refresh_completion_label(
                                                                send_outcome.captured_and_closed,
                                                            )
                                                        ),
                                                        provider: Some(provider),
                                                        role: Some(report.identity.identity.role),
                                                    });
                                                    notices.insert(
                                                        0,
                                                        SystemNotice {
                                                            title: "runtime refresh sent".into(),
                                                            body: runtime_refresh_notice_body(
                                                                &pane_id,
                                                                &command_label,
                                                                active_only,
                                                                one_at_a_time,
                                                                send_outcome.captured_and_closed,
                                                            ),
                                                            severity: Severity::Good,
                                                            source_kind: SourceKind::ProjectCanonical,
                                                        },
                                                    );
                                                    last_poll = Instant::now() - poll;
                                                }
                                                Some((failed_cmd, e)) => {
                                                    ctx.sink.record(AuditEvent {
                                                        kind: AuditEventKind::RuntimeRefreshFailed,
                                                        pane_id: pane_id.clone(),
                                                        severity: Severity::Warning,
                                                        summary: format!(
                                                            "{pane_id} {failed_cmd} (send failed: {e})"
                                                        ),
                                                        provider: Some(provider),
                                                        role: Some(report.identity.identity.role),
                                                    });
                                                    notices.insert(
                                                        0,
                                                        SystemNotice {
                                                            title: "runtime refresh failed".into(),
                                                            body: format!(
                                                                "{pane_id} → `{failed_cmd}`: tmux error — {e}"
                                                            ),
                                                            severity: Severity::Warning,
                                                            source_kind: SourceKind::ProjectCanonical,
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                    }
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
                                let command =
                                    qmonster::ui::alerts::selected_alert_suggested_command(
                                        &alert_state,
                                        &notices,
                                        &last_reports,
                                        &fresh_alerts,
                                        &alert_times,
                                        &alert_hide_deadlines,
                                        now,
                                    );
                                let notice = match command {
                                    Some(cmd) => match copy_text_to_clipboard(&cmd) {
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
                                        body:
                                            "select an alert with a run command before pressing y"
                                                .into(),
                                        severity: Severity::Concern,
                                        source_kind: SourceKind::ProjectCanonical,
                                    },
                                };
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
                                // P5-3 (v1.10.0): operator responds to a pending
                                // prompt-send proposal on the currently selected
                                // pane.
                                //
                                // 'p' (accept): runs through the TWO-GATE execution
                                //   path:
                                //   1. mode != observe_only (if blocked →
                                //      PromptSendBlocked audit + Warning notice)
                                //   2. allow_auto_prompt_send = true (if off →
                                //      PromptSendAccepted only; no tmux send)
                                //   Both pass → tmux send-keys → PromptSendAccepted
                                //   + PromptSendCompleted (or PromptSendFailed on
                                //   tmux error).
                                //
                                // 'd' (dismiss): always available in every mode;
                                //   records PromptSendRejected.
                                //
                                // EffectRunner::permit stays as the DISPLAY-LAYER
                                // filter (controls whether proposals show up in the
                                // UI). The execution gate is check_send_gate()
                                // above — a SEPARATE gate that does NOT reuse
                                // permit() per the P5-3 spec.
                                let accepting = k.code == KeyCode::Char('p');
                                let selected = pane_state.selected();
                                // P5-3: collect proposals sorted by
                                // proposal_id for deterministic selection
                                // when multiple proposals target one pane.
                                let pending = selected
                                    .and_then(|i| last_reports.get(i))
                                    .and_then(|rep| {
                                        let mut proposals: Vec<_> = rep.effects.iter().filter_map(|e| match e {
                                            qmonster::domain::recommendation::RequestedEffect::PromptSendProposed {
                                                target_pane_id,
                                                slash_command,
                                                proposal_id,
                                            } => Some((proposal_id.clone(), target_pane_id.clone(), slash_command.clone())),
                                            _ => None,
                                        }).collect();
                                        proposals.sort_by(|a, b| a.0.cmp(&b.0));
                                        proposals.into_iter().next().map(|(_, t, c)| (t, c))
                                    });
                                match pending {
                                    None => {
                                        notices.insert(
                                            0,
                                            SystemNotice {
                                                title: if accepting {
                                                    "no pending proposal to accept".into()
                                                } else {
                                                    "no pending proposal to dismiss".into()
                                                },
                                                body: "select a pane that carries a PromptSendProposed effect".into(),
                                                severity: Severity::Concern,
                                                source_kind: SourceKind::ProjectCanonical,
                                            },
                                        );
                                    }
                                    Some((target, cmd)) => {
                                        if accepting {
                                            match check_send_gate(
                                                ctx.config.actions.mode,
                                                ctx.config.actions.allow_auto_prompt_send,
                                            ) {
                                                PromptSendGate::Blocked => {
                                                    // Gemini v1.9.2 ADOPT: log operator intent
                                                    // in observe_only; distinguishes "tried and
                                                    // blocked" from "nothing happened".
                                                    ctx.sink.record(AuditEvent {
                                                        kind: AuditEventKind::PromptSendBlocked,
                                                        pane_id: target.clone(),
                                                        severity: Severity::Warning,
                                                        summary: format!(
                                                            "{target} {cmd} (blocked; observe_only mode)"
                                                        ),
                                                        provider: None,
                                                        role: None,
                                                    });
                                                    notices.insert(
                                                        0,
                                                        SystemNotice {
                                                            title: "accept blocked (observe_only)".into(),
                                                            body: format!(
                                                                "{target} → `{cmd}`: ObserveOnly mode blocks confirmation (PromptSendBlocked logged)"
                                                            ),
                                                            severity: Severity::Warning,
                                                            source_kind: SourceKind::ProjectCanonical,
                                                        },
                                                    );
                                                }
                                                PromptSendGate::AutoSendOff => {
                                                    // Operator confirmed but auto-send is off.
                                                    // Record acceptance (operator intent is real)
                                                    // AND a trailing `PromptSendBlocked` so the
                                                    // audit chain is complete — v1.10.1
                                                    // remediation closing Gemini v1.10.0 #3.
                                                    // No tmux send-keys invocation.
                                                    ctx.sink.record(AuditEvent {
                                                        kind: AuditEventKind::PromptSendAccepted,
                                                        pane_id: target.clone(),
                                                        severity: Severity::Warning,
                                                        summary: format!(
                                                            "{target} {cmd} (acknowledged by operator; auto-send disabled)"
                                                        ),
                                                        provider: None,
                                                        role: None,
                                                    });
                                                    ctx.sink.record(AuditEvent {
                                                        kind: AuditEventKind::PromptSendBlocked,
                                                        pane_id: target.clone(),
                                                        severity: Severity::Warning,
                                                        summary: format!(
                                                            "{target} {cmd} (execution blocked; allow_auto_prompt_send=false)"
                                                        ),
                                                        provider: None,
                                                        role: None,
                                                    });
                                                    notices.insert(
                                                        0,
                                                        SystemNotice {
                                                            title: "proposal accepted (send disabled)".into(),
                                                            body: format!(
                                                                "{target} → `{cmd}` (audit: PromptSendAccepted + PromptSendBlocked; set allow_auto_prompt_send=true to enable execution)"
                                                            ),
                                                            severity: Severity::Good,
                                                            source_kind: SourceKind::ProjectCanonical,
                                                        },
                                                    );
                                                }
                                                PromptSendGate::Execute => {
                                                    // Both gates passed. Record acceptance first,
                                                    // then attempt the tmux send-keys call.
                                                    ctx.sink.record(AuditEvent {
                                                        kind: AuditEventKind::PromptSendAccepted,
                                                        pane_id: target.clone(),
                                                        severity: Severity::Warning,
                                                        summary: format!(
                                                            "{target} {cmd} (acknowledged by operator; executing)"
                                                        ),
                                                        provider: None,
                                                        role: None,
                                                    });
                                                    match ctx.source.send_keys(&target, &cmd) {
                                                        Ok(()) => {
                                                            ctx.sink.record(AuditEvent {
                                                                kind: AuditEventKind::PromptSendCompleted,
                                                                pane_id: target.clone(),
                                                                severity: Severity::Safe,
                                                                summary: format!(
                                                                    "{target} {cmd} (sent; operator-confirmed)"
                                                                ),
                                                                provider: None,
                                                                role: None,
                                                            });
                                                            notices.insert(
                                                                0,
                                                                SystemNotice {
                                                                    title: "command sent".into(),
                                                                    body: format!(
                                                                        "{target} → `{cmd}` (tmux send-keys completed)"
                                                                    ),
                                                                    severity: Severity::Good,
                                                                    source_kind: SourceKind::ProjectCanonical,
                                                                },
                                                            );
                                                        }
                                                        Err(e) => {
                                                            ctx.sink.record(AuditEvent {
                                                                kind: AuditEventKind::PromptSendFailed,
                                                                pane_id: target.clone(),
                                                                severity: Severity::Warning,
                                                                summary: format!(
                                                                    "{target} {cmd} (send failed: {e})"
                                                                ),
                                                                provider: None,
                                                                role: None,
                                                            });
                                                            notices.insert(
                                                                0,
                                                                SystemNotice {
                                                                    title: "send failed".into(),
                                                                    body: format!(
                                                                        "{target} → `{cmd}`: tmux error — {e}"
                                                                    ),
                                                                    severity: Severity::Warning,
                                                                    source_kind: SourceKind::ProjectCanonical,
                                                                },
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            // 'd' dismiss — always available in any mode.
                                            ctx.sink.record(AuditEvent {
                                                kind: AuditEventKind::PromptSendRejected,
                                                pane_id: target.clone(),
                                                severity: Severity::Safe,
                                                summary: format!(
                                                    "{target} {cmd} (dismissed by operator)"
                                                ),
                                                provider: None,
                                                role: None,
                                            });
                                            notices.insert(
                                                0,
                                                SystemNotice {
                                                    title: "proposal dismissed".into(),
                                                    body: format!(
                                                        "{target} → `{cmd}` (PromptSendRejected logged)"
                                                    ),
                                                    severity: Severity::Safe,
                                                    source_kind: SourceKind::ProjectCanonical,
                                                },
                                            );
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
                                }
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
                            let rects = target_picker_rects(viewport);
                            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
                                && rect_contains(close_button_rect(rects.list), m.column, m.row)
                            {
                                target_picker_open = false;
                                continue;
                            }
                            match m.kind {
                                MouseEventKind::ScrollUp
                                    if rect_contains(rects.list, m.column, m.row) =>
                                {
                                    move_selection(
                                        &mut target_picker_state,
                                        target_choices.len(),
                                        -1,
                                    );
                                    refresh_target_preview(
                                        &ctx.source,
                                        &target_choices,
                                        &target_picker_state,
                                        &mut target_preview_title,
                                        &mut target_preview_lines,
                                    );
                                }
                                MouseEventKind::ScrollDown
                                    if rect_contains(rects.list, m.column, m.row) =>
                                {
                                    move_selection(
                                        &mut target_picker_state,
                                        target_choices.len(),
                                        1,
                                    );
                                    refresh_target_preview(
                                        &ctx.source,
                                        &target_choices,
                                        &target_picker_state,
                                        &mut target_preview_title,
                                        &mut target_preview_lines,
                                    );
                                }
                                MouseEventKind::Down(MouseButton::Left) => {
                                    if let Some(row) = list_row_at(rects.list, m)
                                        && let Some(idx) = target_choice_index_at_row(
                                            &target_choices,
                                            &target_picker_state,
                                            row,
                                        )
                                    {
                                        target_picker_state.select(Some(idx));
                                        refresh_target_preview(
                                            &ctx.source,
                                            &target_choices,
                                            &target_picker_state,
                                            &mut target_preview_title,
                                            &mut target_preview_lines,
                                        );
                                        match apply_target_choice(
                                            target_picker_stage,
                                            target_picker_session.as_deref(),
                                            &target_choices,
                                            &target_picker_state,
                                            &mut selected_target,
                                        ) {
                                            Some(TargetPickerOutcome::AdvanceToWindows(
                                                session_name,
                                            )) => {
                                                target_picker_stage = TargetPickerStage::Window;
                                                target_picker_session = Some(session_name);
                                                refresh_target_choices(
                                                    &ctx.source,
                                                    target_picker_stage,
                                                    target_picker_session.as_deref(),
                                                    &mut target_choices,
                                                    &mut target_picker_state,
                                                    selected_target.as_ref(),
                                                );
                                                refresh_target_preview(
                                                    &ctx.source,
                                                    &target_choices,
                                                    &target_picker_state,
                                                    &mut target_preview_title,
                                                    &mut target_preview_lines,
                                                );
                                            }
                                            Some(TargetPickerOutcome::Close(label)) => {
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
                                                        alert_hide_deadlines:
                                                            &mut alert_hide_deadlines,
                                                    },
                                                    now,
                                                );
                                                target_picker_open = false;
                                                last_poll = now - poll;
                                            }
                                            None => {}
                                        }
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }

                        let rects = dashboard_rects(viewport, dashboard_split);
                        match m.kind {
                            MouseEventKind::ScrollUp => {
                                if rect_contains(rects.alerts, m.column, m.row) {
                                    focus = FocusedPanel::Alerts;
                                    last_alert_click = None;
                                    move_selection(
                                        &mut alert_state,
                                        qmonster::ui::alerts::alert_count(
                                            &notices,
                                            &last_reports,
                                            &alert_hide_deadlines,
                                            now,
                                        ),
                                        -1,
                                    );
                                } else if rect_contains(rects.panes, m.column, m.row) {
                                    focus = FocusedPanel::Panes;
                                    move_selection(&mut pane_state, last_reports.len(), -1);
                                }
                            }
                            MouseEventKind::ScrollDown => {
                                if rect_contains(rects.alerts, m.column, m.row) {
                                    focus = FocusedPanel::Alerts;
                                    move_selection(
                                        &mut alert_state,
                                        qmonster::ui::alerts::alert_count(
                                            &notices,
                                            &last_reports,
                                            &alert_hide_deadlines,
                                            now,
                                        ),
                                        1,
                                    );
                                } else if rect_contains(rects.panes, m.column, m.row) {
                                    focus = FocusedPanel::Panes;
                                    last_alert_click = None;
                                    move_selection(&mut pane_state, last_reports.len(), 1);
                                }
                            }
                            MouseEventKind::Down(MouseButton::Left) => {
                                dashboard_split_dragging = false;
                                if rect_contains(rects.divider, m.column, m.row) {
                                    dashboard_split = dashboard_split_from_row(viewport, m.row);
                                    dashboard_split_dragging = true;
                                    last_alert_click = None;
                                } else if rect_contains(
                                    version_badge_rect(rects.footer),
                                    m.column,
                                    m.row,
                                ) {
                                    let panel = capture_repo_panel();
                                    git_modal.open(panel.title, panel.lines);
                                } else if let Some(row) = list_row_at(rects.alerts, m) {
                                    focus = FocusedPanel::Alerts;
                                    let alerts_inner = rects.alerts.inner(Margin {
                                        vertical: 1,
                                        horizontal: 1,
                                    });
                                    if row == 0 {
                                        last_alert_click = None;
                                        if let Some(severity) =
                                            qmonster::ui::alerts::bulk_hide_severity_at_column(
                                                qmonster::ui::alerts::AlertView {
                                                    notices: &notices,
                                                    reports: &last_reports,
                                                    fresh_alerts: &fresh_alerts,
                                                    alert_times: &alert_times,
                                                    hidden_until: &alert_hide_deadlines,
                                                    now,
                                                    target_label: &target,
                                                    focused: true,
                                                },
                                                m.column.saturating_sub(alerts_inner.x),
                                            )
                                        {
                                            toggle_alert_severity_hide(
                                                &mut alert_hide_deadlines,
                                                &notices,
                                                &last_reports,
                                                now,
                                                severity,
                                            );
                                            sync_alert_selection(
                                                &mut alert_state,
                                                &notices,
                                                &last_reports,
                                                &alert_hide_deadlines,
                                                now,
                                            );
                                        }
                                    } else if let Some(hit) = qmonster::ui::alerts::alert_hit_at_row(
                                        &alert_state,
                                        qmonster::ui::alerts::AlertView {
                                            notices: &notices,
                                            reports: &last_reports,
                                            fresh_alerts: &fresh_alerts,
                                            alert_times: &alert_times,
                                            hidden_until: &alert_hide_deadlines,
                                            now,
                                            target_label: &target,
                                            focused: true,
                                        },
                                        alerts_inner.width.saturating_sub(3) as usize,
                                        row.saturating_sub(1),
                                    ) {
                                        alert_state.select(Some(hit.index));
                                        if hit.dismiss {
                                            last_alert_click = None;
                                            toggle_selected_alert_hide(
                                                &mut alert_hide_deadlines,
                                                &alert_state,
                                                &notices,
                                                &last_reports,
                                                now,
                                            );
                                            sync_alert_selection(
                                                &mut alert_state,
                                                &notices,
                                                &last_reports,
                                                &alert_hide_deadlines,
                                                now,
                                            );
                                        } else if let Some(key) = alert_key_at_index(
                                            &notices,
                                            &last_reports,
                                            &alert_hide_deadlines,
                                            now,
                                            hit.index,
                                        ) && register_alert_double_click(
                                            &mut last_alert_click,
                                            &key,
                                            now,
                                        ) {
                                            toggle_selected_alert_hide(
                                                &mut alert_hide_deadlines,
                                                &alert_state,
                                                &notices,
                                                &last_reports,
                                                now,
                                            );
                                            sync_alert_selection(
                                                &mut alert_state,
                                                &notices,
                                                &last_reports,
                                                &alert_hide_deadlines,
                                                now,
                                            );
                                        }
                                    }
                                } else if let Some(row) = list_row_at(rects.panes, m) {
                                    focus = FocusedPanel::Panes;
                                    last_alert_click = None;
                                    if let Some(idx) = qmonster::ui::panels::pane_index_at_row(
                                        &last_reports,
                                        &pane_state,
                                        row,
                                    ) {
                                        pane_state.select(Some(idx));
                                    }
                                } else {
                                    last_alert_click = None;
                                }
                            }
                            MouseEventKind::Drag(MouseButton::Left) if dashboard_split_dragging => {
                                dashboard_split = dashboard_split_from_row(viewport, m.row);
                                last_alert_click = None;
                            }
                            MouseEventKind::Up(MouseButton::Left) if dashboard_split_dragging => {
                                dashboard_split = dashboard_split_from_row(viewport, m.row);
                                dashboard_split_dragging = false;
                                last_alert_click = None;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })();

    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    result
}

fn initial_target<P: PaneSource>(source: &P) -> Option<WindowTarget> {
    source
        .current_target()
        .ok()
        .flatten()
        .or_else(|| source.available_targets().ok()?.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    // P5-3 execution gate tests relocated to
    // `src/policy/gates.rs` in v1.10.1 remediation (Gemini v1.10.0
    // finding #1). See `check_send_gate_*` tests there.

    #[test]
    fn default_config_path_uses_env_root_before_cli_root() {
        let cli_root = PathBuf::from("/cli-qmonster");
        let path = default_config_path(Some(&cli_root), Some("/env-qmonster"));
        assert_eq!(path, PathBuf::from("/env-qmonster/config/qmonster.toml"));
    }

    #[test]
    fn default_config_path_uses_cli_root_when_env_absent() {
        let cli_root = PathBuf::from("/cli-qmonster");
        let path = default_config_path(Some(&cli_root), None);
        assert_eq!(path, PathBuf::from("/cli-qmonster/config/qmonster.toml"));
    }
}
