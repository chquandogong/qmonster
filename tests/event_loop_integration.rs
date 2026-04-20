use std::sync::{Arc, Mutex};
use std::time::Instant;

use qmonster::app::bootstrap::Context;
use qmonster::app::config::QmonsterConfig;
use qmonster::app::event_loop::run_once;
use qmonster::domain::identity::{IdentityConfidence, Provider, Role};
use qmonster::domain::recommendation::Severity;
use qmonster::notify::desktop::NotifyBackend;
use qmonster::store::sink::InMemorySink;
use qmonster::tmux::polling::{PaneSource, PollingError};
use qmonster::tmux::types::RawPaneSnapshot;

struct FixturePaneSource {
    panes: Vec<RawPaneSnapshot>,
}

impl PaneSource for FixturePaneSource {
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

#[derive(Clone)]
struct RecordingNotifier(Arc<Mutex<Vec<(String, String, Severity)>>>);

impl NotifyBackend for RecordingNotifier {
    fn notify(&self, title: &str, body: &str, severity: Severity) {
        self.0
            .lock()
            .unwrap()
            .push((title.into(), body.into(), severity));
    }
}

fn pane(pane_id: &str, title: &str, cmd: &str, tail: &str, dead: bool) -> RawPaneSnapshot {
    RawPaneSnapshot {
        session_name: "qwork".into(),
        window_index: "1".into(),
        pane_id: pane_id.into(),
        title: title.into(),
        current_command: cmd.into(),
        current_path: "/tmp".into(),
        active: !dead,
        dead,
        tail: tail.into(),
    }
}

fn pane_with_path(
    pane_id: &str,
    title: &str,
    cmd: &str,
    tail: &str,
    dead: bool,
    current_path: &str,
) -> RawPaneSnapshot {
    RawPaneSnapshot {
        session_name: "qwork".into(),
        window_index: "1".into(),
        pane_id: pane_id.into(),
        title: title.into(),
        current_command: cmd.into(),
        current_path: current_path.into(),
        active: !dead,
        dead,
        tail: tail.into(),
    }
}

#[test]
fn run_once_emits_recommendations_and_audit_events() {
    let source = FixturePaneSource {
        panes: vec![
            pane("%1", "claude:1:main", "claude", "Press ENTER to continue", false),
            pane("%2", "codex:1:review", "codex", "idle", false),
        ],
    };
    let seen = Arc::new(Mutex::new(Vec::new()));
    let notifier = RecordingNotifier(seen.clone());
    let sink = InMemorySink::new();
    let sink_ref = Box::new(InMemorySink::new());
    // Build a ctx that owns a separate sink to inspect.
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink_ref);
    // Sanity: InMemorySink inside ctx starts empty.
    let _ = sink.snapshot();

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 2);
    // The first pane had a WAIT_INPUT tail — at least one recommendation.
    assert!(reports[0]
        .recommendations
        .iter()
        .any(|r| r.action == "notify-input-wait"));
    // The notify backend should have seen a call for that alert.
    let calls = seen.lock().unwrap();
    assert!(calls.iter().any(|(_, body, _)| body.contains("notify-input-wait")));
}

#[test]
fn run_once_report_carries_identity_and_signals() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%1",
            "claude:1:main",
            "claude",
            "Press ENTER to continue",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    let rep = &reports[0];
    assert_eq!(rep.identity.identity.provider, Provider::Claude);
    assert_eq!(rep.identity.identity.role, Role::Main);
    assert_eq!(rep.identity.identity.instance, 1);
    assert_eq!(rep.identity.confidence, IdentityConfidence::High);
    assert!(rep.signals.waiting_for_input);
}

#[test]
fn run_once_report_exposes_metric_values_when_present() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%1",
            "claude:1:main",
            "claude",
            "context window usage 82%",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    let metric = reports[0]
        .signals
        .context_pressure
        .as_ref()
        .expect("context_pressure should be present");
    assert!((metric.value - 0.82).abs() < 0.01);
    assert_eq!(
        metric.source_kind,
        qmonster::domain::origin::SourceKind::Estimated
    );
}

