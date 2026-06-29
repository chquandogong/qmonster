//! Data model for the Settings overlay — pure value types and small
//! constructors / classifiers. Extracted from `settings.rs` during the
//! post-v2.2.0 documentation diet to keep the rendering and input
//! state machine focused on behaviour.

/// Which advisory section a field belongs to. Drives unit display
/// (USD vs fraction) and validation (cost ≥ 0 vs pct ∈ [0, 1]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Cost,
    Context,
    Quota,
}

/// Whose threshold the field controls — the section-level default or
/// a per-provider override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Default,
    Claude,
    Claude5h,
    ClaudeWeekly,
    Codex,
    Codex5h,
    CodexWeekly,
    Gemini,
}

/// Warning vs critical end of the threshold pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Warning,
    Critical,
}

/// A single editable field — `(Section, Scope, Bound)` triple.
/// Cost/context use default + provider scopes; quota uses default,
/// Claude 5h/weekly, Codex 5h/weekly, and Gemini.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldId {
    pub section: Section,
    pub scope: Scope,
    pub bound: Bound,
}

impl FieldId {
    pub const fn new(section: Section, scope: Scope, bound: Bound) -> Self {
        Self {
            section,
            scope,
            bound,
        }
    }

    /// Phase E E2 (v1.20.0): the dotted TOML path that this field
    /// occupies in `qmonster.toml`. Used by `SettingsOverlay::save()`
    /// to surgically rewrite individual values via `toml_edit` while
    /// leaving the rest of the file's comments and structure
    /// untouched.
    ///
    /// Cost section uses `_usd` suffix; context and quota use
    /// `_pct`. Default scope writes at the section root; per-provider
    /// scopes nest under a sub-table named after the provider (with
    /// the `_5h` / `_weekly` window suffix for Claude/Codex quota).
    pub fn toml_path(self) -> Vec<&'static str> {
        let leaf = match (self.section, self.bound) {
            (Section::Cost, Bound::Warning) => "warning_usd",
            (Section::Cost, Bound::Critical) => "critical_usd",
            (Section::Context, Bound::Warning) | (Section::Quota, Bound::Warning) => "warning_pct",
            (Section::Context, Bound::Critical) | (Section::Quota, Bound::Critical) => {
                "critical_pct"
            }
        };
        let section_key = match self.section {
            Section::Cost => "cost",
            Section::Context => "context",
            Section::Quota => "quota",
        };
        let scope_key = match self.scope {
            Scope::Default => None,
            Scope::Claude => Some("claude"),
            Scope::Claude5h => Some("claude_5h"),
            Scope::ClaudeWeekly => Some("claude_weekly"),
            Scope::Codex => Some("codex"),
            Scope::Codex5h => Some("codex_5h"),
            Scope::CodexWeekly => Some("codex_weekly"),
            Scope::Gemini => Some("gemini"),
        };
        match scope_key {
            None => vec![section_key, leaf],
            Some(s) => vec![section_key, s, leaf],
        }
    }
}

/// The fields in the order they appear in the overlay. Iteration
/// order is `(section outer, scope middle, bound inner)` so a single
/// row of the rendered grid is one `(section, scope)` pair with the
/// warning + critical cells side by side, and adjacent sections sit
/// on consecutive rows of the same scope.
pub fn all_fields() -> Vec<FieldId> {
    let mut out = Vec::with_capacity(28);
    for section in [Section::Cost, Section::Context, Section::Quota] {
        for scope in scopes_for_section(section) {
            for bound in [Bound::Warning, Bound::Critical] {
                out.push(FieldId::new(section, *scope, bound));
            }
        }
    }
    out
}

pub(super) fn scopes_for_section(section: Section) -> &'static [Scope] {
    match section {
        Section::Cost | Section::Context => {
            &[Scope::Default, Scope::Claude, Scope::Codex, Scope::Gemini]
        }
        Section::Quota => &[
            Scope::Default,
            Scope::Claude5h,
            Scope::ClaudeWeekly,
            Scope::Codex5h,
            Scope::CodexWeekly,
            Scope::Gemini,
        ],
    }
}

