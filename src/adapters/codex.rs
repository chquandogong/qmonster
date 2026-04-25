use crate::adapters::ProviderParser;
use crate::adapters::common::{PaneTailHistory, parse_common_signals, parse_count_with_suffix};
use crate::adapters::runtime::{push_provider_fact, split_runtime_list};
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{IdleCause, MetricValue, RuntimeFactKind, SignalSet};

pub struct CodexAdapter;

struct CodexStatus {
    context_pct: u8,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    /// v1.11.2 remediation (Codex v1.11.0 warning): the newest
    /// status-line-shaped line is authoritative. If its model token
    /// is outside the `gpt-/claude-/gemini-` allowlist we still emit
    /// context + tokens (ProviderOfficial) and leave `model` as
    /// `None` so cost/model badges stay blank rather than fall back
    /// to a stale older `/status` frame.
    model: Option<String>,
    worktree_path: Option<String>,
    git_branch: Option<String>,
    reasoning_effort: Option<String>,
}

impl ProviderParser for CodexAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let tail = ctx.tail;
        let pricing = ctx.pricing;
        let mut set = parse_common_signals(tail);
        append_codex_runtime_facts(&mut set, tail);
        let Some(status) = parse_codex_status_line(tail) else {
            // Slice 4: classify idle state. Common-tier markers populated
            // PermissionWait/InputWait if applicable; layer Codex-specific
            // cursor + 5h-limit detection, then stillness fallback.
            if set.idle_state.is_none() {
                set.idle_state = classify_idle_codex(tail, ctx.history);
            }
            return set;
        };

        set.context_pressure = Some(
            MetricValue::new(
                status.context_pct as f32 / 100.0,
                SourceKind::ProviderOfficial,
            )
            .with_confidence(0.95)
            .with_provider(Provider::Codex),
        );
        set.token_count = Some(
            MetricValue::new(status.total_tokens, SourceKind::ProviderOfficial)
                .with_confidence(0.95)
                .with_provider(Provider::Codex),
        );
        if let Some(model) = status.model.as_ref() {
            set.model_name = Some(
                MetricValue::new(model.clone(), SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Codex),
            );
            set.cost_usd = pricing.lookup(Provider::Codex, model).map(|rates| {
                let cost = (status.input_tokens as f64 * rates.input_per_1m
                    + status.output_tokens as f64 * rates.output_per_1m)
                    / 1_000_000.0;
                MetricValue::new(cost, SourceKind::Estimated)
                    .with_confidence(0.7)
                    .with_provider(Provider::Codex)
            });
        }
        set.worktree_path = status.worktree_path.map(|p| {
            MetricValue::new(p, SourceKind::ProviderOfficial)
                .with_confidence(0.95)
                .with_provider(Provider::Codex)
        });
        set.git_branch = status.git_branch.map(|b| {
            MetricValue::new(b, SourceKind::ProviderOfficial)
                .with_confidence(0.95)
                .with_provider(Provider::Codex)
        });
        set.reasoning_effort = status.reasoning_effort.map(|e| {
            MetricValue::new(e, SourceKind::ProviderOfficial)
                .with_confidence(0.6) // stale-risk from /status box tail retention
                .with_provider(Provider::Codex)
        });

        // Slice 4: classify idle state. Common-tier markers populated
        // PermissionWait/InputWait if applicable; layer Codex-specific
        // cursor + 5h-limit detection, then stillness fallback.
        if set.idle_state.is_none() {
            set.idle_state = classify_idle_codex(tail, ctx.history);
        }

        set
    }
}

/// Scan `tail` for a Codex `/status` box line matching
/// `reasoning (xhigh|high|medium|low|auto)`. Return the captured
/// effort value on the first match; None otherwise. Called by
/// parse_codex_status_line's success path (only after the status
/// bar itself matches the shape heuristic).
fn parse_codex_reasoning_effort(tail: &str) -> Option<String> {
    for line in tail.lines().rev() {
        // Anchor to real /status box structure: the box-draw vertical glyph
        // `│` at the line edges AND the literal `Model:` label must both be
        // present on the same line as the `reasoning ` match. This prevents
        // arbitrary transcript prose like "set reasoning high for this run"
        // from being mis-stamped as a ProviderOfficial /status box read.
        // (v1.12.1 remediation — codex-v1.12.0-1 warning.)
        if !line.contains('│') || !line.contains("Model:") {
            continue;
        }
        if let Some(idx) = line.find("reasoning ") {
            let after = &line[idx + "reasoning ".len()..];
            let effort = after
                .split(|c: char| !c.is_ascii_alphabetic())
                .next()
                .unwrap_or("");
            if matches!(effort, "xhigh" | "high" | "medium" | "low" | "auto") {
                return Some(effort.to_string());
            }
        }
    }
    None
}

