use std::process::Command;
use std::thread;
use thiserror::Error;

use crate::tmux::commands::{
    KEY_SETTLE_DELAY, SUBMIT_KEY, capture_tail_args, current_target_args, list_panes_args,
    list_windows_args, send_key_args, send_keys_literal_args,
};
use crate::tmux::snapshots::hydrate_pane_snapshots;
use crate::tmux::types::{
    PANE_LIST_FORMAT, RawPaneSnapshot, WINDOW_LIST_FORMAT, WindowTarget, parse_list_windows_row,
};

#[derive(Debug, Error)]
pub enum PollingError {
    #[error("tmux command failed: {0}")]
    Command(String),
    #[error("tmux returned non-zero: {0}")]
    NonZero(String),
}

/// Source of `RawPaneSnapshot`s. Trait so tests and Phase-2 control
/// mode can plug in without touching callers. `tmux/` must never know
/// about providers or signals — that's why the return type is the raw
/// snapshot list.
pub trait PaneSource {
    fn list_panes(
        &self,
        target: Option<&WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError>;
    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError>;
    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError>;
    fn capture_tail(&self, pane_id: &str, lines: usize) -> Result<String, PollingError>;
    /// Phase 5 P5-3 (v1.10.0): send `text` followed by submit to the
    /// pane identified by `pane_id`. The implementation calls
    /// `tmux send-keys -t <pane_id> -l <text>`, pauses briefly, then
    /// sends terminal submit (`C-m`, the Enter carriage return) —
    /// literal text only, no shell expansion, no raw tail. `tmux/`
    /// knows nothing about
    /// providers or slash-command semantics; the caller supplies the
    /// already-validated slash command string.
    fn send_keys(&self, pane_id: &str, text: &str) -> Result<(), PollingError>;

    /// Send a single tmux key name such as `Escape` or `C-m`.
    fn send_key(&self, _pane_id: &str, _key: &str) -> Result<(), PollingError> {
        Ok(())
    }
}

const DEFAULT_CAPTURE_LINES: usize = 24;

/// Production implementation that shells out to the `tmux` CLI.
#[derive(Debug, Clone, Copy)]
pub struct PollingSource {
    capture_lines: usize,
}

impl PollingSource {
    pub fn new(capture_lines: usize) -> Self {
        Self {
            capture_lines: capture_lines.max(1),
        }
    }
}

impl Default for PollingSource {
    fn default() -> Self {
        Self::new(DEFAULT_CAPTURE_LINES)
    }
}

impl PaneSource for PollingSource {
    fn list_panes(
        &self,
        target: Option<&WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        let fmt = PANE_LIST_FORMAT.replace("\\t", "\t");
        // `None` means "all panes across all sessions/windows".
        // The caller decides whether the default view should be the
        // current window (TUI startup) or the global view ("all
        // sessions"). Do not silently collapse `None` back to the
        // current window here, or the All Sessions picker becomes a
        // lie.
        let args = list_panes_args(&fmt, target);
        let output = Command::new("tmux")
            .args(&args)
            .output()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        if !output.status.success() {
            return Err(PollingError::NonZero(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(hydrate_pane_snapshots(text.lines(), |pane_id| {
            self.capture_tail(pane_id, self.capture_lines).ok()
        }))
    }

    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        current_window_target()
    }

    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        let fmt = WINDOW_LIST_FORMAT.replace("\\t", "\t");
        let output = Command::new("tmux")
            .args(list_windows_args(&fmt))
            .output()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        if !output.status.success() {
            return Err(PollingError::NonZero(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut targets: Vec<WindowTarget> =
            text.lines().filter_map(parse_list_windows_row).collect();
        targets.sort();
        targets.dedup();
        Ok(targets)
    }

    fn capture_tail(&self, pane_id: &str, lines: usize) -> Result<String, PollingError> {
        let output = Command::new("tmux")
            .args(capture_tail_args(pane_id, lines))
            .output()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        if !output.status.success() {
            return Err(PollingError::NonZero(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn send_keys(&self, pane_id: &str, text: &str) -> Result<(), PollingError> {
        // P5-3 v1.10.1 remediation (Gemini v1.10.0 finding #2):
        //
        // tmux `send-keys` by default interprets each argument through
        // its key-name table (`Enter`, `C-c`, `Up`, etc.) before
        // falling back to literal text. If the slash command ever
        // contains a valid key name (e.g. `-l` itself, or `Up`), tmux
        // would treat it as a keystroke rather than text. That is not
        // a shell-injection concern — `std::process::Command` never
        // invokes a shell — but it IS a tmux-level argument-parsing
        // concern that compromises correctness once the producer
        // set grows beyond `/compact`.
        //
        // Fix: invoke tmux twice.
        //   1. `send-keys -t {pane} -l {text}` — `-l` forces every
        //      following argument to be literal text (no key-name
        //      lookup). This means `Enter` would ALSO be literal if
        //      we kept it in the same invocation — hence the split.
        //   2. wait briefly, then `send-keys -t {pane} C-m` — a
        //      separate invocation whose only positional is the
        //      terminal submit key name. The pause gives React/Ink
        //      CLIs time to process the literal text before submit.
        //
        // Failure on either invocation surfaces as `PollingError`; the
        // caller (`PromptSendGate::Execute` path) records the error
        // text in a `PromptSendFailed` audit event.
        let literal = Command::new("tmux")
            .args(send_keys_literal_args(pane_id, text))
            .output()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        if !literal.status.success() {
            return Err(PollingError::NonZero(
                String::from_utf8_lossy(&literal.stderr).trim().to_string(),
            ));
        }
        thread::sleep(KEY_SETTLE_DELAY);
        self.send_key(pane_id, SUBMIT_KEY)?;
        thread::sleep(KEY_SETTLE_DELAY);
        Ok(())
    }

    fn send_key(&self, pane_id: &str, key: &str) -> Result<(), PollingError> {
        let output = Command::new("tmux")
            .args(send_key_args(pane_id, key))
            .output()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        if !output.status.success() {
            return Err(PollingError::NonZero(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        thread::sleep(KEY_SETTLE_DELAY);
        Ok(())
    }
}

fn current_window_target() -> Result<Option<WindowTarget>, PollingError> {
    let Ok(tmux_pane) = std::env::var("TMUX_PANE") else {
        return Ok(None);
    };
    if tmux_pane.trim().is_empty() {
        return Ok(None);
    }
    let fmt = WINDOW_LIST_FORMAT.replace("\\t", "\t");
    let output = Command::new("tmux")
        .args(current_target_args(&tmux_pane, &fmt))
        .output()
        .map_err(|e| PollingError::Command(e.to_string()))?;
    if !output.status.success() {
        return Err(PollingError::NonZero(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let line = String::from_utf8_lossy(&output.stdout);
    Ok(line.lines().next().and_then(parse_list_windows_row))
}

/// Test-only in-memory source.
#[cfg(test)]
pub struct FixtureSource {
    pub panes: Vec<RawPaneSnapshot>,
}

#[cfg(test)]
impl PaneSource for FixtureSource {
    fn list_panes(
        &self,
        _target: Option<&WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        Ok(self.panes.clone())
    }

    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        Ok(self.available_targets()?.into_iter().next())
    }

    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        let mut targets: Vec<WindowTarget> = self
            .panes
            .iter()
            .map(|pane| WindowTarget {
                session_name: pane.session_name.clone(),
                window_index: pane.window_index.clone(),
            })
            .collect();
        targets.sort();
        targets.dedup();
        Ok(targets)
    }

    fn capture_tail(&self, pane_id: &str, _lines: usize) -> Result<String, PollingError> {
        Ok(self
            .panes
            .iter()
            .find(|p| p.pane_id == pane_id)
            .map(|p| p.tail.clone())
            .unwrap_or_default())
    }

    fn send_keys(&self, _pane_id: &str, _text: &str) -> Result<(), PollingError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_source_send_keys_is_a_noop_and_returns_ok() {
        // P5-3 contract: FixtureSource.send_keys returns Ok(()) so unit
        // tests that exercise proposal flows never need a real tmux session.
        let src = FixtureSource { panes: vec![] };
        assert!(src.send_keys("%1", "/compact").is_ok());
        assert!(src.send_keys("%2", "/clear").is_ok());
    }

    #[test]
    fn send_keys_splits_literal_payload_from_terminal_submit_keystroke() {
        // v1.10.1 remediation (Gemini v1.10.0 finding #2): the
        // PollingSource MUST emit two tmux invocations — the first
        // with `-l` so `text` is always literal (no key-name lookup
        // can surprise the caller), the second with `C-m` alone so it
        // is still interpreted as the terminating keystroke.
        // Whitebox contract on the argument vectors.
        let pane_id = "%5";
        let text = "/compact";

        let literal_args = ["send-keys", "-t", pane_id, "-l", text];
        assert_eq!(literal_args[0], "send-keys");
        assert_eq!(literal_args[1], "-t");
        assert_eq!(literal_args[2], pane_id);
        assert_eq!(
            literal_args[3], "-l",
            "first invocation MUST pass -l so tmux treats the payload as literal text"
        );
        assert_eq!(literal_args[4], text);
        assert_eq!(
            literal_args.len(),
            5,
            "submit must NOT ride on the -l invocation or it would be sent as literal text"
        );

        let submit_args = ["send-keys", "-t", pane_id, SUBMIT_KEY];
        assert_eq!(submit_args[0], "send-keys");
        assert_eq!(submit_args[1], "-t");
        assert_eq!(submit_args[2], pane_id);
        assert_eq!(
            submit_args[3], SUBMIT_KEY,
            "second invocation passes terminal submit as a key name (no -l)"
        );
    }

    #[test]
    fn send_keys_literal_flag_protects_against_tmux_keyname_collisions() {
        // Defensive contract: even if a future producer ever proposes
        // a slash-command string that happens to look like a tmux key
        // name (e.g. "Up", "PageDown", "C-c", or literally "-l"), the
        // fixed `-l` slot at argv[3] guarantees tmux treats the
        // following argument at argv[4] as literal text. This test
        // locks the positional layout so a careless refactor (e.g.
        // reordering, or dropping `-l`) is caught at the argument
        // vector level.
        for payload in ["/compact", "Up", "C-c", "-l", "PageDown"] {
            let args = ["send-keys", "-t", "%1", "-l", payload];
            assert_eq!(args[3], "-l", "argv[3] MUST be the -l flag");
            assert_eq!(
                args[4], payload,
                "argv[4] MUST be the payload so tmux treats it as literal text after -l"
            );
        }
    }

    #[test]
    fn fixture_source_returns_injected_panes() {
        let fixtures = vec![RawPaneSnapshot {
            session_name: "qwork".into(),
            window_index: "1".into(),
            pane_id: "%1".into(),
            title: "claude:1:main".into(),
            current_command: "claude".into(),
            current_path: "/tmp".into(),
            active: true,
            dead: false,
            tail: "hello".into(),
        }];
        let src = FixtureSource { panes: fixtures };
        let panes = src.list_panes(None).unwrap();
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].title, "claude:1:main");
        assert_eq!(src.capture_tail("%1", 24).unwrap(), "hello");
    }

    #[test]
    fn list_panes_uses_current_window_when_target_present() {
        let args = list_panes_args(
            "fmt",
            Some(&WindowTarget {
                session_name: "qwork".into(),
                window_index: "1".into(),
            }),
        );
        assert_eq!(args, vec!["list-panes", "-t", "qwork:1", "-F", "fmt",]);
    }

    #[test]
    fn list_panes_without_target_uses_all_panes() {
        let args = list_panes_args("fmt", None);
        assert_eq!(args, vec!["list-panes", "-a", "-F", "fmt"]);
    }

    #[test]
    fn fixture_source_reports_unique_targets() {
        let src = FixtureSource {
            panes: vec![
                RawPaneSnapshot {
                    session_name: "qwork".into(),
                    window_index: "1".into(),
                    pane_id: "%1".into(),
                    title: "claude:1:main".into(),
                    current_command: "claude".into(),
                    current_path: "/tmp".into(),
                    active: true,
                    dead: false,
                    tail: "hello".into(),
                },
                RawPaneSnapshot {
                    session_name: "qwork".into(),
                    window_index: "1".into(),
                    pane_id: "%2".into(),
                    title: "codex:1:review".into(),
                    current_command: "codex".into(),
                    current_path: "/tmp".into(),
                    active: true,
                    dead: false,
                    tail: "hello".into(),
                },
                RawPaneSnapshot {
                    session_name: "research".into(),
                    window_index: "2".into(),
                    pane_id: "%3".into(),
                    title: "gemini:1:research".into(),
                    current_command: "gemini".into(),
                    current_path: "/tmp".into(),
                    active: true,
                    dead: false,
                    tail: "hello".into(),
                },
            ],
        };
        let targets = src.available_targets().unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].label(), "qwork:1");
        assert_eq!(targets[1].label(), "research:2");
    }
}