/// Top-level pages inside the Settings modal. Thresholds and
/// Integrations are editable; Parameters, Rules, and Badges are
/// read-only reference pages for the currently loaded runtime
/// configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Thresholds,
    Integrations,
    Parameters,
    Rules,
    Badges,
}

impl SettingsTab {
    pub const fn label(self) -> &'static str {
        match self {
            SettingsTab::Thresholds => "Thresholds",
            SettingsTab::Integrations => "Integrations",
            SettingsTab::Parameters => "Parameters",
            SettingsTab::Rules => "Rules",
            SettingsTab::Badges => "Badges",
        }
    }

    pub const fn index(self) -> usize {
        match self {
            SettingsTab::Thresholds => 0,
            SettingsTab::Integrations => 1,
            SettingsTab::Parameters => 2,
            SettingsTab::Rules => 3,
            SettingsTab::Badges => 4,
        }
    }

    pub const fn next(self) -> Self {
        match self {
            SettingsTab::Thresholds => SettingsTab::Integrations,
            SettingsTab::Integrations => SettingsTab::Parameters,
            SettingsTab::Parameters => SettingsTab::Rules,
            SettingsTab::Rules => SettingsTab::Badges,
            SettingsTab::Badges => SettingsTab::Thresholds,
        }
    }

    pub const fn previous(self) -> Self {
        match self {
            SettingsTab::Thresholds => SettingsTab::Badges,
            SettingsTab::Integrations => SettingsTab::Thresholds,
            SettingsTab::Parameters => SettingsTab::Integrations,
            SettingsTab::Rules => SettingsTab::Parameters,
            SettingsTab::Badges => SettingsTab::Rules,
        }
    }

    /// v1.51.0: Thresholds / Integrations / Parameters carry editable
    /// rows; Rules / Badges are reference-only. Used by the tab strip
    /// renderer to draw a `║` between groups and to suffix the body
    /// title with `(editable)` vs `(reference)`.
    pub const fn is_editable(self) -> bool {
        matches!(
            self,
            SettingsTab::Thresholds | SettingsTab::Integrations | SettingsTab::Parameters
        )
    }
}

pub(super) const SETTINGS_TABS: [SettingsTab; 5] = [
    SettingsTab::Thresholds,
    SettingsTab::Integrations,
    SettingsTab::Parameters,
    SettingsTab::Rules,
    SettingsTab::Badges,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationField {
    ClaudeSidefile,
}

impl IntegrationField {
    pub const fn label(self) -> &'static str {
        match self {
            IntegrationField::ClaudeSidefile => "Claude sidefile export",
        }
    }

    pub(super) const fn next(self) -> Self {
        match self {
            IntegrationField::ClaudeSidefile => IntegrationField::ClaudeSidefile,
        }
    }

    pub(super) const fn previous(self) -> Self {
        match self {
            IntegrationField::ClaudeSidefile => IntegrationField::ClaudeSidefile,
        }
    }
}

