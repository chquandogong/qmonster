use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;

/// Qmonster runtime config. Intentionally small for Phase 1. Runtime
/// actuation flags live here (NOT in mission.yaml — r2 envelope-vs-
/// runtime split).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QmonsterConfig {
    #[serde(default)]
    pub tmux: TmuxConfig,
    #[serde(default)]
    pub actions: ActionsConfig,
    #[serde(default)]
    pub refresh: RefreshConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub token: TokenConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub idle: IdleConfig,
    #[serde(default)]
    pub cost: CostConfig,
    #[serde(default)]
    pub context: ContextConfig,
    #[serde(default)]
    pub quota: QuotaConfig,
    #[serde(default)]
    pub security: SecurityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TmuxConfig {
    pub source: TmuxSourceMode,
    pub poll_interval_ms: u64,
    pub capture_lines: usize,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self {
            source: TmuxSourceMode::Auto,
            poll_interval_ms: 2000,
            capture_lines: 24,
        }
    }
}

impl TmuxConfig {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TmuxSourceMode {
    Auto,
    Polling,
    ControlMode,
}

impl TmuxSourceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Polling => "polling",
            Self::ControlMode => "control_mode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionsMode {
    ObserveOnly,
    RecommendOnly,
    SafeAuto,
}

impl ActionsMode {
    /// Lower = safer.
    fn safety_rank(self) -> u8 {
        match self {
            ActionsMode::ObserveOnly => 0,
            ActionsMode::RecommendOnly => 1,
            ActionsMode::SafeAuto => 2,
        }
    }

