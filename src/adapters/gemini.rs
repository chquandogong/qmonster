use crate::adapters::ProviderParser;
use crate::adapters::common::parse_common_signals;
use crate::adapters::runtime::push_provider_fact;
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{IdleCause, MetricValue, RuntimeFact, RuntimeFactKind, SignalSet};

pub struct GeminiAdapter;

impl ProviderParser for GeminiAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let mut set = parse_common_signals(ctx.tail);
        append_gemini_runtime_facts(&mut set, ctx.tail);
        if let Some(status) = parse_gemini_status(ctx.tail) {
            if let Some(model) = status.model {
                set.model_name = Some(
                    MetricValue::new(model, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
            if let Some(branch) = status.branch {
                set.git_branch = Some(
                    MetricValue::new(branch, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
            if let Some(workspace) = status.workspace {
                set.worktree_path = Some(
                    MetricValue::new(workspace, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
            if let Some(context) = status.context_pressure {
                set.context_pressure = Some(
                    MetricValue::new(context, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
            if let Some(quota) = status.quota_pressure {
                set.quota_pressure = Some(
                    MetricValue::new(quota, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
            // Phase E E1 (v1.21.0): surface the status-table `memory`
            // column as a ProviderOfficial fact. Stays None when the
            // column is absent (operators can hide it via Gemini's
            // `/footer` settings) — absence is honesty.
            if let Some(mem_mb) = status.memory_mb {
                set.process_memory_mb = Some(
                    MetricValue::new(mem_mb, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
        } else if let Some(context) = parse_gemini_context_pressure(ctx.tail) {
            set.context_pressure = Some(
                MetricValue::new(context, SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Gemini),
            );
        }
        // Phase F F-4b (v1.33.0): when the operator presses `u` on a
        // Gemini pane, runtime_refresh cycles `/model` →
        // `/stats session` → `/stats model` only when the pane is
        // idle/stale/limit-hit; active panes skip `/model` and cycle
        // only the stats surfaces. `merge_runtime_refresh_tail` splices
        // captured panels into the parsed tail. `/model` feeds RESET,
        // `/stats session` feeds SID/CALLS, and `/stats model` feeds
        // token/cache counts, mirroring the F-4 (Codex) and F-5b
        // (Claude) enrichment patterns.
        let session = parse_gemini_stats_session(ctx.tail);
        if let Some(sid) = session.session_id.as_ref() {
            set.runtime_facts.push(
                RuntimeFact::new(
                    RuntimeFactKind::SessionId,
                    sid,
                    SourceKind::ProviderOfficial,
                )
                .with_provider(Provider::Gemini)
                .with_confidence(0.95),
            );
        }
        if let Some(tool_calls) = session.tool_calls {
            set.runtime_facts.push(
                RuntimeFact::new(
                    RuntimeFactKind::ToolCalls,
                    tool_calls.to_string(),
                    SourceKind::ProviderOfficial,
                )
                .with_provider(Provider::Gemini)
                .with_confidence(0.95),
            );
        }
        let current_model = set.model_name.as_ref().map(|m| m.value.as_str());
        for reset in parse_gemini_model_resets(ctx.tail, current_model) {
            set.runtime_facts.push(
                RuntimeFact::new(
                    RuntimeFactKind::ModelReset,
                    reset.display_value(),
                    SourceKind::ProviderOfficial,
                )
                .with_provider(Provider::Gemini)
                .with_confidence(0.90),
            );
        }
        if let Some(model) = parse_gemini_stats_model(ctx.tail) {
            let metric = |v| {
                MetricValue::new(v, SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Gemini)
            };
            if set.input_tokens.is_none()
                && let Some(n) = model.input_tokens
            {
                set.input_tokens = Some(metric(n));
            }
            if set.output_tokens.is_none()
                && let Some(n) = model.output_tokens
            {
                set.output_tokens = Some(metric(n));
            }
            // Honesty: `cache_reads` is None for OAuth (Cache Reads row
            // hidden by Google). Only populate cached_input_tokens when
            // the row was actually present — never synthesize a zero,
            // which would render a misleading 0% CACHE badge.
            if set.cached_input_tokens.is_none()
                && let Some(n) = model.cache_reads
            {
                set.cached_input_tokens = Some(metric(n));
            }
        }
        if set.idle_state.is_none() {
            set.idle_state = classify_idle_gemini(ctx.tail, ctx.history);
        }
        set
    }
}

#[derive(Default)]
struct GeminiStatus {
    context_pressure: Option<f32>,
    quota_pressure: Option<f32>,
    branch: Option<String>,
    sandbox: Option<String>,
    model: Option<String>,
    workspace: Option<String>,
    /// Phase E E1 (v1.21.0): Gemini status-table `memory` column,
    /// canonicalized to MiB at parse time. `None` when the column is
    /// absent or the cell value cannot be unit-parsed.
    memory_mb: Option<f64>,
}

fn parse_gemini_status(tail: &str) -> Option<GeminiStatus> {
    let lines: Vec<&str> = tail.lines().collect();
    for idx in (0..lines.len()).rev() {
        let line = lines[idx];
        if !is_gemini_status_header(line) {
            continue;
        }

        let header_cols = split_gemini_status_columns(line);
        for data_line in lines.iter().skip(idx + 1) {
            let trimmed = data_line.trim();
            if trimmed.is_empty() || is_gemini_separator(trimmed) {
                continue;
            }

            let data_cols = split_gemini_status_columns(data_line);
            let status = gemini_status_from_columns(&header_cols, &data_cols);
            if status.context_pressure.is_some()
                || status.quota_pressure.is_some()
                || status.branch.is_some()
                || status.sandbox.is_some()
                || status.model.is_some()
                || status.workspace.is_some()
                || status.memory_mb.is_some()
            {
                return Some(status);
            }
            break;
        }
    }
    None
}

fn gemini_status_from_columns(header_cols: &[&str], data_cols: &[&str]) -> GeminiStatus {
    let mut status = GeminiStatus::default();
    // CFX-2 / gemini-v1.15.0-1: `split_gemini_status_columns` skips
    // empty cells, so a blank data column collapses the row by one and
    // shifts every following field one header to the left. Without
    // this guard the parser would populate `/model` from the
    // workspace path or `context` from a memory string and stamp them
    // ProviderOfficial. Refuse the row entirely instead.
    if header_cols.len() != data_cols.len() {
        return status;
    }
    for (idx, header) in header_cols.iter().enumerate() {
        let Some(value) = data_cols
            .get(idx)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        match header.trim().to_lowercase().as_str() {
            "branch" => status.branch = Some(value.to_string()),
            "sandbox" => status.sandbox = Some(value.to_string()),
            "/model" | "model" => status.model = Some(value.to_string()),
            "workspace (/directory)" | "workspace" | "directory" => {
                status.workspace = Some(value.to_string())
            }
            "context" => status.context_pressure = parse_used_percent(value),
            "quota" => status.quota_pressure = parse_used_percent(value),
            "memory" => status.memory_mb = parse_memory_mb(value),
            _ => {}
        }
    }
    status
}

/// Phase E E1 (v1.21.0): parse the Gemini status-table `memory`
/// column into MiB. Accepts `<float> MB` and `<float> GB` (the two
/// units the Gemini CLI is observed to render); returns `None` when
/// the suffix is missing or the number is unparseable. Other unit
/// suffixes (`KB`, `PB`, ...) are not assumed — fall through to None.
fn parse_memory_mb(cell: &str) -> Option<f64> {
    let trimmed = cell.trim();
    let (num_str, multiplier) = if let Some(n) = trimmed.strip_suffix(" MB") {
        (n.trim_end(), 1.0_f64)
    } else if let Some(n) = trimmed.strip_suffix(" GB") {
        (n.trim_end(), 1024.0_f64)
    } else {
        return None;
    };
    num_str.trim().parse::<f64>().ok().map(|v| v * multiplier)
}

fn parse_gemini_context_pressure(tail: &str) -> Option<f32> {
    parse_gemini_status(tail).and_then(|status| status.context_pressure)
}

fn append_gemini_runtime_facts(set: &mut SignalSet, tail: &str) {
    let lower = tail.to_lowercase();
    if lower.contains("yolo ctrl+y") {
        // CFX-1: prose-substring match — could appear in transcripts /
        // docs / chat — demote to Heuristic.
        push_provider_fact(
            &mut set.runtime_facts,
            Provider::Gemini,
            RuntimeFactKind::AutoMode,
            "YOLO Ctrl+Y",
            0.75,
            SourceKind::Heuristic,
        );
    }
    if let Some(status) = parse_gemini_status(tail) {
        // Status table only populates after the column-parity check
        // (CFX-2) — safe to label ProviderOfficial.
        if let Some(sandbox) = status.sandbox {
            push_provider_fact(
                &mut set.runtime_facts,
                Provider::Gemini,
                RuntimeFactKind::Sandbox,
                sandbox,
                0.95,
                SourceKind::ProviderOfficial,
            );
        }
        if let Some(workspace) = status.workspace {
            push_provider_fact(
                &mut set.runtime_facts,
                Provider::Gemini,
                RuntimeFactKind::AllowedDirectory,
                workspace,
                0.95,
                SourceKind::ProviderOfficial,
            );
        }
    }
}

fn split_gemini_status_columns(line: &str) -> Vec<&str> {
    let mut cols = Vec::new();
    let mut start = 0;
    let mut whitespace_start = None;
    let mut whitespace_count = 0;

    for (idx, ch) in line.char_indices() {
        if ch.is_whitespace() {
            if whitespace_count == 0 {
                whitespace_start = Some(idx);
            }
            whitespace_count += 1;
            continue;
        }

        if whitespace_count >= 2 {
            if let Some(end) = whitespace_start {
                let col = line[start..end].trim();
                if !col.is_empty() {
                    cols.push(col);
                }
            }
            start = idx;
        }
        whitespace_start = None;
        whitespace_count = 0;
    }

    let col = line[start..].trim();
    if !col.is_empty() {
        cols.push(col);
    }
    cols
}

fn parse_used_percent(cell: &str) -> Option<f32> {
    let (num, rest) = cell.trim().split_once('%')?;
    if !rest.trim_start().starts_with("used") {
        return None;
    }
    let pct = num.trim().parse::<u8>().ok()?;
    if pct > 100 {
        return None;
    }
    Some(pct as f32 / 100.0)
}

fn classify_idle_gemini(
    tail: &str,
    history: &crate::adapters::common::PaneTailHistory,
) -> Option<IdleCause> {
    if gemini_limit_hit(tail) {
        return Some(IdleCause::LimitHit);
    }
    if gemini_in_progress_marker(tail) || history.changed_within_capacity() {
        return None;
    }
    if gemini_idle_cursor(tail) {
        return Some(IdleCause::WorkComplete);
    }
    if history.is_still(history.capacity()) {
        return Some(IdleCause::Stale);
    }
    None
}

/// True when the tail contains Gemini's input placeholder in the live
/// idle area.
///
/// The `*  ` prefix (asterisk + 2 spaces) is the prompt glyph Gemini
/// CLI renders when it is waiting for the next user message.
///
/// We cannot just check the last non-empty line because production
/// Gemini renders a status table below the placeholder. We also cannot
/// scan the whole tail blindly: an old placeholder can stay in scrollback
/// after a new request has started, keeping the pane falsely IDLE until
/// enough output pushes it out. The suffix after the placeholder must
/// therefore be empty or only Gemini UI chrome/status rows.
fn gemini_idle_cursor(tail: &str) -> bool {
    let lines: Vec<&str> = tail.lines().collect();
    lines.iter().enumerate().any(|(idx, line)| {
        let t = line.trim_start();
        t.starts_with("*  ")
            && t.contains("Type your message")
            && gemini_suffix_is_idle_chrome(&lines[idx + 1..])
    })
}

fn gemini_suffix_is_idle_chrome(lines: &[&str]) -> bool {
    let mut saw_status_header = false;
    let mut saw_status_data = false;

    for raw in lines {
        let line = raw.trim();
        if line.is_empty() || is_gemini_separator(line) {
            continue;
        }

        if !saw_status_header && is_gemini_status_header(line) {
            saw_status_header = true;
            continue;
        }

        if saw_status_header && !saw_status_data && is_gemini_status_data(line) {
            saw_status_data = true;
            continue;
        }

        return false;
    }

    true
}

fn gemini_in_progress_marker(tail: &str) -> bool {
    let mut checked = 0;
    for raw in tail.lines().rev() {
        let line = raw.trim();
        if line.is_empty() || is_gemini_separator(line) {
            continue;
        }
        checked += 1;
        if checked > 12 {
            break;
        }

        let lower = line.to_lowercase();
        if lower.contains("thinking") && (lower.contains("...") || lower.contains('…')) {
            return true;
        }
    }
    false
}

fn is_gemini_separator(line: &str) -> bool {
    !line.is_empty()
        && line
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '-' | '─' | '━' | '═' | '▄' | '▀'))
}

fn is_gemini_status_header(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("branch")
        && lower.contains("sandbox")
        && lower.contains("/model")
        && lower.contains("quota")
        && lower.contains("context")
        && lower.contains("memory")
}

fn is_gemini_status_data(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("% used") && lower.contains("sandbox")
}

/// True only when Gemini's validated status table reports quota exhaustion.
fn gemini_limit_hit(tail: &str) -> bool {
    parse_gemini_status(tail)
        .and_then(|status| status.quota_pressure)
        .is_some_and(|quota| (quota - 1.0).abs() < f32::EPSILON)
}

/// Phase F F-4b (v1.33.0): runtime facts the `/stats session` panel
/// surfaces. Both fields are independently optional — Gemini hides the
/// `Tool Calls:` row when `tools.totalCalls` is zero, and a partial
/// capture may clip either anchor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GeminiSessionStats {
    pub session_id: Option<String>,
    pub tool_calls: Option<u64>,
}

/// Phase F F-4b (v1.33.0): cumulative token counts the `/stats model`
/// panel surfaces. `cache_reads` is `None` when the "Cache Reads" row
/// is absent (the hardcoded Google FAQ behavior for OAuth Gemini —
/// `hasCached` evaluates false, so the row is omitted entirely). The
/// presence-vs-absence is the auth signal Qmonster preserves so the
/// CACHE badge doesn't render a misleading 0% for OAuth panes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GeminiModelStats {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// None when the "Cache Reads" row is absent (OAuth path) — the
    /// honest signal Qmonster preserves so the CACHE badge doesn't
    /// render a misleading 0% for OAuth Gemini panes.
    pub cache_reads: Option<u64>,
}

/// Gemini `/model` picker rows can render the reset timestamp on the
/// same line as the model or on an indented child row. Keep the raw
/// provider reset text instead of trying to reinterpret timezone/window
/// semantics that Gemini has already formatted for the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GeminiModelReset {
    pub model: String,
    pub reset: String,
}

impl GeminiModelReset {
    fn display_value(&self) -> String {
        format!("{} {}", self.model, self.reset)
    }
}

/// Phase F F-4b (v1.33.0): parse a captured `/stats session` panel
/// for runtime facts the existing surface can populate via
/// RuntimeFacts. Returns the default (all `None`) when the panel
/// anchors aren't present — no Heuristic guessing, only structurally
/// anchored matches survive.
///
/// Anchors: the `Session ID:` and `Tool Calls:` titles emitted by
/// `Section -> StatRow` in the Ink panel. Each line is preceded by
/// box-drawing characters (`│`) and indentation; we strip those and
/// split on the first `:` to get the value.
fn parse_gemini_stats_session(tail: &str) -> GeminiSessionStats {
    let mut out = GeminiSessionStats::default();
    for raw in tail.lines() {
        // Strip the bordered-box vertical glyphs and any leading
        // indentation; do not require a literal `│` though — partial
        // captures (or future Gemini builds rendering the panel
        // borderless) should still parse.
        let cleaned = strip_box_chars(raw);
        let line = cleaned.trim();
        if line.is_empty() {
            continue;
        }
        if out.session_id.is_none()
            && let Some((label, value)) = line.split_once(':')
            && label.trim().eq_ignore_ascii_case("Session ID")
        {
            let v = value.trim();
            if !v.is_empty() {
                out.session_id = Some(v.to_string());
            }
            continue;
        }
        if out.tool_calls.is_none()
            && let Some((label, value)) = line.split_once(':')
            && label.trim().eq_ignore_ascii_case("Tool Calls")
        {
            // Tool Calls renders as `42 ( ✓ 41 ✕ 1 )` (with success/
            // failure breakdown). The leading integer is the
            // `tools.totalCalls` count we want.
            let leading: String = value
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == ',')
                .collect();
            if let Some(n) = parse_count_with_commas(&leading) {
                out.tool_calls = Some(n);
            }
        }
    }
    out
}

/// Phase F F-4b (v1.33.0): parse a captured `/stats model` panel for
/// cumulative token counts. Returns `None` when the table isn't found.
///
/// `cache_reads` is `None` for OAuth (Cache Reads row hidden by
/// Google). Presence-vs-absence is the auth signal — never synthesize
/// a zero. The function only returns `Some` when at least both
/// `input_tokens` and `output_tokens` were extracted; otherwise we
/// don't ship half-data (the honesty rule from F-5b).
///
/// Row shape (after stripping box-drawing/border glyphs and trimming):
///   `Total                                                       1,514,812`
///   `Input                                                       1,234,567`
///   `Cache Reads                                  234,567 (15.5%)`
///   `Output                                                         45,678`
///
/// Each row's first significant whitespace-separated number (with
/// optional commas) is the value.
fn parse_gemini_stats_model(tail: &str) -> Option<GeminiModelStats> {
    let mut input_tokens: Option<u64> = None;
    let mut output_tokens: Option<u64> = None;
    let mut cache_reads: Option<u64> = None;
    let mut in_tokens_section = false;
    let mut saw_token_metric = false;

    for raw in tail.lines() {
        let cleaned = strip_box_chars(raw);
        let line = cleaned.trim();
        if line.is_empty() {
            continue;
        }

        // The "Tokens" section header anchors the metric rows we care
        // about. The `Tokens` label appears alone on a row in the
        // Ink-rendered table. We don't strictly require crossing into
        // the section before matching — the row labels themselves
        // (`Total`, `Input`, `Cache Reads`, `Output`) only appear inside
        // this layout — but tracking the section gives an extra guard
        // against a chat line that happens to mention `Input`.
        if !in_tokens_section
            && let Some(stripped) = line.strip_suffix(':')
            && stripped.eq_ignore_ascii_case("Tokens")
        {
            in_tokens_section = true;
            continue;
        }
        if !in_tokens_section && line.eq_ignore_ascii_case("Tokens") {
            in_tokens_section = true;
            continue;
        }

        // Match the metric label at the start of the line, with the
        // first numeric token that follows being the value. Labels are
        // case-sensitive ("Cache Reads" matches the Ink render exactly).
        if input_tokens.is_none()
            && let Some(rest) = strip_metric_label(line, "Input")
            && let Some(n) = first_count_in(rest)
        {
            input_tokens = Some(n);
            saw_token_metric = true;
            continue;
        }
        if output_tokens.is_none()
            && let Some(rest) = strip_metric_label(line, "Output")
            && let Some(n) = first_count_in(rest)
        {
            output_tokens = Some(n);
            saw_token_metric = true;
            continue;
        }
        if cache_reads.is_none()
            && let Some(rest) = strip_metric_label(line, "Cache Reads")
            && let Some(n) = first_count_in(rest)
        {
            cache_reads = Some(n);
            saw_token_metric = true;
            continue;
        }
    }

    // Honesty rule: only return a stats struct when both required
    // fields parsed AND we saw at least one anchor that is unique to the
    // /stats model panel layout. Otherwise the parser would emit token
    // counts derived from arbitrary tail prose like "Input file". The
    // section header anchor (`Tokens`) is the cleanest such guard.
    if (in_tokens_section || saw_token_metric) && input_tokens.is_some() && output_tokens.is_some()
    {
        Some(GeminiModelStats {
            input_tokens,
            output_tokens,
            cache_reads,
        })
    } else {
        None
    }
}

/// Parse model-specific reset rows from Gemini's `/model` screen.
/// Observed and expected shapes include both:
///
///   `Gemini 2.5 Pro`
///   `  Reset: 5:00 PM (in 1h 12m)`
///
/// and:
///
///   `gemini-2.5-flash Reset: 12:30 AM (4h left)`
///
/// Gemini v0.41 also renders quota rows as:
///
///   `Pro  ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  5%  Resets: 6:45 PM (19h 32m)`
///
/// The parser intentionally requires a nearby model label and at least
/// one digit in the reset text. That keeps chat prose like "Reset:
/// your terminal" out of ProviderOfficial facts. When the current
/// status-table model is known, only matching family rows are surfaced
/// (`gemini-3.1-pro-preview` -> `Pro`, `gemini-*-flash-lite` ->
/// `Flash Lite`, `gemini-*-flash` -> `Flash`).
fn parse_gemini_model_resets(tail: &str, current_model: Option<&str>) -> Vec<GeminiModelReset> {
    let mut out = Vec::new();
    let mut last_model: Option<(String, usize)> = None;

    for (line_idx, raw) in tail.lines().enumerate() {
        let cleaned = normalize_inline_whitespace(&strip_box_chars(raw));
        let line = cleaned.trim();
        if line.is_empty() || is_gemini_separator(line) {
            continue;
        }

        if let Some((reset_idx, reset_label_len)) = find_reset_label(line) {
            let before_reset = line[..reset_idx].trim();
            let reset_text = line[reset_idx + reset_label_len..]
                .trim()
                .trim_matches(|c: char| matches!(c, '|' | '[' | ']'))
                .trim()
                .to_string();
            if reset_text.is_empty() || !reset_text.chars().any(|c| c.is_ascii_digit()) {
                continue;
            }

            let model = extract_gemini_model_name(before_reset)
                .or_else(|| {
                    last_model
                        .as_ref()
                        .filter(|(_, model_idx)| line_idx.saturating_sub(*model_idx) <= 2)
                        .map(|(model, _)| model.clone())
                })
                .filter(|m| !m.is_empty());
            if let Some(model) = model
                && gemini_reset_model_matches_current(&model, current_model)
            {
                let reset = GeminiModelReset {
                    model,
                    reset: reset_text,
                };
                if !out.iter().any(|existing| existing == &reset) {
                    out.push(reset);
                }
            }
            continue;
        }

        if let Some(model) = extract_gemini_model_name(line) {
            last_model = Some((model, line_idx));
        }
    }

    out
}

fn gemini_reset_model_matches_current(reset_model: &str, current_model: Option<&str>) -> bool {
    let Some(current_model) = current_model else {
        return true;
    };
    let Some(current_family) = gemini_model_family(current_model) else {
        return true;
    };
    gemini_model_family(reset_model).is_none_or(|reset_family| reset_family == current_family)
}

fn gemini_model_family(model: &str) -> Option<&'static str> {
    let lower = model.to_ascii_lowercase();
    if lower.contains("flash-lite") || lower.contains("flash lite") {
        Some("flash-lite")
    } else if lower.contains("flash") {
        Some("flash")
    } else if lower.contains("pro") {
        Some("pro")
    } else {
        None
    }
}

fn find_reset_label(line: &str) -> Option<(usize, usize)> {
    let lower = line.to_ascii_lowercase();
    ["resets:", "reset:"]
        .into_iter()
        .filter_map(|label| lower.find(label).map(|idx| (idx, label.len())))
        .min_by_key(|(idx, _)| *idx)
}

fn extract_gemini_model_name(line: &str) -> Option<String> {
    let before_reset = find_reset_label(line)
        .map(|(idx, _)| &line[..idx])
        .unwrap_or(line);
    let candidate = clean_gemini_model_candidate(before_reset)?;
    let lower = before_reset.to_ascii_lowercase();
    if let Some(start) = lower.find("gemini") {
        let candidate = before_reset[start..]
            .trim()
            .trim_matches(|c: char| matches!(c, '|' | '[' | ']' | '(' | ')' | ':' | ','))
            .trim();
        if candidate.is_empty() {
            return None;
        }

        let first = candidate
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches(|c: char| matches!(c, '|' | '[' | ']' | '(' | ')' | ':' | ','));
        if first.to_lowercase().starts_with("gemini-") {
            return Some(first.to_string());
        }

        return Some(candidate.chars().take(64).collect());
    }

    Some(candidate.chars().take(64).collect())
}

fn clean_gemini_model_candidate(line: &str) -> Option<String> {
    let mut candidate = line
        .trim()
        .trim_matches(|c: char| matches!(c, '|' | '[' | ']' | '(' | ')' | ':' | ','))
        .trim();
    while let Some(stripped) = candidate.strip_prefix(|c: char| {
        c.is_whitespace() || matches!(c, '>' | '*' | '-' | '•' | '●' | '○' | '◉' | '✓' | '✔')
    }) {
        candidate = stripped.trim_start();
    }
    if let Some(stripped) = strip_numbered_model_list_prefix(candidate) {
        candidate = stripped;
    }
    if let Some(idx) = candidate.find(is_gemini_usage_bar_char) {
        candidate = candidate[..idx].trim_end();
    }

    let lower = candidate.to_lowercase();
    if candidate.is_empty()
        || lower.contains("select model")
        || lower.contains("choose model")
        || lower.contains("choose a model")
        || lower.contains("stats")
        || lower.contains("session")
        || lower.contains("tokens")
        || lower.contains("reset:")
        || lower.contains("resets:")
    {
        return None;
    }

    let has_gemini = lower.contains("gemini");
    let has_model_word = lower.split_whitespace().any(|word| {
        let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        matches!(word, "pro" | "flash" | "preview")
    });
    let has_digit = lower.chars().any(|c| c.is_ascii_digit());
    if has_gemini || has_model_word || (has_digit && lower.len() <= 48) {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn is_gemini_usage_bar_char(c: char) -> bool {
    matches!(
        c,
        '▬' | '▰'
            | '▱'
            | '█'
            | '▉'
            | '▊'
            | '▋'
            | '▌'
            | '▍'
            | '▎'
            | '▏'
            | '■'
            | '□'
            | '▪'
            | '▫'
    )
}

fn strip_numbered_model_list_prefix(candidate: &str) -> Option<&str> {
    for sep in ['.', ')'] {
        let Some(idx) = candidate.find(sep) else {
            continue;
        };
        let (prefix, rest_with_sep) = candidate.split_at(idx);
        let rest = rest_with_sep[sep.len_utf8()..].trim_start();
        if prefix.chars().all(|c| c.is_ascii_digit())
            && rest_with_sep[sep.len_utf8()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_whitespace())
        {
            return Some(rest);
        }
    }
    None
}

fn normalize_inline_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip Ink/React panel border glyphs and leading whitespace so the
/// caller can treat the remainder as a plain `<label>: <value>` line.
/// Drops `│` `╭` `╮` `╰` `╯` `─` `━` `═` (the box-drawing characters
/// the Ink Box+Text components draw around the panel) plus the
/// surrounding whitespace each one is padded with.
fn strip_box_chars(line: &str) -> String {
    line.chars()
        .filter(|c| !matches!(*c, '│' | '╭' | '╮' | '╰' | '╯' | '─' | '━' | '═'))
        .collect()
}

/// If `line` starts with `label` followed by whitespace, return the
/// remainder of the line (suitable for first-number extraction). Used
/// to anchor `Input` / `Output` / `Cache Reads` row matches.
fn strip_metric_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let line = line.strip_prefix('↳').map(str::trim_start).unwrap_or(line);
    let rest = line.strip_prefix(label)?;
    // Require a whitespace separator so `Input` doesn't match
    // `Inputs` or similar prose.
    if rest.starts_with(|c: char| c.is_whitespace()) {
        Some(rest.trim_start())
    } else {
        None
    }
}

/// Find and parse the first comma-grouped integer in `s`. Returns
/// `None` when no contiguous run of digits/commas is present.
fn first_count_in(s: &str) -> Option<u64> {
    let mut start: Option<usize> = None;
    for (i, ch) in s.char_indices() {
        if ch.is_ascii_digit() {
            if start.is_none() {
                start = Some(i);
            }
        } else if ch == ',' {
            // Only treat commas as part of the run when we've already
            // started seeing digits — a leading comma is just punctuation.
            if start.is_none() {
                continue;
            }
        } else if let Some(begin) = start {
            // End of run.
            let raw = &s[begin..i];
            return parse_count_with_commas(raw);
        }
    }
    if let Some(begin) = start {
        return parse_count_with_commas(&s[begin..]);
    }
    None
}

/// Parse a number with optional thousands-separator commas (e.g.,
/// `1,514,812`) into `u64`. Returns `None` on empty input or anything
/// that isn't a pure digit run after stripping commas.
fn parse_count_with_commas(s: &str) -> Option<u64> {
    let stripped: String = s.chars().filter(|c| *c != ',').collect();
    if stripped.is_empty() {
        return None;
    }
    stripped.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::common::PaneTailHistory;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{IdleCause, RuntimeFactKind};
    use crate::policy::claude_settings::ClaudeSettings;
    use crate::policy::pricing::PricingTable;

    fn id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Gemini,
                instance: 1,
                role: Role::Research,
                pane_id: "%3".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    use crate::adapters::ctx;

    #[test]
    fn gemini_adapter_inherits_subagent_hint() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            "Starting subagent: web-explorer",
            &pricing,
            &settings,
            &history,
        );
        let set = GeminiAdapter.parse(&c);
        assert!(set.subagent_hint);
    }

    #[test]
    fn gemini_type_your_message_placeholder_yields_work_complete() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "previous output\n*  Type your message or @path/to/file";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::WorkComplete));
    }

    #[test]
    fn gemini_status_table_populates_context_pressure_from_context_column() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      47% used      63% used     118.8 MB     cdf3f5ed      user@example.com";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        let metric = set
            .context_pressure
            .as_ref()
            .expect("context pressure parsed");
        assert!((metric.value - 0.63).abs() < f32::EPSILON);
        assert_eq!(metric.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(metric.provider, Some(Provider::Gemini));
        assert_eq!(metric.confidence, Some(0.95));
        assert_eq!(
            set.model_name.as_ref().expect("model parsed").value,
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            set.git_branch.as_ref().expect("branch parsed").value,
            "main"
        );
        assert_eq!(
            set.worktree_path.as_ref().expect("workspace parsed").value,
            "~/projects/mission-spec"
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::Sandbox && f.value == "no sandbox")
        );
        assert!(set.runtime_facts.iter().any(|f| {
            f.kind == RuntimeFactKind::AllowedDirectory && f.value == "~/projects/mission-spec"
        }));
        // CFX-1: status table validated by column-parity guard (CFX-2)
        // — facts derived from it are ProviderOfficial.
        assert!(
            set.runtime_facts
                .iter()
                .all(|f| f.source_kind == SourceKind::ProviderOfficial),
            "all status-table-validated runtime facts must be ProviderOfficial, got: {:?}",
            set.runtime_facts
                .iter()
                .map(|f| (f.kind, f.source_kind))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn gemini_status_table_populates_quota_pressure_from_quota_column() {
        // S3-3: Gemini's status table carries `quota` and `context` as
        // distinct columns. v1.15.0 surfaced `context` as
        // SignalSet.context_pressure; quota stayed only as a limit-hit
        // signal at 100%. This test pins the gradient: a 47% quota
        // value populates `quota_pressure = Some(0.47)` with
        // ProviderOfficial source. The new field is Gemini-only —
        // Codex/Claude have different quota surfaces and stay None
        // until they get a parser.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      47% used      63% used     118.8 MB     cdf3f5ed      user@example.com";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        let metric = set
            .quota_pressure
            .as_ref()
            .expect("quota pressure parsed from quota column");
        assert!((metric.value - 0.47).abs() < f32::EPSILON);
        assert_eq!(metric.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(metric.provider, Some(Provider::Gemini));
        // context_pressure stays accurate too (separate column).
        let ctx_metric = set.context_pressure.as_ref().expect("context parsed");
        assert!((ctx_metric.value - 0.63).abs() < f32::EPSILON);
    }

    #[test]
    fn gemini_quota_pressure_absent_when_status_table_does_not_carry_it() {
        // Honesty: Gemini panes without the quota column (e.g., older
        // CLI build, partial capture) leave quota_pressure as None.
        // The existing prose-only fixture (no structural status block)
        // is the cleanest case.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            "the quota was 100% used yesterday",
            &pricing,
            &settings,
            &history,
        );
        let set = GeminiAdapter.parse(&c);
        assert!(set.quota_pressure.is_none());
    }

    #[test]
    fn gemini_context_pressure_reads_context_not_quota_column() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      100% used     12% used     118.8 MB     cdf3f5ed      user@example.com";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        let metric = set
            .context_pressure
            .as_ref()
            .expect("context pressure parsed");
        assert!((metric.value - 0.12).abs() < f32::EPSILON);
        assert_eq!(
            set.idle_state,
            Some(IdleCause::LimitHit),
            "quota remains a separate limit signal"
        );
    }

    #[test]
    fn gemini_context_100_used_does_not_trigger_quota_limit_hit() {
        // v1.15.10: LimitHit tracks the quota column, not any
        // `100% used` cell in the row. A full context window is
        // surfaced through context_pressure, while quota stays below
        // the usage-limit threshold.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      47% used      100% used    118.8 MB     cdf3f5ed      user@example.com";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
        assert!((set.quota_pressure.as_ref().unwrap().value - 0.47).abs() < f32::EPSILON);
        assert!((set.context_pressure.as_ref().unwrap().value - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn gemini_quota_100_used_with_full_status_columns_yields_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        // 3-column structural anchor: `quota`, `context`, `memory` words
        // present on header row; `100% used` on data row.
        let tail = "\
 branch  sandbox  /model  workspace  quota  context  memory  session  /auth
 main    no sandbox  gemini-3.1  ~/proj  100% used  0% used  119 MB  abc  user@x";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::LimitHit));
    }

    #[test]
    fn gemini_quota_100_used_without_anchor_columns_does_not_fire_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        // Bare `100% used` in prose without the 3-column anchor must
        // not trigger LimitHit.
        let tail = "the quota was 100% used yesterday";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn gemini_status_table_blank_column_does_not_misalign_official_facts() {
        // CFX-2 / gemini-v1.15.0-1 regression: when a data column is
        // blank (here `sandbox`), the 2+-whitespace splitter swallows
        // the empty cell and shifts subsequent cells left. Without a
        // header/data column-count parity guard, the parser would
        // populate `/model` from the workspace path and `workspace`
        // from `47% used` — provider-official labelled but obviously
        // wrong. The parser must instead refuse the misaligned row.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main                        gemini-3.1-pro-preview     ~/projects/mission-spec      47% used      0% used      118.8 MB     cdf3f5ed      user@example.com";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert!(
            set.model_name.is_none(),
            "model must not be populated from a misaligned row"
        );
        assert!(
            set.worktree_path.is_none(),
            "worktree path must not be populated from a misaligned row"
        );
        assert!(
            set.context_pressure.is_none(),
            "context pressure must not be populated from a misaligned row"
        );
        assert!(
            !set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::Sandbox),
            "sandbox runtime fact must not be populated from a misaligned row"
        );
        assert!(
            !set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::AllowedDirectory),
            "allowed-directory runtime fact must not be populated from a misaligned row"
        );
    }

    #[test]
    fn gemini_yolo_hint_populates_runtime_auto_mode() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "YOLO Ctrl+Y", &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        let fact = set
            .runtime_facts
            .iter()
            .find(|f| f.kind == RuntimeFactKind::AutoMode && f.value == "YOLO Ctrl+Y")
            .expect("YOLO Ctrl+Y auto-mode fact emitted");
        // CFX-1: substring match in prose without the structural
        // gemini-status table — Heuristic, not ProviderOfficial.
        assert_eq!(fact.source_kind, SourceKind::Heuristic);
    }

    #[test]
    fn gemini_active_output_yields_none() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "✓  ReadFile foo.rs\n  5 lines";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn gemini_old_placeholder_in_scrollback_with_following_output_is_not_idle_cursor() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
*  Type your message or @path/to/file
──────────────────────────────────────
branch sandbox /model quota context memory
main no sandbox gemini-3.1 ~/proj 47% used 0% used 119 MB

Working on the new request now";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn gemini_changing_tail_with_live_prompt_stays_active() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let previous = "\
Answer line 1
*  Type your message or @path/to/file
branch sandbox /model quota context memory
main no sandbox gemini-3.1 ~/proj 47% used 0% used 119 MB";
        let current = "\
Answer line 1
Answer line 2
*  Type your message or @path/to/file
branch sandbox /model quota context memory
main no sandbox gemini-3.1 ~/proj 47% used 0% used 119 MB";
        let mut history = PaneTailHistory::empty();
        history.push(previous.into());
        history.push(current.into());

        let c = ctx(&id, current, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn gemini_thinking_marker_suppresses_stale_idle_fallback() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let tail = "\
Running tool calls

Thinking...";
        let mut history = PaneTailHistory::empty();
        for _ in 0..history.capacity() {
            history.push(tail.into());
        }

        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    // -----------------------------------------------------------------
    // Phase E E1 (v1.21.0) — Gemini status table memory column
    // -----------------------------------------------------------------

    #[test]
    fn parse_memory_mb_accepts_gemini_status_table_format() {
        // The Gemini CLI status table renders process memory as
        // `<float> MB` (occasional `GB` for long-running sessions).
        // Both must round-trip as MiB for downstream rendering.
        assert!((parse_memory_mb("118.8 MB").unwrap() - 118.8).abs() < f64::EPSILON);
        assert!((parse_memory_mb("47 MB").unwrap() - 47.0).abs() < f64::EPSILON);
        assert!((parse_memory_mb("1.2 GB").unwrap() - (1.2 * 1024.0)).abs() < f64::EPSILON);
        assert!((parse_memory_mb(" 256 MB ").unwrap() - 256.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_memory_mb_rejects_unitless_or_garbage_values() {
        // Without a `MB` / `GB` suffix the value is ambiguous (could
        // be bytes, kilobytes, percent, ...) — return None rather
        // than guessing.
        assert!(parse_memory_mb("118.8").is_none());
        assert!(parse_memory_mb("abc").is_none());
        assert!(parse_memory_mb("").is_none());
        assert!(parse_memory_mb("1 PB").is_none());
    }

    #[test]
    fn gemini_adapter_extracts_memory_from_real_status_table() {
        // End-to-end: a real Gemini v0.39 idle tail (from the
        // `tests/fixtures/real/` corpus shape) must surface the
        // memory column as `process_memory_mb` with
        // ProviderOfficial source.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let tail = " branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth\n main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      47% used      0% used      118.8 MB     cdf3f5ed      <email-redacted>";
        let history = PaneTailHistory::empty();
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);

        let mem = set
            .process_memory_mb
            .as_ref()
            .expect("Gemini status table memory column must populate process_memory_mb");
        assert!(
            (mem.value - 118.8).abs() < f64::EPSILON,
            "got: {}",
            mem.value
        );
        assert_eq!(mem.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(mem.provider, Some(Provider::Gemini));
    }

    #[test]
    fn gemini_adapter_leaves_memory_none_when_status_table_absent() {
        // Pre-render Gemini tail (welcome only, no status table) must
        // not synthesize a memory value — absence is honesty.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let tail =
            " ▝▜▄     Gemini CLI v0.39.0-preview.0\n   ▝▜▄\n  ▗▟▀    Signed in with Google /auth\n";
        let history = PaneTailHistory::empty();
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);

        assert!(
            set.process_memory_mb.is_none(),
            "no status table → no memory value"
        );
    }

    // -----------------------------------------------------------------
    // Phase F F-4b (v1.33.0) — Gemini /stats panel parsing
    // -----------------------------------------------------------------

    #[test]
    fn stats_session_extracts_session_id_and_tool_calls() {
        // Synthetic /stats session panel: bordered box around an
        // `Interaction Summary` Section with `Session ID:` and
        // `Tool Calls:` StatRows. The Ink Tool Calls row renders a
        // success/failure breakdown after the leading total; the
        // parser must pick the first integer.
        let tail = "\
╭─ Session Stats ──────────────────────────────────────╮
│                                                      │
│  Interaction Summary                                 │
│    Session ID:    cdf3f5ed-0123-4567-89ab-cdef01234567│
│    Auth Method:   Signed in with Google              │
│    Tier:          Pro                                │
│    Tool Calls:    42 ( ✓ 41 ✕ 1 )                    │
│    Success Rate:  98%                                │
╰──────────────────────────────────────────────────────╯";
        let stats = parse_gemini_stats_session(tail);
        assert_eq!(
            stats.session_id.as_deref(),
            Some("cdf3f5ed-0123-4567-89ab-cdef01234567")
        );
        assert_eq!(stats.tool_calls, Some(42));
    }

    #[test]
    fn stats_session_returns_default_when_panel_absent() {
        // Ordinary status-table tail (no /stats session panel) —
        // session id and tool count must stay None. Honesty: the
        // bare status table doesn't carry these signals.
        let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      47% used      63% used     118.8 MB     cdf3f5ed      user@example.com";
        let stats = parse_gemini_stats_session(tail);
        assert_eq!(stats, GeminiSessionStats::default());
    }

    #[test]
    fn stats_model_extracts_input_output_with_cache_reads() {
        // API key / Vertex auth: `hasCached` is true, so the Cache
        // Reads row renders inside the Tokens section. All three
        // counts must populate.
        let tail = "\
╭─ Model Usage ────────────────────────────────────────────────────╮
│                                       Input Tokens   Cache Reads   Output Tokens │
│  gemini-3.1-pro-preview                  1,234,567        234,567        45,678  │
│                                                                                  │
│  Tokens                                                                          │
│    Total                                                       1,514,812        │
│    Input                                                       1,234,567        │
│    Cache Reads                                  234,567 (15.5%)                 │
│    Output                                                         45,678        │
╰──────────────────────────────────────────────────────────────────╯";
        let stats = parse_gemini_stats_model(tail).expect("model stats parsed");
        assert_eq!(stats.input_tokens, Some(1_234_567));
        assert_eq!(stats.output_tokens, Some(45_678));
        assert_eq!(stats.cache_reads, Some(234_567));
    }

    #[test]
    fn stats_model_extracts_actual_arrow_prefixed_rows() {
        let tail = "\
╭────────────────────────────────────────────────────────────╮
│  Model Stats For Nerds                                     │
│  Metric                      gemini-3.1-pro-preview        │
│  API                                                       │
│  Requests                    10                            │
│  Tokens                                                    │
│  Total                       190,404                       │
│    ↳ Input                   87,588                        │
│    ↳ Cache Reads             99,632 (53.2%)                │
│    ↳ Thoughts                2,263                         │
│    ↳ Output                  921                           │
╰────────────────────────────────────────────────────────────╯";
        let stats = parse_gemini_stats_model(tail).expect("model stats parsed");
        assert_eq!(stats.input_tokens, Some(87_588));
        assert_eq!(stats.output_tokens, Some(921));
        assert_eq!(stats.cache_reads, Some(99_632));
    }

    #[test]
    fn stats_model_extracts_input_output_when_cache_reads_absent() {
        // OAuth Gemini: `hasCached` is false (Google FAQ — cache reads
        // are hidden for OAuth), so the Cache Reads row is omitted
        // entirely. The parser must surface input/output without
        // synthesizing a zero for cache_reads — absence is the auth
        // signal.
        let tail = "\
╭─ Model Usage ────────────────────────────────────────────────────╮
│                                       Input Tokens   Output Tokens │
│  gemini-3.1-pro-preview                  1,234,567          45,678  │
│                                                                    │
│  Tokens                                                            │
│    Total                                                 1,280,490 │
│    Input                                                 1,234,567 │
│    Output                                                   45,678 │
╰──────────────────────────────────────────────────────────────────╯";
        let stats = parse_gemini_stats_model(tail).expect("model stats parsed");
        assert_eq!(stats.input_tokens, Some(1_234_567));
        assert_eq!(stats.output_tokens, Some(45_678));
        assert_eq!(
            stats.cache_reads, None,
            "OAuth Gemini hides Cache Reads — must not synthesize a zero"
        );
    }

    #[test]
    fn stats_model_returns_none_when_table_absent() {
        // Ordinary status-table tail — no /stats model panel anchors,
        // so the parser must return None rather than guess.
        let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      47% used      63% used     118.8 MB     cdf3f5ed      user@example.com";
        assert!(parse_gemini_stats_model(tail).is_none());
    }

    #[test]
    fn model_screen_extracts_reset_rows_with_model_context() {
        let tail = "\
╭─ Select Model ─────────────────────────────────────────────╮
│  Gemini 2.5 Pro                                            │
│    Reset: 5:00 PM (in 1h 12m)                              │
│  gemini-2.5-flash Reset: 12:30 AM (4h left)                │
╰────────────────────────────────────────────────────────────╯";
        let resets = parse_gemini_model_resets(tail, None);
        assert_eq!(
            resets,
            vec![
                GeminiModelReset {
                    model: "Gemini 2.5 Pro".into(),
                    reset: "5:00 PM (in 1h 12m)".into(),
                },
                GeminiModelReset {
                    model: "gemini-2.5-flash".into(),
                    reset: "12:30 AM (4h left)".into(),
                },
            ]
        );
    }

    #[test]
    fn model_screen_extracts_reset_rows_when_model_label_omits_gemini_word() {
        let tail = "\
╭─ Select Model ─────────────────────────────────────────────╮
│  2.5 Pro                                                   │
│    Reset: 5:00 PM (in 1h 12m)                              │
│  Flash Reset: 12:30 AM (4h left)                           │
╰────────────────────────────────────────────────────────────╯";
        let resets = parse_gemini_model_resets(tail, None);
        assert_eq!(
            resets,
            vec![
                GeminiModelReset {
                    model: "2.5 Pro".into(),
                    reset: "5:00 PM (in 1h 12m)".into(),
                },
                GeminiModelReset {
                    model: "Flash".into(),
                    reset: "12:30 AM (4h left)".into(),
                },
            ]
        );
    }

    #[test]
    fn model_screen_extracts_actual_model_usage_resets_rows() {
        let tail = "\
╭────────────────────────────────────────────────────────────╮
│ Select Model                                               │
│ Model usage                                                │
│ Flash       ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  0%   Resets: 11:13 PM (24h)
│ Flash Lite  ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  0%   Resets: 11:43 AM (12h 30m)
│ Pro         ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  5%   Resets: 6:45 PM (19h 32m)
╰────────────────────────────────────────────────────────────╯";
        let resets = parse_gemini_model_resets(tail, None);
        assert_eq!(
            resets,
            vec![
                GeminiModelReset {
                    model: "Flash".into(),
                    reset: "11:13 PM (24h)".into(),
                },
                GeminiModelReset {
                    model: "Flash Lite".into(),
                    reset: "11:43 AM (12h 30m)".into(),
                },
                GeminiModelReset {
                    model: "Pro".into(),
                    reset: "6:45 PM (19h 32m)".into(),
                },
            ]
        );
    }

    #[test]
    fn model_screen_filters_resets_to_current_model_family() {
        let tail = "\
╭────────────────────────────────────────────────────────────╮
│ Select Model                                               │
│ Model usage                                                │
│ Flash       ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  0%   Resets: 11:13 PM (24h)
│ Flash Lite  ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  0%   Resets: 11:43 AM (12h 30m)
│ Pro         ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  5%   Resets: 6:45 PM (19h 32m)
╰────────────────────────────────────────────────────────────╯";
        let resets = parse_gemini_model_resets(tail, Some("gemini-3.1-pro-preview"));
        assert_eq!(
            resets,
            vec![GeminiModelReset {
                model: "Pro".into(),
                reset: "6:45 PM (19h 32m)".into(),
            }]
        );
    }

    #[test]
    fn model_screen_ignores_reset_prose_without_model_or_time() {
        let tail = "\
assistant says Reset: your terminal prompt
Gemini 2.5 Pro
Reset: not yet
Gemini 2.5 Flash
chat line
another chat line
Reset: 5:00 PM";
        assert!(
            parse_gemini_model_resets(tail, None).is_empty(),
            "ProviderOfficial reset facts require a Gemini model and numeric reset text"
        );
    }

    #[test]
    fn gemini_adapter_parse_populates_input_output_from_stats_model_tail() {
        // End-to-end: GeminiAdapter.parse must surface the /stats model
        // panel's Input/Output rows on SignalSet with
        // ProviderOfficial source and Gemini provider tag, mirroring
        // the F-4 (Codex) and F-5b (Claude) wiring.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
╭─ Model Usage ────────────────────────────────────────────────────╮
│  Tokens                                                          │
│    Total                                                 1,514,812│
│    Input                                                 1,234,567│
│    Cache Reads                              234,567 (15.5%)      │
│    Output                                                   45,678│
╰──────────────────────────────────────────────────────────────────╯";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);

        let inp = set.input_tokens.as_ref().expect("input parsed");
        assert_eq!(inp.value, 1_234_567);
        assert_eq!(inp.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(inp.provider, Some(Provider::Gemini));

        let out = set.output_tokens.as_ref().expect("output parsed");
        assert_eq!(out.value, 45_678);
        assert_eq!(out.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(out.provider, Some(Provider::Gemini));

        let cached = set
            .cached_input_tokens
            .as_ref()
            .expect("cache reads parsed");
        assert_eq!(cached.value, 234_567);
        assert_eq!(cached.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(cached.provider, Some(Provider::Gemini));
    }

    #[test]
    fn gemini_adapter_oauth_path_keeps_cached_input_tokens_none() {
        // OAuth pane: stats-model tail without the Cache Reads row
        // must leave cached_input_tokens None. The CACHE badge then
        // stays blank rather than rendering a misleading 0%.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
╭─ Model Usage ────────────────────────────────────────────────────╮
│  Tokens                                                          │
│    Total                                                 1,280,245│
│    Input                                                 1,234,567│
│    Output                                                   45,678│
╰──────────────────────────────────────────────────────────────────╯";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);

        assert!(set.input_tokens.is_some(), "input still surfaces for OAuth");
        assert!(
            set.output_tokens.is_some(),
            "output still surfaces for OAuth"
        );
        assert!(
            set.cached_input_tokens.is_none(),
            "Cache Reads row absent (OAuth) — cached_input_tokens stays None"
        );
    }

    #[test]
    fn gemini_adapter_parse_emits_session_id_runtime_fact_from_stats_session_tail() {
        // /stats session panel must emit SessionId and ToolCalls
        // RuntimeFacts with
        // ProviderOfficial source and Gemini provider tag, reusing the
        // F-5b session-id kind so existing surfaces light up.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
╭─ Session Stats ──────────────────────────────────────╮
│  Interaction Summary                                 │
│    Session ID:    cdf3f5ed-aaaa-bbbb-cccc-dddddddddddd│
│    Tool Calls:    7 ( ✓ 7 ✕ 0 )                      │
╰──────────────────────────────────────────────────────╯";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        let fact = set
            .runtime_facts
            .iter()
            .find(|f| f.kind == RuntimeFactKind::SessionId)
            .expect("session id runtime fact emitted");
        assert_eq!(fact.value, "cdf3f5ed-aaaa-bbbb-cccc-dddddddddddd");
        assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(fact.provider, Some(Provider::Gemini));

        let calls = set
            .runtime_facts
            .iter()
            .find(|f| f.kind == RuntimeFactKind::ToolCalls)
            .expect("tool call runtime fact emitted");
        assert_eq!(calls.value, "7");
        assert_eq!(calls.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(calls.provider, Some(Provider::Gemini));
    }

    #[test]
    fn gemini_adapter_parse_emits_model_reset_runtime_facts_from_model_screen_tail() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
╭─ Select Model ─────────────────────────────────────────────╮
│  Gemini 2.5 Pro                                            │
│    Reset: 5:00 PM (in 1h 12m)                              │
╰────────────────────────────────────────────────────────────╯";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        let fact = set
            .runtime_facts
            .iter()
            .find(|f| f.kind == RuntimeFactKind::ModelReset)
            .expect("model reset runtime fact emitted");

        assert_eq!(fact.value, "Gemini 2.5 Pro 5:00 PM (in 1h 12m)");
        assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(fact.provider, Some(Provider::Gemini));
    }

    #[test]
    fn gemini_adapter_parse_emits_model_reset_from_actual_usage_rows() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
╭────────────────────────────────────────────────────────────╮
│ Select Model                                               │
│ Model usage                                                │
│ Flash       ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  0%   Resets: 11:13 PM (24h)
│ Flash Lite  ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  0%   Resets: 11:43 AM (12h 30m)
│ Pro         ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬  5%   Resets: 6:45 PM (19h 32m)
╰────────────────────────────────────────────────────────────╯
branch         sandbox             /model                         workspace (/directory)         quota           context          memory           session                       /auth
main           no sandbox          gemini-3.1-pro-preview         ~/Qmonster                     5% used         0% used          238.2 MB         af324058         user@example.com";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        let facts = set
            .runtime_facts
            .iter()
            .filter(|f| f.kind == RuntimeFactKind::ModelReset)
            .collect::<Vec<_>>();

        assert_eq!(
            facts.len(),
            1,
            "only the current Pro model reset should surface"
        );
        let fact = facts[0];
        assert_eq!(fact.value, "Pro 6:45 PM (19h 32m)");
        assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(fact.provider, Some(Provider::Gemini));
    }
}
