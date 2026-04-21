use std::sync::{Arc, Mutex};
use std::time::Instant;

use qmonster::app::bootstrap::Context;
use qmonster::app::config::QmonsterConfig;
use qmonster::app::event_loop::run_once;
use qmonster::domain::audit::{AuditEvent, AuditEventKind};
use qmonster::domain::identity::{IdentityConfidence, Provider, Role};
use qmonster::domain::recommendation::Severity;
use qmonster::notify::desktop::NotifyBackend;
use qmonster::store::sink::{EventSink, InMemorySink};
use qmonster::tmux::polling::{PaneSource, PollingError};
use qmonster::tmux::types::{RawPaneSnapshot, WindowTarget};

/// Audit sink that captures events into a shared buffer for test inspection.
#[derive(Clone)]
struct CaptureSink(Arc<Mutex<Vec<AuditEvent>>>);

impl CaptureSink {
    fn new() -> Self {
        CaptureSink(Arc::new(Mutex::new(Vec::new())))
    }

    fn snapshot(&self) -> Vec<AuditEvent> {
        self.0.lock().unwrap().clone()
    }
}

impl EventSink for CaptureSink {
    fn record(&self, event: AuditEvent) {
        self.0.lock().unwrap().push(event);
    }
}

struct FixturePaneSource {
    panes: Vec<RawPaneSnapshot>,
}

impl PaneSource for FixturePaneSource {
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
            pane(
                "%1",
                "claude:1:main",
                "claude",
                "Press ENTER to continue",
                false,
            ),
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
    assert!(
        reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "notify-input-wait")
    );
    // The notify backend should have seen a call for that alert.
    let calls = seen.lock().unwrap();
    assert!(
        calls
            .iter()
            .any(|(_, body, _)| body.contains("notify-input-wait"))
    );
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
    assert!(
        reports[0]
            .effects
            .contains(&qmonster::domain::recommendation::RequestedEffect::Notify)
    );
    assert!(
        !reports[0]
            .effects
            .contains(&qmonster::domain::recommendation::RequestedEffect::SensitiveNotImplemented)
    );
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
    let mut ctx =
        Context::new(QmonsterConfig::defaults(), source, notifier, sink).with_archive(writer);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert!(reports[0].signals.log_storm, "fixture must be a log storm");
    assert!(
        reports[0]
            .effects
            .contains(&qmonster::domain::recommendation::RequestedEffect::ArchiveLocal)
    );

    // At least one archive file appeared under the archive dir.
    let wrote_any = std::fs::read_dir(paths.archive_dir())
        .unwrap()
        .any(|_| true);
    assert!(
        wrote_any,
        "expected an archive file under {:?}",
        paths.archive_dir()
    );
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
// Phase 3B integration tests: strong-rec UX (G-7)
// ---------------------------------------------------------------------------

