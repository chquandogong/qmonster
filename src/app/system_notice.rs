use crate::app::version_drift::{VersionDiff, VersionSnapshot, compare};
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::store::sink::EventSink;

/// A system-wide notice (not attached to a single pane) that the UI
/// renders above the per-pane panels. Version drift and other global
/// events route through this struct.
#[derive(Debug, Clone)]
pub struct SystemNotice {
    pub title: String,
    pub body: String,
    pub severity: Severity,
    pub source_kind: SourceKind,
}

/// Route a tmux source failure into a deduplicated system notice.
/// Repeated identical errors are suppressed so the alert queue does not
/// churn every poll tick while tmux is unavailable.
pub fn route_tmux_source_failure(
    last_error: &mut Option<String>,
    err: String,
) -> Option<SystemNotice> {
    if last_error.as_deref() == Some(err.as_str()) {
        return None;
    }
    *last_error = Some(err.clone());
    Some(SystemNotice {
        title: "tmux source failed".into(),
        body: err,
        severity: Severity::Warning,
        source_kind: SourceKind::ProjectCanonical,
    })
}

/// Emit a recovery notice once the tmux source starts succeeding after a
/// previous failure.
pub fn route_tmux_source_recovered(last_error: &mut Option<String>) -> Option<SystemNotice> {
    let previous = last_error.take()?;
    Some(SystemNotice {
        title: "tmux source recovered".into(),
        body: format!("previous error cleared: {previous}"),
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
    })
}

/// Record the initial version snapshot with its own audit kind
/// (not reused from pane-identity kinds — Codex finding #4).
pub fn record_startup_snapshot(sink: &dyn EventSink, snapshot: &VersionSnapshot) {
    let summary = snapshot
        .tools
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    sink.record(AuditEvent {
        kind: AuditEventKind::StartupVersionSnapshot,
        pane_id: "n/a".into(),
        severity: Severity::Safe,
        summary: format!("startup versions: {summary}"),
        provider: None,
        role: None,
    });
}

/// Compare two version snapshots and emit (a) a `VersionDriftDetected`
/// audit event per changed tool, and (b) one aggregated `SystemNotice`
/// at warning severity. Returns the notices so the UI can render them.
pub fn route_version_drift(
    before: &VersionSnapshot,
    after: &VersionSnapshot,
    sink: &dyn EventSink,
) -> Vec<SystemNotice> {
    let diffs = compare(before, after);
    if diffs.is_empty() {
        return Vec::new();
    }
    for d in &diffs {
        sink.record(AuditEvent {
            kind: AuditEventKind::VersionDriftDetected,
            pane_id: "n/a".into(),
            severity: Severity::Warning,
            summary: format!("{}: {} -> {}", d.tool, d.before, d.after),
            provider: None,
            role: None,
        });
    }
    vec![SystemNotice {
        title: "version drift".into(),
        body: render_drift_body(&diffs),
        severity: Severity::Warning,
        source_kind: SourceKind::ProviderOfficial,
    }]
}

fn render_drift_body(diffs: &[VersionDiff]) -> String {
    let parts: Vec<String> = diffs
        .iter()
        .map(|d| format!("{}: {} → {}", d.tool, d.before, d.after))
        .collect();
    format!(
        "{} tool version(s) changed — re-verify (official) tags in docs/ai. {}",
        diffs.len(),
        parts.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::version_drift::VersionSnapshot;
    use crate::domain::audit::AuditEventKind;
    use crate::domain::recommendation::Severity;
    use crate::store::sink::InMemorySink;
    use std::collections::BTreeMap;

    fn snap(entries: &[(&str, &str)]) -> VersionSnapshot {
        let mut tools = BTreeMap::new();
        for (k, v) in entries {
            tools.insert((*k).to_string(), (*v).to_string());
        }
        VersionSnapshot { tools }
    }

    #[test]
    fn record_startup_snapshot_uses_dedicated_kind() {
        let sink = InMemorySink::new();
        let s = snap(&[("claude", "1.0.0"), ("tmux", "3.5")]);
        record_startup_snapshot(&sink, &s);
        let events = sink.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::StartupVersionSnapshot);
        assert_eq!(events[0].severity, Severity::Safe);
        assert!(events[0].summary.contains("claude=1.0.0"));
    }

    #[test]
    fn no_drift_yields_no_notice_and_no_audit() {
        let sink = InMemorySink::new();
        let before = snap(&[("claude", "1.0.0")]);
        let after = before.clone();
        let notices = route_version_drift(&before, &after, &sink);
        assert!(notices.is_empty());
        assert_eq!(sink.len(), 0);
    }

    #[test]
    fn drift_produces_warning_notice_and_audit_per_tool() {
        let sink = InMemorySink::new();
        let before = snap(&[("claude", "1.0.0"), ("tmux", "3.5")]);
        let after = snap(&[("claude", "1.1.0"), ("tmux", "3.5")]);
        let notices = route_version_drift(&before, &after, &sink);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].severity, Severity::Warning);
        assert!(notices[0].body.contains("claude"));
        let events = sink.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::VersionDriftDetected);
    }

    #[test]
    fn notice_carries_a_source_kind_for_ui_badge() {
        let sink = InMemorySink::new();
        let before = snap(&[("claude", "1.0.0")]);
        let after = snap(&[("claude", "1.1.0")]);
        let notices = route_version_drift(&before, &after, &sink);
        assert_eq!(
            notices[0].source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
    }

    #[test]
    fn new_tool_appears_as_drift() {
        let sink = InMemorySink::new();
        let before = snap(&[("claude", "1.0.0")]);
        let after = snap(&[("claude", "1.0.0"), ("codex", "0.9")]);
        let notices = route_version_drift(&before, &after, &sink);
        assert_eq!(notices.len(), 1);
        assert!(notices[0].body.contains("codex"));
    }

    #[test]
    fn repeated_tmux_source_failure_is_deduplicated() {
        let mut last = None;
        let first =
            route_tmux_source_failure(&mut last, "tmux not running".into()).expect("first notice");
        assert_eq!(first.title, "tmux source failed");
        let second = route_tmux_source_failure(&mut last, "tmux not running".into());
        assert!(
            second.is_none(),
            "same tmux source error should not spam notices"
        );
    }

    #[test]
    fn tmux_source_recovery_emits_good_notice_once() {
        let mut last = Some("tmux not running".into());
        let notice = route_tmux_source_recovered(&mut last).expect("recovery notice");
        assert_eq!(notice.title, "tmux source recovered");
        assert_eq!(notice.severity, Severity::Good);
        assert!(notice.body.contains("tmux not running"));
        assert!(route_tmux_source_recovered(&mut last).is_none());
    }

    fn _ensure(_: SystemNotice) {}
}