    fn from_str_maybe(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "observe_only" | "observe-only" | "observe" => Some(Self::ObserveOnly),
            "recommend_only" | "recommend-only" | "recommend" => Some(Self::RecommendOnly),
            "safe_auto" | "safe-auto" => Some(Self::SafeAuto),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionsConfig {
    pub mode: ActionsMode,
    pub allow_auto_notifications: bool,
    pub allow_auto_archive: bool,
    pub allow_auto_prompt_send: bool,
    pub allow_destructive_actions: bool,
}

impl Default for ActionsConfig {
    fn default() -> Self {
        Self {
            mode: ActionsMode::RecommendOnly,
            allow_auto_notifications: true,
            allow_auto_archive: true,
            allow_auto_prompt_send: false,
            allow_destructive_actions: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshPolicy {
    ManualOnly,
    Automatic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RefreshConfig {
    pub policy: RefreshPolicy,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            policy: RefreshPolicy::ManualOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSensitivity {
    Minimal,
    Balanced,
    Forensic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub sensitivity: LogSensitivity,
    pub retention_days: u64,
    pub big_output_chars: usize,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            sensitivity: LogSensitivity::Balanced,
            retention_days: 14,
            big_output_chars: 2200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TokenConfig {
    pub quota_tight: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StorageConfig {
    /// `~/.qmonster/` by default; tests override via env `QMONSTER_ROOT`.
    pub root: Option<String>,
}

/// Operator-controlled security posture surfacing. Runtime facts remain
/// visible as badges regardless of this setting; enabling this flag
/// promotes permissive modes into passive Concern recommendations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SecurityConfig {
    pub posture_advisories: bool,
}

/// Tuning knobs for the idle-stillness detector. Operators can set
/// these in the TOML passed with `--config PATH`; out-of-range values
/// are clamped inside `PaneTailHistory::new`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IdleConfig {
    /// Number of consecutive identical tail snapshots required before a pane
    /// is declared still. Clamped to [2, 12] by `PaneTailHistory::new`.
    pub stillness_polls: usize,
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self { stillness_polls: 4 }
    }
}

/// v1.15.16: per-provider cost-pressure thresholds. Operators with
/// different pricing tiers (e.g. Anthropic Sonnet/Opus vs OpenAI
/// gpt-5.4 vs Gemini Pro) can override the default `cost_pressure_*`
/// advisory bands without rebuilding.
///
/// Threshold semantics:
///
/// - `warning_usd`: cost_pressure_warning fires when
///   `cost_usd >= warning_usd && cost_usd < critical_usd`.
/// - `critical_usd`: cost_pressure_critical fires when
///   `cost_usd >= critical_usd`.
///
/// Defaults track empirical pricing-tier expectations as of 2026-04-27
/// (operator can tune freely):
///
/// - **Default** (Codex / unspecified providers): `$5 / $20` —
///   matches the v1.15.14 hardcoded baseline. Codex `gpt-5.4`
///   pricing is ~`$1/M` input + `$10/M` output; a 10M input + 2M
///   output session lands around `$30`, well into critical.
/// - **Claude** override: `$10 / $30` — Claude Sonnet 4.x / Opus 4.x
///   pricing is meaningfully higher per 1M tokens; the same workload
///   lands at roughly 2x the Codex cost, so the bands shift up.
/// - **Codex** override: unset (uses the default `$5 / $20`). Listed
///   explicitly in the config struct so the operator sees the slot.
/// - **Gemini** override: `$3 / $10` — Gemini Pro pricing tends
///   lower; the operator wants the warning to fire earlier in
///   absolute USD terms.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct CostConfig {
    pub warning_usd: f64,
    pub critical_usd: f64,
    /// Optional per-provider override for Claude panes. When `Some`,
    /// replaces the default thresholds for Provider::Claude only.
    pub claude: Option<CostProviderConfig>,
    /// Optional per-provider override for Codex panes. The shipping
    /// default is `None` (Codex uses the top-level defaults), but the
    /// slot is here so operators can pin Codex-specific values.
    pub codex: Option<CostProviderConfig>,
    /// Optional per-provider override for Gemini panes. When `Some`,
    /// replaces the default thresholds for Provider::Gemini only.
    pub gemini: Option<CostProviderConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CostProviderConfig {
    pub warning_usd: f64,
    pub critical_usd: f64,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            warning_usd: 5.0,
            critical_usd: 20.0,
            claude: Some(CostProviderConfig {
                warning_usd: 10.0,
                critical_usd: 30.0,
            }),
            codex: None,
            gemini: Some(CostProviderConfig {
                warning_usd: 3.0,
                critical_usd: 10.0,
            }),
        }
    }
}

impl CostConfig {
    pub fn warning_for(&self, provider: crate::domain::identity::Provider) -> f64 {
        self.override_for(provider)
            .map(|o| o.warning_usd)
            .unwrap_or(self.warning_usd)
    }

    pub fn critical_for(&self, provider: crate::domain::identity::Provider) -> f64 {
        self.override_for(provider)
            .map(|o| o.critical_usd)
            .unwrap_or(self.critical_usd)
    }

    fn override_for(
        &self,
        provider: crate::domain::identity::Provider,
    ) -> Option<&CostProviderConfig> {
        use crate::domain::identity::Provider;
        match provider {
            Provider::Claude => self.claude.as_ref(),
            Provider::Codex => self.codex.as_ref(),
            Provider::Gemini => self.gemini.as_ref(),
            Provider::Qmonster | Provider::Unknown => None,
        }
    }
}

/// v1.15.17: per-provider context_pressure thresholds. Mirrors the
/// CostConfig shape but for the fraction-of-context-window metric
/// (`signals.context_pressure`, 0..=1.0). The operator workflow at
/// 75% / 85% is roughly uniform across providers — checkpoint then
/// `/compact` — but operators with different `/compact` tolerance
/// (e.g. ones who want to act earlier on Gemini's 1M window vs
/// Claude's 200K) may want per-provider bands.
///
/// The `_pct` suffix on field names disambiguates the threshold's
/// unit: it's a fraction in 0..=1.0, not a 0..=100 percentage.
/// Examples: `warning_pct = 0.75` means "fire warning when
/// context_pressure crosses 75%". An accidental `warning_pct = 75`
/// would be silently impossible to satisfy (>1.0 always false), so
/// the example TOML calls this out explicitly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    pub warning_pct: f32,
    pub critical_pct: f32,
    pub claude: Option<PressureProviderConfig>,
    pub codex: Option<PressureProviderConfig>,
    pub gemini: Option<PressureProviderConfig>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            warning_pct: 0.75,
            critical_pct: 0.85,
            claude: None,
            codex: None,
            gemini: None,
        }
    }
}

impl ContextConfig {
    pub fn warning_for(&self, provider: crate::domain::identity::Provider) -> f32 {
        self.override_for(provider)
            .map(|o| o.warning_pct)
            .unwrap_or(self.warning_pct)
    }

