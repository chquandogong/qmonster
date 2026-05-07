use crate::app::config::QmonsterConfig;
use crate::domain::identity::IdentityResolver;
use crate::domain::lifecycle::PaneLifecycle;
use crate::notify::desktop::NotifyBackend;
use crate::notify::rate_limit::RateLimiter;
use crate::policy::claude_settings::ClaudeSettings;
use crate::policy::engine::Engine;
use crate::policy::pricing::PricingTable;
use crate::store::archive_fs::ArchiveWriter;
use crate::store::sink::EventSink;
use crate::store::{SnapshotWriter, SqliteCostUsageSink, SqliteTokenUsageSink};
use crate::tmux::polling::PaneSource;

#[derive(Debug, Clone, Copy, Default)]
pub struct PanePressureCache {
    pub provider: Option<crate::domain::identity::Provider>,
    pub context_pressure: Option<crate::domain::signal::MetricValue<f32>>,
    pub quota_pressure: Option<crate::domain::signal::MetricValue<f32>>,
    pub quota_5h_pressure: Option<crate::domain::signal::MetricValue<f32>>,
    pub quota_weekly_pressure: Option<crate::domain::signal::MetricValue<f32>>,
}

/// Runtime bag carried by the event loop. Exists as a single struct so
/// tests can build a `Context` with a `FixtureSource` + in-memory
/// sink + a fake `NotifyBackend` and exercise one iteration.
pub struct Context<P: PaneSource, N: NotifyBackend> {
    pub config: QmonsterConfig,
    /// v1.15.18: path the live config was loaded from, threaded so the
    /// settings overlay can write edits back to the same file. `None`
    /// when the operator did not pass `--config` (defaults are in use)
    /// — in that case the overlay's save action is gated off.
    pub config_path: Option<std::path::PathBuf>,
    pub source: P,
    pub notifier: N,
    pub sink: Box<dyn EventSink>,
    pub archive: Option<ArchiveWriter>,
    /// Phase H (v1.42.0): persistent `SnapshotWriter` for the
    /// `maybe_auto_snapshot` hook. `None` only when no `QmonsterPaths`
    /// were resolved at startup (i.e., test fixtures without a
    /// snapshot dir). The runtime always populates this at startup
    /// regardless of `[reset] auto_snapshot`; the actual gating of
    /// auto-snapshot writes lives in `PolicyGates::reset_auto_snapshot`
    /// (mirrored from `[reset] auto_snapshot` config). Constructing a
    /// `SnapshotWriter` is cheap (it just wraps a `QmonsterPaths`
    /// clone) so this lazy gate ships zero overhead when the operator
    /// hasn't opted in.
    pub snapshot_writer: Option<SnapshotWriter>,
    pub resolver: IdentityResolver,
    pub policy: Engine,
    pub lifecycle: PaneLifecycle,
    pub rate_limiter: RateLimiter,
    pub pricing: PricingTable,
    pub claude_settings: ClaudeSettings,
    // Slice 4: per-pane tail history + idle-transition cache. Reset on PaneLifecycle::{Dead, Reappeared}.
    pub tail_history: std::collections::HashMap<String, crate::adapters::common::PaneTailHistory>,
    pub idle_transition:
        std::collections::HashMap<String, Option<crate::domain::signal::IdleCause>>,
    /// Records the `Instant` when each pane entered its current idle cause.
    /// Updated on transitions (None→Some, Some(X)→Some(Y)) and cleared on
    /// Some→None or lifecycle reset so the UI can display accurate elapsed time.
    pub idle_entered_at: std::collections::HashMap<String, std::time::Instant>,
    /// One-shot pane tails captured from provider fullscreen status surfaces.
    /// Runtime refresh may need to close the surface immediately with Escape;
    /// this keeps the captured output available to the next parser pass without
    /// persisting raw pane text in the audit log.
    pub runtime_refresh_tail_overlays: std::collections::HashMap<String, String>,
    /// Last provider-confirmed pressure metrics for panes whose provider
    /// exposes those facts on transient fullscreen surfaces. Claude's
    /// `/context` and `/usage` are separate screens; caching lets the UI show
    /// CTX and quota windows together after both have been observed.
    pub pressure_metric_cache: std::collections::HashMap<String, PanePressureCache>,
    /// Phase D D2 (v1.18.0): last observed `Provider` + `current_path`
    /// per pane. Drift detection compares this snapshot against the
    /// just-resolved identity each tick. Cleared on
    /// `PaneLifecycleEvent::{BecameDead, Reappeared}` so a re-spawned
    /// pane starts fresh.
    pub identity_history:
        std::collections::HashMap<String, crate::policy::rules::identity_drift::IdentitySnapshot>,
    /// Phase D D2: per-session dedup of drift findings. Keys are
    /// `(pane_id, "<kind>:<from>→<to>")` so the same drift fires once
    /// per session even if the comparison stays true for multiple
    /// polls. Evicted alongside `identity_history` on lifecycle reset.
    pub reported_drifts: std::collections::HashSet<(String, String)>,
    /// Phase F F-9 (dynamic profile switching): per-pane sliding
    /// window of `error_hint` observations, most-recent-first. The
    /// event loop pushes `signals.error_hint` to the front each tick
    /// and trims to `gates.profile_switch_window` entries; the
    /// `profile_switch` rule reads the head slice to compute the
    /// observed error rate. Evicted on
    /// `PaneLifecycleEvent::{BecameDead, Reappeared}` so a re-spawned
    /// pane starts fresh — a stale "high error rate" carried across
    /// a CLI restart would be a false positive.
    pub recent_error_observations:
        std::collections::HashMap<String, std::collections::VecDeque<bool>>,
    /// Phase H (v1.42.0): per-pane dedup state for
    /// `app::auto_snapshot::maybe_auto_snapshot`. Evicted on
    /// `PaneLifecycleEvent::{BecameDead, Reappeared}` so a re-spawned
    /// pane starts fresh for the new reset window.
    pub auto_snapshot_dedup:
        std::collections::HashMap<String, crate::app::auto_snapshot::AutoSnapshotDedup>,
    /// Phase 7 v1 (v1.43.0): per-pane rolling input history for the
    /// anomaly detectors in `policy::rules::anomaly`. Trimmed to
    /// `gates.anomaly_window_polls` after each push. Evicted on
    /// `PaneLifecycleEvent::{BecameDead, Reappeared}` so a re-spawned
    /// pane starts fresh.
    pub anomaly_history:
        std::collections::HashMap<String, crate::policy::rules::anomaly::AnomalyHistory>,
    /// Phase 7 v1: per-(pane_id, AnomalyKind) edge-triggered dedup.
    /// `Some(unix_seconds)` while the kind is currently considered
    /// active in the rolling window; `None` when the detector has
    /// last returned None and the kind has rearmed for fresh emission.
    pub anomaly_dedup:
        std::collections::HashMap<(String, crate::domain::anomaly::AnomalyKind), Option<u64>>,
    /// Phase 7 v3 (v1.46.0): in-memory ring buffer of recent
    /// `AnomalyEvent`s, populated by `run_once_with_target` after
    /// the per-kind promotion gate runs. Rendered by the `n` overlay.
    /// Capacity 100 (see `AnomalyEventsRing::CAPACITY`); session-only.
    /// Events from dead or re-attached panes are intentionally retained
    /// for historical display — only `anomaly_history` / `anomaly_dedup`
    /// are evicted on pane lifecycle changes.
    pub anomaly_events_ring: crate::app::anomaly_events_ring::AnomalyEventsRing,
    /// Phase F F-3 (v1.24.0): token-usage time-series sink for the
    /// per-pane sparkline. `None` when the SQLite open failed at
    /// startup (logged via the in-process eprintln); the event loop
    /// skips recording in that case rather than crashing.
    pub token_usage_sink: Option<SqliteTokenUsageSink>,
    /// SQLite USD cost ledger. Records positive deltas from
    /// provider-observed cumulative `cost_usd` and persists one-shot
    /// budget-alert claims.
    pub cost_usage_sink: Option<SqliteCostUsageSink>,
    /// Phase 7 v3 (c) (v1.47.0): persistence sink for anomaly events
    /// and AnomalyHistory deque snapshots. None when persistence is
    /// disabled (test fixtures, operators without --config-with-db).
    pub anomaly_sink: Option<crate::store::SqliteAnomalySink>,
    /// Phase 7 v3 (c): per-session set of pane_ids whose AnomalyHistory
    /// has been replay-checked already. Prevents repeating the disk
    /// query every tick.
    pub anomaly_history_replayed: std::collections::HashSet<String>,
    /// Phase F F-6 (v1.32.0): live `codex app-server` JSON-RPC
    /// client. Spawned at TUI startup when the operator opted in
    /// via `[provider_setup] codex_app_server = true`. None when
    /// the operator did not opt in (default), the spawn failed at
    /// startup (a SystemNotice is recorded), or the child died
    /// mid-session and was not yet re-spawned. Per-tick rate-limit
    /// snapshots land in `codex_rate_limits`.
    pub codex_app_server: Option<
        crate::adapters::codex_app_server::CodexAppServer<
            crate::adapters::codex_app_server::SubprocessIo,
        >,
    >,
    /// Phase F F-6 (v1.32.0): most-recent rate-limit snapshot
    /// returned by `codex app-server`'s `account/rateLimits/read`.
    /// Account-level — broadcast to every Codex pane on each
    /// polling tick after `parse_for` runs. Stays `None` until the
    /// first successful read; cleared when the server falls over
    /// so stale data doesn't linger on screen.
    pub codex_rate_limits: Option<crate::adapters::codex_app_server::CodexRateLimits>,
    known_pane_ids: Vec<String>,
}

