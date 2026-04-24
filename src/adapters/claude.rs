use crate::adapters::ProviderParser;
use crate::adapters::common::{parse_common_signals, parse_count_with_suffix};
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, SignalSet};

pub struct ClaudeAdapter;

impl ProviderParser for ClaudeAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let tail = ctx.tail;
        let mut set = parse_common_signals(tail);
        let lower = tail.to_lowercase();

        if let Some(p) = parse_context_percent_claude(&lower) {
            set.context_pressure = Some(
                MetricValue::new(p / 100.0, SourceKind::Estimated)
                    .with_confidence(0.6)
                    .with_provider(Provider::Claude),
            );
        }

        if let Some(n) = parse_claude_output_tokens(tail) {
            set.token_count = Some(
                MetricValue::new(n, SourceKind::ProviderOfficial)
                    .with_confidence(0.85)
                    .with_provider(Provider::Claude),
            );
        }

        set
    }
}

fn parse_context_percent_claude(lower: &str) -> Option<f32> {
    for line in lower.lines() {
        if line.contains("claude") && line.contains('%') {
            let mut digits = String::new();
            let mut seen_dot = false;
            for ch in line.chars() {
                if ch.is_ascii_digit() {
                    digits.push(ch);
                } else if ch == '.' && !seen_dot {
                    digits.push(ch);
                    seen_dot = true;
                } else if ch == '%' {
                    if let Ok(v) = digits.parse::<f32>() {
                        return Some(v);
                    }
                    digits.clear();
                    seen_dot = false;
                } else {
                    digits.clear();
                    seen_dot = false;
                }
            }
        }
    }
    None
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
    ) -> ParserContext<'a> {
        ParserContext {
            identity: id,
            tail,
            pricing,
            claude_settings: settings,
        }
    }

    #[test]
    fn claude_adapter_inherits_common_signals() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, "Press ENTER to continue", &pricing, &settings);
        let set = ClaudeAdapter.parse(&c);
        assert!(set.waiting_for_input);
    }

    #[test]
    fn claude_adapter_parses_claude_specific_percent() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, "claude context 88%", &pricing, &settings);
        let set = ClaudeAdapter.parse(&c);
        let m = set.context_pressure.expect("parsed");
        assert!((m.value - 0.88).abs() < 0.01);
        assert_eq!(m.source_kind, SourceKind::Estimated);
        assert_eq!(m.confidence, Some(0.6));
        assert_eq!(m.provider, Some(Provider::Claude));
    }

    #[test]
    fn claude_adapter_extracts_output_tokens_from_working_line() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let tail =
            "✶ Exploring adapter parsing surface… (1m 34s · ↓ 4.3k tokens · thought for 11s)";
        let c = ctx(&id, tail, &pricing, &settings);
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
        let tail = "\
✽ Exploring… (2m · ↓ 8.6k tokens)
  ⎿  Done (27 tool uses · 95.1k tokens · 1m 21s)";
        let c = ctx(&id, tail, &pricing, &settings);
        let set = ClaudeAdapter.parse(&c);
        let m = set.token_count.expect("tokens parsed");
        assert_eq!(m.value, 95_100);
    }

    #[test]
    fn claude_adapter_returns_none_token_count_when_no_marker() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let c = ctx(
            &id,
            "regular claude output with no token marker",
            &pricing,
            &settings,
        );
        let set = ClaudeAdapter.parse(&c);
        assert!(set.token_count.is_none());
    }

    #[test]
    fn claude_adapter_never_populates_model_name_from_tail() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, "✶ Working… (↓ 100 tokens)", &pricing, &settings);
        let set = ClaudeAdapter.parse(&c);
        assert!(
            set.model_name.is_none(),
            "Claude model is not parseable in Slice 1"
        );
        assert!(
            set.cost_usd.is_none(),
            "Claude cost requires input tokens which Claude tail does not expose"
        );
    }
}
