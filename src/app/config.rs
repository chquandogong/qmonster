use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub reset: ResetConfig,
    #[serde(default)]
    pub anomaly: AnomalyConfig,
    #[serde(default)]
    pub provider_setup: ProviderSetupConfig,
    #[serde(default)]
    pub profile_switch: ProfileSwitchConfig,
    #[serde(default)]
    pub ux: UxConfig,
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

/// Phase F F-7-config (v1.28.0): operator-tunable thresholds for the
/// cache-aware advisory rules in `src/policy/rules/cache.rs`. Defaults
/// match the v1.26.0 (F-7) and v1.27.0 (F-7b) hardcoded constants
/// exactly, so an operator with no `[cache]` section gets the original
/// behavior. Each field is documented with the rule it influences.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    /// `recommend_cache_hot_compact_warning` fires when
    /// `cache_hit_ratio > hot_ratio_threshold`. Default 0.6 (60%).
    pub hot_ratio_threshold: f64,
    /// `recommend_compact_when_cache_cold` fires when
    /// `cache_hit_ratio < cold_ratio_threshold`. Default 0.3 (30%).
    pub cold_ratio_threshold: f64,
    /// `recommend_cache_hot_compact_warning` requires
    /// `context_pressure < hot_low_ctx_threshold` (still has headroom
    /// — operator could keep working). Default 0.7 (70%).
    pub hot_low_ctx_threshold: f32,
    /// `recommend_compact_when_cache_cold` requires
    /// `context_pressure > cold_high_ctx_threshold` (filling — should
    /// compact). Default 0.6 (60%).
    pub cold_high_ctx_threshold: f32,
    /// `recommend_cache_drift_compact` fires when the cache hit ratio
    /// has dropped by ≥ `drift_drop_threshold` between the oldest
    /// sample in the recent window and the current SignalSet. Default
    /// 0.30 (30 percentage points).
    pub drift_drop_threshold: f64,
    /// `recommend_cache_drift_compact` requires
    /// `recent_token_samples.len() ≥ drift_min_samples`. Default 4.
    pub drift_min_samples: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            hot_ratio_threshold: 0.6,
            cold_ratio_threshold: 0.3,
            hot_low_ctx_threshold: 0.7,
            cold_high_ctx_threshold: 0.6,
            drift_drop_threshold: 0.30,
            drift_min_samples: 4,
        }
    }
}

/// Phase F F-7d (v1.35.0): operator-tunable thresholds for the
/// reset-aware advisories in `src/policy/rules/reset.rs`. Defaults
/// match the v1.34.0 (F-7c) hardcoded constants exactly so an
/// operator with no `[reset]` section gets the original behavior.
///
/// `wait_pressure_threshold` / `wait_eta_secs` gate
/// `recommend_wait_for_reset` (Concern advisory: pause until reset
/// when quota high AND reset close).
///
/// `snapshot_pressure_threshold` / `snapshot_eta_secs` gate
/// `recommend_snapshot_before_reset` (Good advisory: write a
/// snapshot when reset is imminent so handoff state survives).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct ResetConfig {
    /// `recommend_wait_for_reset` requires
    /// `quota_*_pressure ≥ wait_pressure_threshold`. Default 0.85
    /// (85% of the window used).
    pub wait_pressure_threshold: f32,
    /// `recommend_wait_for_reset` requires the resets_at countdown
    /// to be ≤ `wait_eta_secs`. Default 1800 (30 minutes).
    pub wait_eta_secs: u64,
    /// `recommend_snapshot_before_reset` requires
    /// `quota_*_pressure ≥ snapshot_pressure_threshold` so the
    /// advisory only fires when there's meaningful session state
    /// worth preserving. Default 0.50 (50%).
    pub snapshot_pressure_threshold: f32,
    /// `recommend_snapshot_before_reset` requires the resets_at
    /// countdown to be ≤ `snapshot_eta_secs`. Default 300 (5 mins).
    pub snapshot_eta_secs: u64,
    /// Phase H (v1.42.0): when `true`, the actuation hook in
    /// `src/app/auto_snapshot.rs` automatically writes one snapshot per
    /// `(pane_id, quota_kind, window_id)` whenever
    /// `recommend_snapshot_before_reset` fires. Default `false` —
    /// existing v1.41.0 operators see no behavior change. Reuses the
    /// F-7d `snapshot_pressure_threshold` / `snapshot_eta_secs`
    /// thresholds; this flag is the actuation gate, not a new
    /// detection threshold.
    pub auto_snapshot: bool,
}

