pub mod agent_memory;
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
    /// Phase F F-2 (v1.23.0): pane's current working directory, used
    /// to discover provider-specific memory files at the project
    /// root. Empty when the pane has no resolved cwd. Consumed by
    /// `parse_for_with_environment` (added in Task 3) to fill
    /// `SignalSet.agent_memory_bytes` via
    /// `agent_memory::read_agent_memory_bytes_with_filesystem`.
    pub current_path: &'a str,
}

/// Provider-specific parser. Each adapter receives a ParserContext
/// bundle and emits typed signals. Identity inference never happens
/// here (r2 non-negotiable; see ARCHITECTURE.md).
pub trait ProviderParser {
    fn parse(&self, ctx: &ParserContext) -> SignalSet;
}

/// Dispatch helper — pick the right adapter by provider.
pub fn parse_for(ctx: &ParserContext) -> SignalSet {
    let home = directories::BaseDirs::new().map(|bd| bd.home_dir().to_path_buf());
    parse_for_with_environment(ctx, std::path::Path::new("/proc"), home.as_deref())
}

/// Test seam — accepts an alternate `/proc` root and an explicit
/// `home_dir`. Tests inject both via tempdir-rooted trees. F-1's
/// `parse_for_with_proc_root` is preserved as a backward-compat
/// shim that calls this with `home_dir = None`.
#[doc(hidden)]
pub fn parse_for_with_environment(
    ctx: &ParserContext,
    proc_root: &std::path::Path,
    home_dir: Option<&std::path::Path>,
) -> SignalSet {
    let mut signals = match ctx.identity.identity.provider {
        Provider::Claude => claude::ClaudeAdapter.parse(ctx),
        Provider::Codex => codex::CodexAdapter.parse(ctx),
        Provider::Gemini => gemini::GeminiAdapter.parse(ctx),
        Provider::Qmonster => qmonster::QmonsterAdapter.parse(ctx),
        Provider::Unknown => common::parse_common_signals(ctx.tail),
    };
    // F-1: process_memory_mb fill from /proc descendant RSS when the
    // adapter left it None (Gemini's status-table ProviderOfficial
    // path is preserved by the is_none() guard).
    if signals.process_memory_mb.is_none()
        && let Some(pid) = ctx.pane_pid
        && let Some(mb) = process_memory::read_descendant_rss_mb_with_proc_root(pid, proc_root)
    {
        signals.process_memory_mb = Some(crate::domain::signal::MetricValue::new(
            mb,
            crate::domain::origin::SourceKind::Heuristic,
        ));
    }
    // F-2: agent_memory_bytes fill from filesystem scan. Always
    // Heuristic (file existence is observation, not load
    // confirmation). Fill skipped when current_path is empty so the
    // global ~/.claude/CLAUDE.md cannot be mis-attributed to a pane
    // without a resolved cwd.
    if signals.agent_memory_bytes.is_none()
        && !ctx.current_path.is_empty()
        && let Some(bytes) = agent_memory::read_agent_memory_bytes_with_filesystem(
            ctx.identity.identity.provider,
            std::path::Path::new(ctx.current_path),
            home_dir,
        )
    {
        signals.agent_memory_bytes = Some(crate::domain::signal::MetricValue::new(
            bytes,
            crate::domain::origin::SourceKind::Heuristic,
        ));
    }
    signals
}

/// Backward-compat shim: F-1 callers (existing tests) can keep using
/// the `parse_for_with_proc_root` signature. New code should use
/// `parse_for_with_environment` so it can also exercise the
/// agent-memory fill path with a fake home dir.
#[doc(hidden)]
pub fn parse_for_with_proc_root(ctx: &ParserContext, proc_root: &std::path::Path) -> SignalSet {
    parse_for_with_environment(ctx, proc_root, None)
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
        current_path: "", // F-2: test fixture; production wires from snapshot.current_path
    }
}
