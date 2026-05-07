//! Phase 7 v1 (v1.43.0): per-pane anomaly observation rules.
//! Pure detection over `AnomalyHistory`; the orchestrator
//! `eval_anomalies` (Task 9) applies edge-triggered dedup and
//! minimum-confidence filtering.
//!
//! v1 ships four kinds. v2 adds CostSlope, TokenSlope,
//! MemoryGrowth, SubagentSideEffect.

use std::collections::{HashMap, VecDeque};

use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::policy::gates::PolicyGates;

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

/// CacheDiscontinuity: fires either when `cache_hit_ratio` drops by
/// at least `min_drop` over the window (oldest→newest), OR when
/// F-7b cache-drift fires `>= 2` times inside the window. Confidence
/// is `High` when the drop is `>= 1.5 * min_drop` or when F-7b
/// fired `>= 4` times; otherwise `Medium`. The evidence row
/// records the cache_hit_ratio drop when present, else falls
/// back to the drift-fires count.
pub fn detect_cache_discontinuity(
    history: &AnomalyHistory,
    window_polls: usize,
    min_drop: f32,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    let drift_count = history
        .cache_drift_fires
        .iter()
        .take(window_polls)
        .filter(|b| **b)
        .count();
    let alternate_trigger = drift_count >= 2;

    let ratios: Vec<f32> = history
        .cache_hit_ratios
        .iter()
        .take(window_polls)
        .filter_map(|r| *r)
        .collect();

    let mut drop_value: Option<f32> = None;
    let mut before_ratio: Option<f32> = None;
    let mut after_ratio: Option<f32> = None;
    if ratios.len() >= 2 {
        let newest = ratios.first().copied().unwrap();
        let oldest = ratios.last().copied().unwrap();
        if oldest - newest >= min_drop {
            drop_value = Some(oldest - newest);
            before_ratio = Some(oldest);
            after_ratio = Some(newest);
        }
    }

    if !alternate_trigger && drop_value.is_none() {
        return None;
    }

    let confidence = if drop_value.map(|d| d >= min_drop * 1.5).unwrap_or(false) || drift_count >= 4
    {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };

    let evidence = AnomalyEvidence {
        metric_name: "cache_hit_ratio",
        before: before_ratio
            .map(|r| format!("{r:.2}"))
            .unwrap_or_else(|| format!("drift_fires={drift_count}")),
        after: after_ratio
            .map(|r| format!("{r:.2}"))
            .unwrap_or_else(|| format!("drift_fires={drift_count}")),
        sample_count: ratios.len(),
        source_kind: SourceKind::ProviderOfficial,
    };

    Some(AnomalySignal {
        kind: AnomalyKind::CacheDiscontinuity,
        confidence,
        severity: Severity::Concern,
        evidence: vec![evidence],
        window_polls,
        detected_at: now_unix_seconds,
    })
}

/// Phase 7 v1 orchestrator. Runs every detector against `history`,
/// applies edge-triggered dedup against `dedup` (per `(pane_id,
/// kind)` key), filters by `gates.anomaly_min_confidence`, and
/// returns the surviving signals. Mutates `dedup` in place per the
/// edge-triggered semantics:
///
/// - detector Some + dedup None  → emit; record now in dedup
/// - detector Some + dedup Some  → suppress (still active)
/// - detector None + dedup Some  → rearm (clear dedup)
/// - detector None + dedup None  → no-op
///
/// A signal filtered out by `min_confidence` does NOT record in
/// dedup so a future high-confidence emission can still fire.
pub fn eval_anomalies(
    pane_id: &str,
    history: &AnomalyHistory,
    gates: &PolicyGates,
    dedup: &mut HashMap<(String, AnomalyKind), Option<u64>>,
    now_unix_seconds: u64,
) -> Vec<AnomalySignal> {
    if !gates.anomaly_enabled {
        return Vec::new();
    }

    let kinds_with_results: Vec<(AnomalyKind, Option<AnomalySignal>)> = vec![
        (
            AnomalyKind::IdentityChurn,
            detect_identity_churn(
                history,
                gates.anomaly_window_polls,
                gates.anomaly_identity_churn_min_flips,
                now_unix_seconds,
            ),
        ),
        (
            AnomalyKind::ErrorBurst,
            detect_error_burst(
                history,
                gates.anomaly_window_polls,
                gates.anomaly_error_burst_threshold,
                now_unix_seconds,
            ),
        ),
        (
            AnomalyKind::CacheDiscontinuity,
            detect_cache_discontinuity(
                history,
                gates.anomaly_window_polls,
                gates.anomaly_cache_discontinuity_drop,
                now_unix_seconds,
            ),
        ),
        (
            AnomalyKind::CrossPaneEditCluster,
            detect_cross_pane_edit_cluster(
                history,
                gates.anomaly_window_polls,
                gates.anomaly_cross_pane_cluster_min_findings,
                now_unix_seconds,
            ),
        ),
    ];

    let mut out = Vec::new();
    for (kind, raw) in kinds_with_results {
        let key = (pane_id.to_string(), kind);
        let prev = dedup.get(&key).copied().flatten();
        match (raw, prev) {
            (Some(sig), None) => {
                if sig.confidence >= gates.anomaly_min_confidence {
                    dedup.insert(key, Some(now_unix_seconds));
                    out.push(sig);
                }
                // If filtered by confidence, do NOT record in dedup so
                // a future High-confidence emit can still fire.
            }
            (Some(_sig), Some(_)) => {
                // Suppress — still active in this window.
            }
            (None, Some(_)) => {
                // Rearm: detector returned None this tick.
                dedup.insert(key, None);
            }
            (None, None) => {
                // No-op.
            }
        }
    }
    out
}

