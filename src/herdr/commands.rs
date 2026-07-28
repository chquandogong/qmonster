//! herdr CLI invocation: pure argument builders + the process wrapper.
//!
//! Mirrors `tmux/commands.rs` + `tmux/polling_process.rs`: builders are
//! pure and unit-tested; `run_herdr` is the only process boundary.

use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::tmux::polling::PollingError;

/// herdr's canonical Enter key name for `pane send-keys` (verified
/// live 2026-07-27: `herdr pane send-keys <id> enter` → exit 0; help
/// documents lowercase canonical names, e.g. `esc`).
pub(crate) const HERDR_SUBMIT_KEY: &str = "enter";

/// Same 80ms settle the tmux backend uses between the literal text
/// invocation and the submit keystroke. Kept as a local constant (not
/// imported from `tmux::commands`) so the backends stay decoupled.
pub(crate) const HERDR_KEY_SETTLE_DELAY: Duration = Duration::from_millis(80);

pub(crate) fn pane_list_args() -> Vec<String> {
    vec!["pane".into(), "list".into()]
}

pub(crate) fn tab_list_args() -> Vec<String> {
    vec!["tab".into(), "list".into()]
}

pub(crate) fn workspace_list_args() -> Vec<String> {
    vec!["workspace".into(), "list".into()]
}

/// Deliberately NO `--lines` flag: herdr counts `--lines N` from the
/// bottom of the raw grid, so a top-anchored TUI (codex with a short
/// transcript) yields N blank rows — verified live 2026-07-28
/// (`--lines 24` → 0 bytes on a pane whose status line parses fine
/// with the default read). The default read returns the trimmed
/// recent content (~viewport-sized, 45-63 lines observed); the tail
/// bound is applied in `HerdrSource::capture_tail` instead.
pub(crate) fn pane_read_args(pane_id: &str) -> Vec<String> {
    vec![
        "pane".into(),
        "read".into(),
        pane_id.into(),
        "--format".into(),
        "text".into(),
    ]
}

pub(crate) fn process_info_args(pane_id: &str) -> Vec<String> {
    vec![
        "pane".into(),
        "process-info".into(),
        "--pane".into(),
        pane_id.into(),
    ]
}

pub(crate) fn send_text_args(pane_id: &str, text: &str) -> Vec<String> {
    vec![
        "pane".into(),
        "send-text".into(),
        pane_id.into(),
        text.into(),
    ]
}

pub(crate) fn send_key_args(pane_id: &str, key: &str) -> Vec<String> {
    vec![
        "pane".into(),
        "send-keys".into(),
        pane_id.into(),
        key.into(),
    ]
}

/// Hard ceiling per herdr CLI invocation (CFX-320-5): a wedged herdr
/// socket must stall at most one bounded call, not the polling tick
/// forever. Generous vs. observed latencies (list/read ≪ 1s).
pub(crate) const HERDR_CLI_TIMEOUT: Duration = Duration::from_secs(10);

/// The single process boundary to the `herdr` binary. Reuses
/// `PollingError` so `PaneSource` signatures stay identical across
/// backends (a missing binary / stopped server surfaces exactly like
/// a dead tmux today).
pub(crate) fn run_herdr(args: &[String]) -> Result<String, PollingError> {
    run_command_with_timeout("herdr", args, HERDR_CLI_TIMEOUT)
}

/// Spawn with piped io, drain stdout/stderr on reader threads (so a
/// full pipe can never deadlock the wait loop), poll to a deadline,
/// kill on expiry. Non-zero exits keep exit-status + bounded
/// stderr/stdout context (CFX-320-5: herdr reports structured errors
/// on stdout for some failures — don't discard them).
fn run_command_with_timeout(
    bin: &str,
    args: &[String],
    timeout: Duration,
) -> Result<String, PollingError> {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PollingError::Command(format!("{bin} spawn: {e}")))?;

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stdout_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Deliberately DETACH the reader threads instead of
                    // joining: killing the child does not kill any
                    // grandchild it forked, and a surviving grandchild
                    // keeps the pipe open — joining here would block for
                    // the grandchild's lifetime and defeat the timeout
                    // (caught by run_command_with_timeout_kills_wedged_process,
                    // which would otherwise take 30s instead of ~150ms).
                    drop(out_reader);
                    drop(err_reader);
                    return Err(PollingError::Command(format!(
                        "{bin} timed out after {}s: {}",
                        timeout.as_secs(),
                        args.join(" ")
                    )));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(PollingError::Command(format!("{bin} wait: {e}")));
            }
        }
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    if !status.success() {
        return Err(PollingError::NonZero(format!(
            "{bin} exited {status}: {}",
            bounded_diagnostic(&stderr, &stdout)
        )));
    }
    Ok(String::from_utf8_lossy(&stdout).to_string())
}

