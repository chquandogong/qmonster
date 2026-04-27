use std::io::Write;
use std::sync::Mutex;
use std::thread;

use crate::tmux::commands::{
    KEY_SETTLE_DELAY, SUBMIT_KEY, capture_tail_args, current_target_args, list_panes_args,
    list_windows_args, send_key_args, send_keys_literal_args,
};
use crate::tmux::control_process::ControlModeProcess;
use crate::tmux::control_protocol::{command_line, is_control_transport_error, read_control_block};
use crate::tmux::polling::{PaneSource, PollingError};
use crate::tmux::snapshots::hydrate_pane_snapshots;
use crate::tmux::targets::{parse_current_target, parse_window_targets};
use crate::tmux::types::{PANE_LIST_FORMAT, RawPaneSnapshot, WINDOW_LIST_FORMAT, WindowTarget};

const DEFAULT_CAPTURE_LINES: usize = 24;

/// Production `PaneSource` backed by one tmux control-mode client.
///
/// This intentionally keeps the same raw tmux command surface as
/// `PollingSource`; control mode is a transport swap, not a provider-aware
/// layer.
#[derive(Debug)]
pub struct ControlModeSource {
    capture_lines: usize,
    client: Mutex<ControlModeClient>,
}

impl ControlModeSource {
    pub fn attach_current(capture_lines: usize) -> Result<Self, PollingError> {
        Ok(Self {
            capture_lines: capture_lines.max(1),
            client: Mutex::new(ControlModeClient::attach_current()?),
        })
    }

    fn run(&self, args: &[String]) -> Result<Vec<String>, PollingError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| PollingError::Command("control-mode client lock poisoned".into()))?;
        run_with_reconnect(&mut *client, args, ControlModeClient::attach_current)
    }
}

impl PaneSource for ControlModeSource {
    fn list_panes(
        &self,
        target: Option<&WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        let fmt = PANE_LIST_FORMAT.replace("\\t", "\t");
        let output = self.run(&list_panes_args(&fmt, target))?;
        Ok(hydrate_pane_snapshots(output.iter(), |pane_id| {
            self.capture_tail(pane_id, self.capture_lines).ok()
        }))
    }

    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        let Ok(tmux_pane) = std::env::var("TMUX_PANE") else {
            return Ok(None);
        };
        if tmux_pane.trim().is_empty() {
            return Ok(None);
        }
        let fmt = WINDOW_LIST_FORMAT.replace("\\t", "\t");
        let output = self.run(&current_target_args(&tmux_pane, &fmt))?;
        Ok(parse_current_target(output.iter()))
    }

    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        let fmt = WINDOW_LIST_FORMAT.replace("\\t", "\t");
        let output = self.run(&list_windows_args(&fmt))?;
        Ok(parse_window_targets(output.iter()))
    }

    fn capture_tail(&self, pane_id: &str, lines: usize) -> Result<String, PollingError> {
        let output = self.run(&capture_tail_args(pane_id, lines))?;
        Ok(output.join("\n"))
    }

    fn send_keys(&self, pane_id: &str, text: &str) -> Result<(), PollingError> {
        self.run(&send_keys_literal_args(pane_id, text))?;
        thread::sleep(KEY_SETTLE_DELAY);
        self.send_key(pane_id, SUBMIT_KEY)?;
        thread::sleep(KEY_SETTLE_DELAY);
        Ok(())
    }

    fn send_key(&self, pane_id: &str, key: &str) -> Result<(), PollingError> {
        self.run(&send_key_args(pane_id, key))?;
        thread::sleep(KEY_SETTLE_DELAY);
        Ok(())
    }
}

impl Default for ControlModeSource {
    fn default() -> Self {
        Self::attach_current(DEFAULT_CAPTURE_LINES)
            .expect("failed to attach tmux control-mode source")
    }
}

#[derive(Debug)]
struct ControlModeClient {
    process: ControlModeProcess,
}

impl ControlModeClient {
    fn attach_current() -> Result<Self, PollingError> {
        let mut client = Self {
            process: ControlModeProcess::attach_current()?,
        };
        let _ = client.read_response()?;
        Ok(client)
    }

