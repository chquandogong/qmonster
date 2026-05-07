//! Phase 7 v1 (v1.43.0): per-pane anomaly observation rules.
//! Pure detection over `AnomalyHistory`; the orchestrator
//! `eval_anomalies` (Task 9) applies edge-triggered dedup and
//! minimum-confidence filtering.
//!
//! v1 ships four kinds. v2 adds CostSlope, TokenSlope,
//! MemoryGrowth, SubagentSideEffect.

use std::collections::VecDeque;

use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;

/// Per-pane rolling history the detectors consume. The event loop
/// pushes one observation per tick (newest pushed to front, trim
/// to `window_polls`). Each field corresponds to one detector.
#[derive(Debug, Default, Clone)]
pub struct AnomalyHistory {
    /// IdentityChurn input: (provider_label, current_path) snapshot
    /// per tick. Most-recent-first.
    pub identity_snapshots: VecDeque<(String, String)>,
    /// ErrorBurst input: `signals.error_hint` per tick. Most-recent-first.
    pub error_hints: VecDeque<bool>,
    /// CacheDiscontinuity input #1: `cache_hit_ratio` per tick when
    /// the signal is populated, else None. Most-recent-first.
    pub cache_hit_ratios: VecDeque<Option<f32>>,
    /// CacheDiscontinuity input #2: did F-7b fire this tick?
    /// Determined by the caller from the recommendation slice.
    pub cache_drift_fires: VecDeque<bool>,
    /// CrossPaneEditCluster input: paths of `ConcurrentFileEdit`
    /// findings emitted this tick, if any. Most-recent-first; each
    /// entry is the set of paths that fired together.
    pub cross_pane_edit_paths: VecDeque<Vec<String>>,
}

impl AnomalyHistory {
    /// Trim every deque to `cap`. Called after each push.
    pub fn trim(&mut self, cap: usize) {
        while self.identity_snapshots.len() > cap {
            self.identity_snapshots.pop_back();
        }
        while self.error_hints.len() > cap {
            self.error_hints.pop_back();
        }
        while self.cache_hit_ratios.len() > cap {
            self.cache_hit_ratios.pop_back();
        }
        while self.cache_drift_fires.len() > cap {
            self.cache_drift_fires.pop_back();
        }
        while self.cross_pane_edit_paths.len() > cap {
            self.cross_pane_edit_paths.pop_back();
        }
    }
}

/// IdentityChurn: count `(provider, path)` flips inside the window.
/// `min_flips` is the threshold; below it, return `None`. Confidence
/// is `Medium` at exactly threshold, `High` at 1.5× threshold or
/// more. Evidence captures the most recent transition only — the
/// last `before` and `after` pair the operator can grep for.
pub fn detect_identity_churn(
    history: &AnomalyHistory,
    window_polls: usize,
    min_flips: usize,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    if history.identity_snapshots.len() < 2 {
        return None;
    }
    // Take up to window_polls newest-first snapshots; iterate adjacent
    // pairs and count flips. The window slice is already DESC; pair[0]
    // is newer, pair[1] is older — so transition is older→newer.
    let window_slice: Vec<&(String, String)> = history
        .identity_snapshots
        .iter()
        .take(window_polls)
        .collect();
    let mut flips = 0usize;
    let mut last_change_after: Option<&(String, String)> = None;
    let mut last_change_before: Option<&(String, String)> = None;
    for pair in window_slice.windows(2) {
        if pair[0] != pair[1] {
            flips += 1;
            // Most-recent transition wins (we iterate newest-first).
            if last_change_after.is_none() {
                last_change_after = Some(pair[0]);
                last_change_before = Some(pair[1]);
            }
        }
    }
    if flips < min_flips {
        return None;
    }
    let confidence = if flips as f32 >= 1.5 * min_flips as f32 {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };
    let before = last_change_before
        .map(|(p, path)| format!("{p}:{path}"))
        .unwrap_or_default();
    let after = last_change_after
        .map(|(p, path)| format!("{p}:{path}"))
        .unwrap_or_default();
    Some(AnomalySignal {
        kind: AnomalyKind::IdentityChurn,
        confidence,
        severity: Severity::Concern,
        evidence: vec![AnomalyEvidence {
            metric_name: "provider_path",
            before,
            after,
            sample_count: flips,
            source_kind: SourceKind::Estimated,
        }],
        window_polls,
        detected_at: now_unix_seconds,
    })
}

/// ErrorBurst: error rate over the most recent `window_polls` of
/// `signals.error_hint`. Returns None when the buffer is shorter
/// than `window_polls` (no false-positive on cold start) or when
/// the rate is below `threshold`. Confidence is `Medium` at
/// threshold, `High` at ≥ 1.5× threshold.
///
/// Evidence captures the first-half vs second-half rate within the
/// window so the operator can see the trend direction; the window
/// is most-recent-first, so the second half (`window[..half]`) is
/// the newer slice and the first half (`window[half..]`) is older.
pub fn detect_error_burst(
    history: &AnomalyHistory,
    window_polls: usize,
    threshold: f32,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    if history.error_hints.len() < window_polls {
        return None;
    }
    let window: Vec<bool> = history
        .error_hints
        .iter()
        .take(window_polls)
        .copied()
        .collect();
    let count = window.iter().filter(|b| **b).count();
    let rate = count as f32 / window_polls as f32;
    if rate < threshold {
        return None;
    }
    let confidence = if rate >= threshold * 1.5 {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };
    let half = window_polls / 2;
    let newer_count = window[..half].iter().filter(|b| **b).count();
    let older_count = window[half..].iter().filter(|b| **b).count();
    let newer_rate = if half == 0 {
        0.0
    } else {
        newer_count as f32 / half as f32
    };
    let older_rate = if window_polls - half == 0 {
        0.0
    } else {
        older_count as f32 / (window_polls - half) as f32
    };
    Some(AnomalySignal {
        kind: AnomalyKind::ErrorBurst,
        confidence,
        severity: Severity::Concern,
        evidence: vec![AnomalyEvidence {
            metric_name: "error_rate",
            before: format!("{older_rate:.2}"),
            after: format!("{newer_rate:.2}"),
            sample_count: window_polls,
            source_kind: SourceKind::Estimated,
        }],
        window_polls,
        detected_at: now_unix_seconds,
    })
}

