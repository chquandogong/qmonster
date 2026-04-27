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
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }
}

impl Drop for ControlModeProcess {
    fn drop(&mut self) {
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
