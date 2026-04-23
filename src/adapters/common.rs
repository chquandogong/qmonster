use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, SignalSet, TaskType};

const LOG_STORM_LINE_THRESHOLD: usize = 8;
const BIG_OUTPUT_CHARS: usize = 2200;
const LOG_STORM_LARGE_LINES: usize = 16;

const WAITING_MARKERS: &[&str] = &[
    "needs your input",
    "waiting for input",
    "press enter",
    "continue?",
    "select an option",
    "y/n",
    "yes/no",
];

const PERMISSION_MARKERS: &[&str] = &[
    "permission",
    "allow",
    "approve",
    "approval",
    "dangerous",
    "requires approval",
];

const ERROR_MARKERS: &[&str] = &[
    "traceback",
    "panic",
    "exception",
    "fatal",
    "error:",
    "failed",
];

const VERBOSE_MARKERS: &[&str] = &[
    "i'd be happy to help",
    "let me know if",
    "sure!",
    "absolutely",
    "great question",
];

const SUBAGENT_MARKERS: &[&str] = &[
    "starting subagent",
    "spawning subagent",
    "launching subagent",
    "delegating task",
    "subagent complete",
];

/// Cross-provider parser used by every adapter as a base layer. Each
/// per-provider file layers its own patterns on top and returns the
/// final `SignalSet` (r2 rule: no cross-provider logic in a provider
/// adapter).
pub fn parse_common_signals(tail: &str) -> SignalSet {
    let lower = tail.to_lowercase();
    let lines: Vec<&str> = tail.lines().collect();
    let output_chars = tail.chars().count();

    let long_lines = lines.iter().filter(|l| l.chars().count() > 100).count();
    let verbose_answer =
        VERBOSE_MARKERS.iter().any(|m| lower.contains(m)) || (lines.len() >= 10 && long_lines >= 4);

    let log_like = lines.iter().filter(|line| is_log_like(line)).count();
    let log_storm = log_like >= LOG_STORM_LINE_THRESHOLD
        || (output_chars >= BIG_OUTPUT_CHARS && lines.len() >= LOG_STORM_LARGE_LINES);

    SignalSet {
        waiting_for_input: WAITING_MARKERS.iter().any(|m| lower.contains(m)),
        permission_prompt: PERMISSION_MARKERS.iter().any(|m| lower.contains(m)),
        log_storm,
        repeated_output: false,
        verbose_answer,
        error_hint: ERROR_MARKERS.iter().any(|m| lower.contains(m)),
        subagent_hint: SUBAGENT_MARKERS.iter().any(|m| lower.contains(m)),
        output_chars,
        task_type: detect_task_type(&lower),
        context_pressure: parse_context_pressure(&lower),
        token_count: None,
        cost_usd: None,
        model_name: None,
    }
}

pub fn is_log_like(line: &str) -> bool {
    let lower = line.to_lowercase();
    const LEVELS: &[&str] = &[
        "info", "debug", "warn", "warning", "error", "trace", "fatal", "stderr", "stdout",
    ];
    let has_level = LEVELS.iter().any(|m| lower.contains(m));
    let has_timestamp = line.contains("202")
        || (line.contains(':') && line.contains('-'))
        || (line.contains('[') && line.contains(']'));
    has_level || has_timestamp
}

fn parse_context_pressure(lower: &str) -> Option<MetricValue<f32>> {
    for line in lower.lines() {
        if !(line.contains("context")
            || line.contains("window")
            || line.contains("usage")
            || line.contains("compact"))
        {
            continue;
        }
        if let Some(percent) = parse_first_percent(line) {
            return Some(MetricValue::new(percent / 100.0, SourceKind::Estimated));
        }
    }
    None
}

fn parse_first_percent(line: &str) -> Option<f32> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            let mut seen_dot = false;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || (!seen_dot && chars[i] == '.')) {
                if chars[i] == '.' {
                    seen_dot = true;
                }
                i += 1;
            }
            if i < chars.len() && chars[i] == '%' {
                let s: String = chars[start..i].iter().collect();
                if let Ok(v) = s.parse::<f32>() {
                    return Some(v);
                }
            }
        }
        i += 1;
    }
    None
}

fn detect_task_type(lower: &str) -> TaskType {
    if lower.contains("resume")
        || lower.contains("continue previous")
        || lower.contains("last session")
    {
        TaskType::SessionResume
    } else if lower.contains("review") || lower.contains("pull request") {
        TaskType::Review
    } else if lower.contains("codex exec") || lower.contains("scripted") {
        TaskType::Automation
    } else if lower.contains("summary") || lower.contains("summarize") || lower.contains("tl;dr") {
        TaskType::Summary
    } else if lower.contains("grep")
        || lower.contains("find ")
        || lower.contains("search")
        || lower.contains("symbol")
        || lower.contains("callers")
        || lower.contains("references")
    {
        TaskType::CodeExploration
    } else if lower.contains("traceback")
        || lower.contains("stack trace")
        || lower.contains("panic")
    {
        TaskType::LogTriage
    } else {
        TaskType::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_storm_triggers_on_many_log_like_lines() {
        let tail = [
            "2026-04-20T00:00:00 INFO build step 1",
            "2026-04-20T00:00:01 INFO build step 2",
            "2026-04-20T00:00:02 WARN deprecated call",
            "2026-04-20T00:00:03 ERROR connection reset",
            "2026-04-20T00:00:04 INFO retry 1",
            "2026-04-20T00:00:05 INFO retry 2",
            "2026-04-20T00:00:06 DEBUG state=x",
            "2026-04-20T00:00:07 INFO done",
            "2026-04-20T00:00:08 INFO ok",
        ]
        .join("\n");
        let set = parse_common_signals(&tail);
        assert!(set.log_storm, "expected log storm with many log-like lines");
    }

    #[test]
    fn waiting_for_input_on_prompt_text() {
        let tail = "...\nPress ENTER to continue\n";
        let set = parse_common_signals(tail);
        assert!(set.waiting_for_input);
    }

    #[test]
    fn permission_prompt_on_approval_word() {
        let tail = "This action requires approval (y/n)";
        let set = parse_common_signals(tail);
        assert!(set.permission_prompt);
    }

    #[test]
    fn error_hint_on_traceback() {
        let tail = "Traceback (most recent call last):\n  File foo.rs";
        let set = parse_common_signals(tail);
        assert!(set.error_hint);
    }

    #[test]
    fn verbose_marker_triggers_verbose_answer() {
        let tail = "I'd be happy to help with that and explain each step.";
        let set = parse_common_signals(tail);
        assert!(set.verbose_answer);
    }

    #[test]
    fn subagent_hint_on_spawning_language() {
        let tail = "Starting subagent: researcher-1\nDelegating task...";
        let set = parse_common_signals(tail);
        assert!(set.subagent_hint);
    }

    #[test]
    fn context_pressure_parsed_as_estimated() {
        let tail = "context window usage 82%";
        let set = parse_common_signals(tail);
        let m = set.context_pressure.expect("context_pressure parsed");
        assert!((m.value - 0.82).abs() < 0.01);
        assert_eq!(m.source_kind, crate::domain::origin::SourceKind::Estimated);
    }
}