#[test]
fn context_pressure_rec_is_marked_strong_end_to_end() {
    // Fixture: Claude Main pane whose tail contains "context window usage 82%"
    // → triggers context_pressure_warning (severity Warning, is_strong true).
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
    assert_eq!(reports.len(), 1);

    let rec = reports[0]
        .recommendations
        .iter()
        .find(|r| r.action == "context-pressure: checkpoint")
        .expect("context_pressure_warning must fire for 82% pressure");

    assert!(
        rec.is_strong,
        "context-pressure: checkpoint must be marked is_strong"
    );

    // Codex v1.7.3 finding #1: suggested_command must be a pure,
    // runnable in-pane command — exact `/compact`, not a mixed-mode
    // prose string that would render as an impossible `run: `#
    // press 's' ...`` in the UI / --once.
    assert_eq!(
        rec.suggested_command.as_deref(),
        Some("/compact"),
        "strong-rec contract: suggested_command is a runnable /compact; \
         snapshot precondition belongs in next_step"
    );

    // Codex v1.7.3 finding #2: the snapshot precondition is locked in a
    // structurally separate field. Ordering is inherent (next_step
    // always precedes suggested_command in render) so there is no
    // substring-ordering loophole to exploit.
    let step = rec
        .next_step
        .as_deref()
        .expect("strong rec must carry a next_step explaining the snapshot precondition");
    assert!(
        step.contains("snapshot"),
        "next_step must describe the snapshot precondition. got: {step}"
    );

    // Regression guard on the rendered format: the live format helper
    // must emit `next: … — run: `/compact`` in that order. Any shape
    // drift (dropping the `next:` segment, swapping the order, re-
    // introducing mixed-mode prose in suggested_command) fails here.
    let body = qmonster::ui::alerts::format_strong_rec_body(rec, &reports[0].pane_id);
    let next_idx = body
        .find("next: ")
        .expect("rendered body must contain a `next: ` segment");
    let run_idx = body
        .find("run: `/compact`")
        .expect("rendered body must contain a `run: `/compact`` segment");
    assert!(
        next_idx < run_idx,
        "ordering contract: `next:` precedes `run:` in the render. body: {body}"
    );
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
    let rep_1 = reports
        .iter()
        .find(|r| r.pane_id == "%1")
        .expect("%1 report");
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

#[test]
fn dispatch_notify_filters_concern_when_warning_coexists() {
    // Tail contains BOTH:
    //   "Press ENTER to continue" → waiting_for_input → Warning → notify-input-wait
    //   "I'd be happy to help"   → verbose_answer     → Concern → verbose-output
    // The policy engine emits RequestedEffect::Notify (because Warning rec exists).
    // dispatch_notify must fire the notifier ONLY for the Warning-severity rec,
    // NOT for the Concern-severity rec (Codex Phase-3A finding #1).
    let tail = "I'd be happy to help with that.\nPress ENTER to continue";
    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "claude", tail, false)],
    };
    let seen = Arc::new(Mutex::new(Vec::new()));
    let notifier = RecordingNotifier(seen.clone());
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");

    // Sanity: both recommendations must be present in the report.
    assert!(
        reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "notify-input-wait"),
        "notify-input-wait (Warning) rec must be present"
    );
    assert!(
        reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "verbose-output"),
        "verbose-output (Concern) rec must be present"
    );

    // dispatch_notify must have fired exactly once — for the Warning rec only.
    let calls = seen.lock().unwrap();
    assert_eq!(
        calls.len(),
        1,
        "exactly one notification expected (Warning only, not Concern)"
    );
    // The single notification corresponds to the Warning-severity rec.
    assert_eq!(
        calls[0].2,
        Severity::Warning,
        "the sole notification must carry Warning severity"
    );
}

#[test]
fn concern_severity_rec_audit_logged_as_recommendation_emitted() {
    // Pane tail triggers ONLY verbose_answer (Concern-severity, "verbose-output" action).
    // alert_event must log it as RecommendationEmitted, NOT AlertFired
    // (Codex Phase-3A finding #2).
    let tail = "I'd be happy to help with that task.";
    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "claude", tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let capture = CaptureSink::new();
    let mut ctx = Context::new(
        QmonsterConfig::defaults(),
        source,
        notifier,
        Box::new(capture.clone()),
    );

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");

    // Sanity: verbose-output rec must be present.
    assert!(
        reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "verbose-output"),
        "verbose-output (Concern) rec must be present"
    );

    let events = capture.snapshot();
    // At least one event for the verbose-output rec must be RecommendationEmitted.
    assert!(
        events
            .iter()
            .any(|e| e.kind == AuditEventKind::RecommendationEmitted
                && e.summary.contains("verbose-output")),
        "verbose-output rec must be audit-logged as RecommendationEmitted, not AlertFired"
    );
    // No AlertFired event for verbose-output.
    assert!(
        !events
            .iter()
            .any(|e| e.kind == AuditEventKind::AlertFired && e.summary.contains("verbose-output")),
        "verbose-output (Concern) rec must NOT be audit-logged as AlertFired"
    );
}

#[test]
fn warning_severity_rec_audit_logged_as_alert_fired() {
    // Pane tail triggers ONLY waiting_for_input (Warning-severity, "notify-input-wait" action).
    // alert_event must log it as AlertFired (Codex Phase-3A finding #2).
    let tail = "Press ENTER to continue";
    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "claude", tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let capture = CaptureSink::new();
    let mut ctx = Context::new(
        QmonsterConfig::defaults(),
        source,
        notifier,
        Box::new(capture.clone()),
    );

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");

    // Sanity: notify-input-wait rec must be present.
    assert!(
        reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "notify-input-wait"),
        "notify-input-wait (Warning) rec must be present"
    );

    let events = capture.snapshot();
    // The notify-input-wait rec must be logged as AlertFired.
    assert!(
        events.iter().any(
            |e| e.kind == AuditEventKind::AlertFired && e.summary.contains("notify-input-wait")
        ),
        "notify-input-wait (Warning) rec must be audit-logged as AlertFired"
    );
}

