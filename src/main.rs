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
use qmonster::notify::desktop::DesktopNotifier;
use qmonster::store::sink::InMemorySink;
use qmonster::tmux::polling::PollingSource;
use qmonster::ui::dashboard::render_dashboard;

#[derive(Debug, Parser)]
#[command(
    name = "qmonster",
    about = "Observe-first TUI for multi-CLI tmux work (Phase 1 MVP)"
)]
struct Cli {
    /// Path to a TOML config file.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Safer-only config overrides as key=value (e.g. `actions.mode=observe_only`).
    #[arg(long, value_name = "KEY=VALUE")]
    set: Vec<String>,

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

    // Parse --set into (key, value) tuples for the audit-aware applier.
    let mut pairs: Vec<(String, String)> = Vec::new();
    for kv in &cli.set {
        let Some((k, v)) = kv.split_once('=') else {
            anyhow::bail!("--set expects key=value, got {kv}");
        };
        pairs.push((k.trim().into(), v.trim().into()));
    }

    let source = PollingSource::new();
    let notifier = DesktopNotifier;
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(config, source, notifier, sink);

    // Apply overrides AFTER Context exists so rejections hit the sink.
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

    // Startup version snapshot — dedicated audit kind.
    let versions = capture_versions();
    record_startup_snapshot(&*ctx.sink, &versions);

    if cli.once {
        println!("qmonster versions captured:");
        for (k, v) in &versions.tools {
            println!("  {k}: {v}");
        }
        println!();
        let reports = run_once(&mut ctx, Instant::now())?;
        print_reports(&reports);
        return Ok(());
    }

    run_tui(&mut ctx, versions)
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

fn run_tui<P, N>(ctx: &mut Context<P, N>, mut versions: VersionSnapshot) -> anyhow::Result<()>
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
    let mut notices: Vec<SystemNotice> = Vec::new();
    let mut last_poll = Instant::now() - poll; // force first poll immediately

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
                        // Operator-requested manual refresh: re-check versions.
                        let fresh = capture_versions();
                        let new_notices = route_version_drift(&versions, &fresh, &*ctx.sink);
                        if !new_notices.is_empty() {
                            notices = new_notices;
                        }
                        versions = fresh;
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
