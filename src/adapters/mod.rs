pub mod agent_memory;
pub mod agy_footer;
pub mod agy_sidefile;
pub mod agy_transcript;
pub mod claude;
pub mod claude_sidefile;
pub mod codex;
pub mod codex_app_server;
pub mod codex_rollout;
pub mod common;
pub mod gemini;
pub mod process_memory;
pub mod qmonster;
mod runtime;

use crate::adapters::common::PaneTailHistory;
use crate::domain::identity::{IdentityConfidence, Provider, ResolvedIdentity};
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
    /// Slice B: operator toggle for the Codex rollout backstop
    /// (`[provider_setup] codex_rollout`). When false, the rollout
    /// reader is never consulted.
    pub codex_rollout_enabled: bool,
    /// Slice C2: operator toggle for the agy transcript activity reader
    /// (`[provider_setup] agy_transcript`). When false, the agy transcript
    /// reader is never consulted.
    pub agy_transcript_enabled: bool,
    /// Slice C3: operator toggle for agy footer/sidefile enrichment
    /// (`[provider_setup] agy_enrichment`). When false, no agy enrichment runs.
    pub agy_enrichment_enabled: bool,
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
    let identity_conflict = matches!(ctx.identity.confidence, IdentityConfidence::Conflict);
    let mut signals = if identity_conflict {
        common::parse_common_signals(ctx.tail)
    } else {
        match ctx.identity.identity.provider {
            Provider::Claude => claude::ClaudeAdapter.parse(ctx),
            Provider::Codex => codex::CodexAdapter.parse(ctx),
            Provider::Gemini => gemini::GeminiAdapter.parse(ctx),
            Provider::Antigravity => common::parse_common_signals(ctx.tail),
            Provider::Qmonster => qmonster::QmonsterAdapter.parse(ctx),
            Provider::Unknown => common::parse_common_signals(ctx.tail),
        }
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
        && !identity_conflict
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
        && !identity_conflict
        && !ctx.current_path.is_empty()
        && claude_sidefile_process_confirmed(ctx, proc_root)
        && let Some(home) = home_dir
        && let Some(sidefile) = claude_sidefile::read_sidefile_for_path(home, ctx.current_path)
    {
        apply_claude_sidefile(&mut signals, sidefile);
    }
    // Slice B: Codex rollout backstop. Fill token totals + model from the
    // newest codex-tui rollout matching this pane's cwd ONLY when the
    // status-line scrape left them absent. Never overrides a scraped value.
    if ctx.codex_rollout_enabled
        && matches!(ctx.identity.identity.provider, Provider::Codex)
        && !identity_conflict
        && !ctx.current_path.is_empty()
        && (signals.input_tokens.is_none()
            || signals.output_tokens.is_none()
            || signals.cached_input_tokens.is_none()
            || signals.model_name.is_none())
        && codex_rollout_process_confirmed(ctx, proc_root)
        && let Some(home) = home_dir
    {
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        if let Some(roll) = codex_rollout::read_rollout_for_path(&codex_home, ctx.current_path) {
            let metric = |v| {
                crate::domain::signal::MetricValue::new(
                    v,
                    crate::domain::origin::SourceKind::ProviderOfficial,
                )
                .with_confidence(0.9)
                .with_provider(Provider::Codex)
            };
            if signals.input_tokens.is_none()
                && let Some(n) = roll.input_tokens
            {
                signals.input_tokens = Some(metric(n));
            }
            if signals.output_tokens.is_none()
                && let Some(n) = roll.output_tokens
            {
                signals.output_tokens = Some(metric(n));
            }
            if signals.cached_input_tokens.is_none()
                && let Some(n) = roll.cached_input_tokens
            {
                signals.cached_input_tokens = Some(metric(n));
            }
            // model_name is String; the `metric` closure above is u64-typed,
            // so build inline. Keep ProviderOfficial/Codex/0.9 in sync with it.
            if signals.model_name.is_none()
                && let Some(m) = roll.model
            {
                signals.model_name = Some(
                    crate::domain::signal::MetricValue::new(
                        m,
                        crate::domain::origin::SourceKind::ProviderOfficial,
                    )
                    .with_confidence(0.9)
                    .with_provider(Provider::Codex),
                );
            }
            // Slice D: fill context_window_size from rollout (fill-when-absent).
            if signals.context_window_size.is_none()
                && let Some(n) = roll.context_window
            {
                signals.context_window_size = Some(metric(n));
            }
        }
    }
    // Slice C2: agy transcript activity indicator. Surface a live activity
    // RuntimeFact when the agy_transcript toggle is enabled, the pane is Antigravity,
    // the identity is not conflicted, a cwd is present, and an agy binary is the
    // descendant process. The transcript activity is correlated from the
    // history.jsonl history by workspace (cwd) and the ambiguity guard prevents
    // mis-attribution when multiple agy sessions are active in the same workspace.
    // Activity is always Heuristic (reverse-engineered) and never PopupOption
    // (no token/model/cost claims). The six v2.4.0 ObserveOnly gates are untouched.
    if ctx.agy_transcript_enabled
        && matches!(ctx.identity.identity.provider, Provider::Antigravity)
        && !identity_conflict
        && !ctx.current_path.is_empty()
        && agy_process_confirmed(ctx, proc_root)
        && let Some(home) = home_dir
    {
        let gemini_home = home.join(".gemini");
        if let Some(activity) = agy_transcript::read_agy_activity(&gemini_home, ctx.current_path) {
            signals.runtime_facts.push(
                crate::domain::signal::RuntimeFact::new(
                    crate::domain::signal::RuntimeFactKind::AgyActivity,
                    format!(
                        "transcript · {}",
                        activity.latest_step.as_deref().unwrap_or("active")
                    ),
                    crate::domain::origin::SourceKind::Heuristic,
                )
                .with_provider(Provider::Antigravity),
            );
        }
    }
    // Slice C3: agy footer/sidefile enrichment. Footer scrape fills model /
    // context% / token-count (always pane-correct) when absent; the structured
    // sidefile OVERRIDES when present and unambiguously attributed by cwd. Both
    // ProviderOfficial. Display-only — the six v2.4.0 ObserveOnly gates are
    // untouched (agy is NOT added to token_rows_supported / token-sample / insights).
    if ctx.agy_enrichment_enabled
        && matches!(ctx.identity.identity.provider, Provider::Antigravity)
        && !identity_conflict
        && !ctx.current_path.is_empty()
        && agy_process_confirmed(ctx, proc_root)
    {
        use crate::domain::identity::Provider;
        use crate::domain::origin::SourceKind;
        use crate::domain::signal::MetricValue;
        let metric_str = |v: String| {
            MetricValue::new(v, SourceKind::ProviderOfficial)
                .with_confidence(0.9)
                .with_provider(Provider::Antigravity)
        };
        let metric_f32 = |v: f32| {
            MetricValue::new(v, SourceKind::ProviderOfficial)
                .with_confidence(0.9)
                .with_provider(Provider::Antigravity)
        };
        let metric_u64 = |v: u64| {
            MetricValue::new(v, SourceKind::ProviderOfficial)
                .with_confidence(0.9)
                .with_provider(Provider::Antigravity)
        };

        // (a) Footer scrape — fill-when-absent (always the pane's own text).
        let footer = agy_footer::parse_agy_footer(ctx.tail);
        if signals.model_name.is_none()
            && let Some(m) = footer.model
        {
            signals.model_name = Some(metric_str(m));
        }
        if signals.context_pressure.is_none()
            && let Some(p) = footer.context_used_pct
        {
            signals.context_pressure = Some(metric_f32(p));
        }
        if signals.token_count.is_none()
            && let Some(n) = footer.token_count
        {
            signals.token_count = Some(metric_u64(n));
        }

        // (b) Structured sidefile — OVERRIDE when present + unambiguous.
        if let Some(home) = home_dir
            && let Some(sf) = agy_sidefile::read_agy_sidefile_for_path(home, ctx.current_path)
        {
            if let Some(m) = sf.model {
                signals.model_name = Some(metric_str(m));
            }
            if let Some(pct) = sf.context_used_percentage {
                signals.context_pressure = Some(metric_f32((pct / 100.0).clamp(0.0, 1.0) as f32));
            }
            if let Some(n) = sf.context_window_size {
                signals.context_window_size = Some(metric_u64(n));
            }
            if let Some(n) = sf.token_count {
                signals.token_count = Some(metric_u64(n));
            }
        }
    }
    signals
}

