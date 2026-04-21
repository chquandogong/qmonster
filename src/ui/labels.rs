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