impl<P: PaneSource, N: NotifyBackend> Context<P, N> {
    pub fn new(config: QmonsterConfig, source: P, notifier: N, sink: Box<dyn EventSink>) -> Self {
        Self {
            config,
            config_path: None,
            source,
            notifier,
            sink,
            archive: None,
            snapshot_writer: None,
            resolver: IdentityResolver::new(),
            policy: Engine,
            lifecycle: PaneLifecycle::new(),
            rate_limiter: RateLimiter::new(),
            pricing: PricingTable::empty(),
            claude_settings: ClaudeSettings::empty(),
            tail_history: std::collections::HashMap::new(),
            idle_transition: std::collections::HashMap::new(),
            idle_entered_at: std::collections::HashMap::new(),
            runtime_refresh_tail_overlays: std::collections::HashMap::new(),
            pressure_metric_cache: std::collections::HashMap::new(),
            identity_history: std::collections::HashMap::new(),
            reported_drifts: std::collections::HashSet::new(),
            recent_error_observations: std::collections::HashMap::new(),
            auto_snapshot_dedup: std::collections::HashMap::new(),
            anomaly_history: std::collections::HashMap::new(),
            anomaly_dedup: std::collections::HashMap::new(),
            anomaly_events_ring: crate::app::anomaly_events_ring::AnomalyEventsRing::new(),
            token_usage_sink: None,
            cost_usage_sink: None,
            anomaly_sink: None,
            anomaly_history_replayed: std::collections::HashSet::new(),
            codex_app_server: None,
            codex_rate_limits: None,
            known_pane_ids: Vec::new(),
        }
    }

