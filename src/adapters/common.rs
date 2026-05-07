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
/// Returns a short label naming the matched pattern class, or `None`
/// when the tail carries no recognised error marker. Used by adapters
/// to populate `signals.error_hint` (`is_some()`) and
/// `signals.error_hint_kind` (label) in lockstep, and by the
/// `ErrorBurst` anomaly detector (`src/policy/rules/anomaly.rs`) so its
/// evidence row tells the operator *what kind* of error spiked
/// instead of just "error rate ≥ threshold". First match wins, so a
/// line that matches both `error: ` and `error[E…]` resolves as
/// `rust_compiler_error`.
///
/// Labels are stable identifiers (snake_case, ASCII-only) — they
/// surface in audit-event summaries and `n` overlay rows, so renaming
/// any of them is a contract change.
pub fn classify_error_hint(tail: &str) -> Option<&'static str> {
    for line in tail.lines() {
        let trimmed = line.trim_start();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("traceback (most recent") {
            return Some("traceback");
        }
        if lower.starts_with("exception in thread") {
            return Some("jvm_exception");
        }
        if lower.contains("panicked at ") || lower.starts_with("panic: ") {
            return Some("rust_panic");
        }
        if lower.starts_with("error[") {
            return Some("rust_compiler_error");
        }
        if lower.starts_with("error: ") {
            return Some("error_prefix");
        }
        if lower.starts_with("fatal: ") || lower.starts_with("fatal error:") {
            return Some("fatal_prefix");
        }
    }
    None
}

const VERBOSE_MARKERS: &[&str] = &[
    "i'd be happy to help",
    "let me know if",
    "sure!",
    "absolutely",
    "great question",
];

