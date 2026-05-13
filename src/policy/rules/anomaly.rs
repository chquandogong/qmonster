//! Phase 7 v1 (v1.43.0): per-pane anomaly observation rules.
//! Pure detection over `AnomalyHistory`; the orchestrator
//! `eval_anomalies` applies edge-triggered dedup and
//! minimum-confidence filtering.

use std::collections::{HashMap, VecDeque};

use crate::domain::anomaly::{
    AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::policy::gates::PolicyGates;

/// Phase 7 v2 (v1.44.0): map detector confidence to severity.
fn severity_for(confidence: AnomalyConfidence) -> Severity {
    match confidence {
        AnomalyConfidence::High => Severity::Warning,
        _ => Severity::Concern,
    }
}

/// Per-pane rolling history the detectors consume.
#[derive(Debug, Default, Clone)]
pub struct AnomalyHistory {
    pub tick_unix_ms_samples: VecDeque<u64>,
    pub tick_unix_secs_samples: VecDeque<u64>,
    pub identity_snapshots: VecDeque<(String, String)>,
    pub error_hints: VecDeque<bool>,
    pub error_hint_kinds: VecDeque<Option<&'static str>>,
    pub cache_hit_ratios: VecDeque<Option<f32>>,
    pub cache_drift_fires: VecDeque<bool>,
    pub cross_pane_edit_paths: VecDeque<Vec<String>>,
    pub cost_usd_samples: VecDeque<Option<f64>>,
}

impl AnomalyHistory {
    /// Trim every deque to `cap`. Called after each push.
    pub fn trim(&mut self, cap: usize) {
        while self.tick_unix_secs_samples.len() > cap {
            self.tick_unix_secs_samples.pop_back();
        }
        while self.tick_unix_ms_samples.len() > cap {
            self.tick_unix_ms_samples.pop_back();
        }
        while self.identity_snapshots.len() > cap {
            self.identity_snapshots.pop_back();
        }
        while self.error_hints.len() > cap {
            self.error_hints.pop_back();
        }
        while self.error_hint_kinds.len() > cap {
            self.error_hint_kinds.pop_back();
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
        while self.cost_usd_samples.len() > cap {
            self.cost_usd_samples.pop_back();
        }
    }
}

pub fn detect_identity_churn(
    history: &AnomalyHistory,
    window_polls: usize,
    min_flips: usize,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    if history.identity_snapshots.len() < 2 {
        return None;
    }
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
        severity: severity_for(confidence),
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
    let mut evidence = vec![AnomalyEvidence {
        metric_name: "error_rate",
        before: format!("{older_rate:.2}"),
        after: format!("{newer_rate:.2}"),
        sample_count: window_polls,
        source_kind: SourceKind::Estimated,
    }];
    if let Some((dominant_kind, dominant_count)) =
        dominant_error_kind(&history.error_hint_kinds, window_polls)
    {
        evidence.push(AnomalyEvidence {
            metric_name: "dominant_kind",
            before: dominant_kind.to_string(),
            after: format!("{dominant_count}/{count}"),
            sample_count: count,
            source_kind: SourceKind::Estimated,
        });
    }
    Some(AnomalySignal {
        kind: AnomalyKind::ErrorBurst,
        confidence,
        severity: severity_for(confidence),
        evidence,
        window_polls,
        detected_at: now_unix_seconds,
    })
}

fn dominant_error_kind(
    kinds: &VecDeque<Option<&'static str>>,
    window_polls: usize,
) -> Option<(&'static str, usize)> {
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for entry in kinds.iter().take(window_polls).flatten() {
        *counts.entry(*entry).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by(|(la, ca), (lb, cb)| ca.cmp(cb).then_with(|| lb.cmp(la)))
}

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
        severity: severity_for(confidence),
        evidence: vec![evidence],
        window_polls,
        detected_at: now_unix_seconds,
    })
}

