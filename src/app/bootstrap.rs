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
use crate::tmux::polling::PaneSource;

/// Runtime bag carried by the event loop. Exists as a single struct so
/// tests can build a `Context` with a `FixtureSource` + in-memory
/// sink + a fake `NotifyBackend` and exercise one iteration.
pub struct Context<P: PaneSource, N: NotifyBackend> {
    pub config: QmonsterConfig,
    pub source: P,
    pub notifier: N,
    pub sink: Box<dyn EventSink>,
    pub archive: Option<ArchiveWriter>,
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
    known_pane_ids: Vec<String>,
}

impl<P: PaneSource, N: NotifyBackend> Context<P, N> {
    pub fn new(config: QmonsterConfig, source: P, notifier: N, sink: Box<dyn EventSink>) -> Self {
        Self {
            config,
            source,
            notifier,
            sink,
            archive: None,
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
            known_pane_ids: Vec::new(),
        }
    }

    pub fn with_archive(mut self, writer: ArchiveWriter) -> Self {
        self.archive = Some(writer);
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

    pub fn known_pane_ids(&self) -> &[String] {
        &self.known_pane_ids
    }

    pub fn set_known_pane_ids(&mut self, ids: Vec<String>) {
        self.known_pane_ids = ids;
    }
}
