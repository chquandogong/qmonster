use std::io::BufRead;

use crate::tmux::polling::PollingError;

pub(super) fn read_control_block<R: BufRead>(reader: &mut R) -> Result<Vec<String>, PollingError> {
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

pub(super) fn command_line(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote_tmux_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn is_control_transport_error(err: &PollingError) -> bool {
    match err {
        PollingError::Command(message) => {
            message.contains("Broken pipe")
                || message.contains("broken pipe")
                || message.contains("control-mode")
        }
        PollingError::NonZero(message) => {
            message.starts_with("%exit")
                || message.contains("control-mode stream ended")
                || message.contains("control-mode client")
        }
    }
}

fn trim_line(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
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
    fn control_transport_error_detection_targets_client_lifecycle_only() {
        assert!(is_control_transport_error(&PollingError::NonZero(
            "%exit detached".into()
        )));
        assert!(is_control_transport_error(&PollingError::NonZero(
            "tmux control-mode stream ended before %begin".into()
        )));
        assert!(is_control_transport_error(&PollingError::Command(
            "Broken pipe".into()
        )));
        assert!(!is_control_transport_error(&PollingError::NonZero(
            "bad target".into()
        )));
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
}