    fn run_command(&mut self, args: &[String]) -> Result<Vec<String>, PollingError> {
        writeln!(self.process.stdin, "{}", command_line(args))
            .map_err(|e| PollingError::Command(e.to_string()))?;
        self.process
            .stdin
            .flush()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        self.read_response()
    }

    fn read_response(&mut self) -> Result<Vec<String>, PollingError> {
        read_control_block(&mut self.process.stdout)
    }
}

trait ControlModeCommandRunner {
    fn run_command(&mut self, args: &[String]) -> Result<Vec<String>, PollingError>;
}

impl ControlModeCommandRunner for ControlModeClient {
    fn run_command(&mut self, args: &[String]) -> Result<Vec<String>, PollingError> {
        Self::run_command(self, args)
    }
}

fn run_with_reconnect<C, F>(
    client: &mut C,
    args: &[String],
    reconnect: F,
) -> Result<Vec<String>, PollingError>
where
    C: ControlModeCommandRunner,
    F: FnOnce() -> Result<C, PollingError>,
{
    match client.run_command(args) {
        Ok(output) => Ok(output),
        Err(first) if is_control_transport_error(&first) => {
            *client = reconnect().map_err(|reconnect| {
                PollingError::Command(format!("control-mode reconnect after {first}: {reconnect}"))
            })?;
            client.run_command(args)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[derive(Debug)]
    struct ScriptedClient {
        responses: VecDeque<Result<Vec<String>, PollingError>>,
        call_count: Arc<AtomicUsize>,
    }

    impl ScriptedClient {
        fn new(responses: Vec<Result<Vec<String>, PollingError>>) -> Self {
            Self {
                responses: responses.into(),
                call_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn with_counter(
            responses: Vec<Result<Vec<String>, PollingError>>,
            call_count: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                responses: responses.into(),
                call_count,
            }
        }
    }

    impl ControlModeCommandRunner for ScriptedClient {
        fn run_command(&mut self, _args: &[String]) -> Result<Vec<String>, PollingError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.responses
                .pop_front()
                .unwrap_or_else(|| Err(PollingError::Command("script exhausted".into())))
        }
    }

    #[test]
    fn lifecycle_error_reconnects_once_and_retries_command() {
        let first_counter = Arc::new(AtomicUsize::new(0));
        let retry_counter = Arc::new(AtomicUsize::new(0));
        let mut client = ScriptedClient::with_counter(
            vec![Err(PollingError::NonZero("%exit detached".into()))],
            first_counter.clone(),
        );
        let args = vec!["list-panes".into()];

        let output = run_with_reconnect(&mut client, &args, || {
            Ok(ScriptedClient::with_counter(
                vec![Ok(vec!["ok".into()])],
                retry_counter.clone(),
            ))
        })
        .unwrap();

        assert_eq!(output, vec!["ok"]);
        assert_eq!(first_counter.load(Ordering::SeqCst), 1);
        assert_eq!(retry_counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn command_error_does_not_reconnect() {
        let mut client = ScriptedClient::new(vec![Err(PollingError::NonZero("bad target".into()))]);
        let args = vec!["list-panes".into()];
        let reconnect_calls = Arc::new(AtomicUsize::new(0));

        let err = run_with_reconnect(&mut client, &args, || {
            reconnect_calls.fetch_add(1, Ordering::SeqCst);
            Ok(ScriptedClient::new(vec![Ok(vec!["unexpected".into()])]))
        })
        .unwrap_err();

        assert_eq!(err.to_string(), "tmux returned non-zero: bad target");
        assert_eq!(reconnect_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reconnect_failure_includes_original_lifecycle_error() {
        let mut client =
            ScriptedClient::new(vec![Err(PollingError::Command("Broken pipe".into()))]);
        let args = vec!["list-panes".into()];

        let err = run_with_reconnect(&mut client, &args, || {
            Err(PollingError::Command("tmux missing".into()))
        })
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "tmux command failed: control-mode reconnect after tmux command failed: Broken pipe: tmux command failed: tmux missing"
        );
    }
}
