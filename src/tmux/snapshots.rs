use crate::tmux::types::{RawPaneSnapshot, parse_list_panes_row};

pub(crate) fn hydrate_pane_snapshots<I, S, F>(lines: I, mut capture_tail: F) -> Vec<RawPaneSnapshot>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: FnMut(&str) -> Option<String>,
{
    let mut rows = Vec::new();
    for line in lines {
        if let Some(mut snap) = parse_list_panes_row(line.as_ref()) {
            if let Some(tail) = capture_tail(&snap.pane_id) {
                snap.tail = tail;
            }
            rows.push(snap);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hydrates_valid_pane_rows_with_captured_tail() {
        let rows = hydrate_pane_snapshots(
            [
                "qmonster\t0\t%1\tclaude:1:main\tclaude\t/home/q\t1\t0",
                "too\tfew",
                "qmonster\t0\t%2\tcodex:1:review\tcodex\t/home/q\t0\t0",
            ],
            |pane_id| Some(format!("tail for {pane_id}")),
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].pane_id, "%1");
        assert_eq!(rows[0].tail, "tail for %1");
        assert_eq!(rows[1].pane_id, "%2");
        assert_eq!(rows[1].tail, "tail for %2");
    }

    #[test]
    fn leaves_tail_empty_when_capture_fails() {
        let rows = hydrate_pane_snapshots(
            ["qmonster\t0\t%1\tclaude:1:main\tclaude\t/home/q\t1\t0"],
            |_| None,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tail, "");
    }
}
