use crate::app::config::ActionsMode;
use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{RequestedEffect, Severity};
use crate::policy::gates::{PromptSendGate, check_send_gate};
use crate::store::EventSink;
use crate::tmux::polling::PaneSource;

pub fn handle_prompt_send_action<P: PaneSource>(
    source: &P,
    sink: &dyn EventSink,
    reports: &[PaneReport],
    selected: Option<usize>,
    accepting: bool,
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
) -> SystemNotice {
    let pending = selected
        .and_then(|i| reports.get(i))
        .and_then(first_prompt_send_proposal);

    match pending {
        None => no_pending_notice(accepting),
        Some((target, cmd)) if accepting => {
            accept_prompt_send(source, sink, mode, allow_auto_prompt_send, target, cmd)
        }
        Some((target, cmd)) => dismiss_prompt_send(sink, target, cmd),
    }
}

fn first_prompt_send_proposal(report: &PaneReport) -> Option<(String, String)> {
    let mut proposals: Vec<_> = report
        .effects
        .iter()
        .filter_map(|effect| match effect {
            RequestedEffect::PromptSendProposed {
                target_pane_id,
                slash_command,
                proposal_id,
            } => Some((
                proposal_id.clone(),
                target_pane_id.clone(),
                slash_command.clone(),
            )),
            _ => None,
        })
        .collect();
    proposals.sort_by(|a, b| a.0.cmp(&b.0));
    proposals
        .into_iter()
        .next()
        .map(|(_, target, cmd)| (target, cmd))
}

fn no_pending_notice(accepting: bool) -> SystemNotice {
    SystemNotice {
        title: if accepting {
            "no pending proposal to accept".into()
        } else {
            "no pending proposal to dismiss".into()
        },
        body: "select a pane that carries a PromptSendProposed effect".into(),
        severity: Severity::Concern,
        source_kind: SourceKind::ProjectCanonical,
    }
}

fn accept_prompt_send<P: PaneSource>(
    source: &P,
    sink: &dyn EventSink,
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
    target: String,
    cmd: String,
) -> SystemNotice {
    match check_send_gate(mode, allow_auto_prompt_send) {
        PromptSendGate::Blocked => {
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendBlocked,
                pane_id: target.clone(),
                severity: Severity::Warning,
                summary: format!("{target} {cmd} (blocked; observe_only mode)"),
                provider: None,
                role: None,
            });
            SystemNotice {
                title: "accept blocked (observe_only)".into(),
                body: format!(
                    "{target} \u{2192} `{cmd}`: ObserveOnly mode blocks confirmation (PromptSendBlocked logged)"
                ),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
            }
        }
        PromptSendGate::AutoSendOff => {
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendAccepted,
                pane_id: target.clone(),
                severity: Severity::Warning,
                summary: format!("{target} {cmd} (acknowledged by operator; auto-send disabled)"),
                provider: None,
                role: None,
            });
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendBlocked,
                pane_id: target.clone(),
                severity: Severity::Warning,
                summary: format!(
                    "{target} {cmd} (execution blocked; allow_auto_prompt_send=false)"
                ),
                provider: None,
                role: None,
            });
            SystemNotice {
                title: "proposal accepted (send disabled)".into(),
                body: format!(
                    "{target} \u{2192} `{cmd}` (audit: PromptSendAccepted + PromptSendBlocked; set allow_auto_prompt_send=true to enable execution)"
                ),
                severity: Severity::Good,
                source_kind: SourceKind::ProjectCanonical,
            }
        }
        PromptSendGate::Execute => execute_prompt_send(source, sink, target, cmd),
    }
}

fn execute_prompt_send<P: PaneSource>(
    source: &P,
    sink: &dyn EventSink,
    target: String,
    cmd: String,
) -> SystemNotice {
    sink.record(AuditEvent {
        kind: AuditEventKind::PromptSendAccepted,
        pane_id: target.clone(),
        severity: Severity::Warning,
        summary: format!("{target} {cmd} (acknowledged by operator; executing)"),
        provider: None,
        role: None,
    });
    match source.send_keys(&target, &cmd) {
        Ok(()) => {
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendCompleted,
                pane_id: target.clone(),
                severity: Severity::Safe,
                summary: format!("{target} {cmd} (sent; operator-confirmed)"),
                provider: None,
                role: None,
            });
            SystemNotice {
                title: "command sent".into(),
                body: format!("{target} \u{2192} `{cmd}` (tmux send-keys completed)"),
                severity: Severity::Good,
                source_kind: SourceKind::ProjectCanonical,
            }
        }
        Err(e) => {
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendFailed,
                pane_id: target.clone(),
                severity: Severity::Warning,
                summary: format!("{target} {cmd} (send failed: {e})"),
                provider: None,
                role: None,
            });
            SystemNotice {
                title: "send failed".into(),
                body: format!("{target} \u{2192} `{cmd}`: tmux error \u{2014} {e}"),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
            }
        }
    }
}

