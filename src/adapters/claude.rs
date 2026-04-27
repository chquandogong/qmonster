use crate::adapters::ProviderParser;
use crate::adapters::common::{PaneTailHistory, parse_common_signals, parse_count_with_suffix};
use crate::adapters::runtime::{push_provider_fact, split_runtime_list};
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{IdleCause, MetricValue, RuntimeFactKind, SignalSet};

pub struct ClaudeAdapter;

#[derive(Debug, Clone, PartialEq)]
struct ClaudeStatusLine {
    model: String,
    reasoning_effort: Option<String>,
    context_pressure: Option<f32>,
    quota_5h_pressure: Option<f32>,
    quota_weekly_pressure: Option<f32>,
    worktree_path: Option<String>,
}

impl ProviderParser for ClaudeAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let tail = ctx.tail;
        let mut set = parse_common_signals(tail);
        append_claude_runtime_facts(&mut set, tail, ctx.claude_settings);

        if let Some(status) = parse_claude_status_line(tail) {
            if let Some(pct) = status.context_pressure {
                set.context_pressure = Some(
                    MetricValue::new(pct, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Claude),
                );
            }
            if let Some(pct) = status.quota_5h_pressure {
                set.quota_5h_pressure = Some(
                    MetricValue::new(pct, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Claude),
                );
            }
            if let Some(pct) = status.quota_weekly_pressure {
                set.quota_weekly_pressure = Some(
                    MetricValue::new(pct, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Claude),
                );
            }
            set.model_name = Some(
                MetricValue::new(status.model, SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Claude),
            );
            if let Some(effort) = status.reasoning_effort {
                set.reasoning_effort = Some(
                    MetricValue::new(effort, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Claude),
                );
            }
            set.worktree_path = status.worktree_path.map(|path| {
                MetricValue::new(path, SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Claude)
            });
        }
        if set.context_pressure.is_none()
            && let Some(pct) = parse_claude_context_pressure(tail)
        {
            set.context_pressure = Some(
                MetricValue::new(pct, SourceKind::ProviderOfficial)
                    .with_confidence(0.9)
                    .with_provider(Provider::Claude),
            );
        }
        let (quota_5h, quota_weekly) = parse_claude_usage_quotas(tail);
        if set.quota_5h_pressure.is_none() {
            set.quota_5h_pressure = quota_5h.map(|pct| {
                MetricValue::new(pct, SourceKind::ProviderOfficial)
                    .with_confidence(0.9)
                    .with_provider(Provider::Claude)
            });
        }
        if set.quota_weekly_pressure.is_none() {
            set.quota_weekly_pressure = quota_weekly.map(|pct| {
                MetricValue::new(pct, SourceKind::ProviderOfficial)
                    .with_confidence(0.9)
                    .with_provider(Provider::Claude)
            });
        }

        if let Some(n) = parse_claude_output_tokens(tail) {
            set.token_count = Some(
                MetricValue::new(n, SourceKind::ProviderOfficial)
                    .with_confidence(0.85)
                    .with_provider(Provider::Claude),
            );
        }

        // Settings remain a fallback when the live statusline is absent.
        if set.model_name.is_none()
            && let Some(m) = ctx.claude_settings.model()
        {
            set.model_name = Some(
                MetricValue::new(m.to_string(), SourceKind::ProviderOfficial)
                    .with_confidence(0.9)
                    .with_provider(Provider::Claude),
            );
        }

        // Slice 4: classify idle state. parse_common_signals already
        // populated PermissionWait/InputWait from markers. We refine
        // with Claude-specific cursor + limit detection, then fall back
        // to stillness if history shows the tail unchanged.
        if set.idle_state.is_none() {
            set.idle_state = classify_idle_claude(tail, ctx.history);
        }

        set
    }
}

fn classify_idle_claude(tail: &str, history: &PaneTailHistory) -> Option<IdleCause> {
    // Step 2 — limit-hit text (Claude /status panel patterns).
    if claude_limit_hit(tail) {
        return Some(IdleCause::LimitHit);
    }
    // Step 3 — idle cursor at last non-empty line.
    if claude_idle_cursor(tail) {
        return Some(IdleCause::WorkComplete);
    }
    // Step 4 — stillness fallback.
    if history.is_still(history.capacity()) {
        return Some(IdleCause::Stale);
    }
    None
}

fn claude_idle_cursor(tail: &str) -> bool {
    // The last non-empty line is the bare `❯` prompt-ready glyph.
    // Do not match `❯ do work`: once the user submits a request, many
    // panes keep that echo visible until the provider's first output.
    // Treating any `❯ ...` prefix as idle keeps stale IDLE state alive.
    let last = tail.lines().rev().find(|l| !l.trim().is_empty());
    last.is_some_and(|l| l.trim() == "❯")
}

