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
///
/// v1.15.16: `cost_warning_usd` / `cost_critical_usd` are resolved
/// per-pane from the operator's `[cost]` config section + the pane's
/// resolved provider.
///
/// v1.15.17: `context_warning_pct` / `context_critical_pct` and
/// `quota_warning_pct` / `quota_critical_pct` follow the same lift-out
/// pattern from `[context]` and `[quota]` sections respectively. The
/// advisory rules in `policy::rules::advisories` read these fields
/// directly so per-provider overrides flow through without each rule
/// re-touching the config.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolicyGates {
    pub quota_tight: bool,
    pub identity_confidence: IdentityConfidence,
    pub cost_warning_usd: f64,
    pub cost_critical_usd: f64,
    pub context_warning_pct: f32,
    pub context_critical_pct: f32,
    pub quota_warning_pct: f32,
    pub quota_critical_pct: f32,
}

impl Default for PolicyGates {
    fn default() -> Self {
        Self {
            quota_tight: false,
            identity_confidence: IdentityConfidence::Unknown,
            // Mirror Cost/Context/QuotaConfig::default() top-level
            // defaults so unit tests that build a default PolicyGates
            // inherit the same baseline thresholds the production
            // engine sees on a provider that has no override.
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
        }
    }
}

impl PolicyGates {
    pub fn from_config_and_identity(
        token: &crate::app::config::TokenConfig,
        cost: &crate::app::config::CostConfig,
        context: &crate::app::config::ContextConfig,
        quota: &crate::app::config::QuotaConfig,
        provider: crate::domain::identity::Provider,
        conf: IdentityConfidence,
    ) -> Self {
        Self {
            quota_tight: token.quota_tight,
            identity_confidence: conf,
            cost_warning_usd: cost.warning_for(provider),
            cost_critical_usd: cost.critical_for(provider),
            context_warning_pct: context.warning_for(provider),
            context_critical_pct: context.critical_for(provider),
            quota_warning_pct: quota.warning_for(provider),
            quota_critical_pct: quota.critical_for(provider),
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
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
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
        use crate::app::config::{ContextConfig, CostConfig, QuotaConfig, TokenConfig};
        use crate::domain::identity::Provider;
        let cfg = TokenConfig { quota_tight: true };
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let gates = PolicyGates::from_config_and_identity(
            &cfg,
            &cost,
            &context,
            &quota,
            Provider::Codex,
            IdentityConfidence::Medium,
        );
        assert!(gates.quota_tight);
        assert_eq!(gates.identity_confidence, IdentityConfidence::Medium);
        // Codex falls through to top-level CostConfig defaults ($5 / $20).
        assert!((gates.cost_warning_usd - 5.0).abs() < f64::EPSILON);
        assert!((gates.cost_critical_usd - 20.0).abs() < f64::EPSILON);
        // Context + quota fall through to top-level defaults (0.75 / 0.85).
        assert!((gates.context_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((gates.context_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((gates.quota_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((gates.quota_critical_pct - 0.85).abs() < f32::EPSILON);
    }

    #[test]
    fn policy_gates_resolves_per_provider_cost_overrides() {
        // v1.15.16: CostConfig::default() ships per-provider overrides.
        // Claude → $10 / $30, Gemini → $3 / $10, Codex falls through to
        // the top-level $5 / $20.
        use crate::app::config::{ContextConfig, CostConfig, QuotaConfig, TokenConfig};
        use crate::domain::identity::Provider;
        let cfg = TokenConfig { quota_tight: false };
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let claude = PolicyGates::from_config_and_identity(
            &cfg,
            &cost,
            &context,
            &quota,
            Provider::Claude,
            IdentityConfidence::High,
        );
        assert!((claude.cost_warning_usd - 10.0).abs() < f64::EPSILON);
        assert!((claude.cost_critical_usd - 30.0).abs() < f64::EPSILON);
        let codex = PolicyGates::from_config_and_identity(
            &cfg,
            &cost,
            &context,
            &quota,
            Provider::Codex,
            IdentityConfidence::High,
        );
        assert!((codex.cost_warning_usd - 5.0).abs() < f64::EPSILON);
        assert!((codex.cost_critical_usd - 20.0).abs() < f64::EPSILON);
        let gemini = PolicyGates::from_config_and_identity(
            &cfg,
            &cost,
            &context,
            &quota,
            Provider::Gemini,
            IdentityConfidence::High,
        );
        assert!((gemini.cost_warning_usd - 3.0).abs() < f64::EPSILON);
        assert!((gemini.cost_critical_usd - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn policy_gates_resolves_per_provider_context_and_quota_overrides() {
        // v1.15.17: ContextConfig + QuotaConfig follow the same
        // per-provider override pattern as CostConfig. The default
        // top-level threshold is 0.75 / 0.85; an override under
        // [context.gemini] (or [quota.claude], etc.) replaces both
        // values for that provider only. Other providers still see
        // the top-level defaults.
        use crate::app::config::{
            ContextConfig, CostConfig, PressureProviderConfig, QuotaConfig, TokenConfig,
        };
        use crate::domain::identity::Provider;
        let cfg = TokenConfig { quota_tight: false };
        let cost = CostConfig::default();
        let context = ContextConfig {
            gemini: Some(PressureProviderConfig {
                warning_pct: 0.60,
                critical_pct: 0.75,
            }),
            ..ContextConfig::default()
        };
        let quota = QuotaConfig {
            claude: Some(PressureProviderConfig {
                warning_pct: 0.80,
                critical_pct: 0.90,
            }),
            ..QuotaConfig::default()
        };

        // Gemini sees the [context.gemini] override; quota falls
        // through to the top-level QuotaConfig default.
        let gemini = PolicyGates::from_config_and_identity(
            &cfg,
            &cost,
            &context,
            &quota,
            Provider::Gemini,
            IdentityConfidence::High,
        );
        assert!((gemini.context_warning_pct - 0.60).abs() < f32::EPSILON);
        assert!((gemini.context_critical_pct - 0.75).abs() < f32::EPSILON);
        assert!((gemini.quota_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((gemini.quota_critical_pct - 0.85).abs() < f32::EPSILON);

        // Claude sees the [quota.claude] override; context falls
        // through to the top-level ContextConfig default.
        let claude = PolicyGates::from_config_and_identity(
            &cfg,
            &cost,
            &context,
            &quota,
            Provider::Claude,
            IdentityConfidence::High,
        );
        assert!((claude.context_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((claude.context_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((claude.quota_warning_pct - 0.80).abs() < f32::EPSILON);
        assert!((claude.quota_critical_pct - 0.90).abs() < f32::EPSILON);

        // Codex has no override on either; both fall through.
        let codex = PolicyGates::from_config_and_identity(
            &cfg,
            &cost,
            &context,
            &quota,
            Provider::Codex,
            IdentityConfidence::High,
        );
        assert!((codex.context_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((codex.context_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((codex.quota_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((codex.quota_critical_pct - 0.85).abs() < f32::EPSILON);
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
