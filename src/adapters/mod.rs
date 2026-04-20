pub mod claude;
pub mod codex;
pub mod common;
pub mod gemini;
pub mod qmonster;

use crate::domain::identity::{Provider, ResolvedIdentity};
use crate::domain::signal::SignalSet;

/// Provider-specific parser. Each adapter receives a resolved identity
/// plus the raw pane tail and emits typed signals. Identity inference
/// never happens here (r2 non-negotiable; see ARCHITECTURE.md).
pub trait ProviderParser {
    fn parse(&self, identity: &ResolvedIdentity, tail: &str) -> SignalSet;
}

/// Dispatch helper — pick the right adapter by provider.
pub fn parse_for(identity: &ResolvedIdentity, tail: &str) -> SignalSet {
    match identity.identity.provider {
        Provider::Claude => claude::ClaudeAdapter.parse(identity, tail),
        Provider::Codex => codex::CodexAdapter.parse(identity, tail),
        Provider::Gemini => gemini::GeminiAdapter.parse(identity, tail),
        Provider::Qmonster => qmonster::QmonsterAdapter.parse(identity, tail),
        Provider::Unknown => common::parse_common_signals(tail),
    }
}

pub use common::parse_common_signals;
