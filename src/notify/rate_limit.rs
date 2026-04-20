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
}