    pub fn critical_for(&self, provider: crate::domain::identity::Provider) -> f32 {
        self.override_for(provider)
            .map(|o| o.critical_pct)
            .unwrap_or(self.critical_pct)
    }

    fn override_for(
        &self,
        provider: crate::domain::identity::Provider,
    ) -> Option<&PressureProviderConfig> {
        use crate::domain::identity::Provider;
        match provider {
            Provider::Claude => self.claude.as_ref(),
            Provider::Codex => self.codex.as_ref(),
            Provider::Gemini => self.gemini.as_ref(),
            Provider::Qmonster | Provider::Unknown => None,
        }
    }
}

/// v1.15.17: per-provider quota_pressure thresholds. Same shape as
/// `ContextConfig`. Today only Gemini populates `quota_pressure` (via
/// the v0.39 status table `quota` column); the per-provider override
/// slots are present so a future provider that exposes a quota metric
/// inherits the same configurability without code change.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct QuotaConfig {
    pub warning_pct: f32,
    pub critical_pct: f32,
    pub claude: Option<PressureProviderConfig>,
    pub codex: Option<PressureProviderConfig>,
    pub gemini: Option<PressureProviderConfig>,
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            warning_pct: 0.75,
            critical_pct: 0.85,
            claude: None,
            codex: None,
            gemini: None,
        }
    }
}

impl QuotaConfig {
    pub fn warning_for(&self, provider: crate::domain::identity::Provider) -> f32 {
        self.override_for(provider)
            .map(|o| o.warning_pct)
            .unwrap_or(self.warning_pct)
    }

    pub fn critical_for(&self, provider: crate::domain::identity::Provider) -> f32 {
        self.override_for(provider)
            .map(|o| o.critical_pct)
            .unwrap_or(self.critical_pct)
    }

    fn override_for(
        &self,
        provider: crate::domain::identity::Provider,
    ) -> Option<&PressureProviderConfig> {
        use crate::domain::identity::Provider;
        match provider {
            Provider::Claude => self.claude.as_ref(),
            Provider::Codex => self.codex.as_ref(),
            Provider::Gemini => self.gemini.as_ref(),
            Provider::Qmonster | Provider::Unknown => None,
        }
    }
}

/// Shared shape for per-provider context/quota threshold overrides.
/// `CostConfig` keeps its own provider-override struct because USD
/// thresholds use a different unit (f64 USD vs f32 fraction).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PressureProviderConfig {
    pub warning_pct: f32,
    pub critical_pct: f32,
}

