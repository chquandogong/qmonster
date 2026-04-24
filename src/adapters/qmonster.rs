use crate::adapters::ProviderParser;
use crate::domain::signal::SignalSet;

pub struct QmonsterAdapter;

impl ProviderParser for QmonsterAdapter {
    /// The monitor pane is its own pane — we do not run the generic
    /// parser over its heartbeat output. Phase 1 returns an empty
    /// signal set; Phase 2+ will surface self-heartbeat metrics.
    fn parse(&self, _ctx: &crate::adapters::ParserContext) -> SignalSet {
        SignalSet::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::ParserContext;
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
    ) -> ParserContext<'a> {
        ParserContext {
            identity: id,
            tail,
            pricing,
            claude_settings: settings,
        }
    }

    #[test]
    fn qmonster_adapter_returns_empty_signals() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, "heartbeat tick 42", &pricing, &settings);
        let set = QmonsterAdapter.parse(&c);
        assert!(!set.waiting_for_input);
        assert!(!set.log_storm);
    }
}
