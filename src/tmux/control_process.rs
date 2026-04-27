use std::io::{BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

use crate::tmux::polling::PollingError;

#[derive(Debug)]
pub(crate) struct ControlModeProcess {
    child: Child,
    pub(crate) stdin: ChildStdin,
    pub(crate) stdout: BufReader<ChildStdout>,
    stderr: Option<ChildStderr>,
}

impl ControlModeProcess {
    pub(crate) fn attach_current() -> Result<Self, PollingError> {
        Self::attach_current_with(control_mode_attach_args())
    }

    pub(crate) fn attach_current_legacy() -> Result<Self, PollingError> {
        Self::attach_current_with(legacy_control_mode_attach_args())
    }

    fn attach_current_with(args: &[&str]) -> Result<Self, PollingError> {
        let mut child = Command::new("tmux")
            .args(args)
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
        let stderr = child.stderr.take();
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr,
        })
    }

    pub(crate) fn attach_error(&mut self, err: PollingError) -> PollingError {
        let status = self.child.try_wait().ok().flatten().map(|s| s.to_string());
        let stderr = if status.is_some() {
            self.stderr
                .as_mut()
                .and_then(|stderr| read_stderr(stderr).ok())
        } else {
            None
        };
        attach_error_with_diagnostics(err, status.as_deref(), stderr.as_deref())
    }
}

fn control_mode_attach_args() -> &'static [&'static str] {
    // `ignore-size` keeps the hidden control client from resizing panes.
    // `no-output` avoids streaming pane output we only read on demand.
    &["-C", "attach-session", "-f", "ignore-size,no-output"]
}

fn legacy_control_mode_attach_args() -> &'static [&'static str] {
    &["-C", "attach-session"]
}

fn attach_error_with_diagnostics(
    err: PollingError,
    status: Option<&str>,
    stderr: Option<&str>,
) -> PollingError {
    let mut details = vec![err.to_string()];
    if let Some(status) = status.map(str::trim).filter(|s| !s.is_empty()) {
        details.push(format!("status: {status}"));
    }
    if let Some(stderr) = stderr.map(str::trim).filter(|s| !s.is_empty()) {
        details.push(format!("stderr: {stderr}"));
    }
    PollingError::Command(format!(
        "tmux control-mode attach failed: {}",
        details.join("; ")
    ))
}

fn read_stderr(stderr: &mut ChildStderr) -> std::io::Result<String> {
    let mut text = String::new();
    stderr.read_to_string(&mut text)?;
    Ok(text)
}

impl Drop for ControlModeProcess {
    fn drop(&mut self) {
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_args_lock_tmux_control_mode_contract() {
        assert_eq!(
            control_mode_attach_args(),
            &["-C", "attach-session", "-f", "ignore-size,no-output"]
        );
        assert_eq!(legacy_control_mode_attach_args(), &["-C", "attach-session"]);
    }

    #[test]
    fn attach_error_includes_original_error_status_and_stderr() {
        let err = attach_error_with_diagnostics(
            PollingError::NonZero("tmux control-mode stream ended before %begin".into()),
            Some("exit status: 1"),
            Some(" unknown client flag: no-output\n"),
        );

        assert_eq!(
            err.to_string(),
            "tmux command failed: tmux control-mode attach failed: tmux returned non-zero: tmux control-mode stream ended before %begin; status: exit status: 1; stderr: unknown client flag: no-output"
        );
    }

    #[test]
    fn attach_error_omits_blank_status_and_stderr() {
        let err = attach_error_with_diagnostics(
            PollingError::NonZero("%exit no sessions".into()),
            Some("  "),
            Some("\n"),
        );

        assert_eq!(
            err.to_string(),
            "tmux command failed: tmux control-mode attach failed: tmux returned non-zero: %exit no sessions"
        );
    }
}
