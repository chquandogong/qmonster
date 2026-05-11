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
    pub security_posture_advisories: bool,
    pub identity_confidence: IdentityConfidence,
    pub cost_warning_usd: f64,
    pub cost_critical_usd: f64,
    pub context_warning_pct: f32,
    pub context_critical_pct: f32,
    pub quota_warning_pct: f32,
    pub quota_critical_pct: f32,
    pub quota_5h_warning_pct: f32,
    pub quota_5h_critical_pct: f32,
    pub quota_weekly_warning_pct: f32,
    pub quota_weekly_critical_pct: f32,
    /// Phase F F-7-config (v1.28.0): operator-tunable cache-rule
    /// thresholds. Populated from `[cache]` in `qmonster.toml` via
    /// `from_inputs`. Defaults match the F-7/F-7b hardcoded
    /// constants exactly so no behavior change when the section is absent.
    pub cache_hot_ratio: f64,
    pub cache_cold_ratio: f64,
    pub cache_hot_low_ctx: f32,
    pub cache_cold_high_ctx: f32,
    pub cache_drift_drop: f64,
    pub cache_drift_min_samples: usize,
    /// Phase F F-7d (v1.35.0): operator-tunable reset-rule
    /// thresholds. Populated from `[reset]` in `qmonster.toml` via
    /// `from_inputs`. Defaults match the F-7c
    /// hardcoded constants exactly so no behavior change when the
    /// section is absent.
    pub reset_wait_pressure: f32,
    pub reset_wait_eta_secs: u64,
    pub reset_snapshot_pressure: f32,
    pub reset_snapshot_eta_secs: u64,
    /// Phase H (v1.42.0): when `true`, `app::auto_snapshot::maybe_auto_snapshot`
    /// auto-actuates the matching `Recommendation` once per
    /// `(pane_id, quota_kind, window_id)`. Mirrors `[reset] auto_snapshot`.
    pub reset_auto_snapshot: bool,
    /// Phase 7 v1 (v1.43.0): when `false`, `eval_anomalies` returns
    /// an empty Vec immediately. Mirrors `[anomaly] enabled`.
    pub anomaly_enabled: bool,
    /// Rolling-window length for every detector.
    pub anomaly_window_polls: usize,
    /// Minimum confidence for a detector signal to reach `PaneReport`.
    pub anomaly_min_confidence: crate::domain::anomaly::AnomalyConfidence,
    /// IdentityChurn detector: minimum (provider, path) flips inside
    /// `anomaly_window_polls`.
    pub anomaly_identity_churn_min_flips: usize,
    /// ErrorBurst detector: minimum error rate over the window.
    pub anomaly_error_burst_threshold: f32,
    /// CacheDiscontinuity detector: minimum cache_hit_ratio drop.
    pub anomaly_cache_discontinuity_drop: f32,
    /// CrossPaneEditCluster detector: minimum same-path findings.
    pub anomaly_cross_pane_cluster_min_findings: usize,
    /// Phase 7 v2 (v1.45.0): CostSlope detector threshold (USD/hour).
    pub anomaly_cost_slope_usd_per_hour: f64,
    /// Phase 7 v2 (v1.45.0): TokenSlope detector threshold (tokens/poll).
    pub anomaly_token_slope_input_per_poll: u64,
    /// Phase 7 v2 (v1.45.0): MemoryGrowth detector threshold (MB).
    pub anomaly_memory_growth_mb: f64,
    /// Phase 7 v3 (v1.46.0): per-kind minimum confidence for promotion.
    pub anomaly_promote_identity_churn: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_error_burst: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_cache_discontinuity: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_cross_pane_edit_cluster: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_cost_slope: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_token_slope: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_memory_growth: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_subagent_side_effect: crate::domain::anomaly::AnomalyConfidence,
    /// Phase D D1 (v1.17.0): opt-in cross-window concurrent-work
    /// detection. When `true`, the cross-pane rule emits
    /// `CrossPaneKind::CrossWindowConcurrentWork` for groups whose panes
    /// share `current_path` + `git_branch` but live in two or more tmux
    /// windows. Stays `false` by default to honor the observe-first /
    /// recommend-only principle and keep alert volume predictable for
    /// operators who legitimately keep the same repo open across
    /// windows (e.g. a scratch session next to a main implementation
    /// session).
    pub cross_window_findings: bool,
    /// Phase D D2 (v1.18.0): opt-in identity-drift detection. When
    /// `true`, `policy::rules::identity_drift` compares each pane's
    /// resolved provider + `current_path` against the last observed
    /// snapshot and emits a passive `Concern` recommendation on
    /// transition. Stays `false` by default — most operators accept
    /// CLI swaps as routine, and the resolver already tolerates them.
    pub identity_drift_findings: bool,
    /// Phase F F-8 (multi-pane orchestration): when `true`, the
    /// cross-pane rule emits `CrossPaneKind::ConcurrentFileEdit` when
    /// two or more busy Main/Review panes have recently touched the
    /// same absolute file path. Stays `false` by default because
    /// active-file detection is `Heuristic` (tail-derived from
    /// provider tool-call markers) and operators who run a
    /// `codex-review` pane alongside a Claude main pane often read
    /// each other's files intentionally.
    pub cross_pane_file_findings: bool,
    /// Phase F F-9 (dynamic profile switching): when `true`, the
    /// `profile_switch` rule may emit a passive `Concern`
    /// recommendation that nudges the operator from the current
    /// healthy-state baseline profile toward a more conservative
    /// variant (`*-script-low-token` / `*-review`) when the pane's
    /// recent error rate exceeds `profile_switch_error_rate`. Stays
    /// `false` by default — the recommender only fires after a
    /// configured number of polls, and operators who deliberately run
    /// experimental code expect short bursts of errors to be normal.
    pub profile_switch_enabled: bool,
    /// Number of recent polls used to compute the per-pane error
    /// rate consumed by `profile_switch`. Mirrors the F-7b cache-drift
    /// `cache_drift_min_samples` pattern: the rule only fires once
    /// the window is full so a single transient error never flips
    /// the recommendation. Default 10 polls.
    pub profile_switch_window: usize,
    /// Error-rate (0..1) at which `profile_switch` fires. Comparison
    /// is `>=`, so the default 0.5 means "≥ 5 of the last 10 polls
    /// flagged `error_hint`". Operators on chronically-noisy
    /// workloads can raise this to 0.7 or 0.8 to keep the rec quiet.
    pub profile_switch_error_rate: f32,
}