/// CrossPaneEditCluster: count `ConcurrentFileEdit` findings per
/// path across the window. Fires when any single path appears
/// `>= min_findings` times. Confidence is `Medium` at threshold,
/// `High` at `>= 2× min_findings`. Evidence records the winning
/// path and its count.
pub fn detect_cross_pane_edit_cluster(
    history: &AnomalyHistory,
    window_polls: usize,
    min_findings: usize,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for tick_paths in history.cross_pane_edit_paths.iter().take(window_polls) {
        for path in tick_paths {
            *counts.entry(path.as_str()).or_insert(0) += 1;
        }
    }
    let (winning_path, winning_count) = counts.iter().max_by_key(|(_, c)| **c)?;
    if *winning_count < min_findings {
        return None;
    }
    let confidence = if *winning_count >= min_findings * 2 {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };
    Some(AnomalySignal {
        kind: AnomalyKind::CrossPaneEditCluster,
        confidence,
        severity: Severity::Concern,
        evidence: vec![AnomalyEvidence {
            metric_name: "concurrent_edit_path",
            before: winning_path.to_string(),
            after: winning_count.to_string(),
            sample_count: *winning_count,
            source_kind: SourceKind::Estimated,
        }],
        window_polls,
        detected_at: now_unix_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
    use crate::policy::gates::PolicyGates;
    use std::collections::HashMap;

    fn fixture_gates() -> PolicyGates {
        PolicyGates {
            anomaly_enabled: true,
            anomaly_window_polls: 20,
            anomaly_min_confidence: AnomalyConfidence::Medium,
            anomaly_identity_churn_min_flips: 3,
            anomaly_error_burst_threshold: 0.5,
            anomaly_cache_discontinuity_drop: 0.30,
            anomaly_cross_pane_cluster_min_findings: 3,
            ..PolicyGates::default()
        }
    }

    #[test]
    fn eval_anomalies_returns_empty_when_disabled() {
        let mut gates = fixture_gates();
        gates.anomaly_enabled = false;
        let history = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let mut dedup: HashMap<(String, AnomalyKind), Option<u64>> = HashMap::new();
        let result = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_000);
        assert!(result.is_empty());
        assert!(dedup.is_empty());
    }

    #[test]
    fn eval_anomalies_edge_triggered_emit_then_suppress_then_rearm_then_emit() {
        let gates = fixture_gates();
        let history = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let mut dedup: HashMap<(String, AnomalyKind), Option<u64>> = HashMap::new();

        // Tick 1: detector returns Some, dedup empty → emit, dedup set
        let r1 = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_000);
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].kind, AnomalyKind::IdentityChurn);

        // Tick 2: detector still Some, dedup occupied → suppress
        let r2 = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_005);
        assert!(r2.is_empty());

        // Tick 3: history clears (only one snapshot left) → detector None, rearm
        let quiet = churn_history(vec![("claude", "/r")]);
        let r3 = eval_anomalies("%1", &quiet, &gates, &mut dedup, 1_700_000_010);
        assert!(r3.is_empty());

        // Tick 4: detector Some again, dedup rearmed → emit
        let r4 = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_015);
        assert_eq!(r4.len(), 1, "should re-emit after rearm");
    }

    #[test]
    fn eval_anomalies_min_confidence_filters_medium() {
        let mut gates = fixture_gates();
        gates.anomaly_min_confidence = AnomalyConfidence::High;
        // Exactly threshold flips → Medium (filtered out at min_confidence=High)
        let history = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let mut dedup: HashMap<(String, AnomalyKind), Option<u64>> = HashMap::new();
        let result = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_000);
        assert!(
            result.is_empty(),
            "Medium signal filtered at min_confidence=High"
        );
        // Critical: a future High signal should still be allowed to emit because
        // the filtered Medium did NOT record dedup.
        let key = ("%1".to_string(), AnomalyKind::IdentityChurn);
        assert!(
            !matches!(dedup.get(&key), Some(Some(_))),
            "filtered signal must not lock dedup"
        );
    }

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

    fn cache_history(ratios: Vec<Option<f32>>, drift_fires: Vec<bool>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for r in ratios.into_iter().rev() {
            h.cache_hit_ratios.push_front(r);
        }
        for d in drift_fires.into_iter().rev() {
            h.cache_drift_fires.push_front(d);
        }
        h
    }

    #[test]
    fn detect_cache_discontinuity_fires_on_30pp_drop() {
        // Newest 10 ticks @ 0.41, older 10 ticks @ 0.85 → drop = 0.44 ≥ 0.30
        // Order: newest first. So [0.41 × 10, 0.85 × 10] is what we pass.
        let mut ratios = vec![Some(0.41); 10];
        ratios.extend(vec![Some(0.85); 10]);
        let h = cache_history(ratios, vec![false; 20]);
        let sig = detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000);
        let s = sig.expect("0.44 drop should fire");
        assert_eq!(s.kind, AnomalyKind::CacheDiscontinuity);
        assert_eq!(s.evidence[0].metric_name, "cache_hit_ratio");
    }

    #[test]
    fn detect_cache_discontinuity_fires_on_repeated_drift() {
        // No 30pp drop, but F-7b fired twice in window
        let mut drift = vec![false; 20];
        drift[0] = true;
        drift[5] = true;
        let h = cache_history(vec![Some(0.50); 20], drift);
        let sig = detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000);
        assert!(sig.is_some(), "alternate trigger: F-7b fired ≥ 2 times");
    }

    #[test]
    fn detect_cache_discontinuity_quiet_window_no_fire() {
        let h = cache_history(vec![Some(0.50); 20], vec![false; 20]);
        assert!(detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000).is_none());
    }

    fn cross_pane_history(per_tick_paths: Vec<Vec<&str>>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for tick in per_tick_paths.into_iter().rev() {
            h.cross_pane_edit_paths
                .push_front(tick.into_iter().map(String::from).collect());
        }
        h
    }

    #[test]
    fn detect_cross_pane_edit_cluster_same_path_threshold() {
        // 4 ticks each emitting `/r/foo.rs` → 4 findings on same path
        let mut ticks: Vec<Vec<&str>> = vec![vec!["/r/foo.rs"]; 4];
        ticks.extend(vec![vec![]; 16]);
        let h = cross_pane_history(ticks);
        let sig = detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000);
        let s = sig.expect("4 same-path findings ≥ 3 threshold");
        assert_eq!(s.kind, AnomalyKind::CrossPaneEditCluster);
        assert_eq!(s.evidence[0].metric_name, "concurrent_edit_path");
    }

    #[test]
    fn detect_cross_pane_edit_cluster_different_paths_no_fire() {
        let h = cross_pane_history(vec![
            vec!["/r/a.rs"],
            vec!["/r/b.rs"],
            vec!["/r/c.rs"],
            vec!["/r/d.rs"],
        ]);
        assert!(detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_cross_pane_edit_cluster_below_threshold() {
        let h = cross_pane_history(vec![vec!["/r/foo.rs"], vec!["/r/foo.rs"]]); // only 2
        assert!(detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000).is_none());
    }
}
