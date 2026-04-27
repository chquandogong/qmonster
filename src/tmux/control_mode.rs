use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::thread;

use crate::tmux::commands::{
    KEY_SETTLE_DELAY, SUBMIT_KEY, capture_tail_args, current_target_args, list_panes_args,
    list_windows_args, send_key_args, send_keys_literal_args,
};
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
        match client.run_command(args) {
            Ok(output) => Ok(output),
            Err(first) if is_control_transport_error(&first) => {
                *client = ControlModeClient::attach_current().map_err(|reconnect| {
                    PollingError::Command(format!(
                        "control-mode reconnect after {first}: {reconnect}"
                    ))
                })?;
                client.run_command(args)
            }
            Err(e) => Err(e),
        }
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
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl ControlModeClient {
    fn attach_current() -> Result<Self, PollingError> {
        let mut child = Command::new("tmux")
            .args(["-C", "attach-session"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| PollingError::Command("tmux control-mode stdin missing".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PollingError::Command("tmux control-mode stdout missing".into()))?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };
        let _ = client.read_response()?;
        Ok(client)
    }

    fn run_command(&mut self, args: &[String]) -> Result<Vec<String>, PollingError> {
        writeln!(self.stdin, "{}", command_line(args))
            .map_err(|e| PollingError::Command(e.to_string()))?;
        self.stdin
            .flush()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        self.read_response()
    }

    fn read_response(&mut self) -> Result<Vec<String>, PollingError> {
        read_control_block(&mut self.stdout)
    }
}

impl Drop for ControlModeClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
