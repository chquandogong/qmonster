use crate::domain::recommendation::{Recommendation, Severity};

/// Trait so tests can assert notifications without spawning real
/// desktop popups.
pub trait NotifyBackend {
    fn notify(&self, title: &str, body: &str, severity: Severity);
}

/// Production backend — `notify-rust`. The implementation is kept
/// intentionally small; severity is only used to pick urgency.
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopNotifier;

impl NotifyBackend for DesktopNotifier {
    fn notify(&self, title: &str, body: &str, severity: Severity) {
        use notify_rust::Notification;
        // Best-effort — we do not abort the event loop if the desktop
        // has no notification daemon running.
        let _ = Notification::new()
            .summary(title)
            .body(body)
            .urgency(urgency_for(severity))
            .show();
    }
}

/// Pure mapping from Severity to notify-rust Urgency. Pulled out so
/// the mapping is unit-testable without spawning real desktop popups.
pub fn urgency_for(severity: Severity) -> notify_rust::Urgency {
    use notify_rust::Urgency;
    match severity {
        Severity::Risk | Severity::Warning => Urgency::Critical,
        Severity::Concern => Urgency::Normal,
        Severity::Good | Severity::Safe => Urgency::Low,
    }
}

/// Utility for translating a `Recommendation` into (title, body).
pub fn summarize(rec: &Recommendation, pane_id: &str) -> (String, String) {
    let title = format!("Qmonster [{}]", rec.severity.letter());
    let body = format!("{pane_id}: {} — {}", rec.action, rec.reason);
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::origin::SourceKind;
    use notify_rust::Urgency;
    use std::cell::RefCell;

    fn make_rec(severity: Severity) -> Recommendation {
        Recommendation {
            action: "notify-input-wait",
            reason: "waiting".into(),
            severity,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
        }
    }

    #[test]
    fn summarize_format_is_stable() {
        let rec = make_rec(Severity::Warning);
        let (title, body) = summarize(&rec, "%1");
        assert_eq!(title, "Qmonster [W]");
        assert!(body.starts_with("%1: notify-input-wait"));
    }

    #[test]
    fn summarize_letter_covers_every_severity() {
        for (sev, expected) in [
            (Severity::Risk, "R"),
            (Severity::Warning, "W"),
            (Severity::Concern, "C"),
            (Severity::Good, "G"),
            (Severity::Safe, "S"),
        ] {
            let rec = make_rec(sev);
            let (title, _) = summarize(&rec, "%1");
            assert!(
                title.contains(&format!("[{expected}]")),
                "expected severity letter [{expected}] in title; got: {title}"
            );
        }
    }

    #[test]
    fn urgency_for_maps_severity_to_urgency() {
        // Critical mapping: Risk and Warning both wake the user.
        assert!(matches!(urgency_for(Severity::Risk), Urgency::Critical));
        assert!(matches!(urgency_for(Severity::Warning), Urgency::Critical));
        // Normal: Concern.
        assert!(matches!(urgency_for(Severity::Concern), Urgency::Normal));
        // Low: positive / baseline observations.
        assert!(matches!(urgency_for(Severity::Good), Urgency::Low));
        assert!(matches!(urgency_for(Severity::Safe), Urgency::Low));
    }

    /// In-memory backend that records every notify call. Lets us
    /// assert event-loop wiring without spawning a real desktop popup.
    struct RecordingBackend {
        calls: RefCell<Vec<(String, String, Severity)>>,
    }
    impl RecordingBackend {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }
    impl NotifyBackend for RecordingBackend {
        fn notify(&self, title: &str, body: &str, severity: Severity) {
            self.calls
                .borrow_mut()
                .push((title.into(), body.into(), severity));
        }
    }

    #[test]
    fn recording_backend_captures_each_call_independently() {
        // Locks the NotifyBackend trait shape against accidental signature
        // drift, which would otherwise be only caught by integration tests.
        let b = RecordingBackend::new();
        b.notify("t1", "b1", Severity::Risk);
        b.notify("t2", "b2", Severity::Concern);
        let calls = b.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].2, Severity::Risk);
        assert_eq!(calls[1].2, Severity::Concern);
    }
}