fn parse_codex_status_line(tail: &str) -> Option<CodexStatus> {
    // v1.11.2 remediation (Codex v1.11.0 warning): the newest
    // status-line-shaped line is authoritative. Bottom-up scan stops
    // after the first shape match so we never silently fall back to
    // an older `/status` frame whose values are stale. Per-field
    // extraction is independent of the model gate: context + 3 token
    // counts gate the result; model is optional — if the newest
    // line's model token is outside the known allowlist the parser
    // still emits context/tokens and leaves model `None` so cost and
    // model badges stay blank (honest) rather than drift forward.
    for line in tail.lines().rev() {
        if !(line.contains("Context") && line.contains("% used") && line.contains(" · ")) {
            continue;
        }

        let mut context_pct: Option<u8> = None;
        let mut total_tokens: Option<u64> = None;
        let mut input_tokens: Option<u64> = None;
        let mut output_tokens: Option<u64> = None;
        let mut model: Option<String> = None;
        let mut worktree_path: Option<String> = None;
        let mut git_branch: Option<String> = None;
        let mut skip_next_plain_identifier = false;

        for token in line.split(" · ").map(str::trim) {
            // worktree: first ~/- or /-prefixed token
            if worktree_path.is_none() && (token.starts_with('~') || token.starts_with('/')) {
                worktree_path = Some(token.to_string());
                continue;
            }
            // model: provider-known prefix
            if model.is_none()
                && (token.starts_with("gpt-")
                    || token.starts_with("claude-")
                    || token.starts_with("gemini-"))
            {
                model = Some(token.to_string());
                // Project name token sits immediately after model. Skip it.
                skip_next_plain_identifier = true;
                continue;
            }
            // Context pressure (Slice 1)
            if let Some(rest) = token.strip_prefix("Context ")
                && let Some(pct_str) = rest.strip_suffix("% used")
                && let Ok(pct) = pct_str.parse::<u8>()
            {
                context_pct = Some(pct);
                continue;
            }
            // Token counts (Slice 1)
            if total_tokens.is_none()
                && let Some(num) = token.strip_suffix(" used")
                && let Some(n) = parse_count_with_suffix(num.trim())
            {
                total_tokens = Some(n);
                continue;
            }
            if input_tokens.is_none()
                && let Some(num) = token.strip_suffix(" in")
                && let Some(n) = parse_count_with_suffix(num.trim())
            {
                input_tokens = Some(n);
                continue;
            }
            if output_tokens.is_none()
                && let Some(num) = token.strip_suffix(" out")
                && let Some(n) = parse_count_with_suffix(num.trim())
            {
                output_tokens = Some(n);
                continue;
            }
            // Plain identifier: either the project name (skip once) or the branch.
            if git_branch.is_none() && model.is_some() && is_plain_identifier(token) {
                if skip_next_plain_identifier {
                    skip_next_plain_identifier = false;
                    continue;
                }
                git_branch = Some(token.to_string());
                continue;
            }
        }

        // Newest-line authoritative (v1.11.2): stop at the first
        // shape-matching line whether or not every field parsed.
        if let (Some(c), Some(tot), Some(inp), Some(out)) =
            (context_pct, total_tokens, input_tokens, output_tokens)
        {
            let reasoning_effort = parse_codex_reasoning_effort(tail);
            return Some(CodexStatus {
                context_pct: c,
                total_tokens: tot,
                input_tokens: inp,
                output_tokens: out,
                model,
                worktree_path,
                git_branch,
                reasoning_effort,
            });
        }
        return None;
    }
    None
}

fn is_plain_identifier(s: &str) -> bool {
    if s.is_empty() || s.len() > 60 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '_' || c == '.' || c == '-')
}

fn append_codex_runtime_facts(set: &mut SignalSet, tail: &str) {
    for line in tail.lines() {
        append_codex_status_fact(&mut set.runtime_facts, line);
    }
}

