pub mod claude;
pub mod codex;
pub mod common;
pub mod gemini;
pub mod process_memory;
pub mod qmonster;
mod runtime;

use crate::adapters::common::PaneTailHistory;
use crate::domain::identity::{Provider, ResolvedIdentity};
use crate::domain::signal::SignalSet;
use crate::policy::claude_settings::ClaudeSettings;
use crate::policy::pricing::PricingTable;

/// Inputs the adapter layer needs when producing a SignalSet from a
/// pane tail. The struct keeps the trait method signature stable as
/// Slice 3+ introduce more cross-cutting observability inputs.
pub struct ParserContext<'a> {
    pub identity: &'a ResolvedIdentity,
    pub tail: &'a str,
    pub pricing: &'a PricingTable,
    pub claude_settings: &'a ClaudeSettings,
    pub history: &'a PaneTailHistory,
    /// Phase F F-1: tmux `#{pane_pid}` for descendant-RSS lookup. May
    /// be `None` for legacy fixtures or when tmux emitted a non-integer
    /// value. Consumed by `parse_for` to fill `SignalSet.process_memory_mb`
    /// via `process_memory::read_descendant_rss_mb` when no
    /// provider-native memory signal was emitted.
    pub pane_pid: Option<u32>,
}

/// Provider-specific parser. Each adapter receives a ParserContext
/// bundle and emits typed signals. Identity inference never happens
/// here (r2 non-negotiable; see ARCHITECTURE.md).
pub trait ProviderParser {
    fn parse(&self, ctx: &ParserContext) -> SignalSet;
}

/// Dispatch helper — pick the right adapter by provider.
pub fn parse_for(ctx: &ParserContext) -> SignalSet {
    match ctx.identity.identity.provider {
        Provider::Claude => claude::ClaudeAdapter.parse(ctx),
        Provider::Codex => codex::CodexAdapter.parse(ctx),
        Provider::Gemini => gemini::GeminiAdapter.parse(ctx),
        Provider::Qmonster => qmonster::QmonsterAdapter.parse(ctx),
        Provider::Unknown => common::parse_common_signals(ctx.tail),
    }
}

pub use common::parse_common_signals;

/// Test-only constructor for `ParserContext`. Hoisted out of each
/// adapter's `mod tests` to remove the 4-way duplication that the
/// Slice 3 housekeeping bundle flagged.
#[cfg(test)]
pub(crate) fn ctx<'a>(
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
        pane_pid: None,
    }
}
