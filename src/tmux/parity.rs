use std::collections::{BTreeMap, BTreeSet};

use crate::tmux::polling::{PaneSource, PollingError};
use crate::tmux::types::{RawPaneSnapshot, WindowTarget};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PaneParityKey {
    pub session_name: String,
    pub window_index: String,
    pub pane_id: String,
}

impl PaneParityKey {
    fn from_snapshot(snapshot: &RawPaneSnapshot) -> Self {
        Self {
            session_name: snapshot.session_name.clone(),
            window_index: snapshot.window_index.clone(),
            pane_id: snapshot.pane_id.clone(),
        }
    }

    pub fn label(&self) -> String {
        format!(
            "{}:{} {}",
            self.session_name, self.window_index, self.pane_id
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneFieldMismatch {
    pub key: PaneParityKey,
    pub field: &'static str,
    pub polling: String,
    pub control_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailMismatch {
    pub key: PaneParityKey,
    pub polling_lines: usize,
    pub control_mode_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxSourceParityReport {
    pub target: Option<WindowTarget>,
    pub polling_current_target: Option<WindowTarget>,
    pub control_mode_current_target: Option<WindowTarget>,
    pub only_polling_targets: Vec<WindowTarget>,
    pub only_control_mode_targets: Vec<WindowTarget>,
    pub only_polling_panes: Vec<PaneParityKey>,
    pub only_control_mode_panes: Vec<PaneParityKey>,
    pub title_mismatches: Vec<PaneFieldMismatch>,
    pub pane_mismatches: Vec<PaneFieldMismatch>,
    pub tail_mismatches: Vec<TailMismatch>,
    pub polling_pane_count: usize,
    pub control_mode_pane_count: usize,
}

impl TmuxSourceParityReport {
    pub fn structural_mismatch_count(&self) -> usize {
        usize::from(self.polling_current_target != self.control_mode_current_target)
            + self.only_polling_targets.len()
            + self.only_control_mode_targets.len()
            + self.only_polling_panes.len()
            + self.only_control_mode_panes.len()
            + self.pane_mismatches.len()
    }

    pub fn passes(&self, strict_tail: bool) -> bool {
        self.passes_with_options(strict_tail, false)
    }

    pub fn passes_with_options(&self, strict_tail: bool, strict_title: bool) -> bool {
        self.structural_mismatch_count() == 0
            && (!strict_tail || self.tail_mismatches.is_empty())
            && (!strict_title || self.title_mismatches.is_empty())
    }
}

pub fn compare_pane_sources<P, C>(
    polling: &P,
    control_mode: &C,
    target: Option<&WindowTarget>,
    capture_lines: usize,
) -> Result<TmuxSourceParityReport, PollingError>
where
    P: PaneSource,
    C: PaneSource,
{
    let polling_current_target = polling.current_target()?;
    let control_mode_current_target = control_mode.current_target()?;

    let polling_targets = sorted_targets(polling.available_targets()?);
    let control_mode_targets = sorted_targets(control_mode.available_targets()?);
    let polling_target_set: BTreeSet<_> = polling_targets.iter().cloned().collect();
    let control_mode_target_set: BTreeSet<_> = control_mode_targets.iter().cloned().collect();

    let polling_panes = polling.list_panes(target)?;
    let control_mode_panes = control_mode.list_panes(target)?;
    let polling_pane_count = polling_panes.len();
    let control_mode_pane_count = control_mode_panes.len();
    let polling_by_key = snapshots_by_key(polling_panes);
    let control_mode_by_key = snapshots_by_key(control_mode_panes);
    let polling_keys: BTreeSet<_> = polling_by_key.keys().cloned().collect();
    let control_mode_keys: BTreeSet<_> = control_mode_by_key.keys().cloned().collect();

    let mut title_mismatches = Vec::new();
    let mut pane_mismatches = Vec::new();
    let mut tail_mismatches = Vec::new();
    for key in polling_keys.intersection(&control_mode_keys) {
        let polling_snapshot = &polling_by_key[key];
        let control_mode_snapshot = &control_mode_by_key[key];
        compare_field(
            &mut title_mismatches,
            key,
            "title",
            &polling_snapshot.title,
            &control_mode_snapshot.title,
        );
        compare_field(
            &mut pane_mismatches,
            key,
            "current_command",
            &polling_snapshot.current_command,
            &control_mode_snapshot.current_command,
        );
        compare_field(
            &mut pane_mismatches,
            key,
            "current_path",
            &polling_snapshot.current_path,
            &control_mode_snapshot.current_path,
        );
        compare_field(
            &mut pane_mismatches,
            key,
            "active",
            &polling_snapshot.active.to_string(),
            &control_mode_snapshot.active.to_string(),
        );
        compare_field(
            &mut pane_mismatches,
            key,
            "dead",
            &polling_snapshot.dead.to_string(),
            &control_mode_snapshot.dead.to_string(),
        );

        let polling_tail = normalize_tail(&polling.capture_tail(&key.pane_id, capture_lines)?);
        let control_mode_tail =
            normalize_tail(&control_mode.capture_tail(&key.pane_id, capture_lines)?);
        if polling_tail != control_mode_tail {
            tail_mismatches.push(TailMismatch {
                key: key.clone(),
                polling_lines: line_count(&polling_tail),
                control_mode_lines: line_count(&control_mode_tail),
            });
        }
    }

    Ok(TmuxSourceParityReport {
        target: target.cloned(),
        polling_current_target,
        control_mode_current_target,
        only_polling_targets: polling_target_set
            .difference(&control_mode_target_set)
            .cloned()
            .collect(),
        only_control_mode_targets: control_mode_target_set
            .difference(&polling_target_set)
            .cloned()
            .collect(),
        only_polling_panes: polling_keys
            .difference(&control_mode_keys)
            .cloned()
            .collect(),
        only_control_mode_panes: control_mode_keys
            .difference(&polling_keys)
            .cloned()
            .collect(),
        title_mismatches,
        pane_mismatches,
        tail_mismatches,
        polling_pane_count,
        control_mode_pane_count,
    })
}

pub fn compare_all_pane_source_targets<P, C>(
    polling: &P,
    control_mode: &C,
    capture_lines: usize,
) -> Result<Vec<TmuxSourceParityReport>, PollingError>
where
    P: PaneSource,
    C: PaneSource,
{
    let targets = all_targets(
        polling.available_targets()?,
        control_mode.available_targets()?,
    );
    targets
        .iter()
        .map(|target| compare_pane_sources(polling, control_mode, Some(target), capture_lines))
        .collect()
}

fn all_targets(
    polling_targets: Vec<WindowTarget>,
    control_mode_targets: Vec<WindowTarget>,
) -> Vec<WindowTarget> {
    polling_targets
        .into_iter()
        .chain(control_mode_targets)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn sorted_targets(mut targets: Vec<WindowTarget>) -> Vec<WindowTarget> {
    targets.sort();
    targets.dedup();
    targets
}

fn snapshots_by_key(snapshots: Vec<RawPaneSnapshot>) -> BTreeMap<PaneParityKey, RawPaneSnapshot> {
    snapshots
        .into_iter()
        .map(|snapshot| (PaneParityKey::from_snapshot(&snapshot), snapshot))
        .collect()
}

fn compare_field(
    mismatches: &mut Vec<PaneFieldMismatch>,
    key: &PaneParityKey,
    field: &'static str,
    polling: &str,
    control_mode: &str,
) {
    if polling != control_mode {
        mismatches.push(PaneFieldMismatch {
            key: key.clone(),
            field,
            polling: polling.into(),
            control_mode: control_mode.into(),
        });
    }
}

fn normalize_tail(tail: &str) -> String {
    tail.replace("\r\n", "\n")
        .trim_end_matches(['\r', '\n'])
        .to_string()
}

fn line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestSource {
        panes: Vec<RawPaneSnapshot>,
        current_target: Option<WindowTarget>,
        targets: Vec<WindowTarget>,
    }

    impl TestSource {
        fn new(panes: Vec<RawPaneSnapshot>) -> Self {
            let targets = sorted_targets(
                panes
                    .iter()
                    .map(|pane| WindowTarget {
                        session_name: pane.session_name.clone(),
                        window_index: pane.window_index.clone(),
                    })
                    .collect(),
            );
            Self {
                current_target: targets.first().cloned(),
                targets,
                panes,
            }
        }

        fn with_targets(mut self, targets: Vec<WindowTarget>) -> Self {
            self.current_target = targets.first().cloned();
            self.targets = sorted_targets(targets);
            self
        }
    }

    impl PaneSource for TestSource {
        fn list_panes(
            &self,
            _target: Option<&WindowTarget>,
        ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
            Ok(self.panes.clone())
        }

        fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
            Ok(self.current_target.clone())
        }

        fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
            Ok(self.targets.clone())
        }

        fn capture_tail(&self, pane_id: &str, _lines: usize) -> Result<String, PollingError> {
            Ok(self
                .panes
                .iter()
                .find(|pane| pane.pane_id == pane_id)
                .map(|pane| pane.tail.clone())
                .unwrap_or_default())
        }

        fn send_keys(&self, _pane_id: &str, _text: &str) -> Result<(), PollingError> {
            Ok(())
        }
    }

    #[test]
    fn matching_sources_pass_with_trailing_tail_newline_difference() {
        let polling = TestSource::new(vec![pane("%1", "claude", "tail\n")]);
        let control_mode = TestSource::new(vec![pane("%1", "claude", "tail")]);

        let report = compare_pane_sources(&polling, &control_mode, None, 24).unwrap();

        assert!(report.passes(true), "{report:#?}");
        assert_eq!(report.polling_pane_count, 1);
        assert_eq!(report.control_mode_pane_count, 1);
    }

    #[test]
    fn reports_structural_and_tail_mismatches() {
        let polling = TestSource::new(vec![pane("%1", "claude", "old tail")]);
        let mut control_pane = pane("%1", "codex", "new tail\nextra");
        control_pane.active = false;
        let control_mode = TestSource::new(vec![control_pane, pane("%2", "gemini", "")]);

        let report = compare_pane_sources(&polling, &control_mode, None, 24).unwrap();

        assert!(!report.passes(false));
        assert_eq!(report.only_control_mode_panes.len(), 1);
        assert_eq!(report.pane_mismatches.len(), 2);
        assert_eq!(report.tail_mismatches.len(), 1);
        assert_eq!(report.structural_mismatch_count(), 3);
    }

    #[test]
    fn title_mismatches_warn_by_default_and_fail_when_strict() {
        let polling = TestSource::new(vec![pane("%1", "claude", "tail")]);
        let mut control_pane = pane("%1", "claude", "tail");
        control_pane.title = "qmonster changed".into();
        let control_mode = TestSource::new(vec![control_pane]);

        let report = compare_pane_sources(&polling, &control_mode, None, 24).unwrap();

        assert!(report.passes(false), "{report:#?}");
        assert!(!report.passes_with_options(false, true));
        assert_eq!(report.title_mismatches.len(), 1);
        assert_eq!(report.structural_mismatch_count(), 0);
    }

    #[test]
    fn compare_all_targets_runs_the_union_of_available_targets() {
        let qmonster = target("qmonster", "0");
        let scratch = target("scratch", "1");
        let polling = TestSource::new(vec![pane("%1", "claude", "tail")])
            .with_targets(vec![qmonster.clone()]);
        let control_mode =
            TestSource::new(vec![pane("%1", "claude", "tail")]).with_targets(vec![scratch]);

        let reports = compare_all_pane_source_targets(&polling, &control_mode, 24).unwrap();

        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].target, Some(qmonster));
        assert!(
            reports
                .iter()
                .all(|report| report.structural_mismatch_count() > 0)
        );
    }

    fn pane(pane_id: &str, command: &str, tail: &str) -> RawPaneSnapshot {
        RawPaneSnapshot {
            session_name: "qmonster".into(),
            window_index: "0".into(),
            pane_id: pane_id.into(),
            title: "qmonster".into(),
            current_command: command.into(),
            current_path: "/tmp/qmonster".into(),
            active: true,
            dead: false,
            tail: tail.into(),
        }
    }

    fn target(session_name: &str, window_index: &str) -> WindowTarget {
        WindowTarget {
            session_name: session_name.into(),
            window_index: window_index.into(),
        }
    }
}
