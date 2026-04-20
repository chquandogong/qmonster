use crate::app::config::QmonsterConfig;
use crate::domain::identity::IdentityResolver;
use crate::domain::lifecycle::PaneLifecycle;
use crate::notify::desktop::NotifyBackend;
use crate::notify::rate_limit::RateLimiter;
use crate::policy::engine::Engine;
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
    pub resolver: IdentityResolver,
    pub policy: Engine,
    pub lifecycle: PaneLifecycle,
    pub rate_limiter: RateLimiter,
    known_pane_ids: Vec<String>,
}

impl<P: PaneSource, N: NotifyBackend> Context<P, N> {
    pub fn new(config: QmonsterConfig, source: P, notifier: N, sink: Box<dyn EventSink>) -> Self {
        Self {
            config,
            source,
            notifier,
            sink,
            resolver: IdentityResolver::new(),
            policy: Engine,
            lifecycle: PaneLifecycle::new(),
            rate_limiter: RateLimiter::new(),
            known_pane_ids: Vec::new(),
        }
    }

    pub fn known_pane_ids(&self) -> &[String] {
        &self.known_pane_ids
    }

    pub fn set_known_pane_ids(&mut self, ids: Vec<String>) {
        self.known_pane_ids = ids;
    }
}