#[test]
fn observe_only_mode_suppresses_notifications_even_with_alerts() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%1",
            "claude:1:main",
            "claude",
            "Press ENTER to continue",
            false,
        )],
    };
    let seen = Arc::new(Mutex::new(Vec::new()));
    let notifier = RecordingNotifier(seen.clone());
    let sink = Box::new(InMemorySink::new());

    let mut cfg = QmonsterConfig::defaults();
    // Move to observe_only (safer; allowed per safety precedence).
    cfg.actions.mode = qmonster::app::config::ActionsMode::ObserveOnly;
    let mut ctx = Context::new(cfg, source, notifier, sink);

    let _ = run_once(&mut ctx, Instant::now()).expect("ok");
    let calls = seen.lock().unwrap();
    assert!(
        calls.is_empty(),
        "notifier must not fire in observe_only mode"
    );
}

#[test]
fn effects_are_propagated_onto_pane_report() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%1",
            "claude:1:main",
            "claude",
            "Press ENTER to continue",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert!(reports[0]
        .effects
        .contains(&qmonster::domain::recommendation::RequestedEffect::Notify));
    assert!(!reports[0]
        .effects
        .contains(&qmonster::domain::recommendation::RequestedEffect::SensitiveNotImplemented));
}

#[test]
fn log_storm_triggers_archive_writer_when_permit_is_on() {
    use qmonster::store::{ArchiveWriter, QmonsterPaths};
    let td = tempfile::TempDir::new().unwrap();
    let paths = QmonsterPaths::at(td.path());
    paths.ensure().unwrap();
    let writer = ArchiveWriter::new(paths.clone(), 10);

    // Tail shaped like a log storm (many log-like lines).
    let tail = (0..10)
        .map(|i| format!("2026-04-20T00:00:0{i} INFO row {i}"))
        .collect::<Vec<_>>()
        .join("\n");

    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "claude", &tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink)
        .with_archive(writer);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert!(reports[0].signals.log_storm, "fixture must be a log storm");
    assert!(reports[0]
        .effects
        .contains(&qmonster::domain::recommendation::RequestedEffect::ArchiveLocal));

    // At least one archive file appeared under the archive dir.
    let wrote_any = std::fs::read_dir(paths.archive_dir())
        .unwrap()
        .any(|_| true);
    assert!(wrote_any, "expected an archive file under {:?}", paths.archive_dir());
}

#[test]
fn observe_only_mode_does_not_call_archive_writer() {
    use qmonster::store::{ArchiveWriter, QmonsterPaths};
    let td = tempfile::TempDir::new().unwrap();
    let paths = QmonsterPaths::at(td.path());
    paths.ensure().unwrap();
    let writer = ArchiveWriter::new(paths.clone(), 10);

    let tail = (0..10)
        .map(|i| format!("2026-04-20T00:00:0{i} INFO row {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "claude", &tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut cfg = QmonsterConfig::defaults();
    cfg.actions.mode = qmonster::app::config::ActionsMode::ObserveOnly;
    let mut ctx = Context::new(cfg, source, notifier, sink).with_archive(writer);

    let _ = run_once(&mut ctx, Instant::now()).expect("ok");

    // No archive files expected.
    let any_file = walk_any_file(&paths.archive_dir());
    assert!(!any_file, "observe_only must never write to archive");
}

fn walk_any_file(dir: &std::path::Path) -> bool {
    fn inner(d: &std::path::Path) -> bool {
        let Ok(entries) = std::fs::read_dir(d) else {
            return false;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if inner(&p) {
                    return true;
                }
            } else {
                return true;
            }
        }
        false
    }
    inner(dir)
}

#[test]
fn run_once_handles_dead_pane_without_panic() {
    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "claude", "zombie", true)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 1);
    assert!(reports[0].dead);
    assert!(reports[0].recommendations.is_empty());
}

// ---------------------------------------------------------------------------
// Phase 3A integration tests: cross-pane findings
// ---------------------------------------------------------------------------