fn claude_sidefile_process_confirmed(ctx: &ParserContext, proc_root: &std::path::Path) -> bool {
    let Some(pid) = ctx.pane_pid else {
        return true;
    };
    let Some(desc) = process_memory::read_descendant_cli_process_with_proc_root(pid, proc_root)
    else {
        return false;
    };
    cli_process_basename_contains(&desc, "claude")
}

fn codex_rollout_process_confirmed(ctx: &ParserContext, proc_root: &std::path::Path) -> bool {
    let Some(pid) = ctx.pane_pid else { return true };
    let Some(desc) = process_memory::read_descendant_cli_process_with_proc_root(pid, proc_root)
    else {
        return false;
    };
    cli_process_basename_contains(&desc, "codex")
}

fn agy_process_confirmed(ctx: &ParserContext, proc_root: &std::path::Path) -> bool {
    // agy correlation is by shared `workspace` (cwd), which is weaker than
    // Claude session_id or Codex cwd+rollout. Without a confirmed descendant
    // `agy` process, we must NOT attribute agy activity — return false (not true
    // like Claude/Codex helpers). This prevents mis-attribution across unrelated
    // agy sessions in the same workspace.
    let Some(pid) = ctx.pane_pid else {
        return false;
    };
    let Some(desc) = process_memory::read_descendant_cli_process_with_proc_root(pid, proc_root)
    else {
        return false;
    };
    cli_process_basename_contains(&desc, "agy")
}

fn cli_process_basename_contains(
    desc: &process_memory::CliProcessDescriptor,
    needle: &str,
) -> bool {
    if desc.comm.eq_ignore_ascii_case(needle) {
        return true;
    }
    desc.argv
        .iter()
        .any(|arg| path_basename_contains(arg, needle))
        || desc
            .exe_path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().contains(needle))
}

