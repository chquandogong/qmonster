use std::collections::HashMap;

use crate::app::config::ActionsMode;
use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::domain::signal::IdleCause;
use crate::store::EventSink;
use crate::tmux::polling::PaneSource;

pub fn runtime_refresh_provider_label(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Gemini => "Gemini",
        Provider::Qmonster => "Qmonster",
        Provider::Unknown => "Unknown provider",
    }
}

pub fn runtime_refresh_commands(
    provider: Provider,
    _idle_state: Option<IdleCause>,
) -> &'static [&'static str] {
    runtime_refresh_provider_commands(provider)
}

pub fn runtime_refresh_uses_active_safe_only(idle_state: Option<IdleCause>) -> bool {
    matches!(idle_state, None | Some(IdleCause::Stale))
}

pub fn runtime_refresh_dispatch_commands(
    provider: Provider,
    idle_state: Option<IdleCause>,
    pane_id: &str,
    offsets: &mut HashMap<String, usize>,
) -> Vec<&'static str> {
    let commands = runtime_refresh_commands(provider, idle_state);
    if commands.is_empty() {
        return Vec::new();
    }
    if runtime_refresh_sends_one_command_at_a_time(provider, idle_state) {
        let key = format!(
            "{pane_id}:{}-runtime",
            runtime_refresh_provider_key(provider)
        );
        let idx = offsets.entry(key).or_insert(0);
        let command = commands[*idx % commands.len()];
        *idx = (*idx + 1) % commands.len();
        return vec![command];
    }
    commands.to_vec()
}

pub fn runtime_refresh_sends_one_command_at_a_time(
    provider: Provider,
    _idle_state: Option<IdleCause>,
) -> bool {
    matches!(provider, Provider::Claude | Provider::Gemini)
}

pub fn runtime_refresh_sends_escape_first(
    provider: Provider,
    _idle_state: Option<IdleCause>,
) -> bool {
    matches!(provider, Provider::Claude)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RuntimeRefreshSendOutcome {
    pub failed: Option<(String, String)>,
    pub captured_and_closed: bool,
}

pub fn send_runtime_refresh_commands<P: PaneSource>(
    source: &P,
    pane_id: &str,
    provider: Provider,
    idle_state: Option<IdleCause>,
    commands: &[&str],
    capture_lines: usize,
    tail_overlays: &mut HashMap<String, String>,
) -> RuntimeRefreshSendOutcome {
    let mut outcome = RuntimeRefreshSendOutcome::default();
    if runtime_refresh_sends_escape_first(provider, idle_state)
        && let Err(e) = source.send_key(pane_id, "Escape")
    {
        outcome.failed = Some(("Escape".into(), e.to_string()));
        return outcome;
    }

    for cmd in commands {
        if let Err(e) = source.send_keys(pane_id, cmd) {
            outcome.failed = Some(((*cmd).to_string(), e.to_string()));
            break;
        }
        if runtime_refresh_captures_then_closes(provider, cmd) {
            match source.capture_tail(pane_id, capture_lines) {
                Ok(tail) => {
                    if !tail.trim().is_empty() {
                        tail_overlays.insert(pane_id.to_string(), tail);
                    }
                }
                Err(e) => {
                    outcome.failed = Some((format!("{cmd} capture"), e.to_string()));
                }
            }
            if let Err(e) = source.send_key(pane_id, "Escape") {
                if outcome.failed.is_none() {
                    outcome.failed = Some(("Escape".into(), e.to_string()));
                }
            } else if outcome.failed.is_none() {
                outcome.captured_and_closed = true;
            }
            if outcome.failed.is_some() {
                break;
            }
        }
    }

    outcome
}

pub fn runtime_refresh_command_label(commands: &[&str]) -> String {
    commands.join(", ")
}

pub fn runtime_refresh_request_label(active_only: bool, one_at_a_time: bool) -> &'static str {
    if one_at_a_time {
        "operator-requested cycled runtime refresh"
    } else if active_only {
        "operator-requested active-safe runtime refresh"
    } else {
        "operator-requested full runtime refresh"
    }
}

pub fn runtime_refresh_completion_label(captured_and_closed: bool) -> &'static str {
    if captured_and_closed {
        "sent with terminal submit; captured then closed with Escape"
    } else {
        "sent with terminal submit"
    }
}

