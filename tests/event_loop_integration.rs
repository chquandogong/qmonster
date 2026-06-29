use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::{path::Path, process::Command};

use qmonster::app::bootstrap::Context;
use qmonster::app::config::QmonsterConfig;
use qmonster::app::event_loop::run_once;
use qmonster::domain::audit::{AuditEvent, AuditEventKind};
use qmonster::domain::identity::{IdentityConfidence, Provider, Role};
use qmonster::domain::origin::SourceKind;
use qmonster::domain::recommendation::Severity;
use qmonster::notify::desktop::NotifyBackend;
use qmonster::store::sink::{EventSink, InMemorySink};
use qmonster::tmux::polling::{PaneSource, PollingError};
use qmonster::tmux::types::{RawPaneSnapshot, WindowTarget};

mod sim;
use qmonster::domain::signal::{IdleCause, RuntimeFactKind};
use sim::PollSim;

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
    fn send_keys(&self, _pane_id: &str, _text: &str) -> Result<(), PollingError> {
        Ok(())
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
        pane_pid: None, // F-1: test fixture; production rows fill from tmux
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
        pane_pid: None, // F-1: test fixture; production rows fill from tmux
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
    // The first pane had a WAIT_INPUT tail — idle::eval_idle_transition fires
    // a "pane-state" rec (replaces the old notify-input-wait alert rule).
    assert!(
        reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "pane-state")
    );
    // The notify backend should have seen a call for that alert.
    let calls = seen.lock().unwrap();
    assert!(calls.iter().any(|(_, body, _)| body.contains("pane-state")));
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
    assert!(matches!(
        rep.signals.idle_state,
        Some(qmonster::domain::signal::IdleCause::InputWait)
    ));
}

#[test]
fn run_once_records_identity_suppression_for_title_command_conflict() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%1",
            "claude:1:main",
            "node /usr/bin/gemini --yolo",
            "◇  Ready (Qmonster)",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = CaptureSink::new();
    let sink_handle = sink.clone();
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, Box::new(sink));

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");

    assert_eq!(reports[0].identity.confidence, IdentityConfidence::Conflict);
    assert!(
        reports[0].signals.runtime_facts.is_empty(),
        "conflict pane must not receive provider-official runtime facts"
    );
    let events = sink_handle.snapshot();
    assert!(events.iter().any(|event| {
        event.kind == AuditEventKind::IdentitySuppressed
            && event.pane_id == "%1"
            && event.summary.contains("title=claude:1:main")
            && event
                .summary
                .contains("command=node /usr/bin/gemini --yolo")
    }));
}

#[test]
fn run_once_treats_qmonster_command_as_monitor_despite_stale_provider_title() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%3",
            "gemini:1:research",
            "qmonster",
            "Alerts · target %1 || visible:0 · new:0 · auto-hide:0\nPanes · target %1\nfocus: panes",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = CaptureSink::new();
    let sink_handle = sink.clone();
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, Box::new(sink));

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");

    assert_eq!(reports[0].identity.identity.provider, Provider::Qmonster);
    assert_eq!(
        reports[0].identity.identity.role,
        qmonster::domain::identity::Role::Monitor
    );
    assert_eq!(reports[0].identity.confidence, IdentityConfidence::Medium);
    assert!(
        !sink_handle
            .snapshot()
            .iter()
            .any(|event| event.kind == AuditEventKind::IdentitySuppressed)
    );
}