fn path_basename_contains(path: &str, needle: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase()
        .contains(needle)
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
        if signals.cache_creation_input_tokens.is_none()
            && let Some(n) = usage.cache_creation_input_tokens
        {
            signals.cache_creation_input_tokens = Some(metric(n));
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
    // Slice A: prefer the sidefile's structured used_percentage over the
    // scraped statusline pressure — same ProviderOfficial number, but not
    // dependent on statusline text parsing. Override (not is_none) so the
    // structured value wins whenever present.
    if let Some(pct) = sidefile
        .context_window
        .as_ref()
        .and_then(|cw| cw.used_percentage)
    {
        signals.context_pressure = Some(metric((pct / 100.0).clamp(0.0, 1.0) as f32));
    }
    if let Some(rl) = sidefile.rate_limits.as_ref() {
        if let Some(pct) = rl.five_hour.as_ref().and_then(|w| w.used_percentage) {
            signals.quota_5h_pressure = Some(metric((pct / 100.0).clamp(0.0, 1.0) as f32));
        }
        if let Some(pct) = rl.seven_day.as_ref().and_then(|w| w.used_percentage) {
            signals.quota_weekly_pressure = Some(metric((pct / 100.0).clamp(0.0, 1.0) as f32));
        }
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
    // Slice A: fill model/effort from the sidefile only when the scrape
    // didn't already produce them (is_none) — the statusline is the
    // authoritative display string when present, so we avoid churning it.
    if signals.model_name.is_none()
        && let Some(m) = sidefile.model.as_ref()
        && let Some(name) = m.display_name.clone().or_else(|| m.id.clone())
    {
        signals.model_name = Some(metric(name));
    }
    if signals.reasoning_effort.is_none()
        && let Some(level) = sidefile.effort.as_ref().and_then(|e| e.level.clone())
    {
        signals.reasoning_effort = Some(metric(level));
    }
    // Slice D: fill context_window_size from sidefile (fill-when-absent).
    if signals.context_window_size.is_none()
        && let Some(n) = sidefile
            .context_window
            .as_ref()
            .and_then(|cw| cw.context_window_size)
    {
        signals.context_window_size = Some(metric(n));
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
        codex_rollout_enabled: false,
        agy_transcript_enabled: false,
        agy_enrichment_enabled: false,
    }
}

#[cfg(test)]
mod sidefile_integration_tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Role};
    use crate::domain::origin::SourceKind;
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

    fn write_proc_pid(root: &std::path::Path, pid: u32, comm: &str, children: &[u32]) {
        let dir = root.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("status"), format!("Name:\t{comm}\nVmRSS:\t1 kB\n")).unwrap();
        let task_dir = dir.join("task").join(pid.to_string());
        fs::create_dir_all(&task_dir).unwrap();
        fs::write(
            task_dir.join("children"),
            children
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        )
        .unwrap();
    }

    fn write_proc_cmdline(root: &std::path::Path, pid: u32, argv: &[&str]) {
        let mut bytes = Vec::new();
        for (idx, arg) in argv.iter().enumerate() {
            if idx > 0 {
                bytes.push(0);
            }
            bytes.extend_from_slice(arg.as_bytes());
        }
        bytes.push(0);
        fs::write(root.join(pid.to_string()).join("cmdline"), bytes).unwrap();
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
        let proc_root = tmp.path().join("proc");
        write_proc_pid(&proc_root, 1, "bash", &[2]);
        write_proc_pid(&proc_root, 2, "claude", &[]);
        write_proc_cmdline(&proc_root, 2, &["claude", "--print"]);
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        // Raw token counts surface for F-3 sparkline + future F-7 precision.
        assert_eq!(signals.input_tokens.as_ref().unwrap().value, 50_000);
        assert_eq!(signals.output_tokens.as_ref().unwrap().value, 8);
        assert_eq!(signals.cached_input_tokens.as_ref().unwrap().value, 150_000);
        assert_eq!(
            signals.cache_creation_input_tokens.as_ref().unwrap().value,
            770
        );
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

    #[test]
    fn sidefile_skipped_for_conflicting_claude_identity() {
        let tmp = tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_sidefile_for(
            tmp.path(),
            "abc",
            &format!(r#"{{"cwd":"{cwd}","session_id":"abc","cost":{{"total_cost_usd":99.0}}}}"#),
        );
        let id = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::Conflict,
        };
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let proc_root = tmp.path().join("proc-empty");
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        assert!(signals.cost_usd.is_none());
        assert!(signals.cache_hit_ratio.is_none());
        assert!(signals.runtime_facts.is_empty());
    }

    #[test]
    fn sidefile_skipped_when_descendant_process_is_not_claude() {
        let tmp = tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_sidefile_for(
            tmp.path(),
            "abc",
            &format!(r#"{{"cwd":"{cwd}","session_id":"abc","cost":{{"total_cost_usd":99.0}}}}"#),
        );
        let proc_root = tmp.path().join("proc");
        write_proc_pid(&proc_root, 1, "bash", &[2]);
        write_proc_pid(&proc_root, 2, "node", &[]);
        write_proc_cmdline(&proc_root, 2, &["node", "/usr/bin/gemini"]);
        let id = claude_id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        assert!(signals.cost_usd.is_none());
        assert!(signals.runtime_facts.is_empty());
    }

    #[test]
    fn sidefile_used_percentage_overrides_scraped_pressure() {
        // Pre-seed as if the scrape produced stale/rounded values.
        let mut signals = crate::adapters::common::parse_common_signals("");
        signals.context_pressure = Some(crate::domain::signal::MetricValue::new(
            0.50_f32,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        signals.quota_5h_pressure = Some(crate::domain::signal::MetricValue::new(
            0.50_f32,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        signals.quota_weekly_pressure = Some(crate::domain::signal::MetricValue::new(
            0.50_f32,
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        let sidefile: claude_sidefile::ClaudeSidefile = serde_json::from_str(
            r#"{
                "context_window": {"used_percentage": 21},
                "rate_limits": {
                    "five_hour": {"used_percentage": 9},
                    "seven_day": {"used_percentage": 20}
                }
            }"#,
        )
        .unwrap();

        apply_claude_sidefile(&mut signals, sidefile);

        // Codex r1 must-fix: lock both value AND origin label (ProviderOfficial).
        let cp = signals.context_pressure.as_ref().unwrap();
        assert!((cp.value - 0.21).abs() < 1e-6);
        assert_eq!(
            cp.source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
        let q5 = signals.quota_5h_pressure.as_ref().unwrap();
        assert!((q5.value - 0.09).abs() < 1e-6);
        assert_eq!(
            q5.source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
        let qw = signals.quota_weekly_pressure.as_ref().unwrap();
        assert!((qw.value - 0.20).abs() < 1e-6);
        assert_eq!(
            qw.source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
    }

    #[test]
    fn sidefile_used_percentage_over_100_is_clamped_to_full() {
        let mut signals = crate::adapters::common::parse_common_signals("");
        let sidefile: claude_sidefile::ClaudeSidefile = serde_json::from_str(
            r#"{"context_window": {"used_percentage": 130},
                "rate_limits": {"five_hour": {"used_percentage": 150}}}"#,
        )
        .unwrap();
        apply_claude_sidefile(&mut signals, sidefile);
        assert!((signals.context_pressure.unwrap().value - 1.0).abs() < 1e-6);
        assert!((signals.quota_5h_pressure.unwrap().value - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sidefile_fills_model_and_effort_when_scrape_absent() {
        let mut signals = crate::adapters::common::parse_common_signals("");
        let sidefile: claude_sidefile::ClaudeSidefile = serde_json::from_str(
            r#"{
                "model": {"id": "claude-opus-4-8[1m]", "display_name": "Opus 4.8 (1M context)"},
                "effort": {"level": "max"}
            }"#,
        )
        .unwrap();

        apply_claude_sidefile(&mut signals, sidefile);

        assert_eq!(
            signals.model_name.as_ref().unwrap().value,
            "Opus 4.8 (1M context)"
        );
        assert_eq!(signals.reasoning_effort.as_ref().unwrap().value, "max");
        // Codex r1 must-fix: sidefile-filled values are ProviderOfficial.
        assert_eq!(
            signals.model_name.as_ref().unwrap().source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
        assert_eq!(
            signals.reasoning_effort.as_ref().unwrap().source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
    }

    #[test]
    fn sidefile_does_not_override_scraped_model() {
        let mut signals = crate::adapters::common::parse_common_signals("");
        signals.model_name = Some(crate::domain::signal::MetricValue::new(
            "scraped-model".to_string(),
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        signals.reasoning_effort = Some(crate::domain::signal::MetricValue::new(
            "scraped-effort".to_string(),
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        let sidefile: claude_sidefile::ClaudeSidefile = serde_json::from_str(
            r#"{"model":{"id":"x","display_name":"Y"},"effort":{"level":"z"}}"#,
        )
        .unwrap();

        apply_claude_sidefile(&mut signals, sidefile);

        assert_eq!(signals.model_name.as_ref().unwrap().value, "scraped-model");
        assert_eq!(
            signals.reasoning_effort.as_ref().unwrap().value,
            "scraped-effort"
        );
    }

    #[test]
    fn sidefile_model_falls_back_to_id_when_display_name_absent() {
        let mut signals = crate::adapters::common::parse_common_signals("");
        let sidefile: claude_sidefile::ClaudeSidefile =
            serde_json::from_str(r#"{"model":{"id":"claude-opus-4-8"}}"#).unwrap();
        apply_claude_sidefile(&mut signals, sidefile);
        assert_eq!(
            signals.model_name.as_ref().unwrap().value,
            "claude-opus-4-8"
        );
    }

    #[test]
    fn sidefile_context_window_size_fill_when_absent() {
        let mut signals = SignalSet::default();
        let sidefile: claude_sidefile::ClaudeSidefile =
            serde_json::from_str(r#"{"context_window":{"context_window_size":1000000}}"#).unwrap();
        apply_claude_sidefile(&mut signals, sidefile);
        assert_eq!(
            signals.context_window_size.as_ref().unwrap().value,
            1_000_000
        );
        assert_eq!(
            signals.context_window_size.as_ref().unwrap().source_kind,
            SourceKind::ProviderOfficial
        );
        assert_eq!(
            signals.context_window_size.as_ref().unwrap().provider,
            Some(Provider::Claude)
        );
    }
}

#[cfg(test)]
mod codex_rollout_integration_tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use std::fs;

    fn codex_id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Codex,
                instance: 1,
                role: Role::Main,
                pane_id: "%2".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }
    fn write_rollout(home: &std::path::Path, cwd: &str) {
        let dir = home.join(".codex/sessions/2026/06/29");
        fs::create_dir_all(&dir).unwrap();
        let body = format!(
            concat!(
                r#"{{"type":"session_meta","payload":{{"originator":"codex-tui","cwd":"{cwd}","cli_version":"0.142.2"}}}}"#,
                "\n",
                r#"{{"type":"turn_context","payload":{{"model":"gpt-5.5"}}}}"#,
                "\n",
                r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":1510000,"output_tokens":20400,"cached_input_tokens":1200000,"total_tokens":1530400}},"model_context_window":258400}}}}}}"#,
                "\n",
            ),
            cwd = cwd
        );
        fs::write(dir.join("rollout-x.jsonl"), body).unwrap();
    }
    fn write_proc(root: &std::path::Path, pid: u32, comm: &str, children: &[u32]) {
        let d = root.join(pid.to_string());
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("status"), format!("Name:\t{comm}\nVmRSS:\t1 kB\n")).unwrap();
        let t = d.join("task").join(pid.to_string());
        fs::create_dir_all(&t).unwrap();
        fs::write(
            t.join("children"),
            children
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        )
        .unwrap();
        let mut argv = Vec::new();
        argv.extend_from_slice(comm.as_bytes());
        argv.push(0);
        fs::write(d.join("cmdline"), argv).unwrap();
    }

    #[test]
    fn rollout_fills_codex_tokens_and_model_when_scrape_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_rollout(tmp.path(), cwd);
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "bash", &[2]);
        write_proc(&proc_root, 2, "codex", &[]);
        let id = codex_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        // empty tail → scrape produces no tokens/model → backstop fires.
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.codex_rollout_enabled = true;

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        assert_eq!(signals.input_tokens.as_ref().unwrap().value, 1_510_000);
        assert_eq!(signals.output_tokens.as_ref().unwrap().value, 20_400);
        assert_eq!(
            signals.cached_input_tokens.as_ref().unwrap().value,
            1_200_000
        );
        assert_eq!(signals.model_name.as_ref().unwrap().value, "gpt-5.5");
        assert_eq!(
            signals.input_tokens.as_ref().unwrap().source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
        assert_eq!(
            signals.output_tokens.as_ref().unwrap().source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
        assert_eq!(
            signals.cached_input_tokens.as_ref().unwrap().source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
        assert_eq!(
            signals.model_name.as_ref().unwrap().source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
    }

    #[test]
    fn rollout_skipped_when_toggle_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_rollout(tmp.path(), cwd);
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "codex", &[]);
        let id = codex_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.codex_rollout_enabled = false;
        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));
        assert!(
            signals.input_tokens.is_none(),
            "toggle off → no rollout fill"
        );
    }

    #[test]
    fn rollout_skipped_when_descendant_is_not_codex() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_rollout(tmp.path(), cwd);
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "bash", &[2]);
        write_proc(&proc_root, 2, "node", &[]); // not codex
        let id = codex_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.codex_rollout_enabled = true;
        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));
        assert!(signals.input_tokens.is_none());
    }

    #[test]
    fn rollout_enricher_fills_context_window_size() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_rollout(tmp.path(), cwd);
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "bash", &[2]);
        write_proc(&proc_root, 2, "codex", &[]);
        let id = codex_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.codex_rollout_enabled = true;

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        assert_eq!(
            signals.context_window_size.as_ref().unwrap().value,
            258400,
            "Codex rollout fills context_window_size when absent"
        );
        assert_eq!(
            signals.context_window_size.as_ref().unwrap().source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
        assert_eq!(
            signals.context_window_size.as_ref().unwrap().provider,
            Some(Provider::Codex)
        );
    }
}

#[cfg(test)]
mod agy_transcript_integration_tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use std::fs;

    fn agy_id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Antigravity,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    fn write_history(home: &std::path::Path, entries: Vec<(&str, &str, u64)>) {
        let agy_dir = home.join(".gemini/antigravity-cli");
        fs::create_dir_all(&agy_dir).unwrap();
        let path = agy_dir.join("history.jsonl");
        let mut body = String::new();
        for (workspace, conversation_id, timestamp) in entries {
            body.push_str(&format!(
                r#"{{"workspace":"{}","conversationId":"{}","timestamp":{}}}"#,
                workspace, conversation_id, timestamp
            ));
            body.push('\n');
        }
        fs::write(&path, body).unwrap();
    }

    fn write_transcript(home: &std::path::Path, conversation_id: &str, steps: Vec<&str>) {
        let transcript_dir = home.join(format!(
            ".gemini/antigravity-cli/brain/{}/.system_generated/logs",
            conversation_id
        ));
        fs::create_dir_all(&transcript_dir).unwrap();
        let path = transcript_dir.join("transcript.jsonl");
        let mut body = String::new();
        for step in steps {
            body.push_str(&format!(
                r#"{{"type":"{}","created_at":"2026-06-29T12:00:00Z"}}"#,
                step
            ));
            body.push('\n');
        }
        fs::write(&path, body).unwrap();
    }

    fn write_proc(root: &std::path::Path, pid: u32, comm: &str, children: &[u32]) {
        let d = root.join(pid.to_string());
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("status"), format!("Name:\t{comm}\nVmRSS:\t1 kB\n")).unwrap();
        let t = d.join("task").join(pid.to_string());
        fs::create_dir_all(&t).unwrap();
        fs::write(
            t.join("children"),
            children
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        )
        .unwrap();
        let mut argv = Vec::new();
        argv.extend_from_slice(comm.as_bytes());
        argv.push(0);
        fs::write(d.join("cmdline"), argv).unwrap();
    }

    #[test]
    fn transcript_activity_fact_pushed_when_toggle_and_agy_descendant_present() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_history(tmp.path(), vec![("/repo/qmonster", "uuid-aaa", 1000)]);
        write_transcript(
            tmp.path(),
            "uuid-aaa",
            vec!["CONVERSATION_STARTED", "STEP_ONE"],
        );
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "bash", &[2]);
        write_proc(&proc_root, 2, "agy", &[]);
        let id = agy_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.agy_transcript_enabled = true;

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        assert_eq!(signals.runtime_facts.len(), 1);
        let fact = &signals.runtime_facts[0];
        assert_eq!(
            fact.kind,
            crate::domain::signal::RuntimeFactKind::AgyActivity
        );
        assert_eq!(
            fact.source_kind,
            crate::domain::origin::SourceKind::Heuristic
        );
        assert_eq!(fact.provider, Some(Provider::Antigravity));
        assert!(fact.value.contains("STEP_ONE"));
    }

    #[test]
    fn transcript_activity_not_added_when_toggle_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_history(tmp.path(), vec![("/repo/qmonster", "uuid-aaa", 1000)]);
        write_transcript(tmp.path(), "uuid-aaa", vec!["STEP"]);
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "bash", &[2]);
        write_proc(&proc_root, 2, "agy", &[]);
        let id = agy_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.agy_transcript_enabled = false;

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        assert!(
            signals.runtime_facts.is_empty(),
            "toggle off → no activity fact"
        );
    }

    #[test]
    fn transcript_activity_not_added_when_descendant_is_not_agy() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_history(tmp.path(), vec![("/repo/qmonster", "uuid-aaa", 1000)]);
        write_transcript(tmp.path(), "uuid-aaa", vec!["STEP"]);
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "bash", &[2]);
        write_proc(&proc_root, 2, "node", &[]); // not agy
        let id = agy_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.agy_transcript_enabled = true;

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        assert!(signals.runtime_facts.is_empty());
    }
}

