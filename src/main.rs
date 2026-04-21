use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use chrono::Local;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use ratatui::widgets::ListState;

use qmonster::app::bootstrap::Context;
use qmonster::app::config::{QmonsterConfig, load_from_path};
use qmonster::app::event_loop::{PaneReport, run_once, run_once_with_target};
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
use qmonster::notify::desktop::DesktopNotifier;
use qmonster::store::{
    ArchiveWriter, EventSink, InMemorySink, PaneSnapshot, SnapshotInput, SnapshotWriter,
    SqliteAuditSink, sweep,
};
use qmonster::tmux::polling::{PaneSource, PollingSource};
use qmonster::tmux::types::WindowTarget;
use qmonster::ui::dashboard::{DashboardView, render_dashboard};

#[derive(Debug, Parser)]
#[command(
    name = "qmonster",
    about = "Observe-first TUI for multi-CLI tmux work"
)]
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

    let config = match cli.config.as_ref() {
        Some(path) => load_from_path(path).with_context(|| format!("loading {path:?}"))?,
        None => QmonsterConfig::defaults(),
    };
    let mut pairs: Vec<(String, String)> = Vec::new();
    for kv in &cli.set {
        let Some((k, v)) = kv.split_once('=') else {
            anyhow::bail!("--set expects key=value, got {kv}");
        };
        pairs.push((k.trim().into(), v.trim().into()));
    }

    let env_root = std::env::var("QMONSTER_ROOT").ok();
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

    let source = PollingSource::new();
    let notifier = DesktopNotifier;
    let archive = ArchiveWriter::new(paths.clone(), config.logging.big_output_chars);
    let mut ctx = Context::new(config, source, notifier, sink).with_archive(archive);

    if !pairs.is_empty() {
        let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
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
    if may_save_fresh
        && let Err(e) = fresh.save_to(&paths.versions_path())
    {
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
        print_reports(&reports);
        return Ok(());
    }

    run_tui(&mut ctx, fresh, snapshot_writer, startup_notices)
}

