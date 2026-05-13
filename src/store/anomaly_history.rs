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
///
/// # Push contract
///
/// `identity_snapshots` and `error_hints` are pushed **unconditionally** on
/// every tick in `event_loop`: the event loop always has a resolved identity
/// and a bool `signals.error_hint`, so these two deques always grow by one
/// per tick.  To preserve that invariant during replay we push unconditionally
/// here as well.  When the snapshot holds `None` (e.g. captured from an empty
/// history before the first event-loop tick) we push the same sentinel values
/// the live path would produce in that situation: `("", "")` for identity and
/// `false` for error_hint.  This keeps all deque lengths equal across live and
/// replayed paths.
pub fn push_snapshot_into_history_at(
    history: &mut AnomalyHistory,
    tick_unix_secs: u64,
    snap: &AnomalyHistorySnapshot,
) {
    history.tick_unix_secs_samples.push_front(tick_unix_secs);
    history
        .tick_unix_ms_samples
        .push_front(tick_unix_secs.saturating_mul(1000));
    // Unconditional: mirrors event_loop which always pushes identity and
    // error_hint regardless of signal presence.
    let id = snap
        .identity
        .clone()
        .unwrap_or_else(|| (String::new(), String::new()));
    history.identity_snapshots.push_front(id);
    history
        .error_hints
        .push_front(snap.error_hint.unwrap_or(false));
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

pub fn push_snapshot_into_history(history: &mut AnomalyHistory, snap: &AnomalyHistorySnapshot) {
    push_snapshot_into_history_at(history, 0, snap);
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
    fn replay_pushes_sentinel_for_absent_identity_and_error_hint() {
        // identity and error_hint are unconditionally pushed by event_loop
        // (resolved identity + bool are always present).  replay must mirror
        // this so deque lengths stay in sync.  When the snapshot carries None
        // (captured from an empty history) the sentinel ("","") / false is used.
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
        // Unconditional push: sentinel values used when snapshot holds None.
        assert_eq!(
            h.identity_snapshots.front(),
            Some(&("".to_string(), "".to_string())),
            "sentinel pushed for absent identity"
        );
        assert_eq!(
            h.error_hints.front(),
            Some(&false),
            "sentinel pushed for absent error_hint"
        );
        // Non-Option fields always push:
        assert_eq!(h.cache_hit_ratios.front(), Some(&None));
        assert_eq!(h.cache_drift_fires.front(), Some(&false));
        assert_eq!(h.cross_pane_edit_paths.front(), Some(&Vec::<String>::new()));
        assert_eq!(h.subagent_hint_samples.front(), Some(&false));
    }

    #[test]
    fn replay_preserves_deque_length_across_present_and_absent_ticks() {
        // Simulate N ticks: some with a real identity/error_hint, some where
        // the snapshot was captured from an empty history (None).  After
        // replaying all N snapshots, every deque must have length N — matching
        // the unconditional-push contract in event_loop.
        let ticks: Vec<AnomalyHistorySnapshot> = vec![
            AnomalyHistorySnapshot {
                identity: Some(("Codex".to_string(), "/a".to_string())),
                error_hint: Some(true),
                cache_hit_ratio: Some(0.8),
                cache_drift_fire: false,
                cross_pane_edit_paths: Vec::new(),
                cost_usd: None,
                input_tokens: None,
                output_tokens: None,
                process_memory_mb: None,
                agent_memory_bytes: None,
                subagent_hint: false,
            },
            AnomalyHistorySnapshot {
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
            },
            AnomalyHistorySnapshot {
                identity: Some(("Claude".to_string(), "/b".to_string())),
                error_hint: Some(false),
                cache_hit_ratio: None,
                cache_drift_fire: true,
                cross_pane_edit_paths: vec!["src/c.rs".to_string()],
                cost_usd: Some(0.5),
                input_tokens: Some(200),
                output_tokens: Some(100),
                process_memory_mb: Some(64.0),
                agent_memory_bytes: Some(1_000_000),
                subagent_hint: true,
            },
        ];

        let mut replayed = AnomalyHistory::default();
        for snap in &ticks {
            push_snapshot_into_history(&mut replayed, snap);
        }

        let n = ticks.len();
        assert_eq!(
            replayed.identity_snapshots.len(),
            n,
            "identity_snapshots deque length must equal tick count"
        );
        assert_eq!(
            replayed.error_hints.len(),
            n,
            "error_hints deque length must equal tick count"
        );
        // Spot-check that real values were preserved (most-recent-first).
        assert_eq!(
            replayed.identity_snapshots.front(),
            Some(&("Claude".to_string(), "/b".to_string()))
        );
        assert_eq!(replayed.error_hints.front(), Some(&false));
        // Second entry (the None tick) should be the sentinel.
        assert_eq!(
            replayed.identity_snapshots.get(1),
            Some(&("".to_string(), "".to_string()))
        );
        assert_eq!(replayed.error_hints.get(1), Some(&false));
    }
}
