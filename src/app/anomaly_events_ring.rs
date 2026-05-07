//! Phase 7 v3 (v1.46.0): bounded in-memory ring buffer of recent
//! `AnomalyEvent` records. Pushed by `event_loop` after
//! `eval_anomalies` and the per-kind promotion gate run; rendered
//! by the `n` overlay (`src/ui/anomaly_overlay.rs`).
//!
//! Capacity is a `const` (100). Operator-configurable size is
//! deferred. Buffer resets on every session start (no persistence;
//! that is the (c) sub-feature of Phase 7 v3, deferred to v1.47+).

use crate::domain::anomaly::AnomalyEvent;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct AnomalyEventsRing {
    events: VecDeque<AnomalyEvent>,
}

impl Default for AnomalyEventsRing {
    fn default() -> Self {
        Self::new()
    }
}

impl AnomalyEventsRing {
    pub const CAPACITY: usize = 100;

    pub fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(Self::CAPACITY),
        }
    }

    pub fn push(&mut self, event: AnomalyEvent) {
        if self.events.len() >= Self::CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn iter(&self) -> impl Iterator<Item = &AnomalyEvent> + '_ {
        self.events.iter()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
    use crate::domain::recommendation::Severity;

    fn fixture_event(ts: u64) -> AnomalyEvent {
        AnomalyEvent {
            timestamp: ts,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: format!("cost_usd at ts={ts}"),
        }
    }

    #[test]
    fn ring_starts_empty() {
        let ring = AnomalyEventsRing::new();
        assert!(ring.is_empty());
        assert_eq!(ring.len(), 0);
    }

    #[test]
    fn ring_fills_to_exact_capacity_without_eviction() {
        let mut ring = AnomalyEventsRing::new();
        for ts in 0..(AnomalyEventsRing::CAPACITY as u64) {
            ring.push(fixture_event(ts));
        }
        assert_eq!(ring.len(), AnomalyEventsRing::CAPACITY);
        assert!(!ring.is_empty());
        // No eviction: first item is ts=0, last is ts=CAPACITY-1
        let first = ring.iter().next().expect("non-empty");
        assert_eq!(first.timestamp, 0);
        let last = ring.iter().last().expect("non-empty");
        assert_eq!(last.timestamp, (AnomalyEventsRing::CAPACITY as u64) - 1);
    }

    #[test]
    fn ring_push_below_capacity_grows_len() {
        let mut ring = AnomalyEventsRing::new();
        for ts in 0..50 {
            ring.push(fixture_event(ts));
        }
        assert_eq!(ring.len(), 50);
        assert!(!ring.is_empty());
    }

    #[test]
    fn ring_push_at_capacity_drops_oldest() {
        let mut ring = AnomalyEventsRing::new();
        for ts in 0..(AnomalyEventsRing::CAPACITY as u64 + 1) {
            ring.push(fixture_event(ts));
        }
        assert_eq!(ring.len(), AnomalyEventsRing::CAPACITY);
        // oldest (ts=0) is evicted; first remaining is ts=1
        let first = ring.iter().next().expect("non-empty");
        assert_eq!(first.timestamp, 1);
    }

    #[test]
    fn ring_iter_yields_insertion_order_oldest_first() {
        let mut ring = AnomalyEventsRing::new();
        ring.push(fixture_event(10));
        ring.push(fixture_event(20));
        ring.push(fixture_event(30));
        let timestamps: Vec<u64> = ring.iter().map(|e| e.timestamp).collect();
        assert_eq!(timestamps, vec![10, 20, 30]);
    }
}