impl Default for PolicyGates {
    fn default() -> Self {
        Self {
            quota_tight: false,
            security_posture_advisories: false,
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
            quota_5h_warning_pct: 0.75,
            quota_5h_critical_pct: 0.85,
            quota_weekly_warning_pct: 0.75,
            quota_weekly_critical_pct: 0.85,
            cache_hot_ratio: 0.6,
            cache_cold_ratio: 0.3,
            cache_hot_low_ctx: 0.7,
            cache_cold_high_ctx: 0.6,
            cache_drift_drop: 0.30,
            cache_drift_min_samples: 4,
            reset_wait_pressure: 0.85,
            reset_wait_eta_secs: 30 * 60,
            reset_snapshot_pressure: 0.50,
            reset_snapshot_eta_secs: 5 * 60,
            reset_auto_snapshot: false,
            anomaly_enabled: false,
            anomaly_window_polls: 20,
            anomaly_min_confidence: crate::domain::anomaly::AnomalyConfidence::Medium,
            anomaly_identity_churn_min_flips: 3,
            anomaly_error_burst_threshold: 0.5,
            anomaly_cache_discontinuity_drop: 0.30,
            anomaly_cross_pane_cluster_min_findings: 3,
            anomaly_cost_slope_usd_per_hour: 20.0,
            anomaly_token_slope_input_per_poll: 20_000,
            anomaly_memory_growth_mb: 1024.0,
            anomaly_promote_identity_churn: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_error_burst: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_cache_discontinuity: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_cross_pane_edit_cluster:
                crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_cost_slope: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_token_slope: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_memory_growth: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_subagent_side_effect: crate::domain::anomaly::AnomalyConfidence::Medium,
            cross_window_findings: false,
            identity_drift_findings: false,
            cross_pane_file_findings: false,
            profile_switch_enabled: false,
            profile_switch_window: 10,
            profile_switch_error_rate: 0.5,
        }
    }
}

