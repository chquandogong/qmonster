use std::thread;
use std::time::Duration;

use clap::Parser;

use qmonster::tmux::parity::{
    TmuxSourceParityReport, compare_all_pane_source_targets, compare_pane_sources,
};
use qmonster::tmux::types::WindowTarget;
use qmonster::tmux::{ControlModeSource, PollingSource};

#[derive(Debug, Parser)]
#[command(
    name = "qmonster-tmux-parity",
    about = "Compare Qmonster polling and tmux control-mode pane sources"
)]
struct Cli {
    /// Tail lines to capture when comparing common panes.
    #[arg(long, default_value_t = 24)]
    capture_lines: usize,

    /// Restrict pane comparison to one tmux window, formatted as session:index.
    #[arg(long, value_name = "SESSION:WINDOW", conflicts_with = "all_targets")]
    target: Option<String>,

    /// Compare each discovered tmux window target separately.
    #[arg(long)]
    all_targets: bool,

    /// Treat live tail differences as failures. By default they are warnings.
    #[arg(long)]
    strict_tail: bool,

    /// Repeat the parity check using the same control-mode client.
    #[arg(long, default_value_t = 1)]
    repeat: usize,

    /// Delay between repeated parity checks.
    #[arg(long, default_value_t = 0)]
    delay_ms: u64,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.repeat == 0 {
        anyhow::bail!("--repeat must be at least 1");
    }
    let target = cli.target.as_deref().map(parse_target).transpose()?;
    let polling = PollingSource::new(cli.capture_lines);
    let control_mode = ControlModeSource::attach_current(cli.capture_lines)
        .map_err(|e| anyhow::anyhow!("attach tmux control-mode source: {e}"))?;
    let mut failed = false;
    for iteration in 1..=cli.repeat {
        if cli.repeat > 1 {
            println!("tmux source parity run {iteration}/{}", cli.repeat);
        }
        let reports = collect_reports(&polling, &control_mode, &target, &cli)?;
        print_reports(&reports, cli.strict_tail);
        failed |= reports.iter().any(|report| !report.passes(cli.strict_tail));
        if iteration < cli.repeat && cli.delay_ms > 0 {
            thread::sleep(Duration::from_millis(cli.delay_ms));
        }
        if iteration < cli.repeat {
            println!();
        }
    }

    if failed {
        anyhow::bail!("tmux source parity check failed")
    } else {
        Ok(())
    }
}

fn collect_reports(
    polling: &PollingSource,
    control_mode: &ControlModeSource,
    target: &Option<WindowTarget>,
    cli: &Cli,
) -> anyhow::Result<Vec<TmuxSourceParityReport>> {
    if cli.all_targets {
        Ok(compare_all_pane_source_targets(
            polling,
            control_mode,
            cli.capture_lines,
        )?)
    } else {
        Ok(vec![compare_pane_sources(
            polling,
            control_mode,
            target.as_ref(),
            cli.capture_lines,
        )?])
    }
}

fn parse_target(raw: &str) -> anyhow::Result<WindowTarget> {
    let Some((session_name, window_index)) = raw.split_once(':') else {
        anyhow::bail!("--target expects session:index, got {raw}");
    };
    let session_name = session_name.trim();
    let window_index = window_index.trim();
    if session_name.is_empty() || window_index.is_empty() {
        anyhow::bail!("--target expects non-empty session and window index, got {raw}");
    }
    Ok(WindowTarget {
        session_name: session_name.into(),
        window_index: window_index.into(),
    })
}

fn print_reports(reports: &[TmuxSourceParityReport], strict_tail: bool) {
    if reports.is_empty() {
        println!("tmux source parity: no tmux targets discovered");
        println!("status: ok");
        return;
    }
    if reports.len() > 1 {
        println!("tmux source parity targets checked: {}", reports.len());
    }
    for (index, report) in reports.iter().enumerate() {
        if index > 0 {
            println!();
        }
        print_report(report, strict_tail);
    }
    if reports.len() > 1 {
        println!();
        println!(
            "overall status: {}",
            if reports.iter().all(|report| report.passes(strict_tail)) {
                "ok"
            } else {
                "mismatch"
            }
        );
    }
}

fn print_report(report: &TmuxSourceParityReport, strict_tail: bool) {
    let target = report
        .target
        .as_ref()
        .map(WindowTarget::label)
        .unwrap_or_else(|| "all sessions".into());
    println!("tmux source parity target: {target}");
    println!(
        "panes: polling={} control_mode={}",
        report.polling_pane_count, report.control_mode_pane_count
    );
    println!(
        "current target: polling={} control_mode={}",
        target_label(report.polling_current_target.as_ref()),
        target_label(report.control_mode_current_target.as_ref())
    );

    print_targets("only in polling targets", &report.only_polling_targets);
    print_targets(
        "only in control_mode targets",
        &report.only_control_mode_targets,
    );
    print_panes("only in polling panes", &report.only_polling_panes);
    print_panes(
        "only in control_mode panes",
        &report.only_control_mode_panes,
    );
    if !report.pane_mismatches.is_empty() {
        println!("pane metadata mismatches:");
        for mismatch in &report.pane_mismatches {
            println!(
                "  {} {}: polling={:?} control_mode={:?}",
                mismatch.key.label(),
                mismatch.field,
                mismatch.polling,
                mismatch.control_mode
            );
        }
    }
    if !report.tail_mismatches.is_empty() {
        let mode = if strict_tail { "fail" } else { "warn" };
        println!("tail mismatches ({mode}):");
        for mismatch in &report.tail_mismatches {
            println!(
                "  {}: polling_lines={} control_mode_lines={}",
                mismatch.key.label(),
                mismatch.polling_lines,
                mismatch.control_mode_lines
            );
        }
    }
    println!(
        "status: {}",
        if report.passes(strict_tail) {
            "ok"
        } else {
            "mismatch"
        }
    );
}

fn target_label(target: Option<&WindowTarget>) -> String {
    target
        .map(WindowTarget::label)
        .unwrap_or_else(|| "none".into())
}

fn print_targets(label: &str, targets: &[WindowTarget]) {
    if targets.is_empty() {
        return;
    }
    println!("{label}:");
    for target in targets {
        println!("  {}", target.label());
    }
}

fn print_panes(label: &str, panes: &[qmonster::tmux::parity::PaneParityKey]) {
    if panes.is_empty() {
        return;
    }
    println!("{label}:");
    for pane in panes {
        println!("  {}", pane.label());
    }
}