/// Editable fields shown on Settings > Parameters. Threshold rows keep
/// their older dedicated editor; this enum covers the remaining
/// runtime config surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterField {
    TmuxSource,
    TmuxPollIntervalMs,
    TmuxCaptureLines,
    RefreshPolicy,
    IdleStillnessPolls,
    LoggingSensitivity,
    LoggingRetentionDays,
    LoggingBigOutputChars,
    StorageRoot,
    ActionsMode,
    ActionsAutoNotifications,
    ActionsAutoArchive,
    ActionsAutoPromptSend,
    ActionsDestructive,
    UxConfirmActions,
    UxHoverHelp,
    UxHoverHelpTrigger,
    UxHelpLanguage,
    UxTheme,
    TokenQuotaTight,
    SecurityPostureAdvisories,
    SecurityCrossWindowFindings,
    SecurityIdentityDriftFindings,
    SecurityCrossPaneFileFindings,
    CacheHotRatioThreshold,
    CacheColdRatioThreshold,
    CacheHotLowCtxThreshold,
    CacheColdHighCtxThreshold,
    CacheDriftDropThreshold,
    CacheDriftMinSamples,
    ResetWaitPressureThreshold,
    ResetWaitEtaSecs,
    ResetSnapshotPressureThreshold,
    ResetSnapshotEtaSecs,
    ResetAutoSnapshot,
    AnomalyEnabled,
    AnomalyWindowPolls,
    AnomalyMinConfidence,
    AnomalyIdentityChurnMinFlips,
    AnomalyErrorBurstThreshold,
    AnomalyCacheDiscontinuityDrop,
    AnomalyCrossPaneClusterMinFindings,
    AnomalyCostSlopeUsdPerHour,
    AnomalyTokenSlopeInputPerPoll,
    AnomalyMemoryGrowthMb,
    AnomalyRetentionDays,
    AnomalyPromoteIdentityChurn,
    AnomalyPromoteErrorBurst,
    AnomalyPromoteCacheDiscontinuity,
    AnomalyPromoteCrossPaneEditCluster,
    AnomalyPromoteCostSlope,
    AnomalyPromoteTokenSlope,
    AnomalyPromoteMemoryGrowth,
    AnomalyPromoteSubagentSideEffect,
    CostBudgetUsd,
    ProfileSwitchEnabled,
    ProfileSwitchWindowPolls,
    ProfileSwitchErrorRateThreshold,
    FxEnabled,
    FxText,
    FxEffect,
    FxDurationSecs,
    FxHotkeyEnabled,
    FxCelebrationEnabled,
    FxScreensaverEnabled,
    FxScreensaverIdleSecs,
}

pub fn all_parameter_fields() -> Vec<ParameterField> {
    use ParameterField::*;
    vec![
        TmuxSource,
        TmuxPollIntervalMs,
        TmuxCaptureLines,
        RefreshPolicy,
        IdleStillnessPolls,
        LoggingSensitivity,
        LoggingRetentionDays,
        LoggingBigOutputChars,
        StorageRoot,
        ActionsMode,
        ActionsAutoNotifications,
        ActionsAutoArchive,
        ActionsAutoPromptSend,
        ActionsDestructive,
        UxConfirmActions,
        UxHoverHelp,
        UxHoverHelpTrigger,
        UxHelpLanguage,
        UxTheme,
        TokenQuotaTight,
        SecurityPostureAdvisories,
        SecurityCrossWindowFindings,
        SecurityIdentityDriftFindings,
        SecurityCrossPaneFileFindings,
        CacheHotRatioThreshold,
        CacheColdRatioThreshold,
        CacheHotLowCtxThreshold,
        CacheColdHighCtxThreshold,
        CacheDriftDropThreshold,
        CacheDriftMinSamples,
        ResetWaitPressureThreshold,
        ResetWaitEtaSecs,
        ResetSnapshotPressureThreshold,
        ResetSnapshotEtaSecs,
        ResetAutoSnapshot,
        AnomalyEnabled,
        AnomalyWindowPolls,
        AnomalyMinConfidence,
        AnomalyIdentityChurnMinFlips,
        AnomalyErrorBurstThreshold,
        AnomalyCacheDiscontinuityDrop,
        AnomalyCrossPaneClusterMinFindings,
        AnomalyCostSlopeUsdPerHour,
        AnomalyTokenSlopeInputPerPoll,
        AnomalyMemoryGrowthMb,
        AnomalyRetentionDays,
        AnomalyPromoteIdentityChurn,
        AnomalyPromoteErrorBurst,
        AnomalyPromoteCacheDiscontinuity,
        AnomalyPromoteCrossPaneEditCluster,
        AnomalyPromoteCostSlope,
        AnomalyPromoteTokenSlope,
        AnomalyPromoteMemoryGrowth,
        AnomalyPromoteSubagentSideEffect,
        CostBudgetUsd,
        ProfileSwitchEnabled,
        ProfileSwitchWindowPolls,
        ProfileSwitchErrorRateThreshold,
        FxEnabled,
        FxText,
        FxEffect,
        FxDurationSecs,
        FxHotkeyEnabled,
        FxCelebrationEnabled,
        FxScreensaverEnabled,
        FxScreensaverIdleSecs,
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParameterEditKind {
    Bool,
    Enum,
    Number,
    Text,
}
