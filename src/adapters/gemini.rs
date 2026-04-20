use crate::adapters::ProviderParser;
use crate::adapters::common::parse_common_signals;
use crate::domain::identity::ResolvedIdentity;
use crate::domain::signal::SignalSet;

pub struct GeminiAdapter;

impl ProviderParser for GeminiAdapter {
    fn parse(&self, _identity: &ResolvedIdentity, tail: &str) -> SignalSet {
        parse_common_signals(tail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};

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

    #[test]
    fn gemini_adapter_inherits_subagent_hint() {
        let set = GeminiAdapter.parse(&id(), "Starting subagent: web-explorer");
        assert!(set.subagent_hint);
    }
}