pub fn runtime_refresh_notice_body(
    pane_id: &str,
    command_label: &str,
    active_only: bool,
    one_at_a_time: bool,
    captured_and_closed: bool,
) -> String {
    if captured_and_closed && one_at_a_time {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; Claude fullscreen output was captured, then Escape closed the surface so the next `u` can run immediately"
        )
    } else if captured_and_closed {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; Claude fullscreen output was captured, then Escape closed the surface and the next poll will parse the captured output"
        )
    } else if one_at_a_time {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; provider runtime sources are sent one at a time, so press `u` again to cycle the next source"
        )
    } else if active_only {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; active or uncertain pane uses only provider commands verified to run without waiting, and the next poll will parse provider output"
        )
    } else {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; full provider runtime refresh requested, and the next poll will parse provider output"
        )
    }
}

pub struct RuntimeRefreshActionOutcome {
    pub notice: SystemNotice,
    pub force_poll: bool,
}

pub fn handle_runtime_refresh_action<P: PaneSource>(
    source: &P,
    sink: &dyn EventSink,
    selected: Option<&PaneReport>,
    mode: ActionsMode,
    capture_lines: usize,
    offsets: &mut HashMap<String, usize>,
    tail_overlays: &mut HashMap<String, String>,
) -> RuntimeRefreshActionOutcome {
    let Some(report) = selected else {
        return RuntimeRefreshActionOutcome {
            notice: SystemNotice {
                title: "no pane selected".into(),
                body: "select a provider pane before requesting runtime refresh".into(),
                severity: Severity::Concern,
                source_kind: SourceKind::ProjectCanonical,
            },
            force_poll: false,
        };
    };

    let pane_id = report.pane_id.clone();
    let provider = report.identity.identity.provider;
    let active_only = runtime_refresh_uses_active_safe_only(report.idle_state);
    let available_commands = runtime_refresh_commands(provider, report.idle_state);
    if available_commands.is_empty() {
        return RuntimeRefreshActionOutcome {
            notice: SystemNotice {
                title: "runtime refresh unavailable".into(),
                body: format!(
                    "{} has no known read-only runtime slash command",
                    runtime_refresh_provider_label(provider)
                ),
                severity: Severity::Concern,
                source_kind: SourceKind::ProjectCanonical,
            },
            force_poll: false,
        };
    }

    if matches!(mode, ActionsMode::ObserveOnly) {
        let command_label = runtime_refresh_command_label(available_commands);
        sink.record(AuditEvent {
            kind: AuditEventKind::RuntimeRefreshBlocked,
            pane_id: pane_id.clone(),
            severity: Severity::Warning,
            summary: format!("{pane_id} {command_label} (blocked; observe_only mode)"),
            provider: Some(provider),
            role: Some(report.identity.identity.role),
        });
        return RuntimeRefreshActionOutcome {
            notice: SystemNotice {
                title: "runtime refresh blocked".into(),
                body: format!("{pane_id} \u{2192} `{command_label}` blocked by observe_only mode"),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
            },
            force_poll: false,
        };
    }

    let commands =
        runtime_refresh_dispatch_commands(provider, report.idle_state, &pane_id, offsets);
    let command_label = runtime_refresh_command_label(&commands);
    let one_at_a_time = runtime_refresh_sends_one_command_at_a_time(provider, report.idle_state);
    sink.record(AuditEvent {
        kind: AuditEventKind::RuntimeRefreshRequested,
        pane_id: pane_id.clone(),
        severity: Severity::Concern,
        summary: format!(
            "{pane_id} {command_label} ({})",
            runtime_refresh_request_label(active_only, one_at_a_time)
        ),
        provider: Some(provider),
        role: Some(report.identity.identity.role),
    });
    let send_outcome = send_runtime_refresh_commands(
        source,
        &pane_id,
        provider,
        report.idle_state,
        &commands,
        capture_lines,
        tail_overlays,
    );
    match send_outcome.failed {
        None => {
            sink.record(AuditEvent {
                kind: AuditEventKind::RuntimeRefreshCompleted,
                pane_id: pane_id.clone(),
                severity: Severity::Safe,
                summary: format!(
                    "{pane_id} {command_label} ({})",
                    runtime_refresh_completion_label(send_outcome.captured_and_closed)
                ),
                provider: Some(provider),
                role: Some(report.identity.identity.role),
            });
            RuntimeRefreshActionOutcome {
                notice: SystemNotice {
                    title: "runtime refresh sent".into(),
                    body: runtime_refresh_notice_body(
                        &pane_id,
                        &command_label,
                        active_only,
                        one_at_a_time,
                        send_outcome.captured_and_closed,
                    ),
                    severity: Severity::Good,
                    source_kind: SourceKind::ProjectCanonical,
                },
                force_poll: true,
            }
        }
        Some((failed_cmd, e)) => {
            sink.record(AuditEvent {
                kind: AuditEventKind::RuntimeRefreshFailed,
                pane_id: pane_id.clone(),
                severity: Severity::Warning,
                summary: format!("{pane_id} {failed_cmd} (send failed: {e})"),
                provider: Some(provider),
                role: Some(report.identity.identity.role),
            });
            RuntimeRefreshActionOutcome {
                notice: SystemNotice {
                    title: "runtime refresh failed".into(),
                    body: format!("{pane_id} \u{2192} `{failed_cmd}`: tmux error \u{2014} {e}"),
                    severity: Severity::Warning,
                    source_kind: SourceKind::ProjectCanonical,
                },
                force_poll: false,
            }
        }
    }
}

