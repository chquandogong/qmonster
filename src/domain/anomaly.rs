//! Phase 7 v1 (v1.43.0): per-pane anomaly observation types. Pure
//! data only — `Vec<AnomalySignal>` rides on `PaneReport`, the m
//! overlay reads it. v1 emits no Recommendation, no Notify, no
//! audit event; severity is informational metadata, not a gate.

use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;

/// One detected anomaly. Built by a `policy::rules::anomaly::detect_*`
/// function and (after edge-triggered dedup) attached to
/// `PaneReport.anomalies`. Observation surface only — no ID, no
/// recommendation_id, no audit linkage.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalySignal {
    pub kind: AnomalyKind,
    pub confidence: AnomalyConfidence,
    pub severity: Severity,
    pub evidence: Vec<AnomalyEvidence>,
    /// The rolling-window length the detector inspected (`window_polls`).
    pub window_polls: usize,
    /// Unix seconds when the detector first observed this anomaly in
    /// the current edge-triggered window.
    pub detected_at: u64,
}

/// Phase 7 v1 ships four detector kinds; v2 adds the remaining four
/// (`CostSlope`, `TokenSlope`, `MemoryGrowth`, `SubagentSideEffect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnomalyKind {
    IdentityChurn,
    ErrorBurst,
    CacheDiscontinuity,
    CrossPaneEditCluster,
    CostSlope,
    TokenSlope,
    MemoryGrowth,
    SubagentSideEffect,
}

impl AnomalyKind {
    pub fn label(self) -> &'static str {
        match self {
            AnomalyKind::IdentityChurn => "IdentityChurn",
            AnomalyKind::ErrorBurst => "ErrorBurst",
            AnomalyKind::CacheDiscontinuity => "CacheDiscontinuity",
            AnomalyKind::CrossPaneEditCluster => "CrossPaneEditCluster",
            AnomalyKind::CostSlope => "CostSlope",
            AnomalyKind::TokenSlope => "TokenSlope",
            AnomalyKind::MemoryGrowth => "MemoryGrowth",
            AnomalyKind::SubagentSideEffect => "SubagentSideEffect",
        }
    }
}

/// Detector confidence. Operator filters via
/// `[anomaly] min_confidence`. Detector-specific mapping is
/// documented in `policy::rules::anomaly`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnomalyConfidence {
    Low,
    Medium,
    High,
}

impl AnomalyConfidence {
    pub fn label(self) -> &'static str {
        match self {
            AnomalyConfidence::Low => "low",
            AnomalyConfidence::Medium => "medium",
            AnomalyConfidence::High => "high",
        }
    }
}

/// Per-anomaly evidence row — what changed, by how much, over how
/// many samples, and which `SourceKind` saw it. Never embeds raw
/// pane-tail text.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyEvidence {
    pub metric_name: &'static str,
    pub before: String,
    pub after: String,
    pub sample_count: usize,
    pub source_kind: SourceKind,
}

/// Phase 7 v3 (v1.46.0): record pushed into the in-memory
/// `AnomalyEventsRing` for every visible `AnomalySignal`. Carries the
/// promotion-gate result so the `n` overlay can show which signals
/// became Recommendations and which stayed at passive observation.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyEvent {
    pub timestamp: u64,
    pub pane_id: String,
    pub kind: AnomalyKind,
    pub confidence: AnomalyConfidence,
    pub severity: Severity,
    pub promoted: bool,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_labels_are_stable() {
        assert_eq!(AnomalyKind::IdentityChurn.label(), "IdentityChurn");
        assert_eq!(AnomalyKind::ErrorBurst.label(), "ErrorBurst");
        assert_eq!(
            AnomalyKind::CacheDiscontinuity.label(),
            "CacheDiscontinuity"
        );
        assert_eq!(
            AnomalyKind::CrossPaneEditCluster.label(),
            "CrossPaneEditCluster"
        );
    }

    #[test]
    fn confidence_orders_low_lt_medium_lt_high() {
        assert!(AnomalyConfidence::Low < AnomalyConfidence::Medium);
        assert!(AnomalyConfidence::Medium < AnomalyConfidence::High);
    }

    #[test]
    fn kind_labels_cover_v2_variants() {
        assert_eq!(AnomalyKind::CostSlope.label(), "CostSlope");
        assert_eq!(AnomalyKind::TokenSlope.label(), "TokenSlope");
        assert_eq!(AnomalyKind::MemoryGrowth.label(), "MemoryGrowth");
        assert_eq!(
            AnomalyKind::SubagentSideEffect.label(),
            "SubagentSideEffect"
        );
    }

    #[test]
    fn anomaly_event_construction_smoke() {
        let event = AnomalyEvent {
            timestamp: 1_700_000_000,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: "cost_usd: 0.10 → 25.40".to_string(),
        };
        assert_eq!(event.timestamp, 1_700_000_000);
        assert_eq!(event.kind, AnomalyKind::CostSlope);
        assert!(event.promoted);
        assert_eq!(event.reason, "cost_usd: 0.10 → 25.40");
    }
}
