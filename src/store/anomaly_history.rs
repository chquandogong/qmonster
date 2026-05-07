//! Phase 7 v3 (c) (v1.47.0): per-tick snapshot of the inputs the
//! detectors consumed. Mirrors `AnomalyHistory` deque heads, written
//! once per pane per tick. Used for restart-survival replay.
//!
//! Lives in `src/store/` rather than alongside `AnomalyHistory` itself
//! because this type is the persistence boundary — detectors continue
//! to consume from in-memory `AnomalyHistory` deques unchanged.

use crate::policy::rules::anomaly::AnomalyHistory;

#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyHistorySnapshot {
    /// `(provider, current_path)` snapshot, or `None` if the identity
    /// signal was absent that tick.
    pub identity: Option<(String, String)>,
    /// `signals.error_hint` per tick. `None` if the signal was absent.
    pub error_hint: Option<bool>,
    /// `cache_hit_ratio` per tick. `None` if the signal was absent.
    pub cache_hit_ratio: Option<f32>,
    /// Did F-7b cache-drift fire this tick?
    pub cache_drift_fire: bool,
    /// Paths of `ConcurrentFileEdit` findings this tick (possibly empty).
    pub cross_pane_edit_paths: Vec<String>,
    /// Cumulative `signals.cost_usd`. `None` if absent.
    pub cost_usd: Option<f64>,
    /// Cumulative `signals.input_tokens`. `None` if absent.
    pub input_tokens: Option<u64>,
    /// Cumulative `signals.output_tokens`. `None` if absent.
    pub output_tokens: Option<u64>,
    /// `signals.process_memory_mb` (RSS). `None` if absent.
    pub process_memory_mb: Option<f64>,
    /// `signals.agent_memory_bytes`. `None` if absent.
    pub agent_memory_bytes: Option<u64>,
    /// `signals.subagent_hint` per tick.
    pub subagent_hint: bool,
}

impl AnomalyHistorySnapshot {
    /// Build a snapshot from the values that just got pushed onto an
    /// `AnomalyHistory` for the current tick. Reads the front of each
    /// deque (most-recent-first).
    pub fn capture_tick(history: &AnomalyHistory) -> Self {
        Self {
            identity: history.identity_snapshots.front().cloned(),
            error_hint: history.error_hints.front().copied(),
            cache_hit_ratio: history.cache_hit_ratios.front().copied().flatten(),
            cache_drift_fire: history.cache_drift_fires.front().copied().unwrap_or(false),
            cross_pane_edit_paths: history
                .cross_pane_edit_paths
                .front()
                .cloned()
                .unwrap_or_default(),
            cost_usd: history.cost_usd_samples.front().copied().flatten(),
            input_tokens: history.input_token_samples.front().copied().flatten(),
            output_tokens: history.output_token_samples.front().copied().flatten(),
            process_memory_mb: history.process_memory_samples.front().copied().flatten(),
            agent_memory_bytes: history.agent_memory_samples.front().copied().flatten(),
            subagent_hint: history
                .subagent_hint_samples
                .front()
                .copied()
                .unwrap_or(false),
        }
    }
}