fn runtime_refresh_captures_then_closes(provider: Provider, command: &str) -> bool {
    matches!(provider, Provider::Claude) && matches!(command, "/status" | "/context" | "/usage")
}

fn runtime_refresh_provider_commands(provider: Provider) -> &'static [&'static str] {
    // Keep this list to provider-owned control/status surfaces.
    match provider {
        Provider::Claude => &["/status", "/context", "/usage", "/stats"],
        Provider::Codex => &["/status"],
        Provider::Gemini => &["/stats session", "/stats model", "/stats tools"],
        Provider::Qmonster | Provider::Unknown => &[],
    }
}

fn runtime_refresh_provider_key(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
        Provider::Gemini => "gemini",
        Provider::Qmonster => "qmonster",
        Provider::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, ResolvedIdentity, Role};
    use crate::domain::recommendation::RequestedEffect;
    use crate::domain::signal::SignalSet;
    use crate::store::InMemorySink;
    use crate::tmux::polling::PollingError;
    use crate::tmux::types::{RawPaneSnapshot, WindowTarget};
    use std::cell::RefCell;

    #[derive(Default)]
    struct RecordingRefreshSource {
        calls: RefCell<Vec<String>>,
        capture: String,
        fail_send: Option<&'static str>,
    }

    impl RecordingRefreshSource {
        fn with_capture(capture: &str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                capture: capture.into(),
                fail_send: None,
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl PaneSource for RecordingRefreshSource {
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

        fn send_keys(&self, pane_id: &str, keys: &str) -> Result<(), PollingError> {
            self.calls
                .borrow_mut()
                .push(format!("keys:{pane_id}:{keys}"));
            match self.fail_send {
                Some(msg) => Err(PollingError::NonZero(msg.into())),
                None => Ok(()),
            }
        }

        fn send_key(&self, pane_id: &str, key: &str) -> Result<(), PollingError> {
            self.calls.borrow_mut().push(format!("key:{pane_id}:{key}"));
            Ok(())
        }

        fn capture_tail(&self, pane_id: &str, _lines: usize) -> Result<String, PollingError> {
            self.calls.borrow_mut().push(format!("capture:{pane_id}"));
            Ok(self.capture.clone())
        }
    }

    fn base_report(provider: Provider) -> PaneReport {
        PaneReport {
            pane_id: "%1".into(),
            session_name: "qwork".into(),
            window_index: "1".into(),
            provider,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider,
                    instance: 1,
                    role: Role::Main,
                    pane_id: "%1".into(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations: vec![],
            effects: vec![RequestedEffect::Notify],
            dead: false,
            current_path: "/repo".into(),
            current_command: "cli".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
        }
    }

    #[test]
    fn runtime_refresh_action_requires_selected_pane() {
        let source = RecordingRefreshSource::default();
        let sink = InMemorySink::new();
        let mut offsets = HashMap::new();
        let mut overlays = HashMap::new();

        let outcome = handle_runtime_refresh_action(
            &source,
            &sink,
            None,
            ActionsMode::RecommendOnly,
            40,
            &mut offsets,
            &mut overlays,
        );

        assert_eq!(outcome.notice.title, "no pane selected");
        assert_eq!(outcome.notice.severity, Severity::Concern);
        assert!(!outcome.force_poll);
        assert!(sink.is_empty());
    }

    #[test]
    fn runtime_refresh_action_reports_unavailable_provider_without_audit() {
        let source = RecordingRefreshSource::default();
        let sink = InMemorySink::new();
        let report = base_report(Provider::Unknown);
        let mut offsets = HashMap::new();
        let mut overlays = HashMap::new();

        let outcome = handle_runtime_refresh_action(
            &source,
            &sink,
            Some(&report),
            ActionsMode::RecommendOnly,
            40,
            &mut offsets,
            &mut overlays,
        );

        assert_eq!(outcome.notice.title, "runtime refresh unavailable");
        assert_eq!(outcome.notice.severity, Severity::Concern);
        assert!(!outcome.force_poll);
        assert!(sink.is_empty());
    }

    #[test]
    fn runtime_refresh_action_observe_only_records_blocked() {
        let source = RecordingRefreshSource::default();
        let sink = InMemorySink::new();
        let report = base_report(Provider::Codex);
        let mut offsets = HashMap::new();
        let mut overlays = HashMap::new();

        let outcome = handle_runtime_refresh_action(
            &source,
            &sink,
            Some(&report),
            ActionsMode::ObserveOnly,
            40,
            &mut offsets,
            &mut overlays,
        );

        let events = sink.snapshot();
        assert_eq!(outcome.notice.title, "runtime refresh blocked");
        assert!(!outcome.force_poll);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::RuntimeRefreshBlocked);
        assert!(source.calls().is_empty());
    }

    #[test]
    fn runtime_refresh_action_success_records_request_completion_and_forces_poll() {
        let source = RecordingRefreshSource::with_capture("Claude status\nmodel: opus");
        let sink = InMemorySink::new();
        let report = base_report(Provider::Claude);
        let mut offsets = HashMap::new();
        let mut overlays = HashMap::new();

        let outcome = handle_runtime_refresh_action(
            &source,
            &sink,
            Some(&report),
            ActionsMode::RecommendOnly,
            40,
            &mut offsets,
            &mut overlays,
        );

        let events = sink.snapshot();
        assert_eq!(outcome.notice.title, "runtime refresh sent");
        assert!(outcome.force_poll);
        assert_eq!(events[0].kind, AuditEventKind::RuntimeRefreshRequested);
        assert_eq!(events[1].kind, AuditEventKind::RuntimeRefreshCompleted);
        assert_eq!(
            overlays.get("%1").map(String::as_str),
            Some("Claude status\nmodel: opus")
        );
    }

    #[test]
    fn runtime_refresh_action_send_failure_records_failed_without_forcing_poll() {
        let source = RecordingRefreshSource {
            calls: RefCell::new(Vec::new()),
            capture: String::new(),
            fail_send: Some("tmux unavailable"),
        };
        let sink = InMemorySink::new();
        let report = base_report(Provider::Codex);
        let mut offsets = HashMap::new();
        let mut overlays = HashMap::new();

        let outcome = handle_runtime_refresh_action(
            &source,
            &sink,
            Some(&report),
            ActionsMode::RecommendOnly,
            40,
            &mut offsets,
            &mut overlays,
        );

        let events = sink.snapshot();
        assert_eq!(outcome.notice.title, "runtime refresh failed");
        assert!(!outcome.force_poll);
        assert_eq!(events[0].kind, AuditEventKind::RuntimeRefreshRequested);
        assert_eq!(events[1].kind, AuditEventKind::RuntimeRefreshFailed);
    }

    #[test]
    fn runtime_refresh_commands_for_claude_cycle_status_context_usage_stats_when_idle() {
        assert_eq!(
            runtime_refresh_commands(Provider::Claude, Some(IdleCause::WorkComplete)),
            ["/status", "/context", "/usage", "/stats"]
        );
        assert_eq!(
            runtime_refresh_command_label(runtime_refresh_commands(
                Provider::Claude,
                Some(IdleCause::LimitHit)
            )),
            "/status, /context, /usage, /stats"
        );
    }

    #[test]
    fn runtime_refresh_commands_for_claude_active_cycle_same_runtime_sources() {
        assert_eq!(
            runtime_refresh_commands(Provider::Claude, None),
            ["/status", "/context", "/usage", "/stats"]
        );
        assert_eq!(
            runtime_refresh_commands(Provider::Claude, Some(IdleCause::Stale)),
            ["/status", "/context", "/usage", "/stats"]
        );
    }

    #[test]
    fn runtime_refresh_commands_for_codex_use_status_slash_active_or_idle() {
        assert!(runtime_refresh_uses_active_safe_only(None));
        assert!(runtime_refresh_uses_active_safe_only(Some(
            IdleCause::Stale
        )));
        assert_eq!(runtime_refresh_commands(Provider::Codex, None), ["/status"]);
        assert_eq!(
            runtime_refresh_commands(Provider::Codex, Some(IdleCause::WorkComplete)),
            ["/status"]
        );
    }

    #[test]
    fn runtime_refresh_commands_for_gemini_use_stats_slashes_active_or_idle() {
        assert_eq!(
            runtime_refresh_commands(Provider::Gemini, None),
            ["/stats session", "/stats model", "/stats tools"]
        );
        assert_eq!(
            runtime_refresh_commands(Provider::Gemini, Some(IdleCause::WorkComplete)),
            ["/stats session", "/stats model", "/stats tools"]
        );
    }

    #[test]
    fn runtime_refresh_dispatch_cycles_claude_runtime_sources_one_at_a_time() {
        let mut offsets = HashMap::new();
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Claude, None, "%1", &mut offsets),
            ["/status"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Claude, None, "%1", &mut offsets),
            ["/context"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Claude, None, "%1", &mut offsets),
            ["/usage"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Claude, None, "%1", &mut offsets),
            ["/stats"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Claude, None, "%1", &mut offsets),
            ["/status"]
        );
        assert!(runtime_refresh_sends_escape_first(Provider::Claude, None));
    }

    #[test]
    fn runtime_refresh_claude_status_captures_then_closes_surface() {
        let source = RecordingRefreshSource::with_capture("Claude status\nmodel: opus");
        let mut overlays = HashMap::new();
        let outcome = send_runtime_refresh_commands(
            &source,
            "%1",
            Provider::Claude,
            None,
            &["/status"],
            40,
            &mut overlays,
        );

        assert_eq!(
            source.calls(),
            vec![
                "key:%1:Escape",
                "keys:%1:/status",
                "capture:%1",
                "key:%1:Escape"
            ]
        );
        assert_eq!(
            overlays.get("%1").map(String::as_str),
            Some("Claude status\nmodel: opus")
        );
        assert_eq!(
            outcome,
            RuntimeRefreshSendOutcome {
                failed: None,
                captured_and_closed: true
            }
        );
    }

    #[test]
    fn runtime_refresh_claude_context_and_usage_capture_then_close_surface() {
        for command in ["/context", "/usage"] {
            let source = RecordingRefreshSource::with_capture("Claude fullscreen output");
            let mut overlays = HashMap::new();
            let outcome = send_runtime_refresh_commands(
                &source,
                "%1",
                Provider::Claude,
                None,
                &[command],
                40,
                &mut overlays,
            );

            assert_eq!(
                source.calls(),
                vec![
                    "key:%1:Escape".to_string(),
                    format!("keys:%1:{command}"),
                    "capture:%1".to_string(),
                    "key:%1:Escape".to_string(),
                ]
            );
            assert_eq!(
                overlays.get("%1").map(String::as_str),
                Some("Claude fullscreen output")
            );
            assert_eq!(
                outcome,
                RuntimeRefreshSendOutcome {
                    failed: None,
                    captured_and_closed: true
                }
            );
        }
    }

    #[test]
    fn runtime_refresh_dispatch_cycles_gemini_stats_sources_one_at_a_time() {
        let mut offsets = HashMap::new();
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Gemini, None, "%2", &mut offsets),
            ["/stats session"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Gemini, None, "%2", &mut offsets),
            ["/stats model"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Gemini, None, "%2", &mut offsets),
            ["/stats tools"]
        );
        assert_eq!(
            runtime_refresh_dispatch_commands(Provider::Gemini, None, "%2", &mut offsets),
            ["/stats session"]
        );
        assert!(!runtime_refresh_sends_escape_first(Provider::Gemini, None));
    }
}
