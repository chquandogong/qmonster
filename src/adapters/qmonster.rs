use crate::adapters::ProviderParser;
use crate::domain::identity::ResolvedIdentity;
use crate::domain::signal::SignalSet;
use crate::policy::pricing::PricingTable;

pub struct QmonsterAdapter;

impl ProviderParser for QmonsterAdapter {
    /// The monitor pane is its own pane — we do not run the generic
    /// parser over its heartbeat output. Phase 1 returns an empty
    /// signal set; Phase 2+ will surface self-heartbeat metrics.
    fn parse(
        &self,
        _identity: &ResolvedIdentity,
        _tail: &str,
        _pricing: &PricingTable,
    ) -> SignalSet {
        SignalSet::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};
    use crate::policy::pricing::PricingTable;

    #[test]
    fn qmonster_adapter_returns_empty_signals() {
        let id = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Qmonster,
                instance: 1,
                role: Role::Monitor,
                pane_id: "%4".into(),
            },
            confidence: IdentityConfidence::High,
        };
        let set = QmonsterAdapter.parse(&id, "heartbeat tick 42", &PricingTable::empty());
        assert!(!set.waiting_for_input);
        assert!(!set.log_storm);
    }
}