fn claude_limit_hit(tail: &str) -> bool {
    // Pattern A: two-line `Current session\n... 100% used` from /usage.
    // Pattern B: same-line `Current week (...) 100% used`.
    // Pattern C: runtime limit banner (`usage limit reached` plus a
    // reset / retry hint). The reset anchor keeps ordinary prose about
    // usage limits from becoming a fake LIMIT state.
    let lines: Vec<&str> = tail.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        if claude_usage_limit_banner(&lines, i) {
            return true;
        }
        if lower.contains("current session") {
            // Look at next non-empty line for "100% used"
            for next_line in lines.iter().skip(i + 1) {
                let next = next_line.trim();
                if next.is_empty() {
                    continue;
                }
                if next.to_lowercase().contains("100% used") {
                    return true;
                }
                break;
            }
        }
        if lower.contains("current week") && lower.contains("100% used") {
            return true;
        }
    }
    false
}

fn claude_usage_limit_banner(lines: &[&str], idx: usize) -> bool {
    let line = lines[idx].to_lowercase();
    let has_limit_phrase = claude_limit_phrase(&line);
    if !has_limit_phrase {
        return false;
    }

    lines
        .iter()
        .skip(idx)
        .take(8)
        .any(|window_line| has_limit_recovery_hint(&window_line.to_lowercase()))
}

fn claude_limit_phrase(line: &str) -> bool {
    let mentions_limit = line.contains("usage limit")
        || line.contains("rate limit")
        || line.contains("billing limit")
        || line.contains("limit reached")
        || (line.contains("claude") && line.contains("limit"))
        || (line.contains("you have reached") && line.contains("limit"));
    let terminal = line.contains("reached")
        || line.contains("exceeded")
        || line.contains("hit")
        || line.contains("blocked")
        || line.contains("stopped")
        || line.contains("unavailable")
        || line.contains("limit reached");
    mentions_limit && terminal
}

fn has_limit_recovery_hint(line: &str) -> bool {
    line.contains("reset")
        || line.contains("resets")
        || line.contains("retry")
        || line.contains("retry after")
        || line.contains("try again")
        || line.contains("upgrade")
        || line.contains("later")
        || line.contains("until")
        || line.contains("tomorrow")
        || line.contains("available")
}

fn parse_claude_status_line(tail: &str) -> Option<ClaudeStatusLine> {
    tail.lines().rev().find_map(parse_claude_status_line_row)
}

fn parse_claude_status_line_row(line: &str) -> Option<ClaudeStatusLine> {
    let trimmed = line.trim().trim_matches('│').trim();
    let lower = trimmed.to_ascii_lowercase();
    let ctx_idx = find_status_label(&lower, "ctx")?;
    let h5_idx = find_status_label(&lower, "5h")?;
    let d7_idx = find_status_label(&lower, "7d")?;
    if !(ctx_idx < h5_idx && h5_idx < d7_idx) {
        return None;
    }

    // Each percent search is bounded by the next label so that an em-dash
    // placeholder (statusline.sh renders missing JSON fields as "—") in
    // one column cannot bleed the following column's value into itself.
    let context_pressure =
        percent_or_zero_placeholder_in_range(trimmed, ctx_idx + "ctx".len(), h5_idx);
    let quota_5h_pressure = percent_in_range(trimmed, h5_idx + "5h".len(), d7_idx);
    let quota_weekly_pressure = percent_in_range(trimmed, d7_idx + "7d".len(), trimmed.len());
    let (model, reasoning_effort) = parse_claude_statusline_model(&trimmed[..ctx_idx])?;
    let worktree_path = path_after_label(trimmed, d7_idx, "7d");

    Some(ClaudeStatusLine {
        model,
        reasoning_effort,
        context_pressure,
        quota_5h_pressure,
        quota_weekly_pressure,
        worktree_path,
    })
}

fn parse_claude_statusline_model(prefix: &str) -> Option<(String, Option<String>)> {
    let cleaned = prefix.trim().trim_matches('│').trim();
    let (model, effort) = match cleaned.rsplit_once('·') {
        Some((model, effort)) => {
            let effort = effort
                .split_whitespace()
                .next()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            (model.trim(), effort)
        }
        None => (cleaned, None),
    };
    let model = model.trim();
    if model.is_empty() {
        return None;
    }
    Some((model.to_string(), effort))
}

fn find_status_label(line: &str, label: &str) -> Option<usize> {
    line.match_indices(label).find_map(|(idx, _)| {
        let before_ok = idx == 0 || !line.as_bytes()[idx.saturating_sub(1)].is_ascii_alphanumeric();
        let after_idx = idx + label.len();
        let after_ok =
            after_idx >= line.len() || !line.as_bytes()[after_idx].is_ascii_alphanumeric();
        (before_ok && after_ok).then_some(idx)
    })
}

