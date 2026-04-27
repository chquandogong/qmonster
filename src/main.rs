use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use chrono::Local;
use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use ratatui::widgets::ListState;

use qmonster::app::bootstrap::Context;
use qmonster::app::config::{ActionsMode, QmonsterConfig, load_from_path};
use qmonster::app::event_loop::{PaneReport, run_once, run_once_with_target};
use qmonster::app::git_info::capture_repo_panel;
use qmonster::app::path_resolution::pick_root;
use qmonster::app::safety_audit::apply_override_with_audit;
use qmonster::app::system_notice::{
    SystemNotice, record_startup_snapshot, route_polling_failure, route_polling_recovered,
    route_version_drift,
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
    ArchiveWriter, EventSink, InMemorySink, PaneSnapshot, QmonsterPaths, SnapshotInput,
    SnapshotWriter, SqliteAuditSink, sweep,
};
use qmonster::tmux::polling::{PaneSource, PollingSource};
use qmonster::tmux::types::{RawPaneSnapshot, WindowTarget};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusedPanel {
    Alerts,
    Panes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetPickerStage {
    Session,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetChoiceValue {
    AllSessions,
    Session(String),
    Window(WindowTarget),
}

#[derive(Debug, Clone)]
struct TargetChoice {
    label: String,
    value: TargetChoiceValue,
}

#[derive(Debug, Clone)]
struct AlertMouseClick {
    key: String,
    at: Instant,
}

const ALERT_DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(450);

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
    let mut git_modal_open = false;
    let mut git_modal_title = String::new();
    let mut git_modal_lines: Vec<String> = Vec::new();
    let mut git_scroll = 0usize;
    let mut help_open = false;
    let mut help_scroll = 0usize;
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
                            && !help_open
                            && !settings_overlay.is_open()
                            && focus == FocusedPanel::Alerts,
                        panes_focused: !target_picker_open
                            && !help_open
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
                if git_modal_open {
                    qmonster::ui::dashboard::render_git_modal(
                        frame,
                        &git_modal_title,
                        &git_modal_lines,
                        git_scroll as u16,
                    );
                }
                if help_open {
                    qmonster::ui::dashboard::render_help_modal(frame, help_scroll as u16);
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
                        if git_modal_open {
                            let size = terminal.size()?;
                            let max_scroll = qmonster::ui::dashboard::max_git_scroll(
                                Rect::new(0, 0, size.width, size.height),
                                git_modal_lines.len(),
                            );
                            match k.code {
                                KeyCode::Esc => {
                                    git_modal_open = false;
                                    git_scroll = 0;
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    git_scroll = git_scroll.saturating_sub(1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    git_scroll = git_scroll.saturating_add(1).min(max_scroll);
                                }
                                KeyCode::PageUp => {
                                    git_scroll = git_scroll.saturating_sub(8);
                                }
                                KeyCode::PageDown => {
                                    git_scroll = git_scroll.saturating_add(8).min(max_scroll);
                                }
                                KeyCode::Home => git_scroll = 0,
                                KeyCode::End => git_scroll = max_scroll,
                                _ => {}
                            }
                            continue;
                        }

                        if help_open {
                            let size = terminal.size()?;
                            let max_scroll = qmonster::ui::dashboard::max_help_scroll(Rect::new(
                                0,
                                0,
                                size.width,
                                size.height,
                            ));
                            match k.code {
                                KeyCode::Esc | KeyCode::Char('?') => {
                                    help_open = false;
                                    help_scroll = 0;
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    help_scroll = help_scroll.saturating_sub(1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    help_scroll = help_scroll.saturating_add(1).min(max_scroll);
                                }
                                KeyCode::PageUp => {
                                    help_scroll = help_scroll.saturating_sub(8);
                                }
                                KeyCode::PageDown => {
                                    help_scroll = help_scroll.saturating_add(8).min(max_scroll);
                                }
                                KeyCode::Home => help_scroll = 0,
                                KeyCode::End => help_scroll = max_scroll,
                                _ => {}
                            }
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
                            let editing = settings_overlay.edit_buffer().is_some();
                            match k.code {
                                KeyCode::Esc => {
                                    if editing {
                                        settings_overlay.cancel_edit();
                                    } else {
                                        settings_overlay.close();
                                    }
                                }
                                KeyCode::Char('q') if !editing => settings_overlay.close(),
                                KeyCode::Up if !editing => settings_overlay.prev_field(),
                                KeyCode::Down if !editing => settings_overlay.next_field(),
                                KeyCode::Left if !editing => settings_overlay.prev_field(),
                                KeyCode::Right if !editing => settings_overlay.next_field(),
                                KeyCode::Char('e') if !editing => {
                                    settings_overlay.start_edit(&ctx.config);
                                }
                                KeyCode::Char('c') if !editing => {
                                    settings_overlay.clear_override(&mut ctx.config);
                                }
                                KeyCode::Char('w') if !editing => {
                                    if let Some(path) = ctx.config_path.clone() {
                                        let _ = settings_overlay.save(&ctx.config, &path);
                                    } else {
                                        // No --config path was provided at startup;
                                        // there is nothing to write back. Surface the
                                        // explanation in the status banner so the
                                        // operator knows the precondition for 'w'.
                                        settings_overlay.set_save_error(
                                            "no config path — restart with `--config PATH` to enable save"
                                                .to_string(),
                                        );
                                    }
                                }
                                KeyCode::Enter => {
                                    if editing {
                                        let _ = settings_overlay.commit_edit(&mut ctx.config);
                                    } else {
                                        settings_overlay.start_edit(&ctx.config);
                                    }
                                }
                                KeyCode::Backspace if editing => settings_overlay.backspace(),
                                KeyCode::Char(c) if editing => settings_overlay.type_char(c),
                                _ => {}
                            }
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
                                help_open = true;
                                help_scroll = 0;
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
                                    route_version_drift(&versions, &fresh, &*ctx.sink);
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
                                let input = snapshot_input_from(&last_reports, &notices);
                                match snapshot_writer.write(&input) {
                                    Ok(path) => {
                                        ctx.sink.record(AuditEvent {
                                            kind: AuditEventKind::SnapshotWritten,
                                            pane_id: "n/a".into(),
                                            severity: Severity::Safe,
                                            summary: format!("snapshot → {}", path.display()),
                                            provider: None,
                                            role: None,
                                        });
                                        notices.insert(
                                            0,
                                            SystemNotice {
                                                title: "snapshot saved".into(),
                                                body: path.display().to_string(),
                                                severity: Severity::Good,
                                                source_kind: SourceKind::ProjectCanonical,
                                            },
                                        );
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
                                    Err(e) => {
                                        notices.insert(
                                            0,
                                            SystemNotice {
                                                title: "snapshot failed".into(),
                                                body: e.to_string(),
                                                severity: Severity::Warning,
                                                source_kind: SourceKind::ProjectCanonical,
                                            },
                                        );
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
                            // Click on the modal's [x] closes the overlay so
                            // mouse-only operators can dismiss it without
                            // reaching for `Esc`/`q`. Other mouse events are
                            // swallowed so a stray click underneath the modal
                            // does not race with the open overlay.
                            let rects = qmonster::ui::settings::settings_modal_rects(viewport);
                            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
                                && rect_contains(
                                    qmonster::ui::settings::settings_close_button_rect(rects.body),
                                    m.column,
                                    m.row,
                                )
                            {
                                settings_overlay.close();
                            }
                            continue;
                        }

                        if git_modal_open {
                            dashboard_split_dragging = false;
                            let rects = git_modal_rects(viewport);
                            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
                                && rect_contains(close_button_rect(rects.body), m.column, m.row)
                            {
                                git_modal_open = false;
                                git_scroll = 0;
                                continue;
                            }
                            if rect_contains(rects.body, m.column, m.row) {
                                let max_scroll = qmonster::ui::dashboard::max_git_scroll(
                                    viewport,
                                    git_modal_lines.len(),
                                );
                                match m.kind {
                                    MouseEventKind::ScrollUp => {
                                        git_scroll = git_scroll.saturating_sub(1);
                                    }
                                    MouseEventKind::ScrollDown => {
                                        git_scroll = git_scroll.saturating_add(1).min(max_scroll);
                                    }
                                    _ => {}
                                }
                            }
                            continue;
                        }

                        if help_open {
                            dashboard_split_dragging = false;
                            let rects = help_modal_rects(viewport);
                            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
                                && rect_contains(close_button_rect(rects.body), m.column, m.row)
                            {
                                help_open = false;
                                help_scroll = 0;
                                continue;
                            }
                            if rect_contains(rects.body, m.column, m.row) {
                                let max_scroll = qmonster::ui::dashboard::max_help_scroll(viewport);
                                match m.kind {
                                    MouseEventKind::ScrollUp => {
                                        help_scroll = help_scroll.saturating_sub(1);
                                    }
                                    MouseEventKind::ScrollDown => {
                                        help_scroll = help_scroll.saturating_add(1).min(max_scroll);
                                    }
                                    _ => {}
                                }
                            }
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
                                    git_modal_title = panel.title;
                                    git_modal_lines = panel.lines;
                                    git_scroll = 0;
                                    git_modal_open = true;
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

fn snapshot_input_from(reports: &[PaneReport], notices: &[SystemNotice]) -> SnapshotInput {
    SnapshotInput {
        reason: "operator-requested (key: s)".into(),
        pane_summaries: reports
            .iter()
            .map(|r| PaneSnapshot {
                pane_id: r.pane_id.clone(),
                provider: format!("{:?}", r.identity.identity.provider),
                role: format!("{:?}", r.identity.identity.role),
                alerts: r
                    .recommendations
                    .iter()
                    .map(|x| x.action.to_string())
                    .collect(),
            })
            .collect(),
        notices: notices
            .iter()
            .map(|n| format!("[{}] {}: {}", n.severity.letter(), n.title, n.body))
            .collect(),
    }
}

fn initial_target<P: PaneSource>(source: &P) -> Option<WindowTarget> {
    source
        .current_target()
        .ok()
        .flatten()
        .or_else(|| source.available_targets().ok()?.into_iter().next())
}

enum TargetPickerOutcome {
    AdvanceToWindows(String),
    Close(String),
}

fn refresh_target_choices<P: PaneSource>(
    source: &P,
    stage: TargetPickerStage,
    session_name: Option<&str>,
    choices: &mut Vec<TargetChoice>,
    state: &mut ListState,
    selected: Option<&WindowTarget>,
) {
    let targets = source.available_targets().unwrap_or_default();
    *choices = match stage {
        TargetPickerStage::Session => build_session_choices(&targets),
        TargetPickerStage::Window => {
            build_window_choices(&targets, session_name.unwrap_or_default())
        }
    };
    sync_target_choice_selection(state, stage, session_name, choices, selected);
}

fn refresh_target_preview<P: PaneSource>(
    source: &P,
    choices: &[TargetChoice],
    state: &ListState,
    preview_title: &mut String,
    preview_lines: &mut Vec<String>,
) {
    let Some(choice) = state.selected().and_then(|idx| choices.get(idx)) else {
        *preview_title = "Panes".into();
        preview_lines.clear();
        return;
    };

    match &choice.value {
        TargetChoiceValue::AllSessions => {
            *preview_title = "All Sessions".into();
            *preview_lines = vec![
                "all sessions".into(),
                "choose a specific session to inspect its windows and panes".into(),
            ];
        }
        TargetChoiceValue::Session(session_name) => {
            *preview_title = format!("Session · {session_name}");
            *preview_lines = build_session_preview(source, session_name);
        }
        TargetChoiceValue::Window(target) => {
            *preview_title = format!("Window · {}", target.label());
            *preview_lines = build_window_preview(source, target);
        }
    }
}

fn build_session_preview<P: PaneSource>(source: &P, session_name: &str) -> Vec<String> {
    let mut targets: Vec<WindowTarget> = source
        .available_targets()
        .unwrap_or_default()
        .into_iter()
        .filter(|target| target.session_name == session_name)
        .collect();
    targets.sort();
    if targets.is_empty() {
        return vec!["no windows in this session".into()];
    }

    let mut lines = Vec::new();
    for (idx, target) in targets.iter().enumerate() {
        if idx > 0 {
            lines.push(String::new());
        }
        let panes = source.list_panes(Some(target)).unwrap_or_default();
        push_window_tree(&mut lines, target, &panes);
    }
    lines
}

fn build_window_preview<P: PaneSource>(source: &P, target: &WindowTarget) -> Vec<String> {
    let panes = source.list_panes(Some(target)).unwrap_or_default();
    if panes.is_empty() {
        return vec!["no panes found in this window".into()];
    }
    let mut lines = Vec::new();
    push_window_tree(&mut lines, target, &panes);
    lines
}

fn push_window_tree(lines: &mut Vec<String>, target: &WindowTarget, panes: &[RawPaneSnapshot]) {
    let pane_count = panes.len();
    lines.push(format!(
        "window {} ({})",
        target.window_index,
        if pane_count == 1 {
            "1 pane".to_string()
        } else {
            format!("{pane_count} panes")
        }
    ));
    if panes.is_empty() {
        lines.push("└─ no panes found".into());
        return;
    }
    for (idx, pane) in panes.iter().enumerate() {
        let branch = if idx + 1 == panes.len() {
            "└─"
        } else {
            "├─"
        };
        lines.push(format!("{branch} {}", pane_preview_label(pane)));
    }
}

fn pane_preview_label(pane: &RawPaneSnapshot) -> String {
    let active = if pane.active { "*" } else { " " };
    let title = if pane.title.is_empty() {
        "untitled pane"
    } else {
        pane.title.as_str()
    };
    let mut label = format!("{active} {} · {}", pane.pane_id, title);
    if !pane.current_command.is_empty() && pane.current_command != pane.title {
        label.push_str(&format!(" :: {}", pane.current_command));
    }
    if pane.dead {
        label.push_str(" [dead]");
    }
    label
}

fn sync_target_choice_selection(
    state: &mut ListState,
    stage: TargetPickerStage,
    session_name: Option<&str>,
    choices: &[TargetChoice],
    selected: Option<&WindowTarget>,
) {
    if choices.is_empty() {
        state.select(None);
        return;
    }
    let selected_index = match stage {
        TargetPickerStage::Session => {
            let current_session = selected.map(|target| target.session_name.as_str());
            choices
                .iter()
                .position(|choice| match (&choice.value, current_session) {
                    (TargetChoiceValue::AllSessions, None) => true,
                    (TargetChoiceValue::Session(choice_session), Some(current)) => {
                        choice_session == current
                    }
                    _ => false,
                })
                .unwrap_or(0)
        }
        TargetPickerStage::Window => choices
            .iter()
            .position(|choice| match (&choice.value, selected, session_name) {
                (TargetChoiceValue::Window(choice_target), Some(current), Some(session)) => {
                    choice_target == current && current.session_name == session
                }
                _ => false,
            })
            .unwrap_or(0),
    };
    state.select(Some(selected_index));
}

fn apply_target_choice(
    stage: TargetPickerStage,
    session_name: Option<&str>,
    choices: &[TargetChoice],
    state: &ListState,
    selected_target: &mut Option<WindowTarget>,
) -> Option<TargetPickerOutcome> {
    let idx = state.selected()?;
    let choice = choices.get(idx)?;
    match (&choice.value, stage) {
        (TargetChoiceValue::AllSessions, TargetPickerStage::Session) => {
            *selected_target = None;
            Some(TargetPickerOutcome::Close("all sessions".into()))
        }
        (TargetChoiceValue::Session(session), TargetPickerStage::Session) => {
            Some(TargetPickerOutcome::AdvanceToWindows(session.clone()))
        }
        (TargetChoiceValue::Window(target), TargetPickerStage::Window) => {
            if let Some(session) = session_name
                && target.session_name != session
            {
                return None;
            }
            *selected_target = Some(target.clone());
            Some(TargetPickerOutcome::Close(target.label()))
        }
        _ => None,
    }
}

fn build_session_choices(targets: &[WindowTarget]) -> Vec<TargetChoice> {
    let mut sessions: Vec<String> = targets
        .iter()
        .map(|target| target.session_name.clone())
        .collect();
    sessions.sort();
    sessions.dedup();
    let mut choices = vec![TargetChoice {
        label: "all sessions · all windows".into(),
        value: TargetChoiceValue::AllSessions,
    }];
    for session in sessions {
        let mut session_targets: Vec<WindowTarget> = targets
            .iter()
            .filter(|target| target.session_name == session)
            .cloned()
            .collect();
        session_targets.sort();
        choices.push(TargetChoice {
            label: session_choice_label(&session, &session_targets),
            value: TargetChoiceValue::Session(session),
        });
    }
    choices
}

fn build_window_choices(targets: &[WindowTarget], session_name: &str) -> Vec<TargetChoice> {
    let mut session_targets: Vec<WindowTarget> = targets
        .iter()
        .filter(|target| target.session_name == session_name)
        .cloned()
        .collect();
    session_targets.sort();
    session_targets
        .into_iter()
        .map(|target| TargetChoice {
            label: window_choice_label(&target),
            value: TargetChoiceValue::Window(target),
        })
        .collect()
}

fn target_picker_title(stage: TargetPickerStage, session_name: Option<&str>) -> String {
    match (stage, session_name) {
        (TargetPickerStage::Session, _) => "Choose Session".into(),
        (TargetPickerStage::Window, Some(session)) => format!("Choose Window · {session}"),
        (TargetPickerStage::Window, None) => "Choose Window".into(),
    }
}

fn target_picker_hint(stage: TargetPickerStage) -> &'static str {
    match stage {
        TargetPickerStage::Session => {
            "click select · click [x] close · wheel scroll · ↑/↓ item · PgUp/PgDn page · Home/End · Enter open · Esc close"
        }
        TargetPickerStage::Window => {
            "click watch · click [x] close · wheel scroll · ↑/↓ item · PgUp/PgDn page · Home/End · Enter watch · ←/Backspace sessions · Esc close"
        }
    }
}

fn toggle_focus(focus: FocusedPanel) -> FocusedPanel {
    match focus {
        FocusedPanel::Alerts => FocusedPanel::Panes,
        FocusedPanel::Panes => FocusedPanel::Alerts,
    }
}

fn target_label(target: Option<&WindowTarget>) -> String {
    target
        .map(WindowTarget::label)
        .unwrap_or_else(|| "all sessions".into())
}

fn target_switched_notice(label: &str) -> SystemNotice {
    SystemNotice {
        title: "target switched".into(),
        body: format!("now watching {label}"),
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
    }
}

fn runtime_refresh_provider_label(provider: qmonster::domain::identity::Provider) -> &'static str {
    match provider {
        qmonster::domain::identity::Provider::Claude => "Claude",
        qmonster::domain::identity::Provider::Codex => "Codex",
        qmonster::domain::identity::Provider::Gemini => "Gemini",
        qmonster::domain::identity::Provider::Qmonster => "Qmonster",
        qmonster::domain::identity::Provider::Unknown => "Unknown provider",
    }
}

fn runtime_refresh_commands(
    provider: qmonster::domain::identity::Provider,
    _idle_state: Option<IdleCause>,
) -> &'static [&'static str] {
    runtime_refresh_provider_commands(provider)
}

fn runtime_refresh_uses_active_safe_only(idle_state: Option<IdleCause>) -> bool {
    matches!(idle_state, None | Some(IdleCause::Stale))
}

fn runtime_refresh_dispatch_commands(
    provider: qmonster::domain::identity::Provider,
    idle_state: Option<IdleCause>,
    pane_id: &str,
    offsets: &mut HashMap<String, usize>,
) -> Vec<&'static str> {
    let commands = runtime_refresh_commands(provider, idle_state);
    if commands.is_empty() {
        return Vec::new();
    }
    if runtime_refresh_sends_one_command_at_a_time(provider, idle_state) {
        let key = format!(
            "{pane_id}:{}-runtime",
            runtime_refresh_provider_key(provider)
        );
        let idx = offsets.entry(key).or_insert(0);
        let command = commands[*idx % commands.len()];
        *idx = (*idx + 1) % commands.len();
        return vec![command];
    }
    commands.to_vec()
}

fn runtime_refresh_sends_one_command_at_a_time(
    provider: qmonster::domain::identity::Provider,
    _idle_state: Option<IdleCause>,
) -> bool {
    matches!(
        provider,
        qmonster::domain::identity::Provider::Claude | qmonster::domain::identity::Provider::Gemini
    )
}

fn runtime_refresh_sends_escape_first(
    provider: qmonster::domain::identity::Provider,
    _idle_state: Option<IdleCause>,
) -> bool {
    matches!(provider, qmonster::domain::identity::Provider::Claude)
}

#[derive(Debug, Default, PartialEq, Eq)]
struct RuntimeRefreshSendOutcome {
    failed: Option<(String, String)>,
    captured_and_closed: bool,
}

fn send_runtime_refresh_commands<P: PaneSource>(
    source: &P,
    pane_id: &str,
    provider: qmonster::domain::identity::Provider,
    idle_state: Option<IdleCause>,
    commands: &[&str],
    capture_lines: usize,
    tail_overlays: &mut HashMap<String, String>,
) -> RuntimeRefreshSendOutcome {
    let mut outcome = RuntimeRefreshSendOutcome::default();
    if runtime_refresh_sends_escape_first(provider, idle_state)
        && let Err(e) = source.send_key(pane_id, "Escape")
    {
        outcome.failed = Some(("Escape".into(), e.to_string()));
        return outcome;
    }

    for cmd in commands {
        if let Err(e) = source.send_keys(pane_id, cmd) {
            outcome.failed = Some(((*cmd).to_string(), e.to_string()));
            break;
        }
        if runtime_refresh_captures_then_closes(provider, cmd) {
            match source.capture_tail(pane_id, capture_lines) {
                Ok(tail) => {
                    if !tail.trim().is_empty() {
                        tail_overlays.insert(pane_id.to_string(), tail);
                    }
                }
                Err(e) => {
                    outcome.failed = Some((format!("{cmd} capture"), e.to_string()));
                }
            }
            if let Err(e) = source.send_key(pane_id, "Escape") {
                if outcome.failed.is_none() {
                    outcome.failed = Some(("Escape".into(), e.to_string()));
                }
            } else if outcome.failed.is_none() {
                outcome.captured_and_closed = true;
            }
            if outcome.failed.is_some() {
                break;
            }
        }
    }

    outcome
}

fn runtime_refresh_captures_then_closes(
    provider: qmonster::domain::identity::Provider,
    command: &str,
) -> bool {
    matches!(provider, qmonster::domain::identity::Provider::Claude) && command == "/status"
}

fn runtime_refresh_provider_commands(
    provider: qmonster::domain::identity::Provider,
) -> &'static [&'static str] {
    // Keep this list to provider-owned control/status surfaces.
    match provider {
        qmonster::domain::identity::Provider::Claude => &["/status", "/usage", "/stats"],
        qmonster::domain::identity::Provider::Codex => &["/status"],
        qmonster::domain::identity::Provider::Gemini => {
            &["/stats session", "/stats model", "/stats tools"]
        }
        qmonster::domain::identity::Provider::Qmonster
        | qmonster::domain::identity::Provider::Unknown => &[],
    }
}

fn runtime_refresh_provider_key(provider: qmonster::domain::identity::Provider) -> &'static str {
    match provider {
        qmonster::domain::identity::Provider::Claude => "claude",
        qmonster::domain::identity::Provider::Codex => "codex",
        qmonster::domain::identity::Provider::Gemini => "gemini",
        qmonster::domain::identity::Provider::Qmonster => "qmonster",
        qmonster::domain::identity::Provider::Unknown => "unknown",
    }
}

fn runtime_refresh_command_label(commands: &[&str]) -> String {
    commands.join(", ")
}

fn runtime_refresh_request_label(active_only: bool, one_at_a_time: bool) -> &'static str {
    if one_at_a_time {
        "operator-requested cycled runtime refresh"
    } else if active_only {
        "operator-requested active-safe runtime refresh"
    } else {
        "operator-requested full runtime refresh"
    }
}

fn runtime_refresh_completion_label(captured_and_closed: bool) -> &'static str {
    if captured_and_closed {
        "sent with terminal submit; captured then closed with Escape"
    } else {
        "sent with terminal submit"
    }
}

fn runtime_refresh_notice_body(
    pane_id: &str,
    command_label: &str,
    active_only: bool,
    one_at_a_time: bool,
    captured_and_closed: bool,
) -> String {
    if captured_and_closed && one_at_a_time {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; Claude `/status` was captured, then Escape closed the fullscreen surface so the next `u` can run immediately"
        )
    } else if captured_and_closed {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; Claude `/status` was captured, then Escape closed the fullscreen surface and the next poll will parse the captured output"
        )
    } else if one_at_a_time {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; provider runtime sources are sent one at a time, so press `u` again to cycle the next source"
        )
    } else if active_only {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; active or uncertain pane uses only provider commands verified to run without waiting, and the next poll will parse provider output"
        )
    } else {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; full provider runtime refresh requested, and the next poll will parse provider output"
        )
    }
}

