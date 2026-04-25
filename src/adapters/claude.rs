use crate::adapters::ProviderParser;
use crate::adapters::common::{parse_common_signals, parse_count_with_suffix, PaneTailHistory, STILLNESS_WINDOW};
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{IdleCause, MetricValue, SignalSet};

pub struct ClaudeAdapter;

impl ProviderParser for ClaudeAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let tail = ctx.tail;
        let mut set = parse_common_signals(tail);

        // v1.13.1: parse_context_percent_claude dropped — its substring
        // matching of `claude` + `%` fired on operator prose mentioning
        // Claude's percent share, and on Claude's own /status panel
        // bars (`Current week 9% used`) which are rate-limit windows,
        // not context-window pressure. context_pressure for Claude is
        // S3-4 territory (read from ~/.claude/state when shipped, or
        // leave None — honest).

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
    if history.is_still(STILLNESS_WINDOW) {
        return Some(IdleCause::Stale);
    }
    None
}

fn claude_idle_cursor(tail: &str) -> bool {
    // The last non-empty line starts with `❯ ` (Claude prompt-ready
    // glyph + space). The space distinguishes from `❯/status`-style
    // command echoes which have no trailing whitespace before content.
    let last = tail.lines().rev().find(|l| !l.trim().is_empty());
    last.is_some_and(|l| {
        let t = l.trim_start();
        t.starts_with("❯ ")
    })
}

fn claude_limit_hit(tail: &str) -> bool {
    // Pattern A: two-line `Current session\n... 100% used` from /status.
    // Pattern B: same-line `Current week (...) 100% used`.
    let lines: Vec<&str> = tail.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
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
    use crate::adapters::ParserContext;
    use crate::adapters::common::{PaneTailHistory, STILLNESS_WINDOW};
    use crate::domain::signal::IdleCause;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
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

    fn ctx<'a>(
        id: &'a ResolvedIdentity,
        tail: &'a str,
        pricing: &'a PricingTable,
        settings: &'a ClaudeSettings,
        history: &'a PaneTailHistory,
    ) -> ParserContext<'a> {
        ParserContext {
            identity: id,
            tail,
            pricing,
            claude_settings: settings,
            history,
        }
    }

    #[test]
    fn claude_adapter_inherits_common_signals() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "Press ENTER to continue", &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert!(set.waiting_for_input);
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
        let c = ctx(&id, "✶ Working… (↓ 100 tokens)", &pricing, &settings, &history);
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
    fn claude_adapter_leaves_cost_usd_none_regardless_of_settings() {
        // Honesty regression: Claude cost requires input-token data
        // which Claude's tail does not expose. Settings presence must
        // not accidentally unlock cost computation.
        let id = id();
        let pricing = PricingTable::empty();
        let (settings, _f) = settings_with_model("claude-sonnet-4-6");
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "✶ Working… (↓ 100 tokens)", &pricing, &settings, &history);
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
    fn claude_current_session_100_used_yields_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "  Current session\n  ██████████ 100% used\n  Resets 3:40pm\n";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = ClaudeAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::LimitHit));
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
        assert_eq!(set.idle_state, Some(IdleCause::PermissionWait),
            "explicit marker must win over cursor pattern");
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