fn percent_in_range(line: &str, start: usize, bound: usize) -> Option<f32> {
    let segment = line.get(start..bound)?;
    let pct_idx = segment.find('%')?;
    parse_percent_before(segment, pct_idx)
}

fn percent_or_zero_placeholder_in_range(line: &str, start: usize, bound: usize) -> Option<f32> {
    let segment = line.get(start..bound)?;
    if let Some(pct_idx) = segment.find('%') {
        return parse_percent_before(segment, pct_idx);
    }
    status_placeholder_token(segment).then_some(0.0)
}

fn status_placeholder_token(segment: &str) -> bool {
    segment
        .split_whitespace()
        .any(|token| matches!(token, "—" | "–" | "-"))
}

fn path_after_label(line: &str, idx: usize, label: &str) -> Option<String> {
    let start = idx + label.len();
    let rest = &line[start..];
    let before_border = rest.split('│').next().unwrap_or(rest);
    before_border
        .split_whitespace()
        .find(|token| {
            token.starts_with("~/") || token.starts_with('/') || token.starts_with("$HOME/")
        })
        .map(|token| token.trim_matches('│').to_string())
}

fn parse_claude_context_pressure(tail: &str) -> Option<f32> {
    // Claude's `/context` output is not box-bordered like Codex, so keep
    // this anchored to context-labeled percent lines and explicitly avoid
    // `/usage` quota labels. Prefer "used" when both used/left appear.
    if let Some(pct) = parse_claude_context_usage_block(tail) {
        return Some(pct);
    }
    for line in tail.lines().rev() {
        let lower = line.to_lowercase();
        if !lower.contains("context") {
            continue;
        }
        if lower.contains("current session")
            || lower.contains("current week")
            || lower.contains("all models")
        {
            continue;
        }
        if let Some(used) = percent_followed_by_any(&lower, &["used", "full"]) {
            return Some(used);
        }
        if let Some(left) = percent_followed_by_any(&lower, &["left", "remaining", "available"]) {
            return Some((1.0 - left).clamp(0.0, 1.0));
        }
    }
    None
}

fn parse_claude_context_usage_block(tail: &str) -> Option<f32> {
    let lower_tail = tail.to_lowercase();
    if !lower_tail.contains("context usage") {
        return None;
    }

    lower_tail.lines().find_map(|line| {
        if line.contains(':') || !line.contains('/') || !line.contains("tokens") {
            return None;
        }
        percent_inside_parens(line)
    })
}

fn parse_claude_usage_quotas(tail: &str) -> (Option<f32>, Option<f32>) {
    let lines: Vec<&str> = tail.lines().collect();
    let mut quota_5h = None;
    let mut quota_weekly = None;
    for (idx, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        if quota_5h.is_none() && lower.contains("current session") {
            quota_5h = percent_used_near(&lines, idx);
        }
        if quota_weekly.is_none()
            && (lower.contains("current week") && lower.contains("all models")
                || lower.contains("week (all models)"))
        {
            quota_weekly = percent_used_near(&lines, idx);
        }
    }
    (quota_5h, quota_weekly)
}

fn percent_used_near(lines: &[&str], idx: usize) -> Option<f32> {
    lines
        .iter()
        .skip(idx)
        .take(5)
        .find_map(|line| percent_followed_by_any(&line.to_lowercase(), &["used"]))
}

fn percent_followed_by_any(line: &str, words: &[&str]) -> Option<f32> {
    for (idx, _) in line.match_indices('%') {
        let Some(pct) = parse_percent_before(line, idx) else {
            continue;
        };
        let after = line[idx + 1..].trim_start();
        if words.iter().any(|word| after.starts_with(word)) {
            return Some(pct);
        }
    }
    None
}

fn percent_inside_parens(line: &str) -> Option<f32> {
    for (idx, _) in line.match_indices('%') {
        let before = &line[..idx];
        if before.rsplit_once('(').is_none() {
            continue;
        }
        return parse_percent_before(line, idx);
    }
    None
}

fn parse_percent_before(line: &str, pct_idx: usize) -> Option<f32> {
    let bytes = line.as_bytes();
    let mut start = pct_idx;
    while start > 0 {
        let b = bytes[start - 1];
        if b.is_ascii_digit() || b == b'.' {
            start -= 1;
        } else {
            break;
        }
    }
    if start == pct_idx {
        return None;
    }
    let value: f32 = line[start..pct_idx].parse().ok()?;
    (0.0..=100.0).contains(&value).then_some(value / 100.0)
}