fn sync_pane_selection(state: &mut ListState, pane_count: usize) {
    match pane_count {
        0 => state.select(None),
        count => {
            let selected = state.selected().unwrap_or(0).min(count.saturating_sub(1));
            state.select(Some(selected));
        }
    }
}

fn move_selection(state: &mut ListState, pane_count: usize, step: isize) {
    if pane_count == 0 {
        state.select(None);
        return;
    }
    let current = state.selected().unwrap_or(0) as isize;
    let next = (current + step).clamp(0, pane_count.saturating_sub(1) as isize) as usize;
    state.select(Some(next));
}

#[derive(Debug, Clone, Copy)]
enum ScrollDir {
    Up,
    Down,
}

struct DashboardSyncState<'a> {
    alert_state: &'a mut ListState,
    pane_state: &'a mut ListState,
    previous_alerts: &'a mut HashSet<String>,
    fresh_alerts: &'a mut HashSet<String>,
    alert_times: &'a mut HashMap<String, String>,
    alert_hide_deadlines: &'a mut HashMap<String, Instant>,
}

fn page_selection(state: &mut ListState, total: usize, page: usize, dir: ScrollDir) {
    if total == 0 {
        state.select(None);
        return;
    }
    let step = page.max(1) as isize;
    match dir {
        ScrollDir::Up => move_selection(state, total, -step),
        ScrollDir::Down => move_selection(state, total, step),
    }
}