/// Bundled input bag for `PolicyGates::from_inputs`. v1.36.0 lifts the
/// previous 9-positional-arg `from_config_and_identity` signature into
/// this struct so future config additions (e.g. a `[reset]` follow-up
/// or an action-mode override) can extend the input surface without
/// touching every call site. Closes Codex / Gemini OBS feedback from
/// the v1.35.x confirm round.
#[derive(Debug, Clone, Copy)]
pub struct PolicyGateInputs<'a> {
    pub token: &'a crate::app::config::TokenConfig,
    pub cost: &'a crate::app::config::CostConfig,
    pub context: &'a crate::app::config::ContextConfig,
    pub quota: &'a crate::app::config::QuotaConfig,
    pub security: &'a crate::app::config::SecurityConfig,
    pub cache: &'a crate::app::config::CacheConfig,
    pub reset: &'a crate::app::config::ResetConfig,
    pub profile_switch: &'a crate::app::config::ProfileSwitchConfig,
    pub anomaly: &'a crate::app::config::AnomalyConfig,
    pub provider: crate::domain::identity::Provider,
    pub confidence: IdentityConfidence,
}

fn parse_min_confidence(s: &str) -> crate::domain::anomaly::AnomalyConfidence {
    match s {
        "low" => crate::domain::anomaly::AnomalyConfidence::Low,
        "high" => crate::domain::anomaly::AnomalyConfidence::High,
        _ => crate::domain::anomaly::AnomalyConfidence::Medium, // fallback for "medium" or unknown
    }
}

impl PolicyGates {
    /// Phase 7 v3 (v1.46.0): per-kind promotion threshold lookup.
    pub fn promote_min_confidence(
        &self,
        kind: crate::domain::anomaly::AnomalyKind,
    ) -> crate::domain::anomaly::AnomalyConfidence {
        use crate::domain::anomaly::AnomalyKind;
        match kind {
            AnomalyKind::IdentityChurn => self.anomaly_promote_identity_churn,
            AnomalyKind::ErrorBurst => self.anomaly_promote_error_burst,
            AnomalyKind::CacheDiscontinuity => self.anomaly_promote_cache_discontinuity,
            AnomalyKind::CrossPaneEditCluster => self.anomaly_promote_cross_pane_edit_cluster,
            AnomalyKind::CostSlope => self.anomaly_promote_cost_slope,
            AnomalyKind::TokenSlope => self.anomaly_promote_token_slope,
            AnomalyKind::MemoryGrowth => self.anomaly_promote_memory_growth,
            AnomalyKind::SubagentSideEffect => self.anomaly_promote_subagent_side_effect,
        }
    }