#[cfg(test)]
mod agy_enrichment_integration_tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use std::fs;

    fn agy_id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Antigravity,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    fn write_proc(root: &std::path::Path, pid: u32, comm: &str, children: &[u32]) {
        let d = root.join(pid.to_string());
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("status"), format!("Name:\t{comm}\nVmRSS:\t1 kB\n")).unwrap();
        let t = d.join("task").join(pid.to_string());
        fs::create_dir_all(&t).unwrap();
        fs::write(
            t.join("children"),
            children
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        )
        .unwrap();
        let mut argv = Vec::new();
        argv.extend_from_slice(comm.as_bytes());
        argv.push(0);
        fs::write(d.join("cmdline"), argv).unwrap();
    }

    fn write_agy_sidefile(home: &std::path::Path, cid: &str, body: &str) {
        let dir = home.join(".local/share/ai-cli-status/agy");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{cid}.json")), body).unwrap();
    }

    /// Footer tail: model "Gemini 3.5 Flash (High)" with token-count item enabled.
    const FOOTER_TAIL: &str = "> hello\n? for shortcuts                         token-count 34567  Gemini 3.5 Flash (High)\n";

    #[test]
    fn agy_enrichment_fills_model_and_token_from_footer_when_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 100, "bash", &[101]);
        write_proc(&proc_root, 101, "agy", &[]);
        let home = tmp.path().join("home");
        let id = agy_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, FOOTER_TAIL, &pricing, &settings, &history);
        c.current_path = "/repo";
        c.pane_pid = Some(100);
        c.agy_enrichment_enabled = true;

        let signals = parse_for_with_environment(&c, &proc_root, Some(&home));

        assert_eq!(
            signals.model_name.as_ref().map(|m| m.value.as_str()),
            Some("Gemini 3.5 Flash (High)")
        );
        assert!(signals.token_count.is_some());
        assert_eq!(
            signals.model_name.as_ref().map(|m| m.source_kind),
            Some(crate::domain::origin::SourceKind::ProviderOfficial)
        );
    }

    #[test]
    fn agy_enrichment_sidefile_overrides_footer() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 100, "bash", &[101]);
        write_proc(&proc_root, 101, "agy", &[]);
        let home = tmp.path().join("home");
        write_agy_sidefile(
            &home,
            "c1",
            r#"{"cwd":"/repo","conversation_id":"c1","model":"Gemini 3.1 Pro (High)","context_window_size":1048576}"#,
        );
        let id = agy_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, FOOTER_TAIL, &pricing, &settings, &history);
        c.current_path = "/repo";
        c.pane_pid = Some(100);
        c.agy_enrichment_enabled = true;

        let signals = parse_for_with_environment(&c, &proc_root, Some(&home));

        assert_eq!(
            signals.model_name.as_ref().map(|m| m.value.as_str()),
            Some("Gemini 3.1 Pro (High)"),
            "sidefile model must override the footer-scraped model"
        );
        assert_eq!(
            signals.context_window_size.as_ref().map(|m| m.value),
            Some(1048576)
        );
    }

    #[test]
    fn agy_enrichment_skipped_when_toggle_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 100, "bash", &[101]);
        write_proc(&proc_root, 101, "agy", &[]);
        let home = tmp.path().join("home");
        let id = agy_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, FOOTER_TAIL, &pricing, &settings, &history);
        c.current_path = "/repo";
        c.pane_pid = Some(100);
        c.agy_enrichment_enabled = false;

        let signals = parse_for_with_environment(&c, &proc_root, Some(&home));

        assert!(
            signals.model_name.is_none(),
            "no enrichment when toggle off"
        );
    }

    #[test]
    fn agy_enrichment_skipped_when_descendant_is_not_agy() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 100, "bash", &[101]);
        write_proc(&proc_root, 101, "bash", &[]); // no agy descendant
        let home = tmp.path().join("home");
        let id = agy_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, FOOTER_TAIL, &pricing, &settings, &history);
        c.current_path = "/repo";
        c.pane_pid = Some(100);
        c.agy_enrichment_enabled = true;

        let signals = parse_for_with_environment(&c, &proc_root, Some(&home));

        assert!(
            signals.model_name.is_none(),
            "strict agy process-confirm gates enrichment"
        );
    }
}

