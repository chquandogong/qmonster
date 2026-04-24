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
}
