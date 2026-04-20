use crate::adapters::ProviderParser;
use crate::adapters::common::parse_common_signals;
use crate::domain::identity::ResolvedIdentity;
use crate::domain::signal::SignalSet;

pub struct CodexAdapter;

impl ProviderParser for CodexAdapter {
    fn parse(&self, _identity: &ResolvedIdentity, tail: &str) -> SignalSet {
        // Phase 1: Codex-specific parsing is light. The common layer
        // already covers the alerts we ship in Phase 1.
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
                provider: Provider::Codex,
                instance: 1,
                role: Role::Review,
                pane_id: "%2".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    #[test]
    fn codex_adapter_detects_permission_prompt() {
        let set = CodexAdapter.parse(&id(), "This action requires approval");
        assert!(set.permission_prompt);
    }
}
