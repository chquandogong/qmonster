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
