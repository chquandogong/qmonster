use crate::app::event_loop::PaneReport;
use crate::app::system_notice::{SystemNotice, route_version_drift};
use crate::app::version_drift::VersionSnapshot;
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::store::{EventSink, PaneSnapshot, SnapshotInput, SnapshotWriter};

pub fn version_refresh_notices(
    previous: &VersionSnapshot,
    fresh: &VersionSnapshot,
    sink: &dyn EventSink,
) -> Vec<SystemNotice> {
    route_version_drift(previous, fresh, sink)
}

pub fn write_operator_snapshot(
    writer: &SnapshotWriter,
    sink: &dyn EventSink,
    reports: &[PaneReport],
    notices: &[SystemNotice],
) -> SystemNotice {
    let input = snapshot_input_from(reports, notices);
    match writer.write(&input) {
        Ok(path) => {
            sink.record(AuditEvent {
                kind: AuditEventKind::SnapshotWritten,
                pane_id: "n/a".into(),
                severity: Severity::Safe,
                summary: format!("snapshot \u{2192} {}", path.display()),
                provider: None,
                role: None,
            });
            SystemNotice {
                title: "snapshot saved".into(),
                body: path.display().to_string(),
                severity: Severity::Good,
                source_kind: SourceKind::ProjectCanonical,
            }
        }
        Err(e) => SystemNotice {
            title: "snapshot failed".into(),
            body: e.to_string(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
        },
    }
}

fn snapshot_input_from(reports: &[PaneReport], notices: &[SystemNotice]) -> SnapshotInput {
    SnapshotInput {
        reason: "operator-requested (key: s)".into(),
        pane_summaries: reports
            .iter()
            .map(|r| PaneSnapshot {
                pane_id: r.pane_id.clone(),
                provider: format!("{:?}", r.identity.identity.provider),
                role: format!("{:?}", r.identity.identity.role),
                alerts: r
                    .recommendations
                    .iter()
                    .map(|x| x.action.to_string())
                    .collect(),
            })
            .collect(),
        notices: notices
            .iter()
            .map(|n| format!("[{}] {}: {}", n.severity.letter(), n.title, n.body))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InMemorySink, QmonsterPaths};
    use tempfile::{NamedTempFile, TempDir};

    #[test]
    fn snapshot_input_formats_system_notices() {
        let notices = vec![SystemNotice {
            title: "snapshot saved".into(),
            body: "/tmp/qmonster.json".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
        }];

        let input = snapshot_input_from(&[], &notices);

        assert_eq!(input.reason, "operator-requested (key: s)");
        assert_eq!(
            input.notices,
            vec!["[G] snapshot saved: /tmp/qmonster.json"]
        );
    }

    #[test]
    fn write_operator_snapshot_records_audit_and_good_notice() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let writer = SnapshotWriter::new(paths.clone());
        let sink = InMemorySink::new();

        let notice = write_operator_snapshot(&writer, &sink, &[], &[]);

        assert_eq!(notice.title, "snapshot saved");
        assert_eq!(notice.severity, Severity::Good);
        assert!(
            notice
                .body
                .starts_with(paths.snapshot_dir().to_str().unwrap())
        );
        let events = sink.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::SnapshotWritten);
    }

    #[test]
    fn write_operator_snapshot_failure_returns_warning_without_audit() {
        let file = NamedTempFile::new().unwrap();
        let writer = SnapshotWriter::new(QmonsterPaths::at(file.path()));
        let sink = InMemorySink::new();

        let notice = write_operator_snapshot(&writer, &sink, &[], &[]);

        assert_eq!(notice.title, "snapshot failed");
        assert_eq!(notice.severity, Severity::Warning);
        assert!(sink.is_empty());
    }
}