fn append_codex_status_fact(facts: &mut Vec<crate::domain::signal::RuntimeFact>, line: &str) {
    let cleaned = clean_codex_status_line(line);
    let Some((raw_label, raw_value)) = cleaned.split_once(':') else {
        return;
    };
    let label = raw_label.trim().to_ascii_lowercase();
    let value = raw_value.trim();
    let Some(kind) = codex_runtime_kind_for_label(&label) else {
        return;
    };

    for item in split_runtime_list(value) {
        push_provider_fact(facts, Provider::Codex, kind, item, 0.9);
    }

    if label == "permissions" && value.to_lowercase().contains("yolo") {
        push_provider_fact(
            facts,
            Provider::Codex,
            RuntimeFactKind::AutoMode,
            "YOLO mode",
            0.9,
        );
    }
}

fn codex_runtime_kind_for_label(label: &str) -> Option<RuntimeFactKind> {
    match label {
        "permissions" | "approval mode" | "ask for approval" => {
            Some(RuntimeFactKind::PermissionMode)
        }
        "collaboration mode" | "auto mode" | "mode" => Some(RuntimeFactKind::AutoMode),
        "sandbox" | "sandbox mode" => Some(RuntimeFactKind::Sandbox),
        "directory" | "worktree" | "cwd" | "additional directories" | "writable roots" => {
            Some(RuntimeFactKind::AllowedDirectory)
        }
        "agents.md" | "agents" => Some(RuntimeFactKind::AgentConfig),
        "tools" | "available tools" | "allowed tools" => Some(RuntimeFactKind::LoadedTool),
        "skills" | "loaded skills" => Some(RuntimeFactKind::LoadedSkill),
        "plugins" | "mcp servers" | "mcp" => Some(RuntimeFactKind::LoadedPlugin),
        "restricted tools" | "disabled tools" | "disallowed tools" => {
            Some(RuntimeFactKind::RestrictedTool)
        }
        _ => None,
    }
}

fn clean_codex_status_line(line: &str) -> String {
    line.trim()
        .trim_matches('│')
        .trim()
        .trim_matches('─')
        .trim()
        .to_string()
}

fn classify_idle_codex(tail: &str, history: &PaneTailHistory) -> Option<IdleCause> {
    if codex_limit_hit(tail) {
        return Some(IdleCause::LimitHit);
    }
    if codex_idle_cursor(tail) {
        return Some(IdleCause::WorkComplete);
    }
    if history.is_still(history.capacity()) {
        return Some(IdleCause::Stale);
    }
    None
}

fn codex_idle_cursor(tail: &str) -> bool {
    // v1.14.1: skip Codex's bottom-status-line when scanning from end.
    // Real Codex tails render the bottom-status-line below the `› `
    // cursor; checking only the last non-empty line therefore misses
    // the idle state. The bottom-status-line has a known structural
    // shape (Context + " · " + "% used"); skip it (and empties) and
    // check the first substantive line above for the prompt glyph.
    for line in tail.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_codex_bottom_status_line(line) {
            continue;
        }
        return line.trim() == "›";
    }
    false
}

/// Returns true if `line` is Codex's bottom-status-line.
///
/// The bottom-status-line sits below the `› ` idle cursor in real
/// Codex tails and would cause the original last-non-empty-line check
/// to miss the cursor entirely. The structural anchor — all three of
/// `"Context "`, `" · "`, and `"% used"` present — is identical to
/// the one used by `parse_codex_status_line` (Slice 1/2). Defensive
/// against UI drift: a future variant lacking any of the three is not
/// skipped, degrading gracefully to existing last-line behavior.
fn is_codex_bottom_status_line(line: &str) -> bool {
    line.contains("Context ") && line.contains("% used") && line.contains(" · ")
}

fn codex_limit_hit(tail: &str) -> bool {
    // Pattern: `5h limit:` followed by `100% used` or `0% left` on same line.
    for line in tail.lines() {
        let lower = line.to_lowercase();
        if lower.contains("5h limit:")
            && (contains_percent_phrase(&lower, "100% used")
                || contains_percent_phrase(&lower, "0% left"))
        {
            return true;
        }
    }
    false
}

