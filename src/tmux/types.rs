/// tmux format string for `list-panes -a -F …`. Eight tab-separated
/// fields. Kept in sync with `parse_list_panes_row` — any change here
/// requires changing the parser (guarded by a unit test).
pub const PANE_LIST_FORMAT: &str = "#{session_name}\\t#{window_index}\\t#{pane_id}\\t#{pane_title}\\t#{pane_current_command}\\t#{pane_current_path}\\t#{pane_active}\\t#{pane_dead}";

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
}

pub fn parse_list_panes_row(line: &str) -> Option<RawPaneSnapshot> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 8 {
        return None;
    }
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_panes_row_splits_eight_fields() {
        let line = "qwork\t1\t%42\tclaude:1:main\tclaude\t/home/a\t1\t0";
        let row = parse_list_panes_row(line).expect("parse ok");
        assert_eq!(row.session_name, "qwork");
        assert_eq!(row.window_index, "1");
        assert_eq!(row.pane_id, "%42");
        assert_eq!(row.title, "claude:1:main");
        assert_eq!(row.current_command, "claude");
        assert_eq!(row.current_path, "/home/a");
        assert!(row.active);
        assert!(!row.dead);
    }

    #[test]
    fn parse_list_panes_rejects_short_rows() {
        assert!(parse_list_panes_row("too\tfew\tfields").is_none());
    }

    #[test]
    fn pane_list_format_string_has_eight_tab_separated_fields() {
        // Guarding the tmux format string we ship. If this changes we
        // must update both the parser and the capture test.
        let fmt = PANE_LIST_FORMAT;
        let token_count = fmt.split("\\t").count();
        assert_eq!(token_count, 8, "format = {fmt}");
    }
}
