pub mod claude;
pub mod codex;
pub mod common;
pub mod gemini;
pub mod qmonster;

use crate::domain::identity::{Provider, ResolvedIdentity};
use crate::domain::signal::SignalSet;
use crate::policy::pricing::PricingTable;

/// Provider-specific parser. Each adapter receives a resolved identity
/// plus the raw pane tail and emits typed signals. Identity inference
/// never happens here (r2 non-negotiable; see ARCHITECTURE.md).
pub trait ProviderParser {
    fn parse(&self, identity: &ResolvedIdentity, tail: &str, pricing: &PricingTable) -> SignalSet;
}

/// Dispatch helper — pick the right adapter by provider.
pub fn parse_for(identity: &ResolvedIdentity, tail: &str, pricing: &PricingTable) -> SignalSet {
    match identity.identity.provider {
        Provider::Claude => claude::ClaudeAdapter.parse(identity, tail, pricing),
        Provider::Codex => codex::CodexAdapter.parse(identity, tail, pricing),
        Provider::Gemini => gemini::GeminiAdapter.parse(identity, tail, pricing),
        Provider::Qmonster => qmonster::QmonsterAdapter.parse(identity, tail, pricing),
        Provider::Unknown => common::parse_common_signals(tail),
    }
}

pub use common::parse_common_signals;