fn dismiss_prompt_send(sink: &dyn EventSink, target: String, cmd: String) -> SystemNotice {
    sink.record(AuditEvent {
        kind: AuditEventKind::PromptSendRejected,
        pane_id: target.clone(),
        severity: Severity::Safe,
        summary: format!("{target} {cmd} (dismissed by operator)"),
        provider: None,
        role: None,
    });
    SystemNotice {
        title: "proposal dismissed".into(),
        body: format!("{target} \u{2192} `{cmd}` (PromptSendRejected logged)"),
        severity: Severity::Safe,
        source_kind: SourceKind::ProjectCanonical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::signal::SignalSet;
    use crate::store::InMemorySink;
    use crate::tmux::polling::PollingError;
    use crate::tmux::types::{RawPaneSnapshot, WindowTarget};
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestSource {
        calls: Mutex<Vec<(String, String)>>,
        fail: Option<&'static str>,
    }

    impl PaneSource for TestSource {
        fn list_panes(
            &self,
            _target: Option<&WindowTarget>,
        ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
            Ok(vec![])
        }

        fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
            Ok(None)
        }

        fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
            Ok(vec![])
        }

        fn capture_tail(&self, _pane_id: &str, _lines: usize) -> Result<String, PollingError> {
            Ok(String::new())
        }

        fn send_keys(&self, pane_id: &str, text: &str) -> Result<(), PollingError> {
            self.calls
                .lock()
                .unwrap()
                .push((pane_id.to_string(), text.to_string()));
            match self.fail {
                Some(msg) => Err(PollingError::NonZero(msg.into())),
                None => Ok(()),
            }
        }
    }

    fn base_report(effects: Vec<RequestedEffect>) -> PaneReport {
        PaneReport {
            pane_id: "%1".into(),
            session_name: "qwork".into(),
            window_index: "1".into(),
            provider: Provider::Claude,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Claude,
                    instance: 1,
                    role: Role::Main,
                    pane_id: "%1".into(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations: vec![],
            effects,
            dead: false,
            current_path: "/repo".into(),
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
        }
    }

    fn proposal(id: &str, cmd: &str) -> RequestedEffect {
        RequestedEffect::PromptSendProposed {
            target_pane_id: "%1".into(),
            slash_command: cmd.into(),
            proposal_id: id.into(),
        }
    }

    #[test]
    fn no_pending_accept_returns_concern_without_audit() {
        let source = TestSource::default();
        let sink = InMemorySink::new();

        let notice = handle_prompt_send_action(
            &source,
            &sink,
            &[base_report(vec![])],
            Some(0),
            true,
            ActionsMode::RecommendOnly,
            false,
        );

        assert_eq!(notice.title, "no pending proposal to accept");
        assert_eq!(notice.severity, Severity::Concern);
        assert!(sink.is_empty());
    }

    #[test]
    fn observe_only_accept_records_blocked_only() {
        let source = TestSource::default();
        let sink = InMemorySink::new();

        let notice = handle_prompt_send_action(
            &source,
            &sink,
            &[base_report(vec![proposal("%1:/status", "/status")])],
            Some(0),
            true,
            ActionsMode::ObserveOnly,
            true,
        );

        let events = sink.snapshot();
        assert_eq!(notice.title, "accept blocked (observe_only)");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::PromptSendBlocked);
        assert!(source.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn auto_send_off_records_accept_then_blocked() {
        let source = TestSource::default();
        let sink = InMemorySink::new();

        let notice = handle_prompt_send_action(
            &source,
            &sink,
            &[base_report(vec![proposal("%1:/status", "/status")])],
            Some(0),
            true,
            ActionsMode::RecommendOnly,
            false,
        );

        let kinds: Vec<_> = sink.snapshot().iter().map(|event| event.kind).collect();
        assert_eq!(notice.title, "proposal accepted (send disabled)");
        assert_eq!(
            kinds,
            vec![
                AuditEventKind::PromptSendAccepted,
                AuditEventKind::PromptSendBlocked
            ]
        );
        assert!(source.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn execute_accept_sends_lowest_proposal_id_and_records_completion() {
        let source = TestSource::default();
        let sink = InMemorySink::new();

        let notice = handle_prompt_send_action(
            &source,
            &sink,
            &[base_report(vec![
                proposal("%1:/z-last", "/z-last"),
                proposal("%1:/a-first", "/a-first"),
            ])],
            Some(0),
            true,
            ActionsMode::RecommendOnly,
            true,
        );

        let events = sink.snapshot();
        assert_eq!(notice.title, "command sent");
        assert_eq!(
            *source.calls.lock().unwrap(),
            vec![("%1".into(), "/a-first".into())]
        );
        assert_eq!(events[0].kind, AuditEventKind::PromptSendAccepted);
        assert_eq!(events[1].kind, AuditEventKind::PromptSendCompleted);
    }

    #[test]
    fn execute_send_failure_records_failed_notice() {
        let source = TestSource {
            calls: Mutex::new(Vec::new()),
            fail: Some("tmux unavailable"),
        };
        let sink = InMemorySink::new();

        let notice = handle_prompt_send_action(
            &source,
            &sink,
            &[base_report(vec![proposal("%1:/status", "/status")])],
            Some(0),
            true,
            ActionsMode::RecommendOnly,
            true,
        );

        let events = sink.snapshot();
        assert_eq!(notice.title, "send failed");
        assert_eq!(events[0].kind, AuditEventKind::PromptSendAccepted);
        assert_eq!(events[1].kind, AuditEventKind::PromptSendFailed);
    }

    #[test]
    fn dismiss_records_rejected_without_sending() {
        let source = TestSource::default();
        let sink = InMemorySink::new();

        let notice = handle_prompt_send_action(
            &source,
            &sink,
            &[base_report(vec![proposal("%1:/status", "/status")])],
            Some(0),
            false,
            ActionsMode::RecommendOnly,
            true,
        );

        let events = sink.snapshot();
        assert_eq!(notice.title, "proposal dismissed");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::PromptSendRejected);
        assert!(source.calls.lock().unwrap().is_empty());
    }
}