    pub fn from_inputs(inputs: PolicyGateInputs<'_>) -> Self {
        let PolicyGateInputs {
            token,
            cost,
            context,
            quota,
            security,
            cache,
            reset,
            profile_switch,
            anomaly,
            provider,
            confidence,
        } = inputs;
        Self {
            quota_tight: token.quota_tight,
            security_posture_advisories: security.posture_advisories,
            identity_confidence: confidence,
            cost_warning_usd: cost.warning_for(provider),
            cost_critical_usd: cost.critical_for(provider),
            context_warning_pct: context.warning_for(provider),
            context_critical_pct: context.critical_for(provider),
            quota_warning_pct: quota.warning_for(provider),
            quota_critical_pct: quota.critical_for(provider),
            quota_5h_warning_pct: quota
                .warning_for_window(provider, crate::app::config::QuotaWindow::FiveHour),
            quota_5h_critical_pct: quota
                .critical_for_window(provider, crate::app::config::QuotaWindow::FiveHour),
            quota_weekly_warning_pct: quota
                .warning_for_window(provider, crate::app::config::QuotaWindow::Weekly),
            quota_weekly_critical_pct: quota
                .critical_for_window(provider, crate::app::config::QuotaWindow::Weekly),
            cache_hot_ratio: cache.hot_ratio_threshold,
            cache_cold_ratio: cache.cold_ratio_threshold,
            cache_hot_low_ctx: cache.hot_low_ctx_threshold,
            cache_cold_high_ctx: cache.cold_high_ctx_threshold,
            cache_drift_drop: cache.drift_drop_threshold,
            cache_drift_min_samples: cache.drift_min_samples,
            reset_wait_pressure: reset.wait_pressure_threshold,
            reset_wait_eta_secs: reset.wait_eta_secs,
            reset_snapshot_pressure: reset.snapshot_pressure_threshold,
            reset_snapshot_eta_secs: reset.snapshot_eta_secs,
            reset_auto_snapshot: reset.auto_snapshot,
            anomaly_enabled: anomaly.enabled,
            anomaly_window_polls: anomaly.window_polls,
            anomaly_min_confidence: parse_min_confidence(&anomaly.min_confidence),
            anomaly_identity_churn_min_flips: anomaly.identity_churn_min_flips,
            anomaly_error_burst_threshold: anomaly.error_burst_threshold,
            anomaly_cache_discontinuity_drop: anomaly.cache_discontinuity_drop,
            anomaly_cross_pane_cluster_min_findings: anomaly.cross_pane_cluster_min_findings,
            anomaly_cost_slope_usd_per_hour: anomaly.cost_slope_usd_per_hour,
            anomaly_token_slope_input_per_poll: anomaly.token_slope_input_per_poll,
            anomaly_memory_growth_mb: anomaly.memory_growth_mb,
            anomaly_promote_identity_churn: parse_min_confidence(&anomaly.promote.identity_churn),
            anomaly_promote_error_burst: parse_min_confidence(&anomaly.promote.error_burst),
            anomaly_promote_cache_discontinuity: parse_min_confidence(
                &anomaly.promote.cache_discontinuity,
            ),
            anomaly_promote_cross_pane_edit_cluster: parse_min_confidence(
                &anomaly.promote.cross_pane_edit_cluster,
            ),
            anomaly_promote_cost_slope: parse_min_confidence(&anomaly.promote.cost_slope),
            anomaly_promote_token_slope: parse_min_confidence(&anomaly.promote.token_slope),
            anomaly_promote_memory_growth: parse_min_confidence(&anomaly.promote.memory_growth),
            anomaly_promote_subagent_side_effect: parse_min_confidence(
                &anomaly.promote.subagent_side_effect,
            ),
            cross_window_findings: security.cross_window_findings,
            identity_drift_findings: security.identity_drift_findings,
            cross_pane_file_findings: security.cross_pane_file_findings,
            profile_switch_enabled: profile_switch.enabled,
            profile_switch_window: profile_switch.window_polls,
            profile_switch_error_rate: profile_switch.error_rate_threshold,
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
    fn provider_specific_suppressed_on_identity_conflict() {
        assert!(!allow_provider_specific(IdentityConfidence::Conflict));
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
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::High,
            cost_warning_usd: 5.0,
            cost_critical_usd: 20.0,
            context_warning_pct: 0.75,
            context_critical_pct: 0.85,
            quota_warning_pct: 0.75,
            quota_critical_pct: 0.85,
            quota_5h_warning_pct: 0.75,
            quota_5h_critical_pct: 0.85,
            quota_weekly_warning_pct: 0.75,
            quota_weekly_critical_pct: 0.85,
            cache_hot_ratio: 0.6,
            cache_cold_ratio: 0.3,
            cache_hot_low_ctx: 0.7,
            cache_cold_high_ctx: 0.6,
            cache_drift_drop: 0.30,
            cache_drift_min_samples: 4,
            reset_wait_pressure: 0.85,
            reset_wait_eta_secs: 30 * 60,
            reset_snapshot_pressure: 0.50,
            reset_snapshot_eta_secs: 5 * 60,
            reset_auto_snapshot: false,
            anomaly_enabled: false,
            anomaly_window_polls: 20,
            anomaly_min_confidence: crate::domain::anomaly::AnomalyConfidence::Medium,
            anomaly_identity_churn_min_flips: 3,
            anomaly_error_burst_threshold: 0.5,
            anomaly_cache_discontinuity_drop: 0.30,
            anomaly_cross_pane_cluster_min_findings: 3,
            anomaly_cost_slope_usd_per_hour: 20.0,
            anomaly_token_slope_input_per_poll: 20_000,
            anomaly_memory_growth_mb: 1024.0,
            cross_window_findings: false,
            identity_drift_findings: false,
            cross_pane_file_findings: false,
            profile_switch_enabled: false,
            profile_switch_window: 10,
            profile_switch_error_rate: 0.5,
            ..PolicyGates::default()
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
    fn policy_gates_from_inputs_reads_both() {
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, QuotaConfig, SecurityConfig, TokenConfig,
        };
        use crate::domain::identity::Provider;
        let cfg = TokenConfig { quota_tight: true };
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let security = SecurityConfig {
            posture_advisories: true,
            cross_window_findings: false,
            identity_drift_findings: false,
            cross_pane_file_findings: false,
        };
        let cache = CacheConfig {
            hot_ratio_threshold: 0.45,
            cold_ratio_threshold: 0.20,
            hot_low_ctx_threshold: 0.65,
            cold_high_ctx_threshold: 0.72,
            drift_drop_threshold: 0.25,
            drift_min_samples: 6,
        };
        let reset = crate::app::config::ResetConfig::default();
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();
        let anomaly = crate::app::config::AnomalyConfig::default();
        let gates = PolicyGates::from_inputs(PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider: Provider::Codex,
            confidence: IdentityConfidence::Medium,
        });
        assert!(gates.quota_tight);
        assert!(gates.security_posture_advisories);
        assert_eq!(gates.identity_confidence, IdentityConfidence::Medium);
        // Codex falls through to top-level CostConfig defaults ($5 / $20).
        assert!((gates.cost_warning_usd - 5.0).abs() < f64::EPSILON);
        assert!((gates.cost_critical_usd - 20.0).abs() < f64::EPSILON);
        // Context + quota fall through to top-level defaults (0.75 / 0.85).
        assert!((gates.context_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((gates.context_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((gates.quota_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((gates.quota_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((gates.quota_5h_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((gates.quota_5h_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((gates.quota_weekly_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((gates.quota_weekly_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((gates.cache_hot_ratio - 0.45).abs() < f64::EPSILON);
        assert!((gates.cache_cold_ratio - 0.20).abs() < f64::EPSILON);
        assert!((gates.cache_hot_low_ctx - 0.65).abs() < f32::EPSILON);
        assert!((gates.cache_cold_high_ctx - 0.72).abs() < f32::EPSILON);
        assert!((gates.cache_drift_drop - 0.25).abs() < f64::EPSILON);
        assert_eq!(gates.cache_drift_min_samples, 6);
    }

    #[test]
    fn policy_gates_propagates_custom_reset_config() {
        // Codex v1.35.x confirm round suggested follow-up: lock that
        // non-default ResetConfig values flow into PolicyGates.reset_*
        // through from_inputs. Mirrors the cache-config propagation
        // assertions in policy_gates_from_inputs_reads_both above.
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, QuotaConfig, ResetConfig, SecurityConfig,
            TokenConfig,
        };
        use crate::domain::identity::Provider;
        let cfg = TokenConfig::default();
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let security = SecurityConfig::default();
        let cache = CacheConfig::default();
        let reset = ResetConfig {
            wait_pressure_threshold: 0.72,
            wait_eta_secs: 45 * 60,
            snapshot_pressure_threshold: 0.40,
            snapshot_eta_secs: 10 * 60,
            ..ResetConfig::default()
        };
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();
        let anomaly = crate::app::config::AnomalyConfig::default();
        let gates = PolicyGates::from_inputs(PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider: Provider::Claude,
            confidence: IdentityConfidence::High,
        });
        assert!((gates.reset_wait_pressure - 0.72).abs() < f32::EPSILON);
        assert_eq!(gates.reset_wait_eta_secs, 45 * 60);
        assert!((gates.reset_snapshot_pressure - 0.40).abs() < f32::EPSILON);
        assert_eq!(gates.reset_snapshot_eta_secs, 10 * 60);
    }

    #[test]
    fn from_inputs_propagates_reset_auto_snapshot() {
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, QuotaConfig, ResetConfig, SecurityConfig,
            TokenConfig,
        };
        use crate::domain::identity::Provider;
        let cfg = TokenConfig::default();
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let security = SecurityConfig::default();
        let cache = CacheConfig::default();
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();
        let anomaly = crate::app::config::AnomalyConfig::default();

        let reset_on = ResetConfig {
            auto_snapshot: true,
            ..ResetConfig::default()
        };
        let gates = PolicyGates::from_inputs(PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset_on,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider: Provider::Claude,
            confidence: IdentityConfidence::High,
        });
        assert!(gates.reset_auto_snapshot);

        let reset_off = ResetConfig {
            auto_snapshot: false,
            ..ResetConfig::default()
        };
        let gates_off = PolicyGates::from_inputs(PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset_off,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider: Provider::Claude,
            confidence: IdentityConfidence::High,
        });
        assert!(!gates_off.reset_auto_snapshot);
    }

    #[test]
    fn policy_gates_resolves_per_provider_cost_overrides() {
        // v1.15.16: CostConfig::default() ships per-provider overrides.
        // Claude → $10 / $30, Gemini → $3 / $10, Codex falls through to
        // the top-level $5 / $20.
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, QuotaConfig, SecurityConfig, TokenConfig,
        };
        use crate::domain::identity::Provider;
        let cfg = TokenConfig { quota_tight: false };
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let security = SecurityConfig::default();
        let cache = CacheConfig::default();
        let reset = crate::app::config::ResetConfig::default();
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();
        let anomaly = crate::app::config::AnomalyConfig::default();
        let inputs_for = |provider: Provider| PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider,
            confidence: IdentityConfidence::High,
        };
        let claude = PolicyGates::from_inputs(inputs_for(Provider::Claude));
        assert!((claude.cost_warning_usd - 10.0).abs() < f64::EPSILON);
        assert!((claude.cost_critical_usd - 30.0).abs() < f64::EPSILON);
        let codex = PolicyGates::from_inputs(inputs_for(Provider::Codex));
        assert!((codex.cost_warning_usd - 5.0).abs() < f64::EPSILON);
        assert!((codex.cost_critical_usd - 20.0).abs() < f64::EPSILON);
        let gemini = PolicyGates::from_inputs(inputs_for(Provider::Gemini));
        assert!((gemini.cost_warning_usd - 3.0).abs() < f64::EPSILON);
        assert!((gemini.cost_critical_usd - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn policy_gates_resolves_per_provider_context_and_quota_overrides() {
        // v1.15.17: ContextConfig + QuotaConfig follow the same
        // per-provider override pattern as CostConfig. The default
        // top-level threshold is 0.75 / 0.85; an override under
        // [context.gemini] replaces both values for that provider only.
        // Quota can now override Claude/Codex 5h and weekly windows
        // independently.
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, PressureProviderConfig, QuotaConfig,
            SecurityConfig, TokenConfig,
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
            claude_5h: Some(PressureProviderConfig {
                warning_pct: 0.80,
                critical_pct: 0.90,
            }),
            claude_weekly: Some(PressureProviderConfig {
                warning_pct: 0.70,
                critical_pct: 0.88,
            }),
            codex_weekly: Some(PressureProviderConfig {
                warning_pct: 0.65,
                critical_pct: 0.82,
            }),
            ..QuotaConfig::default()
        };
        let security = SecurityConfig::default();
        let cache = CacheConfig::default();
        let reset = crate::app::config::ResetConfig::default();
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();
        let anomaly = crate::app::config::AnomalyConfig::default();
        let inputs_for = |provider: Provider| PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider,
            confidence: IdentityConfidence::High,
        };

        // Gemini sees the [context.gemini] override; quota falls
        // through to the top-level QuotaConfig default.
        let gemini = PolicyGates::from_inputs(inputs_for(Provider::Gemini));
        assert!((gemini.context_warning_pct - 0.60).abs() < f32::EPSILON);
        assert!((gemini.context_critical_pct - 0.75).abs() < f32::EPSILON);
        assert!((gemini.quota_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((gemini.quota_critical_pct - 0.85).abs() < f32::EPSILON);

        // Claude sees the split [quota.claude_*] overrides; context
        // falls through to the top-level ContextConfig default.
        let claude = PolicyGates::from_inputs(inputs_for(Provider::Claude));
        assert!((claude.context_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((claude.context_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((claude.quota_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((claude.quota_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((claude.quota_5h_warning_pct - 0.80).abs() < f32::EPSILON);
        assert!((claude.quota_5h_critical_pct - 0.90).abs() < f32::EPSILON);
        assert!((claude.quota_weekly_warning_pct - 0.70).abs() < f32::EPSILON);
        assert!((claude.quota_weekly_critical_pct - 0.88).abs() < f32::EPSILON);

        // Codex has only a weekly override; 5h falls through.
        let codex = PolicyGates::from_inputs(inputs_for(Provider::Codex));
        assert!((codex.context_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((codex.context_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((codex.quota_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((codex.quota_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((codex.quota_5h_warning_pct - 0.75).abs() < f32::EPSILON);
        assert!((codex.quota_5h_critical_pct - 0.85).abs() < f32::EPSILON);
        assert!((codex.quota_weekly_warning_pct - 0.65).abs() < f32::EPSILON);
        assert!((codex.quota_weekly_critical_pct - 0.82).abs() < f32::EPSILON);
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

    #[test]
    fn from_inputs_propagates_anomaly_config() {
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, QuotaConfig, ResetConfig, SecurityConfig,
            TokenConfig,
        };
        use crate::domain::anomaly::AnomalyConfidence;
        use crate::domain::identity::Provider;
        let cfg = TokenConfig::default();
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let security = SecurityConfig::default();
        let cache = CacheConfig::default();
        let reset = ResetConfig::default();
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();
        let anomaly = crate::app::config::AnomalyConfig {
            enabled: true,
            window_polls: 25,
            min_confidence: "high".to_string(),
            identity_churn_min_flips: 4,
            error_burst_threshold: 0.7,
            cache_discontinuity_drop: 0.40,
            cross_pane_cluster_min_findings: 5,
            cost_slope_usd_per_hour: 20.0,
            token_slope_input_per_poll: 20_000,
            memory_growth_mb: 1024.0,
            retention_days: 30,
            promote: crate::app::config::AnomalyPromoteConfig::default(),
        };

        let gates = PolicyGates::from_inputs(PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider: Provider::Claude,
            confidence: IdentityConfidence::High,
        });
        assert!(gates.anomaly_enabled);
        assert_eq!(gates.anomaly_window_polls, 25);
        assert_eq!(gates.anomaly_min_confidence, AnomalyConfidence::High);
        assert_eq!(gates.anomaly_identity_churn_min_flips, 4);
        assert!((gates.anomaly_error_burst_threshold - 0.7).abs() < f32::EPSILON);
        assert!((gates.anomaly_cache_discontinuity_drop - 0.40).abs() < f32::EPSILON);
        assert_eq!(gates.anomaly_cross_pane_cluster_min_findings, 5);
    }

    #[test]
    fn from_inputs_min_confidence_string_maps_to_medium_for_invalid() {
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, QuotaConfig, ResetConfig, SecurityConfig,
            TokenConfig,
        };
        use crate::domain::anomaly::AnomalyConfidence;
        use crate::domain::identity::Provider;
        let cfg = TokenConfig::default();
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let security = SecurityConfig::default();
        let cache = CacheConfig::default();
        let reset = ResetConfig::default();
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();
        let anomaly = crate::app::config::AnomalyConfig {
            min_confidence: "garbage".to_string(),
            ..crate::app::config::AnomalyConfig::default()
        };

        let gates = PolicyGates::from_inputs(PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider: Provider::Claude,
            confidence: IdentityConfidence::High,
        });
        assert_eq!(
            gates.anomaly_min_confidence,
            AnomalyConfidence::Medium,
            "fallback"
        );
    }

    #[test]
    fn from_inputs_min_confidence_low_and_high() {
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, QuotaConfig, ResetConfig, SecurityConfig,
            TokenConfig,
        };
        use crate::domain::anomaly::AnomalyConfidence;
        use crate::domain::identity::Provider;
        let cfg = TokenConfig::default();
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let security = SecurityConfig::default();
        let cache = CacheConfig::default();
        let reset = ResetConfig::default();
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();

        for (s, expected) in [
            ("low", AnomalyConfidence::Low),
            ("high", AnomalyConfidence::High),
        ] {
            let anomaly = crate::app::config::AnomalyConfig {
                min_confidence: s.to_string(),
                ..crate::app::config::AnomalyConfig::default()
            };
            let gates = PolicyGates::from_inputs(PolicyGateInputs {
                token: &cfg,
                cost: &cost,
                context: &context,
                quota: &quota,
                security: &security,
                cache: &cache,
                reset: &reset,
                profile_switch: &profile_switch,
                anomaly: &anomaly,
                provider: Provider::Claude,
                confidence: IdentityConfidence::High,
            });
            assert_eq!(gates.anomaly_min_confidence, expected, "mapping for {s}");
        }
    }

    #[test]
    fn from_inputs_propagates_v2_anomaly_thresholds() {
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, QuotaConfig, ResetConfig, SecurityConfig,
            TokenConfig,
        };
        use crate::domain::identity::Provider;
        let cfg = TokenConfig::default();
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let security = SecurityConfig::default();
        let cache = CacheConfig::default();
        let reset = ResetConfig::default();
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();
        let anomaly = crate::app::config::AnomalyConfig {
            cost_slope_usd_per_hour: 40.0,
            token_slope_input_per_poll: 60_000,
            memory_growth_mb: 2048.0,
            ..crate::app::config::AnomalyConfig::default()
        };

        let gates = PolicyGates::from_inputs(PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider: Provider::Claude,
            confidence: IdentityConfidence::High,
        });
        assert!((gates.anomaly_cost_slope_usd_per_hour - 40.0).abs() < f64::EPSILON);
        assert_eq!(gates.anomaly_token_slope_input_per_poll, 60_000);
        assert!((gates.anomaly_memory_growth_mb - 2048.0).abs() < f64::EPSILON);
    }

    #[test]
    fn promote_min_confidence_returns_configured_field_per_kind() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
        let gates = PolicyGates {
            anomaly_promote_identity_churn: AnomalyConfidence::Low,
            anomaly_promote_error_burst: AnomalyConfidence::Medium,
            anomaly_promote_cache_discontinuity: AnomalyConfidence::High,
            anomaly_promote_cross_pane_edit_cluster: AnomalyConfidence::Low,
            anomaly_promote_cost_slope: AnomalyConfidence::Medium,
            anomaly_promote_token_slope: AnomalyConfidence::High,
            anomaly_promote_memory_growth: AnomalyConfidence::Low,
            anomaly_promote_subagent_side_effect: AnomalyConfidence::Medium,
            ..PolicyGates::default()
        };
        let cases = [
            (AnomalyKind::IdentityChurn, AnomalyConfidence::Low),
            (AnomalyKind::ErrorBurst, AnomalyConfidence::Medium),
            (AnomalyKind::CacheDiscontinuity, AnomalyConfidence::High),
            (AnomalyKind::CrossPaneEditCluster, AnomalyConfidence::Low),
            (AnomalyKind::CostSlope, AnomalyConfidence::Medium),
            (AnomalyKind::TokenSlope, AnomalyConfidence::High),
            (AnomalyKind::MemoryGrowth, AnomalyConfidence::Low),
            (AnomalyKind::SubagentSideEffect, AnomalyConfidence::Medium),
        ];
        for (kind, expected) in cases {
            assert_eq!(
                gates.promote_min_confidence(kind),
                expected,
                "kind {kind:?}"
            );
        }
    }

    #[test]
    fn policy_gates_default_promote_thresholds_match_spec() {
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
        let gates = PolicyGates::default();
        let cases = [
            (AnomalyKind::IdentityChurn, AnomalyConfidence::High),
            (AnomalyKind::ErrorBurst, AnomalyConfidence::High),
            (AnomalyKind::CacheDiscontinuity, AnomalyConfidence::High),
            (AnomalyKind::CrossPaneEditCluster, AnomalyConfidence::High),
            (AnomalyKind::CostSlope, AnomalyConfidence::High),
            (AnomalyKind::TokenSlope, AnomalyConfidence::High),
            (AnomalyKind::MemoryGrowth, AnomalyConfidence::High),
            (AnomalyKind::SubagentSideEffect, AnomalyConfidence::Medium),
        ];
        for (kind, expected) in cases {
            assert_eq!(
                gates.promote_min_confidence(kind),
                expected,
                "default mismatch for {kind:?}"
            );
        }
    }

    #[test]
    fn from_inputs_parses_promote_config() {
        use crate::app::config::{
            CacheConfig, ContextConfig, CostConfig, QuotaConfig, ResetConfig, SecurityConfig,
            TokenConfig,
        };
        use crate::domain::anomaly::AnomalyConfidence;
        use crate::domain::identity::Provider;
        let cfg = TokenConfig::default();
        let cost = CostConfig::default();
        let context = ContextConfig::default();
        let quota = QuotaConfig::default();
        let security = SecurityConfig::default();
        let cache = CacheConfig::default();
        let reset = ResetConfig::default();
        let profile_switch = crate::app::config::ProfileSwitchConfig::default();
        let anomaly = crate::app::config::AnomalyConfig {
            promote: crate::app::config::AnomalyPromoteConfig {
                cost_slope: "low".to_string(),
                subagent_side_effect: "high".to_string(),
                ..crate::app::config::AnomalyPromoteConfig::default()
            },
            ..crate::app::config::AnomalyConfig::default()
        };
        let gates = PolicyGates::from_inputs(PolicyGateInputs {
            token: &cfg,
            cost: &cost,
            context: &context,
            quota: &quota,
            security: &security,
            cache: &cache,
            reset: &reset,
            profile_switch: &profile_switch,
            anomaly: &anomaly,
            provider: Provider::Claude,
            confidence: IdentityConfidence::High,
        });
        assert_eq!(gates.anomaly_promote_cost_slope, AnomalyConfidence::Low);
        assert_eq!(
            gates.anomaly_promote_subagent_side_effect,
            AnomalyConfidence::High
        );
    }
}