fn append_claude_runtime_facts(
    set: &mut SignalSet,
    tail: &str,
    settings: &crate::policy::claude_settings::ClaudeSettings,
) {
    // Settings file is operator-curated provider config — ProviderOfficial.
    if let Some(mode) = settings.permission_mode() {
        push_provider_fact(
            &mut set.runtime_facts,
            Provider::Claude,
            RuntimeFactKind::PermissionMode,
            mode,
            0.85,
            SourceKind::ProviderOfficial,
        );
    }
    for dir in settings.additional_directories() {
        push_provider_fact(
            &mut set.runtime_facts,
            Provider::Claude,
            RuntimeFactKind::AllowedDirectory,
            dir,
            0.85,
            SourceKind::ProviderOfficial,
        );
    }
    for tool in settings.allowed_tools() {
        push_provider_fact(
            &mut set.runtime_facts,
            Provider::Claude,
            RuntimeFactKind::LoadedTool,
            tool,
            0.85,
            SourceKind::ProviderOfficial,
        );
    }
    for tool in settings.disallowed_tools() {
        push_provider_fact(
            &mut set.runtime_facts,
            Provider::Claude,
            RuntimeFactKind::RestrictedTool,
            tool,
            0.85,
            SourceKind::ProviderOfficial,
        );
    }

    append_claude_tail_runtime_facts(&mut set.runtime_facts, tail);
}

fn append_claude_tail_runtime_facts(
    facts: &mut Vec<crate::domain::signal::RuntimeFact>,
    tail: &str,
) {
    let lines: Vec<&str> = tail.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        // CFX-1: Claude's tail does not have a structural panel
        // anchor (no `│` borders like Codex) so any prose line that
        // mentions "Tools:" / "Working directory:" / "bypass
        // permissions on" looks identical to a real status surface.
        // Demote tail-derived facts to Heuristic; only the settings
        // file path stays ProviderOfficial.
        let is_statusline_permission_hint = trimmed.starts_with("⏵⏵");
        let permission_source = if is_statusline_permission_hint {
            SourceKind::ProviderOfficial
        } else {
            SourceKind::Heuristic
        };
        if lower.contains("bypass permissions on") {
            push_provider_fact(
                facts,
                Provider::Claude,
                RuntimeFactKind::PermissionMode,
                "bypass permissions on",
                0.95,
                permission_source,
            );
        } else if lower.contains("bypass permissions off") {
            push_provider_fact(
                facts,
                Provider::Claude,
                RuntimeFactKind::PermissionMode,
                "bypass permissions off",
                0.95,
                permission_source,
            );
        }

        append_claude_labeled_runtime_fact(facts, trimmed);

        if let Some(skill) = extract_named_call(trimmed, "Skill")
            && claude_skill_window_loaded(&lines, idx)
        {
            push_provider_fact(
                facts,
                Provider::Claude,
                RuntimeFactKind::LoadedSkill,
                skill,
                0.9,
                SourceKind::Heuristic,
            );
        }

        if let Some(tool) = extract_observed_tool_call(trimmed) {
            push_provider_fact(
                facts,
                Provider::Claude,
                RuntimeFactKind::LoadedTool,
                format!("{tool} (observed)"),
                0.7,
                SourceKind::Heuristic,
            );
        }
    }
}

fn append_claude_labeled_runtime_fact(
    facts: &mut Vec<crate::domain::signal::RuntimeFact>,
    line: &str,
) {
    let Some((raw_label, raw_value)) = line.split_once(':') else {
        return;
    };
    let label = raw_label
        .trim()
        .trim_matches('│')
        .trim()
        .to_ascii_lowercase();
    let value = raw_value.trim().trim_matches('│').trim();

    let kind = match label.as_str() {
        "permission mode" | "permissions" => Some(RuntimeFactKind::PermissionMode),
        "allowed directories" | "additional directories" | "add dirs" | "working directory" => {
            Some(RuntimeFactKind::AllowedDirectory)
        }
        "allowed tools" | "available tools" | "tools" => Some(RuntimeFactKind::LoadedTool),
        "disallowed tools" | "disabled tools" | "restricted tools" => {
            Some(RuntimeFactKind::RestrictedTool)
        }
        "loaded skills" | "skills" => Some(RuntimeFactKind::LoadedSkill),
        "loaded plugins" | "plugins" | "mcp servers" => Some(RuntimeFactKind::LoadedPlugin),
        _ => None,
    };

    let Some(kind) = kind else {
        return;
    };
    for item in split_runtime_list(value) {
        // Tail-prose `key: value` matches are too liberal to label
        // ProviderOfficial; demote (CFX-1).
        push_provider_fact(
            facts,
            Provider::Claude,
            kind,
            item,
            0.9,
            SourceKind::Heuristic,
        );
    }
}

