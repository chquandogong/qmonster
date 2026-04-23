use crate::app::config::ActionsMode;
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

/// Phase 5 P5-3 second execution gate. Distinct from `EffectRunner::permit`
/// (which is the display-layer filter for `RequestedEffect` allow-list).
/// The send-keys path fires only when:
///
/// (a) the operator pressed `p` (explicit confirmation upstream), and
/// (b) `allow_auto_prompt_send = true` in the config.
///
/// This enum is the second gate's verdict per operator keystroke.
/// v1.10.1 remediation moved this type from `src/main.rs` into the
/// pure `policy` module so a future non-TUI surface (headless runner,
/// MCP server) can share the same authority check without pulling in
/// the ratatui/crossterm dependencies of the interactive loop
/// (Gemini v1.10.0 finding #1).
#[derive(Debug, PartialEq, Eq)]
pub enum PromptSendGate {
    /// `actions.mode = observe_only` — record `PromptSendBlocked`;
    /// no `PromptSendAccepted` fires because acceptance itself is
    /// refused at the gate.
    Blocked,
    /// Mode allows, but `allow_auto_prompt_send = false` — record
    /// `PromptSendAccepted` (operator intent is real) AND
    /// `PromptSendBlocked` (system refused execution); no tmux
    /// invocation. v1.10.1 remediation added the trailing
    /// `PromptSendBlocked` event so the audit chain is complete
    /// (Gemini v1.10.0 finding #3).
    AutoSendOff,
    /// Both gates pass — record `PromptSendAccepted`, attempt
    /// `tmux send-keys`, then record `PromptSendCompleted` or
    /// `PromptSendFailed` based on the result.
    Execute,
}

/// Decide the `PromptSendGate` verdict for a given actions mode and
/// auto-send flag. Pure function; callers assemble the right inputs
/// from their context (TUI / headless / future surfaces).
///
/// Gate matrix:
/// - `observe_only` (any flag value)          → `Blocked`
/// - other mode + `allow_auto_prompt_send=false` → `AutoSendOff`
/// - other mode + `allow_auto_prompt_send=true`  → `Execute`
pub fn check_send_gate(mode: ActionsMode, allow_auto_prompt_send: bool) -> PromptSendGate {
    if mode == ActionsMode::ObserveOnly {
        PromptSendGate::Blocked
    } else if !allow_auto_prompt_send {
        PromptSendGate::AutoSendOff
    } else {
        PromptSendGate::Execute
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
        assert!(
            !gates.quota_tight,
            "safety default: quota_tight must be false"
        );
        assert_eq!(
            gates.identity_confidence,
            IdentityConfidence::Unknown,
            "safety default: low-confidence behavior until explicitly raised"
        );
    }

    #[test]
    fn policy_gates_from_config_and_identity_reads_both() {
        use crate::app::config::TokenConfig;
        let cfg = TokenConfig { quota_tight: true };
        let gates = PolicyGates::from_config_and_identity(&cfg, IdentityConfidence::Medium);
        assert!(gates.quota_tight);
        assert_eq!(gates.identity_confidence, IdentityConfidence::Medium);
    }

    // -----------------------------------------------------------------
    // PromptSendGate / check_send_gate (v1.10.1 remediation — moved
    // from src/main.rs per Gemini v1.10.0 finding #1).
    // -----------------------------------------------------------------

    #[test]
    fn check_send_gate_observe_only_is_always_blocked() {
        // Safety-precedence: observe_only refuses acceptance regardless
        // of the auto-send flag — the mode wins over the flag.
        assert_eq!(
            check_send_gate(ActionsMode::ObserveOnly, false),
            PromptSendGate::Blocked
        );
        assert_eq!(
            check_send_gate(ActionsMode::ObserveOnly, true),
            PromptSendGate::Blocked,
            "observe_only must block even when allow_auto_prompt_send is true"
        );
    }

    #[test]
    fn check_send_gate_recommend_only_with_auto_send_off_yields_auto_send_off() {
        // Operator intent is accepted; execution is refused by config.
        // v1.10.1: this path now emits both PromptSendAccepted AND
        // PromptSendBlocked so the audit chain is complete.
        assert_eq!(
            check_send_gate(ActionsMode::RecommendOnly, false),
            PromptSendGate::AutoSendOff
        );
    }

    #[test]
    fn check_send_gate_recommend_only_with_auto_send_on_yields_execute() {
        assert_eq!(
            check_send_gate(ActionsMode::RecommendOnly, true),
            PromptSendGate::Execute
        );
    }

    #[test]
    fn check_send_gate_safe_auto_with_auto_send_on_yields_execute() {
        // SafeAuto is reserved for a later phase but is already a
        // non-observe_only mode; the gate treats it symmetrically with
        // RecommendOnly at the flag layer.
        assert_eq!(
            check_send_gate(ActionsMode::SafeAuto, true),
            PromptSendGate::Execute
        );
    }

    #[test]
    fn check_send_gate_safe_auto_with_auto_send_off_yields_auto_send_off() {
        assert_eq!(
            check_send_gate(ActionsMode::SafeAuto, false),
            PromptSendGate::AutoSendOff
        );
    }
}
