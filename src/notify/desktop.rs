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
        use notify_rust::{Notification, Urgency};
        let urgency = match severity {
            Severity::Risk | Severity::Warning => Urgency::Critical,
            Severity::Concern => Urgency::Normal,
            Severity::Good | Severity::Safe => Urgency::Low,
        };
        // Best-effort — we do not abort the event loop if the desktop
        // has no notification daemon running.
        let _ = Notification::new()
            .summary(title)
            .body(body)
            .urgency(urgency)
            .show();
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

    #[test]
    fn summarize_format_is_stable() {
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "waiting".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
        };
        let (title, body) = summarize(&rec, "%1");
        assert_eq!(title, "Qmonster [W]");
        assert!(body.starts_with("%1: notify-input-wait"));
    }
}