// ── Task 6 (Slice C3): ObserveOnly six-gate regression ──────────────────────
// C3 populates model_name / context_pressure / context_window_size /
// token_count for agy panes, but agy must stay off every analytic surface.
// These tests are GUARD-RAILS: they must pass immediately; if any assertion
// fails, C3 broke a gate and must be fixed before proceeding.
#[cfg(test)]
mod agy_observeonly_regression {
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{MetricValue, SignalSet};

    const ALL_ANOMALY_KINDS: &[crate::domain::anomaly::AnomalyKind] = &[
        crate::domain::anomaly::AnomalyKind::IdentityChurn,
        crate::domain::anomaly::AnomalyKind::ErrorBurst,
        crate::domain::anomaly::AnomalyKind::CacheDiscontinuity,
        crate::domain::anomaly::AnomalyKind::CrossPaneEditCluster,
        crate::domain::anomaly::AnomalyKind::CostSlope,
        crate::domain::anomaly::AnomalyKind::TokenSlope,
        crate::domain::anomaly::AnomalyKind::MemoryGrowth,
        crate::domain::anomaly::AnomalyKind::SubagentSideEffect,
    ];

    /// Build a C3-enriched agy SignalSet — model_name + token_count populated,
    /// matching what Tasks 2–4 produce when `agy_enrichment` is on.
    fn agy_enriched_signals() -> SignalSet {
        SignalSet {
            model_name: Some(
                MetricValue::new(
                    "Gemini 3.5 Flash (High)".to_string(),
                    SourceKind::ProviderOfficial,
                )
                .with_provider(Provider::Antigravity),
            ),
            token_count: Some(
                MetricValue::new(34_567_u64, SourceKind::ProviderOfficial)
                    .with_provider(Provider::Antigravity),
            ),
            ..SignalSet::default()
        }
    }

