//! Phase 7 v1 (v1.43.0): per-pane anomaly observation types. Pure
//! data only — `Vec<AnomalySignal>` rides on `PaneReport`, the m
//! overlay reads it. v1 emits no Recommendation, no Notify, no
//! audit event; severity is informational metadata, not a gate.

use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;

/// Canonical action string emitted for `AnomalyKind::SubagentSideEffect`.
/// Single source of truth: the policy rule emits this verbatim, the insights
/// store lists it in `KNOWN_ACTION_PREFIXES`, and the integration tests
/// classify against it via `starts_with`. The leading `⚠ correlation only`
/// disclaimer is intentional (v2.2.0 P1-2 honesty rule).
pub const SUBAGENT_SIDE_EFFECT_ACTION: &str =
    "anomaly: subagent activity ⚠ correlated with other anomalies";

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

    pub fn try_from_label(s: &str) -> Option<Self> {
        Some(match s {
            "IdentityChurn" => AnomalyKind::IdentityChurn,
            "ErrorBurst" => AnomalyKind::ErrorBurst,
            "CacheDiscontinuity" => AnomalyKind::CacheDiscontinuity,
            "CrossPaneEditCluster" => AnomalyKind::CrossPaneEditCluster,
            "CostSlope" => AnomalyKind::CostSlope,
            "TokenSlope" => AnomalyKind::TokenSlope,
            "MemoryGrowth" => AnomalyKind::MemoryGrowth,
            "SubagentSideEffect" => AnomalyKind::SubagentSideEffect,
            _ => return None,
        })
    }

    /// v2.2.0 (P1-1): the set of providers for which this detector can
    /// fire today. Returns an empty slice meaning "all providers" only
    /// when the detector consumes provider-agnostic signals. Otherwise
    /// returns the explicit list of providers whose adapter populates
    /// the detector's required input signal.
    ///
    /// Used by `n` overlay + Settings Rules tab to render
    /// `n/a (provider)` instead of leaving operators to wonder why a
    /// detector is dim. This is the canonical reference for the
    /// detector-provider coverage matrix flagged in the v2.2.0
    /// critical evaluation (the `MemoryGrowth`-on-Claude silent-dead
    /// problem).
    ///
    /// **Provenance**: this matrix mirrors what each `detect_*` function
    /// in `policy::rules::anomaly` actually consumes — verified against
    /// the adapter `SignalSet` writes as of v2.2.0:
    /// - `process_memory_mb`        → Gemini only (status table)
    /// - `cost_usd`                 → Codex (Pricing-derived)
    /// - `cached_input_tokens` /    → Codex tokens (welcome panel),
    ///   `cache_hit_ratio`            Claude % (statusline)
    /// - `subagent_hint` (`● Task`) → Claude tool-call marker
    /// - `error_hint` / identity     → all 3 providers
    pub fn supported_providers(self) -> &'static [crate::domain::identity::Provider] {
        use crate::domain::identity::Provider::*;
        match self {
            // Identity churn observes pane identity over time — works on
            // any pane regardless of provider.
            AnomalyKind::IdentityChurn => &[Claude, Codex, Gemini],
            // Error burst observes `signals.error_hint` populated by the
            // cross-provider `classify_error_hint` matcher.
            AnomalyKind::ErrorBurst => &[Claude, Codex, Gemini],
            // Cache discontinuity needs `cache_hit_ratio` or
            // `cached_input_tokens`. Claude populates the % directly;
            // Codex populates raw counts. Gemini OAuth is structurally
            // unsupported (no cache reads surface).
            AnomalyKind::CacheDiscontinuity => &[Claude, Codex],
            // Cross-pane edit cluster needs `active_files` populated from
            // tool-call markers. Claude `● Edit/Write/MultiEdit/Update`
            // and Codex `*** Update File:` populate; Gemini does not.
            AnomalyKind::CrossPaneEditCluster => &[Claude, Codex],
            // Cost slope needs `cost_usd`. Today only Codex populates it
            // directly. Claude/Gemini panes report cost only when the
            // operator curates `pricing.toml`, but the cost is still
            // populated through the same `signals.cost_usd` path —
            // include both as "supported when pricing curated".
            AnomalyKind::CostSlope => &[Claude, Codex, Gemini],
            // Token slope needs cumulative `input_tokens`. Codex
            // populates from bottom status line. Claude `token_count`
            // surfaces output tokens not cumulative input — leave
            // unsupported until Claude exposes a cumulative input
            // counter. Gemini status table does not expose either.
            AnomalyKind::TokenSlope => &[Codex],
            // Memory growth needs `process_memory_mb`. Only Gemini
            // populates this from its status-table `memory` column.
            AnomalyKind::MemoryGrowth => &[Gemini],
            // Subagent side effect needs `subagent_hint`. The
            // `SUBAGENT_MARKERS` cross-provider phrase list catches
            // `● Task(` (Claude). Codex/Gemini have no equivalent
            // sub-agent surface (their multi-step tools run in-context).
            AnomalyKind::SubagentSideEffect => &[Claude],
        }
    }

    /// True iff this detector can fire for the given provider given the
    /// current adapter coverage.
    pub fn supports_provider(self, provider: crate::domain::identity::Provider) -> bool {
        self.supported_providers().contains(&provider)
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

    pub fn try_from_label(s: &str) -> Option<Self> {
        Some(match s {
            "low" => AnomalyConfidence::Low,
            "medium" => AnomalyConfidence::Medium,
            "high" => AnomalyConfidence::High,
            _ => return None,
        })
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
    pub evidence: Vec<AnomalyEvidence>,
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
    fn anomaly_kind_label_roundtrip() {
        let kinds = [
            AnomalyKind::IdentityChurn,
            AnomalyKind::ErrorBurst,
            AnomalyKind::CacheDiscontinuity,
            AnomalyKind::CrossPaneEditCluster,
            AnomalyKind::CostSlope,
            AnomalyKind::TokenSlope,
            AnomalyKind::MemoryGrowth,
            AnomalyKind::SubagentSideEffect,
        ];
        for kind in kinds {
            assert_eq!(AnomalyKind::try_from_label(kind.label()), Some(kind));
        }
    }

    #[test]
    fn anomaly_kind_try_from_label_rejects_garbage() {
        assert_eq!(AnomalyKind::try_from_label(""), None);
        assert_eq!(AnomalyKind::try_from_label("identitychurn"), None); // case-sensitive
        assert_eq!(AnomalyKind::try_from_label("Bogus"), None);
    }

    #[test]
    fn anomaly_confidence_label_roundtrip() {
        for c in [
            AnomalyConfidence::Low,
            AnomalyConfidence::Medium,
            AnomalyConfidence::High,
        ] {
            assert_eq!(AnomalyConfidence::try_from_label(c.label()), Some(c));
        }
        assert_eq!(AnomalyConfidence::try_from_label("LOW"), None);
        assert_eq!(AnomalyConfidence::try_from_label(""), None);
    }

    #[test]
    fn supported_providers_matrix_locks_v2_2_0_coverage() {
        use crate::domain::identity::Provider;
        // Provider-agnostic: identity + error burst fire for all 3.
        for kind in [AnomalyKind::IdentityChurn, AnomalyKind::ErrorBurst] {
            for p in [Provider::Claude, Provider::Codex, Provider::Gemini] {
                assert!(kind.supports_provider(p), "{:?} must support {:?}", kind, p);
            }
        }

        // Two-provider detectors.
        for kind in [
            AnomalyKind::CacheDiscontinuity,
            AnomalyKind::CrossPaneEditCluster,
        ] {
            assert!(kind.supports_provider(Provider::Claude));
            assert!(kind.supports_provider(Provider::Codex));
            assert!(!kind.supports_provider(Provider::Gemini));
        }

        // Cost slope is supported on all three when pricing is curated.
        for p in [Provider::Claude, Provider::Codex, Provider::Gemini] {
            assert!(AnomalyKind::CostSlope.supports_provider(p));
        }
    }

    #[test]
    fn unknown_and_qmonster_provider_are_never_supported() {
        use crate::domain::identity::Provider;
        let kinds = [
            AnomalyKind::IdentityChurn,
            AnomalyKind::ErrorBurst,
            AnomalyKind::CacheDiscontinuity,
            AnomalyKind::CrossPaneEditCluster,
            AnomalyKind::CostSlope,
            AnomalyKind::TokenSlope,
            AnomalyKind::MemoryGrowth,
            AnomalyKind::SubagentSideEffect,
        ];
        for kind in kinds {
            assert!(
                !kind.supports_provider(Provider::Unknown),
                "{:?} must not claim Unknown support",
                kind
            );
            assert!(
                !kind.supports_provider(Provider::Qmonster),
                "{:?} must not claim Qmonster support",
                kind
            );
        }
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
            evidence: Vec::new(),
        };
        assert_eq!(event.timestamp, 1_700_000_000);
        assert_eq!(event.kind, AnomalyKind::CostSlope);
        assert!(event.promoted);
        assert_eq!(event.reason, "cost_usd: 0.10 → 25.40");
    }
}