pub fn eval_anomalies(
    pane_id: &str,
    provider: crate::domain::identity::Provider,
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
            AnomalyKind::IdentityChurn
                .supports_provider(provider)
                .then(|| {
                    detect_identity_churn(
                        history,
                        gates.anomaly_window_polls,
                        gates.anomaly_identity_churn_min_flips,
                        now_unix_seconds,
                    )
                })
                .flatten(),
        ),
        (
            AnomalyKind::ErrorBurst,
            AnomalyKind::ErrorBurst
                .supports_provider(provider)
                .then(|| {
                    detect_error_burst(
                        history,
                        gates.anomaly_window_polls,
                        gates.anomaly_error_burst_threshold,
                        now_unix_seconds,
                    )
                })
                .flatten(),
        ),
        (
            AnomalyKind::CacheDiscontinuity,
            AnomalyKind::CacheDiscontinuity
                .supports_provider(provider)
                .then(|| {
                    detect_cache_discontinuity(
                        history,
                        gates.anomaly_window_polls,
                        gates.anomaly_cache_discontinuity_drop,
                        now_unix_seconds,
                    )
                })
                .flatten(),
        ),
        (
            AnomalyKind::CrossPaneEditCluster,
            AnomalyKind::CrossPaneEditCluster
                .supports_provider(provider)
                .then(|| {
                    detect_cross_pane_edit_cluster(
                        history,
                        gates.anomaly_window_polls,
                        gates.anomaly_cross_pane_cluster_min_findings,
                        now_unix_seconds,
                    )
                })
                .flatten(),
        ),
        (
            AnomalyKind::CostSlope,
            AnomalyKind::CostSlope
                .supports_provider(provider)
                .then(|| {
                    detect_cost_slope(
                        history,
                        gates.anomaly_window_polls,
                        gates.anomaly_cost_slope_usd_per_hour,
                        now_unix_seconds,
                    )
                })
                .flatten(),
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
            }
            (Some(_sig), Some(_)) => {}
            (None, Some(_)) => {
                dedup.insert(key, None);
            }
            (None, None) => {}
        }
    }

    out
}

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
        severity: severity_for(confidence),
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

fn elapsed_secs_for_window(history: &AnomalyHistory, window_polls: usize) -> Option<f64> {
    let ms_ticks: Vec<u64> = history
        .tick_unix_ms_samples
        .iter()
        .take(window_polls)
        .copied()
        .collect();
    if ms_ticks.len() >= window_polls {
        let newest = *ms_ticks.first()?;
        let oldest = *ms_ticks.last()?;
        let elapsed_ms = newest.saturating_sub(oldest);
        if elapsed_ms > 0 {
            return Some(elapsed_ms as f64 / 1000.0);
        }
    }

    let ticks: Vec<u64> = history
        .tick_unix_secs_samples
        .iter()
        .take(window_polls)
        .copied()
        .collect();
    if ticks.len() < window_polls {
        return None;
    }
    let newest = *ticks.first()?;
    let oldest = *ticks.last()?;
    let elapsed = newest.saturating_sub(oldest);
    (elapsed > 0).then_some(elapsed as f64)
}

const NOMINAL_POLL_INTERVAL_SECS: f64 = 2.0;

fn nominal_elapsed_secs(window_polls: usize) -> f64 {
    (window_polls as f64) * NOMINAL_POLL_INTERVAL_SECS
}

pub fn detect_cost_slope(
    history: &AnomalyHistory,
    window_polls: usize,
    threshold_usd_per_hour: f64,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    if history.cost_usd_samples.len() < window_polls {
        return None;
    }
    let window: Vec<f64> = history
        .cost_usd_samples
        .iter()
        .take(window_polls)
        .filter_map(|s| *s)
        .collect();
    if window.len() < 2 {
        return None;
    }
    let newest = window.first().copied().unwrap();
    let oldest = window.last().copied().unwrap();
    let window_secs = elapsed_secs_for_window(history, window_polls)
        .unwrap_or_else(|| nominal_elapsed_secs(window_polls));
    if window_secs <= 0.0 {
        return None;
    }
    let slope_usd_per_hour = (newest - oldest) / window_secs * 3600.0;
    if slope_usd_per_hour < threshold_usd_per_hour {
        return None;
    }
    let confidence = if slope_usd_per_hour >= threshold_usd_per_hour * 1.5 {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };
    Some(AnomalySignal {
        kind: AnomalyKind::CostSlope,
        confidence,
        severity: severity_for(confidence),
        evidence: vec![
            AnomalyEvidence {
                metric_name: "cost_slope_usd_per_hour",
                before: format!("{oldest:.2}"),
                after: format!("{newest:.2}"),
                sample_count: window.len(),
                source_kind: SourceKind::ProviderOfficial,
            },
            AnomalyEvidence {
                metric_name: "sample_coverage",
                before: format!("{}/{}", window.len(), window_polls),
                after: format!("elapsed_secs={window_secs:.0}"),
                sample_count: window.len(),
                source_kind: SourceKind::Estimated,
            },
        ],
        window_polls,
        detected_at: now_unix_seconds,
    })
}

