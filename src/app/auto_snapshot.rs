//! Phase H (v1.42.0): opt-in actuation hook for the F-7c
//! `recommend_snapshot_before_reset` recommendation. When
//! `[reset] auto_snapshot = true`, an operator opts in to having
//! the snapshot written automatically — once per
//! `(pane_id, quota_kind, window_id)` triple. The dedup key is
//! the provider-reported `quota_*_resets_at` (rounded to the
//! nearest minute) so all ticks inside a single reset window
//! agree on the same key.

/// Per-pane, per-quota-window dedup state for Phase H.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AutoSnapshotDedup {
    /// Last `quota_5h_resets_at` (rounded to minute) for which a
    /// snapshot was successfully written, or `None` if no snapshot
    /// has yet been written in the current 5h window.
    pub quota_5h_window_id: Option<u64>,
    /// Same as `quota_5h_window_id` but for the weekly window.
    pub quota_weekly_window_id: Option<u64>,
}

/// Quota window that fired the recommendation. Phase H tracks each
/// quota window's dedup independently so a single tick that fires
/// both can run two snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaKind {
    FiveHour,
    Weekly,
}

impl QuotaKind {
    #[allow(dead_code)] // used in Tasks 4-5 (Phase H)
    pub(crate) fn label(self) -> &'static str {
        match self {
            QuotaKind::FiveHour => "5h",
            QuotaKind::Weekly => "weekly",
        }
    }
}

impl AutoSnapshotDedup {
    /// Returns `true` if the given `(quota_kind, window_id)` has
    /// already been snapshotted in this window. Floor `window_id` to
    /// the nearest minute *before* calling so jitter on
    /// `quota_*_resets_at` doesn't break the match.
    pub fn matches(&self, kind: QuotaKind, window_id: u64) -> bool {
        match kind {
            QuotaKind::FiveHour => self.quota_5h_window_id == Some(window_id),
            QuotaKind::Weekly => self.quota_weekly_window_id == Some(window_id),
        }
    }

    /// Mark a successful snapshot for `(quota_kind, window_id)`.
    pub fn set(&mut self, kind: QuotaKind, window_id: u64) {
        match kind {
            QuotaKind::FiveHour => self.quota_5h_window_id = Some(window_id),
            QuotaKind::Weekly => self.quota_weekly_window_id = Some(window_id),
        }
    }
}

/// Round a `quota_*_resets_at` Unix timestamp (seconds) down to
/// the nearest minute. This absorbs sub-minute jitter from F-5b /
/// F-6 so two ticks emitted within the same window share the same
/// `window_id`.
pub fn floor_to_minute(unix_seconds: u64) -> u64 {
    unix_seconds - (unix_seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_default_has_no_window_ids() {
        let d = AutoSnapshotDedup::default();
        assert_eq!(d.quota_5h_window_id, None);
        assert_eq!(d.quota_weekly_window_id, None);
    }

    #[test]
    fn dedup_set_and_matches_round_trip() {
        let mut d = AutoSnapshotDedup::default();
        assert!(!d.matches(QuotaKind::FiveHour, 1_700_000_000));
        d.set(QuotaKind::FiveHour, 1_700_000_000);
        assert!(d.matches(QuotaKind::FiveHour, 1_700_000_000));
        assert!(!d.matches(QuotaKind::FiveHour, 1_700_000_060)); // different minute
    }

    #[test]
    fn dedup_5h_and_weekly_independent() {
        let mut d = AutoSnapshotDedup::default();
        d.set(QuotaKind::FiveHour, 1_700_000_000);
        assert!(d.matches(QuotaKind::FiveHour, 1_700_000_000));
        assert!(!d.matches(QuotaKind::Weekly, 1_700_000_000));
        d.set(QuotaKind::Weekly, 1_700_000_000);
        assert!(d.matches(QuotaKind::Weekly, 1_700_000_000));
    }

    #[test]
    fn floor_to_minute_drops_seconds() {
        // 1_700_000_037 % 60 == 57 → floors to 1_699_999_980.
        assert_eq!(floor_to_minute(1_700_000_037), 1_699_999_980);
        assert_eq!(floor_to_minute(60), 60);
        assert_eq!(floor_to_minute(59), 0);
        assert_eq!(floor_to_minute(120), 120);
        assert_eq!(floor_to_minute(121), 120);
    }
}
