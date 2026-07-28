//! `HerdrSource` — the herdr backend behind the `PaneSource` trait.
//!
//! Global by design (operator decision 2026-07-27): `current_target()`
//! is `None`, so the default view spans every herdr workspace. A
//! `Some(target)` filters to one workspace (session_name = workspace
//! label); `window_index` in herdr targets is the literal `"all"`.

use std::collections::HashMap;
use std::thread;

use serde::de::DeserializeOwned;

use crate::herdr::commands::{
    HERDR_KEY_SETTLE_DELAY, HERDR_SUBMIT_KEY, pane_list_args, pane_read_args, process_info_args,
    run_herdr, send_key_args, send_text_args, tab_list_args, workspace_list_args,
};
use crate::herdr::types::{HerdrPane, HerdrProcessInfo, HerdrTab, HerdrWorkspace, parse_envelope};
use crate::tmux::polling::{PaneSource, PollingError};
use crate::tmux::types::{RawPaneSnapshot, WindowTarget};

/// Herdr targets are workspace-granular; `window_index` carries this
/// literal so `WindowTarget::label()` reads `"<workspace>:all"`.
const HERDR_TARGET_ALL: &str = "all";

/// A pane joined with its workspace/tab display names, pre-sort.
/// Intermediate between the raw herdr rows and `RawPaneSnapshot`
/// (tail + process enrichment happen per pane afterwards).
#[derive(Debug, Clone)]
pub(crate) struct PendingPane {
    pub(crate) pane: HerdrPane,
    pub(crate) session_name: String,
    pub(crate) window_index: String,
}

/// Join panes with workspace/tab labels, apply the inclusion filter
/// (agent panes + the monitor's own pane; plain shells opt-in), and
/// sort by (workspace number, tab number, pane_id) so the dashboard
/// groups panes by workspace without any UI-side changes.
pub(crate) fn assemble_snapshots(
    workspaces: &[HerdrWorkspace],
    tabs: &[HerdrTab],
    panes: &[HerdrPane],
    include_shell_panes: bool,
    self_pane_id: Option<&str>,
) -> Vec<PendingPane> {
    let ws_by_id: HashMap<&str, &HerdrWorkspace> = workspaces
        .iter()
        .map(|w| (w.workspace_id.as_str(), w))
        .collect();
    let tab_by_id: HashMap<&str, &HerdrTab> = tabs.iter().map(|t| (t.tab_id.as_str(), t)).collect();

    let mut rows: Vec<(u32, u32, PendingPane)> = panes
        .iter()
        .filter(|pane| {
            pane.agent.is_some()
                || self_pane_id.is_some_and(|id| id == pane.pane_id)
                || include_shell_panes
        })
        .map(|pane| {
            let ws = ws_by_id.get(pane.workspace_id.as_str());
            let tab = tab_by_id.get(pane.tab_id.as_str());
            let session_name = ws
                .and_then(|w| w.label.clone())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| pane.workspace_id.clone());
            let window_index = tab
                .and_then(|t| t.label.clone())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| tab_id_suffix(&pane.tab_id));
            let ws_order = ws.and_then(|w| w.number).unwrap_or(u32::MAX);
            let tab_order = tab.and_then(|t| t.number).unwrap_or(u32::MAX);
            (
                ws_order,
                tab_order,
                PendingPane {
                    pane: pane.clone(),
                    session_name,
                    window_index,
                },
            )
        })
        .collect();
    rows.sort_by(|a, b| {
        (a.0, a.1, a.2.pane.pane_id.as_str()).cmp(&(b.0, b.1, b.2.pane.pane_id.as_str()))
    });
    rows.into_iter().map(|(_, _, p)| p).collect()
}

