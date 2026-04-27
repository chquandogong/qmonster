use std::collections::HashMap;

use crate::domain::identity::Provider;
use crate::domain::signal::IdleCause;
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
            "{pane_id} → `{command_label}` sent with terminal submit; Claude `/status` was captured, then Escape closed the fullscreen surface so the next `u` can run immediately"
        )
    } else if captured_and_closed {
        format!(
            "{pane_id} → `{command_label}` sent with terminal submit; Claude `/status` was captured, then Escape closed the fullscreen surface and the next poll will parse the captured output"
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

fn runtime_refresh_captures_then_closes(provider: Provider, command: &str) -> bool {
    matches!(provider, Provider::Claude) && command == "/status"
}

fn runtime_refresh_provider_commands(provider: Provider) -> &'static [&'static str] {
    // Keep this list to provider-owned control/status surfaces.
    match provider {
        Provider::Claude => &["/status", "/usage", "/stats"],
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
    use crate::tmux::polling::PollingError;
    use crate::tmux::types::{RawPaneSnapshot, WindowTarget};
    use std::cell::RefCell;

    #[derive(Default)]
    struct RecordingRefreshSource {
        calls: RefCell<Vec<String>>,
        capture: String,
    }

    impl RecordingRefreshSource {
        fn with_capture(capture: &str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                capture: capture.into(),
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
            Ok(())
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

    #[test]
    fn runtime_refresh_commands_for_claude_cycle_status_usage_stats_when_idle() {
        assert_eq!(
            runtime_refresh_commands(Provider::Claude, Some(IdleCause::WorkComplete)),
            ["/status", "/usage", "/stats"]
        );
        assert_eq!(
            runtime_refresh_command_label(runtime_refresh_commands(
                Provider::Claude,
                Some(IdleCause::LimitHit)
            )),
            "/status, /usage, /stats"
        );
    }

    #[test]
    fn runtime_refresh_commands_for_claude_active_cycle_same_runtime_sources() {
        assert_eq!(
            runtime_refresh_commands(Provider::Claude, None),
            ["/status", "/usage", "/stats"]
        );
        assert_eq!(
            runtime_refresh_commands(Provider::Claude, Some(IdleCause::Stale)),
            ["/status", "/usage", "/stats"]
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
