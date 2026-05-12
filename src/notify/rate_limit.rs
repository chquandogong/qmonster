use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::domain::recommendation::Severity;

/// Per-(pane, action) throttling. Severity determines the minimum gap
/// between fires: `Risk` fires most often so the operator never misses
/// an approval wait; `Safe` events are suppressed aggressively.
#[derive(Debug, Default)]
pub struct RateLimiter {
    last: HashMap<(String, &'static str), Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn should_fire(
        &mut self,
        pane_id: &str,
        action: &'static str,
        severity: Severity,
        now: Instant,
    ) -> bool {
        let key = (pane_id.to_string(), action);
        let gap = min_gap(severity);
        match self.last.get(&key).copied() {
            Some(prev) if now.saturating_duration_since(prev) < gap => false,
            _ => {
                self.last.insert(key, now);
                true
            }
        }
    }
}

fn min_gap(severity: Severity) -> Duration {
    match severity {
        Severity::Risk => Duration::from_secs(5),
        Severity::Warning => Duration::from_secs(30),
        Severity::Concern => Duration::from_secs(60),
        Severity::Good | Severity::Safe => Duration::from_secs(120),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::recommendation::Severity;
    use std::time::{Duration, Instant};

    #[test]
    fn first_event_is_always_allowed() {
        let mut l = RateLimiter::new();
        assert!(l.should_fire("%1", "log-storm", Severity::Warning, Instant::now()));
    }

    #[test]
    fn duplicate_event_within_window_is_throttled() {
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        assert!(l.should_fire("%1", "log-storm", Severity::Warning, t0));
        assert!(!l.should_fire(
            "%1",
            "log-storm",
            Severity::Warning,
            t0 + Duration::from_millis(500)
        ));
    }

    #[test]
    fn duplicate_event_after_window_fires_again() {
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        assert!(l.should_fire("%1", "log-storm", Severity::Warning, t0));
        let later = t0 + Duration::from_secs(60);
        assert!(l.should_fire("%1", "log-storm", Severity::Warning, later));
    }

    #[test]
    fn risk_severity_has_shorter_throttle_than_warning() {
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        l.should_fire("%1", "permission", Severity::Risk, t0);
        l.should_fire("%2", "log-storm", Severity::Warning, t0);
        // 6 seconds later, a Risk event should fire again, Warning should NOT.
        let dt = t0 + Duration::from_secs(6);
        assert!(l.should_fire("%1", "permission", Severity::Risk, dt));
        assert!(!l.should_fire("%2", "log-storm", Severity::Warning, dt));
    }

    #[test]
    fn different_actions_on_same_pane_are_independent() {
        // Throttling a "log-storm" notification on %1 must not suppress
        // a separate "permission-wait" notification on the same pane.
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        assert!(l.should_fire("%1", "log-storm", Severity::Warning, t0));
        assert!(l.should_fire("%1", "permission-wait", Severity::Risk, t0));
        // Within 1s neither fires again.
        let dt = t0 + Duration::from_secs(1);
        assert!(!l.should_fire("%1", "log-storm", Severity::Warning, dt));
        assert!(!l.should_fire("%1", "permission-wait", Severity::Risk, dt));
    }

    #[test]
    fn same_action_on_different_panes_is_independent() {
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        assert!(l.should_fire("%1", "log-storm", Severity::Warning, t0));
        // A second pane firing the same action immediately must not be
        // throttled by %1's recent fire.
        assert!(l.should_fire("%2", "log-storm", Severity::Warning, t0));
    }

    #[test]
    fn concern_severity_obeys_sixty_second_gap() {
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        l.should_fire("%1", "ctx-warn", Severity::Concern, t0);
        // 59 seconds later: still throttled.
        assert!(!l.should_fire(
            "%1",
            "ctx-warn",
            Severity::Concern,
            t0 + Duration::from_secs(59)
        ));
        // 60 seconds later: free to fire.
        assert!(l.should_fire(
            "%1",
            "ctx-warn",
            Severity::Concern,
            t0 + Duration::from_secs(60)
        ));
    }

    #[test]
    fn safe_severity_uses_longest_gap() {
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        l.should_fire("%1", "noise", Severity::Safe, t0);
        // 119s later still throttled — Safe gets the longest gap (120s).
        assert!(!l.should_fire("%1", "noise", Severity::Safe, t0 + Duration::from_secs(119)));
        assert!(l.should_fire("%1", "noise", Severity::Safe, t0 + Duration::from_secs(120)));
    }

    #[test]
    fn first_fire_resets_window_even_when_severity_changes_on_second_call() {
        // If a pane escalates Concern → Warning on the same action,
        // the second call's severity decides whether the window has
        // elapsed. With Warning's 30s gap, a Concern fire at t0
        // should NOT block a Warning escalation at t0 + 31s.
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        l.should_fire("%1", "ctx", Severity::Concern, t0);
        assert!(l.should_fire("%1", "ctx", Severity::Warning, t0 + Duration::from_secs(31)));
    }
}
