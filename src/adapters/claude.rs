use crate::adapters::ProviderParser;
use crate::adapters::common::parse_common_signals;
use crate::domain::identity::ResolvedIdentity;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, SignalSet};

pub struct ClaudeAdapter;

impl ProviderParser for ClaudeAdapter {
    fn parse(&self, _identity: &ResolvedIdentity, tail: &str) -> SignalSet {
        let mut set = parse_common_signals(tail);
        let lower = tail.to_lowercase();

        // Claude-specific: "claude" + percentage typically indicates
        // context usage. Still Estimated until a provider doc gives us
        // an official structure.
        if set.context_pressure.is_none()
            && let Some(p) = parse_context_percent_claude(&lower)
        {
            set.context_pressure = Some(MetricValue::new(p / 100.0, SourceKind::Estimated));
        }
        set
    }
}

fn parse_context_percent_claude(lower: &str) -> Option<f32> {
    for line in lower.lines() {
        if line.contains("claude") && line.contains('%') {
            let mut digits = String::new();
            let mut seen_dot = false;
            for ch in line.chars() {
                if ch.is_ascii_digit() {
                    digits.push(ch);
                } else if ch == '.' && !seen_dot {
                    digits.push(ch);
                    seen_dot = true;
                } else if ch == '%' {
                    if let Ok(v) = digits.parse::<f32>() {
                        return Some(v);
                    }
                    digits.clear();
                    seen_dot = false;
                } else {
                    digits.clear();
                    seen_dot = false;
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};

    fn id() -> ResolvedIdentity {
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

    #[test]
    fn claude_adapter_inherits_common_signals() {
        let tail = "Press ENTER to continue";
        let set = ClaudeAdapter.parse(&id(), tail);
        assert!(set.waiting_for_input);
    }

    #[test]
    fn claude_adapter_parses_claude_specific_percent() {
        let tail = "claude context 88%";
        let set = ClaudeAdapter.parse(&id(), tail);
        let m = set.context_pressure.expect("parsed");
        assert!((m.value - 0.88).abs() < 0.01);
    }
}