// ---------------------------------------------------------------------------
// Phase 4 P4-1 v1.8.1 integration test — structured profile payload flows
// end-to-end from Engine::evaluate through run_once/PaneReport to the
// live renderer (qmonster::ui::panels::format_profile_lines). Codex
// Phase-4 P4-1 review finding #1: the ProviderProfile must survive past
// Recommendation and reach an operator-visible surface with every
// lever's key + value + citation + per-lever SourceKind. This test
// fails if the structured payload is dropped (the v1.8.0 regression)
// or if any of those four pieces disappears from the rendered body.
// ---------------------------------------------------------------------------

#[test]
fn claude_default_profile_levers_flow_end_to_end_to_the_panel_renderer() {
    // Fixture: a healthy Claude main pane — no wait, no permission
    // prompt, no log-storm / error markers. The tail is short enough
    // that output_chars stays well under any alert threshold, and no
    // context_pressure marker is emitted. Under these conditions
    // recommend_claude_default should fire (healthy-state baseline).
    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "claude", "ok\n", false)],
    };
    let seen = Arc::new(Mutex::new(Vec::new()));
    let notifier = RecordingNotifier(seen.clone());
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    let rec = reports[0]
        .recommendations
        .iter()
        .find(|r| r.action == "provider-profile: claude-default")
        .expect("claude-default profile rec must fire for a healthy Claude main pane");

    // Severity is Good (positive advisory), profile is Some(_), and
    // the structured bundle must carry ProjectCanonical authority at
    // the bundle level + ProviderOfficial authority per lever. This
    // is the contract the v1.8.0 ledger claimed and v1.8.1 finally
    // delivers end-to-end.
    assert_eq!(rec.severity, Severity::Good);
    let profile = rec
        .profile
        .as_ref()
        .expect("Codex v1.8.1 finding #1: ProviderProfile must be attached to the rec");
    assert_eq!(profile.levers.len(), 3);

    // Exercise the LIVE renderer the --once path and the TUI panel
    // both call. If the renderer ever drops a lever field, this
    // assertion block fails end-to-end, not just at the unit level.
    let lines = qmonster::ui::panels::format_profile_lines(rec);
    assert_eq!(lines.len(), 4, "1 header + 3 lever rows");

    // Header line: profile name + lever count + ProjectCanonical label.
    assert!(
        lines[0].contains("claude-default"),
        "header names profile: {}",
        lines[0]
    );
    assert!(
        lines[0].contains("3 levers"),
        "header counts levers: {}",
        lines[0]
    );
    assert!(
        lines[0].contains("[Qmonster]"),
        "header carries ProjectCanonical label so operator sees bundle authority: {}",
        lines[0]
    );

    // Every lever row MUST carry the ProviderOfficial label + key +
    // value + citation. The whole point of Phase 4 is to surface
    // provider-native authority honestly; a silent drop of ANY of
    // those four pieces fails here.
    let lever_lines = &lines[1..];
    for line in lever_lines {
        assert!(
            line.contains("[Official]"),
            "every lever row carries the ProviderOfficial label: {line}"
        );
    }

    // Spot-check one lever end-to-end: BASH_MAX_OUTPUT_LENGTH = 30000
    // with a docs citation containing "bash output cap".
    let bash = lever_lines
        .iter()
        .find(|l| l.contains("BASH_MAX_OUTPUT_LENGTH"))
        .expect("BASH_MAX_OUTPUT_LENGTH lever line present");
    assert!(
        bash.contains("30000"),
        "BASH_MAX_OUTPUT_LENGTH value visible: {bash}"
    );
    assert!(
        bash.contains("bash output cap"),
        "BASH_MAX_OUTPUT_LENGTH citation visible — Codex finding required per-lever citations to reach the live path: {bash}"
    );

    // Notify gate (>= Warning) MUST still NOT fire for a profile rec
    // alone — the healthy-state advisory stays passive.
    let effects = &reports[0].effects;
    assert!(
        !effects.contains(&qmonster::domain::recommendation::RequestedEffect::Notify),
        "profile rec is Severity::Good and must not trigger Notify; effects: {:?}",
        effects
    );
}

