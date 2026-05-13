//! Phase 7 v3 (c) (v1.47.0): per-tick snapshot of the inputs the
//! detectors consumed. Mirrors `AnomalyHistory` deque heads, written
//! once per pane per tick. Used for restart-survival replay.

use crate::policy::rules::anomaly::AnomalyHistory;

#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyHistorySnapshot {
    pub identity: Option<(String, String)>,
    pub error_hint: Option<bool>,
    pub cache_hit_ratio: Option<f32>,
    pub cache_drift_fire: bool,
    pub cross_pane_edit_paths: Vec<String>,
    pub cost_usd: Option<f64>,
}

impl AnomalyHistorySnapshot {
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
        }
    }
}

pub fn push_snapshot_into_history_at(
    history: &mut AnomalyHistory,
    tick_unix_secs: u64,
    snap: &AnomalyHistorySnapshot,
) {
    history.tick_unix_secs_samples.push_front(tick_unix_secs);
    history
        .tick_unix_ms_samples
        .push_front(tick_unix_secs.saturating_mul(1000));
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
    }
}
