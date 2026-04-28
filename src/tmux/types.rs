/// tmux format string for `list-panes -a -F …`. Nine tab-separated
/// fields. Kept in sync with `parse_list_panes_row` — any change here
/// requires changing the parser (guarded by a unit test).
pub const PANE_LIST_FORMAT: &str = "#{session_name}\\t#{window_index}\\t#{pane_id}\\t#{pane_title}\\t#{pane_current_command}\\t#{pane_current_path}\\t#{pane_active}\\t#{pane_dead}\\t#{pane_pid}";
pub const WINDOW_LIST_FORMAT: &str = "#{session_name}\\t#{window_index}";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowTarget {
    pub session_name: String,
    pub window_index: String,
}

impl WindowTarget {
    pub fn label(&self) -> String {
        format!("{}:{}", self.session_name, self.window_index)
    }
}

/// One row of `tmux list-panes` output. Raw in the sense that no
/// provider/role inference has happened yet (r2 boundary: `tmux/`
/// knows nothing about providers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPaneSnapshot {
    pub session_name: String,
    pub window_index: String,
    pub pane_id: String,
    pub title: String,
    pub current_command: String,
    pub current_path: String,
    pub active: bool,
    pub dead: bool,
    pub tail: String,
    /// Phase F F-1: tmux `#{pane_pid}` — the foreground shell PID.
    /// `None` when the row came from a legacy 8-field fixture or when
    /// tmux emitted a non-integer value. Consumers that need RSS for
    /// the actual AI CLI must walk descendants from this PID via
    /// `adapters::process_memory::read_descendant_rss_mb`.
    pub pane_pid: Option<u32>,
}

pub fn parse_list_panes_row(line: &str) -> Option<RawPaneSnapshot> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 8 {
        return None;
    }
    let pane_pid = parts.get(8).and_then(|s| s.trim().parse::<u32>().ok());
    Some(RawPaneSnapshot {
        session_name: parts[0].trim().to_string(),
        window_index: parts[1].trim().to_string(),
        pane_id: parts[2].trim().to_string(),
        title: parts[3].trim().to_string(),
        current_command: parts[4].trim().to_string(),
        current_path: parts[5].trim().to_string(),
        active: parts[6].trim() == "1",
        dead: parts[7].trim() == "1",
        tail: String::new(),
        pane_pid,
    })
}

pub fn parse_list_windows_row(line: &str) -> Option<WindowTarget> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 2 {
        return None;
    }
    Some(WindowTarget {
        session_name: parts[0].trim().to_string(),
        window_index: parts[1].trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_panes_row_splits_nine_fields_including_pid() {
        let line = "qwork\t1\t%42\tclaude:1:main\tclaude\t/home/a\t1\t0\t12345";
        let row = parse_list_panes_row(line).expect("parse ok");
        assert_eq!(row.session_name, "qwork");
        assert_eq!(row.window_index, "1");
        assert_eq!(row.pane_id, "%42");
        assert_eq!(row.title, "claude:1:main");
        assert_eq!(row.current_command, "claude");
        assert_eq!(row.current_path, "/home/a");
        assert!(row.active);
        assert!(!row.dead);
        assert_eq!(row.pane_pid, Some(12345));
    }

    #[test]
    fn parse_list_panes_row_tolerates_missing_pane_pid_legacy_eight_fields() {
        // Pre-Phase-F snapshots in saved fixtures or third-party callers
        // that hand-build the format string still parse, with pane_pid None.
        let line = "qwork\t1\t%42\tclaude:1:main\tclaude\t/home/a\t1\t0";
        let row = parse_list_panes_row(line).expect("parse ok");
        assert_eq!(row.pane_pid, None);
    }

    #[test]
    fn parse_list_panes_row_treats_unparseable_pid_as_none() {
        // tmux emits an integer; a corrupted snapshot or surprise format
        // change MUST NOT panic.
        let line = "qwork\t1\t%42\tclaude:1:main\tclaude\t/home/a\t1\t0\tnot-a-pid";
        let row = parse_list_panes_row(line).expect("parse ok");
        assert_eq!(row.pane_pid, None);
    }

    #[test]
    fn parse_list_panes_row_treats_empty_pid_field_as_none() {
        // tmux can emit a trailing tab when #{pane_pid} is unresolved
        // (e.g. mid-spawn). Empty must degrade to None, not "0".
        let line = "qwork\t1\t%42\tclaude:1:main\tclaude\t/home/a\t1\t0\t";
        let row = parse_list_panes_row(line).expect("parse ok");
        assert_eq!(row.pane_pid, None);
    }

    #[test]
    fn pane_list_format_string_has_nine_tab_separated_fields() {
        let fmt = PANE_LIST_FORMAT;
        let token_count = fmt.split("\\t").count();
        assert_eq!(token_count, 9, "format = {fmt}");
    }

    #[test]
    fn parse_list_panes_rejects_short_rows() {
        assert!(parse_list_panes_row("too\tfew\tfields").is_none());
    }

    #[test]
    fn parse_list_windows_row_extracts_session_and_window() {
        let row = parse_list_windows_row("qwork\t1").expect("parse ok");
        assert_eq!(row.session_name, "qwork");
        assert_eq!(row.window_index, "1");
        assert_eq!(row.label(), "qwork:1");
    }

    #[test]
    fn window_list_format_has_two_fields() {
        let token_count = WINDOW_LIST_FORMAT.split("\\t").count();
        assert_eq!(token_count, 2);
    }
}