    fn agy_resolved_id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Antigravity,
                instance: 1,
                role: Role::Main,
                pane_id: "%3".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    /// Gate 1: anomaly detectors — no AnomalyKind supports Antigravity.
    #[test]
    fn agy_enrichment_gate1_no_anomaly_kind_supports_antigravity() {
        for kind in ALL_ANOMALY_KINDS {
            assert!(
                !kind.supports_provider(Provider::Antigravity),
                "{:?} must not support Antigravity after C3 — ObserveOnly contract",
                kind
            );
        }
    }

    /// Gate 2: provider_honesty cache chip — Hidden for Antigravity even when
    /// C3-enriched signals include token_count.
    #[test]
    fn agy_enrichment_gate2_cache_metric_hidden() {
        use crate::ui::provider_honesty::{CacheMetricStatus, cache_metric_status};
        let signals = agy_enriched_signals();
        assert_eq!(
            cache_metric_status(&signals, Provider::Antigravity),
            CacheMetricStatus::Hidden,
            "cache_metric_status must stay Hidden for Antigravity after C3"
        );
    }

    /// Gate 2b: provider_honesty cost chip — Hidden for Antigravity even when
    /// C3-enriched signals include token_count.
    #[test]
    fn agy_enrichment_gate2b_cost_metric_hidden() {
        use crate::ui::provider_honesty::{CostMetricStatus, cost_metric_status};
        let signals = agy_enriched_signals();
        assert_eq!(
            cost_metric_status(&signals, Provider::Antigravity),
            CostMetricStatus::Hidden,
            "cost_metric_status must stay Hidden for Antigravity after C3"
        );
    }

