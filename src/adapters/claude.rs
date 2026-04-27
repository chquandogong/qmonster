use crate::adapters::ProviderParser;
use crate::adapters::common::{PaneTailHistory, parse_common_signals, parse_count_with_suffix};
use crate::adapters::runtime::{push_provider_fact, split_runtime_list};
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{IdleCause, MetricValue, RuntimeFactKind, SignalSet};

pub struct ClaudeAdapter;

impl ProviderParser for ClaudeAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let tail = ctx.tail;
        let mut set = parse_common_signals(tail);
        append_claude_runtime_facts(&mut set, tail, ctx.claude_settings);

        if let Some(pct) = parse_claude_context_pressure(tail) {
            set.context_pressure = Some(
                MetricValue::new(pct, SourceKind::ProviderOfficial)
                    .with_confidence(0.9)
                    .with_provider(Provider::Claude),
            );
        }
        let (quota_5h, quota_weekly) = parse_claude_usage_quotas(tail);
        set.quota_5h_pressure = quota_5h.map(|pct| {
            MetricValue::new(pct, SourceKind::ProviderOfficial)
                .with_confidence(0.9)
                .with_provider(Provider::Claude)
        });
        set.quota_weekly_pressure = quota_weekly.map(|pct| {
            MetricValue::new(pct, SourceKind::ProviderOfficial)
                .with_confidence(0.9)
                .with_provider(Provider::Claude)
        });

        if let Some(n) = parse_claude_output_tokens(tail) {
            set.token_count = Some(
                MetricValue::new(n, SourceKind::ProviderOfficial)
                    .with_confidence(0.85)
                    .with_provider(Provider::Claude),
            );
        }

        // Slice 2: model from external ~/.claude/settings.json (not tail).
        // Confidence 0.9 (< Codex's 0.95) because CLI flags can override
        // the settings value at invocation time.
        if let Some(m) = ctx.claude_settings.model() {
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

fn parse_claude_context_pressure(tail: &str) -> Option<f32> {
    // Claude's `/context` output is not box-bordered like Codex, so keep
    // this anchored to context-labeled percent lines and explicitly avoid
    // `/usage` quota labels. Prefer "used" when both used/left appear.
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
        if lower.contains("bypass permissions on") {
            push_provider_fact(
                facts,
                Provider::Claude,
                RuntimeFactKind::PermissionMode,
                "bypass permissions on",
                0.95,
                SourceKind::Heuristic,
            );
        } else if lower.contains("bypass permissions off") {
            push_provider_fact(
                facts,
                Provider::Claude,
                RuntimeFactKind::PermissionMode,
                "bypass permissions off",
                0.95,
                SourceKind::Heuristic,
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
    fn claude_adapter_never_populates_model_name_from_tail() {
        // Honesty regression: with EMPTY settings, the Claude adapter
        // must not populate model_name from the tail alone. Claude's
        // tail does not expose the model; only the settings-read path
        // (tested separately) may populate this field.
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
        // CFX-1: tail-prose facts have no structural panel anchor on
        // Claude — they must label as Heuristic, not ProviderOfficial.
        assert!(
            set.runtime_facts
                .iter()
                .all(|f| f.source_kind == SourceKind::Heuristic),
            "all tail-derived runtime facts must be Heuristic, got: {:?}",
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