impl Default for ResetConfig {
    fn default() -> Self {
        Self {
            wait_pressure_threshold: 0.85,
            wait_eta_secs: 30 * 60,
            snapshot_pressure_threshold: 0.50,
            snapshot_eta_secs: 5 * 60,
            auto_snapshot: false,
        }
    }
}

/// Phase 7 v1 (v1.43.0): operator-tunable thresholds for the
/// anomaly observation layer in `src/policy/rules/anomaly.rs`.
/// Defaults are deliberately conservative — the entire layer is
/// off by default, the rolling window is 20 polls, and each
/// detector's threshold is set so a brief blip does not fire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnomalyConfig {
    /// Master switch. When false, `eval_anomalies` returns an empty
    /// Vec on every tick; UI rendering is a no-op.
    pub enabled: bool,
    /// Rolling-window length used by every detector. 20 polls at the
    /// default 5-second tick is a 100-second window — long enough to
    /// catch a real burst, short enough to clear quickly.
    pub window_polls: usize,
    /// Detector output below this confidence is filtered before
    /// reaching `PaneReport`. Values: `"low"`, `"medium"`, `"high"`.
    pub min_confidence: String,
    /// IdentityChurn fires when (provider, current_path) flips
    /// `>= identity_churn_min_flips` times inside `window_polls`.
    pub identity_churn_min_flips: usize,
    /// ErrorBurst fires when error rate over `window_polls` is
    /// `>= error_burst_threshold`.
    pub error_burst_threshold: f32,
    /// CacheDiscontinuity fires when `cache_hit_ratio` drops by
    /// at least this much across the window (or when F-7b fires
    /// twice in the window — alternate trigger).
    pub cache_discontinuity_drop: f32,
    /// CrossPaneEditCluster fires when at least this many
    /// `ConcurrentFileEdit` findings target the same path inside
    /// the window.
    pub cross_pane_cluster_min_findings: usize,
    /// Phase 7 v2 (v1.45.0): CostSlope detector threshold. Fires when
    /// `(cost_now - cost_oldest) / window_secs * 3600 >= cost_slope_usd_per_hour`.
    /// Default 20.0 (matches prelim spec § Configuration).
    pub cost_slope_usd_per_hour: f64,
    /// Phase 7 v2 (v1.45.0): TokenSlope detector threshold. Fires when
    /// `(input_now - input_oldest) / window_polls >= token_slope_input_per_poll`.
    /// Default 20_000 (matches prelim spec § Configuration).
    pub token_slope_input_per_poll: u64,
    /// Phase 7 v2 (v1.45.0): MemoryGrowth detector threshold. Fires when
    /// `process_memory_mb_now - process_memory_mb_oldest >= memory_growth_mb`.
    /// Default 1024.0 (matches prelim spec § Configuration).
    pub memory_growth_mb: f64,
    /// Phase 7 v3 (v1.46.0): per-kind promotion-gate thresholds.
    pub promote: AnomalyPromoteConfig,
}

/// Phase 7 v3 (v1.46.0): per-`AnomalyKind` minimum-confidence
/// threshold for promoting an `AnomalySignal` to a `Recommendation`.
/// Distinct from the global `AnomalyConfig.min_confidence` (visibility
/// filter) — `[anomaly.promote]` gates promotion only.
///
/// Defaults preserve v1.45.0 behavior for the 7 kinds that today
/// promote only at High confidence; `subagent_side_effect = "medium"`
/// activates the previously-unreachable v1.45.0 promote-matrix branch
/// for `SubagentSideEffect`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnomalyPromoteConfig {
    pub identity_churn: String,
    pub error_burst: String,
    pub cache_discontinuity: String,
    pub cross_pane_edit_cluster: String,
    pub cost_slope: String,
    pub token_slope: String,
    pub memory_growth: String,
    pub subagent_side_effect: String,
}

