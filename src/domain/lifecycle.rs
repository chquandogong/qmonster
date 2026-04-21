use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneLifecycleEvent {
    Appeared,
    Reappeared,
    BecameDead,
    Unchanged,
}

/// Tracks pane alive/dead transitions so the policy engine can drain
/// alerts and reset pressure state on zombie panes / session re-attach
/// (Gemini G-12).
#[derive(Debug, Default)]
pub struct PaneLifecycle {
    seen: HashMap<String, bool>, // pane_id -> dead?
}

impl PaneLifecycle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, pane_id: &str, dead: bool) -> PaneLifecycleEvent {
        let prev = self.seen.insert(pane_id.to_string(), dead);
        match (prev, dead) {
            (None, _) => PaneLifecycleEvent::Appeared,
            (Some(true), false) => PaneLifecycleEvent::Reappeared,
            (Some(false), true) => PaneLifecycleEvent::BecameDead,
            (Some(prev_dead), cur_dead) if prev_dead == cur_dead => PaneLifecycleEvent::Unchanged,
            _ => PaneLifecycleEvent::Unchanged,
        }
    }

    /// Mark a pane as gone from the tmux list (session detach, window
    /// closed, etc.). Internally treated as dead so the next `observe`
    /// with `dead=false` produces `Reappeared`, which is the signal
    /// policy uses to drain stale alerts on re-attach.
    pub fn forget(&mut self, pane_id: &str) -> bool {
        if let Some(state) = self.seen.get_mut(pane_id) {
            *state = true;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alive_then_dead_emits_dropped() {
        let mut lc = PaneLifecycle::new();
        let ev = lc.observe("%1", false);
        assert_eq!(ev, PaneLifecycleEvent::Appeared);
        let ev = lc.observe("%1", true);
        assert_eq!(ev, PaneLifecycleEvent::BecameDead);
    }

    #[test]
    fn dead_then_alive_emits_reappeared() {
        let mut lc = PaneLifecycle::new();
        lc.observe("%1", true); // appears already dead
        let ev = lc.observe("%1", false);
        assert_eq!(ev, PaneLifecycleEvent::Reappeared);
    }

    #[test]
    fn disappear_then_reappear_emits_reappeared_with_reset() {
        let mut lc = PaneLifecycle::new();
        lc.observe("%1", false);
        let removed = lc.forget("%1");
        assert!(removed);
        let ev = lc.observe("%1", false);
        assert_eq!(ev, PaneLifecycleEvent::Reappeared);
    }

    #[test]
    fn steady_alive_emits_unchanged() {
        let mut lc = PaneLifecycle::new();
        lc.observe("%1", false);
        let ev = lc.observe("%1", false);
        assert_eq!(ev, PaneLifecycleEvent::Unchanged);
    }
}
