use std::process::Command;

use crate::tmux::polling::PollingError;

pub(crate) fn run_tmux(args: &[String]) -> Result<String, PollingError> {
    let output = Command::new("tmux")
        .args(args)
        .output()
        .map_err(|e| PollingError::Command(e.to_string()))?;
    if !output.status.success() {
        return Err(PollingError::NonZero(stderr_text(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn stderr_text(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_text_trims_tmux_diagnostics() {
        assert_eq!(stderr_text(b" no server running \n"), "no server running");
    }
}