fn select_first(state: &mut ListState, total: usize) {
    if total == 0 {
        state.select(None);
        return;
    }
    state.select(Some(0));
}

fn select_last(state: &mut ListState, total: usize) {
    if total == 0 {
        state.select(None);
        return;
    }
    state.select(Some(total.saturating_sub(1)));
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn list_row_at(rect: Rect, event: MouseEvent) -> Option<u16> {
    let inner = rect.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    rect_contains(inner, event.column, event.row).then_some(event.row.saturating_sub(inner.y))
}

fn target_choice_index_at_row(
    choices: &[TargetChoice],
    state: &ListState,
    row: u16,
) -> Option<usize> {
    let mut remaining = row;
    for (idx, choice) in choices.iter().enumerate().skip(state.offset()) {
        let height = choice.label.lines().count().max(1) as u16;
        if remaining < height {
            return Some(idx);
        }
        remaining = remaining.saturating_sub(height);
    }
    None
}

fn sync_alert_selection(
    state: &mut ListState,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    alert_hide_deadlines: &HashMap<String, Instant>,
    now: Instant,
) {
    let count = qmonster::ui::alerts::alert_count(notices, reports, alert_hide_deadlines, now);
    match count {
        0 => state.select(None),
        total => {
            let selected = state.selected().unwrap_or(0).min(total.saturating_sub(1));
            state.select(Some(selected));
        }
    }
}

fn toggle_selected_alert_hide(
    alert_hide_deadlines: &mut HashMap<String, Instant>,
    alert_state: &ListState,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    now: Instant,
) {
    let Some(selected_idx) = alert_state.selected() else {
        return;
    };
    let keys =
        qmonster::ui::alerts::visible_alert_keys(notices, reports, alert_hide_deadlines, now);
    let Some(key) = keys.get(selected_idx) else {
        return;
    };
    if alert_hide_deadlines.remove(key).is_none() {
        alert_hide_deadlines.insert(
            key.clone(),
            now + qmonster::ui::alerts::ALERT_AUTO_HIDE_DELAY,
        );
    }
}

fn toggle_alert_severity_hide(
    alert_hide_deadlines: &mut HashMap<String, Instant>,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    now: Instant,
    severity: Severity,
) {
    let keys = qmonster::ui::alerts::actionable_alert_keys_for_severity(
        notices,
        reports,
        alert_hide_deadlines,
        now,
        severity,
    );
    if keys.is_empty() {
        return;
    }
    let all_pending = keys.iter().all(|key| {
        alert_hide_deadlines
            .get(key)
            .is_some_and(|deadline| *deadline > now)
    });
    if all_pending {
        for key in keys {
            alert_hide_deadlines.remove(&key);
        }
        return;
    }
    for key in keys {
        alert_hide_deadlines.insert(key, now + qmonster::ui::alerts::ALERT_AUTO_HIDE_DELAY);
    }
}

fn alert_key_at_index(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    alert_hide_deadlines: &HashMap<String, Instant>,
    now: Instant,
    idx: usize,
) -> Option<String> {
    qmonster::ui::alerts::visible_alert_keys(notices, reports, alert_hide_deadlines, now)
        .get(idx)
        .cloned()
}

fn register_alert_double_click(
    last_click: &mut Option<AlertMouseClick>,
    key: &str,
    now: Instant,
) -> bool {
    if last_click.as_ref().is_some_and(|previous| {
        previous.key == key
            && now.saturating_duration_since(previous.at) <= ALERT_DOUBLE_CLICK_WINDOW
    }) {
        *last_click = None;
        return true;
    }

    *last_click = Some(AlertMouseClick {
        key: key.to_string(),
        at: now,
    });
    false
}

fn sync_dashboard_state(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    state: DashboardSyncState<'_>,
    now: Instant,
) {
    sync_pane_selection(state.pane_state, reports.len());
    refresh_alert_state(
        notices,
        reports,
        state.previous_alerts,
        state.fresh_alerts,
        state.alert_times,
        state.alert_hide_deadlines,
    );
    sync_alert_selection(
        state.alert_state,
        notices,
        reports,
        state.alert_hide_deadlines,
        now,
    );
}

fn update_pane_state_flashes(
    reports: &[PaneReport],
    last_states: &mut HashMap<String, Option<IdleCause>>,
    flashes: &mut HashMap<String, qmonster::ui::panels::PaneStateFlash>,
    now: Instant,
) {
    let current_panes: HashSet<&str> = reports
        .iter()
        .map(|report| report.pane_id.as_str())
        .collect();
    last_states.retain(|pane_id, _| current_panes.contains(pane_id.as_str()));
    flashes
        .retain(|pane_id, flash| current_panes.contains(pane_id.as_str()) && flash.is_active(now));

    for report in reports {
        let current = report.idle_state;
        match last_states.insert(report.pane_id.clone(), current) {
            Some(previous) if previous != current => {
                flashes.insert(
                    report.pane_id.clone(),
                    qmonster::ui::panels::PaneStateFlash::new(current, now),
                );
            }
            _ => {}
        }
    }
}

fn refresh_alert_state(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    previous_alerts: &mut HashSet<String>,
    fresh_alerts: &mut HashSet<String>,
    alert_times: &mut HashMap<String, String>,
    alert_hide_deadlines: &mut HashMap<String, Instant>,
) {
    let current = qmonster::ui::alerts::alert_fingerprints(notices, reports);
    let timestamp = Local::now().format("%H:%M:%S").to_string();
    let disappeared: Vec<String> = previous_alerts.difference(&current).cloned().collect();
    for key in disappeared {
        alert_times.remove(&key);
    }
    alert_hide_deadlines.retain(|key, _| current.contains(key));

    *fresh_alerts = current.difference(previous_alerts).cloned().collect();
    for key in fresh_alerts.iter() {
        alert_times.insert(key.clone(), timestamp.clone());
    }
    *previous_alerts = current;
}

fn session_choice_label(session_name: &str, targets: &[WindowTarget]) -> String {
    let mut lines = vec![format!(
        "{session_name} ({})",
        if targets.len() == 1 {
            "1 window".to_string()
        } else {
            format!("{} windows", targets.len())
        }
    )];
    for (idx, target) in targets.iter().enumerate() {
        let branch = if idx + 1 == targets.len() {
            "└─"
        } else {
            "├─"
        };
        lines.push(format!("{branch} window {}", target.window_index));
    }
    lines.join("\n")
}

fn window_choice_label(target: &WindowTarget) -> String {
    format!("{} · window {}", target.session_name, target.window_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use qmonster::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use qmonster::domain::recommendation::Recommendation;
    use qmonster::domain::signal::{IdleCause, SignalSet};
    use std::cell::RefCell;

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

    #[derive(Default)]
    struct RecordingRefreshSource {
        calls: RefCell<Vec<String>>,
        capture: String,
    }

    impl RecordingRefreshSource {
        fn with_capture(capture: &str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                capture: capture.into(),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl PaneSource for RecordingRefreshSource {
        fn list_panes(
            &self,
            _target: Option<&WindowTarget>,
        ) -> Result<Vec<RawPaneSnapshot>, qmonster::tmux::polling::PollingError> {
            Ok(Vec::new())
        }

        fn current_target(
            &self,
        ) -> Result<Option<WindowTarget>, qmonster::tmux::polling::PollingError> {
            Ok(None)
        }

        fn available_targets(
            &self,
        ) -> Result<Vec<WindowTarget>, qmonster::tmux::polling::PollingError> {
            Ok(Vec::new())
        }

        fn capture_tail(
            &self,
            _pane_id: &str,
            lines: usize,
        ) -> Result<String, qmonster::tmux::polling::PollingError> {
            self.calls.borrow_mut().push(format!("capture:{lines}"));
            Ok(self.capture.clone())
        }

        fn send_keys(
            &self,
            _pane_id: &str,
            text: &str,
        ) -> Result<(), qmonster::tmux::polling::PollingError> {
            self.calls.borrow_mut().push(format!("send:{text}"));
            Ok(())
        }

        fn send_key(
            &self,
            _pane_id: &str,
            key: &str,
        ) -> Result<(), qmonster::tmux::polling::PollingError> {
            self.calls.borrow_mut().push(format!("key:{key}"));
            Ok(())
        }
    }

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
            effects: vec![],
            dead: false,
            recommendations: recs,
            current_path: "/repo".into(),
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
        }
    }

    #[test]
    fn update_pane_state_flashes_tracks_idle_and_active_transitions() {
        let now = Instant::now();
        let mut last_states = HashMap::new();
        let mut flashes = HashMap::new();
        let mut report = base_report(vec![]);

        update_pane_state_flashes(&[report.clone()], &mut last_states, &mut flashes, now);
        assert!(
            flashes.is_empty(),
            "initial observation should establish baseline without flashing"
        );

        report.idle_state = Some(IdleCause::InputWait);
        update_pane_state_flashes(
            &[report.clone()],
            &mut last_states,
            &mut flashes,
            now + std::time::Duration::from_millis(10),
        );
        assert_eq!(
            flashes.get("%1").map(|flash| flash.state),
            Some(Some(IdleCause::InputWait))
        );

        report.idle_state = None;
        update_pane_state_flashes(
            &[report],
            &mut last_states,
            &mut flashes,
            now + std::time::Duration::from_millis(20),
        );
        assert_eq!(flashes.get("%1").map(|flash| flash.state), Some(None));
    }

    #[test]
    fn list_row_at_ignores_block_border_and_returns_body_row() {
        let rect = Rect::new(10, 4, 30, 8);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 6,
            modifiers: KeyModifiers::empty(),
        };
        assert_eq!(list_row_at(rect, event), Some(1));
    }

    #[test]
    fn target_choice_index_accounts_for_multiline_tree_items() {
        let choices = vec![
            TargetChoice {
                label: "session-a\n├─ window 0\n└─ pane %1".into(),
                value: TargetChoiceValue::Session("session-a".into()),
            },
            TargetChoice {
                label: "session-b\n└─ window 1".into(),
                value: TargetChoiceValue::Session("session-b".into()),
            },
        ];
        let mut state = ListState::default();
        state.select(Some(0));

        assert_eq!(target_choice_index_at_row(&choices, &state, 0), Some(0));
        assert_eq!(target_choice_index_at_row(&choices, &state, 2), Some(0));
        assert_eq!(target_choice_index_at_row(&choices, &state, 3), Some(1));
    }

    #[test]
    fn rect_contains_excludes_coordinates_on_outer_edge() {
        let rect = Rect::new(2, 3, 4, 5);
        assert!(rect_contains(rect, 2, 3));
        assert!(rect_contains(rect, 5, 7));
        assert!(!rect_contains(rect, 6, 7));
        assert!(!rect_contains(rect, 5, 8));
    }

    #[test]
    fn runtime_refresh_commands_for_claude_cycle_status_usage_stats_when_idle() {
        assert_eq!(
            runtime_refresh_commands(Provider::Claude, Some(IdleCause::WorkComplete)),
            ["/status", "/usage", "/stats"]
        );
        assert_eq!(
            runtime_refresh_command_label(runtime_refresh_commands(
                Provider::Claude,
                Some(IdleCause::LimitHit)
            )),
            "/status, /usage, /stats"
        );
    }

    #[test]
    fn runtime_refresh_commands_for_claude_active_cycle_same_runtime_sources() {
        assert_eq!(
            runtime_refresh_commands(Provider::Claude, None),
            ["/status", "/usage", "/stats"]
        );
        assert_eq!(
            runtime_refresh_commands(Provider::Claude, Some(IdleCause::Stale)),
            ["/status", "/usage", "/stats"]
        );
    }

    #[test]
    fn runtime_refresh_commands_for_codex_use_status_slash_active_or_idle() {
        // `PaneSource::send_keys` appends Enter after the literal slash
        // command, so Codex status UI executes instead of just leaving
        // the slash command typed in the prompt.
        assert_eq!(runtime_refresh_commands(Provider::Codex, None), ["/status"]);
        assert_eq!(
            runtime_refresh_commands(Provider::Codex, Some(IdleCause::WorkComplete)),
            ["/status"]
        );
    }

    #[test]
    fn runtime_refresh_commands_for_gemini_use_stats_slashes_active_or_idle() {
        assert_eq!(
            runtime_refresh_commands(Provider::Gemini, None),
            ["/stats session", "/stats model", "/stats tools"]
        );
        assert_eq!(
            runtime_refresh_commands(Provider::Gemini, Some(IdleCause::WorkComplete)),
            ["/stats session", "/stats model", "/stats tools"]
        );
    }

    #[test]
    fn runtime_refresh_dispatch_cycles_claude_runtime_sources_one_at_a_time() {
        let mut offsets = HashMap::new();
        assert_eq!(
            runtime_refresh_dispatch_commands(
                Provider::Claude,
                Some(IdleCause::WorkComplete),
                "%1",
                &mut offsets,
            ),
            vec!["/status"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(
                Provider::Claude,
                Some(IdleCause::WorkComplete),
                "%1",
                &mut offsets,
            ),
            vec!["/usage"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(
                Provider::Claude,
                Some(IdleCause::WorkComplete),
                "%1",
                &mut offsets,
            ),
            vec!["/stats"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(
                Provider::Claude,
                Some(IdleCause::WorkComplete),
                "%1",
                &mut offsets,
            ),
            vec!["/status"]
        );
        assert!(runtime_refresh_sends_escape_first(
            Provider::Claude,
            Some(IdleCause::WorkComplete)
        ));
    }

    #[test]
    fn runtime_refresh_claude_status_captures_then_closes_surface() {
        let source = RecordingRefreshSource::with_capture("Permission mode: bypass permissions on");
        let mut overlays = HashMap::new();
        let outcome = send_runtime_refresh_commands(
            &source,
            "%1",
            Provider::Claude,
            Some(IdleCause::WorkComplete),
            &["/status"],
            24,
            &mut overlays,
        );

        assert_eq!(
            source.calls(),
            vec!["key:Escape", "send:/status", "capture:24", "key:Escape"]
        );
        assert_eq!(
            overlays.get("%1").map(String::as_str),
            Some("Permission mode: bypass permissions on")
        );
        assert!(outcome.captured_and_closed);
        assert_eq!(outcome.failed, None);
    }

    #[test]
    fn runtime_refresh_dispatch_cycles_gemini_stats_sources_one_at_a_time() {
        let mut offsets = HashMap::new();
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Gemini, None, "%2", &mut offsets),
            vec!["/stats session"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Gemini, None, "%2", &mut offsets),
            vec!["/stats model"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Gemini, None, "%2", &mut offsets),
            vec!["/stats tools"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Gemini, None, "%2", &mut offsets),
            vec!["/stats session"]
        );
        assert!(!runtime_refresh_sends_escape_first(Provider::Gemini, None));
    }

    #[test]
    fn register_alert_double_click_requires_same_key_within_window() {
        let now = Instant::now();
        let mut tracker = None;

        assert!(!register_alert_double_click(&mut tracker, "a", now));
        assert!(register_alert_double_click(
            &mut tracker,
            "a",
            now + Duration::from_millis(200)
        ));
        assert!(tracker.is_none());
    }

    #[test]
    fn register_alert_double_click_ignores_stale_or_different_clicks() {
        let now = Instant::now();
        let mut tracker = None;

        assert!(!register_alert_double_click(&mut tracker, "a", now));
        assert!(!register_alert_double_click(
            &mut tracker,
            "b",
            now + Duration::from_millis(200)
        ));
        assert!(!register_alert_double_click(
            &mut tracker,
            "b",
            now + Duration::from_millis(700)
        ));
    }

    #[test]
    fn toggle_alert_severity_hide_only_targets_actionable_alerts() {
        let notice = SystemNotice {
            title: "snapshot saved".into(),
            body: "/tmp/x".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
        };
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "waiting".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let reports = vec![base_report(vec![rec.clone()])];
        let now = Instant::now();
        let mut hidden = HashMap::new();

        toggle_alert_severity_hide(&mut hidden, &[notice], &reports, now, Severity::Good);

        let actionable = qmonster::ui::alerts::actionable_alert_keys_for_severity(
            &[],
            &reports,
            &HashMap::new(),
            now,
            Severity::Good,
        );
        assert_eq!(hidden.len(), actionable.len());
        assert!(actionable.iter().all(|key| hidden.contains_key(key)));
    }
}