#[test]
fn run_once_does_not_attach_stale_provider_token_samples_to_qmonster_pane() {
    use qmonster::store::{QmonsterPaths, SqliteTokenUsageSink, TokenSample};

    let td = tempfile::TempDir::new().unwrap();
    let paths = QmonsterPaths::at(td.path());
    paths.ensure().unwrap();

    let token_sink = SqliteTokenUsageSink::open(&paths.sqlite_path())
        .expect("SqliteTokenUsageSink::open must succeed on a fresh tempdir");
    for idx in 0..3 {
        token_sink
            .record_sample(&TokenSample {
                ts_unix_ms: idx * 1_000,
                pane_id: "%3".into(),
                provider: Provider::Codex,
                input_tokens: Some(300_000 + (idx as u64) * 1_000),
                output_tokens: Some(10_000),
                cost_usd: None,
                cached_input_tokens: None,
            })
            .expect("seed token sample");
    }

    let source = FixturePaneSource {
        panes: vec![pane(
            "%3",
            "gemini:1:research",
            "qmonster",
            "Alerts · target %1 || visible:0 · new:0 · auto-hide:0\nPanes · target %1\nfocus: panes",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut ctx = Context::new(
        QmonsterConfig::defaults(),
        source,
        notifier,
        Box::new(InMemorySink::new()),
    )
    .with_token_usage_sink(token_sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");

    assert_eq!(reports[0].identity.identity.provider, Provider::Qmonster);
    assert!(
        reports[0].recent_token_samples.is_empty(),
        "Qmonster pane must not inherit stale token samples from a prior provider in the same pane id"
    );
}

#[test]
fn run_once_populates_cli_version_from_provider_surface() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%1",
            "codex:1:review",
            "codex",
            "│  >_ OpenAI Codex (v0.122.0)  │",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    let fact = reports[0]
        .signals
        .runtime_facts
        .iter()
        .find(|fact| fact.kind == RuntimeFactKind::CliVersion)
        .expect("Codex provider surface should populate CLI version fact");
    assert_eq!(fact.value, "0.122.0");
    assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
    assert_eq!(
        qmonster::ui::panels::pane_panel_title(&reports[0]),
        "qwork:1 · Codex review · CLI 0.122.0 [Official] · %1"
    );
}

#[test]
fn run_once_omits_cli_version_for_qmonster_pane() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%4",
            "qmonster:1:monitor",
            "qmonster",
            "Qmonster monitor v9.9.9",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert!(
        reports[0]
            .signals
            .runtime_facts
            .iter()
            .all(|fact| fact.kind != RuntimeFactKind::CliVersion),
        "qmonster monitor pane should never surface CLI version"
    );
    assert_eq!(
        qmonster::ui::panels::pane_panel_title(&reports[0]),
        "qwork:1 · Qmonster monitor · %4"
    );
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
    assert!((metric.value - 0.82).abs() < 1e-9);
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

#[test]
fn quota_pressure_critical_rec_fires_end_to_end_on_gemini_pane() {
    // v1.15.11 integration: a Gemini pane whose validated status
    // table reports quota at 92% must surface
    // `quota-pressure: act now` (Risk severity, is_strong true,
    // Estimated advisory source, suggested_command = None) through
    // run_once, even before the 100% LimitHit fires. The metric remains
    // ProviderOfficial; the 85% threshold is Qmonster policy. This is the
    // gap the v1.15.11 advisory rule pair was designed to bridge.
    let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      92% used      10% used     118.8 MB     cdf3f5ed      user@example.com";
    let source = FixturePaneSource {
        panes: vec![pane("%3", "gemini:1:research", "node", tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 1);

    // Sanity: quota_pressure populated as ProviderOfficial.
    let quota = reports[0]
        .signals
        .quota_pressure
        .as_ref()
        .expect("quota_pressure must be populated from the validated status table");
    assert!((quota.value - 0.92).abs() < 1e-6);
    assert_eq!(quota.source_kind, SourceKind::ProviderOfficial);

    // Sanity: 92% must NOT trigger the binary 100% LimitHit. The
    // gradient advisory replaces that gap.
    assert_ne!(reports[0].signals.idle_state, Some(IdleCause::LimitHit));

    // The advisory rule must fire on this pane.
    let rec = reports[0]
        .recommendations
        .iter()
        .find(|r| r.action == "quota-pressure: act now")
        .expect("quota_pressure_critical must fire for 92% quota");

    assert_eq!(rec.severity, Severity::Risk);
    assert_eq!(rec.source_kind, SourceKind::Estimated);
    assert!(
        rec.is_strong,
        "quota-pressure: act now must be marked is_strong (CHECKPOINT slot)"
    );
    assert_eq!(
        rec.suggested_command, None,
        "rate-limited quota cannot be resolved by a slash command"
    );
    let step = rec
        .next_step
        .as_deref()
        .expect("next_step must describe the operator's options");
    assert!(
        step.contains("snapshot"),
        "next_step should mention the snapshot precondition. got: {step}"
    );
    assert!(
        step.contains("switch") || step.contains("model") || step.contains("account"),
        "next_step should mention pacing / model-switch alternatives. got: {step}"
    );
}

#[test]
fn cost_pressure_critical_rec_fires_end_to_end_on_codex_pane() {
    // v1.15.14 + v1.15.15 integration: a Codex pane whose bottom
    // status line carries token counts that compute (with the
    // pricing table) to >= $20 USD must surface
    // `cost-pressure: act now` (Risk severity, is_strong true,
    // Estimated source, suggested_command = None) through run_once.
    // The fixture uses 25M input tokens × $1.00 / 1M = $25.00,
    // crossing the cost_pressure_critical threshold.
    use qmonster::policy::pricing::PricingTable;
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    write!(
        f,
        r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.0
output_per_1m = 10.0
"#
    )
    .unwrap();
    let pricing = PricingTable::load_from_toml(f.path()).unwrap();

    let tail = "Context 5% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 95% used · 5h 90% · weekly 90% · 0.122.0 · 258K window · 25M used · 25M in · 0 out · <redacted> · gp";
    let source = FixturePaneSource {
        panes: vec![pane("%1", "codex:1:main", "codex", tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx =
        Context::new(QmonsterConfig::defaults(), source, notifier, sink).with_pricing(pricing);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 1);

    // Sanity: cost_usd populated as Estimated, value crosses $20.
    let cost = reports[0]
        .signals
        .cost_usd
        .as_ref()
        .expect("cost_usd must populate from token counts × pricing");
    assert!(
        cost.value >= 20.0,
        "fixture must cross the cost_pressure_critical threshold; got {}",
        cost.value
    );
    assert_eq!(cost.source_kind, SourceKind::Estimated);

    // The advisory rule must fire on this pane.
    let rec = reports[0]
        .recommendations
        .iter()
        .find(|r| r.action == "cost-pressure: act now")
        .expect("cost_pressure_critical must fire when cost_usd >= $20");

    assert_eq!(rec.severity, Severity::Risk);
    assert!(
        rec.is_strong,
        "cost-pressure: act now must be marked is_strong (CHECKPOINT slot)"
    );
    assert_eq!(rec.source_kind, SourceKind::Estimated);
    assert_eq!(
        rec.suggested_command, None,
        "session cost cannot be resolved by a slash command"
    );
    let step = rec
        .next_step
        .as_deref()
        .expect("next_step must describe the operator's options");
    assert!(
        step.contains("snapshot"),
        "next_step should mention the snapshot precondition. got: {step}"
    );
    assert!(
        step.contains("model") || step.contains("pause") || step.contains("switch"),
        "next_step should mention pause / model-switch alternatives. got: {step}"
    );
}

#[test]
fn cost_budget_warning_fires_once_from_sqlite_cost_ledger() {
    use qmonster::domain::recommendation::RequestedEffect;
    use qmonster::policy::pricing::PricingTable;
    use qmonster::store::SqliteCostUsageSink;
    use std::io::Write;

    let mut f = tempfile::NamedTempFile::new().unwrap();
    write!(
        f,
        r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.0
output_per_1m = 10.0
"#
    )
    .unwrap();
    let pricing = PricingTable::load_from_toml(f.path()).unwrap();

    let tail = "Context 95% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 5% used · 5h 10% · weekly 10% · 0.122.0 · 258K window · 25M used · 25M in · 0 out · <redacted> · gp";
    let source = FixturePaneSource {
        panes: vec![pane("%1", "codex:1:main", "codex", tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let td = tempfile::tempdir().unwrap();
    let cost_sink = SqliteCostUsageSink::open(&td.path().join("q.db")).unwrap();
    let mut config = QmonsterConfig::defaults();
    config.cost.budget_usd = 30.0;
    config.cost.warning_usd = 900.0;
    config.cost.critical_usd = 1000.0;
    let mut ctx = Context::new(config, source, notifier, sink)
        .with_pricing(pricing)
        .with_cost_usage_sink(cost_sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("first poll ok");
    let rec = reports[0]
        .recommendations
        .iter()
        .find(|r| r.action == "cost-budget: 80% reached")
        .expect("$25 tracked spend should cross 80% of the $30 test budget");
    assert_eq!(rec.severity, Severity::Warning);
    assert_eq!(rec.source_kind, SourceKind::Estimated);
    assert!(
        reports[0].effects.contains(&RequestedEffect::Notify),
        "budget Warning must request Notify"
    );

    let reports = run_once(&mut ctx, Instant::now()).expect("second poll ok");
    assert!(
        !reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "cost-budget: 80% reached"),
        "the SQLite alert claim table must keep the 80% budget alert one-shot"
    );
}

#[test]
fn strong_context_pressure_rec_emits_prompt_send_proposal_end_to_end() {
    // Phase 5 P5-2 (v1.9.2) integration: when context_pressure_warning
    // fires (is_strong + suggested_command = `/compact`), the engine
    // graduates the hint into a structured `PromptSendProposed`
    // effect that rides on `PaneReport.effects`. The dispatch loop
    // stays inert (no side-effect fires for the proposal — that is
    // P5-3); we only verify the structured proposal is visible on
    // the report and carries the source pane + slash command.
    let source = FixturePaneSource {
        panes: vec![pane(
            "%7",
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

    let proposal = reports[0]
        .effects
        .iter()
        .find_map(|e| match e {
            qmonster::domain::recommendation::RequestedEffect::PromptSendProposed {
                target_pane_id,
                slash_command,
                proposal_id,
            } => Some((
                target_pane_id.as_str(),
                slash_command.as_str(),
                proposal_id.as_str(),
            )),
            _ => None,
        })
        .expect(
            "P5-2/P5-3 contract: strong context_pressure_warning rec must graduate to a PromptSendProposed effect on the owning pane",
        );
    assert_eq!(
        (proposal.0, proposal.1),
        ("%7", "/compact"),
        "proposal carries the source pane id + strong rec's slash command verbatim"
    );
    // P5-3: proposal_id is stable "{pane_id}:{slash_command}".
    assert_eq!(
        proposal.2, "%7:/compact",
        "P5-3: proposal_id must be stable '{{pane_id}}:{{slash_command}}'"
    );

    // The UI helper must render a line that names the pane, the
    // slash command, and both operator keys (default config =
    // recommend_only → accept gate passes).
    let rendered = qmonster::ui::alerts::format_prompt_send_proposal("%7", "/compact", true);
    assert!(rendered.contains("%7"));
    assert!(rendered.contains("`/compact`"));
    assert!(rendered.contains("[p] accept"));
    assert!(rendered.contains("[d] dismiss"));
}

#[test]
fn run_once_writes_recommendation_lifecycle_events() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("qmonster.sqlite3");
    let lifecycle_sink =
        qmonster::store::SqliteRecommendationLifecycleSink::open(&db_path).unwrap();
    let source = FixturePaneSource {
        panes: vec![pane(
            "%7",
            "claude:1:main",
            "claude",
            "context window usage 82%",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink)
        .with_insights_db_path(db_path.clone())
        .with_recommendation_lifecycle_sink(lifecycle_sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert!(
        reports[0].effects.iter().any(|effect| matches!(
            effect,
            qmonster::domain::recommendation::RequestedEffect::PromptSendProposed {
                slash_command,
                ..
            } if slash_command == "/compact"
        )),
        "fixture must produce the strong /compact prompt-send proposal"
    );

    let now = qmonster::app::event_loop::current_unix_ms();
    let store = qmonster::store::SqliteInsightsStore::open(&db_path).unwrap();
    let snapshot = store
        .snapshot_with_ignored_ttl(
            qmonster::store::InsightsWindow {
                since_ms: now.saturating_sub(60_000),
                until_ms: now.saturating_add(1_000),
            },
            0,
        )
        .unwrap();

    assert!(snapshot.ignored_available);
    assert!(
        snapshot
            .situations
            .iter()
            .any(|row| row.situation == "Context pressure" && row.emitted >= 1),
        "lifecycle writer must preserve the design-situation label"
    );
    let row = snapshot
        .actions
        .iter()
        .find(|row| row.action == "/compact")
        .expect("strong prompt recommendations are ledgered by slash command");
    assert_eq!(row.emitted, 1);
    assert_eq!(row.ignored, 1);
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

fn busy_codex_tail(path: &str, branch: &str) -> String {
    format!(
        "{}\nContext 50% left · {path} · gpt-5.4 · Qmonster · {branch} · Context 50% used · 0.122.0 · 500K used · 400K in · 100K out",
        busy_tail()
    )
}

fn init_clean_git_repo(path: &Path) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("git command runs");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "qmonster@example.test"]);
    run(&["config", "user.name", "Qmonster Test"]);
    std::fs::write(path.join("README.md"), "test\n").expect("write fixture");
    run(&["add", "README.md"]);
    run(&["commit", "-m", "init"]);
}

#[test]
fn concurrent_mutating_work_surfaces_in_cross_pane_findings() {
    use qmonster::domain::recommendation::CrossPaneKind;

    let tail = busy_codex_tail("/tmp/repo", "main");
    let source = FixturePaneSource {
        panes: vec![
            pane_with_path("%1", "codex:1:main", "node", &tail, false, "/tmp/repo"),
            pane_with_path("%2", "codex:2:main", "node", &tail, false, "/tmp/repo"),
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
fn concurrent_mutating_work_enriches_clean_repo_with_worktree_command() {
    use qmonster::domain::recommendation::CrossPaneKind;

    let repo = tempfile::tempdir().expect("temp repo");
    init_clean_git_repo(repo.path());
    let repo_path = repo.path().to_str().expect("utf8 path");
    let tail = busy_codex_tail(repo_path, "main");
    let source = FixturePaneSource {
        panes: vec![
            pane_with_path("%1", "codex:1:main", "node", &tail, false, repo_path),
            pane_with_path("%2", "codex:2:main", "node", &tail, false, repo_path),
        ],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    let finding = reports
        .iter()
        .flat_map(|r| r.cross_pane_findings.iter())
        .find(|f| matches!(f.kind, CrossPaneKind::ConcurrentMutatingWork))
        .expect("concurrent finding");
    let cmd = finding
        .suggested_command
        .as_deref()
        .expect("clean git repo should get executable worktree command");

    assert!(cmd.starts_with("git -C "), "got: {cmd}");
    assert!(cmd.contains(" worktree add -b "), "got: {cmd}");
    assert!(cmd.contains("main-split"), "got: {cmd}");
    assert!(
        !cmd.contains('<') && !cmd.trim_start().starts_with('#'),
        "command must be copyable as-is: {cmd}"
    );
}

#[test]
fn concurrent_does_not_trigger_across_different_current_paths() {
    let tail_a = busy_codex_tail("/tmp/repo-a", "main");
    let tail_b = busy_codex_tail("/tmp/repo-b", "main");
    let source = FixturePaneSource {
        panes: vec![
            pane_with_path("%1", "codex:1:main", "node", &tail_a, false, "/tmp/repo-a"),
            pane_with_path("%2", "codex:2:main", "node", &tail_b, false, "/tmp/repo-b"),
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

    let tail = busy_codex_tail("/repo", "main");
    // Three panes in the same directory, IDs out of lex order.
    let source = FixturePaneSource {
        panes: vec![
            pane_with_path("%9", "codex:9:main", "node", &tail, false, "/repo"),
            pane_with_path("%1", "codex:1:main", "node", &tail, false, "/repo"),
            pane_with_path("%5", "codex:5:main", "node", &tail, false, "/repo"),
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
fn cross_window_concurrent_work_fires_end_to_end_when_security_gate_enabled() {
    // Phase D D1 (v1.17.0): two healthy Codex Main panes touching the
    // same path + branch but living in different tmux windows surface
    // a CrossWindowConcurrentWork finding (Concern, distinct kind from
    // the same-window ConcurrentMutatingWork Warning) once the operator
    // opts into `[security] cross_window_findings = true`.
    use qmonster::app::config::SecurityConfig;
    use qmonster::domain::recommendation::CrossPaneKind;

    let tail = busy_codex_tail("/tmp/repo", "main");
    let pane_a = RawPaneSnapshot {
        session_name: "qmonster".into(),
        window_index: "0".into(),
        pane_id: "%1".into(),
        title: "codex:1:main".into(),
        current_command: "node".into(),
        current_path: "/tmp/repo".into(),
        active: true,
        dead: false,
        tail: tail.clone(),
        pane_pid: None, // F-1: test fixture; production rows fill from tmux
    };
    let pane_b = RawPaneSnapshot {
        session_name: "scratch".into(),
        window_index: "0".into(),
        pane_id: "%2".into(),
        title: "codex:2:main".into(),
        current_command: "node".into(),
        current_path: "/tmp/repo".into(),
        active: true,
        dead: false,
        tail,
        pane_pid: None, // F-1: test fixture; production rows fill from tmux
    };
    let source = FixturePaneSource {
        panes: vec![pane_a, pane_b],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut config = QmonsterConfig::defaults();
    config.security = SecurityConfig {
        posture_advisories: false,
        cross_window_findings: true,
        identity_drift_findings: false,
        cross_pane_file_findings: false,
    };
    let mut ctx = Context::new(config, source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 2);

    let with_findings: Vec<_> = reports
        .iter()
        .filter(|r| !r.cross_pane_findings.is_empty())
        .collect();
    assert_eq!(
        with_findings.len(),
        1,
        "exactly one report should carry the cross-window finding (anchor only)"
    );

    let finding = &with_findings[0].cross_pane_findings[0];
    assert_eq!(finding.kind, CrossPaneKind::CrossWindowConcurrentWork);
    assert!(
        finding.reason.contains("across windows"),
        "reason text must surface cross-window scope: {:?}",
        finding.reason
    );
    assert!(finding.reason.contains("qmonster:0"));
    assert!(finding.reason.contains("scratch:0"));
}

#[test]
fn concurrent_file_edit_fires_end_to_end_when_security_gate_enabled() {
    // Phase F F-8: two healthy Claude Main panes that recently called
    // `● Edit(file: "src/foo.rs")` on the same file inside the same
    // worktree must surface a `ConcurrentFileEdit` finding once the
    // operator opts into `[security] cross_pane_file_findings = true`.
    use qmonster::app::config::SecurityConfig;
    use qmonster::domain::recommendation::CrossPaneKind;

    // Compose a Claude tail with the canonical busy markers PLUS a
    // recent `● Edit(...)` Claude tool call. The shared `busy_tail()`
    // helper (see top of this file) already supplies the verbose / log
    // shape that keeps the pane out of the input/permission-wait gate.
    let mut claude_tail = busy_tail();
    claude_tail.push_str("\n● Edit(file: \"src/foo.rs\")\n");

    let pane_a = RawPaneSnapshot {
        session_name: "qmonster".into(),
        window_index: "0".into(),
        pane_id: "%1".into(),
        title: "claude:1:main".into(),
        current_command: "node".into(),
        current_path: "/tmp/repo".into(),
        active: true,
        dead: false,
        tail: claude_tail.clone(),
        pane_pid: None,
    };
    let pane_b = RawPaneSnapshot {
        session_name: "qmonster".into(),
        window_index: "0".into(),
        pane_id: "%2".into(),
        title: "claude:2:review".into(),
        current_command: "node".into(),
        current_path: "/tmp/repo".into(),
        active: true,
        dead: false,
        tail: claude_tail,
        pane_pid: None,
    };
    let source = FixturePaneSource {
        panes: vec![pane_a, pane_b],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut config = QmonsterConfig::defaults();
    config.security = SecurityConfig {
        posture_advisories: false,
        cross_window_findings: false,
        identity_drift_findings: false,
        cross_pane_file_findings: true,
    };
    let mut ctx = Context::new(config, source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 2);

    let file_findings: Vec<_> = reports
        .iter()
        .flat_map(|r| r.cross_pane_findings.iter())
        .filter(|f| matches!(f.kind, CrossPaneKind::ConcurrentFileEdit))
        .collect();
    assert_eq!(
        file_findings.len(),
        1,
        "exactly one ConcurrentFileEdit finding must surface for the shared file"
    );
    let finding = file_findings[0];
    assert_eq!(finding.anchor_pane_id, "%1");
    assert_eq!(finding.other_pane_ids, vec!["%2".to_string()]);
    assert!(
        finding.reason.contains("/tmp/repo/src/foo.rs"),
        "reason must call out the resolved absolute file path: {:?}",
        finding.reason
    );
}

#[test]
fn concurrent_file_edit_stays_silent_when_security_gate_disabled() {
    // Same scenario as above, but without the opt-in gate. The rule
    // must produce no ConcurrentFileEdit finding even though the file
    // overlap is real and the tail markers are present.
    use qmonster::domain::recommendation::CrossPaneKind;

    let mut claude_tail = busy_tail();
    claude_tail.push_str("\n● Edit(file: \"src/foo.rs\")\n");

    let pane_a = RawPaneSnapshot {
        session_name: "qmonster".into(),
        window_index: "0".into(),
        pane_id: "%1".into(),
        title: "claude:1:main".into(),
        current_command: "node".into(),
        current_path: "/tmp/repo".into(),
        active: true,
        dead: false,
        tail: claude_tail.clone(),
        pane_pid: None,
    };
    let pane_b = RawPaneSnapshot {
        session_name: "qmonster".into(),
        window_index: "0".into(),
        pane_id: "%2".into(),
        title: "claude:2:main".into(),
        current_command: "node".into(),
        current_path: "/tmp/repo".into(),
        active: true,
        dead: false,
        tail: claude_tail,
        pane_pid: None,
    };
    let source = FixturePaneSource {
        panes: vec![pane_a, pane_b],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    // Default config — gate stays off.
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    let file_findings: Vec<_> = reports
        .iter()
        .flat_map(|r| r.cross_pane_findings.iter())
        .filter(|f| matches!(f.kind, CrossPaneKind::ConcurrentFileEdit))
        .collect();
    assert!(
        file_findings.is_empty(),
        "default config must not produce a ConcurrentFileEdit finding: {file_findings:?}"
    );
}

#[test]
fn provider_drift_fires_once_when_security_gate_enabled_and_dedups_on_repeat_polls() {
    // Phase D D2 (v1.18.0): pane %1 starts as Claude, then the
    // operator quits Claude and starts Codex in the same pane. The
    // first poll establishes the baseline, the second poll fires the
    // drift Concern, and the third poll (still Codex) must stay
    // silent because the dedup table already saw this transition.
    use qmonster::app::config::SecurityConfig;
    use std::cell::RefCell;

    struct DriftSource {
        panes: RefCell<Vec<RawPaneSnapshot>>,
    }
    impl PaneSource for DriftSource {
        fn list_panes(
            &self,
            _target: Option<&WindowTarget>,
        ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
            Ok(self.panes.borrow().clone())
        }
        fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
            Ok(self.available_targets()?.into_iter().next())
        }
        fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
            let mut t: Vec<WindowTarget> = self
                .panes
                .borrow()
                .iter()
                .map(|p| WindowTarget {
                    session_name: p.session_name.clone(),
                    window_index: p.window_index.clone(),
                })
                .collect();
            t.sort();
            t.dedup();
            Ok(t)
        }
        fn capture_tail(&self, pane_id: &str, _lines: usize) -> Result<String, PollingError> {
            Ok(self
                .panes
                .borrow()
                .iter()
                .find(|p| p.pane_id == pane_id)
                .map(|p| p.tail.clone())
                .unwrap_or_default())
        }
        fn send_keys(&self, _pane_id: &str, _text: &str) -> Result<(), PollingError> {
            Ok(())
        }
    }

    let claude_pane = pane_with_path("%1", "claude:1:main", "claude", "ready", false, "/tmp/repo");
    let codex_pane = pane_with_path("%1", "codex:1:main", "node", "ready", false, "/tmp/repo");

    let source = DriftSource {
        panes: RefCell::new(vec![claude_pane]),
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut config = QmonsterConfig::defaults();
    config.security = SecurityConfig {
        posture_advisories: false,
        cross_window_findings: false,
        identity_drift_findings: true,
        cross_pane_file_findings: false,
    };
    let mut ctx = Context::new(config, source, notifier, sink);

    // Poll 1: baseline Claude. No prior snapshot → no drift.
    let r1 = run_once(&mut ctx, Instant::now()).expect("ok");
    assert!(
        !r1[0]
            .recommendations
            .iter()
            .any(|r| r.action.starts_with("identity-drift")),
        "first poll establishes baseline; no drift yet"
    );

    // Operator swaps Claude for Codex in the same pane.
    *ctx.source.panes.borrow_mut() = vec![codex_pane];

    // Poll 2: provider drift fires.
    let r2 = run_once(&mut ctx, Instant::now()).expect("ok");
    let drift_recs: Vec<_> = r2[0]
        .recommendations
        .iter()
        .filter(|r| r.action.starts_with("identity-drift"))
        .collect();
    assert_eq!(
        drift_recs.len(),
        1,
        "second poll surfaces exactly one drift recommendation, got {drift_recs:?}"
    );
    assert_eq!(drift_recs[0].action, "identity-drift: provider changed");
    assert!(
        drift_recs[0].reason.contains("Claude") && drift_recs[0].reason.contains("Codex"),
        "drift reason names both endpoints: {:?}",
        drift_recs[0].reason
    );

    // Poll 3: same Codex pane. Dedup table must keep this silent.
    let r3 = run_once(&mut ctx, Instant::now()).expect("ok");
    assert!(
        !r3[0]
            .recommendations
            .iter()
            .any(|r| r.action.starts_with("identity-drift")),
        "third poll must dedup the same drift transition"
    );
}

#[test]
fn identity_drift_stays_silent_when_security_gate_off_by_default() {
    // Same Claude→Codex swap as the test above, but with default
    // config — `[security] identity_drift_findings = false`. The
    // drift rule must stay silent so default operators don't see new
    // alerts after a v1.18.0 upgrade.
    use std::cell::RefCell;

    struct DriftSource {
        panes: RefCell<Vec<RawPaneSnapshot>>,
    }
    impl PaneSource for DriftSource {
        fn list_panes(
            &self,
            _target: Option<&WindowTarget>,
        ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
            Ok(self.panes.borrow().clone())
        }
        fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
            Ok(self.available_targets()?.into_iter().next())
        }
        fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
            let mut t: Vec<WindowTarget> = self
                .panes
                .borrow()
                .iter()
                .map(|p| WindowTarget {
                    session_name: p.session_name.clone(),
                    window_index: p.window_index.clone(),
                })
                .collect();
            t.sort();
            t.dedup();
            Ok(t)
        }
        fn capture_tail(&self, pane_id: &str, _lines: usize) -> Result<String, PollingError> {
            Ok(self
                .panes
                .borrow()
                .iter()
                .find(|p| p.pane_id == pane_id)
                .map(|p| p.tail.clone())
                .unwrap_or_default())
        }
        fn send_keys(&self, _pane_id: &str, _text: &str) -> Result<(), PollingError> {
            Ok(())
        }
    }

    let claude_pane = pane_with_path("%1", "claude:1:main", "claude", "ready", false, "/tmp/repo");
    let codex_pane = pane_with_path("%1", "codex:1:main", "node", "ready", false, "/tmp/repo");

    let source = DriftSource {
        panes: RefCell::new(vec![claude_pane]),
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    // Default config — drift gate stays off.
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let _ = run_once(&mut ctx, Instant::now()).expect("ok");
    *ctx.source.panes.borrow_mut() = vec![codex_pane];
    let r2 = run_once(&mut ctx, Instant::now()).expect("ok");

    assert!(
        !r2[0]
            .recommendations
            .iter()
            .any(|r| r.action.starts_with("identity-drift")),
        "default config must keep identity drift silent"
    );
}

#[test]
fn cross_window_concurrent_work_stays_silent_when_gate_off_by_default() {
    // Same scenario as above but with default config — the gate is
    // off by default so neither cross-window nor same-window findings
    // fire (the panes are not in the same window for the legacy
    // path either).
    let tail = busy_codex_tail("/tmp/repo", "main");
    let pane_a = RawPaneSnapshot {
        session_name: "qmonster".into(),
        window_index: "0".into(),
        pane_id: "%1".into(),
        title: "codex:1:main".into(),
        current_command: "node".into(),
        current_path: "/tmp/repo".into(),
        active: true,
        dead: false,
        tail: tail.clone(),
        pane_pid: None, // F-1: test fixture; production rows fill from tmux
    };
    let pane_b = RawPaneSnapshot {
        session_name: "scratch".into(),
        window_index: "0".into(),
        pane_id: "%2".into(),
        title: "codex:2:main".into(),
        current_command: "node".into(),
        current_path: "/tmp/repo".into(),
        active: true,
        dead: false,
        tail,
        pane_pid: None, // F-1: test fixture; production rows fill from tmux
    };
    let source = FixturePaneSource {
        panes: vec![pane_a, pane_b],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    for r in &reports {
        assert!(
            r.cross_pane_findings.is_empty(),
            "cross-window findings stay opt-in; default config must keep pane {} quiet",
            r.pane_id
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
    //   "Press ENTER to continue" → idle_state=InputWait → Warning → pane-state
    //   "I'd be happy to help"   → verbose_answer        → Concern → verbose-output
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
    // idle::eval_idle_transition fires "pane-state" (replaces notify-input-wait).
    assert!(
        reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "pane-state"),
        "pane-state (Warning) rec must be present"
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
    // Pane tail triggers idle_state=InputWait (Warning-severity, "pane-state" action via
    // idle::eval_idle_transition). alert_event must log it as AlertFired (Codex Phase-3A #2).
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

    // Sanity: pane-state rec must be present (replaces notify-input-wait).
    assert!(
        reports[0]
            .recommendations
            .iter()
            .any(|r| r.action == "pane-state"),
        "pane-state (Warning) rec must be present"
    );

    let events = capture.snapshot();
    // The pane-state rec must be logged as AlertFired.
    assert!(
        events
            .iter()
            .any(|e| e.kind == AuditEventKind::AlertFired && e.summary.contains("pane-state")),
        "pane-state (Warning) rec must be audit-logged as AlertFired"
    );
}

// ---------------------------------------------------------------------------
// Phase 5 P5-3 (v1.10.0) integration tests
// ---------------------------------------------------------------------------

/// A `PaneSource` that wraps `FixturePaneSource` and captures `send_keys`
/// calls so tests can verify the execution gate behaviour without needing a
/// live tmux session.
struct CapturingPaneSource {
    inner: FixturePaneSource,
    calls: Arc<Mutex<Vec<(String, String)>>>,
    send_result: Result<(), String>,
}

impl CapturingPaneSource {
    fn new(panes: Vec<RawPaneSnapshot>) -> Self {
        Self {
            inner: FixturePaneSource { panes },
            calls: Arc::new(Mutex::new(Vec::new())),
            send_result: Ok(()),
        }
    }

    fn with_send_error(mut self, msg: &str) -> Self {
        self.send_result = Err(msg.to_string());
        self
    }

    fn recorded_sends(&self) -> Vec<(String, String)> {
        self.calls.lock().unwrap().clone()
    }
}

impl PaneSource for CapturingPaneSource {
    fn list_panes(
        &self,
        target: Option<&WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        self.inner.list_panes(target)
    }
    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        self.inner.current_target()
    }
    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        self.inner.available_targets()
    }
    fn capture_tail(&self, pane_id: &str, lines: usize) -> Result<String, PollingError> {
        self.inner.capture_tail(pane_id, lines)
    }
    fn send_keys(&self, pane_id: &str, text: &str) -> Result<(), PollingError> {
        self.calls
            .lock()
            .unwrap()
            .push((pane_id.to_string(), text.to_string()));
        match &self.send_result {
            Ok(()) => Ok(()),
            Err(msg) => Err(PollingError::NonZero(msg.clone())),
        }
    }
}

#[test]
fn prompt_send_proposal_carries_stable_proposal_id_end_to_end() {
    // P5-3: verify proposal_id is set to "{pane_id}:{slash_command}" by
    // the engine producer and survives the full run_once pipeline onto
    // PaneReport.effects.
    let source = FixturePaneSource {
        panes: vec![pane(
            "%3",
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

    let proposal = reports[0]
        .effects
        .iter()
        .find_map(|e| match e {
            qmonster::domain::recommendation::RequestedEffect::PromptSendProposed {
                target_pane_id,
                slash_command,
                proposal_id,
            } => Some((
                target_pane_id.as_str(),
                slash_command.as_str(),
                proposal_id.as_str(),
            )),
            _ => None,
        })
        .expect(
            "P5-3: context_pressure_warning must produce a PromptSendProposed with proposal_id",
        );

    assert_eq!(proposal.0, "%3");
    assert_eq!(proposal.1, "/compact");
    assert_eq!(
        proposal.2, "%3:/compact",
        "P5-3: proposal_id must equal '{{pane_id}}:{{slash_command}}'"
    );
}

#[test]
fn capturing_source_records_send_keys_calls() {
    // P5-3: verify the CapturingPaneSource fixture correctly records
    // send_keys calls so that higher-level tests can use it.
    let src = CapturingPaneSource::new(vec![]);
    assert!(src.send_keys("%1", "/compact").is_ok());
    assert!(src.send_keys("%2", "/clear").is_ok());
    let calls = src.recorded_sends();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], ("%1".to_string(), "/compact".to_string()));
    assert_eq!(calls[1], ("%2".to_string(), "/clear".to_string()));
}

#[test]
fn capturing_source_propagates_configured_error() {
    // P5-3: CapturingPaneSource can simulate a tmux error so we can
    // test PromptSendFailed paths without a live tmux session.
    let src = CapturingPaneSource::new(vec![]).with_send_error("no server running");
    let err = src.send_keys("%1", "/compact").unwrap_err();
    assert!(
        err.to_string().contains("no server running"),
        "error message must propagate; got: {err}"
    );
}

#[test]
fn codex_status_line_end_to_end_populates_four_metrics() {
    use qmonster::adapters::{ParserContext, parse_for};
    use qmonster::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use qmonster::domain::origin::SourceKind;
    use qmonster::policy::claude_settings::ClaudeSettings;
    use qmonster::policy::pricing::PricingTable;
    use std::io::Write;

    let identity = ResolvedIdentity {
        identity: PaneIdentity {
            provider: Provider::Codex,
            instance: 1,
            role: Role::Review,
            pane_id: "%9".into(),
        },
        confidence: IdentityConfidence::High,
    };
    let tail = "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp";

    // v1.11.2 remediation (Gemini v1.11.0 must-fix #1): load the
    // pricing fixture via the operator-facing TOML path, not via a
    // test-only public API. Keep `_f` bound so the tempfile outlives
    // the parse call.
    let mut f = tempfile::NamedTempFile::new().unwrap();
    write!(
        f,
        r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.00
output_per_1m = 10.00
"#
    )
    .unwrap();
    let pricing = PricingTable::load_from_toml(f.path()).unwrap();
    let _f = f;

    let settings = ClaudeSettings::empty();
    let history = qmonster::adapters::common::PaneTailHistory::empty();
    let ctx = ParserContext {
        identity: &identity,
        tail,
        pricing: &pricing,
        claude_settings: &settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
        current_path: "", // F-2: test fixture; production wires from snapshot.current_path
        codex_rollout_enabled: false,
        agy_transcript_enabled: false,
        agy_enrichment_enabled: false,
    };
    let signals = parse_for(&ctx);

    assert_eq!(
        signals.context_pressure.as_ref().unwrap().source_kind,
        SourceKind::ProviderOfficial
    );
    assert!((signals.quota_5h_pressure.as_ref().unwrap().value - 0.02).abs() < 1e-6);
    assert!((signals.quota_weekly_pressure.as_ref().unwrap().value - 0.01).abs() < 1e-6);
    assert_eq!(signals.token_count.as_ref().unwrap().value, 1_530_000);
    assert_eq!(signals.model_name.as_ref().unwrap().value, "gpt-5.4");
    let cost = signals.cost_usd.as_ref().unwrap();
    assert!((cost.value - 1.714).abs() < 1e-9);
    assert_eq!(cost.source_kind, SourceKind::Estimated);
}

#[test]
fn codex_status_line_end_to_end_without_pricing_populates_three_metrics() {
    use qmonster::adapters::{ParserContext, parse_for};
    use qmonster::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use qmonster::policy::claude_settings::ClaudeSettings;
    use qmonster::policy::pricing::PricingTable;

    let identity = ResolvedIdentity {
        identity: PaneIdentity {
            provider: Provider::Codex,
            instance: 1,
            role: Role::Review,
            pane_id: "%9".into(),
        },
        confidence: IdentityConfidence::High,
    };
    let tail = "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp";

    let pricing = PricingTable::empty();
    let settings = ClaudeSettings::empty();
    let history = qmonster::adapters::common::PaneTailHistory::empty();
    let ctx = ParserContext {
        identity: &identity,
        tail,
        pricing: &pricing,
        claude_settings: &settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
        current_path: "", // F-2: test fixture; production wires from snapshot.current_path
        codex_rollout_enabled: false,
        agy_transcript_enabled: false,
        agy_enrichment_enabled: false,
    };
    let signals = parse_for(&ctx);

    assert!(signals.context_pressure.is_some());
    assert!(signals.token_count.is_some());
    assert!(signals.model_name.is_some());
    assert!(signals.cost_usd.is_none());
}

#[path = "fixtures/codex.rs"]
mod codex_fixtures;

#[test]
fn codex_status_line_end_to_end_populates_seven_metrics() {
    use qmonster::adapters::{ParserContext, parse_for};
    use qmonster::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use qmonster::domain::origin::SourceKind;
    use qmonster::policy::claude_settings::ClaudeSettings;
    use qmonster::policy::pricing::PricingTable;
    use std::io::Write;
    use tempfile::NamedTempFile;

    let identity = ResolvedIdentity {
        identity: PaneIdentity {
            provider: Provider::Codex,
            instance: 1,
            role: Role::Review,
            pane_id: "%9".into(),
        },
        confidence: IdentityConfidence::High,
    };

    // Tail has the /status box AND the status bar (newest line at bottom).
    let tail = format!(
        "{}\n{}",
        codex_fixtures::CODEX_STATUS_BOX_FIXTURE,
        codex_fixtures::CODEX_STATUS_FIXTURE_V0_122_0
    );

    // Pricing: operator-supplied $1/M input, $10/M output for gpt-5.4.
    let mut pricing_toml = NamedTempFile::new().unwrap();
    write!(
        pricing_toml,
        r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.00
output_per_1m = 10.00
"#
    )
    .unwrap();
    let pricing = PricingTable::load_from_toml(pricing_toml.path()).unwrap();
    let claude_settings = ClaudeSettings::empty();
    let history = qmonster::adapters::common::PaneTailHistory::empty();

    let ctx = ParserContext {
        identity: &identity,
        tail: &tail,
        pricing: &pricing,
        claude_settings: &claude_settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
        current_path: "", // F-2: test fixture; production wires from snapshot.current_path
        codex_rollout_enabled: false,
        agy_transcript_enabled: false,
        agy_enrichment_enabled: false,
    };

    let signals = parse_for(&ctx);

    // Slice 1 fields (still populated, provider-official)
    assert_eq!(
        signals.context_pressure.as_ref().unwrap().source_kind,
        SourceKind::ProviderOfficial
    );
    assert!((signals.quota_5h_pressure.as_ref().unwrap().value - 0.02).abs() < 1e-6);
    assert!((signals.quota_weekly_pressure.as_ref().unwrap().value - 0.01).abs() < 1e-6);
    assert_eq!(signals.token_count.as_ref().unwrap().value, 1_530_000);
    assert_eq!(signals.model_name.as_ref().unwrap().value, "gpt-5.4");
    let cost = signals.cost_usd.as_ref().unwrap();
    assert!((cost.value - 1.714).abs() < 1e-9);
    assert_eq!(cost.source_kind, SourceKind::Estimated);

    // Slice 2 fields — the new three
    let branch = signals.git_branch.as_ref().expect("branch populated");
    assert_eq!(branch.value, "main");
    assert_eq!(branch.source_kind, SourceKind::ProviderOfficial);

    let worktree = signals.worktree_path.as_ref().expect("worktree populated");
    assert_eq!(worktree.value, "~/Qmonster");
    assert_eq!(worktree.source_kind, SourceKind::ProviderOfficial);

    let effort = signals.reasoning_effort.as_ref().expect("effort populated");
    assert_eq!(effort.value, "xhigh");
    assert_eq!(effort.source_kind, SourceKind::ProviderOfficial);
    assert_eq!(effort.confidence, Some(0.6));
}

#[test]
fn claude_adapter_end_to_end_reads_model_from_claude_settings() {
    use qmonster::adapters::{ParserContext, parse_for};
    use qmonster::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use qmonster::domain::origin::SourceKind;
    use qmonster::policy::claude_settings::ClaudeSettings;
    use qmonster::policy::pricing::PricingTable;
    use std::io::Write;
    use tempfile::NamedTempFile;

    let identity = ResolvedIdentity {
        identity: PaneIdentity {
            provider: Provider::Claude,
            instance: 1,
            role: Role::Main,
            pane_id: "%1".into(),
        },
        confidence: IdentityConfidence::High,
    };

    let mut settings_json = NamedTempFile::new().unwrap();
    write!(settings_json, r#"{{"model": "claude-sonnet-4-6"}}"#).unwrap();
    let claude_settings = ClaudeSettings::load_from_path(settings_json.path()).unwrap();
    let pricing = PricingTable::empty();

    let tail = "✶ Working… (1m · ↓ 500 tokens · thought for 1s)";
    let history = qmonster::adapters::common::PaneTailHistory::empty();
    let ctx = ParserContext {
        identity: &identity,
        tail,
        pricing: &pricing,
        claude_settings: &claude_settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
        current_path: "", // F-2: test fixture; production wires from snapshot.current_path
        codex_rollout_enabled: false,
        agy_transcript_enabled: false,
        agy_enrichment_enabled: false,
    };

    let signals = parse_for(&ctx);

    // Slice 2: model comes from settings, not tail.
    let model = signals
        .model_name
        .as_ref()
        .expect("model populated from settings");
    assert_eq!(model.value, "claude-sonnet-4-6");
    assert_eq!(model.source_kind, SourceKind::ProviderOfficial);
    assert_eq!(model.confidence, Some(0.9));
    assert_eq!(model.provider, Some(Provider::Claude));

    // Claude tail still populates token_count (Slice 1 behavior).
    assert_eq!(signals.token_count.as_ref().unwrap().value, 500);

    // Honesty: cost stays None (no input tokens on Claude tail).
    assert!(signals.cost_usd.is_none());
}

// ---------------------------------------------------------------------------
// Slice 4 Task 14: multi-poll PollSim idle-state transition tests
// ---------------------------------------------------------------------------

#[test]
fn five_polls_with_same_tail_produces_stale_idle_state() {
    let mut sim = PollSim::new(4);
    for _ in 0..5 {
        sim.feed("idle Claude tail without markers or cursor");
    }
    assert_eq!(sim.last_signal_set().idle_state, Some(IdleCause::Stale));
}

#[test]
fn stillness_polls_config_controls_stale_window() {
    let mut sim = PollSim::new(2);
    sim.feed("same quiet tail");
    assert_ne!(sim.last_signal_set().idle_state, Some(IdleCause::Stale));
    sim.feed("same quiet tail");
    assert_eq!(sim.last_signal_set().idle_state, Some(IdleCause::Stale));
}

#[test]
fn changing_tails_never_produce_stale_idle_state() {
    let mut sim = PollSim::new(4);
    for i in 0..5 {
        sim.feed(&format!("changing tail iteration {i}"));
    }
    assert_ne!(sim.last_signal_set().idle_state, Some(IdleCause::Stale));
}

#[test]
fn transition_into_input_wait_fires_one_alert_only() {
    let mut sim = PollSim::new(4);
    sim.feed("normal output");
    sim.feed("needs your input now");
    sim.feed("needs your input now");
    sim.feed("needs your input now");
    let alerts = sim.alerts_emitted_with_action("pane-state");
    assert_eq!(alerts.len(), 1);
}

#[test]
fn distinct_cause_transition_fires_new_alert() {
    let mut sim = PollSim::new(4);
    sim.feed("needs your input now"); // None → InputWait
    sim.feed("this action requires approval (y/n)"); // → PermissionWait
    let alerts = sim.alerts_emitted_with_action("pane-state");
    assert_eq!(alerts.len(), 2);
}

#[test]
fn limit_hit_transition_fires_risk_severity() {
    let mut sim = PollSim::new_codex(4);
    sim.feed("active output");
    sim.feed("│  5h limit:    [████████████████████] 100% used  │");
    let alerts = sim.alerts_emitted_with_action("pane-state");
    assert_eq!(alerts.len(), 1);
    use qmonster::domain::recommendation::Severity;
    assert_eq!(alerts[0].severity, Severity::Risk);
}

#[test]
fn returning_to_active_clears_idle_state_no_new_alert() {
    let mut sim = PollSim::new(4);
    sim.feed("needs your input now");
    sim.feed("normal output");
    let alerts = sim.alerts_emitted_with_action("pane-state");
    assert_eq!(alerts.len(), 1, "only the entry into idle fires");
}

#[test]
fn claude_statusline_end_to_end_populates_runtime_metrics_without_overlay() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%9",
            "claude:1:main",
            "claude",
            "Opus 4.7 (1M context)·max  CTX 51%  5h 9%  7d 1%  ~/Qmonster\n│› Implement {feature}\n⏵⏵ bypass permissions on (shift+tab to cycle)",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    let signals = &reports[0].signals;

    assert_eq!(
        signals.model_name.as_ref().unwrap().value,
        "Opus 4.7 (1M context)"
    );
    assert_eq!(signals.reasoning_effort.as_ref().unwrap().value, "max");
    assert_eq!(signals.worktree_path.as_ref().unwrap().value, "~/Qmonster");
    assert!((signals.context_pressure.as_ref().unwrap().value - 0.51).abs() < 1e-6);
    assert!((signals.quota_5h_pressure.as_ref().unwrap().value - 0.09).abs() < 1e-6);
    assert!((signals.quota_weekly_pressure.as_ref().unwrap().value - 0.01).abs() < 1e-6);
    assert_eq!(
        signals.context_pressure.as_ref().unwrap().source_kind,
        SourceKind::ProviderOfficial
    );
}

#[test]
fn runtime_refresh_tail_overlay_is_parsed_once_then_consumed() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%9",
            "claude:1:main",
            "claude",
            "previous output\n\n❯ ",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);
    ctx.runtime_refresh_tail_overlays.insert(
        "%9".into(),
        "Current session\n████████████████████ 100% used".into(),
    );

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");

    assert_eq!(reports[0].idle_state, Some(IdleCause::LimitHit));
    assert!(
        ctx.runtime_refresh_tail_overlays.is_empty(),
        "runtime refresh capture should be a one-shot parser overlay"
    );
}

#[test]
fn runtime_refresh_tail_overlay_preserves_live_idle_cursor() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%9",
            "claude:1:main",
            "claude",
            "previous output\n\n❯ ",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);
    ctx.runtime_refresh_tail_overlays.insert(
        "%9".into(),
        "Context Usage\n\nOpus 4.7 (1M context)\n143.3k/1m tokens (14%)".into(),
    );

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");

    assert_eq!(
        reports[0].idle_state,
        Some(IdleCause::WorkComplete),
        "informational runtime captures must not hide the live prompt-ready cursor"
    );
    assert!((reports[0].signals.context_pressure.as_ref().unwrap().value - 0.14).abs() < 1e-6);
}

#[test]
fn gemini_model_reset_overlay_persists_after_transient_picker_closes() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%9",
            "gemini:1:main",
            "node",
            "\
previous output
▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄
 *   Type your message or @path/to/file
▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
 branch         sandbox             /model                         workspace (/directory)         quota           context          memory           session                       /auth
 main           no sandbox          gemini-3.1-pro-preview         ~/Qmonster                     5% used         0% used          238.2 MB         af324058         user@example.com",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);
    ctx.runtime_refresh_tail_overlays.insert(
        "%9".into(),
        "\
╭────────────────────────────────────────────────────────────╮
│ Select Model                                               │
│ Model usage                                                │
│ Flash       ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  0%   Resets: 11:13 PM (24h)
│ Flash Lite  ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  0%   Resets: 11:43 AM (12h 30m)
│ Pro         ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  5%   Resets: 6:45 PM (19h 32m)
╰────────────────────────────────────────────────────────────╯"
            .into(),
    );

    let reports = run_once(&mut ctx, Instant::now()).expect("first poll ok");
    assert_eq!(reports[0].idle_state, Some(IdleCause::WorkComplete));
    let reset_facts = reports[0]
        .signals
        .runtime_facts
        .iter()
        .filter(|fact| fact.kind == RuntimeFactKind::ModelReset)
        .collect::<Vec<_>>();
    assert_eq!(reset_facts.len(), 1);
    assert_eq!(reset_facts[0].value, "Pro 6:45 PM (19h 32m)");
    assert!(
        ctx.runtime_refresh_tail_overlays.contains_key("%9"),
        "Gemini /model picker captures are transient and must persist after the first parse"
    );

    let reports = run_once(&mut ctx, Instant::now()).expect("second poll ok");
    let reset_facts = reports[0]
        .signals
        .runtime_facts
        .iter()
        .filter(|fact| fact.kind == RuntimeFactKind::ModelReset)
        .collect::<Vec<_>>();
    assert_eq!(reset_facts.len(), 1);
    assert_eq!(reset_facts[0].value, "Pro 6:45 PM (19h 32m)");
    assert!(
        ctx.runtime_refresh_tail_overlays.contains_key("%9"),
        "reset badge should not disappear just because the closed picker is no longer in live tail"
    );
}

#[test]
fn claude_pressure_metrics_survive_separate_runtime_surfaces() {
    let source = FixturePaneSource {
        panes: vec![pane(
            "%9",
            "claude:1:main",
            "claude",
            "previous output\n\n❯ ",
            false,
        )],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    ctx.runtime_refresh_tail_overlays.insert(
        "%9".into(),
        "Context\nContext window: 82% used\nEsc to cancel".into(),
    );
    let reports = run_once(&mut ctx, Instant::now()).expect("context ok");
    assert!((reports[0].signals.context_pressure.as_ref().unwrap().value - 0.82).abs() < 1e-6);
    assert!(reports[0].signals.quota_5h_pressure.is_none());

    ctx.runtime_refresh_tail_overlays.insert(
        "%9".into(),
        "Current session\n0% used\n\nCurrent week (all models)\n████████ 36% used".into(),
    );
    let reports = run_once(&mut ctx, Instant::now()).expect("usage ok");
    assert!(
        (reports[0].signals.context_pressure.as_ref().unwrap().value - 0.82).abs() < 1e-6,
        "Claude /usage should retain the last /context metric"
    );
    assert!((reports[0].signals.quota_5h_pressure.as_ref().unwrap().value - 0.0).abs() < 1e-6);
    assert!(
        (reports[0]
            .signals
            .quota_weekly_pressure
            .as_ref()
            .unwrap()
            .value
            - 0.36)
            .abs()
            < 1e-6
    );

    let reports = run_once(&mut ctx, Instant::now()).expect("cached ok");
    assert!((reports[0].signals.context_pressure.as_ref().unwrap().value - 0.82).abs() < 1e-6);
    assert!((reports[0].signals.quota_5h_pressure.as_ref().unwrap().value - 0.0).abs() < 1e-6);
    assert!(
        (reports[0]
            .signals
            .quota_weekly_pressure
            .as_ref()
            .unwrap()
            .value
            - 0.36)
            .abs()
            < 1e-6
    );

    ctx.source.panes[0].tail = "Opus 4.7 (1M context)·max  CTX —  5h 0%  7d 36%  ~/Qmonster".into();
    let reports = run_once(&mut ctx, Instant::now()).expect("post-clear statusline ok");
    assert!(
        (reports[0].signals.context_pressure.as_ref().unwrap().value - 0.0).abs() < 1e-6,
        "Claude /clear statusline must overwrite the cached pre-clear CTX with 0%"
    );
}

#[test]
fn event_loop_records_token_usage_sample_when_codex_tail_has_in_out() {
    // F-3 Task 2: when a Codex pane's bottom-status tail surfaces
    // token counts ("400K in / 100K out"), the per-poll writer must land
    // a row in token_usage_samples. The existing Codex adapter
    // populates SignalSet.input_tokens / output_tokens from that
    // surface, so this test pins the sink-write step in event_loop.rs.
    use qmonster::store::{QmonsterPaths, SqliteTokenUsageSink};

    let td = tempfile::TempDir::new().unwrap();
    let paths = QmonsterPaths::at(td.path());
    paths.ensure().unwrap();

    // Codex bottom-status tail that the adapter is known to parse for
    // input_tokens=400_000 and output_tokens=100_000. The format mirrors
    // the busy_codex_tail helper used elsewhere in this file.
    let tail = "Context 50% left · /tmp · gpt-5.4 · Qmonster · main · Context 50% used · 0.122.0 · \
         500K used · 400K in · 100K out";

    let source = FixturePaneSource {
        panes: vec![pane("%1", "codex:1:main", "node", tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let audit_sink = Box::new(InMemorySink::new());
    let token_sink = SqliteTokenUsageSink::open(&paths.sqlite_path())
        .expect("SqliteTokenUsageSink::open must succeed on a fresh tempdir");

    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, audit_sink)
        .with_token_usage_sink(token_sink);

    run_once(&mut ctx, Instant::now()).expect("run_once must succeed");

    // Open a fresh read-side sink and confirm at least one row was written.
    let read_sink = SqliteTokenUsageSink::open(&paths.sqlite_path())
        .expect("read-side SqliteTokenUsageSink::open must succeed");
    let got = read_sink
        .recent_samples("%1", 5)
        .expect("recent_samples must succeed");

    assert!(
        !got.is_empty(),
        "F-3: event loop must persist at least one token_usage sample after a Codex tail with in/out"
    );
    assert_eq!(got[0].pane_id, "%1");
    assert!(
        got[0].input_tokens.is_some() || got[0].output_tokens.is_some(),
        "at least one of input_tokens / output_tokens should be Some; got {:?}",
        got[0]
    );
}

#[test]
fn profile_switch_recommendation_fires_after_window_full_of_errors() {
    // Phase F F-9 (dynamic profile switching): once the operator opts
    // into `[profile_switch] enabled = true`, repeated polls of a
    // Claude main pane with an error_hint marker (here a Rust panic
    // line) must fill the per-pane error window and trigger the
    // profile-switch recommendation.
    use qmonster::app::config::ProfileSwitchConfig;

    // Tail with a `panic:` line at start — caught by detect_error_hint
    // — plus the busy_tail volume so the pane is not idle/waiting.
    let mut error_tail = String::from("panic: index out of bounds at lib.rs:42\n");
    error_tail.push_str(&busy_tail());

    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "node", &error_tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut config = QmonsterConfig::defaults();
    // Small window for fast test — fire once 3-of-3 polls flag errors.
    config.profile_switch = ProfileSwitchConfig {
        enabled: true,
        window_polls: 3,
        error_rate_threshold: 0.5,
    };
    let mut ctx = Context::new(config, source, notifier, sink);

    // Polls 1 & 2: window not yet full; rule must stay silent.
    for _ in 0..2 {
        let reports = run_once(&mut ctx, Instant::now()).expect("run_once ok");
        let pane_report = reports.iter().find(|r| r.pane_id == "%1").unwrap();
        assert!(
            !pane_report
                .recommendations
                .iter()
                .any(|r| r.action.starts_with("profile-switch:")),
            "profile-switch rec must not fire before window is full"
        );
    }

    // Poll 3: window is now full of `error_hint = true`; rule fires.
    let reports = run_once(&mut ctx, Instant::now()).expect("run_once ok");
    let pane_report = reports.iter().find(|r| r.pane_id == "%1").unwrap();
    let rec = pane_report
        .recommendations
        .iter()
        .find(|r| r.action.starts_with("profile-switch:"))
        .expect("profile-switch rec must fire on poll 3 (full error window)");
    assert_eq!(rec.severity, Severity::Concern);
    assert_eq!(rec.source_kind, SourceKind::ProjectCanonical);
    assert!(
        rec.reason.contains("claude-default") && rec.reason.contains("claude-script-low-token"),
        "reason must name both profiles: {:?}",
        rec.reason
    );
}

#[test]
fn profile_switch_recommendation_stays_silent_when_disabled() {
    // Default config — `[profile_switch] enabled = false` — the rule
    // must produce no recommendation regardless of how many error
    // polls accumulate.
    let mut error_tail = String::from("panic: x\n");
    error_tail.push_str(&busy_tail());

    let source = FixturePaneSource {
        panes: vec![pane("%1", "claude:1:main", "node", &error_tail, false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    for _ in 0..15 {
        let reports = run_once(&mut ctx, Instant::now()).expect("run_once ok");
        let pane_report = reports.iter().find(|r| r.pane_id == "%1").unwrap();
        assert!(
            !pane_report
                .recommendations
                .iter()
                .any(|r| r.action.starts_with("profile-switch:")),
            "default config must not surface profile-switch recommendations"
        );
    }
}

#[test]
fn event_loop_anomalies_fire_when_enabled_and_history_full() {
    // Phase 7 v1 Task 10: verify PaneReport.anomalies is populated when
    // [anomaly] enabled=true and AnomalyHistory already contains enough
    // identity-churn history to fire the IdentityChurn detector.
    //
    // Strategy: pre-populate ctx.anomaly_history with 3 alternating
    // (provider, path) snapshots — that gives 2 flips, which meets
    // identity_churn_min_flips=2. Run one tick; the BEFORE push in
    // run_once_with_target adds a 4th entry (a 3rd flip), so the
    // detector fires at High confidence.
    use qmonster::app::config::{AnomalyConfig, AnomalyPromoteConfig};
    use qmonster::app::event_loop::run_once_with_target;
    use qmonster::domain::anomaly::AnomalyKind;
    use qmonster::policy::rules::anomaly::AnomalyHistory;

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly = AnomalyConfig {
        enabled: true,
        window_polls: 5,
        min_confidence: "medium".to_string(),
        identity_churn_min_flips: 2,
        error_burst_threshold: 0.5,
        cache_discontinuity_drop: 0.30,
        cross_pane_cluster_min_findings: 3,
        cost_slope_usd_per_hour: 20.0,
        token_slope_input_per_poll: 20_000,
        memory_growth_mb: 1024.0,
        retention_days: 30,
        promote: AnomalyPromoteConfig::default(),
    };
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()));

    // Pre-populate 3 alternating (provider, path) entries → 2 flips.
    // The live-path BEFORE push will add a 4th entry (same direction as
    // entry 0, so flip count stays at 2 or grows to 3 depending on
    // provider resolution from the tail — either way it meets min_flips=2).
    let mut h = AnomalyHistory::default();
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    h.identity_snapshots
        .push_back(("Claude".to_string(), "/a".to_string()));
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    ctx.anomaly_history.insert("%99".to_string(), h);

    let (reports, _notices) = run_once_with_target(&mut ctx, Instant::now(), None).expect("run ok");

    let pane_report = reports.iter().find(|r| r.pane_id == "%99").unwrap();

    assert!(
        !pane_report.anomalies.is_empty(),
        "PaneReport.anomalies must be non-empty when enabled=true and history has 2+ flips; \
         got: {:?}",
        pane_report.anomalies
    );
    assert!(
        pane_report
            .anomalies
            .iter()
            .any(|s| s.kind == AnomalyKind::IdentityChurn),
        "expected IdentityChurn signal; got: {:?}",
        pane_report.anomalies
    );
}

#[test]
fn event_loop_anomalies_off_baseline_regression() {
    // Phase 7 v1 Task 10: with [anomaly] enabled=false, the
    // eval_anomalies call must be a no-op: PaneReport.anomalies is empty
    // and ctx.anomaly_dedup remains empty even when history is full of
    // churn-worthy snapshots. v1.51.0: anomaly.enabled default flipped
    // to true, so this test now explicitly disables to exercise the
    // off-path it actually covers.
    use qmonster::app::event_loop::run_once_with_target;
    use qmonster::domain::anomaly::AnomalyKind;
    use qmonster::policy::rules::anomaly::AnomalyHistory;

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly.enabled = false;
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()));

    // Pre-populate the same churn-worthy history as the enabled test.
    let mut h = AnomalyHistory::default();
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    h.identity_snapshots
        .push_back(("Claude".to_string(), "/a".to_string()));
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    ctx.anomaly_history.insert("%99".to_string(), h);

    let (reports, _notices) = run_once_with_target(&mut ctx, Instant::now(), None).expect("run ok");

    let pane_report = reports.iter().find(|r| r.pane_id == "%99").unwrap();

    assert!(
        pane_report.anomalies.is_empty(),
        "PaneReport.anomalies must be empty when anomaly.enabled=false; \
         got: {:?}",
        pane_report.anomalies
    );

    // Dedup map must remain empty — eval_anomalies is a no-op, so
    // no (pane_id, AnomalyKind) entries should be inserted.
    assert!(
        ctx.anomaly_dedup
            .keys()
            .filter(|(_, k)| *k == AnomalyKind::IdentityChurn)
            .count()
            == 0,
        "anomaly_dedup must have no IdentityChurn entries when disabled; \
         keys: {:?}",
        ctx.anomaly_dedup.keys().collect::<Vec<_>>()
    );
}

#[test]
fn event_loop_promotes_warning_anomalies_to_recommendations_and_notify() {
    // Phase 7 v2 Task 3: verify that promote_anomalies_to_recommendations is
    // wired into the event loop: Warning+ anomaly signals must appear in
    // PaneReport.recommendations and RequestedEffect::Notify must fire.
    //
    // Strategy: pre-populate ctx.anomaly_history with 3 alternating
    // identity snapshots — that gives 2 flips, meeting
    // identity_churn_min_flips=2. The live-path BEFORE push in
    // run_once_with_target adds a 4th entry; the detector fires
    // IdentityChurn at High confidence → severity=Warning.
    // The promote block must then add a Recommendation and Notify effect.
    use qmonster::app::config::{AnomalyConfig, AnomalyPromoteConfig};
    use qmonster::app::event_loop::run_once_with_target;
    use qmonster::domain::anomaly::AnomalyKind;
    use qmonster::domain::recommendation::RequestedEffect;
    use qmonster::policy::rules::anomaly::AnomalyHistory;

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly = AnomalyConfig {
        enabled: true,
        window_polls: 5,
        min_confidence: "medium".to_string(),
        identity_churn_min_flips: 2,
        error_burst_threshold: 0.5,
        cache_discontinuity_drop: 0.30,
        cross_pane_cluster_min_findings: 3,
        cost_slope_usd_per_hour: 20.0,
        token_slope_input_per_poll: 20_000,
        memory_growth_mb: 1024.0,
        retention_days: 30,
        promote: AnomalyPromoteConfig::default(),
    };
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()));

    // Pre-populate 3 alternating (provider, path) entries → 2 flips.
    let mut h = AnomalyHistory::default();
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    h.identity_snapshots
        .push_back(("Claude".to_string(), "/a".to_string()));
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    ctx.anomaly_history.insert("%99".to_string(), h);

    let (reports, _notices) = run_once_with_target(&mut ctx, Instant::now(), None).expect("run ok");

    let pane_report = reports.iter().find(|r| r.pane_id == "%99").unwrap();

    // The underlying AnomalySignal must still be present (m overlay row).
    assert!(
        pane_report
            .anomalies
            .iter()
            .any(|s| s.kind == AnomalyKind::IdentityChurn),
        "expected IdentityChurn anomaly signal; got: {:?}",
        pane_report.anomalies
    );

    // promote_anomalies_to_recommendations must have added the Recommendation.
    assert!(
        pane_report
            .recommendations
            .iter()
            .any(|r| r.action == "anomaly: identity churn detected"),
        "expected 'anomaly: identity churn detected' recommendation; got: {:?}",
        pane_report.recommendations
    );

    // Notify must have been pushed (Warning+ severity triggers it).
    assert!(
        pane_report.effects.contains(&RequestedEffect::Notify),
        "expected RequestedEffect::Notify in effects; got: {:?}",
        pane_report.effects
    );

    // M-1: deliver_effects must have actually invoked the notifier
    // backend (not merely queued the effect). This guards against the
    // sequencing bug where the promote block ran AFTER deliver_effects,
    // silently dropping desktop notifications.
    let recorded = ctx.notifier.0.lock().unwrap();
    assert!(
        !recorded.is_empty(),
        "RecordingNotifier must have been called for Warning+ anomaly; got: {recorded:?}"
    );
}

#[test]
fn event_loop_no_promotion_when_anomaly_disabled() {
    // Phase 7 v2 Task 3: with [anomaly] enabled=false, no anomaly-
    // driven Recommendation or Notify must appear even when history contains
    // churn-worthy snapshots. v1.51.0: anomaly.enabled default flipped to
    // true, so this disabled-mode regression test explicitly opts back out.
    use qmonster::app::event_loop::run_once_with_target;
    use qmonster::domain::recommendation::RequestedEffect;
    use qmonster::policy::rules::anomaly::AnomalyHistory;

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly.enabled = false;
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()));

    // Pre-populate the same churn-worthy history as the enabled test.
    let mut h = AnomalyHistory::default();
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    h.identity_snapshots
        .push_back(("Claude".to_string(), "/a".to_string()));
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    ctx.anomaly_history.insert("%99".to_string(), h);

    let (reports, _notices) = run_once_with_target(&mut ctx, Instant::now(), None).expect("run ok");

    let pane_report = reports.iter().find(|r| r.pane_id == "%99").unwrap();

    // eval_anomalies returns empty when disabled → no promotion either.
    assert!(
        pane_report.anomalies.is_empty(),
        "PaneReport.anomalies must be empty when anomaly.enabled=false; \
         got: {:?}",
        pane_report.anomalies
    );

    // No anomaly-driven Recommendation should be present.
    assert!(
        !pane_report
            .recommendations
            .iter()
            .any(|r| r.action.starts_with("anomaly:")),
        "no anomaly-driven recommendations when disabled; got: {:?}",
        pane_report.recommendations
    );

    // No anomaly-driven Notify either (nothing to promote → no Notify push).
    assert!(
        !pane_report.effects.contains(&RequestedEffect::Notify),
        "no Notify when anomaly disabled and no other Warning+ recommendations; \
         got: {:?}",
        pane_report.effects
    );
}

#[test]
fn event_loop_v2_detector_history_push_does_not_panic() {
    // Phase 7 v2 detectors Task 10: smoke-test the event_loop wiring.
    // Verify the new pre-rule history push for the 6 v2 deques runs
    // without panic when [anomaly] enabled=true. We don't assert that
    // a specific v2 anomaly fires (synthetic FixturePaneSource doesn't
    // populate cost_usd/input_tokens/etc. signals), only that the live
    // tick completes and PaneReport.anomalies is a valid slice.
    use qmonster::app::config::{AnomalyConfig, AnomalyPromoteConfig};
    use qmonster::app::event_loop::run_once_with_target;

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly = AnomalyConfig {
        enabled: true,
        window_polls: 5,
        min_confidence: "medium".to_string(),
        identity_churn_min_flips: 3,
        error_burst_threshold: 0.5,
        cache_discontinuity_drop: 0.30,
        cross_pane_cluster_min_findings: 3,
        cost_slope_usd_per_hour: 20.0,
        token_slope_input_per_poll: 20_000,
        memory_growth_mb: 1024.0,
        retention_days: 30,
        promote: AnomalyPromoteConfig::default(),
    };
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()));

    // Run a few ticks so the v2 deques accumulate (mostly None values
    // from synthetic source, but the push should not panic).
    for _ in 0..5 {
        let (_reports, _notices) =
            run_once_with_target(&mut ctx, Instant::now(), None).expect("run ok");
    }

    // After 5 ticks, the AnomalyHistory entry for %99 should exist and
    // its v2 deques should be populated (likely all None, since the
    // synthetic source doesn't populate cost/token/memory signals).
    let history = ctx
        .anomaly_history
        .get("%99")
        .expect("AnomalyHistory entry exists after 5 ticks");
    assert_eq!(history.cost_usd_samples.len(), 5);
    assert_eq!(history.input_token_samples.len(), 5);
    assert_eq!(history.output_token_samples.len(), 5);
    assert_eq!(history.process_memory_samples.len(), 5);
    assert_eq!(history.agent_memory_samples.len(), 5);
    assert_eq!(history.subagent_hint_samples.len(), 5);
}

#[test]
fn event_loop_pushes_anomaly_events_into_ring_buffer() {
    // Phase 7 v3 (a+b) Task 12: verify that run_once_with_target pushes
    // every visible AnomalySignal into ctx.anomaly_events_ring with the
    // correct `promoted` bool reflecting the per-kind promotion gate.
    //
    // Strategy: mirror the v1.45.0 IdentityChurn promotion test —
    // pre-populate ctx.anomaly_history with 3 alternating identity
    // snapshots, run one tick, then assert:
    //   - the ring buffer has at least one event
    //   - the IdentityChurn event has promoted=true (default High threshold,
    //     and the IdentityChurn detector emits at High confidence)
    //   - the AnomalyEvent fields (pane_id, kind, confidence, severity)
    //     match the corresponding AnomalySignal in PaneReport.anomalies
    //
    // This guards against the ring buffer being silently empty (push site
    // missing) or carrying the wrong promoted bool (gate decision divergence).
    use qmonster::app::config::{AnomalyConfig, AnomalyPromoteConfig};
    use qmonster::app::event_loop::run_once_with_target;
    use qmonster::domain::anomaly::AnomalyKind;
    use qmonster::policy::rules::anomaly::AnomalyHistory;

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly = AnomalyConfig {
        enabled: true,
        window_polls: 5,
        min_confidence: "medium".to_string(),
        identity_churn_min_flips: 2,
        error_burst_threshold: 0.5,
        cache_discontinuity_drop: 0.30,
        cross_pane_cluster_min_findings: 3,
        cost_slope_usd_per_hour: 20.0,
        token_slope_input_per_poll: 20_000,
        memory_growth_mb: 1024.0,
        retention_days: 30,
        promote: AnomalyPromoteConfig::default(),
    };
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()));

    // Pre-populate 3 alternating (provider, path) entries → 2 flips.
    let mut h = AnomalyHistory::default();
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    h.identity_snapshots
        .push_back(("Claude".to_string(), "/a".to_string()));
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    ctx.anomaly_history.insert("%99".to_string(), h);

    // Sanity: ring is empty before the tick.
    assert!(
        ctx.anomaly_events_ring.is_empty(),
        "ring buffer must start empty"
    );

    let (reports, _notices) = run_once_with_target(&mut ctx, Instant::now(), None).expect("run ok");

    // Ring buffer must capture at least the IdentityChurn signal.
    assert!(
        !ctx.anomaly_events_ring.is_empty(),
        "ring buffer must have events after a tick that fired anomalies"
    );

    let identity_churn_events: Vec<_> = ctx
        .anomaly_events_ring
        .iter()
        .filter(|e| e.kind == AnomalyKind::IdentityChurn)
        .collect();
    assert_eq!(
        identity_churn_events.len(),
        1,
        "exactly one IdentityChurn event in ring; got {} — events: {:?}",
        identity_churn_events.len(),
        ctx.anomaly_events_ring.iter().collect::<Vec<_>>()
    );

    let event = identity_churn_events[0];
    assert!(
        event.promoted,
        "High-confidence IdentityChurn must be promoted at default High threshold"
    );
    assert_eq!(event.pane_id, "%99", "pane_id must match");
    assert_eq!(event.kind, AnomalyKind::IdentityChurn);

    // Cross-check against PaneReport.anomalies — the ring event's confidence
    // and severity must match the source AnomalySignal.
    let pane_report = reports.iter().find(|r| r.pane_id == "%99").unwrap();
    let signal = pane_report
        .anomalies
        .iter()
        .find(|s| s.kind == AnomalyKind::IdentityChurn)
        .expect("IdentityChurn signal in PaneReport.anomalies");
    assert_eq!(
        event.confidence, signal.confidence,
        "ring event confidence must match source signal"
    );
    assert_eq!(
        event.severity, signal.severity,
        "ring event severity must match source signal"
    );
}

#[test]
fn event_loop_persists_anomaly_event_to_audit_db() {
    // Phase 7 v3 (c) Task 12: verify run_once_with_target writes a row
    // to anomaly_events when a signal fires and ctx.anomaly_sink is set.
    use qmonster::app::config::{AnomalyConfig, AnomalyPromoteConfig};
    use qmonster::app::event_loop::run_once_with_target;
    use qmonster::domain::anomaly::AnomalyKind;
    use qmonster::policy::rules::anomaly::AnomalyHistory;
    use qmonster::store::SqliteAnomalySink;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("audit.db");

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly = AnomalyConfig {
        enabled: true,
        window_polls: 5,
        min_confidence: "medium".to_string(),
        identity_churn_min_flips: 2,
        error_burst_threshold: 0.5,
        cache_discontinuity_drop: 0.30,
        cross_pane_cluster_min_findings: 3,
        cost_slope_usd_per_hour: 20.0,
        token_slope_input_per_poll: 20_000,
        memory_growth_mb: 1024.0,
        promote: AnomalyPromoteConfig::default(),
        retention_days: 30,
    };
    let sink = SqliteAnomalySink::open(&db_path).unwrap();
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()))
        .with_anomaly_sink(sink);

    let mut h = AnomalyHistory::default();
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    h.identity_snapshots
        .push_back(("Claude".to_string(), "/a".to_string()));
    h.identity_snapshots
        .push_back(("Codex".to_string(), "/b".to_string()));
    ctx.anomaly_history.insert("%99".to_string(), h);

    let _ = run_once_with_target(&mut ctx, std::time::Instant::now(), None).expect("run ok");

    // Re-open the DB independently and query — verifies the row was committed.
    let read_sink = SqliteAnomalySink::open(&db_path).unwrap();
    let events = read_sink.fetch_recent_anomaly_events(10);
    assert!(
        events
            .iter()
            .any(|e| e.kind == AnomalyKind::IdentityChurn && e.pane_id == "%99"),
        "expected persisted IdentityChurn event for %99; got {events:?}"
    );
}

#[test]
fn event_loop_replays_anomaly_history_on_first_observation() {
    // Phase 7 v3 (c) Task 12: pre-populate anomaly_history_snapshots
    // for a pane; verify run_once_with_target hydrates the in-memory
    // AnomalyHistory deques on the first tick of a fresh Context.
    use qmonster::app::config::{AnomalyConfig, AnomalyPromoteConfig};
    use qmonster::app::event_loop::{current_unix_ms, run_once_with_target};
    use qmonster::store::{AnomalyHistorySnapshot, SqliteAnomalySink};
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("audit.db");

    // Seed disk with 3 recent snapshots for pane %99.
    let writer_sink = SqliteAnomalySink::open(&db_path).unwrap();
    let now_secs = (current_unix_ms() / 1000) as u64;
    for offset in [4u64, 2, 0] {
        let snap = AnomalyHistorySnapshot {
            identity: Some(("Codex".to_string(), format!("/p{offset}"))),
            error_hint: Some(false),
            cache_hit_ratio: None,
            cache_drift_fire: false,
            cross_pane_edit_paths: Vec::new(),
            cost_usd: Some(1.0 + offset as f64 * 0.5),
            input_tokens: Some(100 + offset),
            output_tokens: Some(50 + offset),
            process_memory_mb: Some(120.0 + offset as f64),
            agent_memory_bytes: Some(1_000_000),
            subagent_hint: false,
        };
        writer_sink
            .upsert_anomaly_history_snapshot("%99", now_secs - offset, &snap)
            .unwrap();
    }
    drop(writer_sink); // release the lock before reopening

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly = AnomalyConfig {
        enabled: true,
        window_polls: 20,
        min_confidence: "medium".to_string(),
        identity_churn_min_flips: 5, // high threshold so IdentityChurn doesn't fire
        error_burst_threshold: 0.5,
        cache_discontinuity_drop: 0.30,
        cross_pane_cluster_min_findings: 3,
        cost_slope_usd_per_hour: 20.0,
        token_slope_input_per_poll: 20_000,
        memory_growth_mb: 1024.0,
        promote: AnomalyPromoteConfig::default(),
        retention_days: 30,
    };

    let sink = SqliteAnomalySink::open(&db_path).unwrap();
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()))
        .with_anomaly_sink(sink);

    // Fresh context: no in-memory history for %99.
    assert!(!ctx.anomaly_history.contains_key("%99"));

    let _ = run_once_with_target(&mut ctx, std::time::Instant::now(), None).expect("run ok");

    // After one tick, the deques should contain the 3 replayed snapshots
    // PLUS the one observation from the live tick.
    let history = ctx
        .anomaly_history
        .get("%99")
        .expect("history was hydrated for %99");
    assert!(
        history.cost_usd_samples.len() >= 3,
        "expected >= 3 cost_usd samples after replay+live; got {}",
        history.cost_usd_samples.len()
    );
}

#[test]
fn context_initializes_insights_load_channel_and_request_id() {
    let source = FixturePaneSource { panes: vec![] };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    assert_eq!(ctx.next_insights_request_id, 0);
    ctx.insights_load_tx
        .send(qmonster::app::insights_load::InsightsLoadOutcome {
            request_id: 99,
            result: Err("ping".into()),
        })
        .unwrap();
    let outcome = ctx.insights_load_rx.recv().unwrap();
    assert_eq!(outcome.request_id, 99);
}

#[test]
fn insights_load_channel_supports_try_recv_drain() {
    let source = FixturePaneSource { panes: vec![] };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    ctx.insights_load_tx
        .send(qmonster::app::insights_load::InsightsLoadOutcome {
            request_id: 5,
            result: Err("nope".into()),
        })
        .unwrap();

    let outcome = ctx
        .insights_load_rx
        .try_recv()
        .expect("queued outcome should be available without blocking");
    assert_eq!(outcome.request_id, 5);
    assert!(outcome.result.is_err());
    assert!(ctx.insights_load_rx.try_recv().is_err());
}

#[test]
fn agy_pane_is_identified_but_emits_no_anomaly_or_recommendation() {
    // Task 7 — ObserveOnly contract end-to-end lock.
    //
    // A 2-pane workspace: Claude main (%1) + agy:1:research (%3).
    // After one full event-loop tick:
    //   1. The agy pane resolves to Provider::Antigravity.
    //   2. The agy pane's anomalies are empty (matrix ObserveOnly gate).
    //   3. The agy pane's recommendations are empty (no policy rule fires).
    //   4. The Claude pane's anomalies and recommendations carry no
    //      Antigravity reference (cross-pollination guard).
    //
    // anomaly.enabled is left at its default (true) so the anomaly matrix
    // is fully active — the empty result proves the ObserveOnly contract
    // at the matrix level, not the off-path shortcircuit.
    let source = FixturePaneSource {
        panes: vec![
            pane("%1", "claude:1:main", "claude", "", false),
            pane("%3", "agy:1:research", "agy", "", false),
        ],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let mut ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    let reports = run_once(&mut ctx, Instant::now()).expect("ok");
    assert_eq!(reports.len(), 2, "both panes must appear in the snapshot");

    // --- Property 1 + 2 + 3: agy pane ---
    let agy = reports
        .iter()
        .find(|r| r.pane_id == "%3")
        .expect("agy pane must appear in the snapshot");

    assert_eq!(
        agy.identity.identity.provider,
        Provider::Antigravity,
        "agy pane must resolve to Provider::Antigravity"
    );
    assert!(
        agy.anomalies.is_empty(),
        "agy pane must emit no anomalies (ObserveOnly contract); got: {:?}",
        agy.anomalies
    );
    assert!(
        agy.recommendations.is_empty(),
        "agy pane must emit no recommendations (ObserveOnly contract); got: {:?}",
        agy.recommendations
    );

    // --- Property 4: Claude pane cross-pollination guard ---
    let claude = reports
        .iter()
        .find(|r| r.pane_id == "%1")
        .expect("claude pane must appear in the snapshot");

    assert_eq!(
        claude.identity.identity.provider,
        Provider::Claude,
        "claude pane must resolve to Provider::Claude"
    );
    for a in &claude.anomalies {
        assert!(
            !format!("{a:?}").contains("Antigravity"),
            "claude pane anomaly must not reference Antigravity; got: {a:?}"
        );
    }
    for r in &claude.recommendations {
        assert!(
            !format!("{r:?}").contains("Antigravity"),
            "claude pane recommendation must not reference Antigravity; got: {r:?}"
        );
    }
}
