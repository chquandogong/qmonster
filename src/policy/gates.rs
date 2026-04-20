use crate::domain::identity::IdentityConfidence;

/// Provider-specific recommendations are suppressed on low-confidence
/// panes (r2 rule; Codex C-7). Alerts still fire.
pub fn allow_provider_specific(conf: IdentityConfidence) -> bool {
    matches!(conf, IdentityConfidence::High | IdentityConfidence::Medium)
}

/// Aggressive token-savings recommendations surface only when the
/// operator has opted into quota-tight mode.
pub fn allow_aggressive(quota_tight: bool) -> bool {
    quota_tight
}

/// Bundled gate inputs for `Engine::evaluate`. Built upstream by
/// `app::event_loop` from the current `QmonsterConfig` + per-pane
/// `ResolvedIdentity`. Pure data — the struct does not read config
/// or IO at evaluation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyGates {
    pub quota_tight: bool,
    pub identity_confidence: IdentityConfidence,
}

impl Default for PolicyGates {
    fn default() -> Self {
        Self {
            quota_tight: false,
            identity_confidence: IdentityConfidence::Unknown,
        }
    }
}

impl PolicyGates {
    pub fn from_config_and_identity(
        token: &crate::app::config::TokenConfig,
        conf: IdentityConfidence,
    ) -> Self {
        Self {
            quota_tight: token.quota_tight,
            identity_confidence: conf,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::IdentityConfidence;

    #[test]
    fn provider_specific_allowed_on_high_confidence() {
        assert!(allow_provider_specific(IdentityConfidence::High));
        assert!(allow_provider_specific(IdentityConfidence::Medium));
    }

    #[test]
    fn provider_specific_suppressed_on_low_or_unknown() {
        assert!(!allow_provider_specific(IdentityConfidence::Low));
        assert!(!allow_provider_specific(IdentityConfidence::Unknown));
    }

    #[test]
    fn aggressive_mode_requires_quota_tight_flag() {
        assert!(!allow_aggressive(false));
        assert!(allow_aggressive(true));
    }

    #[test]
    fn quota_tight_gate_reads_from_config_true() {
        let gates = PolicyGates {
            quota_tight: true,
            identity_confidence: IdentityConfidence::High,
        };
        assert!(gates.quota_tight);
    }

    #[test]
    fn quota_tight_gate_defaults_to_false() {
        let gates = PolicyGates::default();
        assert!(!gates.quota_tight, "safety default: quota_tight must be false");
        assert_eq!(gates.identity_confidence, IdentityConfidence::Unknown,
            "safety default: low-confidence behavior until explicitly raised");
    }

    #[test]
    fn policy_gates_from_config_and_identity_reads_both() {
        use crate::app::config::TokenConfig;
        let cfg = TokenConfig { quota_tight: true };
        let gates = PolicyGates::from_config_and_identity(&cfg, IdentityConfidence::Medium);
        assert!(gates.quota_tight);
        assert_eq!(gates.identity_confidence, IdentityConfidence::Medium);
    }
}
