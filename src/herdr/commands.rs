//! herdr CLI invocation: pure argument builders + the process wrapper.
//!
//! Mirrors `tmux/commands.rs` + `tmux/polling_process.rs`: builders are
//! pure and unit-tested; `run_herdr` is the only process boundary.

use std::process::Command;
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

pub(crate) fn pane_read_args(pane_id: &str, lines: usize) -> Vec<String> {
    vec![
        "pane".into(),
        "read".into(),
        pane_id.into(),
        "--lines".into(),
        lines.to_string(),
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

/// The single process boundary to the `herdr` binary. Reuses
/// `PollingError` so `PaneSource` signatures stay identical across
/// backends (a missing binary / stopped server surfaces exactly like
/// a dead tmux today).
pub(crate) fn run_herdr(args: &[String]) -> Result<String, PollingError> {
    let output = Command::new("herdr")
        .args(args)
        .output()
        .map_err(|e| PollingError::Command(e.to_string()))?;
    if !output.status.success() {
        return Err(PollingError::NonZero(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
            pane_read_args("w1:p1", 24),
            vec!["pane", "read", "w1:p1", "--lines", "24", "--format", "text"]
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
}
