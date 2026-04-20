use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;

use qmonster::app::bootstrap::Context;
use qmonster::app::config::{QmonsterConfig, load_from_path};
use qmonster::app::event_loop::{PaneReport, run_once};
use qmonster::app::safety_audit::apply_override_with_audit;
use qmonster::app::system_notice::{
    SystemNotice, record_startup_snapshot, route_version_drift,
};
use qmonster::app::version_drift::{VersionSnapshot, capture_versions};
use qmonster::domain::audit::{AuditEvent, AuditEventKind};
use qmonster::domain::origin::SourceKind;
use qmonster::domain::recommendation::Severity;
use qmonster::notify::desktop::DesktopNotifier;
use qmonster::store::{
    ArchiveWriter, EventSink, InMemorySink, PaneSnapshot, QmonsterPaths, SnapshotInput,
    SnapshotWriter, SqliteAuditSink, sweep,
};
use qmonster::tmux::polling::PollingSource;
use qmonster::ui::dashboard::render_dashboard;

#[derive(Debug, Parser)]
#[command(
    name = "qmonster",
    about = "Observe-first TUI for multi-CLI tmux work (Phase 2)"
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

    let paths = resolve_paths(cli.root.as_deref(), &config);
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

    // Load previous version snapshot (if any), capture fresh, diff, save.
    let prev = VersionSnapshot::load_from(&paths.versions_path()).unwrap_or(None);
    let fresh = capture_versions();
    let mut startup_notices: Vec<SystemNotice> = Vec::new();
    if let Some(prev) = prev.as_ref() {
        startup_notices = route_version_drift(prev, &fresh, &*ctx.sink);
    }
    record_startup_snapshot(&*ctx.sink, &fresh);
    if let Err(e) = fresh.save_to(&paths.versions_path()) {
        eprintln!("qmonster: could not persist version snapshot: {e}");
    }

    let snapshot_writer = SnapshotWriter::new(paths.clone());

    if cli.once {
        println!("qmonster paths: {}", paths.root().display());
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

fn resolve_paths(cli_root: Option<&std::path::Path>, config: &QmonsterConfig) -> QmonsterPaths {
    if let Some(p) = cli_root {
        return QmonsterPaths::at(p.to_path_buf());
    }
    if let Ok(env) = std::env::var("QMONSTER_ROOT")
        && !env.is_empty()
    {
        return QmonsterPaths::at(PathBuf::from(env));
    }
    if let Some(root) = config.storage.root.as_deref() {
        return QmonsterPaths::at(PathBuf::from(root));
    }
    QmonsterPaths::default_root()
}

fn print_reports(reports: &[PaneReport]) {
    for r in reports {
        println!(
            "{} [{:?}:{}:{:?}] confidence={:?} dead={}",
            r.pane_id,
            r.identity.identity.provider,
            r.identity.identity.instance,
            r.identity.identity.role,
            r.identity.confidence,
            r.dead
        );
        let chips = qmonster::ui::panels::signal_chips(&r.signals);
        if !chips.is_empty() {
            println!("  signals: {}", chips.join(" "));
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
        for rec in &r.recommendations {
            println!(
                "  [{}] [{}] {}: {}",
                rec.severity.letter(),
                rec.source_kind.badge(),
                rec.action,
                rec.reason
            );
        }
    }
}

fn run_tui<P, N>(
    ctx: &mut Context<P, N>,
    mut versions: VersionSnapshot,
    snapshot_writer: SnapshotWriter,
    startup_notices: Vec<SystemNotice>,
) -> anyhow::Result<()>
where
    P: qmonster::tmux::polling::PaneSource,
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

    let result = (|| -> anyhow::Result<()> {
        loop {
            let now = Instant::now();
            if now.saturating_duration_since(last_poll) >= poll {
                last_poll = now;
                last_reports = run_once(ctx, now).unwrap_or_default();
            }

            terminal.draw(|frame| render_dashboard(frame, &notices, &last_reports))?;

            if event::poll(Duration::from_millis(100))?
                && let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('r') => {
                        let fresh = capture_versions();
                        let new_notices = route_version_drift(&versions, &fresh, &*ctx.sink);
                        if !new_notices.is_empty() {
                            notices = new_notices;
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
                            }
                        }
                    }
                    KeyCode::Char('c') => {
                        notices.clear();
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
