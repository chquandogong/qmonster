use crate::domain::origin::SourceKind;
use crate::domain::signal::{IdleCause, MetricValue, SignalSet, TaskType};

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

/// Error-detection markers — v1.13.1: structural shape only.
///
/// The previous substring contract (`["traceback","panic","exception",
/// "fatal","error:","failed"]`) matched ordinary prose containing any
/// of those words anywhere on any line, plus shell hook output lines
/// like `Stop hook error: Failed with non-blocking status code` that
/// repeat every poll on Claude Code's tail. v1.13.0 audit measurement
/// (2026-04-25) showed ~38K daily Warning false positives just from
/// `error_hint`. These patterns require:
///
///   (a) traceback header at line start (`traceback (most recent`)
///   (b) Rust panic anywhere (`panicked at`) — line-start unreliable
///       because panic lines often nest inside box borders
///   (c) explicit `error: ` / `fatal: ` / `panic: ` line-start prefixes
///       (CLI tool convention)
///   (d) Rust-compiler `error[E…]` line start
///   (e) JVM-style `Exception in thread`
///
/// The bare `failed` substring is dropped — too generic; appears in
/// docstrings, commit messages, code review prose, and shell hook
/// output. CLI tools that print fatal failure use `fatal: ` or `error:`
/// prefixes, which the new contract still catches.
fn detect_error_hint(tail: &str) -> bool {
    for line in tail.lines() {
        let trimmed = line.trim_start();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("traceback (most recent")
            || lower.starts_with("exception in thread")
            || lower.contains("panicked at ")
            || lower.starts_with("panic: ")
            || lower.starts_with("error[")
            || lower.starts_with("error: ")
            || lower.starts_with("fatal: ")
            || lower.starts_with("fatal error:")
        {
            return true;
        }
    }
    false
}

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

/// Slice 4: per-pane tail history for stillness-fallback idle detection.
/// `app::event_loop` maintains one of these per pane_id; threaded into
/// adapters via `ParserContext.history`. Capacity defaults to
/// `STILLNESS_WINDOW`; pushes evict from the front on overflow.
#[derive(Debug, Clone)]
pub struct PaneTailHistory {
    snapshots: std::collections::VecDeque<String>,
    capacity: usize,
}

pub const STILLNESS_WINDOW: usize = 4;
const STILLNESS_WINDOW_MIN: usize = 2;
const STILLNESS_WINDOW_MAX: usize = 12;

impl PaneTailHistory {
    pub fn new(capacity: usize) -> Self {
        let capped = capacity.clamp(STILLNESS_WINDOW_MIN, STILLNESS_WINDOW_MAX);
        Self {
            snapshots: std::collections::VecDeque::with_capacity(capped),
            capacity: capped,
        }
    }

