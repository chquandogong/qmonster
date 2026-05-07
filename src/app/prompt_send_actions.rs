use crate::app::config::ActionsMode;
use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{RequestedEffect, Severity};
use crate::policy::gates::{PromptSendGate, check_send_gate};
use crate::store::{EventSink, SqliteRecommendationLifecycleSink};
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
    handle_prompt_send_action_with_lifecycle(
        source,
        sink,
        None,
        reports,
        selected,
        accepting,
        mode,
        allow_auto_prompt_send,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn handle_prompt_send_action_with_lifecycle<P: PaneSource>(
    source: &P,
    sink: &dyn EventSink,
    lifecycle_sink: Option<&SqliteRecommendationLifecycleSink>,
    reports: &[PaneReport],
    selected: Option<usize>,
    accepting: bool,
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
) -> SystemNotice {
    // v1.38 audit-plumb fix: lookup-path now uses the `_full` helper so the
    // proposal_id flows into the audit chain alongside target + slash command,
    // matching the snapshot-path's `handle_prompt_send_action_for_proposal`.
    let pending = selected
        .and_then(|i| reports.get(i))
        .and_then(first_prompt_send_proposal_full);

    match pending {
        None => no_pending_notice(accepting),
        Some((target, cmd, proposal_id)) if accepting => accept_prompt_send(
            source,
            sink,
            lifecycle_sink,
            mode,
            allow_auto_prompt_send,
            target,
            cmd,
            proposal_id,
        ),
        Some((target, cmd, proposal_id)) => {
            dismiss_prompt_send(sink, lifecycle_sink, target, cmd, proposal_id)
        }
    }
}

/// Variant of `handle_prompt_send_action` that uses an explicit snapshot
/// of the proposal to act on (rather than re-selecting the first proposal
/// from the live report). Use this when an `ActionExplainModal` confirm
/// path needs to honor exactly the proposal the operator saw in the
/// modal, even if the live report's proposal ordering has changed.
///
/// Mirrors `handle_prompt_send_action`'s audit chain semantics — the
/// underlying `accept_prompt_send` / `dismiss_prompt_send` helpers are
/// reused directly.
#[allow(clippy::too_many_arguments)]
pub fn handle_prompt_send_action_for_proposal<P: PaneSource>(
    source: &P,
    sink: &dyn EventSink,
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
    target_pane_id: &str,
    slash_command: &str,
    proposal_id: &str,
    accepting: bool,
) -> SystemNotice {
    handle_prompt_send_action_for_proposal_with_lifecycle(
        source,
        sink,
        None,
        mode,
        allow_auto_prompt_send,
        target_pane_id,
        slash_command,
        proposal_id,
        accepting,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn handle_prompt_send_action_for_proposal_with_lifecycle<P: PaneSource>(
    source: &P,
    sink: &dyn EventSink,
    lifecycle_sink: Option<&SqliteRecommendationLifecycleSink>,
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
    target_pane_id: &str,
    slash_command: &str,
    proposal_id: &str,
    accepting: bool,
) -> SystemNotice {
    if accepting {
        accept_prompt_send(
            source,
            sink,
            lifecycle_sink,
            mode,
            allow_auto_prompt_send,
            target_pane_id.to_string(),
            slash_command.to_string(),
            proposal_id.to_string(),
        )
    } else {
        dismiss_prompt_send(
            sink,
            lifecycle_sink,
            target_pane_id.to_string(),
            slash_command.to_string(),
            proposal_id.to_string(),
        )
    }
}

pub(crate) fn first_prompt_send_proposal(report: &PaneReport) -> Option<(String, String)> {
    first_prompt_send_proposal_full(report).map(|(target, cmd, _)| (target, cmd))
}

/// v1.38 Bug B fix — sibling of `first_prompt_send_proposal` that also
/// returns the `proposal_id`. The Action Explainer modal needs to
/// snapshot the proposal_id at modal-open time so the confirm path
/// can validate the proposal still exists in the live reports (and
/// surface a "proposal vanished" notice if polling has since dropped
/// it). Sort key matches the original helper — lowest proposal_id
/// wins for deterministic operator selection.
pub(crate) fn first_prompt_send_proposal_full(
    report: &PaneReport,
) -> Option<(String, String, String)> {
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
        .map(|(pid, target, cmd)| (target, cmd, pid))
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

#[allow(clippy::too_many_arguments)]
fn accept_prompt_send<P: PaneSource>(
    source: &P,
    sink: &dyn EventSink,
    lifecycle_sink: Option<&SqliteRecommendationLifecycleSink>,
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
    target: String,
    cmd: String,
    proposal_id: String,
) -> SystemNotice {
    match check_send_gate(mode, allow_auto_prompt_send) {
        PromptSendGate::Blocked => {
            let summary =
                format!("{target} {cmd} (proposal {proposal_id}) (blocked; observe_only mode)");
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendBlocked,
                pane_id: target.clone(),
                severity: Severity::Warning,
                summary: summary.clone(),
                provider: None,
                role: None,
            });
            record_prompt_lifecycle_outcome(
                lifecycle_sink,
                &target,
                &cmd,
                crate::store::RecommendationOutcome::Blocked,
                summary,
            );
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
            let accepted_summary = format!(
                "{target} {cmd} (proposal {proposal_id}) (acknowledged by operator; auto-send disabled)"
            );
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendAccepted,
                pane_id: target.clone(),
                severity: Severity::Warning,
                summary: accepted_summary.clone(),
                provider: None,
                role: None,
            });
            record_prompt_lifecycle_outcome(
                lifecycle_sink,
                &target,
                &cmd,
                crate::store::RecommendationOutcome::Accepted,
                accepted_summary,
            );
            let blocked_summary = format!(
                "{target} {cmd} (proposal {proposal_id}) (execution blocked; allow_auto_prompt_send=false)"
            );
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendBlocked,
                pane_id: target.clone(),
                severity: Severity::Warning,
                summary: blocked_summary.clone(),
                provider: None,
                role: None,
            });
            record_prompt_lifecycle_outcome(
                lifecycle_sink,
                &target,
                &cmd,
                crate::store::RecommendationOutcome::Blocked,
                blocked_summary,
            );
            SystemNotice {
                title: "proposal accepted (send disabled)".into(),
                body: format!(
                    "{target} \u{2192} `{cmd}` (audit: PromptSendAccepted + PromptSendBlocked; set allow_auto_prompt_send=true to enable execution)"
                ),
                severity: Severity::Good,
                source_kind: SourceKind::ProjectCanonical,
            }
        }
        PromptSendGate::Execute => {
            execute_prompt_send(source, sink, lifecycle_sink, target, cmd, proposal_id)
        }
    }
}