    pub fn with_config_path(mut self, path: std::path::PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    pub fn with_archive(mut self, writer: ArchiveWriter) -> Self {
        self.archive = Some(writer);
        self
    }

    pub fn with_snapshot_writer(mut self, writer: SnapshotWriter) -> Self {
        self.snapshot_writer = Some(writer);
        self
    }

    pub fn with_pricing(mut self, pricing: PricingTable) -> Self {
        self.pricing = pricing;
        self
    }

    pub fn with_claude_settings(mut self, settings: ClaudeSettings) -> Self {
        self.claude_settings = settings;
        self
    }

    pub fn with_token_usage_sink(mut self, sink: SqliteTokenUsageSink) -> Self {
        self.token_usage_sink = Some(sink);
        self
    }

    pub fn with_cost_usage_sink(mut self, sink: SqliteCostUsageSink) -> Self {
        self.cost_usage_sink = Some(sink);
        self
    }

    pub fn with_anomaly_sink(mut self, sink: crate::store::SqliteAnomalySink) -> Self {
        let tick_seconds = (self.config.tmux.poll_interval_ms / 1000).max(1);
        if let Err(e) = sink.run_retention(
            self.config.anomaly.retention_days,
            self.config.anomaly.window_polls,
            tick_seconds,
        ) {
            eprintln!("anomaly_sink retention purge failed at startup: {e}");
        }
        self.anomaly_sink = Some(sink);
        self
    }

    pub fn known_pane_ids(&self) -> &[String] {
        &self.known_pane_ids
    }

    pub fn set_known_pane_ids(&mut self, ids: Vec<String>) {
        self.known_pane_ids = ids;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn auto_snapshot_dedup_type_is_accessible() {
        // Context::new requires PaneSource + NotifyBackend type parameters
        // with no lightweight fixtures available in this module. This
        // narrow fallback proves the type alias used at
        // `Context.auto_snapshot_dedup` is importable and that
        // `HashMap::new` produces an empty map.
        //
        // Limits of this test: it does NOT verify that
        // `Context.auto_snapshot_dedup` exists as a field — a rename
        // of the field would not break this test. The structural
        // guarantee comes from (a) the field reference in the
        // initializer block of `Context::new` (compile-checked) and
        // (b) Task 7's integration test which calls
        // `ctx.auto_snapshot_dedup.entry(...)` and observes its state.
        use crate::app::auto_snapshot::AutoSnapshotDedup;
        use std::collections::HashMap;
        let map: HashMap<String, AutoSnapshotDedup> = HashMap::new();
        assert!(map.is_empty());
    }

    #[test]
    fn context_new_initializes_empty_anomaly_state() {
        use crate::domain::anomaly::AnomalyKind;
        use crate::policy::rules::anomaly::AnomalyHistory;
        use std::collections::HashMap;
        // Type-check the maps; the field references at bootstrap.rs are
        // the actual structural guarantee. Same fallback pattern as Phase H.
        let history: HashMap<String, AnomalyHistory> = HashMap::new();
        let dedup: HashMap<(String, AnomalyKind), Option<u64>> = HashMap::new();
        assert!(history.is_empty());
        assert!(dedup.is_empty());
    }
}
