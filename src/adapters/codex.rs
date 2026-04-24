use crate::adapters::ProviderParser;
use crate::adapters::common::{parse_common_signals, parse_count_with_suffix};
use crate::domain::identity::{Provider, ResolvedIdentity};
use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, SignalSet};
use crate::policy::pricing::PricingTable;

pub struct CodexAdapter;

struct CodexStatus {
    context_pct: u8,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    model: String,
}

impl ProviderParser for CodexAdapter {
    fn parse(&self, _identity: &ResolvedIdentity, tail: &str, pricing: &PricingTable) -> SignalSet {
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
        set.model_name = Some(
            MetricValue::new(status.model.clone(), SourceKind::ProviderOfficial)
                .with_confidence(0.95)
                .with_provider(Provider::Codex),
        );
        set.cost_usd = pricing.lookup(Provider::Codex, &status.model).map(|rates| {
            let cost = (status.input_tokens as f64 * rates.input_per_1m
                + status.output_tokens as f64 * rates.output_per_1m)
                / 1_000_000.0;
            MetricValue::new(cost, SourceKind::Estimated)
                .with_confidence(0.7)
                .with_provider(Provider::Codex)
        });

        set
    }
}

fn parse_codex_status_line(tail: &str) -> Option<CodexStatus> {
    // bottom-up — prefer the most recent frame's status bar over the
    // /status command's bordered box output (which goes stale).
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

        // Require all four to consider the line a valid status bar.
        if let (Some(c), Some(tot), Some(i), Some(o), Some(m)) = (
            context_pct,
            total_tokens,
            input_tokens,
            output_tokens,
            model,
        ) {
            return Some(CodexStatus {
                context_pct: c,
                total_tokens: tot,
                input_tokens: i,
                output_tokens: o,
                model: m,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};
    use crate::domain::origin::SourceKind;
    use crate::policy::pricing::{PricingRates, PricingTable};

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

    // Redacted sample taken from ~/.qmonster/archive/2026-04-23/_65/
    // Codex CLI 0.122.0 status bar.
    const STATUS_LINE: &str = "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp";

    fn pricing_with_gpt_5_4() -> PricingTable {
        let mut t = PricingTable::empty();
        t.insert_for_test(
            Provider::Codex,
            "gpt-5.4".into(),
            PricingRates {
                input_per_1m: 1.00,
                output_per_1m: 10.00,
            },
        );
        t
    }

    #[test]
    fn codex_adapter_detects_permission_prompt() {
        let set = CodexAdapter.parse(
            &id(),
            "This action requires approval",
            &PricingTable::empty(),
        );
        assert!(set.permission_prompt);
    }

    #[test]
    fn codex_adapter_extracts_four_metrics_from_status_line_with_pricing() {
        let set = CodexAdapter.parse(&id(), STATUS_LINE, &pricing_with_gpt_5_4());

        let ctx = set.context_pressure.as_ref().expect("context parsed");
        assert!((ctx.value - 0.27).abs() < 0.001);
        assert_eq!(ctx.source_kind, SourceKind::ProviderOfficial);

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
        let set = CodexAdapter.parse(&id(), STATUS_LINE, &PricingTable::empty());
        assert!(set.context_pressure.is_some());
        assert!(set.token_count.is_some());
        assert!(set.model_name.is_some());
        assert!(set.cost_usd.is_none());
    }

    #[test]
    fn codex_adapter_falls_back_to_common_when_status_line_absent() {
        let tail = "Press ENTER to continue\nno status bar here";
        let set = CodexAdapter.parse(&id(), tail, &PricingTable::empty());
        assert!(set.waiting_for_input);
        assert!(set.context_pressure.is_none());
        assert!(set.token_count.is_none());
        assert!(set.model_name.is_none());
    }
}
