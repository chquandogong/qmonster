//! Phase 7 v1 (v1.43.0): per-pane anomaly observation rules.
//! Pure detection over `AnomalyHistory`; the orchestrator
//! `eval_anomalies` (Task 9) applies edge-triggered dedup and
//! minimum-confidence filtering.
//!
//! v1 ships four kinds. v2 adds CostSlope, TokenSlope,
//! MemoryGrowth, SubagentSideEffect.

use std::collections::{HashMap, VecDeque};

use crate::domain::anomaly::{
    AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal, SUBAGENT_SIDE_EFFECT_ACTION,
};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::policy::gates::PolicyGates;

/// Phase 7 v2 (v1.44.0): map detector confidence to severity.
/// `High` confidence promotes to `Warning` so the
/// `promote_anomalies_to_recommendations` filter (gate
/// `severity >= Warning`) emits a `Recommendation` and triggers
/// `RequestedEffect::Notify`. Medium and Low stay at `Concern` —
/// observation only, no Recommendation, no Notify.
fn severity_for(confidence: AnomalyConfidence) -> Severity {
    match confidence {
        AnomalyConfidence::High => Severity::Warning,
        _ => Severity::Concern,
    }
}

/// Per-pane rolling history the detectors consume. The event loop
/// pushes one observation per tick (newest pushed to front, trim
/// to `window_polls`). Each field corresponds to one detector.
#[derive(Debug, Default, Clone)]
pub struct AnomalyHistory {
    /// Observation timestamp per tick, most-recent-first. Used by
    /// slope detectors to normalize sparse or delayed samples by real
    /// elapsed time instead of assuming the nominal poll interval.
    pub tick_unix_ms_samples: VecDeque<u64>,
    /// Legacy persisted observation timestamp per tick, most-recent-first.
    /// SQLite replay still stores seconds, so detectors fall back to this
    /// deque when millisecond ticks are unavailable.
    pub tick_unix_secs_samples: VecDeque<u64>,
    /// IdentityChurn input: (provider_label, current_path) snapshot
    /// per tick. Most-recent-first.
    pub identity_snapshots: VecDeque<(String, String)>,
    /// ErrorBurst input: `signals.error_hint` per tick. Most-recent-first.
    pub error_hints: VecDeque<bool>,
    /// Phase 7 v2 evidence enrichment: parallel to `error_hints`. Holds
    /// the matched pattern class label (`Some("rust_panic")`,
    /// `Some("error_prefix")`, …) per tick, or `None` when `error_hint`
    /// was `false`. The detector aggregates these to surface a
    /// `dominant_kind` evidence row. Session-only — replay from the
    /// SQLite snapshot table leaves this deque empty (the bool surface
    /// is enough to keep `detect_error_burst` correct; the kind row is
    /// just gracefully omitted in that case).
    pub error_hint_kinds: VecDeque<Option<&'static str>>,
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
    /// Phase 7 v2 (v1.45.0): CostSlope detector input. Cumulative
    /// cost_usd from `signals.cost_usd`, most-recent-first. `None`
    /// when the signal was absent that tick.
    pub cost_usd_samples: VecDeque<Option<f64>>,
    /// Phase 7 v2 (v1.45.0): TokenSlope detector primary input.
    /// Cumulative `signals.input_tokens`, most-recent-first.
    pub input_token_samples: VecDeque<Option<u64>>,
    /// Phase 7 v2 (v1.45.0): TokenSlope detector evidence-only secondary.
    /// Cumulative `signals.output_tokens`, most-recent-first.
    pub output_token_samples: VecDeque<Option<u64>>,
    /// Phase 7 v2 (v1.45.0): MemoryGrowth detector primary input.
    /// `signals.process_memory_mb` (RSS), most-recent-first.
    pub process_memory_samples: VecDeque<Option<f64>>,
    /// Phase 7 v2 (v1.45.0): MemoryGrowth detector evidence-only secondary.
    /// `signals.agent_memory_bytes`, most-recent-first.
    pub agent_memory_samples: VecDeque<Option<u64>>,
    /// Phase 7 v2 (v1.45.0): SubagentSideEffect detector input.
    /// `signals.subagent_hint` per tick, most-recent-first.
    pub subagent_hint_samples: VecDeque<bool>,
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
        while self.input_token_samples.len() > cap {
            self.input_token_samples.pop_back();
        }
        while self.output_token_samples.len() > cap {
            self.output_token_samples.pop_back();
        }
        while self.process_memory_samples.len() > cap {
            self.process_memory_samples.pop_back();
        }
        while self.agent_memory_samples.len() > cap {
            self.agent_memory_samples.pop_back();
        }
        while self.subagent_hint_samples.len() > cap {
            self.subagent_hint_samples.pop_back();
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
    let mut evidence = vec![AnomalyEvidence {
        metric_name: "error_rate",
        before: format!("{older_rate:.2}"),
        after: format!("{newer_rate:.2}"),
        sample_count: window_polls,
        source_kind: SourceKind::Estimated,
    }];
    // Phase 7 v2 evidence enrichment: when the parallel
    // `error_hint_kinds` deque has at least as many entries as the
    // bool window, surface the dominant pattern class so the operator
    // sees *what kind* of error spiked. Replay-restored histories
    // (whose `error_hint_kinds` deque is empty by construction —
    // session-only field) silently skip this row.
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

/// Phase 7 v2 evidence enrichment helper: scan the most-recent
/// `window_polls` of `kinds` and return the most-frequent non-`None`
/// label with its count. Returns `None` when no kind appeared in the
/// window (e.g. detector fires on a stale-restored history with an
/// empty `error_hint_kinds` deque).
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
        // Stable tie-break: highest count first; on ties prefer the
        // lexicographically smaller label so test assertions stay
        // deterministic across HashMap iteration order.
        .max_by(|(la, ca), (lb, cb)| ca.cmp(cb).then_with(|| lb.cmp(la)))
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
        severity: severity_for(confidence),
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
///
/// v2.2.0 (P1-1): each detector also gates on
/// `kind.supports_provider(provider)`. Detectors whose required input
/// signal is not populated for the pane's provider are skipped before
/// the detector runs (avoiding wasted work) and never contribute to the
/// dedup map. This closes the "MemoryGrowth on Claude is silently
/// dead" honesty gap surfaced by the v2.2.0 critical evaluation.
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
        (
            AnomalyKind::TokenSlope,
            AnomalyKind::TokenSlope
                .supports_provider(provider)
                .then(|| {
                    detect_token_slope(
                        history,
                        gates.anomaly_window_polls,
                        gates.anomaly_token_slope_input_per_poll,
                        now_unix_seconds,
                    )
                })
                .flatten(),
        ),
        (
            AnomalyKind::MemoryGrowth,
            AnomalyKind::MemoryGrowth
                .supports_provider(provider)
                .then(|| {
                    detect_memory_growth(
                        history,
                        gates.anomaly_window_polls,
                        gates.anomaly_memory_growth_mb,
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

    // Phase 7 v2 (v1.45.0): SubagentSideEffect runs LAST against the
    // post-dedup `out` slice — it correlates subagent_hint observation
    // with other anomalies in the same window. Apply the same edge-
    // triggered dedup contract as the other kinds: emit only on
    // Some-after-None edge, suppress on continuous Some, rearm on None.
    let raw_side_effect =
        detect_subagent_side_effect(history, &out, gates.anomaly_window_polls, now_unix_seconds);
    let key = (pane_id.to_string(), AnomalyKind::SubagentSideEffect);
    let prev = dedup.get(&key).copied().flatten();
    match (raw_side_effect, prev) {
        (Some(sig), None) => {
            if sig.confidence >= gates.anomaly_min_confidence {
                dedup.insert(key, Some(now_unix_seconds));
                out.push(sig);
            }
        }
        (Some(_sig), Some(_)) => {
            // Suppress while still active.
        }
        (None, Some(_)) => {
            // Rearm.
            dedup.insert(key, None);
        }
        (None, None) => {
            // No-op.
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

/// CostSlope: cost_usd cumulative delta over `window_polls` ticks,
/// normalized to USD per hour. Fires when slope ≥ `threshold_usd_per_hour`.
/// Uses real sample timestamps when available; otherwise falls back to
/// the default `window_polls × 2s` nominal polling interval.
/// Confidence: `High` at ≥ 1.5× threshold, otherwise `Medium`.
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

/// TokenSlope: input_tokens cumulative delta over `window_polls`,
/// normalized to tokens-per-poll. Real elapsed time is converted back
/// into default-poll-equivalent units when timestamps are available.
/// Fires when `slope_per_poll >= threshold_input_per_poll`.
/// Confidence: `High` at ≥ 1.5× threshold, otherwise `Medium`.
pub fn detect_token_slope(
    history: &AnomalyHistory,
    window_polls: usize,
    threshold_input_per_poll: u64,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    if history.input_token_samples.len() < window_polls {
        return None;
    }
    let window: Vec<u64> = history
        .input_token_samples
        .iter()
        .take(window_polls)
        .filter_map(|s| *s)
        .collect();
    if window.len() < 2 {
        return None;
    }
    let newest = window.first().copied().unwrap();
    let oldest = window.last().copied().unwrap();
    if newest < oldest {
        return None;
    }
    let window_secs = elapsed_secs_for_window(history, window_polls)
        .unwrap_or_else(|| nominal_elapsed_secs(window_polls));
    let poll_equivalent = (window_secs / NOMINAL_POLL_INTERVAL_SECS).max(1.0);
    let slope_per_poll = (newest - oldest) as f64 / poll_equivalent;
    if slope_per_poll < threshold_input_per_poll as f64 {
        return None;
    }
    let confidence = if slope_per_poll >= threshold_input_per_poll as f64 * 1.5 {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };
    Some(AnomalySignal {
        kind: AnomalyKind::TokenSlope,
        confidence,
        severity: severity_for(confidence),
        evidence: vec![
            AnomalyEvidence {
                metric_name: "input_tokens_per_poll",
                before: format!("{oldest}"),
                after: format!("{newest}"),
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

/// MemoryGrowth: simple delta of `process_memory_mb` over the window
/// in MB. Fires when growth ≥ `threshold_mb`. Confidence: `High` at
/// ≥ 1.5× threshold, otherwise `Medium`.
pub fn detect_memory_growth(
    history: &AnomalyHistory,
    window_polls: usize,
    threshold_mb: f64,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    if history.process_memory_samples.len() < window_polls {
        return None;
    }
    let window: Vec<f64> = history
        .process_memory_samples
        .iter()
        .take(window_polls)
        .filter_map(|s| *s)
        .collect();
    if window.len() < 2 {
        return None;
    }
    let newest = window.first().copied().unwrap();
    let oldest = window.last().copied().unwrap();
    let growth_mb = newest - oldest;
    if growth_mb < threshold_mb {
        return None;
    }
    let confidence = if growth_mb >= threshold_mb * 1.5 {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };
    Some(AnomalySignal {
        kind: AnomalyKind::MemoryGrowth,
        confidence,
        severity: severity_for(confidence),
        evidence: vec![AnomalyEvidence {
            metric_name: "process_memory_mb",
            before: format!("{oldest:.0}"),
            after: format!("{newest:.0}"),
            sample_count: window.len(),
            source_kind: SourceKind::Estimated,
        }],
        window_polls,
        detected_at: now_unix_seconds,
    })
}

/// SubagentSideEffect: correlation annotator. Fires when
/// `subagent_hint = true` is observed at least once in the window
/// AND `other_anomalies` is non-empty. Confidence is always Medium
/// (binary correlation, not numeric). Reason lists the co-occurring
/// kinds. Per Phase D D3-C honesty note: this is correlation, not
/// attribution — providers do not expose per-subagent counters.
pub fn detect_subagent_side_effect(
    history: &AnomalyHistory,
    other_anomalies: &[AnomalySignal],
    window_polls: usize,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    if other_anomalies.is_empty() {
        return None;
    }
    let subagent_count = history
        .subagent_hint_samples
        .iter()
        .take(window_polls)
        .filter(|b| **b)
        .count();
    if subagent_count == 0 {
        return None;
    }

    let mut cooccurring_kinds: Vec<&str> = other_anomalies.iter().map(|a| a.kind.label()).collect();
    cooccurring_kinds.sort();
    cooccurring_kinds.dedup();
    let kinds_csv = cooccurring_kinds.join(", ");

    Some(AnomalySignal {
        kind: AnomalyKind::SubagentSideEffect,
        confidence: AnomalyConfidence::Medium,
        severity: severity_for(AnomalyConfidence::Medium),
        evidence: vec![
            AnomalyEvidence {
                metric_name: "⚠ correlation only — providers do not expose per-subagent token attribution",
                before: "no_subagent".to_string(),
                after: "subagent_active".to_string(),
                sample_count: subagent_count,
                source_kind: SourceKind::Estimated,
            },
            AnomalyEvidence {
                metric_name: "cooccurring_kinds",
                before: String::new(),
                after: kinds_csv,
                sample_count: other_anomalies.len(),
                source_kind: SourceKind::Estimated,
            },
        ],
        window_polls,
        detected_at: now_unix_seconds,
    })
}

pub fn promote_anomalies_to_recommendations(
    signals: &[AnomalySignal],
    gates: &crate::policy::gates::PolicyGates,
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
            AnomalyKind::TokenSlope => {
                let (oldest, newest) = evidence
                    .map(|e| (e.before.clone(), e.after.clone()))
                    .unwrap_or((String::new(), String::new()));
                (
                    "anomaly: token slope detected",
                    format!(
                        "input tokens climbing from {oldest} to {newest} over the {} polls window; confidence {}",
                        sig.window_polls,
                        sig.confidence.label()
                    ),
                    "check for repeated long inputs; consider /compact or context profile switch"
                        .to_string(),
                )
            }
            AnomalyKind::MemoryGrowth => {
                let (oldest, newest) = evidence
                    .map(|e| (e.before.clone(), e.after.clone()))
                    .unwrap_or((String::new(), String::new()));
                (
                    "anomaly: memory growth detected",
                    format!(
                        "process memory grew from {oldest} MB to {newest} MB over the {} polls window; confidence {}",
                        sig.window_polls,
                        sig.confidence.label()
                    ),
                    "check the provider for memory leak; consider restart if growth is sustained"
                        .to_string(),
                )
            }
            AnomalyKind::SubagentSideEffect => {
                let count = evidence.map(|e| e.sample_count).unwrap_or(0);
                (
                    SUBAGENT_SIDE_EFFECT_ACTION,
                    format!(
                        "⚠ correlation only — providers do not expose per-subagent token attribution. \
                         subagent_hint observed {count} times in {} polls while other anomalies fired in the same window",
                        sig.window_polls
                    ),
                    "the subagent activity is *correlated* with the co-occurring anomaly, not proven to be the source. inspect the subagent's recent output before attributing cause".to_string(),
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
            anomaly_token_slope_input_per_poll: 20_000,
            anomaly_memory_growth_mb: 1024.0,
            anomaly_promote_identity_churn: AnomalyConfidence::High,
            anomaly_promote_error_burst: AnomalyConfidence::High,
            anomaly_promote_cache_discontinuity: AnomalyConfidence::High,
            anomaly_promote_cross_pane_edit_cluster: AnomalyConfidence::High,
            anomaly_promote_cost_slope: AnomalyConfidence::High,
            anomaly_promote_token_slope: AnomalyConfidence::High,
            anomaly_promote_memory_growth: AnomalyConfidence::High,
            anomaly_promote_subagent_side_effect: AnomalyConfidence::Medium,
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

        // Tick 1: detector returns Some, dedup empty → emit, dedup set
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

        // Tick 2: detector still Some, dedup occupied → suppress
        let r2 = eval_anomalies(
            "%1",
            crate::domain::identity::Provider::Claude,
            &history,
            &gates,
            &mut dedup,
            1_700_000_005,
        );
        assert!(r2.is_empty());

        // Tick 3: history clears (only one snapshot left) → detector None, rearm
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

        // Tick 4: detector Some again, dedup rearmed → emit
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
        // Exactly threshold flips → Medium (filtered out at min_confidence=High)
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
    fn detect_error_burst_evidence_carries_dominant_error_kind() {
        // 12 errors in 20 ticks; 8× rust_panic + 4× error_prefix → dominant is rust_panic.
        let mut h = AnomalyHistory::default();
        let mut newest_first = Vec::new();
        for _ in 0..8 {
            newest_first.push((true, Some("rust_panic")));
        }
        for _ in 0..4 {
            newest_first.push((true, Some("error_prefix")));
        }
        for _ in 0..8 {
            newest_first.push((false, None));
        }
        for (b, k) in newest_first.into_iter().rev() {
            h.error_hints.push_front(b);
            h.error_hint_kinds.push_front(k);
        }
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000)
            .expect("60% rate should fire above 0.5 threshold");
        let dominant = sig
            .evidence
            .iter()
            .find(|e| e.metric_name == "dominant_kind")
            .expect("dominant_kind evidence row must accompany the rate row");
        assert_eq!(dominant.before, "rust_panic");
        assert_eq!(
            dominant.after, "8/12",
            "after must read as <dominant_count>/<total_error_count>",
        );
    }

    #[test]
    fn detect_error_burst_evidence_omits_dominant_kind_when_replay_only() {
        // Replay-restored history fills error_hints (bool persistence)
        // but leaves error_hint_kinds empty (session-only field). The
        // detector must still fire on the bool window AND must not
        // emit a dominant_kind row in this case.
        let mut h = AnomalyHistory::default();
        for _ in 0..12 {
            h.error_hints.push_front(true);
        }
        for _ in 0..8 {
            h.error_hints.push_front(false);
        }
        // intentionally leave h.error_hint_kinds empty
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000).expect("rate row must still fire");
        assert!(
            sig.evidence
                .iter()
                .all(|e| e.metric_name != "dominant_kind"),
            "dominant_kind row must be silently omitted when error_hint_kinds is empty",
        );
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

    #[test]
    fn severity_for_high_returns_warning() {
        assert_eq!(severity_for(AnomalyConfidence::High), Severity::Warning);
    }

    #[test]
    fn severity_for_medium_low_returns_concern() {
        assert_eq!(severity_for(AnomalyConfidence::Medium), Severity::Concern);
        assert_eq!(severity_for(AnomalyConfidence::Low), Severity::Concern);
    }

    #[test]
    fn detect_identity_churn_emits_warning_when_high_confidence() {
        // 5 flips when threshold is 3 → 5 ≥ 1.5 * 3 = 4.5 → High → Warning
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
        assert_eq!(sig.severity, Severity::Warning);
    }

    #[test]
    fn detect_identity_churn_emits_concern_when_medium_confidence() {
        // exactly 3 flips → Medium (3 < 1.5*3=4.5)
        let h = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let sig = detect_identity_churn(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::Medium);
        assert_eq!(sig.severity, Severity::Concern);
    }

    #[test]
    fn detect_error_burst_emits_warning_when_high_confidence() {
        // 16 errors / 20 = 0.80 ≥ 1.5 * 0.5 = 0.75 → High
        let mut bools = vec![true; 16];
        bools.extend(vec![false; 4]);
        let h = errors_history(bools);
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
    }

    #[test]
    fn detect_error_burst_emits_concern_when_medium_confidence() {
        // 12 errors / 20 = 0.60 ≥ 0.5 but < 0.75 → Medium
        let mut bools = vec![true; 12];
        bools.extend(vec![false; 8]);
        let h = errors_history(bools);
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::Medium);
        assert_eq!(sig.severity, Severity::Concern);
    }

    #[test]
    fn detect_cache_discontinuity_emits_warning_when_high_confidence() {
        // 0.45-pp drop ≥ 1.5 * 0.30 = 0.45 → High
        let mut ratios = vec![Some(0.40); 10];
        ratios.extend(vec![Some(0.85); 10]);
        let h = cache_history(ratios, vec![false; 20]);
        let sig = detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
    }

    #[test]
    fn detect_cache_discontinuity_emits_concern_when_medium_confidence() {
        // 0.34-pp drop ≥ 0.30 but < 0.45 → Medium
        let mut ratios = vec![Some(0.51); 10];
        ratios.extend(vec![Some(0.85); 10]);
        let h = cache_history(ratios, vec![false; 20]);
        let sig = detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::Medium);
        assert_eq!(sig.severity, Severity::Concern);
    }

    #[test]
    fn detect_cross_pane_edit_cluster_emits_warning_when_high_confidence() {
        // 6 findings with min=3 → 6 ≥ 2*3 → High
        let mut ticks: Vec<Vec<&str>> = vec![vec!["/r/foo.rs"]; 6];
        ticks.extend(vec![vec![]; 14]);
        let h = cross_pane_history(ticks);
        let sig = detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
    }

    #[test]
    fn detect_cross_pane_edit_cluster_emits_concern_when_medium_confidence() {
        // 4 findings with min=3 → 4 ≥ 3 but < 6 → Medium
        let mut ticks: Vec<Vec<&str>> = vec![vec!["/r/foo.rs"]; 4];
        ticks.extend(vec![vec![]; 16]);
        let h = cross_pane_history(ticks);
        let sig = detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::Medium);
        assert_eq!(sig.severity, Severity::Concern);
    }

    fn make_signal(kind: AnomalyKind, confidence: AnomalyConfidence) -> AnomalySignal {
        let severity = severity_for(confidence);
        let (metric_name, before, after) = match kind {
            AnomalyKind::IdentityChurn => (
                "provider_path",
                "claude:/r".to_string(),
                "codex:/r".to_string(),
            ),
            AnomalyKind::ErrorBurst => ("error_rate", "0.10".to_string(), "0.80".to_string()),
            AnomalyKind::CacheDiscontinuity => {
                ("cache_hit_ratio", "0.85".to_string(), "0.40".to_string())
            }
            AnomalyKind::CrossPaneEditCluster => (
                "concurrent_edit_path",
                "/r/foo.rs".to_string(),
                "6".to_string(),
            ),
            AnomalyKind::CostSlope => unreachable!("Phase 7 v2 detectors land in Tasks 5-9"),
            AnomalyKind::TokenSlope => unreachable!("Phase 7 v2 detectors land in Tasks 5-9"),
            AnomalyKind::MemoryGrowth => unreachable!("Phase 7 v2 detectors land in Tasks 5-9"),
            AnomalyKind::SubagentSideEffect => {
                unreachable!("Phase 7 v2 detectors land in Tasks 5-9")
            }
        };
        AnomalySignal {
            kind,
            confidence,
            severity,
            evidence: vec![AnomalyEvidence {
                metric_name,
                before,
                after,
                sample_count: 6,
                source_kind: SourceKind::Estimated,
            }],
            window_polls: 20,
            detected_at: 1_700_000_000,
        }
    }

    #[test]
    fn promote_returns_empty_for_concern_only() {
        let signals = vec![
            make_signal(AnomalyKind::IdentityChurn, AnomalyConfidence::Medium),
            make_signal(AnomalyKind::ErrorBurst, AnomalyConfidence::Low),
        ];
        let promoted = promote_anomalies_to_recommendations(&signals, &fixture_gates());
        assert!(promoted.is_empty());
    }

    #[test]
    fn promote_emits_recommendation_for_warning_signal() {
        let signals = vec![make_signal(
            AnomalyKind::IdentityChurn,
            AnomalyConfidence::High,
        )];
        let promoted = promote_anomalies_to_recommendations(&signals, &fixture_gates());
        assert_eq!(promoted.len(), 1);
        assert_eq!(promoted[0].action, "anomaly: identity churn detected");
        assert_eq!(promoted[0].severity, Severity::Warning);
    }

    #[test]
    fn promote_maps_each_kind_to_distinct_action() {
        let signals = vec![
            make_signal(AnomalyKind::IdentityChurn, AnomalyConfidence::High),
            make_signal(AnomalyKind::ErrorBurst, AnomalyConfidence::High),
            make_signal(AnomalyKind::CacheDiscontinuity, AnomalyConfidence::High),
            make_signal(AnomalyKind::CrossPaneEditCluster, AnomalyConfidence::High),
        ];
        let promoted = promote_anomalies_to_recommendations(&signals, &fixture_gates());
        assert_eq!(promoted.len(), 4);
        let actions: Vec<&str> = promoted.iter().map(|r| r.action).collect();
        assert!(actions.contains(&"anomaly: identity churn detected"));
        assert!(actions.contains(&"anomaly: error burst detected"));
        assert!(actions.contains(&"anomaly: cache discontinuity detected"));
        assert!(actions.contains(&"anomaly: cross-pane edit cluster detected"));
        // All four must be distinct
        let mut sorted = actions.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 4, "all 4 actions must be distinct");
    }

    #[test]
    fn promote_recommendation_severity_matches_signal() {
        let signals = vec![make_signal(
            AnomalyKind::ErrorBurst,
            AnomalyConfidence::High,
        )];
        let promoted = promote_anomalies_to_recommendations(&signals, &fixture_gates());
        assert_eq!(promoted[0].severity, signals[0].severity);
    }

    #[test]
    fn promote_recommendation_source_kind_matches_evidence() {
        let signals = vec![make_signal(
            AnomalyKind::CacheDiscontinuity,
            AnomalyConfidence::High,
        )];
        let promoted = promote_anomalies_to_recommendations(&signals, &fixture_gates());
        assert_eq!(promoted[0].source_kind, signals[0].evidence[0].source_kind);
    }

    #[test]
    fn anomaly_history_v2_fields_default_empty() {
        let h = AnomalyHistory::default();
        assert!(h.cost_usd_samples.is_empty());
        assert!(h.input_token_samples.is_empty());
        assert!(h.output_token_samples.is_empty());
        assert!(h.process_memory_samples.is_empty());
        assert!(h.agent_memory_samples.is_empty());
        assert!(h.subagent_hint_samples.is_empty());
    }

    #[test]
    fn anomaly_history_trim_caps_v2_deques() {
        let mut h = AnomalyHistory::default();
        for i in 0..30 {
            h.cost_usd_samples.push_front(Some(i as f64 * 0.5));
            h.input_token_samples.push_front(Some(i as u64 * 1000));
            h.output_token_samples.push_front(Some(i as u64 * 800));
            h.process_memory_samples.push_front(Some(i as f64 * 10.0));
            h.agent_memory_samples.push_front(Some(i as u64 * 1024));
            h.subagent_hint_samples.push_front(i % 4 == 0);
        }
        h.trim(20);
        assert_eq!(h.cost_usd_samples.len(), 20);
        assert_eq!(h.input_token_samples.len(), 20);
        assert_eq!(h.output_token_samples.len(), 20);
        assert_eq!(h.process_memory_samples.len(), 20);
        assert_eq!(h.agent_memory_samples.len(), 20);
        assert_eq!(h.subagent_hint_samples.len(), 20);
    }

    fn cost_history(samples: Vec<Option<f64>>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        // Pass newest-first; reverse-iter then push_front to preserve order.
        for s in samples.into_iter().rev() {
            h.cost_usd_samples.push_front(s);
        }
        h
    }

    fn cost_history_with_ticks(samples: Vec<Option<f64>>, ticks: Vec<u64>) -> AnomalyHistory {
        let mut h = cost_history(samples);
        for tick in ticks.into_iter().rev() {
            h.tick_unix_secs_samples.push_front(tick);
        }
        h
    }

    fn cost_history_with_ms_ticks(samples: Vec<Option<f64>>, ticks: Vec<u64>) -> AnomalyHistory {
        let mut h = cost_history(samples);
        for tick in ticks.into_iter().rev() {
            h.tick_unix_ms_samples.push_front(tick);
        }
        h
    }

    #[test]
    fn detect_cost_slope_pure_positive_high_confidence() {
        // window=20 polls × 5s = 100s. cost climbs 0 → 100 → slope = 100/100 * 3600 = 3600 USD/hour.
        // 3600 ≥ 1.5 × 20.0 = 30.0 → High → Warning.
        let mut samples = vec![Some(100.0)];
        for i in (0..19).rev() {
            samples.push(Some((i as f64) * (100.0 / 19.0)));
        }
        let h = cost_history(samples);
        let sig = detect_cost_slope(&h, 20, 20.0, 1_700_000_000).unwrap();
        assert_eq!(sig.kind, AnomalyKind::CostSlope);
        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
        assert_eq!(sig.evidence[0].metric_name, "cost_slope_usd_per_hour");
    }

    #[test]
    fn detect_cost_slope_pure_positive_medium_confidence() {
        // 7-cent delta over the default 40s fallback = 6.3 USD/hour
        // → ≥ 5.0 threshold but < 7.5 → Medium.
        let mut samples = vec![Some(0.07)];
        for i in (0..19).rev() {
            samples.push(Some((i as f64) * (0.07 / 19.0)));
        }
        let h = cost_history(samples);
        let sig = detect_cost_slope(&h, 20, 5.0, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::Medium);
        assert_eq!(sig.severity, Severity::Concern);
    }

    #[test]
    fn detect_cost_slope_below_threshold_returns_none() {
        // 10-cent delta over 100s = 3.6 USD/hour → < 20.0 threshold → None.
        let mut samples = vec![Some(0.10)];
        for i in (0..19).rev() {
            samples.push(Some((i as f64) * (0.10 / 19.0)));
        }
        let h = cost_history(samples);
        assert!(detect_cost_slope(&h, 20, 20.0, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_cost_slope_fallback_uses_default_two_second_poll_interval() {
        let mut samples = vec![Some(0.23)];
        for i in (0..19).rev() {
            samples.push(Some((i as f64) * (0.23 / 19.0)));
        }
        let h = cost_history(samples);

        assert!(detect_cost_slope(&h, 20, 20.0, 1_700_000_000).is_some());
    }

    #[test]
    fn detect_cost_slope_uses_actual_elapsed_seconds_when_available() {
        let mut samples = vec![Some(1.0)];
        for i in (0..19).rev() {
            samples.push(Some((i as f64) * (1.0 / 19.0)));
        }
        let ticks: Vec<u64> = (0..20).map(|i| 1_700_000_000 - i * 60).collect();
        let h = cost_history_with_ticks(samples, ticks);

        assert!(detect_cost_slope(&h, 20, 20.0, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_cost_slope_prefers_millisecond_ticks_when_available() {
        let mut samples = vec![Some(1.0)];
        for i in (0..19).rev() {
            samples.push(Some((i as f64) * (1.0 / 19.0)));
        }
        let ticks: Vec<u64> = (0..20).map(|i| 1_700_000_000_000 - i * 60_000).collect();
        let h = cost_history_with_ms_ticks(samples, ticks);

        assert!(detect_cost_slope(&h, 20, 20.0, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_cost_slope_reports_coverage_and_elapsed_seconds() {
        let mut samples = vec![Some(1.0)];
        for i in (0..19).rev() {
            samples.push(Some((i as f64) * (1.0 / 19.0)));
        }
        let ticks: Vec<u64> = (0..20).map(|i| 1_700_000_000 - i * 5).collect();
        let h = cost_history_with_ticks(samples, ticks);

        let sig = detect_cost_slope(&h, 20, 20.0, 1_700_000_000).unwrap();
        let coverage = sig
            .evidence
            .iter()
            .find(|e| e.metric_name == "sample_coverage")
            .unwrap();
        assert_eq!(coverage.before, "20/20");
        assert_eq!(coverage.after, "elapsed_secs=95");
    }

    #[test]
    fn detect_cost_slope_insufficient_samples_returns_none() {
        let h = cost_history(vec![Some(50.0), Some(0.0)]); // only 2, window 20
        assert!(detect_cost_slope(&h, 20, 20.0, 1_700_000_000).is_none());
    }

    fn token_history(input: Vec<Option<u64>>, output: Vec<Option<u64>>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for s in input.into_iter().rev() {
            h.input_token_samples.push_front(s);
        }
        for s in output.into_iter().rev() {
            h.output_token_samples.push_front(s);
        }
        h
    }

    fn token_history_with_ticks(
        input: Vec<Option<u64>>,
        output: Vec<Option<u64>>,
        ticks: Vec<u64>,
    ) -> AnomalyHistory {
        let mut h = token_history(input, output);
        for tick in ticks.into_iter().rev() {
            h.tick_unix_secs_samples.push_front(tick);
        }
        h
    }

    #[test]
    fn detect_token_slope_pure_positive_high_confidence() {
        let mut input = vec![Some(800_000)];
        input.extend(vec![Some(0); 19]);
        let output = vec![None; 20];
        let h = token_history(input, output);

        let sig = detect_token_slope(&h, 20, 20_000, 1_700_000_000).unwrap();

        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
        assert_eq!(sig.kind, AnomalyKind::TokenSlope);
    }

    #[test]
    fn detect_token_slope_reports_coverage_and_elapsed_seconds() {
        let mut input = vec![Some(1_000_000)];
        input.extend(vec![Some(0); 19]);
        let output = vec![None; 20];
        let ticks: Vec<u64> = (0..20).map(|i| 1_700_000_000 - i * 5).collect();
        let h = token_history_with_ticks(input, output, ticks);

        let sig = detect_token_slope(&h, 20, 20_000, 1_700_000_000).unwrap();
        let coverage = sig
            .evidence
            .iter()
            .find(|e| e.metric_name == "sample_coverage")
            .unwrap();

        assert_eq!(coverage.before, "20/20");
        assert_eq!(coverage.after, "elapsed_secs=95");
    }

    #[test]
    fn detect_memory_growth_pure_positive_high_confidence() {
        let mut h = AnomalyHistory::default();
        h.process_memory_samples.push_front(Some(1800.0));
        for _ in 0..19 {
            h.process_memory_samples.push_back(Some(100.0));
        }

        let sig = detect_memory_growth(&h, 20, 1024.0, 1_700_000_000).unwrap();

        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
        assert_eq!(sig.kind, AnomalyKind::MemoryGrowth);
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
        .map(|k| AnomalySignal {
            kind: k,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            evidence: vec![AnomalyEvidence {
                metric_name: "test",
                before: "0".to_string(),
                after: "100".to_string(),
                sample_count: 20,
                source_kind: SourceKind::Estimated,
            }],
            window_polls: 20,
            detected_at: 1_700_000_000,
        })
        .collect();
        let promoted = promote_anomalies_to_recommendations(&signals, &fixture_gates());
        assert_eq!(promoted.len(), 5);
        let mut actions: Vec<&str> = promoted.iter().map(|r| r.action).collect();
        actions.sort();
        actions.dedup();
        assert_eq!(
            actions.len(),
            5,
            "all 5 actions must be distinct: {actions:?}"
        );
    }

    #[test]
    fn promote_gate_blocks_below_per_kind_threshold() {
        use crate::domain::anomaly::{
            AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
        };
        use crate::domain::origin::SourceKind;
        let gates = fixture_gates(); // defaults: cost_slope = High
        let signal = AnomalySignal {
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::Medium,
            severity: severity_for(AnomalyConfidence::Medium),
            window_polls: 20,
            evidence: vec![AnomalyEvidence {
                metric_name: "cost_usd",
                before: "0.10".to_string(),
                after: "25.40".to_string(),
                sample_count: 20,
                source_kind: SourceKind::Estimated,
            }],
            detected_at: 1_700_000_000,
        };
        let recs = promote_anomalies_to_recommendations(&[signal], &gates);
        assert!(
            recs.is_empty(),
            "Medium CostSlope must NOT promote at default High threshold"
        );
    }

    #[test]
    fn promote_gate_admits_at_per_kind_threshold() {
        use crate::domain::anomaly::{
            AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
        };
        use crate::domain::origin::SourceKind;
        let gates = PolicyGates {
            anomaly_promote_cost_slope: AnomalyConfidence::Medium,
            ..fixture_gates()
        };
        let signal = AnomalySignal {
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::Medium,
            severity: severity_for(AnomalyConfidence::Medium),
            window_polls: 20,
            evidence: vec![AnomalyEvidence {
                metric_name: "cost_usd",
                before: "0.10".to_string(),
                after: "25.40".to_string(),
                sample_count: 20,
                source_kind: SourceKind::Estimated,
            }],
            detected_at: 1_700_000_000,
        };
        let recs = promote_anomalies_to_recommendations(&[signal], &gates);
        assert_eq!(
            recs.len(),
            1,
            "Medium CostSlope must promote at Medium threshold"
        );
    }

    #[test]
    fn promote_gate_high_threshold_admits_high() {
        use crate::domain::anomaly::{
            AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
        };
        use crate::domain::origin::SourceKind;
        let gates = fixture_gates(); // default cost_slope = High
        let signal = AnomalySignal {
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: severity_for(AnomalyConfidence::High),
            window_polls: 20,
            evidence: vec![AnomalyEvidence {
                metric_name: "cost_usd",
                before: "0.10".to_string(),
                after: "25.40".to_string(),
                sample_count: 20,
                source_kind: SourceKind::Estimated,
            }],
            detected_at: 1_700_000_000,
        };
        let recs = promote_anomalies_to_recommendations(&[signal], &gates);
        assert_eq!(
            recs.len(),
            1,
            "High CostSlope must promote at default High threshold"
        );
    }

    #[test]
    fn promote_gate_low_threshold_admits_all_levels() {
        use crate::domain::anomaly::{
            AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
        };
        use crate::domain::origin::SourceKind;
        let gates = PolicyGates {
            anomaly_promote_cost_slope: AnomalyConfidence::Low,
            ..fixture_gates()
        };
        let make = |conf: AnomalyConfidence| AnomalySignal {
            kind: AnomalyKind::CostSlope,
            confidence: conf,
            severity: severity_for(conf),
            window_polls: 20,
            evidence: vec![AnomalyEvidence {
                metric_name: "cost_usd",
                before: "0.10".to_string(),
                after: "25.40".to_string(),
                sample_count: 20,
                source_kind: SourceKind::Estimated,
            }],
            detected_at: 1_700_000_000,
        };
        let signals = vec![
            make(AnomalyConfidence::Low),
            make(AnomalyConfidence::Medium),
            make(AnomalyConfidence::High),
        ];
        let recs = promote_anomalies_to_recommendations(&signals, &gates);
        assert_eq!(
            recs.len(),
            3,
            "Low threshold admits all three confidence levels"
        );
    }
}