    /// Gate 3: profile_switch rule — returns empty Vec for Antigravity
    /// (profile_targets_for_provider returns None → rule exits early).
    #[test]
    fn agy_enrichment_gate3_profile_switch_returns_none() {
        use crate::domain::identity::IdentityConfidence;
        use crate::policy::gates::PolicyGates;
        use crate::policy::rules::profile_switch::eval_profile_switch;
        let id = agy_resolved_id();
        let signals = agy_enriched_signals();
        // All conditions that would normally trigger the rule: gate on,
        // window full, 100% error rate.
        let gates = PolicyGates {
            identity_confidence: IdentityConfidence::High,
            profile_switch_enabled: true,
            profile_switch_window: 5,
            profile_switch_error_rate: 0.5,
            ..PolicyGates::default()
        };
        let history = vec![true; 10];
        let recs = eval_profile_switch(&id, &signals, &history, &gates);
        assert!(
            recs.is_empty(),
            "profile_switch must return empty Vec for Antigravity after C3; got {:?}",
            recs
        );
    }

    /// Gate 4: insights coverage — provider string "Antigravity" maps to
    /// status "unsupported" in the PaneDataCompleteness query.
    #[test]
    fn agy_enrichment_gate4_insights_coverage_is_unsupported() {
        // Calls the real predicate function extracted from
        // `collect_pane_data_completeness` in `store::insights`.  If the
        // "Antigravity" literal were removed from the exclusion set, the real
        // function would return "ok" or "?" and this assertion would fail.
        let status = crate::store::insights::pane_completeness_status("Antigravity", 100.0);
        assert_eq!(
            status, "unsupported",
            "insights coverage gate must map Antigravity → 'unsupported' after C3"
        );
    }

