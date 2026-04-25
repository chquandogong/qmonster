//! Multi-poll deterministic test harness for Slice 4 idle-state
//! integration tests. Each `PollSim::feed(tail)` call drives one full
//! `run_once` iteration against a single in-process pane, accumulating
//! signals and emitted alerts so callers can assert transition contracts
//! without touching live tmux or filesystem state.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use qmonster::app::bootstrap::Context;
use qmonster::app::config::QmonsterConfig;
use qmonster::app::event_loop::run_once;
use qmonster::domain::recommendation::{Recommendation, Severity};
use qmonster::domain::signal::SignalSet;
use qmonster::notify::desktop::NotifyBackend;
use qmonster::store::sink::InMemorySink;
use qmonster::tmux::polling::{PaneSource, PollingError};
use qmonster::tmux::types::{RawPaneSnapshot, WindowTarget};

// ---------------------------------------------------------------------------
// Minimal re-implementations of the test fixtures from
// event_loop_integration.rs (those are test-private; we cannot re-export
// them from another integration test file).
// ---------------------------------------------------------------------------

struct SharedPaneSource {
    pane_id: &'static str,
    title: &'static str,
    cmd: &'static str,
    tail: Arc<Mutex<String>>,
}

impl PaneSource for SharedPaneSource {
    fn list_panes(
        &self,
        _target: Option<&WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        Ok(vec![RawPaneSnapshot {
            session_name: "qwork".into(),
            window_index: "1".into(),
            pane_id: self.pane_id.into(),
            title: self.title.into(),
            current_command: self.cmd.into(),
            current_path: "/tmp".into(),
            active: true,
            dead: false,
            tail: self.tail.lock().unwrap().clone(),
        }])
    }
    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        Ok(Some(WindowTarget {
            session_name: "qwork".into(),
            window_index: "1".into(),
        }))
    }
    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        Ok(vec![WindowTarget {
            session_name: "qwork".into(),
            window_index: "1".into(),
        }])
    }
    fn capture_tail(&self, _pane_id: &str, _lines: usize) -> Result<String, PollingError> {
        Ok(self.tail.lock().unwrap().clone())
    }
    fn send_keys(&self, _pane_id: &str, _text: &str) -> Result<(), PollingError> {
        Ok(())
    }
}

struct NullNotifier;

impl NotifyBackend for NullNotifier {
    fn notify(&self, _title: &str, _body: &str, _severity: Severity) {}
}

// ---------------------------------------------------------------------------
// PollSim public API
// ---------------------------------------------------------------------------

/// Multi-poll test harness. Construct with `PollSim::new(stillness_polls)`,
/// then call `feed(tail)` once per simulated iteration.
pub struct PollSim {
    tail: Arc<Mutex<String>>,
    ctx: Context<SharedPaneSource, NullNotifier>,
    /// Last SignalSet produced by `feed`.
    last_signals: SignalSet,
    /// All "pane-state" (or other action) alerts emitted across all feeds.
    emitted_alerts: Vec<Recommendation>,
}

impl PollSim {
    /// Builds a Claude main pane sim with the given stillness window.
    pub fn new(stillness_polls: usize) -> Self {
        Self::with_provider(stillness_polls, "%1", "claude:1:main", "claude")
    }

    /// Builds a Codex review pane sim (use for LimitHit / 5h-limit tests).
    pub fn new_codex(stillness_polls: usize) -> Self {
        Self::with_provider(stillness_polls, "%2", "codex:1:review", "codex")
    }

    fn with_provider(
        stillness_polls: usize,
        pane_id: &'static str,
        title: &'static str,
        cmd: &'static str,
    ) -> Self {
        let tail = Arc::new(Mutex::new(String::new()));
        let source = SharedPaneSource {
            pane_id,
            title,
            cmd,
            tail: Arc::clone(&tail),
        };
        let mut cfg = QmonsterConfig::defaults();
        cfg.idle.stillness_polls = stillness_polls;
        let ctx = Context::new(cfg, source, NullNotifier, Box::new(InMemorySink::new()));
        Self {
            tail,
            ctx,
            last_signals: SignalSet::default(),
            emitted_alerts: Vec::new(),
        }
    }

    /// Run one polling iteration with `tail` as the pane content.
    pub fn feed(&mut self, tail: &str) {
        *self.tail.lock().unwrap() = tail.to_string();
        let reports = run_once(&mut self.ctx, Instant::now()).expect("run_once ok");
        if let Some(rep) = reports.first() {
            self.last_signals = rep.signals.clone();
            for rec in &rep.recommendations {
                self.emitted_alerts.push(rec.clone());
            }
        }
    }

    /// The `SignalSet` produced by the most recent `feed` call.
    pub fn last_signal_set(&self) -> &SignalSet {
        &self.last_signals
    }

    /// All recommendations whose `action == action` emitted across every feed.
    pub fn alerts_emitted_with_action(&self, action: &str) -> Vec<&Recommendation> {
        self.emitted_alerts
            .iter()
            .filter(|r| r.action == action)
            .collect()
    }
}