fn execute_prompt_send<P: PaneSource>(
    source: &P,
    sink: &dyn EventSink,
    lifecycle_sink: Option<&SqliteRecommendationLifecycleSink>,
    target: String,
    cmd: String,
    proposal_id: String,
) -> SystemNotice {
    let accepted_summary =
        format!("{target} {cmd} (proposal {proposal_id}) (acknowledged by operator; executing)");
    sink.record(AuditEvent {
        kind: AuditEventKind::PromptSendAccepted,
        pane_id: target.clone(),
        severity: Severity::Warning,
        summary: accepted_summary.clone(),
        provider: None,
        role: None,
    });
    record_prompt_lifecycle_outcome(
        lifecycle_sink,
        &target,
        &cmd,
        crate::store::RecommendationOutcome::Accepted,
        accepted_summary,
    );
    match source.send_keys(&target, &cmd) {
        Ok(()) => {
            let completed_summary =
                format!("{target} {cmd} (proposal {proposal_id}) (sent; operator-confirmed)");
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendCompleted,
                pane_id: target.clone(),
                severity: Severity::Safe,
                summary: completed_summary.clone(),
                provider: None,
                role: None,
            });
            record_prompt_lifecycle_outcome(
                lifecycle_sink,
                &target,
                &cmd,
                crate::store::RecommendationOutcome::Completed,
                completed_summary,
            );
            SystemNotice {
                title: "command sent".into(),
                body: format!("{target} \u{2192} `{cmd}` (tmux send-keys completed)"),
                severity: Severity::Good,
                source_kind: SourceKind::ProjectCanonical,
            }
        }
        Err(e) => {
            let failed_summary =
                format!("{target} {cmd} (proposal {proposal_id}) (send failed: {e})");
            sink.record(AuditEvent {
                kind: AuditEventKind::PromptSendFailed,
                pane_id: target.clone(),
                severity: Severity::Warning,
                summary: failed_summary.clone(),
                provider: None,
                role: None,
            });
            record_prompt_lifecycle_outcome(
                lifecycle_sink,
                &target,
                &cmd,
                crate::store::RecommendationOutcome::Failed,
                failed_summary,
            );
            SystemNotice {
                title: "send failed".into(),
                body: format!("{target} \u{2192} `{cmd}`: tmux error \u{2014} {e}"),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
            }
        }
    }
}

