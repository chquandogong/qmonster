use crate::adapters::ProviderParser;
use crate::adapters::common::{parse_common_signals, parse_count_with_suffix};
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, SignalSet};

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
}

impl ProviderParser for CodexAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let tail = ctx.tail;
        let pricing = ctx.pricing;
        let mut set = parse_common_signals(tail);
        let Some(status) = parse_codex_status_line(tail) else {
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

        set
    }
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
        let tokens: Vec<&str> = line.split(" · ").map(str::trim).collect();

        let mut context_pct: Option<u8> = None;
        let mut total_tokens: Option<u64> = None;
        let mut input_tokens: Option<u64> = None;
        let mut output_tokens: Option<u64> = None;
        let mut model: Option<String> = None;

        for tok in &tokens {
            // "Context 27% used"
            if let Some(rest) = tok.strip_prefix("Context ")
                && let Some(pct_str) = rest.strip_suffix("% used")
                && let Ok(pct) = pct_str.parse::<u8>()
            {
                context_pct = Some(pct);
                continue;
            }
            // "1.53M used" — guard against the context "% used" form collision
            if let Some(num) = tok.strip_suffix(" used")
                && !tok.contains("% used")
                && let Some(n) = parse_count_with_suffix(num)
            {
                total_tokens = Some(n);
                continue;
            }
            // "1.51M in"
            if let Some(num) = tok.strip_suffix(" in")
                && let Some(n) = parse_count_with_suffix(num)
            {
                input_tokens = Some(n);
                continue;
            }
            // "20.4K out"
            if let Some(num) = tok.strip_suffix(" out")
                && let Some(n) = parse_count_with_suffix(num)
            {
                output_tokens = Some(n);
                continue;
            }
            // Model name: known provider prefixes
            if model.is_none()
                && (tok.starts_with("gpt-")
                    || tok.starts_with("claude-")
                    || tok.starts_with("gemini-"))
            {
                model = Some((*tok).to_string());
                continue;
            }
        }

        // Newest line is authoritative. Require context + 3 token
        // counts to consider the line a valid status bar; model is
        // optional. Whether we return Some or None here, we do NOT
        // keep scanning upward — an older parseable line must never
        // leak stale ProviderOfficial values.
        return match (context_pct, total_tokens, input_tokens, output_tokens) {
            (Some(c), Some(tot), Some(i), Some(o)) => Some(CodexStatus {
                context_pct: c,
                total_tokens: tot,
                input_tokens: i,
                output_tokens: o,
                model,
            }),
            _ => None,
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::ParserContext;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
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
    ) -> ParserContext<'a> {
        ParserContext {
            identity: id,
            tail,
            pricing,
            claude_settings: settings,
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
        let c = ctx(&id, "This action requires approval", &pricing, &settings);
        let set = CodexAdapter.parse(&c);
        assert!(set.permission_prompt);
    }

    #[test]
    fn codex_adapter_extracts_four_metrics_from_status_line_with_pricing() {
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, STATUS_LINE, &pricing, &settings);
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
        let c = ctx(&id, STATUS_LINE, &pricing, &settings);
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
        let tail = "Press ENTER to continue\nno status bar here";
        let c = ctx(&id, tail, &pricing, &settings);
        let set = CodexAdapter.parse(&c);
        assert!(set.waiting_for_input);
        assert!(set.context_pressure.is_none());
        assert!(set.token_count.is_none());
        assert!(set.model_name.is_none());
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
        let c = ctx(&id, STATUS_LINE_NEWEST_UNKNOWN_MODEL, &pricing, &settings);
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
}
