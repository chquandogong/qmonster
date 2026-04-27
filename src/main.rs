use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;

use qmonster::app::event_loop::run_once;
use qmonster::app::once_report::print_once_reports;
use qmonster::app::startup::{StartupOptions, build_startup_runtime};
use qmonster::app::tui_loop::run_tui;

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