impl Default for AnomalyPromoteConfig {
    fn default() -> Self {
        Self {
            identity_churn: "high".to_string(),
            error_burst: "high".to_string(),
            cache_discontinuity: "high".to_string(),
            cross_pane_edit_cluster: "high".to_string(),
            cost_slope: "high".to_string(),
            token_slope: "high".to_string(),
            memory_growth: "high".to_string(),
            subagent_side_effect: "medium".to_string(),
        }
    }
}

impl Default for AnomalyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            window_polls: 20,
            min_confidence: "medium".to_string(),
            identity_churn_min_flips: 3,
            error_burst_threshold: 0.5,
            cache_discontinuity_drop: 0.30,
            cross_pane_cluster_min_findings: 3,
            cost_slope_usd_per_hour: 20.0,
            token_slope_input_per_poll: 20_000,
            memory_growth_mb: 1024.0,
            promote: AnomalyPromoteConfig::default(),
        }
    }
}

/// Phase G G-2 (v1.30.0): operator-tunable provider integration
/// switches. Persisted in `qmonster.toml`'s `[provider_setup]`
/// section and edited from Settings > Integrations; Provider Setup
/// only mirrors these values while showing copyable guidance.
///
/// `claude_sidefile` defaults to `true` — the recommended Claude
/// statusline writes a per-session JSON sidefile to
/// `~/.local/share/ai-cli-status/claude/<session_id>.json`, which
/// downstream Claude tooling (including F-5's cache reader) consumes.
/// Operators on minimal setups who do not want the sidefile written
/// can opt out via `claude_sidefile = false`.
///
/// `codex_app_server` defaults to `false` — running `codex
/// app-server` is an advanced background daemon and not all
/// operators want it; the toggle stays opt-in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderSetupConfig {
    /// Claude statusline sidefile JSON export. `true` so the
    /// recommended workflow is on by default.
    pub claude_sidefile: bool,
    /// Qmonster-managed Codex App Server polling. `false` because the
    /// App Server is an advanced opt-in.
    pub codex_app_server: bool,
}

impl Default for ProviderSetupConfig {
    fn default() -> Self {
        Self {
            claude_sidefile: true,
            codex_app_server: false,
        }
    }
}

/// Phase F F-9 (dynamic profile switching): operator-tunable knobs
/// for the `profile_switch` policy rule. The rule monitors per-pane
/// `error_hint` observations over a sliding window and recommends
/// switching the active provider profile when the error rate exceeds
/// `error_rate_threshold`. Defaults are conservative: the rule is
/// off by default, the window is 10 polls, and the trigger is 50% —
/// so a healthy run that occasionally hits a transient error never
/// trips the recommendation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileSwitchConfig {
    /// Master switch. The rule only fires when this is `true`. Stays
    /// `false` because the rule is advisory; an operator who has not
    /// opted into recommend-mode profile switches should not see the
    /// nudge.
    pub enabled: bool,
    /// Length of the sliding window over which to compute the error
    /// rate. The rule waits until the window is full so a single
    /// transient error never trips the recommendation.
    pub window_polls: usize,
    /// Error-rate threshold (0..1). The rule fires when the observed
    /// rate over the most recent `window_polls` is `>=` this value.
    /// Defaults to `0.5` so 5+ of the last 10 polls flagged
    /// `error_hint`.
    pub error_rate_threshold: f32,
}

impl Default for ProfileSwitchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            window_polls: 10,
            error_rate_threshold: 0.5,
        }
    }
}

/// v1.38 UX bundle (spec §6.2): operator-tunable cadence for the
/// action-explainer modal that previews what `p` / `d` / `y` will do
/// before executing it. `Always` keeps the existing behaviour (confirm
/// every time); `FirstTime` shows the modal once per action kind in a
/// session, then assumes the operator has read it; `Never` skips the
/// modal entirely for operators who already know what each shortcut
/// does.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UxConfig {
    pub confirm_actions: ConfirmActions,
}

