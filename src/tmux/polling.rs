use std::process::Command;
use thiserror::Error;

use crate::tmux::types::{
    PANE_LIST_FORMAT, RawPaneSnapshot, WINDOW_LIST_FORMAT, WindowTarget, parse_list_panes_row,
    parse_list_windows_row,
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
}

/// Production implementation that shells out to the `tmux` CLI.
#[derive(Debug, Default, Clone, Copy)]
pub struct PollingSource;

impl PollingSource {
    pub fn new() -> Self {
        Self
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
        let mut rows = Vec::new();
        for line in text.lines() {
            if let Some(mut snap) = parse_list_panes_row(line) {
                if let Ok(tail) = self.capture_tail(&snap.pane_id, 24) {
                    snap.tail = tail;
                }
                rows.push(snap);
            }
        }
        Ok(rows)
    }

    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        current_window_target()
    }

    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        let fmt = WINDOW_LIST_FORMAT.replace("\\t", "\t");
        let output = Command::new("tmux")
            .args(["list-windows", "-a", "-F", &fmt])
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
        let start = format!("-{lines}");
        let output = Command::new("tmux")
            .args(["capture-pane", "-p", "-J", "-S", &start, "-t", pane_id])
            .output()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        if !output.status.success() {
            return Err(PollingError::NonZero(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
        .args(["display-message", "-p", "-t", &tmux_pane, &fmt])
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

fn list_panes_args(fmt: &str, target: Option<&WindowTarget>) -> Vec<String> {
    let mut args = vec!["list-panes".to_string()];
    match target {
        Some(target_window) => {
            args.push("-t".to_string());
            args.push(target_window.label());
        }
        None => args.push("-a".to_string()),
    }
    args.push("-F".to_string());
    args.push(fmt.to_string());
    args
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
