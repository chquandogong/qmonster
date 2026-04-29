pub mod agent_memory;
pub mod claude;
pub mod claude_sidefile;
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
    // without a resolved cwd. The is_none() guard mirrors F-1's
    // pattern: if a future adapter parses agent_memory_bytes from a
    // provider-native source (e.g., Codex /memory output), that
    // stronger SourceKind must not be clobbered by this Heuristic
    // fallback.
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
    // Phase F F-5b (v1.31.0): for Claude panes only, enrich the
    // SignalSet with raw token counts, cost, reset eta, and session
    // identity from the sidefile JSON dropped by the recommended
    // statusline. Sidefile-on-default (G-2) means most operators have
    // the file available; absence is silent (best-effort enrichment).
    if matches!(ctx.identity.identity.provider, Provider::Claude)
        && !ctx.current_path.is_empty()
        && let Some(home) = home_dir
        && let Some(sidefile) = claude_sidefile::read_sidefile_for_path(home, ctx.current_path)
    {
        apply_claude_sidefile(&mut signals, sidefile);
    }
    signals
}

/// Phase F F-5b (v1.31.0): copy sidefile fields into the SignalSet
/// using `is_none()` guards so the statusline path's values are
/// preserved when both surfaces populate the same field. Cache hit
/// ratio is the one exception — sidefile carries raw cached/input
/// counts, so its computed ratio is more precise than the
/// rounded-to-integer `cache N%` parsed from the visible statusline,
/// and we always overwrite when the sidefile path produces a value.
fn apply_claude_sidefile(
    signals: &mut crate::domain::signal::SignalSet,
    sidefile: claude_sidefile::ClaudeSidefile,
) {
    use crate::domain::identity::Provider;
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{MetricValue, RuntimeFact, RuntimeFactKind};

    fn metric<T>(v: T) -> MetricValue<T> {
        MetricValue::new(v, SourceKind::ProviderOfficial)
            .with_confidence(0.95)
            .with_provider(Provider::Claude)
    }

    if let Some(usage) = sidefile
        .context_window
        .as_ref()
        .and_then(|cw| cw.current_usage.as_ref())
    {
        if signals.input_tokens.is_none()
            && let Some(n) = usage.input_tokens
        {
            signals.input_tokens = Some(metric(n));
        }
        if signals.output_tokens.is_none()
            && let Some(n) = usage.output_tokens
        {
            signals.output_tokens = Some(metric(n));
        }
        if signals.cached_input_tokens.is_none()
            && let Some(n) = usage.cache_read_input_tokens
        {
            signals.cached_input_tokens = Some(metric(n));
        }
    }
    // Override cache_hit_ratio when the sidefile has raw counts — the
    // sidefile-derived ratio is more precise than the statusline's
    // rounded-to-integer `cache N%` and reliably reflects the live
    // current_usage instead of a previous-turn snapshot.
    if let Some(ratio) = claude_sidefile::cache_hit_ratio(&sidefile) {
        signals.cache_hit_ratio = Some(metric(ratio));
    }
    if signals.cost_usd.is_none()
        && let Some(usd) = sidefile.cost.as_ref().and_then(|c| c.total_cost_usd)
    {
        signals.cost_usd = Some(metric(usd));
    }
    if let Some(rl) = sidefile.rate_limits.as_ref() {
        if signals.quota_5h_resets_at.is_none()
            && let Some(ts) = rl.five_hour.as_ref().and_then(|w| w.resets_at)
        {
            signals.quota_5h_resets_at = Some(metric(ts));
        }
        if signals.quota_weekly_resets_at.is_none()
            && let Some(ts) = rl.seven_day.as_ref().and_then(|w| w.resets_at)
        {
            signals.quota_weekly_resets_at = Some(metric(ts));
        }
    }
    if let Some(sid) = sidefile.session_id.as_ref() {
        signals.runtime_facts.push(
            RuntimeFact::new(
                RuntimeFactKind::SessionId,
                sid,
                SourceKind::ProviderOfficial,
            )
            .with_provider(Provider::Claude)
            .with_confidence(0.95),
        );
    }
    if let Some(tp) = sidefile.transcript_path.as_ref() {
        signals.runtime_facts.push(
            RuntimeFact::new(
                RuntimeFactKind::TranscriptPath,
                tp,
                SourceKind::ProviderOfficial,
            )
            .with_provider(Provider::Claude)
            .with_confidence(0.95),
        );
    }
}