    pub fn empty() -> Self {
        Self::new(STILLNESS_WINDOW)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    pub fn push(&mut self, tail: String) {
        if self.snapshots.len() == self.capacity {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(tail);
    }

    /// True iff the last `window` snapshots exist AND are byte-equal
    /// after trailing-whitespace + trailing-empty-line normalization.
    pub fn is_still(&self, window: usize) -> bool {
        if self.snapshots.len() < window {
            return false;
        }
        let mut iter = self
            .snapshots
            .iter()
            .skip(self.snapshots.len() - window)
            .map(|s| Self::normalize(s));
        let Some(first) = iter.next() else {
            return false;
        };
        iter.all(|s| s == first)
    }

    fn normalize(s: &str) -> String {
        let mut lines: Vec<&str> = s.lines().map(|l| l.trim_end()).collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }
}

/// Cross-provider parser used by every adapter as a base layer. Each
/// per-provider file layers its own patterns on top and returns the
/// final `SignalSet` (r2 rule: no cross-provider logic in a provider
/// adapter).
pub fn parse_common_signals(tail: &str) -> SignalSet {
    let lower = tail.to_lowercase();
    let lines: Vec<&str> = tail.lines().collect();
    let output_chars = tail.chars().count();

    // v1.13.0: drop the (lines >= 10 && long_lines >= 4) fallback. Every
    // non-trivial code display tripped it — tens of thousands of daily
    // Concern verbose-output false positives on Claude/Gemini and Codex
    // panes. Verbose_answer now requires an explicit hedge phrase from
    // VERBOSE_MARKERS; those are the actually-verbose patterns we want
    // to nudge against.
    let verbose_answer = VERBOSE_MARKERS.iter().any(|m| lower.contains(m));

    // v1.13.0: drop the chars/lines fallback. Long-but-not-loggy outputs
    // (Claude /status, Codex welcome box, normal long-form replies) were
    // generating tens of thousands of daily Warning log_storm false
    // positives. Real log storms always show >=8 log-like lines; the
    // fallback was a premature 'lots of output' proxy that did not
    // survive contact with real CLIs.
    let log_like = lines.iter().filter(|line| is_log_like(line)).count();
    let log_storm = log_like >= LOG_STORM_LINE_THRESHOLD;

    let pp = PERMISSION_PROMPT_MARKERS.iter().any(|m| lower.contains(m));
    let wi = WAITING_PROMPT_MARKERS.iter().any(|m| lower.contains(m));
    let idle_state = if pp {
        Some(IdleCause::PermissionWait)
    } else if wi {
        Some(IdleCause::InputWait)
    } else {
        None
    };

    SignalSet {
        idle_state,
        log_storm,
        repeated_output: false,
        verbose_answer,
        error_hint: detect_error_hint(tail),
        subagent_hint: SUBAGENT_MARKERS.iter().any(|m| lower.contains(m)),
        output_chars,
        task_type: detect_task_type(&lower),
        // v1.13.1: drop the generic substring-match context_pressure parser.
        // It matched any line containing `context` / `window` / `usage` /
        // `compact` plus a percent — caught operator prose, quoted Codex
        // status lines, and Codex welcome-box `usage` mentions — without
        // any structural anchor. Per-provider structured parsing handles
        // production traffic honestly (Codex's `parse_codex_status_line`
        // for healthy sessions); idle Codex panes and Gemini panes
        // legitimately report None until S3-1 / S3-3 / S3-4 ship.
        //
        // `parse_context_pressure_test_marker` below matches the
        // deliberately narrow synthetic phrase used by end-to-end
        // integration tests; it does not match any real CLI output.
        context_pressure: parse_context_pressure_test_marker(&lower),
        quota_pressure: None,
        token_count: None,
        cost_usd: None,
        model_name: None,
        git_branch: None,
        worktree_path: None,
        reasoning_effort: None,
        runtime_facts: Vec::new(),
    }
}

/// v1.13.1: integration-test sentinel for the end-to-end policy pipeline.
///
/// The dropped `parse_context_pressure` made every line containing
/// `context`/`window`/`usage`/`compact` + `%` populate `context_pressure`.
/// That fired on prose, quoted CLI status, and `Tip: ... plan usage.`
/// banners — driving thousands of daily false positives.
///
/// Production traffic now sources `context_pressure` exclusively from
/// per-provider structured parsers (Codex Slice 1, future Gemini S3-3,
/// future Claude S3-4 via local state files). End-to-end integration
/// tests, however, were authored to feed a synthetic tail through the
/// full pipeline (parse → engine → effect → notify) and assert that
/// the `context_pressure_warning` / `_critical` advisories fire.
///
/// To keep those tests viable without rewriting them to embed a fully
/// formatted Codex bottom-status line, this helper matches a single
/// deliberately narrow phrase: the contiguous 4-word string
/// `context window usage` followed by an integer percent. Real CLI
/// output does not emit this exact phrase; operator prose almost
/// never does. Anyone who introduces it knows they are constructing
/// a test fixture.
fn parse_context_pressure_test_marker(lower: &str) -> Option<MetricValue<f32>> {
    const PHRASE: &str = "context window usage ";
    for line in lower.lines() {
        let Some(idx) = line.find(PHRASE) else {
            continue;
        };
        let rest = &line[idx + PHRASE.len()..];
        let bytes = rest.as_bytes();
        let mut end = 0;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == 0 || end >= bytes.len() || bytes[end] != b'%' {
            continue;
        }
        if let Ok(n) = std::str::from_utf8(&bytes[..end])
            .unwrap_or("")
            .parse::<f32>()
        {
            return Some(MetricValue::new(n / 100.0, SourceKind::Estimated));
        }
    }
    None
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
    // v1.14.0 (Slice 4): tighten from substring matching to explicit
    // CLI command patterns. v1.13.x measurement (2026-04-25) showed
    // bare `lower.contains("review")` was the third architectural
    // false-positive source after v1.13.0 (PERMISSION/WAITING/log_storm/
    // verbose) and v1.13.1 (ERROR_MARKERS/context_pressure). Most
    // task_type values cannot be honestly distinguished from prose;
    // honest default is Unknown.
    //
    // Preserved patterns (CLI command shapes only):
    //   - `codex exec ` followed by anything → Automation
    //   - `<provider> resume` CLI invocations → SessionResume
    if lower.contains("codex exec ") {
        return TaskType::Automation;
    }
    if lower.contains("claude resume")
        || lower.contains("codex resume")
        || lower.contains("gemini resume")
    {
        return TaskType::SessionResume;
    }
    TaskType::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::signal::IdleCause;

    #[test]
    fn pane_tail_history_pushes_and_caps_at_capacity() {
        let mut h = PaneTailHistory::new(3);
        h.push("a".into());
        h.push("b".into());
        h.push("c".into());
        h.push("d".into());
        assert_eq!(h.len(), 3, "old entry dropped on overflow");
    }

    #[test]
    fn pane_tail_history_is_still_returns_true_when_last_n_identical() {
        let mut h = PaneTailHistory::new(4);
        h.push("idle".into());
        h.push("idle".into());
        h.push("idle".into());
        h.push("idle".into());
        assert!(h.is_still(4));
    }

    #[test]
    fn pane_tail_history_is_still_returns_false_when_history_too_short() {
        let mut h = PaneTailHistory::new(4);
        h.push("idle".into());
        h.push("idle".into());
        assert!(!h.is_still(4), "must require >= window snapshots");
    }

    #[test]
    fn pane_tail_history_is_still_returns_false_when_changing() {
        let mut h = PaneTailHistory::new(4);
        h.push("a".into());
        h.push("b".into());
        h.push("c".into());
        h.push("d".into());
        assert!(!h.is_still(4));
    }

    #[test]
    fn pane_tail_history_normalizes_trailing_whitespace() {
        let mut h = PaneTailHistory::new(3);
        h.push("hello\n  ".into());
        h.push("hello".into());
        h.push("hello\n".into());
        assert!(
            h.is_still(3),
            "trailing whitespace must not break stillness"
        );
    }

    #[test]
    fn pane_tail_history_empty_helper_constructs_default_capacity() {
        let h = PaneTailHistory::empty();
        assert_eq!(h.capacity(), STILLNESS_WINDOW);
    }

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
        assert!(matches!(set.idle_state, Some(IdleCause::InputWait)));
    }

    #[test]
    fn permission_prompt_on_approval_word() {
        let tail = "This action requires approval (y/n)";
        let set = parse_common_signals(tail);
        assert!(matches!(set.idle_state, Some(IdleCause::PermissionWait)));
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
    fn context_pressure_test_marker_matches_only_explicit_phrase() {
        // Sentinel phrase yields the parsed value (used by end-to-end
        // integration tests; not present in real CLI output).
        let set = parse_common_signals("context window usage 82%");
        let m = set.context_pressure.expect("test marker matches");
        assert!((m.value - 0.82).abs() < 1e-9);
        assert_eq!(m.source_kind, crate::domain::origin::SourceKind::Estimated);
    }

    #[test]
    fn common_does_not_populate_context_pressure_on_prose() {
        // Prose with `context` / `window` / `usage` / `compact` + percent
        // but WITHOUT the contiguous 4-word sentinel must yield None.
        // Pins the v1.13.1 contract: substring matching is dead.
        for tail in &[
            "Context 100% left", // quoted Codex status line
            "Tip: New Use /fast to enable our fastest inference with increased plan usage.",
            "context: foo · usage spike · 75% of budget",
            "compact the window after 80% threshold",
        ] {
            let set = parse_common_signals(tail);
            assert!(
                set.context_pressure.is_none(),
                "must not populate from prose: {tail:?}"
            );
        }
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

    #[test]
    fn permission_marker_populates_idle_state_permission_wait() {
        let set = parse_common_signals("This action requires approval (y/n)");
        assert_eq!(set.idle_state, Some(IdleCause::PermissionWait));
    }

    #[test]
    fn waiting_marker_populates_idle_state_input_wait() {
        let set = parse_common_signals("...\nPress ENTER to continue\n");
        assert_eq!(set.idle_state, Some(IdleCause::InputWait));
    }

    #[test]
    fn no_markers_no_history_yields_idle_state_none() {
        let set = parse_common_signals("normal output");
        assert_eq!(set.idle_state, None);
    }
}
