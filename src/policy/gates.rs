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
    pub cache_hot_ratio: f64,
    pub cache_cold_ratio: f64,
    pub cache_hot_low_ctx: f32,
    pub cache_cold_high_ctx: f32,
    pub cache_drift_drop: f64,
    pub cache_drift_min_samples: usize,
    pub reset_wait_pressure: f32,
    pub reset_wait_eta_secs: u64,
    pub reset_snapshot_pressure: f32,
    pub reset_snapshot_eta_secs: u64,
    pub reset_auto_snapshot: bool,
    pub anomaly_enabled: bool,
    pub anomaly_window_polls: usize,
    pub anomaly_min_confidence: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_identity_churn_min_flips: usize,
    pub anomaly_error_burst_threshold: f32,
    pub anomaly_cache_discontinuity_drop: f32,
    pub anomaly_cross_pane_cluster_min_findings: usize,
    pub anomaly_cost_slope_usd_per_hour: f64,
    pub anomaly_promote_identity_churn: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_error_burst: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_cache_discontinuity: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_cross_pane_edit_cluster: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_cost_slope: crate::domain::anomaly::AnomalyConfidence,
    pub cross_window_findings: bool,
    pub identity_drift_findings: bool,
    pub cross_pane_file_findings: bool,
    pub profile_switch_enabled: bool,
    pub profile_switch_window: usize,
    pub profile_switch_error_rate: f32,
}

impl Default for PolicyGates {
    fn default() -> Self {
        Self {
            quota_tight: false,
            security_posture_advisories: false,
            identity_confidence: IdentityConfidence::Unknown,
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
            anomaly_promote_identity_churn: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_error_burst: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_cache_discontinuity: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_cross_pane_edit_cluster:
                crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_cost_slope: crate::domain::anomaly::AnomalyConfidence::High,
            cross_window_findings: false,
            identity_drift_findings: false,
            cross_pane_file_findings: false,
            profile_switch_enabled: false,
            profile_switch_window: 10,
            profile_switch_error_rate: 0.5,
        }
    }
}

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
        _ => crate::domain::anomaly::AnomalyConfidence::Medium,
    }
}

impl PolicyGates {
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
            anomaly_promote_identity_churn: parse_min_confidence(&anomaly.promote.identity_churn),
            anomaly_promote_error_burst: parse_min_confidence(&anomaly.promote.error_burst),
            anomaly_promote_cache_discontinuity: parse_min_confidence(
                &anomaly.promote.cache_discontinuity,
            ),
            anomaly_promote_cross_pane_edit_cluster: parse_min_confidence(
                &anomaly.promote.cross_pane_edit_cluster,
            ),
            anomaly_promote_cost_slope: parse_min_confidence(&anomaly.promote.cost_slope),
            cross_window_findings: security.cross_window_findings,
            identity_drift_findings: security.identity_drift_findings,
            cross_pane_file_findings: security.cross_pane_file_findings,
            profile_switch_enabled: profile_switch.enabled,
            profile_switch_window: profile_switch.window_polls,
            profile_switch_error_rate: profile_switch.error_rate_threshold,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PromptSendGate {
    Blocked,
    AutoSendOff,
    Execute,
}

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
            ..PolicyGates::default()
        };
        assert!(gates.quota_tight);
    }

    #[test]
    fn quota_tight_gate_defaults_to_false() {
        let gates = PolicyGates::default();
        assert!(!gates.quota_tight);
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
    }
}