/// Backward-compat shim — prefer `parse_for_with_environment` for
/// new tests. F-1 callers can keep using this signature; passing
/// `home_dir = None` short-circuits the F-2 agent-memory fill,
/// which is exactly the behavior F-1 tests expect.
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

#[cfg(test)]
mod sidefile_integration_tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Role};
    use crate::domain::signal::RuntimeFactKind;
    use std::fs;
    use tempfile::tempdir;

    fn claude_id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    fn write_sidefile_for(home: &std::path::Path, sid: &str, body: &str) {
        let dir = home.join(".local/share/ai-cli-status/claude");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{sid}.json")), body).unwrap();
    }

    #[test]
    fn sidefile_enriches_claude_pane_with_raw_token_counts_cost_and_resets_at() {
        let tmp = tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_sidefile_for(
            tmp.path(),
            "abc",
            &format!(
                r#"{{
                    "session_id": "abc",
                    "transcript_path": "/home/u/.claude/projects/x/abc.jsonl",
                    "cwd": "{cwd}",
                    "cost": {{"total_cost_usd": 12.34}},
                    "context_window": {{
                        "current_usage": {{
                            "input_tokens": 50000,
                            "output_tokens": 8,
                            "cache_creation_input_tokens": 770,
                            "cache_read_input_tokens": 150000
                        }}
                    }},
                    "rate_limits": {{
                        "five_hour": {{"resets_at": 1700000000}},
                        "seven_day": {{"resets_at": 1700100000}}
                    }}
                }}"#
            ),
        );

        let id = claude_id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let proc_root = tmp.path().join("proc-empty");
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        // Raw token counts surface for F-3 sparkline + future F-7 precision.
        assert_eq!(signals.input_tokens.as_ref().unwrap().value, 50_000);
        assert_eq!(signals.output_tokens.as_ref().unwrap().value, 8);
        assert_eq!(signals.cached_input_tokens.as_ref().unwrap().value, 150_000);
        // cost_usd surfaces from the sidefile (Claude statusline doesn't carry it).
        assert!((signals.cost_usd.as_ref().unwrap().value - 12.34).abs() < 1e-9);
        // cache_hit_ratio overrides the rounded statusline value with the
        // precise count-derived ratio: 150000 / (150000 + 50000) = 0.75.
        assert!((signals.cache_hit_ratio.as_ref().unwrap().value - 0.75).abs() < 1e-9);
        // Resets feed the new `5h resets in <eta>` text.
        assert_eq!(
            signals.quota_5h_resets_at.as_ref().unwrap().value,
            1_700_000_000
        );
        assert_eq!(
            signals.quota_weekly_resets_at.as_ref().unwrap().value,
            1_700_100_000
        );
        // RuntimeFacts carry session identity + transcript pointer.
        assert!(
            signals
                .runtime_facts
                .iter()
                .any(|f| { f.kind == RuntimeFactKind::SessionId && f.value == "abc" })
        );
        assert!(signals.runtime_facts.iter().any(|f| {
            f.kind == RuntimeFactKind::TranscriptPath
                && f.value == "/home/u/.claude/projects/x/abc.jsonl"
        }));
    }

    #[test]
    fn sidefile_skipped_when_current_path_does_not_match_any_cwd() {
        let tmp = tempdir().unwrap();
        write_sidefile_for(
            tmp.path(),
            "abc",
            r#"{"cwd":"/elsewhere","session_id":"abc","cost":{"total_cost_usd":99.0}}"#,
        );
        let id = claude_id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let proc_root = tmp.path().join("proc-empty");
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = "/repo/other";
        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));
        assert!(signals.cost_usd.is_none(), "no cwd match → no enrichment");
        assert!(signals.cached_input_tokens.is_none());
    }

    #[test]
    fn sidefile_skipped_for_non_claude_provider() {
        let tmp = tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_sidefile_for(
            tmp.path(),
            "abc",
            &format!(r#"{{"cwd":"{cwd}","session_id":"abc","cost":{{"total_cost_usd":99.0}}}}"#),
        );
        // Codex pane with the same cwd must not pick up Claude's
        // sidefile — provider-specific surface stays scoped to its
        // own adapter so a Codex pane's metrics never inherit Claude
        // session state.
        let id = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Codex,
                instance: 1,
                role: Role::Main,
                pane_id: "%2".into(),
            },
            confidence: IdentityConfidence::High,
        };
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let proc_root = tmp.path().join("proc-empty");
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));
        assert!(signals.cost_usd.is_none());
    }
}
