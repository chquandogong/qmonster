//! Phase H (v1.42.0): opt-in actuation hook for the F-7c
//! `recommend_snapshot_before_reset` recommendation. When
//! `[reset] auto_snapshot = true`, an operator opts in to having
//! the snapshot written automatically — once per
//! `(pane_id, quota_kind, window_id)` triple. The dedup key is
//! the provider-reported `quota_*_resets_at` (rounded to the
//! nearest minute) so all ticks inside a single reset window
//! agree on the same key.

use crate::domain::recommendation::Recommendation;

/// Per-pane, per-quota-window dedup state for Phase H.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AutoSnapshotDedup {
    /// Last `quota_5h_resets_at` (rounded to minute) for which a
    /// snapshot was successfully written, or `None` if no snapshot
    /// has yet been written in the current 5h window.
    pub quota_5h_window_id: Option<u64>,
    /// Same as `quota_5h_window_id` but for the weekly window.
    pub quota_weekly_window_id: Option<u64>,
}

/// Quota window that fired the recommendation. Phase H tracks each
/// quota window's dedup independently so a single tick that fires
/// both can run two snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaKind {
    FiveHour,
    Weekly,
}

impl QuotaKind {
    #[allow(dead_code)] // used in Tasks 4-5 (Phase H)
    pub(crate) fn label(self) -> &'static str {
        match self {
            QuotaKind::FiveHour => "5h",
            QuotaKind::Weekly => "weekly",
        }
    }
}

impl AutoSnapshotDedup {
    /// Returns `true` if the given `(quota_kind, window_id)` has
    /// already been snapshotted in this window. Floor `window_id` to
    /// the nearest minute *before* calling so jitter on
    /// `quota_*_resets_at` doesn't break the match.
    pub fn matches(&self, kind: QuotaKind, window_id: u64) -> bool {
        match kind {
            QuotaKind::FiveHour => self.quota_5h_window_id == Some(window_id),
            QuotaKind::Weekly => self.quota_weekly_window_id == Some(window_id),
        }
    }

    /// Mark a successful snapshot for `(quota_kind, window_id)`.
    pub fn set(&mut self, kind: QuotaKind, window_id: u64) {
        match kind {
            QuotaKind::FiveHour => self.quota_5h_window_id = Some(window_id),
            QuotaKind::Weekly => self.quota_weekly_window_id = Some(window_id),
        }
    }
}

/// Round a `quota_*_resets_at` Unix timestamp (seconds) down to
/// the nearest minute. This absorbs sub-minute jitter from F-5b /
/// F-6 so two ticks emitted within the same window share the same
/// `window_id`.
pub fn floor_to_minute(unix_seconds: u64) -> u64 {
    unix_seconds - (unix_seconds % 60)
}

/// `Recommendation.action` constant emitted by
/// `recommend_snapshot_before_reset` for the 5h window.
const SNAPSHOT_5H_ACTION: &str = "snapshot before 5h window resets";

/// `Recommendation.action` constant emitted by
/// `recommend_snapshot_before_reset` for the weekly window.
const SNAPSHOT_WEEKLY_ACTION: &str = "snapshot before weekly window resets";

/// Returns `Some(QuotaKind)` when the recommendation is a
/// `snapshot_before_reset` advisory (5h or weekly), `None` otherwise.
/// Phase H acts only on these two action strings; every other
/// recommendation flows through unchanged.
///
/// See `policy::rules::reset::recommend_snapshot_before_reset` (F-7c)
/// for the rule that emits these action strings; the drift guard
/// test `action_constants_match_reset_rule_output` in this module
/// pins the constants against live rule output.
pub fn extract_quota_kind(rec: &Recommendation) -> Option<QuotaKind> {
    match rec.action {
        SNAPSHOT_5H_ACTION => Some(QuotaKind::FiveHour),
        SNAPSHOT_WEEKLY_ACTION => Some(QuotaKind::Weekly),
        _ => None,
    }
}

use crate::app::system_notice::SystemNotice;
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::store::sink::EventSink;
use crate::store::{SnapshotInput, SnapshotWriter};

