use crate::tmux::types::{WindowTarget, parse_list_windows_row};

pub(crate) fn parse_window_targets<I, S>(lines: I) -> Vec<WindowTarget>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut targets: Vec<WindowTarget> = lines
        .into_iter()
        .filter_map(|line| parse_list_windows_row(line.as_ref()))
        .collect();
    targets.sort();
    targets.dedup();
    targets
}

pub(crate) fn parse_current_target<I, S>(lines: I) -> Option<WindowTarget>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    lines
        .into_iter()
        .next()
        .and_then(|line| parse_list_windows_row(line.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_window_targets_sorts_dedups_and_skips_invalid_rows() {
        let targets = parse_window_targets([
            "qmonster\t1",
            "too-few-fields",
            "mission-spec\t0",
            "qmonster\t1",
        ]);

        assert_eq!(
            targets,
            vec![
                WindowTarget {
                    session_name: "mission-spec".into(),
                    window_index: "0".into(),
                },
                WindowTarget {
                    session_name: "qmonster".into(),
                    window_index: "1".into(),
                },
            ]
        );
    }

    #[test]
    fn parse_current_target_preserves_first_row_only_contract() {
        let target = parse_current_target(["bad", "qmonster\t1"]);
        assert_eq!(target, None);

        let target = parse_current_target(["qmonster\t1", "mission-spec\t0"]);
        assert_eq!(
            target,
            Some(WindowTarget {
                session_name: "qmonster".into(),
                window_index: "1".into(),
            })
        );
    }
}
