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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TmuxConfig {
    pub poll_interval_ms: u64,
    pub capture_lines: usize,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self {
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
    fn idle_config_loads_explicit_value_from_toml() {
        let toml = r#"
[idle]
stillness_polls = 6
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.idle.stillness_polls, 6);
    }
}