fn claude_skill_window_loaded(lines: &[&str], idx: usize) -> bool {
    lines
        .iter()
        .skip(idx)
        .take(4)
        .any(|line| line.to_lowercase().contains("successfully loaded skill"))
}

fn extract_named_call(line: &str, name: &str) -> Option<String> {
    let marker = format!("{name}(");
    let start = line.find(&marker)?;
    let rest = &line[start + marker.len()..];
    let end = rest.find(')')?;
    let value = rest[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn extract_observed_tool_call(line: &str) -> Option<String> {
    let trimmed = line
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '●' | '⏺' | '⎿' | '-'))
        .trim_start();
    let open = trimmed.find('(')?;
    let name = trimmed[..open].trim();
    if name == "Skill"
        || name.is_empty()
        || !name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(name.to_string())
}

fn parse_claude_output_tokens(tail: &str) -> Option<u64> {
    // Priority 1: `Done (… · N[kM] tokens · …)` — subagent finished, cumulative.
    for line in tail.lines().rev() {
        if let Some(n) = extract_done_tokens(line) {
            return Some(n);
        }
    }
    // Priority 2: `↓ N[kM] tokens` — live working line.
    for line in tail.lines().rev() {
        if let Some(n) = extract_arrow_tokens(line) {
            return Some(n);
        }
    }
    None
}

fn extract_done_tokens(line: &str) -> Option<u64> {
    // match substring: "· <count> tokens" where the line also contains "Done ("
    if !line.contains("Done (") {
        return None;
    }
    extract_tokens_after_middot(line)
}

fn extract_arrow_tokens(line: &str) -> Option<u64> {
    // match "↓ <count> tokens"
    let idx = line.find('↓')?;
    let rest = &line[idx + '↓'.len_utf8()..];
    extract_tokens_substring(rest)
}

fn extract_tokens_after_middot(line: &str) -> Option<u64> {
    // Look for " · <count> tokens"
    for segment in line.split(" · ") {
        if let Some(n) = extract_tokens_substring(segment) {
            return Some(n);
        }
    }
    None
}

fn extract_tokens_substring(s: &str) -> Option<u64> {
    // Split on whitespace, find a "[number][suffix?]" immediately before "tokens"
    let words: Vec<&str> = s.split_whitespace().collect();
    for w in words.windows(2) {
        if w[1] == "tokens"
            && let Some(n) = parse_count_with_suffix(w[0])
        {
            return Some(n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::common::{PaneTailHistory, STILLNESS_WINDOW};
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
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    use crate::adapters::ctx;

    #[test]
    fn claude_adapter_inherits_common_signals() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            "Press ENTER to continue",
            &pricing,
            &settings,
            &history,
        );
        let set = ClaudeAdapter.parse(&c);
        assert!(matches!(set.idle_state, Some(IdleCause::InputWait)));
    }

    #[test]
    fn claude_adapter_does_not_populate_context_pressure_from_prose_v1_13_1() {
        // v1.13.1: parse_context_percent_claude was dropped — its
        // substring matching of `claude` + `%` fired on operator prose
        // and on Claude's /status rate-limit bars. The Claude adapter
        // now leaves context_pressure None until S3-4 ships a
        // structured ~/.claude/ state reader.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "claude context 88%", &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert!(
            set.context_pressure.is_none(),
            "Claude adapter must not parse context_pressure from tail prose (v1.13.1)"
        );
    }

    #[test]
    fn claude_context_command_populates_context_pressure() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
Context
Context window: 82% used
Esc to cancel";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        let m = set
            .context_pressure
            .expect("/context usage percent should populate CTX");
        assert!((m.value - 0.82).abs() < f32::EPSILON);
        assert_eq!(m.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(m.provider, Some(Provider::Claude));
    }

    #[test]
    fn claude_context_usage_block_populates_context_pressure() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
Context Usage

Opus 4.7 (1M context)
claude-opus-4-7[1m]

143.3k/1m tokens (14%)

Estimated usage by category
System prompt: 8.6k tokens (0.9%)
Messages: 165.5k tokens (16.5%)
Free space: 776.5k (77.6%)";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        let m = set
            .context_pressure
            .expect("real Claude /context usage block should populate CTX");
        assert!((m.value - 0.14).abs() < f32::EPSILON);
        assert_eq!(m.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(m.provider, Some(Provider::Claude));
    }

    #[test]
    fn claude_context_left_is_converted_to_used_pressure() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            "Context remaining: 18% left",
            &pricing,
            &settings,
            &history,
        );
        let set = ClaudeAdapter.parse(&c);
        let m = set.context_pressure.expect("left percent should parse");
        assert!((m.value - 0.82).abs() < 1e-6);
    }

    #[test]
    fn claude_adapter_extracts_output_tokens_from_working_line() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail =
            "✶ Exploring adapter parsing surface… (1m 34s · ↓ 4.3k tokens · thought for 11s)";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        let m = set.token_count.expect("output tokens parsed");
        assert_eq!(m.value, 4_300);
        assert_eq!(m.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(m.provider, Some(Provider::Claude));
    }

    #[test]
    fn claude_adapter_prefers_subagent_done_line_over_working_line() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