/// stderr first, stdout as fallback, clamped so a chatty failure
/// cannot flood the notice surface.
fn bounded_diagnostic(stderr: &[u8], stdout: &[u8]) -> String {
    const MAX: usize = 400;
    let text = if stderr.iter().any(|b| !b.is_ascii_whitespace()) {
        String::from_utf8_lossy(stderr)
    } else {
        String::from_utf8_lossy(stdout)
    };
    let trimmed = text.trim();
    let mut end = trimmed.len().min(MAX);
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    trimmed[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arg_builders_produce_exact_cli_vectors() {
        assert_eq!(pane_list_args(), vec!["pane", "list"]);
        assert_eq!(tab_list_args(), vec!["tab", "list"]);
        assert_eq!(workspace_list_args(), vec!["workspace", "list"]);
        assert_eq!(
            pane_read_args("w1:p1"),
            vec!["pane", "read", "w1:p1", "--format", "text"]
        );
        assert_eq!(
            process_info_args("w1:p1"),
            vec!["pane", "process-info", "--pane", "w1:p1"]
        );
        assert_eq!(
            send_text_args("w1:p1", "/compact"),
            vec!["pane", "send-text", "w1:p1", "/compact"]
        );
        assert_eq!(
            send_key_args("w1:p1", HERDR_SUBMIT_KEY),
            vec!["pane", "send-keys", "w1:p1", "enter"]
        );
    }

    #[test]
    fn settle_delay_matches_the_tmux_backend_cadence() {
        // Same 80ms pause the tmux backend uses between literal text
        // and the submit keystroke (React/Ink CLIs need the gap).
        assert_eq!(HERDR_KEY_SETTLE_DELAY.as_millis(), 80);
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn run_command_with_timeout_returns_stdout_on_success() {
        let out = run_command_with_timeout(
            "/bin/sh",
            &args(&["-c", "printf hello"]),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    fn run_command_with_timeout_kills_wedged_process() {
        // Also locks that the call RETURNS promptly: `sh -c "sleep 30"`
        // may fork, and a surviving grandchild keeps the stdout pipe
        // open, so the implementation must not join its reader threads
        // on the timeout path.
        let started = std::time::Instant::now();
        let err = run_command_with_timeout(
            "/bin/sh",
            &args(&["-c", "sleep 30"]),
            Duration::from_millis(150),
        )
        .unwrap_err();
        let elapsed = started.elapsed();
        assert!(
            err.to_string().contains("timed out"),
            "expected timeout error, got: {err}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout must bound the call; took {elapsed:?}"
        );
    }

    #[test]
    fn run_command_with_timeout_reports_status_and_stderr() {
        let err = run_command_with_timeout(
            "/bin/sh",
            &args(&["-c", "echo oops >&2; exit 3"]),
            Duration::from_secs(5),
        )
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("exit status: 3"), "got: {text}");
        assert!(text.contains("oops"), "got: {text}");
    }

    #[test]
    fn run_command_with_timeout_falls_back_to_stdout_diagnostics() {
        // herdr sometimes reports structured errors on stdout.
        let err = run_command_with_timeout(
            "/bin/sh",
            &args(&["-c", "echo detail-on-stdout; exit 2"]),
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(err.to_string().contains("detail-on-stdout"));
    }

    #[test]
    fn bounded_diagnostic_clamps_runaway_output() {
        let noisy = vec![b'x'; 10_000];
        let text = bounded_diagnostic(&noisy, b"");
        assert!(text.len() <= 400);
    }
}
