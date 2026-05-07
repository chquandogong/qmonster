//! Phase 7 v1 (v1.43.0): per-pane anomaly observation rules.
//! Pure detection over `AnomalyHistory`; the orchestrator
//! `eval_anomalies` (Task 9) applies edge-triggered dedup and
//! minimum-confidence filtering.
//!
//! v1 ships four kinds. v2 adds CostSlope, TokenSlope,
//! MemoryGrowth, SubagentSideEffect.

use std::collections::VecDeque;

use crate::domain::anomaly::AnomalySignal;

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

/// Stub — Tasks 5–9 fill in the detectors and orchestrator.
#[allow(dead_code)]
pub fn eval_anomalies() -> Vec<AnomalySignal> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anomaly_history_default_is_empty() {
        let h = AnomalyHistory::default();
        assert!(h.identity_snapshots.is_empty());
        assert!(h.error_hints.is_empty());
        assert!(h.cache_hit_ratios.is_empty());
        assert!(h.cache_drift_fires.is_empty());
        assert!(h.cross_pane_edit_paths.is_empty());
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