fn contains_percent_phrase(line: &str, phrase: &str) -> bool {
    line.match_indices(phrase)
        .any(|(idx, _)| idx == 0 || !line.as_bytes()[idx - 1].is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::ParserContext;
    use crate::adapters::common::PaneTailHistory;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{IdleCause, RuntimeFactKind};
    use crate::policy::claude_settings::ClaudeSettings;
    use crate::policy::pricing::PricingTable;
    use std::io::Write;
    use tempfile::NamedTempFile;

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

    fn ctx<'a>(
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
        }
    }

    // Redacted sample taken from ~/.qmonster/archive/2026-04-23/_65/
    // Codex CLI 0.122.0 status bar.
    const STATUS_LINE: &str = "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp";

    // v1.11.2 remediation (Gemini v1.11.0 must-fix #1 — remove the
    // `insert_for_test` public API surface): build the pricing
    // fixture via the same TOML path operators use. The returned
    // `NamedTempFile` must be kept alive by the caller (`let
    // (pricing, _f) = ...`) so the file is not deleted before the
    // load completes.
    fn pricing_with_gpt_5_4() -> (PricingTable, NamedTempFile) {
        let mut f = NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.00
output_per_1m = 10.00
"#
        )
        .unwrap();
        let t = PricingTable::load_from_toml(f.path()).unwrap();
        (t, f)
    }

    #[test]
    fn codex_adapter_detects_permission_prompt() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            "This action requires approval",
            &pricing,
            &settings,
            &history,
        );
        let set = CodexAdapter.parse(&c);
        assert!(matches!(set.idle_state, Some(IdleCause::PermissionWait)));
    }

    #[test]
    fn codex_adapter_extracts_four_metrics_from_status_line_with_pricing() {
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, STATUS_LINE, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        let cx = set.context_pressure.as_ref().expect("context parsed");
        assert!((cx.value - 0.27).abs() < 0.001);
        assert_eq!(cx.source_kind, SourceKind::ProviderOfficial);

        let tokens = set.token_count.as_ref().expect("tokens parsed");
        assert_eq!(tokens.value, 1_530_000);
        assert_eq!(tokens.source_kind, SourceKind::ProviderOfficial);

        let model = set.model_name.as_ref().expect("model parsed");
        assert_eq!(model.value, "gpt-5.4");
        assert_eq!(model.source_kind, SourceKind::ProviderOfficial);

        let cost = set.cost_usd.as_ref().expect("cost computed");
        // 1.51M in × $1.00 / 1M + 20.4K out × $10.00 / 1M
        //   = 1.51 + 0.204 = 1.714
        assert!((cost.value - 1.714).abs() < 0.01, "got {}", cost.value);
        assert_eq!(cost.source_kind, SourceKind::Estimated);
    }

    #[test]
    fn codex_adapter_leaves_cost_none_when_pricing_table_empty() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, STATUS_LINE, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert!(set.context_pressure.is_some());
        assert!(set.token_count.is_some());
        assert!(set.model_name.is_some());
        assert!(set.cost_usd.is_none());
    }

    #[test]
    fn codex_adapter_falls_back_to_common_when_status_line_absent() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "Press ENTER to continue\nno status bar here";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert!(matches!(set.idle_state, Some(IdleCause::InputWait)));
        assert!(set.context_pressure.is_none());
        assert!(set.token_count.is_none());
        assert!(set.model_name.is_none());
    }

    #[test]
    fn codex_status_box_populates_runtime_facts_without_bottom_status_line() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
