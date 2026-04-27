use std::time::Duration;

use crate::tmux::types::WindowTarget;

pub(crate) const SUBMIT_KEY: &str = "C-m";
pub(crate) const KEY_SETTLE_DELAY: Duration = Duration::from_millis(80);

pub(crate) fn list_panes_args(fmt: &str, target: Option<&WindowTarget>) -> Vec<String> {
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

pub(crate) fn list_windows_args(fmt: &str) -> Vec<String> {
    vec!["list-windows".into(), "-a".into(), "-F".into(), fmt.into()]
}

pub(crate) fn current_target_args(tmux_pane: &str, fmt: &str) -> Vec<String> {
    vec![
        "display-message".into(),
        "-p".into(),
        "-t".into(),
        tmux_pane.into(),
        fmt.into(),
    ]
}

pub(crate) fn capture_tail_args(pane_id: &str, lines: usize) -> Vec<String> {
    vec![
        "capture-pane".into(),
        "-p".into(),
        "-J".into(),
        "-S".into(),
        format!("-{lines}"),
        "-t".into(),
        pane_id.into(),
    ]
}

pub(crate) fn send_keys_literal_args(pane_id: &str, text: &str) -> Vec<String> {
    vec![
        "send-keys".into(),
        "-t".into(),
        pane_id.into(),
        "-l".into(),
        text.into(),
    ]
}

pub(crate) fn send_key_args(pane_id: &str, key: &str) -> Vec<String> {
    vec!["send-keys".into(), "-t".into(), pane_id.into(), key.into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_panes_args_use_target_or_all_sessions() {
        assert_eq!(
            list_panes_args(
                "fmt",
                Some(&WindowTarget {
                    session_name: "qmonster".into(),
                    window_index: "0".into(),
                }),
            ),
            vec!["list-panes", "-t", "qmonster:0", "-F", "fmt"]
        );
        assert_eq!(
            list_panes_args("fmt", None),
            vec!["list-panes", "-a", "-F", "fmt"]
        );
    }

    #[test]
    fn list_windows_and_current_target_args_are_format_driven() {
        assert_eq!(
            list_windows_args("win-fmt"),
            vec!["list-windows", "-a", "-F", "win-fmt"]
        );
        assert_eq!(
            current_target_args("%7", "win-fmt"),
            vec!["display-message", "-p", "-t", "%7", "win-fmt"]
        );
    }

    #[test]
    fn capture_tail_args_match_polling_contract() {
        assert_eq!(
            capture_tail_args("%5", 24),
            vec!["capture-pane", "-p", "-J", "-S", "-24", "-t", "%5"]
        );
    }

    #[test]
    fn send_keys_args_keep_literal_payload_separate_from_submit_key() {
        assert_eq!(
            send_keys_literal_args("%5", "/compact"),
            vec!["send-keys", "-t", "%5", "-l", "/compact"]
        );
        assert_eq!(
            send_key_args("%5", SUBMIT_KEY),
            vec!["send-keys", "-t", "%5", SUBMIT_KEY]
        );
    }
}
