use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::tmux::polling::PollingError;

#[derive(Debug)]
pub(crate) struct ControlModeProcess {
    child: Child,
    pub(crate) stdin: ChildStdin,
    pub(crate) stdout: BufReader<ChildStdout>,
}

impl ControlModeProcess {
    pub(crate) fn attach_current() -> Result<Self, PollingError> {
        let mut child = Command::new("tmux")
            .args(control_mode_attach_args())
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
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }
}

fn control_mode_attach_args() -> [&'static str; 2] {
    ["-C", "attach-session"]
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
        assert_eq!(control_mode_attach_args(), ["-C", "attach-session"]);
    }
}
