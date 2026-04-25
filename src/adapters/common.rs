use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, SignalSet, TaskType};

const LOG_STORM_LINE_THRESHOLD: usize = 8;

/// Waiting-for-input detection: phrase-level only. Bare "press enter"
/// matches docstrings ("press enter to continue with default..."),
/// "continue?" matches code review prose, "y/n" / "yes/no" match
/// `[y/n]` config rows. (v1.13.0 emergency tightening.) The (y/n)-
/// class permission prompts are owned by PERMISSION_PROMPT_MARKERS;
/// not duplicated here.
const WAITING_PROMPT_MARKERS: &[&str] = &[
    "needs your input",
    "waiting for input",
    "press enter to continue",
    "press enter to retry",
    "select an option:",
    "what would you like to do",
];

/// Permission-prompt detection: only fire on phrases that imply an
/// interactive ask is in progress. Bare nouns like "permission" /
/// "allow" / "approve" / "dangerous" match config-display text such
/// as Codex's `│ permissions: YOLO mode` welcome row, Claude Code's
/// `⏵⏵ bypass permissions on` keyboard hint, or `Permissions: Full
/// Access` config rows — generating tens of thousands of false-
/// positive RISK alerts per day. (v1.13.0 emergency tightening,
/// measured against real captured tails on 2026-04-25.)
const PERMISSION_PROMPT_MARKERS: &[&str] = &[
    "(y/n)",
    "[y/n]",
    "(yes/no)",
    "requires approval",
    "approve this action",
    "approve the patch",
    "approve this command",
    "do you want to",
    "press y to allow",
    "press enter to allow",
    "press enter to approve",
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

    // v1.13.0: drop the chars/lines fallback. Long-but-not-loggy outputs
    // (Claude /status, Codex welcome box, normal long-form replies) were
    // generating tens of thousands of daily Warning log_storm false
    // positives. Real log storms always show >=8 log-like lines; the
    // fallback was a premature 'lots of output' proxy that did not
    // survive contact with real CLIs.
    let log_like = lines.iter().filter(|line| is_log_like(line)).count();
    let log_storm = log_like >= LOG_STORM_LINE_THRESHOLD;

    SignalSet {
        waiting_for_input: WAITING_PROMPT_MARKERS.iter().any(|m| lower.contains(m)),
        permission_prompt: PERMISSION_PROMPT_MARKERS.iter().any(|m| lower.contains(m)),
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
        git_branch: None,
        worktree_path: None,
        reasoning_effort: None,
    }
}

pub fn is_log_like(line: &str) -> bool {
    // v1.13.0: tighten from substring-or-loose-punctuation to structural
    // patterns. The previous contract treated any line containing `:` +
    // `-`, or `[` + `]`, or any of {info, debug, warn, warning, error,
    // trace, fatal, stderr, stdout} as a substring as log-like. That
    // matched ordinary prose, markdown tables (`│ … │`), URLs, code
    // references (`foo.rs:42`), and assistant chat output containing
    // the word 'error' in any context — generating tens of thousands
    // of daily Warning log_storm false positives.
    //
    // True log-line shape requires either:
    //   (a) a bracketed log level   — `[info]`, `[error]`, etc.
    //   (b) a level word at line start — `INFO …`, `ERROR …`
    //   (c) an ISO-8601 timestamp prefix — `2026-04-25T12:34:56` or
    //       `2026-04-25 12:34:56`.
    let lower = line.to_lowercase();
    const BRACKET_LEVELS: &[&str] = &[
        "[info]",
        "[debug]",
        "[warn]",
        "[warning]",
        "[error]",
        "[trace]",
        "[fatal]",
    ];
    if BRACKET_LEVELS.iter().any(|m| lower.contains(m)) {
        return true;
    }

    let trimmed = lower.trim_start();
    if trimmed.starts_with("info ")
        || trimmed.starts_with("debug ")
        || trimmed.starts_with("warn ")
        || trimmed.starts_with("warning ")
        || trimmed.starts_with("error ")
        || trimmed.starts_with("trace ")
        || trimmed.starts_with("fatal ")
    {
        return true;
    }

    // ISO-8601 timestamp prefix: `YYYY-MM-DDTHH:MM` or `YYYY-MM-DD HH:MM`.
    let bytes = line.as_bytes();
    bytes.len() >= 16
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
        && (bytes[10] == b'T' || bytes[10] == b' ')
        && bytes[11..13].iter().all(|b| b.is_ascii_digit())
        && bytes[13] == b':'
        && bytes[14..16].iter().all(|b| b.is_ascii_digit())
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

/// Parse a token count string like "4.3k", "258K", "1.53M", or "4300"
/// into a u64. Returns `None` if the input is not a recognisable number.
/// Case-insensitive for the suffix.
pub fn parse_count_with_suffix(s: &str) -> Option<u64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (num_part, multiplier) = match trimmed.chars().last()? {
        'k' | 'K' => (&trimmed[..trimmed.len() - 1], 1_000.0_f64),
        'm' | 'M' => (&trimmed[..trimmed.len() - 1], 1_000_000.0_f64),
        _ => (trimmed, 1.0_f64),
    };
    let value: f64 = num_part.parse().ok()?;
    Some((value * multiplier).round() as u64)
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

    #[test]
    fn parse_count_with_suffix_handles_plain_integer() {
        assert_eq!(parse_count_with_suffix("4300"), Some(4300));
    }

    #[test]
    fn parse_count_with_suffix_handles_k_suffix() {
        assert_eq!(parse_count_with_suffix("4.3k"), Some(4300));
        assert_eq!(parse_count_with_suffix("258K"), Some(258_000));
    }

    #[test]
    fn parse_count_with_suffix_handles_m_suffix() {
        assert_eq!(parse_count_with_suffix("1.53M"), Some(1_530_000));
        assert_eq!(parse_count_with_suffix("20.4K"), Some(20_400));
    }

    #[test]
    fn parse_count_with_suffix_returns_none_for_garbage() {
        assert_eq!(parse_count_with_suffix(""), None);
        assert_eq!(parse_count_with_suffix("xyz"), None);
    }
}
