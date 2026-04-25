use crate::adapters::ProviderParser;
use crate::adapters::common::parse_common_signals;
use crate::domain::signal::{IdleCause, SignalSet};

pub struct GeminiAdapter;

impl ProviderParser for GeminiAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let mut set = parse_common_signals(ctx.tail);
        if set.idle_state.is_none() {
            set.idle_state = classify_idle_gemini(ctx.tail, ctx.history);
        }
        set
    }
}

fn classify_idle_gemini(
    tail: &str,
    history: &crate::adapters::common::PaneTailHistory,
) -> Option<IdleCause> {
    if gemini_limit_hit(tail) {
        return Some(IdleCause::LimitHit);
    }
    if gemini_idle_cursor(tail) {
        return Some(IdleCause::WorkComplete);
    }
    if history.is_still(crate::adapters::common::STILLNESS_WINDOW) {
        return Some(IdleCause::Stale);
    }
    None
}

/// True when any line in the tail is Gemini's input placeholder.
/// The `*  ` prefix (asterisk + 2 spaces) is the prompt glyph Gemini
/// CLI renders when it is waiting for the next user message.
///
/// We scan the whole tail rather than checking the last non-empty line
/// because in production Gemini always renders a 2-row status bar
/// (column headers + data row) below the placeholder, pushing the
/// placeholder out of last-line position.  The placeholder appears
/// ONLY in idle state, so scanning all lines is safe.
fn gemini_idle_cursor(tail: &str) -> bool {
    tail.lines().any(|line| {
        let t = line.trim_start();
        t.starts_with("*  ") && t.contains("Type your message")
    })
}

/// True when the Gemini status block reports quota exhaustion.
///
/// The detection is intentionally narrow: a header row must contain ALL
/// of `quota`, `context`, and `memory` (the three Gemini status column
/// titles) AND the very next non-empty row must contain `100% used`.
/// Without all three header words the signal does not fire, which
/// prevents bare prose like "the quota was 100% used yesterday" from
/// triggering a spurious LimitHit.
fn gemini_limit_hit(tail: &str) -> bool {
    let lines: Vec<&str> = tail.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        if lower.contains("quota") && lower.contains("context") && lower.contains("memory") {
            for next in lines.iter().skip(i + 1) {
                let trimmed = next.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.contains("100% used") {
                    return true;
                }
                break;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::ParserContext;
    use crate::adapters::common::PaneTailHistory;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::signal::IdleCause;
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
    fn gemini_adapter_inherits_subagent_hint() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "Starting subagent: web-explorer", &pricing, &settings, &history);
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
}
