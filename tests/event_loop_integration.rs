use std::sync::{Arc, Mutex};
use std::time::Instant;

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
use qmonster::domain::signal::IdleCause;
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
}