impl Default for UxConfig {
    fn default() -> Self {
        Self {
            confirm_actions: ConfirmActions::Always,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmActions {
    Always,
    FirstTime,
    Never,
}

/// Operator-controlled security posture surfacing. Runtime facts remain
/// visible as badges regardless of this setting; enabling this flag
/// promotes permissive modes into passive Concern recommendations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SecurityConfig {
    pub posture_advisories: bool,
    /// Phase D D1 (v1.17.0): when `true`, the cross-pane rule emits a
    /// `CrossWindowConcurrentWork` finding when two or more busy
    /// Main/Review panes share `current_path` + `git_branch` but live
    /// in different tmux windows. Stays `false` by default — an
    /// operator running the same repo in a scratch window plus a
    /// main-implementation window is a normal pattern, not a default
    /// alarm.
    pub cross_window_findings: bool,
    /// Phase D D2 (v1.18.0): when `true`, identity-drift rules emit a
    /// passive `Concern` recommendation when a pane's resolved
    /// `Provider` or `current_path` changes between polls (e.g. the
    /// operator quits Claude in a pane and starts Codex in its
    /// place, or `cd`s into a different worktree). Stays `false` by
    /// default — the identity resolver intentionally accepts whatever
    /// the operator runs, so a drift is not a problem unless the
    /// operator wants visibility.
    pub identity_drift_findings: bool,
    /// Phase F F-8 (multi-pane orchestration): when `true`, the
    /// cross-pane rule emits a `CrossPaneKind::ConcurrentFileEdit`
    /// finding when two or more busy Main/Review panes have recently
    /// touched the same absolute file path (parsed from
    /// `● Edit(...)` / `● Write(...)` style tool-call markers in the
    /// pane tail). Stays `false` by default because the active-file
    /// signal is `Heuristic`; opt in once the operator's workflow
    /// makes simultaneous file edits an actual coordination problem.
    pub cross_pane_file_findings: bool,
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
    /// Project/team USD budget for the SQLite cost ledger. Alerts fire
    /// once when tracked spend crosses 80% and 100% of this value.
    /// Set to 0.0 to disable budget alerts.
    pub budget_usd: f64,
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
            budget_usd: 200.0,
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
    pub fn budget_enabled(&self) -> bool {
        self.budget_usd.is_finite() && self.budget_usd > 0.0
    }

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

/// v1.15.17: per-provider quota_pressure thresholds. v1.16.51 splits
/// Claude/Codex quota thresholds into the provider's two visible
/// windows: rolling 5-hour quota and weekly quota. Gemini still exposes
/// a single quota column and uses `gemini` / top-level thresholds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct QuotaConfig {
    pub warning_pct: f32,
    pub critical_pct: f32,
    /// Legacy v1.15.17 field. Deserialized as a fallback for both
    /// Claude quota windows but not written by the settings overlay.
    #[serde(skip_serializing)]
    pub claude: Option<PressureProviderConfig>,
    pub claude_5h: Option<PressureProviderConfig>,
    pub claude_weekly: Option<PressureProviderConfig>,
    /// Legacy v1.15.17 field. Deserialized as a fallback for both
    /// Codex quota windows but not written by the settings overlay.
    #[serde(skip_serializing)]
    pub codex: Option<PressureProviderConfig>,
    pub codex_5h: Option<PressureProviderConfig>,
    pub codex_weekly: Option<PressureProviderConfig>,
    pub gemini: Option<PressureProviderConfig>,
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            warning_pct: 0.75,
            critical_pct: 0.85,
            claude: None,
            claude_5h: None,
            claude_weekly: None,
            codex: None,
            codex_5h: None,
            codex_weekly: None,
            gemini: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaWindow {
    Single,
    FiveHour,
    Weekly,
}

impl QuotaConfig {
    pub fn warning_for(&self, provider: crate::domain::identity::Provider) -> f32 {
        self.override_for_window(provider, QuotaWindow::Single)
            .map(|o| o.warning_pct)
            .unwrap_or(self.warning_pct)
    }

    pub fn critical_for(&self, provider: crate::domain::identity::Provider) -> f32 {
        self.override_for_window(provider, QuotaWindow::Single)
            .map(|o| o.critical_pct)
            .unwrap_or(self.critical_pct)
    }

    pub fn warning_for_window(
        &self,
        provider: crate::domain::identity::Provider,
        window: QuotaWindow,
    ) -> f32 {
        self.override_for_window(provider, window)
            .map(|o| o.warning_pct)
            .unwrap_or(self.warning_pct)
    }

    pub fn critical_for_window(
        &self,
        provider: crate::domain::identity::Provider,
        window: QuotaWindow,
    ) -> f32 {
        self.override_for_window(provider, window)
            .map(|o| o.critical_pct)
            .unwrap_or(self.critical_pct)
    }

    fn override_for_window(
        &self,
        provider: crate::domain::identity::Provider,
        window: QuotaWindow,
    ) -> Option<&PressureProviderConfig> {
        use crate::domain::identity::Provider;
        match provider {
            Provider::Claude => match window {
                QuotaWindow::FiveHour => self.claude_5h.as_ref().or(self.claude.as_ref()),
                QuotaWindow::Weekly => self.claude_weekly.as_ref().or(self.claude.as_ref()),
                QuotaWindow::Single => self.claude.as_ref(),
            },
            Provider::Codex => match window {
                QuotaWindow::FiveHour => self.codex_5h.as_ref().or(self.codex.as_ref()),
                QuotaWindow::Weekly => self.codex_weekly.as_ref().or(self.codex.as_ref()),
                QuotaWindow::Single => self.codex.as_ref(),
            },
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
            provider_setup: ProviderSetupConfig::default(),
            context: ContextConfig::default(),
            quota: QuotaConfig::default(),
            security: SecurityConfig::default(),
            cache: CacheConfig::default(),
            reset: ResetConfig::default(),
            anomaly: AnomalyConfig::default(),
            profile_switch: ProfileSwitchConfig::default(),
            ux: UxConfig::default(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("serialize default config: {0}")]
    Serialize(#[from] toml::ser::Error),
}

pub fn load_from_path(path: &Path) -> Result<QmonsterConfig, ConfigError> {
    load_layered_from_paths(path, None)
}

pub fn load_with_local_override(path: &Path) -> Result<QmonsterConfig, ConfigError> {
    let local_path = local_override_path_for(path);
    let local = if local_path.exists() {
        Some(local_path.as_path())
    } else {
        None
    };
    load_layered_from_paths(path, local)
}

pub fn local_override_path_for(path: &Path) -> PathBuf {
    path.with_file_name("qmonster.local.toml")
}

pub fn load_layered_from_paths(
    base_path: &Path,
    local_override_path: Option<&Path>,
) -> Result<QmonsterConfig, ConfigError> {
    let mut merged = toml::Value::try_from(QmonsterConfig::defaults())?;
    let base_raw = std::fs::read_to_string(base_path)?;
    let base: toml::Value = toml::from_str(&base_raw)?;
    merge_toml_values(&mut merged, base);
    if let Some(path) = local_override_path {
        let local_raw = std::fs::read_to_string(path)?;
        let local: toml::Value = toml::from_str(&local_raw)?;
        merge_toml_values(&mut merged, local);
    }
    Ok(merged.try_into()?)
}

fn merge_toml_values(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, value) in overlay_table {
                match base_table.get_mut(&key) {
                    Some(existing) => merge_toml_values(existing, value),
                    None => {
                        base_table.insert(key, value);
                    }
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value;
        }
    }
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

    #[test]
    fn provider_setup_config_defaults_to_sidefile_on_app_server_off() {
        let absent: QmonsterConfig = toml::from_str("").unwrap();
        assert!(
            absent.provider_setup.claude_sidefile,
            "claude_sidefile must default to true so the recommended workflow is on out of the box"
        );
        assert!(
            !absent.provider_setup.codex_app_server,
            "codex_app_server must default to false (advanced opt-in)"
        );
    }

    #[test]
    fn provider_setup_config_loads_overrides_from_toml() {
        let toml = r#"
[provider_setup]
claude_sidefile = false
codex_app_server = true
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();
        assert!(!cfg.provider_setup.claude_sidefile);
        assert!(cfg.provider_setup.codex_app_server);
    }

    #[test]
    fn provider_setup_config_partial_toml_keeps_other_default() {
        // serde(default) on the struct itself: missing fields fall
        // back to the per-field default. Operators editing only one
        // toggle must not lose the other.
        let toml = r#"
[provider_setup]
codex_app_server = true
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();
        assert!(
            cfg.provider_setup.claude_sidefile,
            "missing claude_sidefile must keep the default-true value"
        );
        assert!(cfg.provider_setup.codex_app_server);
    }

    #[test]
    fn cost_config_defaults_to_200_usd_budget() {
        let cfg: QmonsterConfig = toml::from_str("").unwrap();
        assert!((cfg.cost.budget_usd - 200.0).abs() < f64::EPSILON);
        assert!(cfg.cost.budget_enabled());
    }

    #[test]
    fn cost_config_zero_budget_disables_budget_alerts() {
        let cfg: QmonsterConfig = toml::from_str(
            r#"
[cost]
budget_usd = 0.0
"#,
        )
        .unwrap();
        assert!(!cfg.cost.budget_enabled());
    }

    #[test]
    fn layered_config_merges_local_override_after_base() {
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("qmonster.toml");
        let local = td.path().join("qmonster.local.toml");
        std::fs::write(
            &base,
            r#"
[tmux]
capture_lines = 24

[cost]
budget_usd = 200.0
warning_usd = 5.0
critical_usd = 20.0
"#,
        )
        .unwrap();
        std::fs::write(
            &local,
            r#"
[tmux]
capture_lines = 72

[cost.claude]
warning_usd = 12.5
"#,
        )
        .unwrap();

        let cfg = load_layered_from_paths(&base, Some(&local)).unwrap();

        assert_eq!(cfg.tmux.capture_lines, 72);
        assert!((cfg.cost.budget_usd - 200.0).abs() < f64::EPSILON);
        let claude = cfg.cost.claude.expect("claude override survives merge");
        assert!((claude.warning_usd - 12.5).abs() < f64::EPSILON);
        assert!((claude.critical_usd - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn load_with_local_override_uses_sibling_qmonster_local_toml_when_present() {
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("qmonster.toml");
        let local = td.path().join("qmonster.local.toml");
        std::fs::write(
            &base,
            r#"
[actions]
mode = "recommend_only"
"#,
        )
        .unwrap();
        std::fs::write(
            &local,
            r#"
[actions]
mode = "observe_only"
"#,
        )
        .unwrap();

        let cfg = load_with_local_override(&base).unwrap();

        assert_eq!(cfg.actions.mode, ActionsMode::ObserveOnly);
        assert_eq!(local_override_path_for(&base), local);
    }

    #[test]
    fn reset_config_defaults_match_v1_34_0_constants_exactly() {
        let absent: QmonsterConfig = toml::from_str("").unwrap();
        assert!((absent.reset.wait_pressure_threshold - 0.85).abs() < f32::EPSILON);
        assert_eq!(absent.reset.wait_eta_secs, 30 * 60);
        assert!((absent.reset.snapshot_pressure_threshold - 0.50).abs() < f32::EPSILON);
        assert_eq!(absent.reset.snapshot_eta_secs, 5 * 60);
    }

    #[test]
    fn reset_config_loads_toml_and_keeps_other_defaults() {
        let toml = r#"
[reset]
wait_pressure_threshold = 0.92
snapshot_eta_secs = 600
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();
        assert!((cfg.reset.wait_pressure_threshold - 0.92).abs() < f32::EPSILON);
        // Untouched fields fall back to per-field defaults.
        assert_eq!(cfg.reset.wait_eta_secs, 30 * 60);
        assert!((cfg.reset.snapshot_pressure_threshold - 0.50).abs() < f32::EPSILON);
        assert_eq!(cfg.reset.snapshot_eta_secs, 600);
    }

    #[test]
    fn cache_config_loads_toml_and_defaults_missing_values() {
        let absent: QmonsterConfig = toml::from_str("").unwrap();
        assert!((absent.cache.hot_ratio_threshold - 0.6).abs() < f64::EPSILON);
        assert!((absent.cache.cold_ratio_threshold - 0.3).abs() < f64::EPSILON);
        assert!((absent.cache.hot_low_ctx_threshold - 0.7).abs() < f32::EPSILON);
        assert!((absent.cache.cold_high_ctx_threshold - 0.6).abs() < f32::EPSILON);
        assert!((absent.cache.drift_drop_threshold - 0.30).abs() < f64::EPSILON);
        assert_eq!(absent.cache.drift_min_samples, 4);

        let toml = r#"
[cache]
hot_ratio_threshold = 0.45
drift_min_samples = 7
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();
        assert!((cfg.cache.hot_ratio_threshold - 0.45).abs() < f64::EPSILON);
        assert!((cfg.cache.cold_ratio_threshold - 0.3).abs() < f64::EPSILON);
        assert!((cfg.cache.hot_low_ctx_threshold - 0.7).abs() < f32::EPSILON);
        assert!((cfg.cache.cold_high_ctx_threshold - 0.6).abs() < f32::EPSILON);
        assert!((cfg.cache.drift_drop_threshold - 0.30).abs() < f64::EPSILON);
        assert_eq!(cfg.cache.drift_min_samples, 7);
    }

    #[test]
    fn quota_config_loads_split_claude_and_codex_windows() {
        let toml = r#"
[quota.claude_5h]
warning_pct = 0.70
critical_pct = 0.80

[quota.claude_weekly]
warning_pct = 0.75
critical_pct = 0.90

[quota.codex_5h]
warning_pct = 0.60
critical_pct = 0.72

[quota.codex_weekly]
warning_pct = 0.65
critical_pct = 0.82
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();

        assert!((cfg.quota.claude_5h.unwrap().warning_pct - 0.70).abs() < f32::EPSILON);
        assert!((cfg.quota.claude_weekly.unwrap().critical_pct - 0.90).abs() < f32::EPSILON);
        assert!((cfg.quota.codex_5h.unwrap().critical_pct - 0.72).abs() < f32::EPSILON);
        assert!((cfg.quota.codex_weekly.unwrap().warning_pct - 0.65).abs() < f32::EPSILON);
    }

    #[test]
    fn ux_config_defaults_to_always() {
        let cfg = QmonsterConfig::defaults();
        assert!(matches!(cfg.ux.confirm_actions, ConfirmActions::Always));
    }

    #[test]
    fn ux_config_parses_first_time() {
        let toml_str = r#"
[ux]
confirm_actions = "first_time"
"#;
        let cfg: QmonsterConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(cfg.ux.confirm_actions, ConfirmActions::FirstTime));
    }

    #[test]
    fn ux_config_parses_never() {
        let toml_str = r#"
[ux]
confirm_actions = "never"
"#;
        let cfg: QmonsterConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(cfg.ux.confirm_actions, ConfirmActions::Never));
    }

    #[test]
    fn quota_config_legacy_provider_override_feeds_both_windows() {
        let toml = r#"
[quota.claude]
warning_pct = 0.80
critical_pct = 0.90
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).unwrap();

        assert!(
            (cfg.quota.warning_for_window(
                crate::domain::identity::Provider::Claude,
                QuotaWindow::FiveHour
            ) - 0.80)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (cfg.quota.critical_for_window(
                crate::domain::identity::Provider::Claude,
                QuotaWindow::Weekly
            ) - 0.90)
                .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn reset_config_auto_snapshot_defaults_false() {
        let r = ResetConfig::default();
        assert!(!r.auto_snapshot);
    }

    #[test]
    fn reset_config_auto_snapshot_loads_from_toml() {
        let toml = r#"
[reset]
auto_snapshot = true
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).expect("parse");
        assert!(cfg.reset.auto_snapshot);
    }

    #[test]
    fn reset_config_missing_section_keeps_auto_snapshot_false() {
        let cfg: QmonsterConfig = toml::from_str("").expect("parse");
        assert!(!cfg.reset.auto_snapshot);
    }

    #[test]
    fn reset_config_partial_section_keeps_other_defaults() {
        let toml = r#"
[reset]
auto_snapshot = true
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).expect("parse");
        assert!(cfg.reset.auto_snapshot);
        assert!((cfg.reset.wait_pressure_threshold - 0.85).abs() < f32::EPSILON);
        assert_eq!(cfg.reset.wait_eta_secs, 30 * 60);
        assert!((cfg.reset.snapshot_pressure_threshold - 0.50).abs() < f32::EPSILON);
        assert_eq!(cfg.reset.snapshot_eta_secs, 5 * 60);
    }

    #[test]
    fn anomaly_config_disabled_by_default() {
        let c = AnomalyConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.window_polls, 20);
        assert_eq!(c.min_confidence, "medium");
        assert_eq!(c.identity_churn_min_flips, 3);
        assert!((c.error_burst_threshold - 0.5).abs() < f32::EPSILON);
        assert!((c.cache_discontinuity_drop - 0.30).abs() < f32::EPSILON);
        assert_eq!(c.cross_pane_cluster_min_findings, 3);
    }

    #[test]
    fn anomaly_config_loads_from_toml_with_partial_overrides() {
        let toml = r#"
[anomaly]
enabled = true
window_polls = 30
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).expect("parse");
        assert!(cfg.anomaly.enabled);
        assert_eq!(cfg.anomaly.window_polls, 30);
        assert_eq!(cfg.anomaly.min_confidence, "medium"); // default kept
        assert_eq!(cfg.anomaly.identity_churn_min_flips, 3); // default kept
    }

    #[test]
    fn anomaly_config_missing_section_disables_layer() {
        let cfg: QmonsterConfig = toml::from_str("").expect("parse");
        assert!(!cfg.anomaly.enabled);
    }

    #[test]
    fn anomaly_config_v2_defaults() {
        let c = AnomalyConfig::default();
        assert!((c.cost_slope_usd_per_hour - 20.0).abs() < f64::EPSILON);
        assert_eq!(c.token_slope_input_per_poll, 20_000);
        assert!((c.memory_growth_mb - 1024.0).abs() < f64::EPSILON);
    }

    #[test]
    fn anomaly_config_v2_loads_from_toml() {
        let toml = r#"
[anomaly]
cost_slope_usd_per_hour = 30.0
token_slope_input_per_poll = 50000
memory_growth_mb = 2048
"#;
        let cfg: QmonsterConfig = toml::from_str(toml).expect("parse");
        assert!((cfg.anomaly.cost_slope_usd_per_hour - 30.0).abs() < f64::EPSILON);
        assert_eq!(cfg.anomaly.token_slope_input_per_poll, 50_000);
        assert!((cfg.anomaly.memory_growth_mb - 2048.0).abs() < f64::EPSILON);
    }

    #[test]
    fn anomaly_config_v2_missing_section_keeps_defaults() {
        let cfg: QmonsterConfig = toml::from_str("").expect("parse");
        assert!((cfg.anomaly.cost_slope_usd_per_hour - 20.0).abs() < f64::EPSILON);
        assert_eq!(cfg.anomaly.token_slope_input_per_poll, 20_000);
        assert!((cfg.anomaly.memory_growth_mb - 1024.0).abs() < f64::EPSILON);
    }

    #[test]
    fn anomaly_promote_config_defaults_match_spec() {
        let p = AnomalyPromoteConfig::default();
        assert_eq!(p.identity_churn, "high");
        assert_eq!(p.error_burst, "high");
        assert_eq!(p.cache_discontinuity, "high");
        assert_eq!(p.cross_pane_edit_cluster, "high");
        assert_eq!(p.cost_slope, "high");
        assert_eq!(p.token_slope, "high");
        assert_eq!(p.memory_growth, "high");
        assert_eq!(p.subagent_side_effect, "medium");
    }

    #[test]
    fn anomaly_config_includes_promote_default() {
        let c = AnomalyConfig::default();
        assert_eq!(c.promote.identity_churn, "high");
        assert_eq!(c.promote.subagent_side_effect, "medium");
    }

    #[test]
    fn anomaly_config_omitting_promote_section_uses_defaults() {
        // Pre-v1.46.0 toml file (no [anomaly.promote] section) loads
        // cleanly with v1.46.0 defaults applied.
        let toml = r#"
            [anomaly]
            enabled = true
            window_polls = 20
            min_confidence = "medium"
        "#;
        let cfg: QmonsterConfig = toml::from_str(toml).expect("parse");
        assert_eq!(cfg.anomaly.promote.cost_slope, "high");
        assert_eq!(cfg.anomaly.promote.subagent_side_effect, "medium");
    }
}