/// Stub — Tasks 5–9 fill in the detectors and orchestrator.
#[allow(dead_code)]
pub fn eval_anomalies() -> Vec<AnomalySignal> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};

    fn snap(provider: &str, path: &str) -> (String, String) {
        (provider.to_string(), path.to_string())
    }

    fn churn_history(snapshots: Vec<(&str, &str)>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        // The plan: snapshots passed in order [newest, ..., oldest]; we
        // push_front each to keep the same order. Pass them in
        // newest-first, accept reversal in the helper.
        for (p, path) in snapshots.into_iter().rev() {
            h.identity_snapshots.push_front(snap(p, path));
        }
        h
    }

    #[test]
    fn detect_identity_churn_fires_at_threshold() {
        // 4 distinct snapshots → 3 transitions (flips). Threshold = 3.
        let h = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let sig = detect_identity_churn(&h, 20, 3, 1_700_000_000);
        let s = sig.expect("should fire at 3 flips");
        assert_eq!(s.kind, AnomalyKind::IdentityChurn);
        assert!(matches!(
            s.confidence,
            AnomalyConfidence::Medium | AnomalyConfidence::High
        ));
        assert_eq!(s.evidence.len(), 1);
        assert_eq!(s.evidence[0].metric_name, "provider_path");
        assert_eq!(s.evidence[0].sample_count, 3);
    }

    #[test]
    fn detect_identity_churn_below_threshold_returns_none() {
        let h = churn_history(vec![("claude", "/r"), ("codex", "/r")]); // 1 flip
        assert!(detect_identity_churn(&h, 20, 3, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_identity_churn_empty_history_returns_none() {
        let h = AnomalyHistory::default();
        assert!(detect_identity_churn(&h, 20, 3, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_identity_churn_high_confidence_when_far_above_threshold() {
        // 5 flips when threshold is 3 → 5 ≥ 1.5 * 3 = 4.5 → High
        let h = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let sig = detect_identity_churn(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::High);
    }

    #[test]
    fn anomaly_history_default_is_empty() {
        let h = AnomalyHistory::default();
        assert!(h.identity_snapshots.is_empty());
        assert!(h.error_hints.is_empty());
        assert!(h.cache_hit_ratios.is_empty());
        assert!(h.cache_drift_fires.is_empty());
        assert!(h.cross_pane_edit_paths.is_empty());
    }

    fn errors_history(bools: Vec<bool>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        // bools is newest-first; push them in reverse so push_front gives the same ordering
        for b in bools.into_iter().rev() {
            h.error_hints.push_front(b);
        }
        h
    }

    #[test]
    fn detect_error_burst_fires_above_threshold() {
        // 12 errors in 20 ticks → 0.60 ≥ 0.5
        let mut bools = vec![true; 12];
        bools.extend(vec![false; 8]);
        let h = errors_history(bools);
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000);
        let s = sig.expect("should fire at 60% > 50%");
        assert_eq!(s.kind, AnomalyKind::ErrorBurst);
        assert_eq!(s.evidence[0].metric_name, "error_rate");
        assert_eq!(s.evidence[0].sample_count, 20);
    }

    #[test]
    fn detect_error_burst_below_threshold() {
        let mut bools = vec![true; 6];
        bools.extend(vec![false; 14]);
        let h = errors_history(bools);
        assert!(detect_error_burst(&h, 20, 0.5, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_error_burst_insufficient_samples() {
        // only 3 samples; window 20 — must return None
        let h = errors_history(vec![true, true, true]);
        assert!(detect_error_burst(&h, 20, 0.5, 1_700_000_000).is_none());
    }

    #[test]
    fn trim_caps_each_deque_to_capacity() {
        let mut h = AnomalyHistory::default();
        for i in 0..30 {
            h.identity_snapshots
                .push_front((format!("p{i}"), format!("/path{i}")));
            h.error_hints.push_front(i % 2 == 0);
            h.cache_hit_ratios.push_front(Some(i as f32 / 30.0));
            h.cache_drift_fires.push_front(i % 3 == 0);
            h.cross_pane_edit_paths
                .push_front(vec![format!("/abs/file{i}.rs")]);
        }
        h.trim(20);
        assert_eq!(h.identity_snapshots.len(), 20);
        assert_eq!(h.error_hints.len(), 20);
        assert_eq!(h.cache_hit_ratios.len(), 20);
        assert_eq!(h.cache_drift_fires.len(), 20);
        assert_eq!(h.cross_pane_edit_paths.len(), 20);
    }
}