│  Permissions:                Full Access              │
│  Collaboration mode:         Default                  │
│  Sandbox:                    danger-full-access       │
│  Directory:                  ~/Qmonster               │
│  Agents.md:                  AGENTS.md                │
│  Tools:                      Bash, Read               │
│  Restricted tools:           WebFetch                 │";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        assert!(set.context_pressure.is_none());
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| { f.kind == RuntimeFactKind::PermissionMode && f.value == "Full Access" })
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::AutoMode && f.value == "Default")
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::Sandbox && f.value == "danger-full-access")
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::AllowedDirectory && f.value == "~/Qmonster")
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::AgentConfig && f.value == "AGENTS.md")
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::LoadedTool && f.value == "Bash")
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::RestrictedTool && f.value == "WebFetch")
        );
    }

    #[test]
    fn codex_yolo_permissions_also_populates_auto_mode() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "permissions: YOLO mode", &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::AutoMode && f.value == "YOLO mode")
        );
    }

    // v1.11.2 regression: unknown newest model token must NOT cause
    // the parser to fall back to an older parseable line. Instead,
    // populate context/tokens from the newest line and leave model/cost
    // None.
    const STATUS_LINE_NEWEST_UNKNOWN_MODEL: &str = "\
Context 50% left · ~/old · gpt-5.4 · old-proj · main · Context 50% used · 5h 90% · weekly 80% · 0.122.0 · 128K window · 500K used · 400K in · 100K out · <redacted> · gp
Context 30% left · ~/Qmonster · codex-mini · Qmonster · main · Context 70% used · 5h 85% · weekly 75% · 0.122.0 · 64K window · 2M used · 1.8M in · 200K out · <redacted> · gp";

    #[test]
    fn codex_adapter_newest_line_authoritative_even_with_unknown_model() {
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            STATUS_LINE_NEWEST_UNKNOWN_MODEL,
            &pricing,
            &settings,
            &history,
        );
        let set = CodexAdapter.parse(&c);

        // context_pressure must come from the NEWEST line (70% used), not
        // the older one (50% used).
        let cx = set.context_pressure.as_ref().expect("context from newest");
        assert!(
            (cx.value - 0.70).abs() < 0.001,
            "context {} should come from newest line",
            cx.value
        );

        // token_count must come from the NEWEST line (2M used), not the older (500K used).
        let tokens = set.token_count.as_ref().expect("tokens from newest");
        assert_eq!(tokens.value, 2_000_000);

        // model unknown → model_name None, cost_usd None even though pricing has gpt-5.4.
        assert!(
            set.model_name.is_none(),
            "unknown model prefix must NOT populate model_name"
        );
        assert!(
            set.cost_usd.is_none(),
            "no model → no cost even if pricing is populated"
        );
    }

    #[test]
    fn codex_adapter_extracts_worktree_and_branch_from_status_line() {
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, STATUS_LINE, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        let worktree = set.worktree_path.as_ref().expect("worktree parsed");
        assert_eq!(worktree.value, "~/Qmonster");
        assert_eq!(worktree.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(worktree.provider, Some(Provider::Codex));
        assert_eq!(worktree.confidence, Some(0.95));

        let branch = set.git_branch.as_ref().expect("branch parsed");
        assert_eq!(branch.value, "main");
        assert_eq!(branch.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(branch.provider, Some(Provider::Codex));
        assert_eq!(branch.confidence, Some(0.95));
    }

    #[test]
    fn codex_adapter_branch_extraction_skips_project_name() {
        // Position order in Codex 0.122.0: Context, worktree, model, project, branch.
        // Project ("Qmonster") and branch ("main") are both plain identifiers;
        // the parser must skip project (the token immediately after model)
        // and pick branch (the next plain identifier after that).
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, STATUS_LINE, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        let branch = set.git_branch.as_ref().expect("branch parsed");
        assert_eq!(
            branch.value, "main",
            "branch must be `main`, not `Qmonster` (the project-name token immediately after model)"
        );
    }

    #[test]
    fn codex_adapter_status_line_without_matching_worktree_token_leaves_worktree_none() {
        // Synthetic status-bar-shaped line with no ~/-prefixed cwd token.
        // Still has all four required fields so the status struct parses;
        // worktree just stays None (per-field independence per v1.11.2).
        let tail = "Context 30% left · no-slash · gpt-5.4 · proj · feat · Context 70% used · 5h 90% · weekly 80% · 0.122.0 · 100K window · 500K used · 400K in · 100K out · <rid> · gp";
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        assert!(
            set.worktree_path.is_none(),
            "no `~/`- or `/`-prefixed token means worktree stays None"
        );
        // context/tokens/model still populate from the same line
        assert!(set.context_pressure.is_some());
        assert!(set.token_count.is_some());
        assert!(set.model_name.is_some());
    }

    const STATUS_BOX_SNIPPET: &str = "│  Model:                       gpt-5.4 (reasoning xhigh, summaries auto)                 │";

    const STATUS_LINE_WITH_BOX_ABOVE: &str = concat!(
        "│  Model:                       gpt-5.4 (reasoning xhigh, summaries auto)                 │\n",
        "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp"
    );

    #[test]
    fn codex_adapter_reasoning_effort_reads_xhigh_from_status_box() {
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            STATUS_LINE_WITH_BOX_ABOVE,
            &pricing,
            &settings,
            &history,
        );
        let set = CodexAdapter.parse(&c);

        let effort = set.reasoning_effort.as_ref().expect("effort parsed");
        assert_eq!(effort.value, "xhigh");
        assert_eq!(effort.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(effort.provider, Some(Provider::Codex));
        assert_eq!(
            effort.confidence,
            Some(0.6),
            "confidence 0.6 encodes the stale-risk of /status box tail retention"
        );
    }

    #[test]
    fn codex_adapter_reasoning_effort_falls_through_when_pattern_absent() {
        // Status bar present (so the line parses) but no `reasoning ...` text.
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, STATUS_LINE, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        assert!(
            set.reasoning_effort.is_none(),
            "no /status box snippet -> reasoning_effort stays None"
        );
        // status bar fields still populate
        assert!(set.context_pressure.is_some());
        assert!(set.token_count.is_some());
    }

    #[test]
    fn codex_adapter_reasoning_effort_absent_when_status_bar_missing() {
        // /status snippet present, but no status bar matches the shape.
        // Because reasoning_effort is populated inside parse_codex_status_line's
        // success path, the whole CodexStatus returns None and no field —
        // reasoning_effort included — is set.
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, STATUS_BOX_SNIPPET, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        assert!(set.reasoning_effort.is_none());
        assert!(set.context_pressure.is_none());
    }

    #[test]
    fn codex_adapter_reasoning_effort_rejects_unknown_values() {
        // Future Codex may add new reasoning-effort values (e.g., `custom`,
        // `max`). Task 5's whitelist (xhigh/high/medium/low/auto) is strict
        // — unknown values must produce None, not surface as a fake
        // provider-official stamp. This test pins the whitelist against
        // a future well-meaning refactor that broadens it.
        let tail = concat!(
            "│  Model:                       gpt-5.4 (reasoning custom, summaries auto)               │\n",
            "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp"
        );
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        assert!(
            set.reasoning_effort.is_none(),
            "unknown reasoning value `custom` must NOT populate reasoning_effort"
        );
        // status-bar fields still populate (per-field independence)
        assert!(set.context_pressure.is_some());
        assert!(set.token_count.is_some());
    }

    #[test]
    fn codex_adapter_reasoning_effort_does_not_match_arbitrary_prose() {
        // v1.12.1 regression (codex-v1.12.0-1 warning): a normal transcript
        // line containing `reasoning high` plus a valid status bar must NOT
        // surface reasoning_effort as ProviderOfficial. Only actual /status
        // box lines (containing `│` box glyph AND `Model:` literal) count.
        let tail = concat!(
            "Actually you should set reasoning high for this run; the prior\n",
            "session used reasoning low and it was insufficient.\n",
            "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp"
        );
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        assert!(
            set.reasoning_effort.is_none(),
            "prose `reasoning high` without /status box structure must NOT surface reasoning_effort"
        );
        // status bar fields still populate
        assert!(set.context_pressure.is_some());
        assert!(set.token_count.is_some());
        assert!(set.model_name.is_some());
    }

    #[test]
    fn codex_adapter_reasoning_effort_uses_newest_status_box_when_multiple_present() {
        // Operator ran /status twice with different effort settings.
        // The newer one (below in tail, later in time) must win.
        let tail = concat!(
            "│  Model:                       gpt-5.4 (reasoning low, summaries auto)                  │\n",
            "... other tail output between the two /status invocations ...\n",
            "│  Model:                       gpt-5.4 (reasoning xhigh, summaries auto)                │\n",
            "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp"
        );
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);

        let effort = set.reasoning_effort.as_ref().expect("effort parsed");
        assert_eq!(
            effort.value, "xhigh",
            "parser must return newest /status box value (xhigh), not oldest (low)"
        );
    }

    #[test]
    fn codex_idle_cursor_at_last_line_yields_work_complete() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "previous response text\n\n› ";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::WorkComplete));
    }

    #[test]
    fn codex_idle_cursor_with_bottom_status_line_trailing_yields_work_complete() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        // Real Codex tails always render the bottom-status-line BELOW
        // the `› ` cursor. The detector must skip that line.
        let tail = "some previous output\n\n› \n\nContext 100% left · ~/Qmonster · gpt-5.4 · proj · main · Context 0% used · 0.122.0 · 0 in · 0 out";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::WorkComplete));
    }

    #[test]
    fn codex_prompt_echo_with_request_text_above_status_line_is_not_idle_cursor() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\n› write tests\n\nContext 100% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 0% used · 0.122.0 · 0 in · 0 out · <redacted> · gp";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn codex_5h_limit_100_used_yields_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "│  5h limit:    [████████████████████] 100% used (resets 07:59)  │";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::LimitHit));
    }

    #[test]
    fn codex_5h_limit_0_left_yields_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "│  5h limit:    [                    ] 0% left  │";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::LimitHit));
    }

    #[test]
    fn codex_5h_limit_100_left_does_not_false_fire_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "│  5h limit:    [████████████████████] 100% left (resets 07:59)  │";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert_ne!(set.idle_state, Some(IdleCause::LimitHit));
    }

    #[test]
    fn codex_active_status_line_no_cursor_yields_none() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "Context 73% left · ~/proj · gpt-5.4 · proj · main · Context 27% used · 0.122.0 · 100K used · 50K in · 50K out";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = CodexAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }
}
