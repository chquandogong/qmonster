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
    parse_for_with_proc_root(ctx, std::path::Path::new("/proc"))
}

/// Test-seam variant: delegates to a caller-supplied proc root so unit
/// tests can exercise the `/proc` descendant-RSS fill path without
/// touching the real filesystem.
///
/// Production callers must use [`parse_for`] instead.
#[doc(hidden)]
pub fn parse_for_with_proc_root(ctx: &ParserContext, proc_root: &std::path::Path) -> SignalSet {
    let mut signals = match ctx.identity.identity.provider {
        Provider::Claude => claude::ClaudeAdapter.parse(ctx),
        Provider::Codex => codex::CodexAdapter.parse(ctx),
        Provider::Gemini => gemini::GeminiAdapter.parse(ctx),
        Provider::Qmonster => qmonster::QmonsterAdapter.parse(ctx),
        Provider::Unknown => common::parse_common_signals(ctx.tail),
    };
    // Phase F F-1: fill process_memory_mb from /proc descendant RSS
    // when the provider adapter left it None. Gemini's ProviderOfficial
    // path (parsed from its status table) is preserved by skipping the
    // fill on Some(_).
    if signals.process_memory_mb.is_none()
        && let Some(pid) = ctx.pane_pid
        && let Some(mb) = process_memory::read_descendant_rss_mb_with_proc_root(pid, proc_root)
    {
        signals.process_memory_mb = Some(crate::domain::signal::MetricValue::new(
            mb,
            crate::domain::origin::SourceKind::Heuristic,
        ));
    }
    signals
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