✽ Exploring… (2m · ↓ 8.6k tokens)
  ⎿  Done (27 tool uses · 95.1k tokens · 1m 21s)";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        let m = set.token_count.expect("tokens parsed");
        assert_eq!(m.value, 95_100);
    }

    #[test]
    fn claude_adapter_returns_none_token_count_when_no_marker() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            "regular claude output with no token marker",
            &pricing,
            &settings,
            &history,
        );
        let set = ClaudeAdapter.parse(&c);
        assert!(set.token_count.is_none());
    }

    #[test]
    fn claude_adapter_never_populates_model_name_from_unstructured_tail() {
        // Honesty regression: with EMPTY settings, the Claude adapter
        // must not populate model_name from arbitrary prose or working
        // lines. The structured statusline path is tested separately.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            "✶ Working… (↓ 100 tokens)",
            &pricing,
            &settings,
            &history,
        );
        let set = ClaudeAdapter.parse(&c);
        assert!(
            set.model_name.is_none(),
            "Claude tail must not surface model_name without a ClaudeSettings source"
        );
    }

    #[test]
    fn claude_statusline_populates_model_context_quotas_effort_path_and_permission() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
Opus 4.7 (1M context)·max  CTX 51%  5h 9%  7d 1%  ~/Qmonster
│› Implement {feature}
⏵⏵ bypass permissions on (shift+tab to cycle)";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);

        let model = set.model_name.as_ref().expect("model from statusline");
        assert_eq!(model.value, "Opus 4.7 (1M context)");
        assert_eq!(model.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(model.provider, Some(Provider::Claude));

        assert!((set.context_pressure.as_ref().unwrap().value - 0.51).abs() < 1e-6);
        assert!((set.quota_5h_pressure.as_ref().unwrap().value - 0.09).abs() < 1e-6);
        assert!((set.quota_weekly_pressure.as_ref().unwrap().value - 0.01).abs() < 1e-6);
        assert_eq!(set.reasoning_effort.as_ref().unwrap().value, "max");
        assert_eq!(set.worktree_path.as_ref().unwrap().value, "~/Qmonster");
        assert!(set.runtime_facts.iter().any(|f| {
            f.kind == RuntimeFactKind::PermissionMode
                && f.value == "bypass permissions on"
                && f.source_kind == SourceKind::ProviderOfficial
        }));
    }

    #[test]
    fn claude_statusline_em_dash_ctx_resets_to_zero_after_clear() {
        // statusline.sh renders missing JSON fields as a dim em-dash.
        // After /clear, .context_window.used_percentage is absent so the
        // line shows `CTX —`. Treat that as a provider-visible reset to
        // 0% so the event-loop cache cannot replay the pre-clear CTX.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "Opus 4.7 (1M context)·max  CTX —  5h 88%  7d 43%  ~/Qmonster";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);

        assert!(
            (set.context_pressure.as_ref().unwrap().value - 0.0).abs() < 1e-6,
            "CTX em-dash after /clear must render as 0%, not replay the old cached value"
        );
        assert!((set.quota_5h_pressure.as_ref().unwrap().value - 0.88).abs() < 1e-6);
        assert!((set.quota_weekly_pressure.as_ref().unwrap().value - 0.43).abs() < 1e-6);
        assert_eq!(
            set.model_name.as_ref().unwrap().value,
            "Opus 4.7 (1M context)"
        );
        assert_eq!(set.reasoning_effort.as_ref().unwrap().value, "max");
        assert_eq!(set.worktree_path.as_ref().unwrap().value, "~/Qmonster");
    }

    #[test]
    fn claude_statusline_em_dash_5h_does_not_steal_7d_value() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "Opus 4.7 (1M context)·max  CTX 51%  5h —  7d 1%  ~/Qmonster";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);

        assert!((set.context_pressure.as_ref().unwrap().value - 0.51).abs() < 1e-6);
        assert!(
            set.quota_5h_pressure.is_none(),
            "5h em-dash must leave quota_5h_pressure unset, not bleed 7d's 1%"
        );
        assert!((set.quota_weekly_pressure.as_ref().unwrap().value - 0.01).abs() < 1e-6);
        assert_eq!(
            set.model_name.as_ref().unwrap().value,
            "Opus 4.7 (1M context)"
        );
        assert_eq!(set.worktree_path.as_ref().unwrap().value, "~/Qmonster");
    }

    #[test]
    fn claude_statusline_em_dash_7d_keeps_other_fields() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "Opus 4.7 (1M context)·max  CTX 51%  5h 9%  7d —  ~/Qmonster";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);

        assert!((set.context_pressure.as_ref().unwrap().value - 0.51).abs() < 1e-6);
        assert!((set.quota_5h_pressure.as_ref().unwrap().value - 0.09).abs() < 1e-6);
        assert!(
            set.quota_weekly_pressure.is_none(),
            "7d em-dash must leave quota_weekly_pressure unset"
        );
        assert_eq!(
            set.model_name.as_ref().unwrap().value,
            "Opus 4.7 (1M context)"
        );
        assert_eq!(set.reasoning_effort.as_ref().unwrap().value, "max");
        assert_eq!(set.worktree_path.as_ref().unwrap().value, "~/Qmonster");
    }

    #[test]
    fn claude_statusline_all_em_dashes_keeps_model_and_path() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "Opus 4.7 (1M context)·max  CTX —  5h —  7d —  ~/Qmonster";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);

        assert!((set.context_pressure.as_ref().unwrap().value - 0.0).abs() < 1e-6);
        assert!(set.quota_5h_pressure.is_none());
        assert!(set.quota_weekly_pressure.is_none());
        assert_eq!(
            set.model_name.as_ref().unwrap().value,
            "Opus 4.7 (1M context)"
        );
        assert_eq!(set.reasoning_effort.as_ref().unwrap().value, "max");
        assert_eq!(set.worktree_path.as_ref().unwrap().value, "~/Qmonster");
    }

    use std::io::Write;
    use tempfile::NamedTempFile;

    fn settings_with_model(m: &str) -> (ClaudeSettings, NamedTempFile) {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"model": "{}"}}"#, m).unwrap();
        let s = ClaudeSettings::load_from_path(f.path()).unwrap();
        (s, f)
    }

    #[test]
    fn claude_adapter_populates_model_name_when_settings_has_model() {
        let id = id();
        let pricing = PricingTable::empty();
        let (settings, _f) = settings_with_model("claude-sonnet-4-6");
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "any tail", &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);

        let m = set
            .model_name
            .as_ref()
            .expect("model populated from settings");
        assert_eq!(m.value, "claude-sonnet-4-6");
        assert_eq!(m.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(m.provider, Some(Provider::Claude));
        assert_eq!(
            m.confidence,
            Some(0.9),
            "confidence 0.9 < Codex's 0.95 because CLI flags can override settings"
        );
    }

    #[test]
    fn claude_adapter_populates_runtime_facts_from_settings() {
        let id = id();
        let pricing = PricingTable::empty();
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"{{"permissionMode":"bypassPermissions","allowedTools":["Read"],"disallowedTools":["Bash(rm *)"],"additionalDirectories":["/tmp/shared"]}}"#
        )
        .unwrap();
        let settings = ClaudeSettings::load_from_path(f.path()).unwrap();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "any tail", &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);

        assert!(set.runtime_facts.iter().any(|f| {
            f.kind == RuntimeFactKind::PermissionMode && f.value == "bypassPermissions"
        }));
        assert!(
            set.runtime_facts.iter().any(|f| {
                f.kind == RuntimeFactKind::AllowedDirectory && f.value == "/tmp/shared"
            })
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::LoadedTool && f.value == "Read")
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::RestrictedTool && f.value == "Bash(rm *)")
        );
        // CFX-1: settings file is operator-curated provider config —
        // these facts MUST stay ProviderOfficial. Test guards against
        // accidental demotion in future refactors.
        assert!(
            set.runtime_facts
                .iter()
                .all(|f| f.source_kind == SourceKind::ProviderOfficial),
            "all settings-derived runtime facts must be ProviderOfficial, got: {:?}",
            set.runtime_facts
                .iter()
                .map(|f| (f.kind, f.source_kind))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn claude_prose_with_runtime_labels_does_not_emit_provider_official_facts() {
        // CFX-1 regression: a transcript line such as
        // "Working directory: /tmp" or "Tools: foo, bar" is NOT a
        // structural Claude `/status` panel — it is ordinary chat
        // prose that happens to contain a `key: value` shape. Such
        // lines may still emit a fact for observability, but never
        // labelled ProviderOfficial.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
The agent says: Working directory: /tmp/example
Then it lists: Tools: Read, Edit
And asks about bypass permissions on macOS";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        for fact in &set.runtime_facts {
            assert_ne!(
                fact.source_kind,
                SourceKind::ProviderOfficial,
                "prose-derived fact {:?}={} must not be ProviderOfficial",
                fact.kind,
                fact.value
            );
        }
    }

    #[test]
    fn claude_adapter_leaves_cost_usd_none_regardless_of_settings() {
        // Honesty regression: Claude cost requires input-token data
        // which Claude's tail does not expose. Settings presence must
        // not accidentally unlock cost computation.
        let id = id();
        let pricing = PricingTable::empty();
        let (settings, _f) = settings_with_model("claude-sonnet-4-6");
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            "✶ Working… (↓ 100 tokens)",
            &pricing,
            &settings,
            &history,
        );
        let set = ClaudeAdapter.parse(&c);

        assert!(set.model_name.is_some(), "model populates from settings");
        assert!(
            set.cost_usd.is_none(),
            "cost must stay None — no input-token source on Claude tail"
        );
    }

    #[test]
    fn claude_idle_cursor_at_last_line_yields_work_complete() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "previous output\n\n❯ ";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::WorkComplete));
    }

    #[test]
    fn claude_prompt_echo_with_request_text_is_not_idle_cursor() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "❯ run the tests", &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn claude_current_session_100_used_yields_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "  Current session\n  ██████████ 100% used\n  Resets 3:40pm\n";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::LimitHit));
        let quota = set
            .quota_5h_pressure
            .expect("Current session maps to 5h quota");
        assert!((quota.value - 1.0).abs() < f32::EPSILON);
        assert_eq!(quota.source_kind, SourceKind::ProviderOfficial);
    }

    #[test]
    fn claude_current_week_100_used_yields_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "Current week (all models) 100% used (resets Apr 30)";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::LimitHit));
        let quota = set
            .quota_weekly_pressure
            .expect("Current week (all models) maps to weekly quota");
        assert!((quota.value - 1.0).abs() < f32::EPSILON);
        assert_eq!(quota.provider, Some(Provider::Claude));
    }

    #[test]
    fn claude_usage_limit_banner_beats_idle_cursor() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