/// Phase H entry point. Inspects `recs` for any
/// `snapshot_before_reset` recommendations and, when
/// `auto_snapshot_enabled` is true and the matching
/// `quota_*_resets_at` value is fresh (different from the dedup
/// state), writes a snapshot, records a `SnapshotWritten` audit
/// event, and pushes a Concern-severity `SystemNotice`. On
/// snapshot-writer failure, pushes a Warning `SystemNotice` and
/// leaves dedup untouched so the next tick can retry.
///
/// Pure over `dedup`, `writer`, `sink`, `notices` — none of the
/// callers' surfaces are mutated except via these arguments.
#[allow(clippy::too_many_arguments)]
pub fn maybe_auto_snapshot(
    pane_id: &str,
    recs: &[Recommendation],
    auto_snapshot_enabled: bool,
    quota_5h_resets_at: Option<u64>,
    quota_weekly_resets_at: Option<u64>,
    dedup: &mut AutoSnapshotDedup,
    writer: &SnapshotWriter,
    sink: &dyn EventSink,
    notices: &mut Vec<SystemNotice>,
) {
    if !auto_snapshot_enabled {
        return;
    }

    for rec in recs {
        let kind = match extract_quota_kind(rec) {
            Some(k) => k,
            None => continue,
        };

        let resets_at = match kind {
            QuotaKind::FiveHour => quota_5h_resets_at,
            QuotaKind::Weekly => quota_weekly_resets_at,
        };
        let resets_at = match resets_at {
            Some(v) => v,
            // Rule fired but signal absent — the rule body checks
            // `resets_at` itself, so this path should be unreachable
            // in practice. Be defensive and skip.
            None => continue,
        };
        let window_id = floor_to_minute(resets_at);

        if dedup.matches(kind, window_id) {
            continue;
        }

        let input = SnapshotInput {
            reason: format!(
                "auto_reset_boundary (kind={}, window_id={window_id})",
                kind.label()
            ),
            pane_summaries: vec![],
            notices: vec![],
        };

        match writer.write(&input) {
            Ok(path) => {
                sink.record(AuditEvent {
                    kind: AuditEventKind::SnapshotWritten,
                    pane_id: pane_id.to_string(),
                    severity: Severity::Safe,
                    summary: format!(
                        "auto-snapshot trigger=auto_reset_boundary quota_kind={} window_id={window_id} path={}",
                        kind.label(),
                        path.display()
                    ),
                    provider: None,
                    role: None,
                });
                notices.push(SystemNotice {
                    title: format!("auto-snapshot taken at {} reset boundary", kind.label()),
                    body: format!("see {}", path.display()),
                    severity: Severity::Concern,
                    source_kind: SourceKind::ProjectCanonical,
                });
                dedup.set(kind, window_id);
            }
            Err(e) => {
                notices.push(SystemNotice {
                    title: format!("auto-snapshot failed at {} reset boundary", kind.label()),
                    body: e.to_string(),
                    severity: Severity::Warning,
                    source_kind: SourceKind::ProjectCanonical,
                });
                // Do NOT update dedup — let the next tick retry.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::{Recommendation, Severity};

    fn make_rec(action: &'static str) -> Recommendation {
        Recommendation {
            action,
            reason: "test".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        }
    }

    #[test]
    fn extract_kind_returns_5h_for_5h_action() {
        let rec = make_rec(SNAPSHOT_5H_ACTION);
        assert_eq!(extract_quota_kind(&rec), Some(QuotaKind::FiveHour));
    }

    #[test]
    fn extract_kind_returns_weekly_for_weekly_action() {
        let rec = make_rec(SNAPSHOT_WEEKLY_ACTION);
        assert_eq!(extract_quota_kind(&rec), Some(QuotaKind::Weekly));
    }

    #[test]
    fn extract_kind_returns_none_for_unrelated_action() {
        let rec = make_rec("quota: pause until 5h window resets");
        assert_eq!(extract_quota_kind(&rec), None);
    }

    #[test]
    fn extract_kind_returns_none_for_arbitrary_text() {
        let rec = make_rec("log storm detected");
        assert_eq!(extract_quota_kind(&rec), None);
    }

    #[test]
    fn action_constants_match_reset_rule_output() {
        // Walk the actual reset rule with a hand-built SignalSet that
        // fires both 5h and weekly snapshot_before_reset advisories,
        // then assert the action strings still equal our constants.
        // This catches drift if `recommend_snapshot_before_reset` is
        // ever renamed or reworded without updating Phase H.
        use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};
        use crate::domain::signal::{MetricValue, SignalSet};
        use crate::policy::gates::PolicyGates;

        let id = crate::domain::identity::ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        };

        let now: u64 = 1_700_000_000;
        // Pressure ≥ 0.50 and resets_at within 300s → both snapshot rules fire.
        let signals = SignalSet {
            quota_5h_pressure: Some(MetricValue::new(0.95, SourceKind::ProviderOfficial)),
            quota_5h_resets_at: Some(MetricValue::new(now + 60, SourceKind::ProviderOfficial)),
            quota_weekly_pressure: Some(MetricValue::new(0.95, SourceKind::ProviderOfficial)),
            quota_weekly_resets_at: Some(MetricValue::new(now + 60, SourceKind::ProviderOfficial)),
            ..SignalSet::default()
        };

        let gates = PolicyGates {
            identity_confidence: IdentityConfidence::High,
            ..PolicyGates::default()
        };

        let recs = crate::policy::rules::reset::eval_reset(&id, &signals, &gates, now);
        let snapshot_actions: Vec<&str> = recs
            .iter()
            .filter(|r| r.action.starts_with("snapshot before"))
            .map(|r| r.action)
            .collect();
        assert!(
            snapshot_actions.contains(&SNAPSHOT_5H_ACTION),
            "5h action constant drifted from rule output: {snapshot_actions:?}"
        );
        assert!(
            snapshot_actions.contains(&SNAPSHOT_WEEKLY_ACTION),
            "weekly action constant drifted from rule output: {snapshot_actions:?}"
        );
    }

    #[test]
    fn dedup_default_has_no_window_ids() {
        let d = AutoSnapshotDedup::default();
        assert_eq!(d.quota_5h_window_id, None);
        assert_eq!(d.quota_weekly_window_id, None);
    }

    #[test]
    fn dedup_set_and_matches_round_trip() {
        let mut d = AutoSnapshotDedup::default();
        assert!(!d.matches(QuotaKind::FiveHour, 1_700_000_000));
        d.set(QuotaKind::FiveHour, 1_700_000_000);
        assert!(d.matches(QuotaKind::FiveHour, 1_700_000_000));
        assert!(!d.matches(QuotaKind::FiveHour, 1_700_000_060)); // different minute
    }

    #[test]
    fn dedup_5h_and_weekly_independent() {
        let mut d = AutoSnapshotDedup::default();
        d.set(QuotaKind::FiveHour, 1_700_000_000);
        assert!(d.matches(QuotaKind::FiveHour, 1_700_000_000));
        assert!(!d.matches(QuotaKind::Weekly, 1_700_000_000));
        d.set(QuotaKind::Weekly, 1_700_000_000);
        assert!(d.matches(QuotaKind::Weekly, 1_700_000_000));
        // Setting Weekly must NOT clear FiveHour — guards against a
        // copy-paste field-swap regression in `set`.
        assert!(d.matches(QuotaKind::FiveHour, 1_700_000_000));
    }

    #[test]
    fn floor_to_minute_drops_seconds() {
        // 1_700_000_037 % 60 == 57 → floors to 1_699_999_980.
        assert_eq!(floor_to_minute(1_700_000_037), 1_699_999_980);
        assert_eq!(floor_to_minute(60), 60);
        assert_eq!(floor_to_minute(59), 0);
        assert_eq!(floor_to_minute(120), 120);
        assert_eq!(floor_to_minute(121), 120);
    }

    use crate::app::system_notice::SystemNotice;
    use crate::domain::audit::AuditEventKind;
    use crate::store::paths::QmonsterPaths;
    use crate::store::{InMemorySink, SnapshotWriter};
    use tempfile::TempDir;

    fn make_snapshot_5h_rec() -> Recommendation {
        make_rec(SNAPSHOT_5H_ACTION)
    }
    fn make_snapshot_weekly_rec() -> Recommendation {
        make_rec(SNAPSHOT_WEEKLY_ACTION)
    }

    fn ctx_paths() -> (TempDir, QmonsterPaths) {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        (td, paths)
    }

    #[test]
    fn auto_snapshot_no_op_when_gate_off() {
        let (_td, paths) = ctx_paths();
        let writer = SnapshotWriter::new(paths.clone());
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec()];
        let now: u64 = 1_700_000_000;
        let resets_at: u64 = now + 60;

        maybe_auto_snapshot(
            "%1",
            &recs,
            /* auto_snapshot_enabled = */ false,
            Some(resets_at),
            None,
            &mut dedup,
            &writer,
            &sink,
            &mut notices,
        );

        assert!(notices.is_empty());
        assert!(sink.snapshot().is_empty());
        assert_eq!(dedup, AutoSnapshotDedup::default());
    }

    #[test]
    fn auto_snapshot_fires_once_per_window() {
        let (_td, paths) = ctx_paths();
        let writer = SnapshotWriter::new(paths.clone());
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec()];
        let resets_at: u64 = 1_700_000_060;

        // Tick 1: fires.
        maybe_auto_snapshot(
            "%1",
            &recs,
            true,
            Some(resets_at),
            None,
            &mut dedup,
            &writer,
            &sink,
            &mut notices,
        );
        // Tick 2: dedup blocks.
        maybe_auto_snapshot(
            "%1",
            &recs,
            true,
            Some(resets_at),
            None,
            &mut dedup,
            &writer,
            &sink,
            &mut notices,
        );

        let events = sink.snapshot();
        let snapshot_events: Vec<_> = events
            .iter()
            .filter(|e| e.kind == AuditEventKind::SnapshotWritten)
            .collect();
        assert_eq!(snapshot_events.len(), 1, "dedup should block tick 2");
        assert_eq!(notices.len(), 1, "one notice per actuation");
    }

    #[test]
    fn auto_snapshot_5h_and_weekly_independent() {
        let (_td, paths) = ctx_paths();
        let writer = SnapshotWriter::new(paths.clone());
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec(), make_snapshot_weekly_rec()];
        let resets_5h: u64 = 1_700_000_060;
        let resets_weekly: u64 = 1_700_500_060;

        maybe_auto_snapshot(
            "%1",
            &recs,
            true,
            Some(resets_5h),
            Some(resets_weekly),
            &mut dedup,
            &writer,
            &sink,
            &mut notices,
        );

        let events = sink.snapshot();
        let snapshot_events: Vec<_> = events
            .iter()
            .filter(|e| e.kind == AuditEventKind::SnapshotWritten)
            .collect();
        assert_eq!(snapshot_events.len(), 2, "both kinds run on the same tick");
        assert_eq!(notices.len(), 2);
        assert_eq!(dedup.quota_5h_window_id, Some(floor_to_minute(resets_5h)));
        assert_eq!(
            dedup.quota_weekly_window_id,
            Some(floor_to_minute(resets_weekly))
        );
    }

    #[test]
    fn auto_snapshot_new_window_resets_dedup() {
        let (_td, paths) = ctx_paths();
        let writer = SnapshotWriter::new(paths.clone());
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec()];
        // First window.
        maybe_auto_snapshot(
            "%1",
            &recs,
            true,
            Some(1_700_000_060),
            None,
            &mut dedup,
            &writer,
            &sink,
            &mut notices,
        );
        // Second window — new resets_at differs by more than a minute.
        maybe_auto_snapshot(
            "%1",
            &recs,
            true,
            Some(1_700_018_000),
            None,
            &mut dedup,
            &writer,
            &sink,
            &mut notices,
        );

        let events = sink.snapshot();
        let snapshot_events: Vec<_> = events
            .iter()
            .filter(|e| e.kind == AuditEventKind::SnapshotWritten)
            .collect();
        assert_eq!(
            snapshot_events.len(),
            2,
            "second window allows a fresh snapshot"
        );
    }

    #[test]
    fn auto_snapshot_failure_does_not_dedup() {
        // SnapshotWriter pointed at a path whose snapshot_dir creation
        // succeeds but write fails — same trick as
        // `write_operator_snapshot_failure_returns_warning_without_audit`
        // in src/app/operator_actions.rs.
        use tempfile::NamedTempFile;
        let file = NamedTempFile::new().unwrap();
        let writer = SnapshotWriter::new(QmonsterPaths::at(file.path()));
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec()];
        maybe_auto_snapshot(
            "%1",
            &recs,
            true,
            Some(1_700_000_060),
            None,
            &mut dedup,
            &writer,
            &sink,
            &mut notices,
        );

        assert!(
            sink.snapshot().is_empty(),
            "no audit event on write failure"
        );
        assert_eq!(
            dedup,
            AutoSnapshotDedup::default(),
            "dedup must NOT update on failure"
        );
        assert_eq!(notices.len(), 1, "operator sees a Warning notice");
        assert_eq!(
            notices[0].severity,
            crate::domain::recommendation::Severity::Warning
        );
    }
}