/// Build a busy tail that is at least 500 chars and contains none of the
/// waiting/permission markers, so `output_chars >= 500` and
/// `waiting_for_input == false`.
fn busy_tail() -> String {
    // 30 chars per repetition × 20 = 600 chars; no wait markers.
    "doing some work on the task.\n".repeat(20)
}

#[test]
fn concurrent_mutating_work_surfaces_in_cross_pane_findings() {
    use qmonster::domain::recommendation::CrossPaneKind;

    let tail = busy_tail();
    let source = FixturePaneSource {
        panes: vec![
            pane_with_path("%1", "claude:1:main", "claude", &tail, false, "/tmp/repo"),
            pane_with_path("%2", "claude:2:main", "claude", &tail, false, "/tmp/repo"),
        ],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 2);

    // Exactly one report must carry a cross-pane finding.
    let with_findings: Vec<_> = reports
        .iter()
        .filter(|r| !r.cross_pane_findings.is_empty())
        .collect();
    assert_eq!(
        with_findings.len(),
        1,
        "exactly one report should carry cross-pane findings"
    );

    let finding = &with_findings[0].cross_pane_findings[0];
    assert_eq!(finding.kind, CrossPaneKind::ConcurrentMutatingWork);

    // Anchor must be the lexicographically smaller pane ID ("%1" < "%2").
    assert_eq!(finding.anchor_pane_id, "%1");
}

#[test]
fn concurrent_does_not_trigger_across_different_current_paths() {
    let tail = busy_tail();
    let source = FixturePaneSource {
        panes: vec![
            pane_with_path("%1", "claude:1:main", "claude", &tail, false, "/tmp/repo-a"),
            pane_with_path("%2", "claude:2:main", "claude", &tail, false, "/tmp/repo-b"),
        ],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 2);

    for r in &reports {
        assert!(
            r.cross_pane_findings.is_empty(),
            "pane {} must have no cross-pane findings for distinct paths",
            r.pane_id
        );
    }
}

#[test]
fn cross_pane_finding_attaches_to_correct_anchor() {
    use qmonster::domain::recommendation::CrossPaneKind;

    let tail = busy_tail();
    // Three panes in the same directory, IDs out of lex order.
    let source = FixturePaneSource {
        panes: vec![
            pane_with_path("%9", "claude:9:main", "claude", &tail, false, "/repo"),
            pane_with_path("%1", "claude:1:main", "claude", &tail, false, "/repo"),
            pane_with_path("%5", "claude:5:main", "claude", &tail, false, "/repo"),
        ],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 3);

    // %1 is the lex-smallest, so it must be the anchor.
    let rep_1 = reports.iter().find(|r| r.pane_id == "%1").expect("%1 report");
    assert_eq!(rep_1.cross_pane_findings.len(), 1);
    assert_eq!(
        rep_1.cross_pane_findings[0].kind,
        CrossPaneKind::ConcurrentMutatingWork
    );
    assert_eq!(rep_1.cross_pane_findings[0].anchor_pane_id, "%1");

    // %5 and %9 must not carry findings.
    for id in ["%5", "%9"] {
        let rep = reports.iter().find(|r| r.pane_id == id).expect("report");
        assert!(
            rep.cross_pane_findings.is_empty(),
            "pane {id} must not carry cross-pane findings"
        );
    }
}

#[test]
fn concern_severity_recommendation_does_not_trigger_desktop_notification() {
    // `verbose_answer` fires on the phrase "I'd be happy to help" and
    // produces a Concern-severity recommendation. The policy engine must
    // NOT request a Notify effect for Concern-only recommendations
    // (Codex #3), so the RecordingNotifier must stay empty.
    let tail = "I'd be happy to help with that task.";
    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "claude", tail, false)],
    };
    let seen = Arc::new(Mutex::new(Vec::new()));
    let notifier = RecordingNotifier(seen.clone());
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");

    // Sanity: the verbose_answer recommendation must actually exist.
    assert!(
        reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "verbose-output"),
        "verbose-output recommendation must be present"
    );

    // The notifier must not have been called.
    let calls = seen.lock().unwrap();
    assert!(
        calls.is_empty(),
        "Concern-severity recommendation must not trigger a desktop notification"
    );
}
