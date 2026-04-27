use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crate::tmux::polling::{PaneSource, PollingError};
use crate::tmux::types::{
    PANE_LIST_FORMAT, RawPaneSnapshot, WINDOW_LIST_FORMAT, WindowTarget, parse_list_panes_row,
    parse_list_windows_row,
};

const DEFAULT_CAPTURE_LINES: usize = 24;
const SUBMIT_KEY: &str = "C-m";
const KEY_SETTLE_DELAY: Duration = Duration::from_millis(80);

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
        client.run_command(args)
    }
}

impl PaneSource for ControlModeSource {
    fn list_panes(
        &self,
        target: Option<&WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        let fmt = PANE_LIST_FORMAT.replace("\\t", "\t");
        let output = self.run(&list_panes_args(&fmt, target))?;
        let mut rows = Vec::new();
        for line in output {
            if let Some(mut snap) = parse_list_panes_row(&line) {
                if let Ok(tail) = self.capture_tail(&snap.pane_id, self.capture_lines) {
                    snap.tail = tail;
                }
                rows.push(snap);
            }
        }
        Ok(rows)
    }

    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        let Ok(tmux_pane) = std::env::var("TMUX_PANE") else {
            return Ok(None);
        };
        if tmux_pane.trim().is_empty() {
            return Ok(None);
        }
        let fmt = WINDOW_LIST_FORMAT.replace("\\t", "\t");
        let output = self.run(&[
            "display-message".into(),
            "-p".into(),
            "-t".into(),
            tmux_pane,
            fmt,
        ])?;
        Ok(output.first().and_then(|line| parse_list_windows_row(line)))
    }

    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        let fmt = WINDOW_LIST_FORMAT.replace("\\t", "\t");
        let output = self.run(&["list-windows".into(), "-a".into(), "-F".into(), fmt])?;
        let mut targets: Vec<WindowTarget> = output
            .iter()
            .filter_map(|line| parse_list_windows_row(line))
            .collect();
        targets.sort();
        targets.dedup();
        Ok(targets)
    }

    fn capture_tail(&self, pane_id: &str, lines: usize) -> Result<String, PollingError> {
        let start = format!("-{lines}");
        let output = self.run(&[
            "capture-pane".into(),
            "-p".into(),
            "-J".into(),
            "-S".into(),
            start,
            "-t".into(),
            pane_id.into(),
        ])?;
        Ok(output.join("\n"))
    }

    fn send_keys(&self, pane_id: &str, text: &str) -> Result<(), PollingError> {
        self.run(&[
            "send-keys".into(),
            "-t".into(),
            pane_id.into(),
            "-l".into(),
            text.into(),
        ])?;
        thread::sleep(KEY_SETTLE_DELAY);
        self.send_key(pane_id, SUBMIT_KEY)?;
        thread::sleep(KEY_SETTLE_DELAY);
        Ok(())
    }

    fn send_key(&self, pane_id: &str, key: &str) -> Result<(), PollingError> {
        self.run(&["send-keys".into(), "-t".into(), pane_id.into(), key.into()])?;
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

fn read_control_block<R: BufRead>(reader: &mut R) -> Result<Vec<String>, PollingError> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|e| PollingError::Command(e.to_string()))?
            == 0
        {
            return Err(PollingError::NonZero(
                "tmux control-mode stream ended before %begin".into(),
            ));
        }
        let trimmed = trim_line(&line);
        if trimmed.starts_with("%begin ") {
            break;
        }
        if trimmed.starts_with("%exit") {
            return Err(PollingError::NonZero(trimmed.to_string()));
        }
    }

    let mut output = Vec::new();
    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|e| PollingError::Command(e.to_string()))?
            == 0
        {
            return Err(PollingError::NonZero(
                "tmux control-mode stream ended inside output block".into(),
            ));
        }
        let trimmed = trim_line(&line);
        if trimmed.starts_with("%end ") {
            return Ok(output);
        }
        if trimmed.starts_with("%error ") {
            let body = if output.is_empty() {
                "tmux control-mode command failed".into()
            } else {
                output.join("\n")
            };
            return Err(PollingError::NonZero(body));
        }
        output.push(trimmed.to_string());
    }
}

fn trim_line(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn command_line(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote_tmux_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_tmux_arg(arg: &str) -> String {
    let mut quoted = String::with_capacity(arg.len() + 2);
    quoted.push('"');
    for ch in arg.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

fn list_panes_args(fmt: &str, target: Option<&WindowTarget>) -> Vec<String> {
    let mut args = vec!["list-panes".to_string()];
    match target {
        Some(target_window) => {
            args.push("-t".to_string());
            args.push(target_window.label());
        }
        None => args.push("-a".to_string()),
    }
    args.push("-F".to_string());
    args.push(fmt.to_string());
    args
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn control_block_reader_skips_notifications_before_begin() {
        let raw = "%session-changed $1 qmonster\n%output %1 hi\n%begin 1 2 0\nline 1\nline 2\n%end 1 2 0\n";
        let mut cursor = Cursor::new(raw);
        let output = read_control_block(&mut cursor).unwrap();
        assert_eq!(output, vec!["line 1", "line 2"]);
    }

    #[test]
    fn control_block_reader_surfaces_error_body() {
        let raw = "%begin 1 2 0\nbad target\n%error 1 2 0\n";
        let mut cursor = Cursor::new(raw);
        let err = read_control_block(&mut cursor).unwrap_err();
        assert_eq!(err.to_string(), "tmux returned non-zero: bad target");
    }

    #[test]
    fn control_command_line_quotes_parser_sensitive_args() {
        let args = vec![
            "display-message".into(),
            "-p".into(),
            "a b; c \"d\" \\ e".into(),
        ];
        assert_eq!(
            command_line(&args),
            "\"display-message\" \"-p\" \"a b; c \\\"d\\\" \\\\ e\""
        );
    }

    #[test]
    fn control_list_panes_args_preserve_all_sessions_contract() {
        let args = list_panes_args("fmt", None);
        assert_eq!(args, vec!["list-panes", "-a", "-F", "fmt"]);
    }

    #[test]
    fn control_list_panes_args_target_current_window_when_requested() {
        let args = list_panes_args(
            "fmt",
            Some(&WindowTarget {
                session_name: "qmonster".into(),
                window_index: "0".into(),
            }),
        );
        assert_eq!(args, vec!["list-panes", "-t", "qmonster:0", "-F", "fmt"]);
    }
}