fn print_reports(reports: &[PaneReport]) {
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
            println!("{}", qmonster::ui::alerts::format_strong_rec_body(rec, &rep.pane_id));
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
        let chips = qmonster::ui::panels::signal_chips(&r.signals);
        if !chips.is_empty() {
            println!("  state: {}", chips.join(" | "));
        }
        let metrics = qmonster::ui::panels::metric_row(&r.signals);
        if !metrics.is_empty() {
            println!("  metrics: {metrics}");
        }
        if !r.effects.is_empty() {
            let names: Vec<String> =
                r.effects.iter().map(|e| format!("{e:?}")).collect();
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

#[derive(Debug, Clone)]
struct TargetChoice {
    label: String,
    target: Option<WindowTarget>,
}

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
    execute!(stdout, EnterAlternateScreen).context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let poll = ctx.config.tmux.poll_interval();
    let mut last_reports: Vec<PaneReport> = Vec::new();
    let mut notices: Vec<SystemNotice> = startup_notices;
    let mut last_poll = Instant::now() - poll;
    let mut last_poll_error: Option<String> = None;
    let mut selected_target = initial_target(&ctx.source);
    let mut focus = FocusedPanel::Alerts;
    let mut alert_state = ListState::default();
    let mut pane_state = ListState::default();
    let mut target_picker_open = false;
    let mut target_picker_state = ListState::default();
    let mut target_choices: Vec<TargetChoice> = Vec::new();
    let mut help_open = false;
    let mut previous_alerts: HashSet<String> = HashSet::new();
    let mut fresh_alerts: HashSet<String> = HashSet::new();
    let mut alert_times: HashMap<String, String> = HashMap::new();
    refresh_alert_state(
        &notices,
        &last_reports,
        &mut previous_alerts,
        &mut fresh_alerts,
        &mut alert_times,
    );
    sync_alert_selection(&mut alert_state, &notices, &last_reports);
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
                        last_reports = reports;
                        sync_dashboard_state(
                            &notices,
                            &last_reports,
                            &mut alert_state,
                            &mut pane_state,
                            &mut previous_alerts,
                            &mut fresh_alerts,
                            &mut alert_times,
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
                                &mut alert_state,
                                &mut pane_state,
                                &mut previous_alerts,
                                &mut fresh_alerts,
                                &mut alert_times,
                            );
                        }
                    }
                }
            }

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
                        target_label: &target,
                        alerts_focused: !target_picker_open
                            && !help_open
                            && focus == FocusedPanel::Alerts,
                        panes_focused: !target_picker_open
                            && !help_open
                            && focus == FocusedPanel::Panes,
                    },
                );
                if target_picker_open {
                    let labels: Vec<String> =
                        target_choices.iter().map(|choice| choice.label.clone()).collect();
                    qmonster::ui::dashboard::render_target_picker(
                        frame,
                        &labels,
                        &mut target_picker_state,
                        &target,
                    );
                }
                if help_open {
                    qmonster::ui::dashboard::render_help_modal(frame);
                }
            })?;

            if event::poll(Duration::from_millis(100))?
                && let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                if help_open {
                    match k.code {
                        KeyCode::Esc | KeyCode::Char('?') => help_open = false,
                        _ => {}
                    }
                    continue;
                }

                if target_picker_open {
                    match k.code {
                        KeyCode::Esc | KeyCode::Char('t') => target_picker_open = false,
                        KeyCode::Up | KeyCode::Char('k') => {
                            move_selection(&mut target_picker_state, target_choices.len(), -1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            move_selection(&mut target_picker_state, target_choices.len(), 1);
                        }
                        KeyCode::Enter => {
                            if let Some(label) = apply_target_choice(
                                &target_choices,
                                &target_picker_state,
                                &mut selected_target,
                            ) {
                                notices.insert(0, target_switched_notice(&label));
                                sync_dashboard_state(
                                    &notices,
                                    &last_reports,
                                    &mut alert_state,
                                    &mut pane_state,
                                    &mut previous_alerts,
                                    &mut fresh_alerts,
                                    &mut alert_times,
                                );
                                target_picker_open = false;
                                last_poll = Instant::now() - poll;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Tab => focus = toggle_focus(focus),
                    KeyCode::Char('?') => help_open = true,
                    KeyCode::Up | KeyCode::Char('k') => match focus {
                        FocusedPanel::Alerts => move_selection(
                            &mut alert_state,
                            qmonster::ui::alerts::alert_count(&notices, &last_reports),
                            -1,
                        ),
                        FocusedPanel::Panes => {
                            move_selection(&mut pane_state, last_reports.len(), -1);
                        }
                    },
                    KeyCode::Down | KeyCode::Char('j') => match focus {
                        FocusedPanel::Alerts => move_selection(
                            &mut alert_state,
                            qmonster::ui::alerts::alert_count(&notices, &last_reports),
                            1,
                        ),
                        FocusedPanel::Panes => {
                            move_selection(&mut pane_state, last_reports.len(), 1);
                        }
                    },
                    KeyCode::Char('t') => {
                        refresh_target_choices(
                            &ctx.source,
                            &mut target_choices,
                            &mut target_picker_state,
                            selected_target.as_ref(),
                        );
                        target_picker_open = true;
                    }
                    KeyCode::Char('r') => {
                        let fresh = capture_versions();
                        let new_notices = route_version_drift(&versions, &fresh, &*ctx.sink);
                        if !new_notices.is_empty() {
                            notices = new_notices;
                            sync_dashboard_state(
                                &notices,
                                &last_reports,
                                &mut alert_state,
                                &mut pane_state,
                                &mut previous_alerts,
                                &mut fresh_alerts,
                                &mut alert_times,
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
                                    &mut alert_state,
                                    &mut pane_state,
                                    &mut previous_alerts,
                                    &mut fresh_alerts,
                                    &mut alert_times,
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
                                    &mut alert_state,
                                    &mut pane_state,
                                    &mut previous_alerts,
                                    &mut fresh_alerts,
                                    &mut alert_times,
                                );
                            }
                        }
                    }
                    KeyCode::Char('c') => {
                        notices.clear();
                        sync_dashboard_state(
                            &notices,
                            &last_reports,
                            &mut alert_state,
                            &mut pane_state,
                            &mut previous_alerts,
                            &mut fresh_alerts,
                            &mut alert_times,
                        );
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })();

    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen).ok();
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
                alerts: r.recommendations.iter().map(|x| x.action.to_string()).collect(),
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

fn refresh_target_choices<P: PaneSource>(
    source: &P,
    choices: &mut Vec<TargetChoice>,
    state: &mut ListState,
    selected: Option<&WindowTarget>,
) {
    let mut next_choices = vec![TargetChoice {
        label: "all windows".into(),
        target: None,
    }];
    if let Ok(targets) = source.available_targets() {
        next_choices.extend(targets.into_iter().map(|target| TargetChoice {
            label: target.label(),
            target: Some(target),
        }));
    }
    *choices = next_choices;
    sync_target_choice_selection(state, choices, selected);
}

fn sync_target_choice_selection(
    state: &mut ListState,
    choices: &[TargetChoice],
    selected: Option<&WindowTarget>,
) {
    if choices.is_empty() {
        state.select(None);
        return;
    }
    let selected_index = choices
        .iter()
        .position(|choice| choice.target.as_ref() == selected)
        .unwrap_or(0);
    state.select(Some(selected_index));
}

fn apply_target_choice(
    choices: &[TargetChoice],
    state: &ListState,
    selected_target: &mut Option<WindowTarget>,
) -> Option<String> {
    let idx = state.selected()?;
    let choice = choices.get(idx)?;
    *selected_target = choice.target.clone();
    Some(choice.label.clone())
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
        .unwrap_or_else(|| "all windows".into())
}

fn target_switched_notice(label: &str) -> SystemNotice {
    SystemNotice {
        title: "target switched".into(),
        body: format!("now watching {label}"),
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
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

fn sync_alert_selection(state: &mut ListState, notices: &[SystemNotice], reports: &[PaneReport]) {
    let count = qmonster::ui::alerts::alert_count(notices, reports);
    match count {
        0 => state.select(None),
        total => {
            let selected = state.selected().unwrap_or(0).min(total.saturating_sub(1));
            state.select(Some(selected));
        }
    }
}

fn sync_dashboard_state(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    alert_state: &mut ListState,
    pane_state: &mut ListState,
    previous_alerts: &mut HashSet<String>,
    fresh_alerts: &mut HashSet<String>,
    alert_times: &mut HashMap<String, String>,
) {
    sync_pane_selection(pane_state, reports.len());
    refresh_alert_state(
        notices,
        reports,
        previous_alerts,
        fresh_alerts,
        alert_times,
    );
    sync_alert_selection(alert_state, notices, reports);
}

fn refresh_alert_state(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    previous_alerts: &mut HashSet<String>,
    fresh_alerts: &mut HashSet<String>,
    alert_times: &mut HashMap<String, String>,
) {
    let current = qmonster::ui::alerts::alert_fingerprints(notices, reports);
    let timestamp = Local::now().format("%H:%M:%S").to_string();
    let disappeared: Vec<String> = previous_alerts
        .difference(&current)
        .cloned()
        .collect();
    for key in disappeared {
        alert_times.remove(&key);
    }

    *fresh_alerts = current
        .difference(previous_alerts)
        .cloned()
        .collect();
    for key in fresh_alerts.iter() {
        alert_times.insert(key.clone(), timestamp.clone());
    }
    *previous_alerts = current;
}
