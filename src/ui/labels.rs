use crate::domain::origin::SourceKind;

pub fn source_kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::ProviderOfficial => "Official",
        SourceKind::ProjectCanonical => "Qmonster",
        SourceKind::Heuristic => "Heur",
        SourceKind::Estimated => "Estimate",
    }
}

pub fn source_kind_legend() -> [&'static str; 4] {
    [
        "[Official] provider documentation or vendor defaults",
        "[Qmonster] project rule or canonical project guidance",
        "[Heur] parser or policy heuristic; verify before acting",
        "[Estimate] inferred metric, useful for trend not certainty",
    ]
}

pub fn signal_legend() -> [&'static str; 7] {
    [
        "waiting for input: the agent is paused and needs operator input",
        "approval needed: a permission prompt is blocking progress",
        "log storm: output volume is high enough to hide useful detail",
        "repeated output: the same content is recurring without progress",
        "verbose output: the pane is spending budget on long-form output",
        "error hint: stderr or traceback-like signals were detected",
        "subagent activity: the pane appears to be coordinating other agents",
    ]
}

/// Slice 3 housekeeping: bound a long operator-supplied string to a
/// maximum visible width. Operator-supplied paths (`worktree_path`,
/// `additional_directories`) and branch names can be much wider than
/// the pane card; pre-truncating at the formatter keeps the metric
/// row from pushing the badge stack off-screen.
///
/// `max` counts characters (not bytes), so multi-byte content is
/// handled correctly. When `max < 3`, the function returns the first
/// `max` characters without an ellipsis (no room for `...`).
/// Otherwise the head is `max - 1` characters followed by a single
/// `…` glyph (one printable character, mirrors the visual budget of
/// the original character it replaces).
pub fn ellipsize(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    if max < 3 {
        return chars.iter().take(max).collect();
    }
    let mut out: String = chars.iter().take(max - 1).collect();
    out.push('…');
    out
}

/// Human-friendly token count: `999 -> "999"`, `4_300 -> "4.3K"`,
/// `1_530_000 -> "1.53M"`. Used by metric rows so raw u64 token counts
/// do not dominate the UI.
pub fn format_count_with_suffix(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_count_with_suffix_handles_plain_integers() {
        assert_eq!(format_count_with_suffix(0), "0");
        assert_eq!(format_count_with_suffix(999), "999");
    }

    #[test]
    fn format_count_with_suffix_handles_k_boundary() {
        assert_eq!(format_count_with_suffix(1_000), "1.0K");
        assert_eq!(format_count_with_suffix(4_300), "4.3K");
        assert_eq!(format_count_with_suffix(999_999), "1000.0K");
    }

    #[test]
    fn format_count_with_suffix_handles_m_boundary() {
        assert_eq!(format_count_with_suffix(1_000_000), "1.00M");
        assert_eq!(format_count_with_suffix(1_530_000), "1.53M");
    }

    #[test]
    fn ellipsize_returns_input_when_within_budget() {
        assert_eq!(ellipsize("~/Qmonster", 50), "~/Qmonster");
        assert_eq!(ellipsize("main", 50), "main");
    }

    #[test]
    fn ellipsize_truncates_with_single_ellipsis_glyph() {
        // 50-char path becomes 50 visible characters with the last
        // one replaced by `…` so the stack does not overflow the
        // badge column.
        let long = "/home/operator/very/long/nested/project/path/here/and/more";
        let out = ellipsize(long, 30);
        assert_eq!(out.chars().count(), 30);
        assert!(out.ends_with('…'));
        assert!(out.starts_with("/home/operator/"));
    }

    #[test]
    fn ellipsize_handles_multi_byte_characters_by_char_count() {
        // Korean / Chinese path components are multi-byte. The
        // budget is in CHARACTERS, not BYTES, so the truncation
        // does not cut a glyph in half.
        let s = "/home/operator/한국어경로/매우긴경로이름/끝";
        let out = ellipsize(s, 12);
        assert_eq!(out.chars().count(), 12);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn ellipsize_under_minimum_budget_returns_head_without_ellipsis() {
        // 2 chars is below the 3-char minimum needed to show one
        // visible char + `…`; return just 2 chars to stay honest.
        assert_eq!(ellipsize("abcdef", 2), "ab");
        assert_eq!(ellipsize("abcdef", 0), "");
    }
}