/// Phrase-level markers for "a subagent is being launched" — narrow
/// enough to avoid the v1.13.0 false-positive class. Matched
/// case-insensitively against the lowercased tail.
///
/// `● task(` (Phase D D3-A v1.19.0) catches Claude Code's Task tool
/// tail signature — the universal "● <Tool>(args)" rendering with
/// `Task` as the tool name. That form spawns a separate sub-agent
/// LLM call, which is exactly what this signal cares about. Bare
/// `task` or `Task <number>` (e.g. `Task 1 — capture fixtures` from
/// a TODO list) never matches because the open-paren disambiguates
/// tool calls from prose.
///
/// Codex and Gemini have no equivalent sub-agent surface today —
/// their multi-step tools (`update_plan`, `apply_patch`, etc.) run
/// inside the same agent context — so no per-provider markers are
/// added beyond the cross-provider phrase set. See
/// `docs/ai/ARCHITECTURE.md` "Deferred for later phases" for the
/// permanent honesty note: per-subagent token attribution is
/// blocked on provider data, not on detection.
const SUBAGENT_MARKERS: &[&str] = &[
    "starting subagent",
    "spawning subagent",
    "launching subagent",
    "delegating task",
    "subagent complete",
    "● task(",
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

    pub fn changed_within_capacity(&self) -> bool {
        let window = self.capacity.min(self.snapshots.len());
        window >= 2 && !self.is_still(window)
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

    let error_hint_kind = classify_error_hint(tail);
    SignalSet {
        idle_state,
        log_storm,
        repeated_output: false,
        verbose_answer,
        error_hint: error_hint_kind.is_some(),
        error_hint_kind,
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
        quota_5h_pressure: None,
        quota_weekly_pressure: None,
        quota_5h_resets_at: None,
        quota_weekly_resets_at: None,
        token_count: None,
        input_tokens: None,
        output_tokens: None,
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        cache_hit_ratio: None,
        cost_usd: None,
        model_name: None,
        git_branch: None,
        worktree_path: None,
        reasoning_effort: None,
        process_memory_mb: None,
        agent_memory_bytes: None,
        runtime_facts: Vec::new(),
        active_files: parse_active_files(tail),
    }
}

/// Phase F F-8 (multi-pane orchestration): extract recently-touched
/// file paths from provider tool-call markers in the tail. Designed to
/// run alongside the cross-provider markers in `parse_common_signals`
/// so any provider whose tail uses Claude-Code-style `● <Tool>(...)`
/// rendering gets file-level edit detection for free; per-provider
/// adapters layer their own markers on top (e.g. Codex
/// `*** Update File:` apply_patch blocks).
///
/// Detection rules:
///   - line starts (after trim) with `● Edit(`, `● Write(`,
///     `● MultiEdit(`, or `● Update(` (Claude Code tool render);
///   - the first quoted/unquoted token inside the parens that looks
///     like a path (`.` / `/` present, no spaces, length ≥ 2) is
///     captured as the active-file entry;
///   - up to `MAX_ACTIVE_FILES` distinct paths are returned in
///     most-recent-first order (latest line wins).
///
/// Honesty rule: no path is fabricated. If the parens are empty or
/// contain something we cannot confidently parse as a path, the line
/// is skipped. Tail-derived paths are inherently `Heuristic` —
/// callers should not promote them to `ProviderOfficial`.
pub(crate) fn parse_active_files(tail: &str) -> Vec<String> {
    const MAX_ACTIVE_FILES: usize = 8;
    // Tool markers Claude Code renders for filesystem-touching calls.
    // Codex apply_patch (`*** Update File: …`) is parsed inside the
    // Codex adapter where the surface lives — keep this helper
    // cross-provider and conservative.
    const FILE_TOOL_MARKERS: &[&str] = &[
        "● Edit(",
        "● Write(",
        "● MultiEdit(",
        "● Update(",
        "● NotebookEdit(",
    ];

    let mut out: Vec<String> = Vec::new();
    // Walk lines bottom-to-top so the most recent edit lands first.
    for line in tail.lines().rev() {
        let trimmed = line.trim_start();
        let Some(open) = FILE_TOOL_MARKERS
            .iter()
            .find_map(|m| trimmed.strip_prefix(m))
        else {
            continue;
        };
        let Some(path) = extract_first_path_from_args(open) else {
            continue;
        };
        if !out.iter().any(|p| p == &path) {
            out.push(path);
            if out.len() >= MAX_ACTIVE_FILES {
                break;
            }
        }
    }
    out
}

/// Extract the first path-like token from a tool-call argument string.
/// Handles three Claude-Code argument shapes seen in real captures:
///   - `"src/foo.rs"` (bare quoted path, used by Edit/Write)
///   - `file: "src/foo.rs"` (named-arg form)
///   - `src/foo.rs)` (occasional unquoted, single-arg form)
fn extract_first_path_from_args(args: &str) -> Option<String> {
    // Try double-quoted path first — the most reliable shape and the
    // only one that survives paths with shell metacharacters.
    if let Some(start) = args.find('"')
        && let Some(rel) = args[start + 1..].find('"')
    {
        let candidate = &args[start + 1..start + 1 + rel];
        if looks_like_path(candidate) {
            return Some(candidate.to_string());
        }
    }
    // Fallback: bare token up to a comma, paren, or whitespace.
    let mut end = 0;
    let bytes = args.as_bytes();
    while end < bytes.len() {
        let b = bytes[end];
        if b == b',' || b == b')' || (b as char).is_whitespace() {
            break;
        }
        end += 1;
    }
    let candidate = &args[..end];
    if looks_like_path(candidate) {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn looks_like_path(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 2 {
        return false;
    }
    // Reject obvious non-paths: shell flags, JSON keys, raw numbers.
    if s.starts_with('-') || s.contains('=') {
        return false;
    }
    // A path must have a directory separator OR a file extension.
    let has_sep = s.contains('/');
    let has_ext = s.rsplit_once('.').is_some_and(|(_, ext)| {
        !ext.is_empty() && ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric())
    });
    has_sep || has_ext
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
    fn classify_error_hint_returns_label_for_each_pattern_class() {
        let cases = [
            (
                "Traceback (most recent call last):\n  File foo.rs",
                Some("traceback"),
            ),
            (
                "Exception in thread \"main\" java.lang.NullPointerException",
                Some("jvm_exception"),
            ),
            (
                "thread 'main' panicked at src/foo.rs:12:9:",
                Some("rust_panic"),
            ),
            ("panic: assertion failed", Some("rust_panic")),
            (
                "error[E0277]: the trait bound `T: Foo` is not satisfied",
                Some("rust_compiler_error"),
            ),
            (
                "error: failed to read file `Cargo.toml`",
                Some("error_prefix"),
            ),
            ("fatal: not a git repository", Some("fatal_prefix")),
            (
                "fatal error: 'stdio.h' file not found",
                Some("fatal_prefix"),
            ),
            ("just regular prose, nothing to see here", None),
            ("note: this had error prefix mid-line, won't match", None),
        ];
        for (tail, expected) in cases {
            assert_eq!(
                classify_error_hint(tail),
                expected,
                "tail = {tail:?} expected {expected:?}",
            );
        }
    }

    #[test]
    fn parse_common_signals_keeps_error_hint_bool_and_kind_in_lockstep() {
        // signals.error_hint and signals.error_hint_kind are populated
        // from the same classify_error_hint() call. This contract test
        // guards the invariant so a future refactor that splits the two
        // sources cannot let one drift past the other.
        for tail in [
            "Traceback (most recent call last):",
            "panic: assertion failed",
            "error[E0277]: bound failure",
            "error: missing operand",
            "fatal: bad reference",
            "ordinary prose with no markers",
        ] {
            let set = parse_common_signals(tail);
            assert_eq!(
                set.error_hint,
                set.error_hint_kind.is_some(),
                "lockstep broken for tail = {tail:?}",
            );
            assert_eq!(
                set.error_hint_kind,
                classify_error_hint(tail),
                "kind must match the same classifier the adapter calls",
            );
        }
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

    // -----------------------------------------------------------------
    // Phase D D3-A (v1.19.0) — Claude Task() tool detection
    // -----------------------------------------------------------------

    #[test]
    fn subagent_hint_fires_on_claude_task_tool_call() {
        // Real Claude Code tail signature for the Task subagent tool.
        // The bullet + `Task(` pair is what Claude Code prints when an
        // operator's main agent dispatches a sub-agent.
        let tail = "● Task(description, prompt, general-purpose)\n  ⎿ working...";
        let set = parse_common_signals(tail);
        assert!(
            set.subagent_hint,
            "● Task(...) is the canonical Claude subagent tail marker"
        );
    }

    #[test]
    fn subagent_hint_does_not_fire_on_prose_task_words() {
        // TODO-list / plan prose like `Task 1 — Capture real-world
        // fixtures` looked like a subagent under any bare-word match.
        // The v1.19.0 pattern requires an open paren after `task`, so
        // this kind of prose stays silent.
        let tail = "Task 0 completed\n  Task 1 — Capture real-world fixtures\n  Task 2 — Failing regression tests";
        let set = parse_common_signals(tail);
        assert!(
            !set.subagent_hint,
            "TODO-list prose with `Task <n>` must not be treated as a subagent"
        );
    }

    #[test]
    fn subagent_hint_does_not_fire_on_source_code_task_identifier() {
        // Rust source code that defines or uses a variable / type
        // named `task` is ordinary code, not a subagent dispatch.
        let tail = "let task = spawn(async move { work().await });\nlet result = task.await?;";
        let set = parse_common_signals(tail);
        assert!(
            !set.subagent_hint,
            "source-code `task` identifier must not be treated as a subagent"
        );
    }

    #[test]
    fn subagent_hint_does_not_fire_on_other_claude_tool_calls() {
        // Other Claude Code tool calls (`● Bash(...)`, `● Read(...)`,
        // `● Edit(...)`) run inside the main agent, not as sub-agents.
        // They share the bullet prefix but not the `Task` name, so
        // the marker must not match.
        let tail = "● Bash(git status)\n● Read(/etc/hosts)\n● Edit(file: \"foo.rs\")";
        let set = parse_common_signals(tail);
        assert!(
            !set.subagent_hint,
            "ordinary Claude tool calls (Bash/Read/Edit/...) must not be treated as subagents"
        );
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

    // -----------------------------------------------------------------
    // Phase F F-8 — active-files parser
    // -----------------------------------------------------------------

    #[test]
    fn active_files_extracts_quoted_edit_path() {
        let tail = "● Edit(file: \"src/foo.rs\")\nsome chatter\n";
        let set = parse_common_signals(tail);
        assert_eq!(set.active_files, vec!["src/foo.rs".to_string()]);
    }

    #[test]
    fn active_files_extracts_write_and_multiedit_paths() {
        let tail = "● Write(file: \"docs/x.md\")\n● MultiEdit(file: \"src/lib.rs\")\n";
        let set = parse_common_signals(tail);
        // Most-recent-first order: MultiEdit (lower in tail) wins position 0.
        assert_eq!(
            set.active_files,
            vec!["src/lib.rs".to_string(), "docs/x.md".to_string()]
        );
    }

    #[test]
    fn active_files_dedups_repeated_path() {
        let tail = "● Edit(file: \"src/foo.rs\")\n● Edit(file: \"src/foo.rs\")\n";
        let set = parse_common_signals(tail);
        assert_eq!(
            set.active_files,
            vec!["src/foo.rs".to_string()],
            "the same path appearing twice must dedupe to one entry"
        );
    }

    #[test]
    fn active_files_caps_at_eight_entries() {
        let mut tail = String::new();
        for i in 0..20 {
            tail.push_str(&format!("● Edit(file: \"src/f{i}.rs\")\n"));
        }
        let set = parse_common_signals(&tail);
        assert_eq!(set.active_files.len(), 8);
    }

    #[test]
    fn active_files_skips_empty_parens() {
        // Honesty rule: don't fabricate a path when the args are empty.
        let tail = "● Edit()\n";
        let set = parse_common_signals(tail);
        assert!(set.active_files.is_empty());
    }

    #[test]
    fn active_files_skips_non_path_args() {
        // `● Bash(git status)` is not a file edit; the args do not look
        // like a path. Must not produce a false-positive entry.
        let tail = "● Bash(git status)\n● Read(/etc/hosts)\n";
        let set = parse_common_signals(tail);
        // Read(/etc/hosts) is a Read tool call, not Edit/Write — but
        // even if it were, we explicitly only include edit-class tools.
        assert!(
            set.active_files.is_empty(),
            "Bash and Read tool calls are not edits: {:?}",
            set.active_files
        );
    }

    #[test]
    fn active_files_handles_unquoted_short_form() {
        // Some renderers print `● Edit(src/foo.rs)` without the
        // `file: "…"` named-arg wrapping. Still extract the path.
        let tail = "● Edit(src/foo.rs)\n";
        let set = parse_common_signals(tail);
        assert_eq!(set.active_files, vec!["src/foo.rs".to_string()]);
    }

    #[test]
    fn active_files_rejects_flag_like_args() {
        // `● Bash(--help)` style args should not be treated as paths.
        // Even though our marker list doesn't include Bash, this test
        // belt-and-suspenders the flag rejection in `looks_like_path`.
        let tail = "● Edit(--help)\n";
        let set = parse_common_signals(tail);
        assert!(set.active_files.is_empty());
    }

    #[test]
    fn active_files_default_is_empty_vec() {
        let set = parse_common_signals("");
        assert!(set.active_files.is_empty());
    }
}