// ---------------------------------------------------------------------------
// Phase 4 P4-3 v1.8.3 integration test — aggressive profile with side_effects
// flows end-to-end from Engine::evaluate through run_once/PaneReport to the
// live renderer (qmonster::ui::panels::format_profile_lines) under
// quota_tight mode. This test locks the Gemini G-6 contract: when the
// operator opts into quota-tight, the `claude-script-low-token` profile
// fires with its 8 ProviderOfficial levers + 8 operator-visible
// side_effects, and the renderer surfaces them end-to-end in the same
// render path the TUI panel and `--once` use. Regression would fail here
// if the aggressive profile ever dropped the side_effects list or the
// renderer ever skipped the section.
// ---------------------------------------------------------------------------

#[test]
fn claude_script_low_token_side_effects_flow_end_to_end_under_quota_tight() {
    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "claude", "ok\n", false)],
    };
    let seen = Arc::new(Mutex::new(Vec::new()));
    let notifier = RecordingNotifier(seen.clone());
    let sink = Box::new(InMemorySink::new());

    // Flip the quota-tight toggle via config. Same safety-precedence
    // path the runtime would use; no direct PolicyGates surgery.
    let mut cfg = QmonsterConfig::defaults();
    cfg.token.quota_tight = true;
    let mut ctx = Context::new(cfg, source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    let rec = reports[0]
        .recommendations
        .iter()
        .find(|r| r.action == "provider-profile: claude-script-low-token")
        .expect("claude-script-low-token fires under quota_tight on a healthy Claude main pane");

    // Rec still Severity::Good (positive advisory; stays below the
    // Notify gate). Mutual exclusion: claude-default must NOT co-exist.
    assert_eq!(rec.severity, Severity::Good);
    assert!(
        !reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "provider-profile: claude-default"),
        "mutual exclusion: claude-default must NOT fire when quota_tight is on"
    );

    // Structured profile payload carries both the lever list and the
    // populated side_effects list (Gemini G-6).
    let profile = rec
        .profile
        .as_ref()
        .expect("aggressive profile rec must carry a structured ProviderProfile payload");
    assert_eq!(profile.levers.len(), 8);
    assert_eq!(
        profile.side_effects.len(),
        8,
        "G-6 contract: side_effects is 1:1 with lever count"
    );

    // Live render path: format_profile_lines must surface both the
    // lever block AND the side_effects block. Shape = 1 header + 8
    // levers + 1 side-effects header + 8 entries = 18.
    let lines = qmonster::ui::panels::format_profile_lines(rec);
    assert_eq!(
        lines.len(),
        18,
        "1 profile header + 8 lever rows + 1 side_effects header + 8 entries"
    );

    // Lever block: every row carries the [Official] label.
    let lever_lines = &lines[1..9];
    for line in lever_lines {
        assert!(
            line.contains("[Official]"),
            "lever row carries ProviderOfficial label: {line}"
        );
    }

    // side_effects block: header + each entry as `- <text>`.
    let side_effects_header_idx = lines
        .iter()
        .position(|l| l.starts_with("side_effects"))
        .expect("side_effects header line must be in the rendered output");
    assert!(
        lines[side_effects_header_idx].contains("(8)"),
        "header reports count: {}",
        lines[side_effects_header_idx]
    );
    let entries = &lines[side_effects_header_idx + 1..];
    assert_eq!(entries.len(), 8);
    assert!(
        entries.iter().all(|e| e.starts_with("- ")),
        "every side-effect entry uses the `- <text>` shape"
    );

    // Spot-check one high-risk lever's trade-off reaches the render
    // path with the expected wording — regression would surface here
    // if the string ever drifted or disappeared.
    assert!(
        entries.iter().any(|e| e.contains("DISABLE_AUTO_MEMORY")),
        "G-6 contract: the DISABLE_AUTO_MEMORY side-effect must be visible end-to-end"
    );
    assert!(
        entries.iter().any(|e| e.contains("debugging detail")),
        "G-6 contract: --bare's debugging-detail trade-off must reach the render path"
    );

    // Notify must NOT be in effects — Severity::Good stays below the
    // gate, even when the aggressive profile has 8 side_effects.
    assert!(
        !reports[0]
            .effects
            .contains(&qmonster::domain::recommendation::RequestedEffect::Notify),
        "aggressive profile is still a positive advisory; Notify must not trigger; effects: {:?}",
        reports[0].effects
    );
}