/// tmux-equivalent tail semantics on top of the default herdr read:
/// trim trailing blank rows (a top-anchored TUI leaves the bottom of
/// the grid empty), then keep the last `lines` rows. herdr's own
/// `--lines N` flag counts raw grid rows from the bottom and is NOT
/// used (see `pane_read_args`).
fn tail_last_lines(text: &str, lines: usize) -> String {
    let rows: Vec<&str> = text.lines().collect();
    let end = rows
        .iter()
        .rposition(|row| !row.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    let start = end.saturating_sub(lines.max(1));
    rows[start..end].join("\n")
}

/// `"w1:t7"` → `"t7"`; an id without `:` passes through verbatim.
fn tab_id_suffix(tab_id: &str) -> String {
    tab_id
        .rsplit_once(':')
        .map(|(_, suffix)| suffix.to_string())
        .unwrap_or_else(|| tab_id.to_string())
}

fn fetch<T: DeserializeOwned>(args: &[String], key: &str) -> Result<T, PollingError> {
    let raw = run_herdr(args)?;
    parse_envelope(&raw, key).map_err(PollingError::Command)
}

/// Production herdr backend: shells out to the `herdr` CLI per tick,
/// exactly as `PollingSource` shells out to tmux.
#[derive(Debug, Clone)]
pub struct HerdrSource {
    capture_lines: usize,
    include_shell_panes: bool,
    /// `HERDR_PANE_ID` of the pane Qmonster itself runs in, so the
    /// monitor's own tile stays visible without `include_shell_panes`
    /// (herdr does not classify qmonster as an agent).
    self_pane_id: Option<String>,
}

impl HerdrSource {
    pub fn new(
        capture_lines: usize,
        include_shell_panes: bool,
        self_pane_id: Option<String>,
    ) -> Self {
        Self {
            capture_lines: capture_lines.max(1),
            include_shell_panes,
            self_pane_id,
        }
    }

    fn fetch_pending(&self) -> Result<Vec<PendingPane>, PollingError> {
        let workspaces: Vec<HerdrWorkspace> = fetch(&workspace_list_args(), "workspaces")?;
        let tabs: Vec<HerdrTab> = fetch(&tab_list_args(), "tabs")?;
        let panes: Vec<HerdrPane> = fetch(&pane_list_args(), "panes")?;
        Ok(assemble_snapshots(
            &workspaces,
            &tabs,
            &panes,
            self.include_shell_panes,
            self.self_pane_id.as_deref(),
        ))
    }

    fn snapshot_for(&self, pending: PendingPane) -> RawPaneSnapshot {
        let PendingPane {
            pane,
            session_name,
            window_index,
        } = pending;
        // Per-pane enrichment failures degrade that pane only (empty
        // command / no pid / empty tail), never the whole tick.
        let (current_command, pane_pid) =
            fetch::<HerdrProcessInfo>(&process_info_args(&pane.pane_id), "process_info")
                .ok()
                .and_then(|info| info.foreground_processes.into_iter().next())
                .map(|proc| (proc.name.unwrap_or_default(), proc.pid))
                .unwrap_or_default();
        let tail = self
            .capture_tail(&pane.pane_id, self.capture_lines)
            .unwrap_or_default();
        RawPaneSnapshot {
            session_name,
            window_index,
            pane_id: pane.pane_id,
            title: pane
                .label
                .filter(|l| !l.is_empty())
                .or(pane.terminal_title_stripped)
                .unwrap_or_default(),
            current_command,
            current_path: pane.foreground_cwd.or(pane.cwd).unwrap_or_default(),
            active: pane.focused,
            dead: false,
            tail,
            pane_pid,
            agent_hint: pane.agent,
        }
    }
}

impl PaneSource for HerdrSource {
    fn list_panes(
        &self,
        target: Option<&WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        let pending = self.fetch_pending()?;
        Ok(pending
            .into_iter()
            .filter(|p| target.is_none_or(|t| t.session_name == p.session_name))
            .map(|p| self.snapshot_for(p))
            .collect())
    }

    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        // Global monitor: no current-window narrowing. `None` means
        // "all panes" by the PaneSource contract.
        Ok(None)
    }

    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        let mut workspaces: Vec<HerdrWorkspace> = fetch(&workspace_list_args(), "workspaces")?;
        workspaces.sort_by_key(|w| w.number.unwrap_or(u32::MAX));
        Ok(workspaces
            .into_iter()
            .map(|w| WindowTarget {
                session_name: w.label.filter(|l| !l.is_empty()).unwrap_or(w.workspace_id),
                window_index: HERDR_TARGET_ALL.into(),
            })
            .collect())
    }

    fn capture_tail(&self, pane_id: &str, lines: usize) -> Result<String, PollingError> {
        let raw = run_herdr(&pane_read_args(pane_id))?;
        Ok(tail_last_lines(&raw, lines))
    }

    fn send_keys(&self, pane_id: &str, text: &str) -> Result<(), PollingError> {
        // Same two-step contract as the tmux backend: literal text
        // first (send-text never interprets key names), settle so
        // React/Ink CLIs ingest it, then the submit keystroke.
        run_herdr(&send_text_args(pane_id, text))?;
        thread::sleep(HERDR_KEY_SETTLE_DELAY);
        self.send_key(pane_id, HERDR_SUBMIT_KEY)?;
        thread::sleep(HERDR_KEY_SETTLE_DELAY);
        Ok(())
    }

    fn send_key(&self, pane_id: &str, key: &str) -> Result<(), PollingError> {
        run_herdr(&send_key_args(pane_id, key))?;
        thread::sleep(HERDR_KEY_SETTLE_DELAY);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::types::{HerdrPane, HerdrTab, HerdrWorkspace};

    fn ws(id: &str, label: &str, n: u32) -> HerdrWorkspace {
        HerdrWorkspace {
            workspace_id: id.into(),
            label: Some(label.into()),
            number: Some(n),
        }
    }

    fn tab(id: &str, ws: &str, label: &str, n: u32) -> HerdrTab {
        HerdrTab {
            tab_id: id.into(),
            workspace_id: ws.into(),
            label: Some(label.into()),
            number: Some(n),
        }
    }

    fn pane(id: &str, ws: &str, tab: &str, agent: Option<&str>) -> HerdrPane {
        HerdrPane {
            pane_id: id.into(),
            workspace_id: ws.into(),
            tab_id: tab.into(),
            agent: agent.map(Into::into),
            agent_status: None,
            cwd: Some("/home/u/proj".into()),
            foreground_cwd: Some("/home/u/proj/sub".into()),
            focused: false,
            label: None,
            terminal_title_stripped: Some("u@host: ~/proj".into()),
        }
    }

    #[test]
    fn assembles_only_agent_panes_plus_self_by_default() {
        let out = assemble_snapshots(
            &[ws("w1", "proj", 1)],
            &[tab("w1:t1", "w1", "1-Claude", 1)],
            &[
                pane("w1:p1", "w1", "w1:t1", Some("claude")),
                pane("w1:p2", "w1", "w1:t1", None),
                pane("w1:p3", "w1", "w1:t1", None),
            ],
            false,
            Some("w1:p2"),
        );
        // The plain shell p3 is excluded; p2 is included only as self.
        let ids: Vec<&str> = out.iter().map(|p| p.pane.pane_id.as_str()).collect();
        assert_eq!(ids, vec!["w1:p1", "w1:p2"]);
    }

    #[test]
    fn include_shell_panes_true_admits_plain_shells() {
        let out = assemble_snapshots(
            &[ws("w1", "proj", 1)],
            &[tab("w1:t1", "w1", "1-Claude", 1)],
            &[
                pane("w1:p1", "w1", "w1:t1", Some("claude")),
                pane("w1:p2", "w1", "w1:t1", None),
            ],
            true,
            None,
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn maps_workspace_and_tab_labels_with_id_fallbacks() {
        let unlabeled_ws = HerdrWorkspace {
            workspace_id: "w9".into(),
            label: None,
            number: None,
        };
        let unlabeled_tab = HerdrTab {
            tab_id: "w9:t7".into(),
            workspace_id: "w9".into(),
            label: None,
            number: None,
        };
        let out = assemble_snapshots(
            &[ws("w1", "Qmonster", 1), unlabeled_ws],
            &[tab("w1:t1", "w1", "1-Claude", 1), unlabeled_tab],
            &[
                pane("w1:p1", "w1", "w1:t1", Some("claude")),
                pane("w9:p9", "w9", "w9:t7", Some("codex")),
            ],
            false,
            None,
        );
        assert_eq!(out[0].session_name, "Qmonster");
        assert_eq!(out[0].window_index, "1-Claude");
        // Fallbacks: workspace_id verbatim; tab_id suffix after ':'.
        assert_eq!(out[1].session_name, "w9");
        assert_eq!(out[1].window_index, "t7");
    }

    #[test]
    fn sorts_by_workspace_then_tab_number_then_pane_id() {
        let out = assemble_snapshots(
            &[ws("w2", "beta", 2), ws("w1", "alpha", 1)],
            &[
                tab("w2:t1", "w2", "1-Claude", 1),
                tab("w1:t2", "w1", "2-Codex", 2),
                tab("w1:t1", "w1", "1-Claude", 1),
            ],
            &[
                pane("w2:p1", "w2", "w2:t1", Some("claude")),
                pane("w1:p5", "w1", "w1:t2", Some("codex")),
                pane("w1:p1", "w1", "w1:t1", Some("claude")),
            ],
            false,
            None,
        );
        let ids: Vec<&str> = out.iter().map(|p| p.pane.pane_id.as_str()).collect();
        assert_eq!(ids, vec!["w1:p1", "w1:p5", "w2:p1"]);
    }

    #[test]
    fn tail_last_lines_trims_trailing_blanks_then_bounds() {
        // Regression for the live 2026-07-28 finding: a codex pane
        // with a short transcript is top-anchored, so the raw read
        // ends in blank grid rows; the status line must survive a
        // 24-line tail bound.
        let mut text = String::new();
        for i in 0..10 {
            text.push_str(&format!("row {i}\n"));
        }
        text.push_str("Context 0% used · weekly 98% left\n");
        text.push_str("\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n");
        let tail = tail_last_lines(&text, 24);
        assert!(tail.ends_with("Context 0% used · weekly 98% left"));
        assert_eq!(tail.lines().count(), 11);
    }

    #[test]
    fn tail_last_lines_bounds_long_content_to_last_n() {
        let text = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tail = tail_last_lines(&text, 24);
        assert_eq!(tail.lines().count(), 24);
        assert!(tail.starts_with("line 76"));
        assert!(tail.ends_with("line 99"));
    }

    #[test]
    fn tail_last_lines_handles_all_blank_and_zero_bounds() {
        assert_eq!(tail_last_lines("\n\n\n", 24), "");
        assert_eq!(tail_last_lines("a\nb", 0), "b", "lines is clamped to >= 1");
    }

    #[test]
    fn unknown_workspace_or_tab_rows_still_assemble_with_fallbacks() {
        // A pane whose workspace/tab is missing from the list calls
        // (races happen — herdr list calls are not atomic) must not be
        // dropped: fall back to raw ids so the operator still sees it.
        let out = assemble_snapshots(
            &[],
            &[],
            &[pane("w3:p2", "w3", "w3:t9", Some("agy"))],
            false,
            None,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].session_name, "w3");
        assert_eq!(out[0].window_index, "t9");
    }
}