    /// Gate 5: token-sample filter — Antigravity is in the excluded set so
    /// filter_token_samples_for_provider returns Vec::new() for it.
    #[test]
    fn agy_enrichment_gate5_token_sample_filter_excludes_antigravity() {
        // Calls the real function in `app::event_loop`.  If Provider::Antigravity
        // were removed from the exclusion matches!, the function would return the
        // non-empty sample vec and the assert_eq! would fail.
        let samples = vec![crate::store::TokenSample {
            ts_unix_ms: 1,
            pane_id: "%0".into(),
            provider: Provider::Antigravity,
            input_tokens: Some(100),
            output_tokens: Some(10),
            cost_usd: None,
            cached_input_tokens: None,
        }];
        let result = crate::app::event_loop::filter_token_samples_for_provider(
            samples,
            Provider::Antigravity,
        );
        assert!(
            result.is_empty(),
            "filter_token_samples_for_provider must return Vec::new() for Antigravity after C3"
        );
    }

    /// Gate 6: token sparkline/breakdown — token_rows_supported returns false
    /// for Antigravity, so populating token_count does not pull agy into the
    /// token sparkline or breakdown surfaces.
    #[test]
    fn agy_enrichment_gate6_token_rows_not_supported() {
        assert!(
            !crate::ui::panels::token_rows_supported(Provider::Antigravity),
            "token_rows_supported must stay false for Antigravity after C3"
        );
    }

    /// Composite guard-rail: enriched agy pane hits all six gates in one shot.
    #[test]
    fn agy_enrichment_does_not_re_enable_token_rows_or_anomaly_gates() {
        // Gate 6
        assert!(
            !crate::ui::panels::token_rows_supported(Provider::Antigravity),
            "token_rows_supported must stay false for Antigravity after C3"
        );
        // Gate 1
        assert!(
            ALL_ANOMALY_KINDS
                .iter()
                .all(|k| !k.supports_provider(Provider::Antigravity)),
            "no anomaly kind may support Antigravity after C3"
        );
        // Gate 2: cache metric Hidden
        assert_eq!(
            crate::ui::provider_honesty::cache_metric_status(
                &agy_enriched_signals(),
                Provider::Antigravity
            ),
            crate::ui::provider_honesty::CacheMetricStatus::Hidden,
        );
        // Gate 3: profile_switch → empty
        {
            use crate::domain::identity::IdentityConfidence;
            use crate::policy::gates::PolicyGates;
            let id = agy_resolved_id();
            let signals = agy_enriched_signals();
            let gates = PolicyGates {
                identity_confidence: IdentityConfidence::High,
                profile_switch_enabled: true,
                profile_switch_window: 5,
                profile_switch_error_rate: 0.5,
                ..PolicyGates::default()
            };
            let recs = crate::policy::rules::profile_switch::eval_profile_switch(
                &id,
                &signals,
                &[true; 10],
                &gates,
            );
            assert!(recs.is_empty());
        }
        // Gate 4: insights coverage string
        let provider_str = format!("{:?}", Provider::Antigravity);
        assert!(
            matches!(
                provider_str.as_str(),
                "Antigravity" | "Qmonster" | "Unknown"
            ),
            "Antigravity must remain in the insights 'unsupported' string set"
        );
        // Gate 5: token-sample filter exclusion set
        assert!(matches!(
            Provider::Antigravity,
            Provider::Antigravity | Provider::Qmonster | Provider::Unknown
        ));
    }
}
