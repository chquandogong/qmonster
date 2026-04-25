use crate::adapters::common::parse_common_signals;
use crate::adapters::ProviderParser;
use crate::domain::signal::SignalSet;

pub struct QmonsterAdapter;

impl ProviderParser for QmonsterAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let mut set = parse_common_signals(ctx.tail);
        // Slice 4: self-monitor pane does not fire idle alerts on itself.
        // Override any common-tier marker hit.
        set.idle_state = None;
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::ParserContext;
    use crate::adapters::common::PaneTailHistory;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::policy::claude_settings::ClaudeSettings;
    use crate::policy::pricing::PricingTable;

    fn id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Qmonster,
                instance: 1,
                role: Role::Monitor,
                pane_id: "%4".into(),
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
    fn qmonster_adapter_returns_empty_signals() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "heartbeat tick 42", &pricing, &settings, &history);
        let set = QmonsterAdapter.parse(&c);
        assert!(set.idle_state.is_none());
        assert!(!set.log_storm);
    }

    #[test]
    fn qmonster_adapter_never_emits_idle_state_even_with_markers() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        // Even if tail has prompt markers, qmonster pane (self-monitor)
        // should not fire an idle state — we don't alert ourselves.
        let tail = "this action requires approval (y/n)\n*  Type your message";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = QmonsterAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }
}
