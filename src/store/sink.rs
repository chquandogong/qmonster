use std::sync::Mutex;

use crate::domain::audit::AuditEvent;

/// The audit write surface. The signature intentionally accepts only a
/// typed `AuditEvent` — raw tail bytes cannot reach this path, giving
/// us the type-level separation Codex CSF-2 / Gemini G-8 asked for.
/// Phase-2 backends (sqlite, file journal) will implement this trait.
pub trait EventSink: Send + Sync {
    fn record(&self, event: AuditEvent);
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopSink;

impl EventSink for NoopSink {
    fn record(&self, _event: AuditEvent) {}
}

/// Phase-1 in-memory buffer. Zero IO; events live only for the session.
#[derive(Debug, Default)]
pub struct InMemorySink {
    buf: Mutex<Vec<AuditEvent>>,
}

impl InMemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<AuditEvent> {
        self.buf.lock().expect("poisoned").clone()
    }

    pub fn len(&self) -> usize {
        self.buf.lock().expect("poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl EventSink for InMemorySink {
    fn record(&self, event: AuditEvent) {
        self.buf.lock().expect("poisoned").push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::audit::{AuditEvent, AuditEventKind};
    use crate::domain::identity::{Provider, Role};
    use crate::domain::recommendation::Severity;

    fn sample_event(kind: AuditEventKind) -> AuditEvent {
        AuditEvent {
            kind,
            pane_id: "%1".into(),
            severity: Severity::Safe,
            summary: "test".into(),
            provider: Some(Provider::Claude),
            role: Some(Role::Main),
        }
    }

    #[test]
    fn noop_sink_discards_events() {
        let sink = NoopSink;
        sink.record(sample_event(AuditEventKind::AlertFired));
        // Nothing to assert — type signature + clean compile is the contract.
    }

    #[test]
    fn in_memory_sink_retains_events_in_order() {
        let sink = InMemorySink::new();
        sink.record(sample_event(AuditEventKind::PaneIdentityResolved));
        sink.record(sample_event(AuditEventKind::AlertFired));
        let events = sink.snapshot();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, AuditEventKind::PaneIdentityResolved);
        assert_eq!(events[1].kind, AuditEventKind::AlertFired);
    }

    #[test]
    fn in_memory_sink_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<InMemorySink>();
    }
}