/// Push one snapshot's fields into the corresponding `AnomalyHistory` deques
/// (newest-to-front, matching the existing per-tick push order in
/// `event_loop`). Used by the lazy replay path.
pub fn push_snapshot_into_history(history: &mut AnomalyHistory, snap: &AnomalyHistorySnapshot) {
    if let Some(id) = &snap.identity {
        history.identity_snapshots.push_front(id.clone());
    }
    if let Some(eh) = snap.error_hint {
        history.error_hints.push_front(eh);
    }
    history.cache_hit_ratios.push_front(snap.cache_hit_ratio);
    history.cache_drift_fires.push_front(snap.cache_drift_fire);
    history
        .cross_pane_edit_paths
        .push_front(snap.cross_pane_edit_paths.clone());
    history.cost_usd_samples.push_front(snap.cost_usd);
    history.input_token_samples.push_front(snap.input_tokens);
    history.output_token_samples.push_front(snap.output_tokens);
    history
        .process_memory_samples
        .push_front(snap.process_memory_mb);
    history
        .agent_memory_samples
        .push_front(snap.agent_memory_bytes);
    history.subagent_hint_samples.push_front(snap.subagent_hint);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_then_replay_roundtrips_basic_fields() {
        let mut h = AnomalyHistory::default();
        h.identity_snapshots
            .push_front(("Codex".to_string(), "/r".to_string()));
        h.error_hints.push_front(true);
        h.cache_hit_ratios.push_front(Some(0.5));
        h.cache_drift_fires.push_front(true);
        h.cross_pane_edit_paths
            .push_front(vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
        h.cost_usd_samples.push_front(Some(1.25));
        h.input_token_samples.push_front(Some(1000));
        h.output_token_samples.push_front(Some(500));
        h.process_memory_samples.push_front(Some(150.0));
        h.agent_memory_samples.push_front(Some(2_000_000));
        h.subagent_hint_samples.push_front(true);

        let snap = AnomalyHistorySnapshot::capture_tick(&h);

        let mut replayed = AnomalyHistory::default();
        push_snapshot_into_history(&mut replayed, &snap);

        assert_eq!(
            replayed.identity_snapshots.front(),
            h.identity_snapshots.front()
        );
        assert_eq!(replayed.error_hints.front(), h.error_hints.front());
        assert_eq!(
            replayed.cache_hit_ratios.front(),
            h.cache_hit_ratios.front()
        );
        assert_eq!(
            replayed.cache_drift_fires.front(),
            h.cache_drift_fires.front()
        );
        assert_eq!(
            replayed.cross_pane_edit_paths.front(),
            h.cross_pane_edit_paths.front()
        );
        assert_eq!(
            replayed.cost_usd_samples.front(),
            h.cost_usd_samples.front()
        );
        assert_eq!(
            replayed.input_token_samples.front(),
            h.input_token_samples.front()
        );
        assert_eq!(
            replayed.output_token_samples.front(),
            h.output_token_samples.front()
        );
        assert_eq!(
            replayed.process_memory_samples.front(),
            h.process_memory_samples.front()
        );
        assert_eq!(
            replayed.agent_memory_samples.front(),
            h.agent_memory_samples.front()
        );
        assert_eq!(
            replayed.subagent_hint_samples.front(),
            h.subagent_hint_samples.front()
        );
    }

    #[test]
    fn capture_handles_empty_history() {
        let h = AnomalyHistory::default();
        let snap = AnomalyHistorySnapshot::capture_tick(&h);
        assert_eq!(snap.identity, None);
        assert_eq!(snap.error_hint, None);
        assert_eq!(snap.cache_hit_ratio, None);
        assert!(!snap.cache_drift_fire);
        assert!(snap.cross_pane_edit_paths.is_empty());
        assert_eq!(snap.cost_usd, None);
        assert_eq!(snap.input_tokens, None);
        assert_eq!(snap.output_tokens, None);
        assert_eq!(snap.process_memory_mb, None);
        assert_eq!(snap.agent_memory_bytes, None);
        assert!(!snap.subagent_hint);
    }

    #[test]
    fn replay_skips_absent_optional_fields() {
        let snap = AnomalyHistorySnapshot {
            identity: None,
            error_hint: None,
            cache_hit_ratio: None,
            cache_drift_fire: false,
            cross_pane_edit_paths: Vec::new(),
            cost_usd: None,
            input_tokens: None,
            output_tokens: None,
            process_memory_mb: None,
            agent_memory_bytes: None,
            subagent_hint: false,
        };
        let mut h = AnomalyHistory::default();
        push_snapshot_into_history(&mut h, &snap);
        assert!(
            h.identity_snapshots.is_empty(),
            "no identity push when None"
        );
        assert!(h.error_hints.is_empty(), "no error_hint push when None");
        // Non-Option fields always push:
        assert_eq!(h.cache_hit_ratios.front(), Some(&None));
        assert_eq!(h.cache_drift_fires.front(), Some(&false));
        assert_eq!(h.cross_pane_edit_paths.front(), Some(&Vec::<String>::new()));
        assert_eq!(h.subagent_hint_samples.front(), Some(&false));
    }
}