pub fn promote_anomalies_to_recommendations(
    signals: &[AnomalySignal],
    gates: &PolicyGates,
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    for sig in signals {
        if sig.confidence < gates.promote_min_confidence(sig.kind) {
            continue;
        }
        let evidence = sig.evidence.first();
        let source_kind = evidence
            .map(|e| e.source_kind)
            .unwrap_or(SourceKind::Estimated);
        let (action, reason, next_step) = match sig.kind {
            AnomalyKind::IdentityChurn => {
                let (sample_count, before, after) = evidence
                    .map(|e| (e.sample_count, e.before.clone(), e.after.clone()))
                    .unwrap_or((0, String::new(), String::new()));
                (
                    "anomaly: identity churn detected",
                    format!(
                        "pane identity flipped {sample_count} times in {} polls — verify operator intent or pane reuse; latest flip: {before} → {after}",
                        sig.window_polls
                    ),
                    "pause provider switching; review pane assignments".to_string(),
                )
            }
            AnomalyKind::ErrorBurst => {
                let (newer, older) = evidence
                    .map(|e| (e.after.clone(), e.before.clone()))
                    .unwrap_or((String::new(), String::new()));
                (
                    "anomaly: error burst detected",
                    format!(
                        "error rate {newer} over the most recent half of the {}-poll window (baseline {older}); confidence {}",
                        sig.window_polls,
                        sig.confidence.label()
                    ),
                    "check the recent provider output; consider profile_switch if errors persist"
                        .to_string(),
                )
            }
            AnomalyKind::CacheDiscontinuity => {
                let (before, after) = evidence
                    .map(|e| (e.before.clone(), e.after.clone()))
                    .unwrap_or((String::new(), String::new()));
                (
                    "anomaly: cache discontinuity detected",
                    format!(
                        "cache hit ratio dropped from {before} to {after} (or F-7b cache-drift fired ≥ 2× in {} polls)",
                        sig.window_polls
                    ),
                    "press 's' to snapshot before reset; consider /compact when conversation is large".to_string(),
                )
            }
            AnomalyKind::CrossPaneEditCluster => {
                let (count, path) = evidence
                    .map(|e| (e.after.clone(), e.before.clone()))
                    .unwrap_or((String::new(), String::new()));
                (
                    "anomaly: cross-pane edit cluster detected",
                    format!(
                        "{count} concurrent file-edit findings on `{path}` in {} polls — multiple panes editing the same file",
                        sig.window_polls
                    ),
                    "coordinate edits between panes; the existing F-8 ConcurrentFileEdit findings panel lists which panes".to_string(),
                )
            }
            AnomalyKind::CostSlope => {
                let (oldest, newest) = evidence
                    .map(|e| (e.before.clone(), e.after.clone()))
                    .unwrap_or((String::new(), String::new()));
                (
                    "anomaly: cost slope detected",
                    format!(
                        "cost climbing from {oldest} to {newest} USD over the {} polls window; confidence {}",
                        sig.window_polls,
                        sig.confidence.label()
                    ),
                    "check the recent provider output for runaway loops; review the cost budget"
                        .to_string(),
                )
            }
        };
        out.push(Recommendation {
            action,
            reason,
            severity: sig.severity,
            source_kind,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: Some(next_step),
            profile: None,
        });
    }
    out
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
            anomaly_cost_slope_usd_per_hour: 20.0,
            anomaly_promote_identity_churn: AnomalyConfidence::High,
            anomaly_promote_error_burst: AnomalyConfidence::High,
            anomaly_promote_cache_discontinuity: AnomalyConfidence::High,
            anomaly_promote_cross_pane_edit_cluster: AnomalyConfidence::High,
            anomaly_promote_cost_slope: AnomalyConfidence::High,
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
        let result = eval_anomalies(
            "%1",
            crate::domain::identity::Provider::Claude,
            &history,
            &gates,
            &mut dedup,
            1_700_000_000,
        );
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

        let r1 = eval_anomalies(
            "%1",
            crate::domain::identity::Provider::Claude,
            &history,
            &gates,
            &mut dedup,
            1_700_000_000,
        );
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].kind, AnomalyKind::IdentityChurn);

        let r2 = eval_anomalies(
            "%1",
            crate::domain::identity::Provider::Claude,
            &history,
            &gates,
            &mut dedup,
            1_700_000_005,
        );
        assert!(r2.is_empty());

        let quiet = churn_history(vec![("claude", "/r")]);
        let r3 = eval_anomalies(
            "%1",
            crate::domain::identity::Provider::Claude,
            &quiet,
            &gates,
            &mut dedup,
            1_700_000_010,
        );
        assert!(r3.is_empty());

        let r4 = eval_anomalies(
            "%1",
            crate::domain::identity::Provider::Claude,
            &history,
            &gates,
            &mut dedup,
            1_700_000_015,
        );
        assert_eq!(r4.len(), 1, "should re-emit after rearm");
    }

    #[test]
    fn eval_anomalies_min_confidence_filters_medium() {
        let mut gates = fixture_gates();
        gates.anomaly_min_confidence = AnomalyConfidence::High;
        let history = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let mut dedup: HashMap<(String, AnomalyKind), Option<u64>> = HashMap::new();
        let result = eval_anomalies(
            "%1",
            crate::domain::identity::Provider::Claude,
            &history,
            &gates,
            &mut dedup,
            1_700_000_000,
        );
        assert!(result.is_empty());
        let key = ("%1".to_string(), AnomalyKind::IdentityChurn);
        assert!(!matches!(dedup.get(&key), Some(Some(_))));
    }

    fn snap(provider: &str, path: &str) -> (String, String) {
        (provider.to_string(), path.to_string())
    }

    fn churn_history(snapshots: Vec<(&str, &str)>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for (p, path) in snapshots.into_iter().rev() {
            h.identity_snapshots.push_front(snap(p, path));
        }
        h
    }

    #[test]
    fn detect_identity_churn_fires_at_threshold() {
        let h = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let sig = detect_identity_churn(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.kind, AnomalyKind::IdentityChurn);
    }

    #[test]
    fn detect_identity_churn_below_threshold_returns_none() {
        let h = churn_history(vec![("claude", "/r"), ("codex", "/r")]);
        assert!(detect_identity_churn(&h, 20, 3, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_identity_churn_high_confidence_when_far_above_threshold() {
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

    fn errors_history(bools: Vec<bool>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for b in bools.into_iter().rev() {
            h.error_hints.push_front(b);
        }
        h
    }

    #[test]
    fn detect_error_burst_fires_above_threshold() {
        let mut bools = vec![true; 12];
        bools.extend(vec![false; 8]);
        let h = errors_history(bools);
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000).unwrap();
        assert_eq!(sig.kind, AnomalyKind::ErrorBurst);
    }

    #[test]
    fn detect_error_burst_below_threshold() {
        let mut bools = vec![true; 6];
        bools.extend(vec![false; 14]);
        let h = errors_history(bools);
        assert!(detect_error_burst(&h, 20, 0.5, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_error_burst_evidence_carries_dominant_error_kind() {
        let mut h = AnomalyHistory::default();
        for _ in 0..8 { h.error_hints.push_front(true); h.error_hint_kinds.push_front(Some("rust_panic")); }
        for _ in 0..4 { h.error_hints.push_front(true); h.error_hint_kinds.push_front(Some("error_prefix")); }
        for _ in 0..8 { h.error_hints.push_front(false); h.error_hint_kinds.push_front(None); }
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000).unwrap();
        let dominant = sig.evidence.iter().find(|e| e.metric_name == "dominant_kind").unwrap();
        assert_eq!(dominant.before, "rust_panic");
    }

    #[test]
    fn trim_caps_each_deque_to_capacity() {
        let mut h = AnomalyHistory::default();
        for i in 0..30 {
            h.identity_snapshots.push_front((format!("p{i}"), format!("/path{i}")));
            h.error_hints.push_front(i % 2 == 0);
            h.cache_hit_ratios.push_front(Some(i as f32 / 30.0));
            h.cache_drift_fires.push_front(i % 3 == 0);
            h.cross_pane_edit_paths.push_front(vec![format!("/abs/file{i}.rs")]);
            h.cost_usd_samples.push_front(Some(i as f64));
        }
        h.trim(20);
        assert_eq!(h.identity_snapshots.len(), 20);
        assert_eq!(h.cost_usd_samples.len(), 20);
    }

    fn cache_history(ratios: Vec<Option<f32>>, drift_fires: Vec<bool>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for r in ratios.into_iter().rev() { h.cache_hit_ratios.push_front(r); }
        for d in drift_fires.into_iter().rev() { h.cache_drift_fires.push_front(d); }
        h
    }

    #[test]
    fn detect_cache_discontinuity_fires_on_30pp_drop() {
        let mut ratios = vec![Some(0.41); 10];
        ratios.extend(vec![Some(0.85); 10]);
        let h = cache_history(ratios, vec![false; 20]);
        let sig = detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000).unwrap();
        assert_eq!(sig.kind, AnomalyKind::CacheDiscontinuity);
    }

    fn cross_pane_history(per_tick_paths: Vec<Vec<&str>>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for tick in per_tick_paths.into_iter().rev() {
            h.cross_pane_edit_paths.push_front(tick.into_iter().map(String::from).collect());
        }
        h
    }

    #[test]
    fn detect_cross_pane_edit_cluster_same_path_threshold() {
        let mut ticks: Vec<Vec<&str>> = vec![vec!["/r/foo.rs"]; 4];
        ticks.extend(vec![vec![]; 16]);
        let h = cross_pane_history(ticks);
        let sig = detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.kind, AnomalyKind::CrossPaneEditCluster);
    }

    fn cost_history(samples: Vec<Option<f64>>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for s in samples.into_iter().rev() { h.cost_usd_samples.push_front(s); }
        h
    }

    #[test]
    fn detect_cost_slope_pure_positive_high_confidence() {
        let mut samples = vec![Some(100.0)];
        for i in (0..19).rev() { samples.push(Some((i as f64) * (100.0 / 19.0))); }
        let h = cost_history(samples);
        let sig = detect_cost_slope(&h, 20, 20.0, 1_700_000_000).unwrap();
        assert_eq!(sig.kind, AnomalyKind::CostSlope);
        assert_eq!(sig.confidence, AnomalyConfidence::High);
    }

    fn make_signal(kind: AnomalyKind, confidence: AnomalyConfidence) -> AnomalySignal {
        let severity = severity_for(confidence);
        let (metric_name, before, after) = match kind {
            AnomalyKind::IdentityChurn => ("provider_path", "claude:/r".to_string(), "codex:/r".to_string()),
            AnomalyKind::ErrorBurst => ("error_rate", "0.10".to_string(), "0.80".to_string()),
            AnomalyKind::CacheDiscontinuity => ("cache_hit_ratio", "0.85".to_string(), "0.40".to_string()),
            AnomalyKind::CrossPaneEditCluster => ("concurrent_edit_path", "/r/foo.rs".to_string(), "6".to_string()),
            AnomalyKind::CostSlope => ("cost_slope_usd_per_hour", "0.10".to_string(), "25.40".to_string()),
        };
        AnomalySignal {
            kind, confidence, severity,
            evidence: vec![AnomalyEvidence { metric_name, before, after, sample_count: 6, source_kind: SourceKind::Estimated }],
            window_polls: 20, detected_at: 1_700_000_000,
        }
    }

    #[test]
    fn promote_matrix_5_kind_distinct_actions() {
        let signals: Vec<AnomalySignal> = vec![
            AnomalyKind::IdentityChurn,
            AnomalyKind::ErrorBurst,
            AnomalyKind::CacheDiscontinuity,
            AnomalyKind::CrossPaneEditCluster,
            AnomalyKind::CostSlope,
        ]
        .into_iter()
        .map(|k| make_signal(k, AnomalyConfidence::High))
        .collect();
        let promoted = promote_anomalies_to_recommendations(&signals, &fixture_gates());
        assert_eq!(promoted.len(), 5);
    }
}