fn dismiss_prompt_send(
    sink: &dyn EventSink,
    lifecycle_sink: Option<&SqliteRecommendationLifecycleSink>,
    target: String,
    cmd: String,
    proposal_id: String,
) -> SystemNotice {
    let summary = format!("{target} {cmd} (proposal {proposal_id}) (dismissed by operator)");
    sink.record(AuditEvent {
        kind: AuditEventKind::PromptSendRejected,
        pane_id: target.clone(),
        severity: Severity::Safe,
        summary: summary.clone(),
        provider: None,
        role: None,
    });
    record_prompt_lifecycle_outcome(
        lifecycle_sink,
        &target,
        &cmd,
        crate::store::RecommendationOutcome::Rejected,
        summary,
    );
    SystemNotice {
        title: "proposal dismissed".into(),
        body: format!("{target} \u{2192} `{cmd}` (PromptSendRejected logged)"),
        severity: Severity::Safe,
        source_kind: SourceKind::ProjectCanonical,
    }
}

fn record_prompt_lifecycle_outcome(
    lifecycle_sink: Option<&SqliteRecommendationLifecycleSink>,
    target: &str,
    cmd: &str,
    outcome: crate::store::RecommendationOutcome,
    summary: String,
) {
    crate::app::insights_lifecycle::record_recommendation_outcome(
        lifecycle_sink,
        target,
        cmd,
        outcome,
        summary,
    );
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
            recent_token_samples: Vec::new(), // F-3: test fixture; production fetches via event_loop
            anomalies: vec![],
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

    /// v1.38 explainer regression: when the Action Explainer modal's
    /// confirm path dispatches via `handle_prompt_send_action_for_proposal`,
    /// the snapshotted `(target, slash_command)` must flow through to
    /// `accept_prompt_send` unchanged — even if the live report carries
    /// a lexicographically lower proposal_id that would win
    /// `first_prompt_send_proposal`'s sort.
    ///
    /// Scenario: operator opened the modal on `pid-2`/`/clear`. Between
    /// modal open and Enter, polling added `pid-1`/`/compact` (lex
    /// first) to the same pane. The pre-fix code re-selected via
    /// `first_prompt_send_proposal` inside `handle_prompt_send_action`
    /// and would fire `/compact`. The new sibling helper bypasses that
    /// lookup and fires the snapshotted `/clear`.
    #[test]
    fn snapshot_confirm_dispatches_snapshotted_command_not_first_proposal() {
        let source = TestSource::default();
        let sink = InMemorySink::new();

        // Two proposals on the same pane; pid-1 sorts before pid-2,
        // so first_prompt_send_proposal would pick `/compact`.
        let _report = base_report(vec![
            proposal("%1:pid-1", "/compact"),
            proposal("%1:pid-2", "/clear"),
        ]);

        // Snapshot points at the lex-second proposal (pid-2 / /clear).
        let notice = handle_prompt_send_action_for_proposal(
            &source,
            &sink,
            ActionsMode::RecommendOnly,
            true, // allow_auto_prompt_send -> Execute path
            "%1",
            "/clear",
            "%1:pid-2",
            true, // accepting
        );

        // Dispatch must use the snapshot, not first_prompt_send_proposal.
        let calls = source.calls.lock().unwrap();
        assert_eq!(
            *calls,
            vec![("%1".into(), "/clear".into())],
            "snapshotted slash_command must reach send_keys, not lex-first proposal"
        );
        drop(calls);

        let events = sink.snapshot();
        assert_eq!(notice.title, "command sent");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, AuditEventKind::PromptSendAccepted);
        assert!(
            events[0].summary.contains("/clear"),
            "audit accept event must reference snapshotted command, got: {}",
            events[0].summary
        );
        assert_eq!(events[1].kind, AuditEventKind::PromptSendCompleted);
        assert!(
            events[1].summary.contains("/clear"),
            "audit completion event must reference snapshotted command, got: {}",
            events[1].summary
        );
    }

    /// v1.38 explainer regression: the dismiss path must also honor the
    /// snapshot — the audit `PromptSendRejected` event records the
    /// snapshotted command, not whichever proposal `first_prompt_send_proposal`
    /// would currently elect.
    #[test]
    fn snapshot_dismiss_records_rejected_for_snapshotted_command() {
        let source = TestSource::default();
        let sink = InMemorySink::new();

        let notice = handle_prompt_send_action_for_proposal(
            &source,
            &sink,
            ActionsMode::RecommendOnly,
            true,
            "%1",
            "/clear",
            "%1:pid-2",
            false, // dismissing
        );

        let events = sink.snapshot();
        assert_eq!(notice.title, "proposal dismissed");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::PromptSendRejected);
        assert!(
            events[0].summary.contains("/clear"),
            "rejected audit must reference snapshotted command, got: {}",
            events[0].summary
        );
        assert!(source.calls.lock().unwrap().is_empty());
    }

    /// v1.38 audit-plumb regression: the proposal_id threaded through
    /// `handle_prompt_send_action_for_proposal` must appear in every
    /// audit summary it produces — accept (via execute path) and reject
    /// (via dismiss path) alike. Before this fix the helper accepted
    /// `proposal_id` as `_proposal_id` and dropped it before reaching
    /// `accept_prompt_send` / `dismiss_prompt_send`, leaving auditors
    /// unable to link an event back to its source proposal.
    #[test]
    fn audit_summary_carries_proposal_id_for_accept_and_reject_paths() {
        // Accept path: proposal_id `pid-42` must appear in both the
        // PromptSendAccepted summary and the PromptSendCompleted summary.
        let source_accept = TestSource::default();
        let sink_accept = InMemorySink::new();
        let _ = handle_prompt_send_action_for_proposal(
            &source_accept,
            &sink_accept,
            ActionsMode::RecommendOnly,
            true, // allow_auto_prompt_send -> Execute path
            "%1",
            "/compact",
            "pid-42",
            true, // accepting
        );
        let accept_events = sink_accept.snapshot();
        assert_eq!(accept_events.len(), 2);
        assert_eq!(accept_events[0].kind, AuditEventKind::PromptSendAccepted);
        assert!(
            accept_events[0].summary.contains("pid-42"),
            "PromptSendAccepted summary must carry proposal_id; got: {}",
            accept_events[0].summary
        );
        assert_eq!(accept_events[1].kind, AuditEventKind::PromptSendCompleted);
        assert!(
            accept_events[1].summary.contains("pid-42"),
            "PromptSendCompleted summary must carry proposal_id; got: {}",
            accept_events[1].summary
        );

        // Reject path: proposal_id `pid-99` must appear in the
        // PromptSendRejected summary.
        let source_reject = TestSource::default();
        let sink_reject = InMemorySink::new();
        let _ = handle_prompt_send_action_for_proposal(
            &source_reject,
            &sink_reject,
            ActionsMode::RecommendOnly,
            true,
            "%1",
            "/compact",
            "pid-99",
            false, // dismissing
        );
        let reject_events = sink_reject.snapshot();
        assert_eq!(reject_events.len(), 1);
        assert_eq!(reject_events[0].kind, AuditEventKind::PromptSendRejected);
        assert!(
            reject_events[0].summary.contains("pid-99"),
            "PromptSendRejected summary must carry proposal_id; got: {}",
            reject_events[0].summary
        );

        // Lookup-path coverage: `handle_prompt_send_action` (no
        // _for_proposal suffix) now sources proposal_id from
        // `first_prompt_send_proposal_full` and must propagate it just
        // like the snapshot path. Cover the AutoSendOff branch (records
        // both Accepted and Blocked) to lock both summaries.
        let source_lookup = TestSource::default();
        let sink_lookup = InMemorySink::new();
        let _ = handle_prompt_send_action(
            &source_lookup,
            &sink_lookup,
            &[base_report(vec![proposal("%1:pid-7", "/status")])],
            Some(0),
            true,                       // accepting
            ActionsMode::RecommendOnly, // -> AutoSendOff path when allow_auto_prompt_send=false
            false,
        );
        let lookup_events = sink_lookup.snapshot();
        assert_eq!(lookup_events.len(), 2);
        assert_eq!(lookup_events[0].kind, AuditEventKind::PromptSendAccepted);
        assert!(
            lookup_events[0].summary.contains("%1:pid-7"),
            "lookup-path PromptSendAccepted must carry proposal_id; got: {}",
            lookup_events[0].summary
        );
        assert_eq!(lookup_events[1].kind, AuditEventKind::PromptSendBlocked);
        assert!(
            lookup_events[1].summary.contains("%1:pid-7"),
            "lookup-path PromptSendBlocked must carry proposal_id; got: {}",
            lookup_events[1].summary
        );
    }
}