Claude usage limit reached. Your limit will reset at 3:40pm.

❯ ";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::LimitHit));
    }

    #[test]
    fn claude_limit_reached_retry_after_beats_idle_cursor() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
Claude AI limit reached for this account.
Retry after 3 hours.

❯ ";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::LimitHit));
    }

    #[test]
    fn claude_usage_limit_prose_without_recovery_hint_does_not_fire_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "Please explain what a usage limit reached message means.";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn claude_tail_runtime_facts_include_permissions_tools_skills_and_plugins() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
⏵⏵ bypass permissions on
Allowed tools: Read, Edit
Disallowed tools: Bash(rm *)
Plugins: github
● Skill(superpowers:executing-plans)
  ⎿ Successfully loaded skill
● Bash(command: \"git status\")";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);

        assert!(set.runtime_facts.iter().any(|f| {
            f.kind == RuntimeFactKind::PermissionMode && f.value == "bypass permissions on"
        }));
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::LoadedTool && f.value == "Read")
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::RestrictedTool && f.value == "Bash(rm *)")
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::LoadedPlugin && f.value == "github")
        );
        assert!(set.runtime_facts.iter().any(|f| {
            f.kind == RuntimeFactKind::LoadedSkill && f.value == "superpowers:executing-plans"
        }));
        // CFX-1: free-form labeled tail-prose facts have no structural
        // panel anchor on Claude. The statusline permission hint is the
        // only ProviderOfficial value in this fixture.
        assert!(
            set.runtime_facts
                .iter()
                .filter(|f| f.kind != RuntimeFactKind::PermissionMode)
                .all(|f| f.source_kind == SourceKind::Heuristic),
            "free-form tail-derived runtime facts must be Heuristic, got: {:?}",
            set.runtime_facts
                .iter()
                .map(|f| (f.kind, f.source_kind))
                .collect::<Vec<_>>()
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::LoadedTool && f.value == "Bash (observed)")
        );
    }

    #[test]
    fn claude_active_output_no_cursor_no_history_yields_none() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "✶ Working… (↓ 4.3k tokens)";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn claude_permission_marker_beats_cursor_in_priority() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "❯ this action requires approval (y/n)";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(
            set.idle_state,
            Some(IdleCause::PermissionWait),
            "explicit marker must win over cursor pattern"
        );
    }

    #[test]
    fn claude_stillness_fallback_yields_stale_when_history_full() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let mut history = PaneTailHistory::empty();
        let tail = "stable output without markers or cursor";
        for _ in 0..STILLNESS_WINDOW {
            history.push(tail.into());
        }
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::Stale));
    }
}