impl QmonsterConfig {
    pub fn defaults() -> Self {
        Self {
            tmux: TmuxConfig::default(),
            actions: ActionsConfig::default(),
            refresh: RefreshConfig::default(),
            logging: LoggingConfig::default(),
            token: TokenConfig::default(),
            storage: StorageConfig::default(),
            idle: IdleConfig::default(),
            cost: CostConfig::default(),
            context: ContextConfig::default(),
            quota: QuotaConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse config: {0}")]
    Parse(#[from] toml::de::Error),
}

pub fn load_from_path(path: &Path) -> Result<QmonsterConfig, ConfigError> {
    let raw = std::fs::read_to_string(path)?;
    let cfg: QmonsterConfig = toml::from_str(&raw)?;
    Ok(cfg)
}

/// Result of attempting to apply an env/CLI override to a safety flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyOverride {
    Accepted,
    Rejected { reason: String },
    UnknownKey,
}

/// Safety-precedence rule (r2 §4): env/CLI overrides may only move
/// `actions.mode` / `allow_auto_prompt_send` / `allow_destructive_actions`
/// / `refresh.policy` toward safer. `logging.sensitivity` is free.
pub fn apply_safety_override(cfg: &mut QmonsterConfig, key: &str, value: &str) -> SafetyOverride {
    match key {
        "actions.mode" => {
            let Some(new) = ActionsMode::from_str_maybe(value) else {
                return SafetyOverride::Rejected {
                    reason: format!("unknown actions.mode: {value}"),
                };
            };
            if new.safety_rank() <= cfg.actions.mode.safety_rank() {
                cfg.actions.mode = new;
                SafetyOverride::Accepted
            } else {
                SafetyOverride::Rejected {
                    reason: "actions.mode can only move toward safer".into(),
                }
            }
        }
        "actions.allow_auto_prompt_send" => {
            let requested = parse_bool(value);
            match requested {
                Some(false) => {
                    cfg.actions.allow_auto_prompt_send = false;
                    SafetyOverride::Accepted
                }
                Some(true) => SafetyOverride::Rejected {
                    reason: "allow_auto_prompt_send cannot be raised to true via env/CLI".into(),
                },
                None => SafetyOverride::Rejected {
                    reason: format!("not a bool: {value}"),
                },
            }
        }
        "actions.allow_destructive_actions" => {
            let requested = parse_bool(value);
            match requested {
                Some(false) => {
                    cfg.actions.allow_destructive_actions = false;
                    SafetyOverride::Accepted
                }
                Some(true) => SafetyOverride::Rejected {
                    reason: "allow_destructive_actions cannot be raised to true via env/CLI".into(),
                },
                None => SafetyOverride::Rejected {
                    reason: format!("not a bool: {value}"),
                },
            }
        }
        "refresh.policy" => match value.trim().to_lowercase().as_str() {
            "manual_only" | "manual" => {
                cfg.refresh.policy = RefreshPolicy::ManualOnly;
                SafetyOverride::Accepted
            }
            _ => SafetyOverride::Rejected {
                reason: "refresh.policy can only move to manual_only via env/CLI".into(),
            },
        },
        "logging.sensitivity" => match value.trim().to_lowercase().as_str() {
            "minimal" => {
                cfg.logging.sensitivity = LogSensitivity::Minimal;
                SafetyOverride::Accepted
            }
            "balanced" => {
                cfg.logging.sensitivity = LogSensitivity::Balanced;
                SafetyOverride::Accepted
            }
            "forensic" => {
                cfg.logging.sensitivity = LogSensitivity::Forensic;
                SafetyOverride::Accepted
            }
            other => SafetyOverride::Rejected {
                reason: format!("unknown sensitivity: {other}"),
            },
        },
        _ => SafetyOverride::UnknownKey,
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> QmonsterConfig {
        QmonsterConfig::defaults()
    }

    #[test]
    fn defaults_are_safe() {
        let c = base();
        assert_eq!(c.tmux.source, TmuxSourceMode::Auto);
        assert_eq!(c.actions.mode, ActionsMode::RecommendOnly);
        assert!(!c.actions.allow_auto_prompt_send);
        assert!(!c.actions.allow_destructive_actions);
        assert_eq!(c.refresh.policy, RefreshPolicy::ManualOnly);
        assert_eq!(c.logging.sensitivity, LogSensitivity::Balanced);
    }

    #[test]
    fn env_can_move_actions_mode_toward_safer() {
        let mut c = base();
        let res = apply_safety_override(&mut c, "actions.mode", "observe_only");
        assert!(matches!(res, SafetyOverride::Accepted));
        assert_eq!(c.actions.mode, ActionsMode::ObserveOnly);
    }

    #[test]
    fn env_cannot_move_actions_mode_toward_permissive() {
        let mut c = base();
        let res = apply_safety_override(&mut c, "actions.mode", "safe_auto");
        assert!(matches!(res, SafetyOverride::Rejected { .. }));
        assert_eq!(c.actions.mode, ActionsMode::RecommendOnly);
    }

    #[test]
    fn cannot_flip_allow_auto_prompt_send_to_true() {
        let mut c = base();
        let res = apply_safety_override(&mut c, "actions.allow_auto_prompt_send", "true");
        assert!(matches!(res, SafetyOverride::Rejected { .. }));
        assert!(!c.actions.allow_auto_prompt_send);
    }

    #[test]
    fn can_set_allow_auto_prompt_send_to_false_even_if_already_false() {
        let mut c = base();
        let res = apply_safety_override(&mut c, "actions.allow_auto_prompt_send", "false");
        assert!(matches!(res, SafetyOverride::Accepted));
        assert!(!c.actions.allow_auto_prompt_send);
    }

    #[test]
    fn refresh_policy_cannot_be_relaxed() {
        let mut c = base();
        let res = apply_safety_override(&mut c, "refresh.policy", "automatic");
        assert!(matches!(res, SafetyOverride::Rejected { .. }));
        assert_eq!(c.refresh.policy, RefreshPolicy::ManualOnly);
    }

    #[test]
    fn logging_sensitivity_moves_freely_both_ways() {
        let mut c = base();
        let res = apply_safety_override(&mut c, "logging.sensitivity", "forensic");
        assert!(matches!(res, SafetyOverride::Accepted));
        assert_eq!(c.logging.sensitivity, LogSensitivity::Forensic);
        let res = apply_safety_override(&mut c, "logging.sensitivity", "minimal");
        assert!(matches!(res, SafetyOverride::Accepted));
        assert_eq!(c.logging.sensitivity, LogSensitivity::Minimal);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let mut c = base();
        let res = apply_safety_override(&mut c, "unknown.key", "x");
        assert!(matches!(res, SafetyOverride::UnknownKey));
    }

    #[test]
    fn idle_config_default_stillness_polls_is_4() {
        let cfg = QmonsterConfig::defaults();
        assert_eq!(cfg.idle.stillness_polls, 4);
    }

    #[test]
    fn tmux_source_mode_loads_control_mode_from_toml() {
        let toml = r#"
[tmux]
source = "control_mode"
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.tmux.source, TmuxSourceMode::ControlMode);
    }

    #[test]
    fn tmux_source_mode_loads_auto_from_toml() {
        let toml = r#"
[tmux]
source = "auto"
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.tmux.source, TmuxSourceMode::Auto);
    }

    #[test]
    fn tmux_source_mode_as_str_matches_config_spelling() {
        assert_eq!(TmuxSourceMode::Auto.as_str(), "auto");
        assert_eq!(TmuxSourceMode::Polling.as_str(), "polling");
        assert_eq!(TmuxSourceMode::ControlMode.as_str(), "control_mode");
    }

    #[test]
    fn security_posture_advisories_default_to_off() {
        let cfg = QmonsterConfig::defaults();
        assert!(!cfg.security.posture_advisories);
    }

    #[test]
    fn security_posture_advisories_load_from_toml() {
        let toml = r#"
[security]
posture_advisories = true
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();
        assert!(cfg.security.posture_advisories);
    }

    #[test]
    fn idle_config_loads_explicit_value_from_toml() {
        let toml = r#"
[idle]
stillness_polls = 6
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.idle.stillness_polls, 6);
    }
}
