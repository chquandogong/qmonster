use std::process::Command;
use thiserror::Error;

use crate::tmux::types::{PANE_LIST_FORMAT, RawPaneSnapshot, parse_list_panes_row};

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
    fn list_panes(&self) -> Result<Vec<RawPaneSnapshot>, PollingError>;
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
    fn list_panes(&self) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        let fmt = PANE_LIST_FORMAT.replace("\\t", "\t");
        let output = Command::new("tmux")
            .args(["list-panes", "-a", "-F", &fmt])
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

/// Test-only in-memory source.
#[cfg(test)]
pub struct FixtureSource {
    pub panes: Vec<RawPaneSnapshot>,
}

#[cfg(test)]
impl PaneSource for FixtureSource {
    fn list_panes(&self) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        Ok(self.panes.clone())
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
        let panes = src.list_panes().unwrap();
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].title, "claude:1:main");
        assert_eq!(src.capture_tail("%1", 24).unwrap(), "hello");
    }
}
