use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, bail};
use clap::{Parser, Subcommand};

use qmonster::app::event_loop::run_once;
use qmonster::app::once_report::print_once_reports;
use qmonster::app::startup::{StartupOptions, build_startup_runtime};
use qmonster::app::tui_loop::run_tui;
use qmonster::insights_report::{
    empty_insights_snapshot, format_insights_report_lines, parse_since_arg, resolve_insights_paths,
};
use qmonster::store::{InsightsWindow, SqliteInsightsStore};

#[derive(Debug, Subcommand)]
enum CliCommand {
    Insights {
        #[arg(long, default_value = "24h")]
        since: String,
    },
}

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

    #[command(subcommand)]
    command: Option<CliCommand>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.once && cli.command.is_some() {
        bail!("--once cannot be combined with a subcommand");
    }

    let env_root = std::env::var("QMONSTER_ROOT").ok();
    if let Some(CliCommand::Insights { since }) = cli.command.as_ref() {
        let since_secs = parse_since_arg(since)?;
        let (paths, root_source, _config) = resolve_insights_paths(
            cli.config.as_deref(),
            cli.root.as_deref(),
            &cli.set,
            env_root.as_deref(),
        )?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before UNIX_EPOCH")?;
        let now_ms = i64::try_from(now.as_millis()).context("current time exceeds i64 millis")?;
        let since_delta_ms = i64::try_from(u128::from(since_secs) * 1000)
            .context("--since value exceeds i64 millis")?;
        let window = InsightsWindow {
            since_ms: now_ms.saturating_sub(since_delta_ms),
            until_ms: now_ms,
        };
        let sqlite_path = paths.sqlite_path();
        let snapshot = if sqlite_path.exists() {
            let store = SqliteInsightsStore::open_read_only(&sqlite_path)
                .with_context(|| format!("open insights store at {}", sqlite_path.display()))?;
            store.snapshot(window)?
        } else {
            empty_insights_snapshot(window)
        };
        println!(
            "qmonster paths: {} (source: {:?})",
            paths.root().display(),
            root_source
        );
        for line in format_insights_report_lines(&snapshot) {
            println!("{line}");
        }
        return Ok(());
    }

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
        println!("tmux source: {}", ctx.source.transport_label());
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
