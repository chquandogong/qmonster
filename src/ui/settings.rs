//! v1.15.18: in-TUI settings overlay for cost / context / quota
//! advisory thresholds.
//!
//! Lifts the v1.15.16 / v1.15.17 TOML-only configuration surface into
//! a keyboard-driven modal so the operator can re-tune thresholds
//! without leaving the dashboard. The state machine is config-mutating
//! but does not touch disk on its own — `save()` is an explicit
//! operator action gated on a known config path.
//!
//! Editable fields cover cost / context defaults plus per-provider
//! overrides, and quota defaults plus provider-specific quota windows.
//! Claude/Codex quota rows split into 5h and weekly windows; Gemini
//! keeps its single quota row. Provider/window scopes with no override
//! read as inherited from the section default and display as `(default)`
//! until the operator types a value.

use crate::app::config::{
    ActionsMode, ConfirmActions, ContextConfig, CostConfig, CostProviderConfig, HelpLanguage,
    HoverHelpTrigger, LogSensitivity, PressureProviderConfig, QmonsterConfig, QuotaConfig,
    RefreshPolicy, TmuxSourceMode,
};
use crate::domain::recommendation::Severity;
use crate::ui::scroll_hint;
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use std::path::Path;

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

fn scopes_for_section(section: Section) -> &'static [Scope] {
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

const SETTINGS_TABS: [SettingsTab; 5] = [
    SettingsTab::Thresholds,
    SettingsTab::Integrations,
    SettingsTab::Parameters,
    SettingsTab::Rules,
    SettingsTab::Badges,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationField {
    ClaudeSidefile,
    CodexAppServer,
}

impl IntegrationField {
    pub const fn label(self) -> &'static str {
        match self {
            IntegrationField::ClaudeSidefile => "Claude sidefile export",
            IntegrationField::CodexAppServer => "Codex app-server auto-spawn",
        }
    }

    const fn next(self) -> Self {
        match self {
            IntegrationField::ClaudeSidefile => IntegrationField::CodexAppServer,
            IntegrationField::CodexAppServer => IntegrationField::ClaudeSidefile,
        }
    }

    const fn previous(self) -> Self {
        match self {
            IntegrationField::ClaudeSidefile => IntegrationField::CodexAppServer,
            IntegrationField::CodexAppServer => IntegrationField::ClaudeSidefile,
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
    InsightsIgnoredTtlSecs,
    InsightsDefaultWindowSecs,
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
        InsightsIgnoredTtlSecs,
        InsightsDefaultWindowSecs,
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
enum ParameterEditKind {
    Bool,
    Enum,
    Number,
    Text,
}

/// One-line status banner shown at the bottom of the overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsStatus {
    /// No edits since the overlay was opened (or since the last save).
    Idle,
    /// At least one field has been edited but not yet saved.
    Dirty,
    /// Last commit attempt failed validation; banner shows the reason.
    Error(String),
    /// Last save action succeeded; banner shows the file path written.
    Saved(String),
}

/// Settings overlay state machine. Holds no copy of the config — the
/// caller passes `&QmonsterConfig` (read) or `&mut QmonsterConfig`
/// (write) at each operation so there is one source of truth.
#[derive(Debug, Clone)]
pub struct SettingsOverlay {
    open: bool,
    tab: SettingsTab,
    selected: FieldId,
    selected_integration: IntegrationField,
    selected_parameter: ParameterField,
    scroll_offset: u16,
    edit_buffer: Option<String>,
    status: SettingsStatus,
    /// Tracks dirty-since-open so `is_dirty()` survives a transient
    /// `Saved` / `Error` status banner change.
    dirty: bool,
    dirty_parameters: Vec<ParameterField>,
}

impl Default for SettingsOverlay {
    fn default() -> Self {
        Self {
            open: false,
            tab: SettingsTab::Thresholds,
            selected: FieldId::new(Section::Cost, Scope::Default, Bound::Warning),
            selected_integration: IntegrationField::ClaudeSidefile,
            selected_parameter: ParameterField::TmuxSource,
            scroll_offset: 0,
            edit_buffer: None,
            status: SettingsStatus::Idle,
            dirty: false,
            dirty_parameters: Vec::new(),
        }
    }
}

impl SettingsOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn selected(&self) -> FieldId {
        self.selected
    }

    pub fn tab(&self) -> SettingsTab {
        self.tab
    }

    pub fn selected_integration(&self) -> IntegrationField {
        self.selected_integration
    }

    pub fn selected_parameter(&self) -> ParameterField {
        self.selected_parameter
    }

    pub fn scroll_offset(&self) -> u16 {
        self.scroll_offset
    }

    pub fn switch_tab(&mut self, tab: SettingsTab) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.tab = tab;
        self.scroll_offset = 0;
    }

    pub fn next_tab(&mut self) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.tab = self.tab.next();
        self.scroll_offset = 0;
    }

    pub fn previous_tab(&mut self) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.tab = self.tab.previous();
        self.scroll_offset = 0;
    }

    pub fn select_field(&mut self, field: FieldId) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Thresholds {
            return;
        }
        self.selected = field;
    }

    pub fn select_integration(&mut self, field: IntegrationField) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Integrations {
            return;
        }
        self.selected_integration = field;
    }

    pub fn select_parameter(&mut self, field: ParameterField) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Parameters {
            return;
        }
        self.selected_parameter = field;
    }

    pub fn toggle_hover_help_setting(&mut self, config: &mut QmonsterConfig) {
        if self.edit_buffer.is_some() {
            return;
        }
        config.ux.hover_help = !config.ux.hover_help;
        self.mark_parameter_dirty(ParameterField::UxHoverHelp);
    }

    pub fn toggle_help_language_setting(&mut self, config: &mut QmonsterConfig) {
        if self.edit_buffer.is_some() {
            return;
        }
        config.ux.help_language = config.ux.help_language.toggle();
        self.mark_parameter_dirty(ParameterField::UxHelpLanguage);
    }

    pub fn edit_buffer(&self) -> Option<&str> {
        self.edit_buffer.as_deref()
    }

    pub fn status(&self) -> &SettingsStatus {
        &self.status
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Open the overlay and reset state. The first field
    /// (cost / default / warning) becomes the focus.
    pub fn open(&mut self) {
        self.open = true;
        self.tab = SettingsTab::Thresholds;
        self.selected = FieldId::new(Section::Cost, Scope::Default, Bound::Warning);
        self.selected_integration = IntegrationField::ClaudeSidefile;
        self.selected_parameter = ParameterField::TmuxSource;
        self.scroll_offset = 0;
        self.edit_buffer = None;
        self.status = SettingsStatus::Idle;
        self.dirty = false;
        self.dirty_parameters.clear();
    }

    /// Close the overlay, discarding any in-flight edit buffer. Does
    /// not revert config edits already committed.
    pub fn close(&mut self) {
        self.open = false;
        self.edit_buffer = None;
    }

    /// Move focus to the next field, wrapping at the end of the list.
    /// No-op when an edit buffer is active (must commit/cancel first).
    pub fn next_field(&mut self) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Thresholds {
            return;
        }
        let fields = all_fields();
        let idx = fields.iter().position(|f| *f == self.selected).unwrap_or(0);
        self.selected = fields[(idx + 1) % fields.len()];
    }

    /// Move focus to the previous field, wrapping at the start.
    pub fn prev_field(&mut self) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Thresholds {
            return;
        }
        let fields = all_fields();
        let idx = fields.iter().position(|f| *f == self.selected).unwrap_or(0);
        self.selected = fields[(idx + fields.len() - 1) % fields.len()];
    }

    pub fn next_integration(&mut self) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Integrations {
            return;
        }
        self.selected_integration = self.selected_integration.next();
    }

    pub fn prev_integration(&mut self) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Integrations {
            return;
        }
        self.selected_integration = self.selected_integration.previous();
    }

    pub fn next_parameter(&mut self) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Parameters {
            return;
        }
        let fields = all_parameter_fields();
        let idx = fields
            .iter()
            .position(|f| *f == self.selected_parameter)
            .unwrap_or(0);
        self.selected_parameter = fields[(idx + 1) % fields.len()];
    }

    pub fn prev_parameter(&mut self) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Parameters {
            return;
        }
        let fields = all_parameter_fields();
        let idx = fields
            .iter()
            .position(|f| *f == self.selected_parameter)
            .unwrap_or(0);
        self.selected_parameter = fields[(idx + fields.len() - 1) % fields.len()];
    }

    pub fn scroll_up(&mut self) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self, max: u16) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.scroll_offset = self.scroll_offset.saturating_add(1).min(max);
    }

    pub fn page_up(&mut self, rows: u16) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.scroll_offset = self.scroll_offset.saturating_sub(rows.max(1));
    }

    pub fn page_down(&mut self, rows: u16, max: u16) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.scroll_offset = self.scroll_offset.saturating_add(rows.max(1)).min(max);
    }

    pub fn scroll_top(&mut self) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.scroll_offset = 0;
    }

    pub fn scroll_bottom(&mut self, max: u16) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.scroll_offset = max;
    }

    pub fn set_scroll_offset(&mut self, offset: u16) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.scroll_offset = offset;
    }

    pub fn toggle_integration(&mut self, config: &mut QmonsterConfig) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Integrations {
            return;
        }
        match self.selected_integration {
            IntegrationField::ClaudeSidefile => {
                config.provider_setup.claude_sidefile = !config.provider_setup.claude_sidefile;
            }
            IntegrationField::CodexAppServer => {
                config.provider_setup.codex_app_server = !config.provider_setup.codex_app_server;
            }
        }
        self.dirty = true;
        self.status = SettingsStatus::Dirty;
    }

    /// Read the value the field would resolve to today, or `None` for
    /// a per-provider scope with no override set.
    pub fn read_field(&self, config: &QmonsterConfig, field: FieldId) -> Option<f64> {
        match field.section {
            Section::Cost => read_cost(config, field),
            Section::Context => read_pct(&config.context, field).map(f64::from),
            Section::Quota => read_pct(&config.quota, field).map(f64::from),
        }
    }

    /// What the field's effective value is — the override if set, else
    /// the section default. Always returns `Some` (default scope is
    /// always populated).
    pub fn effective_field(&self, config: &QmonsterConfig, field: FieldId) -> f64 {
        if let Some(v) = self.read_field(config, field) {
            return v;
        }
        // Per-provider scope with no override → resolve to default.
        let default_field = FieldId::new(field.section, Scope::Default, field.bound);
        self.read_field(config, default_field)
            .expect("default scope is always populated")
    }

    /// Begin editing the focused field. Buffer initializes with the
    /// effective (resolved) value formatted for in-place editing.
    pub fn start_edit(&mut self, config: &QmonsterConfig) {
        if !self.open {
            return;
        }
        match self.tab {
            SettingsTab::Thresholds => {
                let v = self.effective_field(config, self.selected);
                // Strip trailing zeros so 5.0 reads back as "5" but 0.75 stays
                // "0.75". Operator who wants "5.00" can type the zeros.
                self.edit_buffer = Some(format_value_for_edit(v, self.selected.section));
                self.status = SettingsStatus::Idle;
            }
            SettingsTab::Parameters
                if matches!(
                    parameter_edit_kind(self.selected_parameter),
                    ParameterEditKind::Number | ParameterEditKind::Text
                ) =>
            {
                self.edit_buffer = Some(parameter_value_for_edit(config, self.selected_parameter));
                self.status = SettingsStatus::Idle;
            }
            _ => {}
        }
    }

    /// Append a character to the edit buffer. Numeric edits accept
    /// digits / dot / minus. Text parameter edits accept printable ASCII.
    pub fn type_char(&mut self, c: char) {
        let free_text = self.tab == SettingsTab::Parameters
            && parameter_edit_kind(self.selected_parameter) == ParameterEditKind::Text;
        let Some(buf) = self.edit_buffer.as_mut() else {
            return;
        };
        if free_text {
            if c.is_ascii() && !c.is_control() {
                buf.push(c);
            }
            return;
        }
        if !c.is_ascii_digit() && c != '.' && c != '-' {
            return;
        }
        // Reject a second decimal point or stray minus.
        if c == '.' && buf.contains('.') {
            return;
        }
        if c == '-' && !buf.is_empty() {
            return;
        }
        buf.push(c);
    }

    /// Remove the last character from the edit buffer.
    pub fn backspace(&mut self) {
        if let Some(buf) = self.edit_buffer.as_mut() {
            buf.pop();
        }
    }

    /// Discard the in-flight edit without touching the config.
    pub fn cancel_edit(&mut self) {
        self.edit_buffer = None;
    }

    /// Set the status banner to an error message. Used by the event
    /// loop when a save action cannot proceed (e.g. no config_path).
    pub fn set_save_error(&mut self, msg: String) {
        self.status = SettingsStatus::Error(msg);
    }

    pub fn activate_parameter(&mut self, config: &mut QmonsterConfig) -> Result<(), String> {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Parameters {
            return Ok(());
        }
        match parameter_edit_kind(self.selected_parameter) {
            ParameterEditKind::Bool => {
                toggle_parameter_bool(config, self.selected_parameter);
                self.mark_parameter_dirty(self.selected_parameter);
                Ok(())
            }
            ParameterEditKind::Enum => {
                cycle_parameter_enum(config, self.selected_parameter)?;
                self.mark_parameter_dirty(self.selected_parameter);
                Ok(())
            }
            ParameterEditKind::Number | ParameterEditKind::Text => {
                self.start_edit(config);
                Ok(())
            }
        }
    }

    /// Parse the edit buffer and apply it to the config if it
    /// validates. Returns `Ok(())` on success, `Err(reason)` on
    /// validation failure (status banner is also updated).
    pub fn commit_edit(&mut self, config: &mut QmonsterConfig) -> Result<(), String> {
        let Some(buf) = self.edit_buffer.take() else {
            return Err("no active edit".into());
        };
        if self.tab == SettingsTab::Parameters {
            return match apply_parameter_edit(config, self.selected_parameter, buf.trim()) {
                Ok(()) => {
                    self.mark_parameter_dirty(self.selected_parameter);
                    Ok(())
                }
                Err(msg) => {
                    self.edit_buffer = Some(buf);
                    self.status = SettingsStatus::Error(msg.clone());
                    Err(msg)
                }
            };
        }
        let parsed: f64 = match buf.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                let msg = format!("not a number: {buf}");
                self.edit_buffer = Some(buf);
                self.status = SettingsStatus::Error(msg.clone());
                return Err(msg);
            }
        };
        match validate_field_value(self.selected, parsed, config) {
            Ok(()) => {
                apply_field_value(config, self.selected, parsed);
                self.dirty = true;
                self.status = SettingsStatus::Dirty;
                Ok(())
            }
            Err(msg) => {
                self.edit_buffer = Some(buf);
                self.status = SettingsStatus::Error(msg.clone());
                Err(msg)
            }
        }
    }

    fn mark_parameter_dirty(&mut self, field: ParameterField) {
        if !self.dirty_parameters.contains(&field) {
            self.dirty_parameters.push(field);
        }
        self.dirty = true;
        self.status = SettingsStatus::Dirty;
    }

    #[cfg(test)]
    pub fn replace_edit_buffer_for_test(&mut self, value: &str) {
        self.edit_buffer = Some(value.to_string());
    }

    /// Clear a per-provider override on the focused field (revert to
    /// section default). No-op for `Scope::Default` fields and during
    /// an active edit. The companion `Bound` on the same `(section,
    /// scope)` row is also cleared because the override struct holds
    /// the pair as a unit — leaving one half set with the other
    /// cleared would leave the override partially populated and is
    /// always operator-confusing.
    pub fn clear_override(&mut self, config: &mut QmonsterConfig) {
        if self.edit_buffer.is_some() || self.tab != SettingsTab::Thresholds {
            return;
        }
        if self.selected.scope == Scope::Default {
            return;
        }
        clear_provider_override(config, self.selected.section, self.selected.scope);
        self.dirty = true;
        self.status = SettingsStatus::Dirty;
    }

    /// Serialize the config as TOML and write it to `path`. Updates
    /// `status` to `Saved(path)` on success or `Error(msg)` on
    /// failure. Does not touch the in-memory config.
    pub fn save(&mut self, config: &QmonsterConfig, path: &Path) -> Result<(), String> {
        // Re-validate every threshold pair so a save cannot persist a
        // pair where warning >= critical (the unit advisories rule
        // would otherwise compute an empty range and silently never fire).
        if let Err(msg) = validate_full_config(config) {
            self.status = SettingsStatus::Error(msg.clone());
            return Err(msg);
        }
        // Phase E E2 (v1.20.0): when the file already exists, route
        // the save through `toml_edit` so operator-authored comments,
        // unrelated sections, and key order in `qmonster.toml` all
        // survive a threshold edit. When the file does not yet exist
        // (fresh operator setup), keep the legacy
        // `toml::to_string_pretty` scaffold so the first write
        // produces the same defaults a new operator is used to.
        let body = match std::fs::read_to_string(path) {
            Ok(existing) => match self.merge_overlay_fields(&existing, config) {
                Ok(s) => s,
                Err(e) => {
                    self.status = SettingsStatus::Error(e.clone());
                    return Err(e);
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                match toml::to_string_pretty(config) {
                    Ok(s) => s,
                    Err(e) => {
                        let msg = format!("serialize: {e}");
                        self.status = SettingsStatus::Error(msg.clone());
                        return Err(msg);
                    }
                }
            }
            Err(e) => {
                let msg = format!("read {}: {e}", path.display());
                self.status = SettingsStatus::Error(msg.clone());
                return Err(msg);
            }
        };
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            let msg = format!("create {}: {e}", parent.display());
            self.status = SettingsStatus::Error(msg.clone());
            return Err(msg);
        }
        if let Err(e) = std::fs::write(path, body) {
            let msg = format!("write {}: {e}", path.display());
            self.status = SettingsStatus::Error(msg.clone());
            return Err(msg);
        }
        self.dirty = false;
        self.dirty_parameters.clear();
        self.status = SettingsStatus::Saved(path.display().to_string());
        Ok(())
    }

    /// Phase E E2 (v1.20.0): surgically update each settings-overlay
    /// field in an existing TOML document while leaving comments,
    /// unrelated sections, and key order untouched. Per-provider
    /// scopes whose `read_field` returns `None` (no override set) are
    /// left as-is — `save` does not promote a default into an
    /// explicit override.
    fn merge_overlay_fields(
        &self,
        existing: &str,
        config: &QmonsterConfig,
    ) -> Result<String, String> {
        let mut doc: toml_edit::DocumentMut = existing
            .parse()
            .map_err(|e: toml_edit::TomlError| format!("parse existing TOML: {e}"))?;

        for field in all_fields() {
            let Some(v) = self.read_field(config, field) else {
                continue;
            };
            let path = field.toml_path();
            set_nested_f64(&mut doc, &path, v);
        }
        set_nested_bool(
            &mut doc,
            &["provider_setup", "claude_sidefile"],
            config.provider_setup.claude_sidefile,
        );
        set_nested_bool(
            &mut doc,
            &["provider_setup", "codex_app_server"],
            config.provider_setup.codex_app_server,
        );
        for field in &self.dirty_parameters {
            merge_parameter_field(&mut doc, config, *field);
        }

        Ok(doc.to_string())
    }
}

/// Helper for `merge_overlay_fields`. Walks the dotted TOML path,
/// creating intermediate tables when missing, and assigns the leaf
/// value. The leaf is always written as a plain `f64`; `toml_edit`
/// preserves the surrounding line comments.
fn set_nested_f64(doc: &mut toml_edit::DocumentMut, path: &[&str], v: f64) {
    use toml_edit::{Item, Table, value};
    if path.is_empty() {
        return;
    }
    let last = path.len() - 1;
    let mut current: &mut Table = doc.as_table_mut();
    for seg in &path[..last] {
        // Ensure the segment exists AND is a table before we descend.
        // Overlay validation rules out the "key exists but is a scalar"
        // case in normal flows; if a malformed file reaches this path
        // anyway, replacing the scalar with a fresh table keeps the
        // save from panicking.
        let needs_replace = current
            .get(seg)
            .map(|item| !item.is_table())
            .unwrap_or(true);
        if needs_replace {
            current.insert(seg, Item::Table(Table::new()));
        }
        current = current
            .get_mut(seg)
            .and_then(|item| item.as_table_mut())
            .expect("segment is a table after the ensure step above");
    }
    current.insert(path[last], value(v));
}

fn set_nested_bool(doc: &mut toml_edit::DocumentMut, path: &[&str], v: bool) {
    use toml_edit::{Item, Table, value};
    if path.is_empty() {
        return;
    }
    let last = path.len() - 1;
    let mut current: &mut Table = doc.as_table_mut();
    for seg in &path[..last] {
        let needs_replace = current
            .get(seg)
            .map(|item| !item.is_table())
            .unwrap_or(true);
        if needs_replace {
            current.insert(seg, Item::Table(Table::new()));
        }
        current = current
            .get_mut(seg)
            .and_then(|item| item.as_table_mut())
            .expect("segment is a table after the ensure step above");
    }
    current.insert(path[last], value(v));
}

fn set_nested_i64(doc: &mut toml_edit::DocumentMut, path: &[&str], v: i64) {
    use toml_edit::{Item, Table, value};
    if path.is_empty() {
        return;
    }
    let last = path.len() - 1;
    let mut current: &mut Table = doc.as_table_mut();
    for seg in &path[..last] {
        let needs_replace = current
            .get(seg)
            .map(|item| !item.is_table())
            .unwrap_or(true);
        if needs_replace {
            current.insert(seg, Item::Table(Table::new()));
        }
        current = current
            .get_mut(seg)
            .and_then(|item| item.as_table_mut())
            .expect("segment is a table after the ensure step above");
    }
    current.insert(path[last], value(v));
}

fn set_nested_str(doc: &mut toml_edit::DocumentMut, path: &[&str], v: &str) {
    use toml_edit::{Item, Table, value};
    if path.is_empty() {
        return;
    }
    let last = path.len() - 1;
    let mut current: &mut Table = doc.as_table_mut();
    for seg in &path[..last] {
        let needs_replace = current
            .get(seg)
            .map(|item| !item.is_table())
            .unwrap_or(true);
        if needs_replace {
            current.insert(seg, Item::Table(Table::new()));
        }
        current = current
            .get_mut(seg)
            .and_then(|item| item.as_table_mut())
            .expect("segment is a table after the ensure step above");
    }
    current.insert(path[last], value(v));
}

fn remove_nested(doc: &mut toml_edit::DocumentMut, path: &[&str]) {
    if path.is_empty() {
        return;
    }
    let last = path.len() - 1;
    let mut current = doc.as_table_mut();
    for seg in &path[..last] {
        let Some(next) = current.get_mut(seg).and_then(|item| item.as_table_mut()) else {
            return;
        };
        current = next;
    }
    current.remove(path[last]);
}

fn parameter_edit_kind(field: ParameterField) -> ParameterEditKind {
    use ParameterEditKind::*;
    use ParameterField::*;
    match field {
        ActionsAutoNotifications
        | ActionsAutoArchive
        | ActionsAutoPromptSend
        | ActionsDestructive
        | UxHoverHelp
        | TokenQuotaTight
        | SecurityPostureAdvisories
        | SecurityCrossWindowFindings
        | SecurityIdentityDriftFindings
        | SecurityCrossPaneFileFindings
        | ResetAutoSnapshot
        | AnomalyEnabled
        | ProfileSwitchEnabled
        | FxEnabled
        | FxHotkeyEnabled
        | FxCelebrationEnabled
        | FxScreensaverEnabled => Bool,
        TmuxSource
        | RefreshPolicy
        | LoggingSensitivity
        | ActionsMode
        | UxConfirmActions
        | UxHoverHelpTrigger
        | UxHelpLanguage
        | AnomalyMinConfidence
        | AnomalyPromoteIdentityChurn
        | AnomalyPromoteErrorBurst
        | AnomalyPromoteCacheDiscontinuity
        | AnomalyPromoteCrossPaneEditCluster
        | AnomalyPromoteCostSlope
        | AnomalyPromoteTokenSlope
        | AnomalyPromoteMemoryGrowth
        | AnomalyPromoteSubagentSideEffect
        | FxEffect => Enum,
        StorageRoot | FxText => Text,
        TmuxPollIntervalMs
        | TmuxCaptureLines
        | IdleStillnessPolls
        | LoggingRetentionDays
        | LoggingBigOutputChars
        | InsightsIgnoredTtlSecs
        | InsightsDefaultWindowSecs
        | CacheHotRatioThreshold
        | CacheColdRatioThreshold
        | CacheHotLowCtxThreshold
        | CacheColdHighCtxThreshold
        | CacheDriftDropThreshold
        | CacheDriftMinSamples
        | ResetWaitPressureThreshold
        | ResetWaitEtaSecs
        | ResetSnapshotPressureThreshold
        | ResetSnapshotEtaSecs
        | AnomalyWindowPolls
        | AnomalyIdentityChurnMinFlips
        | AnomalyErrorBurstThreshold
        | AnomalyCacheDiscontinuityDrop
        | AnomalyCrossPaneClusterMinFindings
        | AnomalyCostSlopeUsdPerHour
        | AnomalyTokenSlopeInputPerPoll
        | AnomalyMemoryGrowthMb
        | AnomalyRetentionDays
        | CostBudgetUsd
        | ProfileSwitchWindowPolls
        | ProfileSwitchErrorRateThreshold
        | FxDurationSecs
        | FxScreensaverIdleSecs => Number,
    }
}

fn parameter_section_label(field: ParameterField) -> &'static str {
    use ParameterField::*;
    match field {
        TmuxSource
        | TmuxPollIntervalMs
        | TmuxCaptureLines
        | RefreshPolicy
        | IdleStillnessPolls
        | LoggingSensitivity
        | LoggingRetentionDays
        | LoggingBigOutputChars
        | StorageRoot => "Runtime",
        ActionsMode
        | ActionsAutoNotifications
        | ActionsAutoArchive
        | ActionsAutoPromptSend
        | ActionsDestructive
        | UxConfirmActions
        | UxHoverHelp
        | UxHoverHelpTrigger
        | UxHelpLanguage => "Actions",
        InsightsIgnoredTtlSecs | InsightsDefaultWindowSecs => "Insights",
        TokenQuotaTight => "Policy Inputs",
        SecurityPostureAdvisories
        | SecurityCrossWindowFindings
        | SecurityIdentityDriftFindings
        | SecurityCrossPaneFileFindings => "Security",
        CacheHotRatioThreshold
        | CacheColdRatioThreshold
        | CacheHotLowCtxThreshold
        | CacheColdHighCtxThreshold
        | CacheDriftDropThreshold
        | CacheDriftMinSamples => "Cache",
        ResetWaitPressureThreshold
        | ResetWaitEtaSecs
        | ResetSnapshotPressureThreshold
        | ResetSnapshotEtaSecs
        | ResetAutoSnapshot => "Reset",
        AnomalyEnabled
        | AnomalyWindowPolls
        | AnomalyMinConfidence
        | AnomalyIdentityChurnMinFlips
        | AnomalyErrorBurstThreshold
        | AnomalyCacheDiscontinuityDrop
        | AnomalyCrossPaneClusterMinFindings
        | AnomalyCostSlopeUsdPerHour
        | AnomalyTokenSlopeInputPerPoll
        | AnomalyMemoryGrowthMb
        | AnomalyRetentionDays => "Anomaly",
        AnomalyPromoteIdentityChurn
        | AnomalyPromoteErrorBurst
        | AnomalyPromoteCacheDiscontinuity
        | AnomalyPromoteCrossPaneEditCluster
        | AnomalyPromoteCostSlope
        | AnomalyPromoteTokenSlope
        | AnomalyPromoteMemoryGrowth
        | AnomalyPromoteSubagentSideEffect => "Anomaly Promote",
        CostBudgetUsd
        | ProfileSwitchEnabled
        | ProfileSwitchWindowPolls
        | ProfileSwitchErrorRateThreshold => "Cost / Profile",
        FxEnabled
        | FxText
        | FxEffect
        | FxDurationSecs
        | FxHotkeyEnabled
        | FxCelebrationEnabled
        | FxScreensaverEnabled
        | FxScreensaverIdleSecs => "FX",
    }
}

fn parameter_label(field: ParameterField) -> &'static str {
    use ParameterField::*;
    match field {
        TmuxSource => "tmux source",
        TmuxPollIntervalMs => "tmux poll_interval_ms",
        TmuxCaptureLines => "tmux capture_lines",
        RefreshPolicy => "refresh policy",
        IdleStillnessPolls => "idle stillness_polls",
        LoggingSensitivity => "logging sensitivity",
        LoggingRetentionDays => "logging retention_days",
        LoggingBigOutputChars => "logging big_output_chars",
        StorageRoot => "storage.root",
        ActionsMode => "actions mode",
        ActionsAutoNotifications => "actions auto_notifications",
        ActionsAutoArchive => "actions auto_archive",
        ActionsAutoPromptSend => "actions auto_prompt_send",
        ActionsDestructive => "actions destructive",
        UxConfirmActions => "ux confirm_actions",
        UxHoverHelp => "ux hover_help",
        UxHoverHelpTrigger => "ux hover_help_trigger",
        UxHelpLanguage => "ux help_language",
        InsightsIgnoredTtlSecs => "insights ignored_ttl_secs",
        InsightsDefaultWindowSecs => "insights default_window_secs",
        TokenQuotaTight => "token quota_tight",
        SecurityPostureAdvisories => "security posture_advisories",
        SecurityCrossWindowFindings => "security cross_window_findings",
        SecurityIdentityDriftFindings => "security identity_drift_findings",
        SecurityCrossPaneFileFindings => "security cross_pane_file_findings",
        CacheHotRatioThreshold => "cache hot_ratio_threshold",
        CacheColdRatioThreshold => "cache cold_ratio_threshold",
        CacheHotLowCtxThreshold => "cache hot_low_ctx_threshold",
        CacheColdHighCtxThreshold => "cache cold_high_ctx_threshold",
        CacheDriftDropThreshold => "cache drift_drop_threshold",
        CacheDriftMinSamples => "cache drift_min_samples",
        ResetWaitPressureThreshold => "reset wait_pressure_threshold",
        ResetWaitEtaSecs => "reset wait_eta_secs",
        ResetSnapshotPressureThreshold => "reset snapshot_pressure_threshold",
        ResetSnapshotEtaSecs => "reset snapshot_eta_secs",
        ResetAutoSnapshot => "reset auto_snapshot",
        AnomalyEnabled => "anomaly enabled",
        AnomalyWindowPolls => "anomaly window_polls",
        AnomalyMinConfidence => "anomaly min_confidence",
        AnomalyIdentityChurnMinFlips => "anomaly identity_churn_min_flips",
        AnomalyErrorBurstThreshold => "anomaly error_burst_threshold",
        AnomalyCacheDiscontinuityDrop => "anomaly cache_discontinuity_drop",
        AnomalyCrossPaneClusterMinFindings => "anomaly cross_pane_cluster_min",
        AnomalyCostSlopeUsdPerHour => "anomaly cost_slope_usd_per_hour",
        AnomalyTokenSlopeInputPerPoll => "anomaly token_slope_input_per_poll",
        AnomalyMemoryGrowthMb => "anomaly memory_growth_mb",
        AnomalyRetentionDays => "anomaly retention_days",
        AnomalyPromoteIdentityChurn => "anomaly.promote identity_churn",
        AnomalyPromoteErrorBurst => "anomaly.promote error_burst",
        AnomalyPromoteCacheDiscontinuity => "anomaly.promote cache_discontinuity",
        AnomalyPromoteCrossPaneEditCluster => "anomaly.promote cross_pane_edit_cluster",
        AnomalyPromoteCostSlope => "anomaly.promote cost_slope",
        AnomalyPromoteTokenSlope => "anomaly.promote token_slope",
        AnomalyPromoteMemoryGrowth => "anomaly.promote memory_growth",
        AnomalyPromoteSubagentSideEffect => "anomaly.promote subagent_side_effect",
        CostBudgetUsd => "cost budget_usd",
        ProfileSwitchEnabled => "profile_switch enabled",
        ProfileSwitchWindowPolls => "profile_switch window_polls",
        ProfileSwitchErrorRateThreshold => "profile_switch error_rate_threshold",
        FxEnabled => "fx enabled",
        FxText => "fx text",
        FxEffect => "fx effect",
        FxDurationSecs => "fx duration_secs",
        FxHotkeyEnabled => "fx hotkey_enabled",
        FxCelebrationEnabled => "fx celebration_enabled",
        FxScreensaverEnabled => "fx screensaver_enabled",
        FxScreensaverIdleSecs => "fx screensaver_idle_secs",
    }
}

fn parameter_toml_path(field: ParameterField) -> Vec<&'static str> {
    use ParameterField::*;
    match field {
        TmuxSource => vec!["tmux", "source"],
        TmuxPollIntervalMs => vec!["tmux", "poll_interval_ms"],
        TmuxCaptureLines => vec!["tmux", "capture_lines"],
        RefreshPolicy => vec!["refresh", "policy"],
        IdleStillnessPolls => vec!["idle", "stillness_polls"],
        LoggingSensitivity => vec!["logging", "sensitivity"],
        LoggingRetentionDays => vec!["logging", "retention_days"],
        LoggingBigOutputChars => vec!["logging", "big_output_chars"],
        StorageRoot => vec!["storage", "root"],
        ActionsMode => vec!["actions", "mode"],
        ActionsAutoNotifications => vec!["actions", "allow_auto_notifications"],
        ActionsAutoArchive => vec!["actions", "allow_auto_archive"],
        ActionsAutoPromptSend => vec!["actions", "allow_auto_prompt_send"],
        ActionsDestructive => vec!["actions", "allow_destructive_actions"],
        UxConfirmActions => vec!["ux", "confirm_actions"],
        UxHoverHelp => vec!["ux", "hover_help"],
        UxHoverHelpTrigger => vec!["ux", "hover_help_trigger"],
        UxHelpLanguage => vec!["ux", "help_language"],
        InsightsIgnoredTtlSecs => vec!["insights", "ignored_ttl_secs"],
        InsightsDefaultWindowSecs => vec!["insights", "default_window_secs"],
        TokenQuotaTight => vec!["token", "quota_tight"],
        SecurityPostureAdvisories => vec!["security", "posture_advisories"],
        SecurityCrossWindowFindings => vec!["security", "cross_window_findings"],
        SecurityIdentityDriftFindings => vec!["security", "identity_drift_findings"],
        SecurityCrossPaneFileFindings => vec!["security", "cross_pane_file_findings"],
        CacheHotRatioThreshold => vec!["cache", "hot_ratio_threshold"],
        CacheColdRatioThreshold => vec!["cache", "cold_ratio_threshold"],
        CacheHotLowCtxThreshold => vec!["cache", "hot_low_ctx_threshold"],
        CacheColdHighCtxThreshold => vec!["cache", "cold_high_ctx_threshold"],
        CacheDriftDropThreshold => vec!["cache", "drift_drop_threshold"],
        CacheDriftMinSamples => vec!["cache", "drift_min_samples"],
        ResetWaitPressureThreshold => vec!["reset", "wait_pressure_threshold"],
        ResetWaitEtaSecs => vec!["reset", "wait_eta_secs"],
        ResetSnapshotPressureThreshold => vec!["reset", "snapshot_pressure_threshold"],
        ResetSnapshotEtaSecs => vec!["reset", "snapshot_eta_secs"],
        ResetAutoSnapshot => vec!["reset", "auto_snapshot"],
        AnomalyEnabled => vec!["anomaly", "enabled"],
        AnomalyWindowPolls => vec!["anomaly", "window_polls"],
        AnomalyMinConfidence => vec!["anomaly", "min_confidence"],
        AnomalyIdentityChurnMinFlips => vec!["anomaly", "identity_churn_min_flips"],
        AnomalyErrorBurstThreshold => vec!["anomaly", "error_burst_threshold"],
        AnomalyCacheDiscontinuityDrop => vec!["anomaly", "cache_discontinuity_drop"],
        AnomalyCrossPaneClusterMinFindings => vec!["anomaly", "cross_pane_cluster_min_findings"],
        AnomalyCostSlopeUsdPerHour => vec!["anomaly", "cost_slope_usd_per_hour"],
        AnomalyTokenSlopeInputPerPoll => vec!["anomaly", "token_slope_input_per_poll"],
        AnomalyMemoryGrowthMb => vec!["anomaly", "memory_growth_mb"],
        AnomalyRetentionDays => vec!["anomaly", "retention_days"],
        AnomalyPromoteIdentityChurn => vec!["anomaly", "promote", "identity_churn"],
        AnomalyPromoteErrorBurst => vec!["anomaly", "promote", "error_burst"],
        AnomalyPromoteCacheDiscontinuity => vec!["anomaly", "promote", "cache_discontinuity"],
        AnomalyPromoteCrossPaneEditCluster => {
            vec!["anomaly", "promote", "cross_pane_edit_cluster"]
        }
        AnomalyPromoteCostSlope => vec!["anomaly", "promote", "cost_slope"],
        AnomalyPromoteTokenSlope => vec!["anomaly", "promote", "token_slope"],
        AnomalyPromoteMemoryGrowth => vec!["anomaly", "promote", "memory_growth"],
        AnomalyPromoteSubagentSideEffect => vec!["anomaly", "promote", "subagent_side_effect"],
        CostBudgetUsd => vec!["cost", "budget_usd"],
        ProfileSwitchEnabled => vec!["profile_switch", "enabled"],
        ProfileSwitchWindowPolls => vec!["profile_switch", "window_polls"],
        ProfileSwitchErrorRateThreshold => vec!["profile_switch", "error_rate_threshold"],
        FxEnabled => vec!["fx", "enabled"],
        FxText => vec!["fx", "text"],
        FxEffect => vec!["fx", "effect"],
        FxDurationSecs => vec!["fx", "duration_secs"],
        FxHotkeyEnabled => vec!["fx", "hotkey_enabled"],
        FxCelebrationEnabled => vec!["fx", "celebration_enabled"],
        FxScreensaverEnabled => vec!["fx", "screensaver_enabled"],
        FxScreensaverIdleSecs => vec!["fx", "screensaver_idle_secs"],
    }
}

fn parameter_value_for_display(config: &QmonsterConfig, field: ParameterField) -> String {
    use ParameterField::*;
    match field {
        TmuxSource => config.tmux.source.as_str().into(),
        TmuxPollIntervalMs => format!("{}ms", config.tmux.poll_interval_ms),
        TmuxCaptureLines => config.tmux.capture_lines.to_string(),
        RefreshPolicy => refresh_policy_label(config.refresh.policy).into(),
        IdleStillnessPolls => format!("{} polls", config.idle.stillness_polls),
        LoggingSensitivity => log_sensitivity_label(config.logging.sensitivity).into(),
        LoggingRetentionDays => format!("{}d", config.logging.retention_days),
        LoggingBigOutputChars => format!("{} chars", config.logging.big_output_chars),
        StorageRoot => config.storage.root.as_deref().unwrap_or("(auto)").into(),
        ActionsMode => actions_mode_label(config.actions.mode).into(),
        ActionsAutoNotifications => on_off(config.actions.allow_auto_notifications).into(),
        ActionsAutoArchive => on_off(config.actions.allow_auto_archive).into(),
        ActionsAutoPromptSend => on_off(config.actions.allow_auto_prompt_send).into(),
        ActionsDestructive => on_off(config.actions.allow_destructive_actions).into(),
        UxConfirmActions => confirm_actions_label(config.ux.confirm_actions).into(),
        UxHoverHelp => on_off(config.ux.hover_help).into(),
        UxHoverHelpTrigger => hover_help_trigger_label(config.ux.hover_help_trigger).into(),
        UxHelpLanguage => help_language_label(config.ux.help_language).into(),
        InsightsIgnoredTtlSecs => seconds_label(config.insights.ignored_ttl_secs),
        InsightsDefaultWindowSecs => seconds_label(config.insights.default_window_secs),
        TokenQuotaTight => on_off(config.token.quota_tight).into(),
        SecurityPostureAdvisories => on_off(config.security.posture_advisories).into(),
        SecurityCrossWindowFindings => on_off(config.security.cross_window_findings).into(),
        SecurityIdentityDriftFindings => on_off(config.security.identity_drift_findings).into(),
        SecurityCrossPaneFileFindings => on_off(config.security.cross_pane_file_findings).into(),
        CacheHotRatioThreshold => pct_label(config.cache.hot_ratio_threshold),
        CacheColdRatioThreshold => pct_label(config.cache.cold_ratio_threshold),
        CacheHotLowCtxThreshold => pct_label(f64::from(config.cache.hot_low_ctx_threshold)),
        CacheColdHighCtxThreshold => pct_label(f64::from(config.cache.cold_high_ctx_threshold)),
        CacheDriftDropThreshold => pct_label(config.cache.drift_drop_threshold),
        CacheDriftMinSamples => config.cache.drift_min_samples.to_string(),
        ResetWaitPressureThreshold => pct_label(f64::from(config.reset.wait_pressure_threshold)),
        ResetWaitEtaSecs => seconds_label(config.reset.wait_eta_secs),
        ResetSnapshotPressureThreshold => {
            pct_label(f64::from(config.reset.snapshot_pressure_threshold))
        }
        ResetSnapshotEtaSecs => seconds_label(config.reset.snapshot_eta_secs),
        ResetAutoSnapshot => on_off(config.reset.auto_snapshot).into(),
        AnomalyEnabled => on_off(config.anomaly.enabled).into(),
        AnomalyWindowPolls => format!("{} polls", config.anomaly.window_polls),
        AnomalyMinConfidence => config.anomaly.min_confidence.clone(),
        AnomalyIdentityChurnMinFlips => config.anomaly.identity_churn_min_flips.to_string(),
        AnomalyErrorBurstThreshold => format!("{:.2}", config.anomaly.error_burst_threshold),
        AnomalyCacheDiscontinuityDrop => format!("{:.2}", config.anomaly.cache_discontinuity_drop),
        AnomalyCrossPaneClusterMinFindings => {
            config.anomaly.cross_pane_cluster_min_findings.to_string()
        }
        AnomalyCostSlopeUsdPerHour => {
            format!("{:.2}", config.anomaly.cost_slope_usd_per_hour)
        }
        AnomalyTokenSlopeInputPerPoll => config.anomaly.token_slope_input_per_poll.to_string(),
        AnomalyMemoryGrowthMb => format!("{:.0} MB", config.anomaly.memory_growth_mb),
        AnomalyRetentionDays => format!("{}d", config.anomaly.retention_days),
        AnomalyPromoteIdentityChurn => config.anomaly.promote.identity_churn.clone(),
        AnomalyPromoteErrorBurst => config.anomaly.promote.error_burst.clone(),
        AnomalyPromoteCacheDiscontinuity => config.anomaly.promote.cache_discontinuity.clone(),
        AnomalyPromoteCrossPaneEditCluster => {
            config.anomaly.promote.cross_pane_edit_cluster.clone()
        }
        AnomalyPromoteCostSlope => config.anomaly.promote.cost_slope.clone(),
        AnomalyPromoteTokenSlope => config.anomaly.promote.token_slope.clone(),
        AnomalyPromoteMemoryGrowth => config.anomaly.promote.memory_growth.clone(),
        AnomalyPromoteSubagentSideEffect => config.anomaly.promote.subagent_side_effect.clone(),
        CostBudgetUsd => format!("${:.2}", config.cost.budget_usd),
        ProfileSwitchEnabled => on_off(config.profile_switch.enabled).into(),
        ProfileSwitchWindowPolls => format!("{} polls", config.profile_switch.window_polls),
        ProfileSwitchErrorRateThreshold => {
            pct_label(f64::from(config.profile_switch.error_rate_threshold))
        }
        FxEnabled => on_off(config.fx.enabled).into(),
        FxText => config.fx.text.clone(),
        FxEffect => config.fx.effect.as_str().into(),
        FxDurationSecs => format!("{}s", config.fx.duration_secs),
        FxHotkeyEnabled => on_off(config.fx.hotkey_enabled).into(),
        FxCelebrationEnabled => on_off(config.fx.celebration_enabled).into(),
        FxScreensaverEnabled => on_off(config.fx.screensaver_enabled).into(),
        FxScreensaverIdleSecs => format!("{}s", config.fx.screensaver_idle_secs),
    }
}

fn parameter_value_for_edit(config: &QmonsterConfig, field: ParameterField) -> String {
    use ParameterField::*;
    match field {
        StorageRoot => config.storage.root.clone().unwrap_or_default(),
        TmuxPollIntervalMs => config.tmux.poll_interval_ms.to_string(),
        TmuxCaptureLines => config.tmux.capture_lines.to_string(),
        IdleStillnessPolls => config.idle.stillness_polls.to_string(),
        LoggingRetentionDays => config.logging.retention_days.to_string(),
        LoggingBigOutputChars => config.logging.big_output_chars.to_string(),
        InsightsIgnoredTtlSecs => config.insights.ignored_ttl_secs.to_string(),
        InsightsDefaultWindowSecs => config.insights.default_window_secs.to_string(),
        CacheHotRatioThreshold => config.cache.hot_ratio_threshold.to_string(),
        CacheColdRatioThreshold => config.cache.cold_ratio_threshold.to_string(),
        CacheHotLowCtxThreshold => config.cache.hot_low_ctx_threshold.to_string(),
        CacheColdHighCtxThreshold => config.cache.cold_high_ctx_threshold.to_string(),
        CacheDriftDropThreshold => config.cache.drift_drop_threshold.to_string(),
        CacheDriftMinSamples => config.cache.drift_min_samples.to_string(),
        ResetWaitPressureThreshold => config.reset.wait_pressure_threshold.to_string(),
        ResetWaitEtaSecs => config.reset.wait_eta_secs.to_string(),
        ResetSnapshotPressureThreshold => config.reset.snapshot_pressure_threshold.to_string(),
        ResetSnapshotEtaSecs => config.reset.snapshot_eta_secs.to_string(),
        AnomalyWindowPolls => config.anomaly.window_polls.to_string(),
        AnomalyIdentityChurnMinFlips => config.anomaly.identity_churn_min_flips.to_string(),
        AnomalyErrorBurstThreshold => config.anomaly.error_burst_threshold.to_string(),
        AnomalyCacheDiscontinuityDrop => config.anomaly.cache_discontinuity_drop.to_string(),
        AnomalyCrossPaneClusterMinFindings => {
            config.anomaly.cross_pane_cluster_min_findings.to_string()
        }
        AnomalyCostSlopeUsdPerHour => config.anomaly.cost_slope_usd_per_hour.to_string(),
        AnomalyTokenSlopeInputPerPoll => config.anomaly.token_slope_input_per_poll.to_string(),
        AnomalyMemoryGrowthMb => config.anomaly.memory_growth_mb.to_string(),
        AnomalyRetentionDays => config.anomaly.retention_days.to_string(),
        CostBudgetUsd => config.cost.budget_usd.to_string(),
        ProfileSwitchWindowPolls => config.profile_switch.window_polls.to_string(),
        ProfileSwitchErrorRateThreshold => config.profile_switch.error_rate_threshold.to_string(),
        FxText => config.fx.text.clone(),
        FxDurationSecs => config.fx.duration_secs.to_string(),
        FxScreensaverIdleSecs => config.fx.screensaver_idle_secs.to_string(),
        _ => parameter_value_for_display(config, field),
    }
}

fn parameter_default_for_display(field: ParameterField) -> String {
    parameter_value_for_display(&QmonsterConfig::defaults(), field)
}

fn confidence_next(value: &str) -> Result<String, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => Ok("medium".into()),
        "medium" => Ok("high".into()),
        "high" => Ok("low".into()),
        other => Err(format!("invalid confidence: {other}")),
    }
}

fn toggle_parameter_bool(config: &mut QmonsterConfig, field: ParameterField) {
    use ParameterField::*;
    match field {
        ActionsAutoNotifications => {
            config.actions.allow_auto_notifications = !config.actions.allow_auto_notifications;
        }
        ActionsAutoArchive => {
            config.actions.allow_auto_archive = !config.actions.allow_auto_archive;
        }
        ActionsAutoPromptSend => {
            config.actions.allow_auto_prompt_send = !config.actions.allow_auto_prompt_send;
        }
        ActionsDestructive => {
            config.actions.allow_destructive_actions = !config.actions.allow_destructive_actions;
        }
        UxHoverHelp => config.ux.hover_help = !config.ux.hover_help,
        TokenQuotaTight => config.token.quota_tight = !config.token.quota_tight,
        SecurityPostureAdvisories => {
            config.security.posture_advisories = !config.security.posture_advisories;
        }
        SecurityCrossWindowFindings => {
            config.security.cross_window_findings = !config.security.cross_window_findings;
        }
        SecurityIdentityDriftFindings => {
            config.security.identity_drift_findings = !config.security.identity_drift_findings;
        }
        SecurityCrossPaneFileFindings => {
            config.security.cross_pane_file_findings = !config.security.cross_pane_file_findings;
        }
        ResetAutoSnapshot => config.reset.auto_snapshot = !config.reset.auto_snapshot,
        AnomalyEnabled => config.anomaly.enabled = !config.anomaly.enabled,
        ProfileSwitchEnabled => config.profile_switch.enabled = !config.profile_switch.enabled,
        FxEnabled => config.fx.enabled = !config.fx.enabled,
        FxHotkeyEnabled => config.fx.hotkey_enabled = !config.fx.hotkey_enabled,
        FxCelebrationEnabled => config.fx.celebration_enabled = !config.fx.celebration_enabled,
        FxScreensaverEnabled => config.fx.screensaver_enabled = !config.fx.screensaver_enabled,
        _ => {}
    }
}

fn cycle_parameter_enum(config: &mut QmonsterConfig, field: ParameterField) -> Result<(), String> {
    use ParameterField::*;
    match field {
        TmuxSource => {
            config.tmux.source = match config.tmux.source {
                TmuxSourceMode::Auto => TmuxSourceMode::Polling,
                TmuxSourceMode::Polling => TmuxSourceMode::ControlMode,
                TmuxSourceMode::ControlMode => TmuxSourceMode::Auto,
            };
        }
        RefreshPolicy => {
            config.refresh.policy = match config.refresh.policy {
                crate::app::config::RefreshPolicy::ManualOnly => {
                    crate::app::config::RefreshPolicy::Automatic
                }
                crate::app::config::RefreshPolicy::Automatic => {
                    crate::app::config::RefreshPolicy::ManualOnly
                }
            };
        }
        LoggingSensitivity => {
            config.logging.sensitivity = match config.logging.sensitivity {
                LogSensitivity::Minimal => LogSensitivity::Balanced,
                LogSensitivity::Balanced => LogSensitivity::Forensic,
                LogSensitivity::Forensic => LogSensitivity::Minimal,
            };
        }
        ActionsMode => {
            config.actions.mode = match config.actions.mode {
                crate::app::config::ActionsMode::ObserveOnly => {
                    crate::app::config::ActionsMode::RecommendOnly
                }
                crate::app::config::ActionsMode::RecommendOnly => {
                    crate::app::config::ActionsMode::SafeAuto
                }
                crate::app::config::ActionsMode::SafeAuto => {
                    crate::app::config::ActionsMode::ObserveOnly
                }
            };
        }
        UxConfirmActions => {
            config.ux.confirm_actions = match config.ux.confirm_actions {
                ConfirmActions::Always => ConfirmActions::FirstTime,
                ConfirmActions::FirstTime => ConfirmActions::Never,
                ConfirmActions::Never => ConfirmActions::Always,
            };
        }
        UxHelpLanguage => config.ux.help_language = config.ux.help_language.toggle(),
        UxHoverHelpTrigger => {
            config.ux.hover_help_trigger = config.ux.hover_help_trigger.toggle();
        }
        AnomalyMinConfidence => {
            config.anomaly.min_confidence = confidence_next(&config.anomaly.min_confidence)?;
        }
        AnomalyPromoteIdentityChurn => {
            config.anomaly.promote.identity_churn =
                confidence_next(&config.anomaly.promote.identity_churn)?;
        }
        AnomalyPromoteErrorBurst => {
            config.anomaly.promote.error_burst =
                confidence_next(&config.anomaly.promote.error_burst)?;
        }
        AnomalyPromoteCacheDiscontinuity => {
            config.anomaly.promote.cache_discontinuity =
                confidence_next(&config.anomaly.promote.cache_discontinuity)?;
        }
        AnomalyPromoteCrossPaneEditCluster => {
            config.anomaly.promote.cross_pane_edit_cluster =
                confidence_next(&config.anomaly.promote.cross_pane_edit_cluster)?;
        }
        AnomalyPromoteCostSlope => {
            config.anomaly.promote.cost_slope =
                confidence_next(&config.anomaly.promote.cost_slope)?;
        }
        AnomalyPromoteTokenSlope => {
            config.anomaly.promote.token_slope =
                confidence_next(&config.anomaly.promote.token_slope)?;
        }
        AnomalyPromoteMemoryGrowth => {
            config.anomaly.promote.memory_growth =
                confidence_next(&config.anomaly.promote.memory_growth)?;
        }
        AnomalyPromoteSubagentSideEffect => {
            config.anomaly.promote.subagent_side_effect =
                confidence_next(&config.anomaly.promote.subagent_side_effect)?;
        }
        FxEffect => config.fx.effect = config.fx.effect.cycle(),
        _ => {}
    }
    Ok(())
}

fn parse_nonnegative_f64(label: &str, raw: &str) -> Result<f64, String> {
    let value: f64 = raw
        .parse()
        .map_err(|_| format!("{label}: not a number: {raw}"))?;
    if !value.is_finite() || value < 0.0 {
        return Err(format!("{label}: must be a finite value >= 0"));
    }
    Ok(value)
}

fn parse_unit_f32(label: &str, raw: &str) -> Result<f32, String> {
    let value: f32 = raw
        .parse()
        .map_err(|_| format!("{label}: not a number: {raw}"))?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(format!("{label}: must be in [0.0, 1.0]"));
    }
    Ok(value)
}

fn parse_u64_value(label: &str, raw: &str, min: u64) -> Result<u64, String> {
    let value: u64 = raw
        .parse()
        .map_err(|_| format!("{label}: must be an integer"))?;
    if value < min {
        return Err(format!("{label}: must be >= {min}"));
    }
    Ok(value)
}

fn parse_usize_value(label: &str, raw: &str, min: usize) -> Result<usize, String> {
    let value: usize = raw
        .parse()
        .map_err(|_| format!("{label}: must be an integer"))?;
    if value < min {
        return Err(format!("{label}: must be >= {min}"));
    }
    Ok(value)
}

fn apply_parameter_edit(
    config: &mut QmonsterConfig,
    field: ParameterField,
    raw: &str,
) -> Result<(), String> {
    use ParameterField::*;
    let label = parameter_label(field);
    match field {
        StorageRoot => {
            let trimmed = raw.trim();
            config.storage.root = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        TmuxPollIntervalMs => config.tmux.poll_interval_ms = parse_u64_value(label, raw, 1)?,
        TmuxCaptureLines => config.tmux.capture_lines = parse_usize_value(label, raw, 1)?,
        IdleStillnessPolls => config.idle.stillness_polls = parse_usize_value(label, raw, 1)?,
        LoggingRetentionDays => config.logging.retention_days = parse_u64_value(label, raw, 1)?,
        LoggingBigOutputChars => {
            config.logging.big_output_chars = parse_usize_value(label, raw, 1)?;
        }
        InsightsIgnoredTtlSecs => {
            config.insights.ignored_ttl_secs = parse_u64_value(label, raw, 0)?;
        }
        InsightsDefaultWindowSecs => {
            config.insights.default_window_secs = parse_u64_value(label, raw, 1)?;
        }
        CacheHotRatioThreshold => {
            config.cache.hot_ratio_threshold = f64::from(parse_unit_f32(label, raw)?);
        }
        CacheColdRatioThreshold => {
            config.cache.cold_ratio_threshold = f64::from(parse_unit_f32(label, raw)?);
        }
        CacheHotLowCtxThreshold => config.cache.hot_low_ctx_threshold = parse_unit_f32(label, raw)?,
        CacheColdHighCtxThreshold => {
            config.cache.cold_high_ctx_threshold = parse_unit_f32(label, raw)?;
        }
        CacheDriftDropThreshold => {
            config.cache.drift_drop_threshold = f64::from(parse_unit_f32(label, raw)?);
        }
        CacheDriftMinSamples => {
            config.cache.drift_min_samples = parse_usize_value(label, raw, 1)?;
        }
        ResetWaitPressureThreshold => {
            config.reset.wait_pressure_threshold = parse_unit_f32(label, raw)?;
        }
        ResetWaitEtaSecs => config.reset.wait_eta_secs = parse_u64_value(label, raw, 0)?,
        ResetSnapshotPressureThreshold => {
            config.reset.snapshot_pressure_threshold = parse_unit_f32(label, raw)?;
        }
        ResetSnapshotEtaSecs => {
            config.reset.snapshot_eta_secs = parse_u64_value(label, raw, 0)?;
        }
        AnomalyWindowPolls => config.anomaly.window_polls = parse_usize_value(label, raw, 1)?,
        AnomalyIdentityChurnMinFlips => {
            config.anomaly.identity_churn_min_flips = parse_usize_value(label, raw, 1)?;
        }
        AnomalyErrorBurstThreshold => {
            config.anomaly.error_burst_threshold = parse_unit_f32(label, raw)?;
        }
        AnomalyCacheDiscontinuityDrop => {
            config.anomaly.cache_discontinuity_drop = parse_unit_f32(label, raw)?;
        }
        AnomalyCrossPaneClusterMinFindings => {
            config.anomaly.cross_pane_cluster_min_findings = parse_usize_value(label, raw, 1)?;
        }
        AnomalyCostSlopeUsdPerHour => {
            config.anomaly.cost_slope_usd_per_hour = parse_nonnegative_f64(label, raw)?;
        }
        AnomalyTokenSlopeInputPerPoll => {
            config.anomaly.token_slope_input_per_poll = parse_u64_value(label, raw, 0)?;
        }
        AnomalyMemoryGrowthMb => {
            config.anomaly.memory_growth_mb = parse_nonnegative_f64(label, raw)?;
        }
        AnomalyRetentionDays => config.anomaly.retention_days = parse_u64_value(label, raw, 1)?,
        CostBudgetUsd => config.cost.budget_usd = parse_nonnegative_f64(label, raw)?,
        ProfileSwitchWindowPolls => {
            config.profile_switch.window_polls = parse_usize_value(label, raw, 1)?;
        }
        ProfileSwitchErrorRateThreshold => {
            config.profile_switch.error_rate_threshold = parse_unit_f32(label, raw)?;
        }
        FxText => {
            // Banner free text — accept any printable ASCII; renderer
            // upper-cases for the block-letter lookup.
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(format!("{label}: must not be empty"));
            }
            config.fx.text = trimmed.to_string();
        }
        FxDurationSecs => {
            config.fx.duration_secs = parse_u64_value(label, raw, 0)? as u32;
        }
        FxScreensaverIdleSecs => {
            // 60s minimum — anything shorter would chew through cache
            // every iteration and is almost certainly a typo.
            config.fx.screensaver_idle_secs = parse_u64_value(label, raw, 60)? as u32;
        }
        _ => return Err(format!("{label}: use Space/e to toggle or cycle")),
    }
    Ok(())
}

fn merge_parameter_field(
    doc: &mut toml_edit::DocumentMut,
    config: &QmonsterConfig,
    field: ParameterField,
) {
    use ParameterField::*;
    let path = parameter_toml_path(field);
    match field {
        TmuxSource => set_nested_str(doc, &path, config.tmux.source.as_str()),
        TmuxPollIntervalMs => set_nested_i64(doc, &path, config.tmux.poll_interval_ms as i64),
        TmuxCaptureLines => set_nested_i64(doc, &path, config.tmux.capture_lines as i64),
        RefreshPolicy => set_nested_str(doc, &path, refresh_policy_label(config.refresh.policy)),
        IdleStillnessPolls => set_nested_i64(doc, &path, config.idle.stillness_polls as i64),
        LoggingSensitivity => {
            set_nested_str(
                doc,
                &path,
                log_sensitivity_label(config.logging.sensitivity),
            );
        }
        LoggingRetentionDays => set_nested_i64(doc, &path, config.logging.retention_days as i64),
        LoggingBigOutputChars => set_nested_i64(doc, &path, config.logging.big_output_chars as i64),
        StorageRoot => match config.storage.root.as_deref() {
            Some(root) if !root.is_empty() => set_nested_str(doc, &path, root),
            _ => remove_nested(doc, &path),
        },
        ActionsMode => set_nested_str(doc, &path, actions_mode_label(config.actions.mode)),
        ActionsAutoNotifications => {
            set_nested_bool(doc, &path, config.actions.allow_auto_notifications);
        }
        ActionsAutoArchive => set_nested_bool(doc, &path, config.actions.allow_auto_archive),
        ActionsAutoPromptSend => set_nested_bool(doc, &path, config.actions.allow_auto_prompt_send),
        ActionsDestructive => set_nested_bool(doc, &path, config.actions.allow_destructive_actions),
        UxConfirmActions => {
            set_nested_str(doc, &path, confirm_actions_label(config.ux.confirm_actions))
        }
        UxHoverHelp => set_nested_bool(doc, &path, config.ux.hover_help),
        UxHoverHelpTrigger => {
            set_nested_str(
                doc,
                &path,
                hover_help_trigger_label(config.ux.hover_help_trigger),
            );
        }
        UxHelpLanguage => set_nested_str(doc, &path, help_language_label(config.ux.help_language)),
        InsightsIgnoredTtlSecs => {
            set_nested_i64(doc, &path, config.insights.ignored_ttl_secs as i64)
        }
        InsightsDefaultWindowSecs => {
            set_nested_i64(doc, &path, config.insights.default_window_secs as i64);
        }
        TokenQuotaTight => set_nested_bool(doc, &path, config.token.quota_tight),
        SecurityPostureAdvisories => {
            set_nested_bool(doc, &path, config.security.posture_advisories);
        }
        SecurityCrossWindowFindings => {
            set_nested_bool(doc, &path, config.security.cross_window_findings);
        }
        SecurityIdentityDriftFindings => {
            set_nested_bool(doc, &path, config.security.identity_drift_findings);
        }
        SecurityCrossPaneFileFindings => {
            set_nested_bool(doc, &path, config.security.cross_pane_file_findings);
        }
        CacheHotRatioThreshold => set_nested_f64(doc, &path, config.cache.hot_ratio_threshold),
        CacheColdRatioThreshold => set_nested_f64(doc, &path, config.cache.cold_ratio_threshold),
        CacheHotLowCtxThreshold => {
            set_nested_f64(doc, &path, f64::from(config.cache.hot_low_ctx_threshold));
        }
        CacheColdHighCtxThreshold => {
            set_nested_f64(doc, &path, f64::from(config.cache.cold_high_ctx_threshold));
        }
        CacheDriftDropThreshold => set_nested_f64(doc, &path, config.cache.drift_drop_threshold),
        CacheDriftMinSamples => set_nested_i64(doc, &path, config.cache.drift_min_samples as i64),
        ResetWaitPressureThreshold => {
            set_nested_f64(doc, &path, f64::from(config.reset.wait_pressure_threshold));
        }
        ResetWaitEtaSecs => set_nested_i64(doc, &path, config.reset.wait_eta_secs as i64),
        ResetSnapshotPressureThreshold => {
            set_nested_f64(
                doc,
                &path,
                f64::from(config.reset.snapshot_pressure_threshold),
            );
        }
        ResetSnapshotEtaSecs => set_nested_i64(doc, &path, config.reset.snapshot_eta_secs as i64),
        ResetAutoSnapshot => set_nested_bool(doc, &path, config.reset.auto_snapshot),
        AnomalyEnabled => set_nested_bool(doc, &path, config.anomaly.enabled),
        AnomalyWindowPolls => set_nested_i64(doc, &path, config.anomaly.window_polls as i64),
        AnomalyMinConfidence => set_nested_str(doc, &path, &config.anomaly.min_confidence),
        AnomalyIdentityChurnMinFlips => {
            set_nested_i64(doc, &path, config.anomaly.identity_churn_min_flips as i64);
        }
        AnomalyErrorBurstThreshold => {
            set_nested_f64(doc, &path, f64::from(config.anomaly.error_burst_threshold));
        }
        AnomalyCacheDiscontinuityDrop => {
            set_nested_f64(
                doc,
                &path,
                f64::from(config.anomaly.cache_discontinuity_drop),
            );
        }
        AnomalyCrossPaneClusterMinFindings => {
            set_nested_i64(
                doc,
                &path,
                config.anomaly.cross_pane_cluster_min_findings as i64,
            );
        }
        AnomalyCostSlopeUsdPerHour => {
            set_nested_f64(doc, &path, config.anomaly.cost_slope_usd_per_hour);
        }
        AnomalyTokenSlopeInputPerPoll => {
            set_nested_i64(doc, &path, config.anomaly.token_slope_input_per_poll as i64);
        }
        AnomalyMemoryGrowthMb => set_nested_f64(doc, &path, config.anomaly.memory_growth_mb),
        AnomalyRetentionDays => set_nested_i64(doc, &path, config.anomaly.retention_days as i64),
        AnomalyPromoteIdentityChurn => {
            set_nested_str(doc, &path, &config.anomaly.promote.identity_churn);
        }
        AnomalyPromoteErrorBurst => {
            set_nested_str(doc, &path, &config.anomaly.promote.error_burst);
        }
        AnomalyPromoteCacheDiscontinuity => {
            set_nested_str(doc, &path, &config.anomaly.promote.cache_discontinuity);
        }
        AnomalyPromoteCrossPaneEditCluster => {
            set_nested_str(doc, &path, &config.anomaly.promote.cross_pane_edit_cluster);
        }
        AnomalyPromoteCostSlope => {
            set_nested_str(doc, &path, &config.anomaly.promote.cost_slope);
        }
        AnomalyPromoteTokenSlope => {
            set_nested_str(doc, &path, &config.anomaly.promote.token_slope);
        }
        AnomalyPromoteMemoryGrowth => {
            set_nested_str(doc, &path, &config.anomaly.promote.memory_growth);
        }
        AnomalyPromoteSubagentSideEffect => {
            set_nested_str(doc, &path, &config.anomaly.promote.subagent_side_effect);
        }
        CostBudgetUsd => set_nested_f64(doc, &path, config.cost.budget_usd),
        ProfileSwitchEnabled => set_nested_bool(doc, &path, config.profile_switch.enabled),
        ProfileSwitchWindowPolls => {
            set_nested_i64(doc, &path, config.profile_switch.window_polls as i64);
        }
        ProfileSwitchErrorRateThreshold => {
            set_nested_f64(
                doc,
                &path,
                f64::from(config.profile_switch.error_rate_threshold),
            );
        }
        FxEnabled => set_nested_bool(doc, &path, config.fx.enabled),
        FxText => set_nested_str(doc, &path, &config.fx.text),
        FxEffect => set_nested_str(doc, &path, config.fx.effect.as_str()),
        FxDurationSecs => set_nested_i64(doc, &path, config.fx.duration_secs as i64),
        FxHotkeyEnabled => set_nested_bool(doc, &path, config.fx.hotkey_enabled),
        FxCelebrationEnabled => set_nested_bool(doc, &path, config.fx.celebration_enabled),
        FxScreensaverEnabled => set_nested_bool(doc, &path, config.fx.screensaver_enabled),
        FxScreensaverIdleSecs => {
            set_nested_i64(doc, &path, config.fx.screensaver_idle_secs as i64);
        }
    }
}

// -----------------------------------------------------------------
// Field-by-field read / write / validate helpers. Internal to the
// module; tests reach in via the public state-machine entry points.
// -----------------------------------------------------------------

fn read_cost(config: &QmonsterConfig, field: FieldId) -> Option<f64> {
    let cost = &config.cost;
    match (field.scope, field.bound) {
        (Scope::Default, Bound::Warning) => Some(cost.warning_usd),
        (Scope::Default, Bound::Critical) => Some(cost.critical_usd),
        (scope, bound) => {
            let provider = match scope {
                Scope::Claude => cost.claude.as_ref(),
                Scope::Codex => cost.codex.as_ref(),
                Scope::Gemini => cost.gemini.as_ref(),
                Scope::Claude5h | Scope::ClaudeWeekly | Scope::Codex5h | Scope::CodexWeekly => None,
                Scope::Default => None,
            }?;
            Some(match bound {
                Bound::Warning => provider.warning_usd,
                Bound::Critical => provider.critical_usd,
            })
        }
    }
}

fn read_pct<C: PctConfig>(config: &C, field: FieldId) -> Option<f32> {
    match (field.scope, field.bound) {
        (Scope::Default, Bound::Warning) => Some(config.warning_pct()),
        (Scope::Default, Bound::Critical) => Some(config.critical_pct()),
        (scope, bound) => {
            let provider = config.provider_override(scope)?;
            Some(match bound {
                Bound::Warning => provider.warning_pct,
                Bound::Critical => provider.critical_pct,
            })
        }
    }
}

trait PctConfig {
    fn warning_pct(&self) -> f32;
    fn critical_pct(&self) -> f32;
    fn provider_override(&self, scope: Scope) -> Option<&PressureProviderConfig>;
}

impl PctConfig for ContextConfig {
    fn warning_pct(&self) -> f32 {
        self.warning_pct
    }
    fn critical_pct(&self) -> f32 {
        self.critical_pct
    }
    fn provider_override(&self, scope: Scope) -> Option<&PressureProviderConfig> {
        match scope {
            Scope::Claude => self.claude.as_ref(),
            Scope::Codex => self.codex.as_ref(),
            Scope::Gemini => self.gemini.as_ref(),
            Scope::Claude5h | Scope::ClaudeWeekly | Scope::Codex5h | Scope::CodexWeekly => None,
            Scope::Default => None,
        }
    }
}

impl PctConfig for QuotaConfig {
    fn warning_pct(&self) -> f32 {
        self.warning_pct
    }
    fn critical_pct(&self) -> f32 {
        self.critical_pct
    }
    fn provider_override(&self, scope: Scope) -> Option<&PressureProviderConfig> {
        match scope {
            Scope::Claude | Scope::Claude5h => self.claude_5h.as_ref().or(self.claude.as_ref()),
            Scope::ClaudeWeekly => self.claude_weekly.as_ref().or(self.claude.as_ref()),
            Scope::Codex | Scope::Codex5h => self.codex_5h.as_ref().or(self.codex.as_ref()),
            Scope::CodexWeekly => self.codex_weekly.as_ref().or(self.codex.as_ref()),
            Scope::Gemini => self.gemini.as_ref(),
            Scope::Default => None,
        }
    }
}

fn validate_field_value(field: FieldId, value: f64, config: &QmonsterConfig) -> Result<(), String> {
    if !value.is_finite() {
        return Err("not a finite number".into());
    }
    match field.section {
        Section::Cost => {
            if value < 0.0 {
                return Err(format!("cost ({:.2}) must be ≥ 0", value));
            }
        }
        Section::Context | Section::Quota => {
            if !(0.0..=1.0).contains(&value) {
                return Err(format!("pct ({:.4}) must be in [0.0, 1.0]", value));
            }
        }
    }
    // Pair invariant: warning < critical on the same (section, scope)
    // row. We compute the would-be pair (current value of the partner
    // field, but with our edit applied to our slot) and reject if the
    // resulting warning is not strictly less than the resulting critical.
    let partner_bound = match field.bound {
        Bound::Warning => Bound::Critical,
        Bound::Critical => Bound::Warning,
    };
    let partner_field = FieldId::new(field.section, field.scope, partner_bound);
    let partner_value = effective_value(config, partner_field);
    let (warn, crit) = match field.bound {
        Bound::Warning => (value, partner_value),
        Bound::Critical => (partner_value, value),
    };
    if warn >= crit {
        return Err(format!(
            "warning ({:.4}) must be < critical ({:.4})",
            warn, crit
        ));
    }
    Ok(())
}

fn effective_value(config: &QmonsterConfig, field: FieldId) -> f64 {
    let direct = match field.section {
        Section::Cost => read_cost(config, field),
        Section::Context => read_pct(&config.context, field).map(f64::from),
        Section::Quota => read_pct(&config.quota, field).map(f64::from),
    };
    if let Some(v) = direct {
        return v;
    }
    let default_field = FieldId::new(field.section, Scope::Default, field.bound);
    match field.section {
        Section::Cost => read_cost(config, default_field).unwrap_or(0.0),
        Section::Context => read_pct(&config.context, default_field)
            .map(f64::from)
            .unwrap_or(0.0),
        Section::Quota => read_pct(&config.quota, default_field)
            .map(f64::from)
            .unwrap_or(0.0),
    }
}

fn apply_field_value(config: &mut QmonsterConfig, field: FieldId, value: f64) {
    match field.section {
        Section::Cost => apply_cost(&mut config.cost, field, value),
        Section::Context => apply_context(&mut config.context, field, value as f32),
        Section::Quota => apply_quota(&mut config.quota, field, value as f32),
    }
}

fn apply_cost(cost: &mut CostConfig, field: FieldId, value: f64) {
    match field.scope {
        Scope::Default => match field.bound {
            Bound::Warning => cost.warning_usd = value,
            Bound::Critical => cost.critical_usd = value,
        },
        Scope::Claude => apply_cost_provider(
            &mut cost.claude,
            cost.warning_usd,
            cost.critical_usd,
            field.bound,
            value,
        ),
        Scope::Codex => apply_cost_provider(
            &mut cost.codex,
            cost.warning_usd,
            cost.critical_usd,
            field.bound,
            value,
        ),
        Scope::Gemini => apply_cost_provider(
            &mut cost.gemini,
            cost.warning_usd,
            cost.critical_usd,
            field.bound,
            value,
        ),
        Scope::Claude5h | Scope::ClaudeWeekly | Scope::Codex5h | Scope::CodexWeekly => {}
    }
}

fn apply_cost_provider(
    slot: &mut Option<CostProviderConfig>,
    fallback_warn: f64,
    fallback_crit: f64,
    bound: Bound,
    value: f64,
) {
    let entry = slot.get_or_insert_with(|| CostProviderConfig {
        warning_usd: fallback_warn,
        critical_usd: fallback_crit,
    });
    match bound {
        Bound::Warning => entry.warning_usd = value,
        Bound::Critical => entry.critical_usd = value,
    }
}

fn apply_context(context: &mut ContextConfig, field: FieldId, value: f32) {
    match field.scope {
        Scope::Default => match field.bound {
            Bound::Warning => context.warning_pct = value,
            Bound::Critical => context.critical_pct = value,
        },
        Scope::Claude => apply_pct_provider(
            &mut context.claude,
            context.warning_pct,
            context.critical_pct,
            field.bound,
            value,
        ),
        Scope::Codex => apply_pct_provider(
            &mut context.codex,
            context.warning_pct,
            context.critical_pct,
            field.bound,
            value,
        ),
        Scope::Gemini => apply_pct_provider(
            &mut context.gemini,
            context.warning_pct,
            context.critical_pct,
            field.bound,
            value,
        ),
        Scope::Claude5h | Scope::ClaudeWeekly | Scope::Codex5h | Scope::CodexWeekly => {}
    }
}

fn apply_quota(quota: &mut QuotaConfig, field: FieldId, value: f32) {
    match field.scope {
        Scope::Default => match field.bound {
            Bound::Warning => quota.warning_pct = value,
            Bound::Critical => quota.critical_pct = value,
        },
        Scope::Claude => apply_pct_provider(
            &mut quota.claude_5h,
            quota.warning_pct,
            quota.critical_pct,
            field.bound,
            value,
        ),
        Scope::Claude5h => apply_pct_provider(
            &mut quota.claude_5h,
            quota.warning_pct,
            quota.critical_pct,
            field.bound,
            value,
        ),
        Scope::ClaudeWeekly => apply_pct_provider(
            &mut quota.claude_weekly,
            quota.warning_pct,
            quota.critical_pct,
            field.bound,
            value,
        ),
        Scope::Codex => apply_pct_provider(
            &mut quota.codex_5h,
            quota.warning_pct,
            quota.critical_pct,
            field.bound,
            value,
        ),
        Scope::Codex5h => apply_pct_provider(
            &mut quota.codex_5h,
            quota.warning_pct,
            quota.critical_pct,
            field.bound,
            value,
        ),
        Scope::CodexWeekly => apply_pct_provider(
            &mut quota.codex_weekly,
            quota.warning_pct,
            quota.critical_pct,
            field.bound,
            value,
        ),
        Scope::Gemini => apply_pct_provider(
            &mut quota.gemini,
            quota.warning_pct,
            quota.critical_pct,
            field.bound,
            value,
        ),
    }
}

fn apply_pct_provider(
    slot: &mut Option<PressureProviderConfig>,
    fallback_warn: f32,
    fallback_crit: f32,
    bound: Bound,
    value: f32,
) {
    let entry = slot.get_or_insert(PressureProviderConfig {
        warning_pct: fallback_warn,
        critical_pct: fallback_crit,
    });
    match bound {
        Bound::Warning => entry.warning_pct = value,
        Bound::Critical => entry.critical_pct = value,
    }
}

fn clear_provider_override(config: &mut QmonsterConfig, section: Section, scope: Scope) {
    match section {
        Section::Cost => {
            let cost = &mut config.cost;
            match scope {
                Scope::Claude => cost.claude = None,
                Scope::Codex => cost.codex = None,
                Scope::Gemini => cost.gemini = None,
                Scope::Claude5h | Scope::ClaudeWeekly | Scope::Codex5h | Scope::CodexWeekly => {}
                Scope::Default => {}
            }
        }
        Section::Context => {
            let ctx = &mut config.context;
            match scope {
                Scope::Claude => ctx.claude = None,
                Scope::Codex => ctx.codex = None,
                Scope::Gemini => ctx.gemini = None,
                Scope::Claude5h | Scope::ClaudeWeekly | Scope::Codex5h | Scope::CodexWeekly => {}
                Scope::Default => {}
            }
        }
        Section::Quota => {
            let q = &mut config.quota;
            match scope {
                Scope::Claude => q.claude_5h = None,
                Scope::Claude5h => q.claude_5h = None,
                Scope::ClaudeWeekly => q.claude_weekly = None,
                Scope::Codex => q.codex_5h = None,
                Scope::Codex5h => q.codex_5h = None,
                Scope::CodexWeekly => q.codex_weekly = None,
                Scope::Gemini => q.gemini = None,
                Scope::Default => {}
            }
        }
    }
}

fn validate_full_config(config: &QmonsterConfig) -> Result<(), String> {
    for field in all_fields() {
        if field.bound != Bound::Warning {
            continue;
        }
        // Effective pair must satisfy warning < critical.
        let warn = effective_value(config, field);
        let crit_field = FieldId::new(field.section, field.scope, Bound::Critical);
        let crit = effective_value(config, crit_field);
        if warn >= crit {
            return Err(format!(
                "{} {}: warning ({:.4}) must be < critical ({:.4})",
                describe_section(field.section),
                describe_scope(field.scope),
                warn,
                crit
            ));
        }
        match field.section {
            Section::Cost => {
                if warn < 0.0 || crit < 0.0 {
                    return Err(format!(
                        "{} {}: cost values must be ≥ 0",
                        describe_section(field.section),
                        describe_scope(field.scope)
                    ));
                }
            }
            Section::Context | Section::Quota => {
                if !(0.0..=1.0).contains(&warn) || !(0.0..=1.0).contains(&crit) {
                    return Err(format!(
                        "{} {}: pct values must be in [0.0, 1.0]",
                        describe_section(field.section),
                        describe_scope(field.scope)
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn describe_section(section: Section) -> &'static str {
    match section {
        Section::Cost => "cost",
        Section::Context => "context",
        Section::Quota => "quota",
    }
}

pub fn describe_scope(scope: Scope) -> &'static str {
    match scope {
        Scope::Default => "default",
        Scope::Claude => "claude",
        Scope::Claude5h => "claude 5h",
        Scope::ClaudeWeekly => "claude weekly",
        Scope::Codex => "codex",
        Scope::Codex5h => "codex 5h",
        Scope::CodexWeekly => "codex weekly",
        Scope::Gemini => "gemini",
    }
}

pub fn describe_bound(bound: Bound) -> &'static str {
    match bound {
        Bound::Warning => "warning",
        Bound::Critical => "critical",
    }
}

fn format_value_for_edit(value: f64, section: Section) -> String {
    match section {
        Section::Cost => format!("{value}"),
        // Pct values often need 2-3 decimals — start with what the
        // operator probably wants to refine, not lossy "0".
        Section::Context | Section::Quota => format!("{value}"),
    }
}

fn format_value_for_display(value: f64, section: Section) -> String {
    match section {
        Section::Cost => format!("{:>7.2}", value),
        Section::Context | Section::Quota => format!("{:>7.4}", value),
    }
}

// -----------------------------------------------------------------
// Rendering. The overlay is a centered modal with three sections
// (cost / context / quota) stacked vertically. Each section header
// names the metric and unit, and four rows below carry the
// each scope's warning/critical cells.
// -----------------------------------------------------------------

const MODAL_WIDTH_PERCENT: u16 = 76;
const MODAL_HEIGHT_PERCENT: u16 = 80;

pub struct SettingsModalRects {
    pub area: Rect,
    pub tabs: Rect,
    pub body: Rect,
    pub hint: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsParameterSidePanelRects {
    pub list: Rect,
    pub help: Rect,
}

pub fn settings_modal_rects(viewport: Rect) -> SettingsModalRects {
    let area = centered_rect(MODAL_WIDTH_PERCENT, MODAL_HEIGHT_PERCENT, viewport);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(area);
    SettingsModalRects {
        area,
        tabs: chunks[0],
        body: chunks[1],
        hint: chunks[2],
    }
}

pub fn settings_visible_body_rows(viewport: Rect) -> u16 {
    settings_modal_rects(viewport).body.height.saturating_sub(2)
}

pub fn settings_parameter_side_panel_rects(body: Rect) -> Option<SettingsParameterSidePanelRects> {
    if body.width < 112 || body.height < 12 {
        return None;
    }
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Length(1),
            Constraint::Min(42),
        ])
        .split(body);
    Some(SettingsParameterSidePanelRects {
        list: chunks[0],
        help: chunks[2],
    })
}

fn settings_parameter_list_rect(viewport: Rect) -> Rect {
    let body = settings_modal_rects(viewport).body;
    settings_parameter_side_panel_rects(body)
        .map(|rects| rects.list)
        .unwrap_or(body)
}

pub fn settings_parameter_list_rect_for_viewport(viewport: Rect) -> Rect {
    settings_parameter_list_rect(viewport)
}

fn settings_parameter_visible_rows(viewport: Rect) -> u16 {
    settings_parameter_list_rect(viewport)
        .height
        .saturating_sub(2)
}

pub fn settings_parameter_visible_rows_for_viewport(viewport: Rect) -> u16 {
    settings_parameter_visible_rows(viewport)
}

pub fn settings_max_scroll(
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
    viewport: Rect,
) -> u16 {
    let visible = if overlay.tab() == SettingsTab::Parameters {
        settings_parameter_visible_rows(viewport) as usize
    } else {
        settings_visible_body_rows(viewport) as usize
    };
    let line_count = if overlay.tab() == SettingsTab::Parameters
        && settings_parameter_side_panel_rects(settings_modal_rects(viewport).body).is_some()
    {
        build_parameter_list_lines(overlay, config).len()
    } else {
        build_body_lines(overlay, config).len()
    };
    line_count.saturating_sub(visible).min(u16::MAX as usize) as u16
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup[1])[1]
}

/// Render the settings overlay as a modal panel. Caller is the
/// dashboard renderer; it reads the latest `QmonsterConfig` from
/// `&Context` so the overlay always shows the up-to-date values
/// (in-flight edits are reflected immediately because `commit_edit`
/// mutates `&mut config`).
pub fn render_settings_modal(
    frame: &mut Frame<'_>,
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
) {
    let rects = settings_modal_rects(frame.area());
    frame.render_widget(Clear, rects.area);

    // v1.51.0: custom tab strip with a `║` divider between the
    // editable group (Thresholds / Integrations / Parameters) and the
    // reference group (Rules / Badges). The single-`│` divider stays
    // between same-group tabs so the inner spacing matches the prior
    // ratatui Tabs widget; only the inter-group seam is highlighted.
    let tab_block = Block::default()
        .title("Settings")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));
    let tab_inner = tab_block.inner(rects.tabs);
    frame.render_widget(tab_block, rects.tabs);
    let tab_line = settings_tab_strip_line(overlay.tab());
    frame.render_widget(Paragraph::new(tab_line), tab_inner);

    let scroll = overlay
        .scroll_offset()
        .min(settings_max_scroll(overlay, config, frame.area()));
    if overlay.tab() == SettingsTab::Parameters
        && let Some(parameter_rects) = settings_parameter_side_panel_rects(rects.body)
    {
        frame.render_widget(
            Paragraph::new(build_parameter_list_lines(overlay, config))
                .scroll((scroll, 0))
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .title(Span::styled(
                            settings_body_title(overlay.tab()),
                            Style::default().add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::BORDER_ACTIVE)),
                ),
            parameter_rects.list,
        );
        frame.render_widget(
            Paragraph::new(build_parameter_help_panel_lines(overlay, config))
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .title(Span::styled(
                            " Parameter help ",
                            Style::default().add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::BORDER_ACTIVE)),
                ),
            parameter_rects.help,
        );
    } else {
        frame.render_widget(
            Paragraph::new(build_body_lines(overlay, config))
                .scroll((scroll, 0))
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .title(Span::styled(
                            settings_body_title(overlay.tab()),
                            Style::default().add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::BORDER_ACTIVE)),
                ),
            rects.body,
        );
    }

    // Mouse-clickable close button at the top-right of the body, mirroring
    // the help / git / target overlays. The companion click handler in the
    // main event loop hit-tests the same rectangle via
    // `settings_close_button_rect()`.
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        settings_close_button_rect(rects.body),
    );

    frame.render_widget(
        Paragraph::new(hint_lines_with_scroll(
            overlay,
            Some((scroll, settings_max_scroll(overlay, config, frame.area()))),
        ))
        .style(Style::default().fg(theme::TEXT_DIM))
        .wrap(Wrap { trim: false }),
        rects.hint,
    );
}

/// Hit-test rectangle for the modal's `[x]` close button. Pure function
/// so the event loop can ask "did this click land on close?" without
/// knowing how the modal renders. Mirrors `dashboard::close_button_rect`
/// — kept local so the settings module is self-contained.
pub fn settings_close_button_rect(body: Rect) -> Rect {
    Rect::new(
        body.x + body.width.saturating_sub(4),
        body.y,
        3.min(body.width),
        1.min(body.height),
    )
}

pub fn settings_tab_index_at(tabs: Rect, column: u16) -> Option<usize> {
    if column < tabs.x {
        return None;
    }
    // v1.51.0: layout mirrors `settings_tab_strip_line` — one border
    // cell, then ` label ` per tab, with `│` between same-group tabs
    // and `║` between the editable and reference groups. Empty space
    // to the right of the final label is intentionally not clickable.
    let mut cursor = tabs.x.saturating_add(1);
    for (idx, tab) in SETTINGS_TABS.iter().enumerate() {
        let label_width = settings_numbered_label(*tab).chars().count() as u16;
        let end = cursor.saturating_add(label_width).saturating_add(2);
        if column < end {
            return Some(idx);
        }
        cursor = end;
        if idx + 1 < SETTINGS_TABS.len() {
            cursor = cursor.saturating_add(1);
            if column < cursor {
                return None;
            }
        }
    }
    None
}

/// v1.51.0: numbered tab label so the `1`-`5` keyboard shortcuts are
/// visible inside the tab strip itself.
pub(crate) fn settings_numbered_label(tab: SettingsTab) -> String {
    let n = tab.index() + 1;
    format!("{n} {}", tab.label())
}

/// v1.51.0: build the styled tab strip with explicit dividers — `│`
/// between same-group tabs, `║` between the editable group
/// (Thresholds/Integrations/Parameters) and the reference group
/// (Rules/Badges). Replaces ratatui's `Tabs` widget so we can vary the
/// inter-tab divider per position.
pub(crate) fn settings_tab_strip_line(active: SettingsTab) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (idx, tab) in SETTINGS_TABS.iter().enumerate() {
        let label = format!(" {} ", settings_numbered_label(*tab));
        let style = if *tab == active {
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_DIM)
        };
        spans.push(Span::styled(label, style));
        if idx + 1 < SETTINGS_TABS.len() {
            // Inter-group seam fires when the current tab is the last
            // editable tab and the next tab is the first reference tab.
            let next = SETTINGS_TABS[idx + 1];
            let seam = if tab.is_editable() != next.is_editable() {
                "║"
            } else {
                "│"
            };
            spans.push(Span::styled(seam, Style::default().fg(theme::TEXT_DIM)));
        }
    }
    Line::from(spans)
}

pub fn settings_field_at(body: Rect, column: u16, row: u16) -> Option<FieldId> {
    settings_field_at_with_scroll(body, column, row, 0)
}

pub fn settings_field_at_with_scroll(
    body: Rect,
    column: u16,
    row: u16,
    scroll_offset: u16,
) -> Option<FieldId> {
    let inner = body.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    if !rect_contains(inner, column, row) {
        return None;
    }
    let mut line_idx = row.saturating_sub(inner.y).saturating_add(scroll_offset);
    let rel_col = column.saturating_sub(inner.x) as usize;
    for (section_idx, section) in [Section::Cost, Section::Context, Section::Quota]
        .into_iter()
        .enumerate()
    {
        if line_idx == 0 {
            return None;
        }
        line_idx = line_idx.saturating_sub(1);
        for scope in scopes_for_section(section) {
            if line_idx == 0 {
                let bound = if rel_col >= 34 {
                    Bound::Critical
                } else {
                    Bound::Warning
                };
                return Some(FieldId::new(section, *scope, bound));
            }
            line_idx = line_idx.saturating_sub(1);
        }
        if section_idx < 2 {
            if line_idx == 0 {
                return None;
            }
            line_idx = line_idx.saturating_sub(1);
        }
    }
    None
}

pub fn settings_integration_field_at(
    body: Rect,
    column: u16,
    row: u16,
) -> Option<IntegrationField> {
    settings_integration_field_at_with_scroll(body, column, row, 0)
}

pub fn settings_integration_field_at_with_scroll(
    body: Rect,
    column: u16,
    row: u16,
    scroll_offset: u16,
) -> Option<IntegrationField> {
    let inner = body.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    if !rect_contains(inner, column, row) {
        return None;
    }
    match row.saturating_sub(inner.y).saturating_add(scroll_offset) {
        1 => Some(IntegrationField::ClaudeSidefile),
        2 => Some(IntegrationField::CodexAppServer),
        _ => None,
    }
}

pub fn settings_parameter_field_at_with_scroll(
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
    body: Rect,
    column: u16,
    row: u16,
    scroll_offset: u16,
) -> Option<ParameterField> {
    let inner = body.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    if !rect_contains(inner, column, row) {
        return None;
    }
    let line_idx = row.saturating_sub(inner.y).saturating_add(scroll_offset) as usize;
    let lines = build_parameter_list_lines(overlay, config);
    let text = lines.get(line_idx).map(line_text)?;
    all_parameter_fields()
        .into_iter()
        .find(|field| parameter_row_text_matches_field(&text, *field))
}

pub fn settings_parameter_field_line_index(
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
    field: ParameterField,
) -> Option<u16> {
    build_parameter_list_lines(overlay, config)
        .iter()
        .position(|line| {
            let text = line_text(line);
            parameter_row_text_matches_field(&text, field)
        })
        .and_then(|idx| u16::try_from(idx).ok())
}

fn parameter_row_text_matches_field(text: &str, field: ParameterField) -> bool {
    text.chars()
        .skip(6)
        .collect::<String>()
        .starts_with(parameter_label(field))
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn rect_contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn settings_body_title(tab: SettingsTab) -> &'static str {
    // v1.51.0: suffix every tab title with `(editable)` or `(reference)`
    // so the operator can tell at a glance whether arrow / Enter / `e`
    // will mutate config or just navigate.
    match tab {
        SettingsTab::Thresholds => " Settings — cost / context / quota thresholds  (editable) ",
        SettingsTab::Integrations => " Settings — provider integrations  (editable) ",
        SettingsTab::Parameters => " Settings — configured parameters  (editable) ",
        SettingsTab::Rules => " Settings — rule conditions  (reference) ",
        SettingsTab::Badges => " Settings — badge glossary  (reference) ",
    }
}

fn build_body_lines<'a>(overlay: &'a SettingsOverlay, config: &'a QmonsterConfig) -> Vec<Line<'a>> {
    match overlay.tab() {
        SettingsTab::Thresholds => build_threshold_body_lines(overlay, config),
        SettingsTab::Integrations => build_integration_body_lines(overlay, config),
        SettingsTab::Parameters => build_parameter_body_lines(overlay, config),
        SettingsTab::Rules => build_rule_body_lines(overlay, config),
        SettingsTab::Badges => build_badge_body_lines(overlay),
    }
}

fn build_threshold_body_lines<'a>(
    overlay: &'a SettingsOverlay,
    config: &'a QmonsterConfig,
) -> Vec<Line<'a>> {
    let mut lines: Vec<Line<'a>> = Vec::new();
    for (i, section) in [Section::Cost, Section::Context, Section::Quota]
        .iter()
        .enumerate()
    {
        if i > 0 {
            lines.push(Line::from(""));
        }
        lines.push(section_header_line(*section));
        for scope in scopes_for_section(*section) {
            lines.push(scope_row_line(overlay, config, *section, *scope));
        }
    }
    lines.push(Line::from(""));
    lines.push(status_line(overlay));
    lines
}

fn build_integration_body_lines(
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
) -> Vec<Line<'static>> {
    vec![
        reference_header_line("[provider_setup]"),
        integration_row_line(
            overlay,
            IntegrationField::ClaudeSidefile,
            config.provider_setup.claude_sidefile,
            "copy Claude statusline snippet with JSON sidefile export",
        ),
        integration_row_line(
            overlay,
            IntegrationField::CodexAppServer,
            config.provider_setup.codex_app_server,
            "auto-spawn qmonster-managed app-server on next TUI start",
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  Provider Setup (P) is read-only. ",
                Style::default().fg(theme::TEXT_DIM),
            ),
            Span::styled(
                "Change these values here, then press w to write TOML.",
                Style::default().fg(theme::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Codex app-server: ", Style::default().fg(theme::TEXT_DIM)),
            Span::styled(
                "restart required after save; Qmonster sends initialize + rateLimits/read.",
                Style::default().fg(theme::TEXT_PRIMARY),
            ),
        ]),
        Line::from(""),
        status_line(overlay),
    ]
}

fn integration_row_line(
    overlay: &SettingsOverlay,
    field: IntegrationField,
    enabled: bool,
    detail: &'static str,
) -> Line<'static> {
    let focused = overlay.is_open()
        && overlay.tab() == SettingsTab::Integrations
        && overlay.selected_integration() == field;
    let cursor = if focused { "▶ " } else { "  " };
    let value = if enabled { "ON " } else { "OFF" };
    let value_style = if focused {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::REVERSED)
    } else if enabled {
        Style::default()
            .fg(theme::severity_color(Severity::Good))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    Line::from(vec![
        Span::raw(format!("    {cursor}")),
        Span::styled(
            format!("{:<29}", field.label()),
            Style::default().fg(theme::TEXT_PRIMARY),
        ),
        Span::styled(format!(" {value} "), value_style),
        Span::styled(detail, Style::default().fg(theme::TEXT_DIM)),
    ])
}

fn build_parameter_body_lines(
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
) -> Vec<Line<'static>> {
    let defaults = QmonsterConfig::defaults();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "  *",
                Style::default().fg(theme::severity_color(Severity::Warning)),
            ),
            Span::styled(
                " differs from the built-in default; unmarked rows are current defaults",
                Style::default().fg(theme::TEXT_DIM),
            ),
        ]),
        Line::from(""),
        reference_header_line("Runtime"),
        setting_row(
            "tmux source/poll/capture",
            format!(
                "{}/{}ms/{} lines",
                config.tmux.source.as_str(),
                config.tmux.poll_interval_ms,
                config.tmux.capture_lines
            ),
            format!(
                "{}/{}ms/{} lines",
                defaults.tmux.source.as_str(),
                defaults.tmux.poll_interval_ms,
                defaults.tmux.capture_lines
            ),
        ),
        setting_row(
            "refresh/idle/logging",
            format!(
                "{}/{} polls/{}",
                refresh_policy_label(config.refresh.policy),
                config.idle.stillness_polls,
                log_sensitivity_label(config.logging.sensitivity)
            ),
            format!(
                "{}/{} polls/{}",
                refresh_policy_label(defaults.refresh.policy),
                defaults.idle.stillness_polls,
                log_sensitivity_label(defaults.logging.sensitivity)
            ),
        ),
        setting_row(
            "logging retention/big output",
            format!(
                "{}d/{} chars",
                config.logging.retention_days, config.logging.big_output_chars
            ),
            format!(
                "{}d/{} chars",
                defaults.logging.retention_days, defaults.logging.big_output_chars
            ),
        ),
        setting_row(
            "storage.root",
            config.storage.root.as_deref().unwrap_or("(auto)").into(),
            defaults.storage.root.as_deref().unwrap_or("(auto)").into(),
        ),
        Line::from(""),
        reference_header_line("Actions"),
        setting_row(
            "mode/send/destructive",
            format!(
                "{}/{}/{}",
                actions_mode_label(config.actions.mode),
                on_off(config.actions.allow_auto_prompt_send),
                on_off(config.actions.allow_destructive_actions)
            ),
            format!(
                "{}/{}/{}",
                actions_mode_label(defaults.actions.mode),
                on_off(defaults.actions.allow_auto_prompt_send),
                on_off(defaults.actions.allow_destructive_actions)
            ),
        ),
        setting_row(
            "actions.notifications/archive",
            format!(
                "{}/{}",
                on_off(config.actions.allow_auto_notifications),
                on_off(config.actions.allow_auto_archive)
            ),
            format!(
                "{}/{}",
                on_off(defaults.actions.allow_auto_notifications),
                on_off(defaults.actions.allow_auto_archive)
            ),
        ),
        setting_row(
            "ux confirm_actions",
            confirm_actions_label(config.ux.confirm_actions).into(),
            confirm_actions_label(defaults.ux.confirm_actions).into(),
        ),
        setting_row(
            "ux hover/language/trigger",
            format!(
                "{}/{}/{}",
                on_off(config.ux.hover_help),
                help_language_label(config.ux.help_language),
                hover_help_trigger_label(config.ux.hover_help_trigger)
            ),
            format!(
                "{}/{}/{}",
                on_off(defaults.ux.hover_help),
                help_language_label(defaults.ux.help_language),
                hover_help_trigger_label(defaults.ux.hover_help_trigger)
            ),
        ),
        setting_row(
            "insights ignored_ttl/default_window",
            format!(
                "{}/{}",
                seconds_label(config.insights.ignored_ttl_secs),
                seconds_label(config.insights.default_window_secs)
            ),
            format!(
                "{}/{}",
                seconds_label(defaults.insights.ignored_ttl_secs),
                seconds_label(defaults.insights.default_window_secs)
            ),
        ),
        Line::from(""),
        reference_header_line("Policy Inputs"),
        setting_row(
            "token quota_tight",
            on_off(config.token.quota_tight).into(),
            on_off(defaults.token.quota_tight).into(),
        ),
        setting_row(
            "provider setup sidefile/server",
            format!(
                "{}/{}",
                on_off(config.provider_setup.claude_sidefile),
                on_off(config.provider_setup.codex_app_server)
            ),
            format!(
                "{}/{}",
                on_off(defaults.provider_setup.claude_sidefile),
                on_off(defaults.provider_setup.codex_app_server)
            ),
        ),
        setting_row(
            "security posture/cross/drift",
            format!(
                "{}/{}/{}",
                on_off(config.security.posture_advisories),
                on_off(config.security.cross_window_findings),
                on_off(config.security.identity_drift_findings)
            ),
            format!(
                "{}/{}/{}",
                on_off(defaults.security.posture_advisories),
                on_off(defaults.security.cross_window_findings),
                on_off(defaults.security.identity_drift_findings)
            ),
        ),
        setting_row(
            "security cross_pane_file",
            on_off(config.security.cross_pane_file_findings).into(),
            on_off(defaults.security.cross_pane_file_findings).into(),
        ),
        setting_row(
            "cache hot/cold ratios",
            format!(
                "{}/{}",
                pct_label(config.cache.hot_ratio_threshold),
                pct_label(config.cache.cold_ratio_threshold)
            ),
            format!(
                "{}/{}",
                pct_label(defaults.cache.hot_ratio_threshold),
                pct_label(defaults.cache.cold_ratio_threshold)
            ),
        ),
        setting_row(
            "cache context hot/cold",
            format!(
                "{}/{}",
                pct_label(f64::from(config.cache.hot_low_ctx_threshold)),
                pct_label(f64::from(config.cache.cold_high_ctx_threshold))
            ),
            format!(
                "{}/{}",
                pct_label(f64::from(defaults.cache.hot_low_ctx_threshold)),
                pct_label(f64::from(defaults.cache.cold_high_ctx_threshold))
            ),
        ),
        setting_row(
            "cache drift drop/samples",
            format!(
                "{}/{}",
                pct_label(config.cache.drift_drop_threshold),
                config.cache.drift_min_samples
            ),
            format!(
                "{}/{}",
                pct_label(defaults.cache.drift_drop_threshold),
                defaults.cache.drift_min_samples
            ),
        ),
        setting_row(
            "reset wait pressure/eta",
            format!(
                "{}/{}",
                pct_label(f64::from(config.reset.wait_pressure_threshold)),
                seconds_label(config.reset.wait_eta_secs)
            ),
            format!(
                "{}/{}",
                pct_label(f64::from(defaults.reset.wait_pressure_threshold)),
                seconds_label(defaults.reset.wait_eta_secs)
            ),
        ),
        setting_row(
            "reset snapshot pressure/eta",
            format!(
                "{}/{}",
                pct_label(f64::from(config.reset.snapshot_pressure_threshold)),
                seconds_label(config.reset.snapshot_eta_secs)
            ),
            format!(
                "{}/{}",
                pct_label(f64::from(defaults.reset.snapshot_pressure_threshold)),
                seconds_label(defaults.reset.snapshot_eta_secs)
            ),
        ),
        setting_row(
            "reset auto_snapshot",
            on_off(config.reset.auto_snapshot).to_string(),
            on_off(defaults.reset.auto_snapshot).to_string(),
        ),
        Line::from(""),
        reference_header_line("Anomaly"),
        setting_row(
            "anomaly enabled",
            on_off(config.anomaly.enabled).to_string(),
            on_off(defaults.anomaly.enabled).to_string(),
        ),
        setting_row(
            "anomaly window_polls",
            config.anomaly.window_polls.to_string(),
            defaults.anomaly.window_polls.to_string(),
        ),
        setting_row(
            "anomaly min_confidence",
            config.anomaly.min_confidence.clone(),
            defaults.anomaly.min_confidence.clone(),
        ),
        setting_row(
            "anomaly identity_churn_min_flips",
            config.anomaly.identity_churn_min_flips.to_string(),
            defaults.anomaly.identity_churn_min_flips.to_string(),
        ),
        setting_row(
            "anomaly error_burst_threshold",
            format!("{:.2}", config.anomaly.error_burst_threshold),
            format!("{:.2}", defaults.anomaly.error_burst_threshold),
        ),
        setting_row(
            "anomaly cache_discontinuity_drop",
            format!("{:.2}", config.anomaly.cache_discontinuity_drop),
            format!("{:.2}", defaults.anomaly.cache_discontinuity_drop),
        ),
        setting_row(
            "anomaly cross_pane_cluster_min",
            config.anomaly.cross_pane_cluster_min_findings.to_string(),
            defaults.anomaly.cross_pane_cluster_min_findings.to_string(),
        ),
        setting_row(
            "anomaly cost_slope_usd_per_hour",
            format!("{:.2}", config.anomaly.cost_slope_usd_per_hour),
            format!("{:.2}", defaults.anomaly.cost_slope_usd_per_hour),
        ),
        setting_row(
            "anomaly token_slope_input_per_poll",
            config.anomaly.token_slope_input_per_poll.to_string(),
            defaults.anomaly.token_slope_input_per_poll.to_string(),
        ),
        setting_row(
            "anomaly memory_growth_mb",
            format!("{:.0}", config.anomaly.memory_growth_mb),
            format!("{:.0}", defaults.anomaly.memory_growth_mb),
        ),
        setting_row(
            "anomaly retention_days",
            config.anomaly.retention_days.to_string(),
            defaults.anomaly.retention_days.to_string(),
        ),
        Line::from(""),
        reference_header_line("Anomaly Promote (per-kind threshold)"),
        setting_row(
            "anomaly.promote identity_churn",
            config.anomaly.promote.identity_churn.clone(),
            defaults.anomaly.promote.identity_churn.clone(),
        ),
        setting_row(
            "anomaly.promote error_burst",
            config.anomaly.promote.error_burst.clone(),
            defaults.anomaly.promote.error_burst.clone(),
        ),
        setting_row(
            "anomaly.promote cache_discontinuity",
            config.anomaly.promote.cache_discontinuity.clone(),
            defaults.anomaly.promote.cache_discontinuity.clone(),
        ),
        setting_row(
            "anomaly.promote cross_pane_edit_cluster",
            config.anomaly.promote.cross_pane_edit_cluster.clone(),
            defaults.anomaly.promote.cross_pane_edit_cluster.clone(),
        ),
        setting_row(
            "anomaly.promote cost_slope",
            config.anomaly.promote.cost_slope.clone(),
            defaults.anomaly.promote.cost_slope.clone(),
        ),
        setting_row(
            "anomaly.promote token_slope",
            config.anomaly.promote.token_slope.clone(),
            defaults.anomaly.promote.token_slope.clone(),
        ),
        setting_row(
            "anomaly.promote memory_growth",
            config.anomaly.promote.memory_growth.clone(),
            defaults.anomaly.promote.memory_growth.clone(),
        ),
        setting_row(
            "anomaly.promote subagent_side_effect",
            config.anomaly.promote.subagent_side_effect.clone(),
            defaults.anomaly.promote.subagent_side_effect.clone(),
        ),
        Line::from(""),
        reference_header_line("Cost / Profile"),
        setting_row(
            "cost budget_usd",
            format!("${:.2}", config.cost.budget_usd),
            format!("${:.2}", defaults.cost.budget_usd),
        ),
        setting_row(
            "profile_switch enabled/threshold/window",
            format!(
                "{}/{}/{} polls",
                on_off(config.profile_switch.enabled),
                pct_label(f64::from(config.profile_switch.error_rate_threshold)),
                config.profile_switch.window_polls
            ),
            format!(
                "{}/{}/{} polls",
                on_off(defaults.profile_switch.enabled),
                pct_label(f64::from(defaults.profile_switch.error_rate_threshold)),
                defaults.profile_switch.window_polls
            ),
        ),
        Line::from(""),
        reference_header_line("Editable Parameters"),
    ];
    append_selected_parameter_help_lines(&mut lines, overlay, config);
    lines.push(Line::from(""));
    append_parameter_editor_lines(&mut lines, overlay, config);
    lines.push(Line::from(""));
    lines.push(status_line(overlay));
    lines
}

fn build_parameter_list_lines(
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
) -> Vec<Line<'static>> {
    let mut lines = build_parameter_body_lines(overlay, config);
    let Some(start) = lines
        .iter()
        .position(|line| line_text(line).trim() == "Selected parameter help")
    else {
        return lines;
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line_text(line).trim().is_empty())
        .map(|(idx, _)| idx + 1)
        .unwrap_or(lines.len());
    lines.drain(start..end);
    remove_status_footer(&mut lines);
    lines
}

fn build_parameter_help_panel_lines(
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    append_selected_parameter_help_lines(&mut lines, overlay, config);
    lines.push(Line::from(""));
    lines.push(status_line(overlay));
    lines
}

fn remove_status_footer(lines: &mut Vec<Line<'static>>) {
    if lines
        .last()
        .map(line_text)
        .is_some_and(|text| text.trim_start().starts_with("status:"))
    {
        lines.pop();
        if lines
            .last()
            .map(line_text)
            .is_some_and(|text| text.trim().is_empty())
        {
            lines.pop();
        }
    }
}

fn build_rule_body_lines(overlay: &SettingsOverlay, config: &QmonsterConfig) -> Vec<Line<'static>> {
    vec![
        reference_header_line("Shared Gates"),
        rule_row(
            "provider-specific",
            "identity confidence must be Medium or High".into(),
        ),
        rule_row(
            "attention wait",
            "cache and memory advisories suppress on input/permission wait".into(),
        ),
        rule_row(
            "aggressive profiles",
            format!("token.quota_tight must be {}", on_off(true)),
        ),
        rule_row(
            "action explainer",
            format!(
                "p/d/y opens modal: always = every time, first_time = once per kind per session, never = skipped; target must exist; current = {}",
                confirm_actions_label(config.ux.confirm_actions)
            ),
        ),
        rule_row(
            "insights ttl ignored",
            format!(
                "classifies unresolved recommendations as TTL ignored after {} without correlated outcome",
                seconds_label(config.insights.ignored_ttl_secs)
            ),
        ),
        Line::from(""),
        reference_header_line("Pressure Rules"),
        rule_row(
            "cost pressure",
            format!(
                "warning >= ${:.2}; critical >= ${:.2}; provider overrides apply",
                config.cost.warning_usd, config.cost.critical_usd
            ),
        ),
        rule_row(
            "cost budget",
            format!(
                "one-shot 80% (concern) / 100% (warning) alerts; budget = ${:.2}; 0.0 disables",
                config.cost.budget_usd
            ),
        ),
        rule_row(
            "context pressure",
            format!(
                "warning >= {}; critical >= {}",
                pct_label(f64::from(config.context.warning_pct)),
                pct_label(f64::from(config.context.critical_pct))
            ),
        ),
        rule_row(
            "quota pressure",
            format!(
                "warning >= {}; critical >= {}; Claude/Codex split 5h + weekly",
                pct_label(f64::from(config.quota.warning_pct)),
                pct_label(f64::from(config.quota.critical_pct))
            ),
        ),
        rule_row(
            "profile switch",
            format!(
                "error-rate over {} polls >= {}; recommends script-low-token profile; enabled = {}",
                config.profile_switch.window_polls,
                pct_label(f64::from(config.profile_switch.error_rate_threshold)),
                on_off(config.profile_switch.enabled)
            ),
        ),
        Line::from(""),
        reference_header_line("Reset"),
        rule_row(
            "wait for reset",
            format!(
                "5h/weekly pressure >= {} and eta <= {}",
                pct_label(f64::from(config.reset.wait_pressure_threshold)),
                seconds_label(config.reset.wait_eta_secs)
            ),
        ),
        rule_row(
            "snapshot before reset",
            format!(
                "any eta <= {} and pressure >= {}; suggests s snapshot (auto when [reset] auto_snapshot = true)",
                seconds_label(config.reset.snapshot_eta_secs),
                pct_label(f64::from(config.reset.snapshot_pressure_threshold))
            ),
        ),
        rule_row(
            "reset sources",
            "Claude sidefile / Codex app-server; Gemini silent until resets_at exists".into(),
        ),
        Line::from(""),
        reference_header_line("Cache / Memory"),
        rule_row(
            "cache hot wait",
            format!(
                "cache > {} and context < {}",
                pct_label(config.cache.hot_ratio_threshold),
                pct_label(f64::from(config.cache.hot_low_ctx_threshold))
            ),
        ),
        rule_row(
            "cache cold compact",
            format!(
                "cache < {} and context > {}",
                pct_label(config.cache.cold_ratio_threshold),
                pct_label(f64::from(config.cache.cold_high_ctx_threshold))
            ),
        ),
        rule_row(
            "cache drift",
            format!(
                "drop >= {} over >= {} token samples",
                pct_label(config.cache.drift_drop_threshold),
                config.cache.drift_min_samples
            ),
        ),
        rule_row("memory bloat", "agent memory files > 50,000 bytes".into()),
        Line::from(""),
        reference_header_line("Opt-in Findings"),
        rule_row(
            "security posture",
            format!(
                "security.posture_advisories = {}",
                on_off(config.security.posture_advisories)
            ),
        ),
        rule_row(
            "cross-window",
            format!(
                "security.cross_window_findings = {}; same repo+branch in >=2 windows",
                on_off(config.security.cross_window_findings)
            ),
        ),
        rule_row(
            "cross-pane file",
            format!(
                "security.cross_pane_file_findings = {}; same absolute file edited from >=2 panes",
                on_off(config.security.cross_pane_file_findings)
            ),
        ),
        rule_row(
            "identity drift",
            format!(
                "security.identity_drift_findings = {}; provider/path changes between polls",
                on_off(config.security.identity_drift_findings)
            ),
        ),
        rule_row(
            "prompt send",
            format!(
                "actions.mode != observe_only and auto_prompt_send = {}",
                on_off(config.actions.allow_auto_prompt_send)
            ),
        ),
        Line::from(""),
        reference_header_line("Anomaly Detectors"),
        rule_row(
            "anomaly: IdentityChurn",
            format!(
                "fires when (provider, path) flips >= {} times in {} polls; promotes to Recommendation when confidence >= {}",
                config.anomaly.identity_churn_min_flips,
                config.anomaly.window_polls,
                config.anomaly.promote.identity_churn,
            ),
        ),
        rule_row(
            "anomaly: ErrorBurst",
            format!(
                "fires when error rate >= {:.0}% over {} polls; promotes to Recommendation when confidence >= {}",
                config.anomaly.error_burst_threshold * 100.0,
                config.anomaly.window_polls,
                config.anomaly.promote.error_burst,
            ),
        ),
        rule_row(
            "anomaly: CacheDiscontinuity",
            format!(
                "fires when cache_hit_ratio drops >= {:.0}pp OR F-7b fires >= 2x in {} polls; promotes to Recommendation when confidence >= {}",
                config.anomaly.cache_discontinuity_drop * 100.0,
                config.anomaly.window_polls,
                config.anomaly.promote.cache_discontinuity,
            ),
        ),
        rule_row(
            "anomaly: CrossPaneEditCluster",
            format!(
                "fires when >= {} ConcurrentFileEdit findings target the same path in {} polls; promotes to Recommendation when confidence >= {}",
                config.anomaly.cross_pane_cluster_min_findings,
                config.anomaly.window_polls,
                config.anomaly.promote.cross_pane_edit_cluster,
            ),
        ),
        rule_row(
            "anomaly: CostSlope",
            format!(
                "fires when cost slope >= {:.2} USD/hour over {} polls; promotes to Recommendation when confidence >= {}",
                config.anomaly.cost_slope_usd_per_hour,
                config.anomaly.window_polls,
                config.anomaly.promote.cost_slope,
            ),
        ),
        rule_row(
            "anomaly: TokenSlope",
            format!(
                "fires when input_tokens slope >= {} per poll over {} polls; promotes to Recommendation when confidence >= {}",
                config.anomaly.token_slope_input_per_poll,
                config.anomaly.window_polls,
                config.anomaly.promote.token_slope,
            ),
        ),
        rule_row(
            "anomaly: MemoryGrowth",
            format!(
                "fires when process_memory_mb grows >= {:.0} MB over {} polls; promotes to Recommendation when confidence >= {}",
                config.anomaly.memory_growth_mb,
                config.anomaly.window_polls,
                config.anomaly.promote.memory_growth,
            ),
        ),
        rule_row(
            "anomaly: SubagentSideEffect",
            format!(
                "fires when subagent_hint observed in {} polls AND another anomaly fires (correlation, not attribution); promotes when subagent_hint co-occurs with another anomaly (confidence >= {})",
                config.anomaly.window_polls, config.anomaly.promote.subagent_side_effect,
            ),
        ),
        Line::from(""),
        status_line(overlay),
    ]
}

fn build_badge_body_lines(overlay: &SettingsOverlay) -> Vec<Line<'static>> {
    vec![
        reference_header_line("Source Labels"),
        badge_row(
            "[Official]",
            "reported by the provider surface or provider-backed sidefile/app-server",
        ),
        badge_row(
            "[Estimate]",
            "computed by Qmonster from observed provider values; useful for trend, verify before acting",
        ),
        badge_row(
            "[Heur]",
            "heuristic local observation such as /proc memory, file scan, or output pattern",
        ),
        badge_row(
            "[Qmonster]",
            "project rule, policy, profile, or local config",
        ),
        Line::from(""),
        reference_header_line("Metric Badges"),
        badge_row(
            "CTX N%",
            "context window used, not remaining; higher means closer to compact/clear pressure",
        ),
        badge_row(
            "QUOTA N%",
            "single quota window used; Gemini status-table quota is normalized to used percentage",
        ),
        badge_row(
            "QUOTA 5H/WEEK",
            "rolling 5-hour / weekly limit used; Claude statusline and Codex status/app-server surfaces",
        ),
        badge_row(
            "RESET 5H/7D",
            "countdown until provider reset timestamp; Claude sidefile or Codex app-server supplies resets_at; rendered as 4d 6h for >=24h, 2h13m / 45m / 30s for <24h",
        ),
        badge_row(
            "TOKENS N",
            "provider-visible session token count; selected pane also shows prompt=input+cached and recent delta",
        ),
        badge_row(
            "token io",
            "expanded pane input/output token counts from provider stats when both sides are known",
        ),
        badge_row(
            "CACHE N%",
            "cache_read / (input + cache_read); ? means waiting for a capable surface, — means this provider/auth surface does not expose cache reads",
        ),
        badge_row(
            "cache io",
            "expanded pane cache read/create token counts when a provider sidefile or stats panel exposes them",
        ),
        badge_row(
            "COST $N",
            "Claude sidefile total_cost_usd is Official; Codex/Gemini cost is Estimate from input/output tokens plus ~/.qmonster/config/pricing.toml; zero-rate entries keep COST hidden",
        ),
        badge_row(
            "MODEL",
            "current provider model parsed from the provider status surface",
        ),
        badge_row(
            "MEM",
            "process resident memory; Gemini Official, Claude/Codex local /proc heuristic; metrics overlay shows ▲/▼/─ trend vs last poll",
        ),
        badge_row(
            "MEM-FILE",
            "bytes in AGENTS.md / CLAUDE.md / GEMINI.md style memory files loaded from repo/home paths",
        ),
        Line::from(""),
        reference_header_line("Context / Runtime"),
        badge_row(
            "PATH / BRANCH",
            "provider-reported working directory and git branch when visible",
        ),
        badge_row(
            "EFFORT",
            "reasoning effort parsed from provider status/config surfaces",
        ),
        badge_row(
            "PERM / MODE",
            "permission or autonomy mode such as bypass permissions, YOLO, or sandbox mode",
        ),
        badge_row(
            "DIR / AGENTS",
            "allowed directory or provider agent config path",
        ),
        badge_row(
            "SID / XSCRIPT",
            "Claude/Gemini session id and Claude transcript path when exported",
        ),
        badge_row("CALLS N", "Gemini /stats session Tool Calls count"),
        badge_row(
            "RESET model",
            "Gemini /model Reset line with provider-rendered time and remaining duration",
        ),
        badge_row(
            "TOOL / SKILL",
            "loaded or observed tool/skill/plugin/runtime capability",
        ),
        Line::from(""),
        reference_header_line("Anomaly"),
        badge_row(
            "ANOMALIES",
            "anomaly count and kinds per pane (in m overlay); aggregated from D2/F-9/F-3+F-7b/F-8 history",
        ),
        Line::from(""),
        status_line(overlay),
    ]
}

fn reference_header_line(label: &'static str) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!("  {label}"),
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )])
}

fn badge_row(label: &'static str, meaning: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("    "),
        Span::styled(format!("{label:<17}"), Style::default().fg(theme::TEXT_DIM)),
        Span::styled(meaning, Style::default().fg(theme::TEXT_PRIMARY)),
    ])
}

fn setting_row(label: &'static str, value: String, default: String) -> Line<'static> {
    let custom = value != default;
    let marker = if custom { "*" } else { " " };
    let marker_style = if custom {
        Style::default().fg(theme::severity_color(Severity::Warning))
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    let default_note = if custom {
        format!("  default {default}")
    } else {
        "  default".to_string()
    };
    Line::from(vec![
        Span::styled(format!("  {marker} "), marker_style),
        Span::styled(format!("{label:<31}"), Style::default().fg(theme::TEXT_DIM)),
        Span::styled(value, Style::default().fg(theme::TEXT_PRIMARY)),
        Span::styled(default_note, Style::default().fg(theme::TEXT_DIM)),
    ])
}

fn append_parameter_editor_lines(
    lines: &mut Vec<Line<'static>>,
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
) {
    let mut current_section: Option<&'static str> = None;
    for field in all_parameter_fields() {
        let section = parameter_section_label(field);
        if current_section != Some(section) {
            if current_section.is_some() {
                lines.push(Line::from(""));
            }
            lines.push(reference_header_line(section));
            current_section = Some(section);
        }
        lines.push(parameter_row_line(overlay, config, field));
    }
}

fn append_selected_parameter_help_lines(
    lines: &mut Vec<Line<'static>>,
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
) {
    let field = overlay.selected_parameter();
    let current = parameter_value_for_display(config, field);
    let default = parameter_default_for_display(field);
    lines.push(reference_header_line("Selected parameter help"));
    lines.push(parameter_help_line(
        "TOML",
        &parameter_toml_path(field).join("."),
    ));
    lines.push(parameter_help_line(
        "Value",
        &format!("{current}  (default {default})"),
    ));
    lines.push(parameter_help_line("Meaning", parameter_help_text(field)));
    lines.push(parameter_help_line("Values", parameter_value_help(field)));
    if let Some(shortcut) = parameter_shortcut_help(field) {
        lines.push(parameter_help_line("Shortcut", shortcut));
    }
    lines.push(parameter_help_line(
        "Save",
        "changes are runtime-only until w writes them to the loaded TOML",
    ));
}

fn parameter_help_line(label: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("    "),
        Span::styled(format!("{label}: "), Style::default().fg(theme::TEXT_DIM)),
        Span::styled(value.to_string(), Style::default().fg(theme::TEXT_PRIMARY)),
    ])
}

fn parameter_help_text(field: ParameterField) -> &'static str {
    use ParameterField::*;
    match field {
        TmuxSource => {
            "selects the tmux transport used to read panes: auto prefers control-mode, then polling"
        }
        TmuxPollIntervalMs => "sets how often Qmonster polls tmux when polling transport is active",
        TmuxCaptureLines => "sets how many terminal tail lines are captured for signal parsing",
        RefreshPolicy => "controls whether runtime refresh is manual-only or automatic",
        IdleStillnessPolls => "number of unchanged tail polls before a pane is considered stale",
        LoggingSensitivity => {
            "controls how much operational evidence Qmonster records in logs and ledgers"
        }
        LoggingRetentionDays => "retention window for stored operational logs",
        LoggingBigOutputChars => {
            "threshold where output is treated as large/verbose for storage and display"
        }
        StorageRoot => "overrides the Qmonster storage root; blank uses the automatic default",
        ActionsMode => "top-level action policy: observe, recommend, or safe automation",
        ActionsAutoNotifications => {
            "permits desktop/system notifications when rules emit eligible alerts"
        }
        ActionsAutoArchive => "permits archive/audit side effects for eligible recommendations",
        ActionsAutoPromptSend => {
            "permits Qmonster to send approved prompt-send proposals to target panes"
        }
        ActionsDestructive => {
            "permits destructive actions; keep off unless the operator explicitly wants them"
        }
        UxConfirmActions => "controls how often p/d/y opens the Action Explainer before dispatch",
        UxHoverHelp => {
            "shows floating help over Alerts and Panes rows so operators can understand labels without leaving Qmonster"
        }
        UxHoverHelpTrigger => {
            "controls whether hover help opens only near row labels or across the full row"
        }
        UxHelpLanguage => "selects the language used by floating help and related glossary text",
        InsightsIgnoredTtlSecs => {
            "time after which unresolved recommendation lifecycle events become TTL-ignored in insights"
        }
        InsightsDefaultWindowSecs => {
            "default reporting window for Token Insights and lifecycle summaries"
        }
        TokenQuotaTight => {
            "marks quota as tight so cache/profile recommendations can become more conservative"
        }
        SecurityPostureAdvisories => {
            "emits passive concerns when provider permissions or sandbox posture are permissive"
        }
        SecurityCrossWindowFindings => {
            "enables cross-window concurrent-work findings for panes sharing repo context"
        }
        SecurityIdentityDriftFindings => {
            "enables findings when a pane's provider or path identity changes between polls"
        }
        SecurityCrossPaneFileFindings => {
            "enables findings for concurrent cross-pane edits touching the same files"
        }
        CacheHotRatioThreshold => "cache hit ratio above which hot-cache compact warnings may fire",
        CacheColdRatioThreshold => {
            "cache hit ratio below which cold-cache compact recommendations may fire"
        }
        CacheHotLowCtxThreshold => {
            "context pressure ceiling used when deciding whether hot cache has room to continue"
        }
        CacheColdHighCtxThreshold => "context pressure floor used when cold cache should compact",
        CacheDriftDropThreshold => {
            "minimum cache-hit drop between samples before cache drift is reported"
        }
        CacheDriftMinSamples => "minimum sample count required before cache drift rules evaluate",
        ResetWaitPressureThreshold => {
            "context/quota pressure threshold used for wait/reset recommendations"
        }
        ResetWaitEtaSecs => {
            "ETA threshold used when deciding whether waiting for reset is worthwhile"
        }
        ResetSnapshotPressureThreshold => "pressure threshold that can trigger snapshot guidance",
        ResetSnapshotEtaSecs => "ETA threshold used with snapshot guidance before a reset window",
        ResetAutoSnapshot => {
            "enables automatic operator snapshot creation when reset rules recommend it"
        }
        AnomalyEnabled => "turns anomaly detection and anomaly event recording on or off",
        AnomalyWindowPolls => "rolling poll window size used by anomaly detectors",
        AnomalyMinConfidence => "minimum anomaly confidence required for surfacing/promotion",
        AnomalyIdentityChurnMinFlips => "number of identity flips required to flag identity churn",
        AnomalyErrorBurstThreshold => "error density threshold for error-burst anomaly detection",
        AnomalyCacheDiscontinuityDrop => "cache ratio drop that counts as a cache discontinuity",
        AnomalyCrossPaneClusterMinFindings => {
            "minimum cross-pane findings needed to form an edit cluster anomaly"
        }
        AnomalyCostSlopeUsdPerHour => "cost growth rate threshold for cost-slope anomaly detection",
        AnomalyTokenSlopeInputPerPoll => {
            "input-token growth threshold per poll for token-slope anomalies"
        }
        AnomalyMemoryGrowthMb => "resident-memory growth threshold for memory anomalies",
        AnomalyRetentionDays => "number of days to retain anomaly event history",
        AnomalyPromoteIdentityChurn
        | AnomalyPromoteErrorBurst
        | AnomalyPromoteCacheDiscontinuity
        | AnomalyPromoteCrossPaneEditCluster
        | AnomalyPromoteCostSlope
        | AnomalyPromoteTokenSlope
        | AnomalyPromoteMemoryGrowth
        | AnomalyPromoteSubagentSideEffect => {
            "per-anomaly confidence threshold used before promoting an anomaly into a recommendation"
        }
        CostBudgetUsd => "operator budget used by cost-budget warning rules",
        ProfileSwitchEnabled => {
            "enables provider profile-switch recommendations when repeated failures suggest a cheaper/safer profile"
        }
        ProfileSwitchWindowPolls => "rolling window size for profile-switch failure analysis",
        ProfileSwitchErrorRateThreshold => {
            "error-rate threshold required before profile-switch recommendations fire"
        }
        FxEnabled => "enables the optional full-screen visual effects overlay",
        FxText => "text shown by the banner-style visual effect",
        FxEffect => "selects the visual effect style used by the hotkey/screensaver overlay",
        FxDurationSecs => {
            "auto-dismiss duration for visual effects; zero means stay until dismissed"
        }
        FxHotkeyEnabled => {
            "enables the operator hotkey for manually opening the visual effect overlay"
        }
        FxCelebrationEnabled => "enables celebratory effects after eligible successful actions",
        FxScreensaverEnabled => "enables the idle screensaver-style visual effect overlay",
        FxScreensaverIdleSecs => "idle time before the screensaver-style visual effect can open",
    }
}

fn parameter_value_help(field: ParameterField) -> &'static str {
    use ParameterField::*;
    match field {
        UxConfirmActions => "always | first_time | never",
        UxHoverHelpTrigger => "label | row",
        UxHelpLanguage => "ko | en",
        TmuxSource => "auto | polling | control_mode",
        FxEffect => "banner | confetti | matrix",
        RefreshPolicy => "manual_only | automatic",
        LoggingSensitivity => "minimal | balanced | forensic",
        ActionsMode => "observe_only | recommend_only | safe_auto",
        AnomalyMinConfidence
        | AnomalyPromoteIdentityChurn
        | AnomalyPromoteErrorBurst
        | AnomalyPromoteCacheDiscontinuity
        | AnomalyPromoteCrossPaneEditCluster
        | AnomalyPromoteCostSlope
        | AnomalyPromoteTokenSlope
        | AnomalyPromoteMemoryGrowth
        | AnomalyPromoteSubagentSideEffect => "low | medium | high",
        ActionsAutoNotifications
        | ActionsAutoArchive
        | ActionsAutoPromptSend
        | ActionsDestructive
        | UxHoverHelp
        | TokenQuotaTight
        | SecurityPostureAdvisories
        | SecurityCrossWindowFindings
        | SecurityIdentityDriftFindings
        | SecurityCrossPaneFileFindings
        | ResetAutoSnapshot
        | AnomalyEnabled
        | ProfileSwitchEnabled
        | FxEnabled
        | FxHotkeyEnabled
        | FxCelebrationEnabled
        | FxScreensaverEnabled => "on | off",
        StorageRoot | FxText => "free text; blank keeps the configured empty value",
        _ => "numeric threshold; edit with e/Enter, then save with w",
    }
}

fn parameter_shortcut_help(field: ParameterField) -> Option<&'static str> {
    match field {
        ParameterField::UxHoverHelp => Some("H"),
        ParameterField::UxHelpLanguage => Some("L"),
        ParameterField::UxHoverHelpTrigger => {
            Some("label mode reduces accidental popups; row mode keeps the old broad hover area")
        }
        ParameterField::UxConfirmActions => Some("affects p/d/y Action Explainer behavior"),
        _ => None,
    }
}

fn parameter_row_line(
    overlay: &SettingsOverlay,
    config: &QmonsterConfig,
    field: ParameterField,
) -> Line<'static> {
    let focused = overlay.is_open()
        && overlay.tab() == SettingsTab::Parameters
        && overlay.selected_parameter() == field;
    let value = if focused && overlay.edit_buffer().is_some() {
        format!("{}_", overlay.edit_buffer().unwrap_or(""))
    } else {
        parameter_value_for_display(config, field)
    };
    let default = parameter_default_for_display(field);
    let custom = value.trim_end_matches('_') != default;
    let marker = if custom { "*" } else { " " };
    let cursor = if focused { "▶ " } else { "  " };
    let value_style = if focused {
        if overlay.edit_buffer().is_some() {
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .bg(theme::BADGE_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::REVERSED)
        }
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };
    let marker_style = if custom {
        Style::default().fg(theme::severity_color(Severity::Warning))
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    let action = match parameter_edit_kind(field) {
        ParameterEditKind::Bool => "toggle",
        ParameterEditKind::Enum => "cycle",
        ParameterEditKind::Number => "edit",
        ParameterEditKind::Text => "edit",
    };
    Line::from(vec![
        Span::styled(format!("  {marker} "), marker_style),
        Span::raw(cursor),
        Span::styled(
            format!("{:<38}", parameter_label(field)),
            Style::default().fg(theme::TEXT_DIM),
        ),
        Span::styled(format!("{:<18}", value), value_style),
        Span::styled(
            format!(" default {default:<12}"),
            Style::default().fg(theme::TEXT_DIM),
        ),
        Span::styled(action, Style::default().fg(theme::TEXT_DIM)),
    ])
}

fn rule_row(label: &'static str, condition: String) -> Line<'static> {
    Line::from(vec![
        Span::raw("    "),
        Span::styled(format!("{label:<20}"), Style::default().fg(theme::TEXT_DIM)),
        Span::styled(condition, Style::default().fg(theme::TEXT_PRIMARY)),
    ])
}

fn actions_mode_label(mode: ActionsMode) -> &'static str {
    match mode {
        ActionsMode::ObserveOnly => "observe_only",
        ActionsMode::RecommendOnly => "recommend_only",
        ActionsMode::SafeAuto => "safe_auto",
    }
}

fn confirm_actions_label(mode: ConfirmActions) -> &'static str {
    match mode {
        ConfirmActions::Always => "always",
        ConfirmActions::FirstTime => "first_time",
        ConfirmActions::Never => "never",
    }
}

fn help_language_label(language: HelpLanguage) -> &'static str {
    language.as_str()
}

fn hover_help_trigger_label(trigger: HoverHelpTrigger) -> &'static str {
    trigger.as_str()
}

fn refresh_policy_label(policy: RefreshPolicy) -> &'static str {
    match policy {
        RefreshPolicy::ManualOnly => "manual_only",
        RefreshPolicy::Automatic => "automatic",
    }
}

fn log_sensitivity_label(sensitivity: LogSensitivity) -> &'static str {
    match sensitivity {
        LogSensitivity::Minimal => "minimal",
        LogSensitivity::Balanced => "balanced",
        LogSensitivity::Forensic => "forensic",
    }
}

fn on_off(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

fn pct_label(v: f64) -> String {
    format!("{:.0}%", v * 100.0)
}

fn seconds_label(seconds: u64) -> String {
    match seconds {
        s if s >= 3600 && s % 3600 == 0 => format!("{}h", s / 3600),
        s if s >= 3600 => format!("{}h{}m", s / 3600, (s % 3600) / 60),
        s if s >= 60 && s % 60 == 0 => format!("{}m", s / 60),
        s if s >= 60 => format!("{}m{}s", s / 60, s % 60),
        s => format!("{s}s"),
    }
}

fn section_header_line(section: Section) -> Line<'static> {
    let unit = match section {
        Section::Cost => "(USD)",
        Section::Context => "(fraction 0..=1)",
        Section::Quota => "(fraction 0..=1)",
    };
    Line::from(vec![
        Span::styled(
            format!("  [{}] {}", describe_section(section), unit),
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "              warning      critical",
            Style::default().fg(theme::TEXT_DIM),
        ),
    ])
}

fn scope_row_line<'a>(
    overlay: &'a SettingsOverlay,
    config: &'a QmonsterConfig,
    section: Section,
    scope: Scope,
) -> Line<'a> {
    let warning_field = FieldId::new(section, scope, Bound::Warning);
    let critical_field = FieldId::new(section, scope, Bound::Critical);
    let focus_warning = overlay.is_open() && overlay.selected() == warning_field;
    let focus_critical = overlay.is_open() && overlay.selected() == critical_field;

    let cursor = if focus_warning || focus_critical {
        "▶ "
    } else {
        "  "
    };
    let scope_label = format!("{:<14}", describe_scope(scope));

    let warning_cell = cell_text(overlay, config, warning_field);
    let critical_cell = cell_text(overlay, config, critical_field);

    let warning_style = cell_style(overlay, focus_warning);
    let critical_style = cell_style(overlay, focus_critical);

    Line::from(vec![
        Span::raw(format!("    {cursor}")),
        Span::raw(scope_label),
        Span::styled(format!(" {warning_cell:>10}"), warning_style),
        Span::raw("   "),
        Span::styled(format!("{critical_cell:>10}"), critical_style),
        Span::styled(
            override_marker(config, section, scope),
            Style::default().fg(theme::TEXT_DIM),
        ),
    ])
}

fn cell_text(overlay: &SettingsOverlay, config: &QmonsterConfig, field: FieldId) -> String {
    let editing =
        overlay.is_open() && overlay.selected() == field && overlay.edit_buffer().is_some();
    if editing {
        let buf = overlay.edit_buffer().unwrap_or("");
        return format!("{buf}_");
    }
    match overlay.read_field(config, field) {
        Some(v) => format_value_for_display(v, field.section)
            .trim()
            .to_string(),
        None => "(default)".into(),
    }
}

fn cell_style(overlay: &SettingsOverlay, focused: bool) -> Style {
    if !focused {
        return Style::default().fg(theme::TEXT_PRIMARY);
    }
    if overlay.edit_buffer().is_some() {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .bg(theme::BADGE_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::REVERSED)
    }
}

fn override_marker(config: &QmonsterConfig, section: Section, scope: Scope) -> &'static str {
    if scope == Scope::Default {
        return "";
    }
    let has_override = match section {
        Section::Cost => match scope {
            Scope::Claude => config.cost.claude.is_some(),
            Scope::Codex => config.cost.codex.is_some(),
            Scope::Gemini => config.cost.gemini.is_some(),
            Scope::Claude5h | Scope::ClaudeWeekly | Scope::Codex5h | Scope::CodexWeekly => false,
            Scope::Default => false,
        },
        Section::Context => match scope {
            Scope::Claude => config.context.claude.is_some(),
            Scope::Codex => config.context.codex.is_some(),
            Scope::Gemini => config.context.gemini.is_some(),
            Scope::Claude5h | Scope::ClaudeWeekly | Scope::Codex5h | Scope::CodexWeekly => false,
            Scope::Default => false,
        },
        Section::Quota => match scope {
            Scope::Claude => config.quota.claude_5h.is_some() || config.quota.claude.is_some(),
            Scope::Claude5h => config.quota.claude_5h.is_some() || config.quota.claude.is_some(),
            Scope::ClaudeWeekly => {
                config.quota.claude_weekly.is_some() || config.quota.claude.is_some()
            }
            Scope::Codex => config.quota.codex_5h.is_some() || config.quota.codex.is_some(),
            Scope::Codex5h => config.quota.codex_5h.is_some() || config.quota.codex.is_some(),
            Scope::CodexWeekly => {
                config.quota.codex_weekly.is_some() || config.quota.codex.is_some()
            }
            Scope::Gemini => config.quota.gemini.is_some(),
            Scope::Default => false,
        },
    };
    if has_override { "  *override" } else { "" }
}

fn status_line(overlay: &SettingsOverlay) -> Line<'static> {
    let (label, fg) = match overlay.status() {
        SettingsStatus::Idle => ("idle".to_string(), theme::TEXT_DIM),
        SettingsStatus::Dirty => (
            "dirty — press 'w' to save".to_string(),
            theme::severity_color(Severity::Warning),
        ),
        SettingsStatus::Error(msg) => {
            return Line::from(vec![
                Span::styled("  status: ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled(
                    format!("error — {msg}"),
                    Style::default().fg(theme::severity_color(Severity::Risk)),
                ),
            ]);
        }
        SettingsStatus::Saved(path) => {
            return Line::from(vec![
                Span::styled("  status: ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled(
                    format!("saved → {path}"),
                    Style::default().fg(theme::severity_color(Severity::Good)),
                ),
            ]);
        }
    };
    Line::from(vec![
        Span::styled("  status: ", Style::default().fg(theme::TEXT_DIM)),
        Span::styled(label, Style::default().fg(fg)),
    ])
}

#[cfg(test)]
fn hint_lines(overlay: &SettingsOverlay) -> Vec<Line<'static>> {
    hint_lines_with_scroll(overlay, None)
}

fn hint_lines_with_scroll(
    overlay: &SettingsOverlay,
    scroll: Option<(u16, u16)>,
) -> Vec<Line<'static>> {
    let editing = overlay.edit_buffer().is_some();
    let line1 = if editing {
        "  EDIT — type digits/'.' · Enter commit · Esc cancel · Backspace delete"
    } else if overlay.tab() == SettingsTab::Thresholds {
        "  [1]-[5]/[Tab] tab · ↑/↓ select · e/Enter edit · c clear · w write · q/Esc close"
    } else if overlay.tab() == SettingsTab::Integrations {
        "  [1]-[5]/[Tab] tab · ↑/↓ select · Space/e/Enter toggle · w write · q/Esc close"
    } else if overlay.tab() == SettingsTab::Parameters {
        "  [1]-[5]/[Tab] tab · ↑/↓ select · e/Enter edit/cycle · Space toggle · H hover · L language · w write · q/Esc close"
    } else {
        "  [1]-[5]/[Tab] tab · ↑/↓/j/k/wheel scroll · PgUp/PgDn · Home/End · q/Esc close"
    };
    let line2 = if matches!(
        overlay.tab(),
        SettingsTab::Thresholds | SettingsTab::Integrations | SettingsTab::Parameters
    ) {
        "  Edits stay in memory until 'w' writes back to the loaded TOML."
    } else {
        "  This tab is read-only reference."
    };
    let line2 = match scroll {
        Some((offset, max)) => {
            format!(
                "{} · {}",
                line2,
                scroll_hint::scroll_status_label(offset, max)
            )
        }
        None => line2.to_string(),
    };
    vec![Line::from(line1), Line::from(line2)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> QmonsterConfig {
        QmonsterConfig::defaults()
    }

    fn rendered_text(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // -----------------------------------------------------------------
    // Cluster A: open / close / navigation.
    // -----------------------------------------------------------------

    #[test]
    fn default_overlay_is_closed() {
        let s = SettingsOverlay::new();
        assert!(!s.is_open());
    }

    #[test]
    fn settings_field_at_maps_body_rows_to_warning_and_critical_cells() {
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = settings_modal_rects(viewport);
        let inner = rects.body.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });

        let warning = settings_field_at(rects.body, inner.x + 20, inner.y + 1)
            .expect("default cost warning cell should hit a field");
        assert_eq!(
            warning,
            FieldId::new(Section::Cost, Scope::Default, Bound::Warning)
        );

        let critical = settings_field_at(rects.body, inner.x + 34, inner.y + 1)
            .expect("default cost critical cell should hit a field");
        assert_eq!(
            critical,
            FieldId::new(Section::Cost, Scope::Default, Bound::Critical)
        );

        assert!(
            settings_field_at(rects.body, inner.x + 20, inner.y).is_none(),
            "section header rows are not editable fields"
        );
    }

    #[test]
    fn open_sets_open_and_focuses_first_field() {
        let mut s = SettingsOverlay::new();
        s.open();
        assert!(s.is_open());
        assert_eq!(s.tab(), SettingsTab::Thresholds);
        assert_eq!(
            s.selected(),
            FieldId::new(Section::Cost, Scope::Default, Bound::Warning)
        );
    }

    #[test]
    fn close_unsets_open_and_clears_edit_buffer() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.start_edit(&cfg());
        s.close();
        assert!(!s.is_open());
        assert_eq!(s.edit_buffer(), None);
    }

    #[test]
    fn next_field_advances_through_all_fields_and_wraps() {
        let mut s = SettingsOverlay::new();
        s.open();
        let start = s.selected();
        for _ in 0..all_fields().len() {
            s.next_field();
        }
        assert_eq!(
            s.selected(),
            start,
            "one full all_fields cycle must wrap to start"
        );
    }

    #[test]
    fn prev_field_reverses() {
        let mut s = SettingsOverlay::new();
        s.open();
        let first = s.selected();
        s.next_field();
        s.prev_field();
        assert_eq!(s.selected(), first);
    }

    #[test]
    fn next_field_is_noop_during_edit() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.start_edit(&cfg());
        let before = s.selected();
        s.next_field();
        assert_eq!(s.selected(), before, "must not move focus while editing");
    }

    #[test]
    fn tab_navigation_cycles_and_is_noop_during_edit() {
        let mut s = SettingsOverlay::new();
        s.open();

        s.next_tab();
        assert_eq!(s.tab(), SettingsTab::Integrations);
        s.next_tab();
        assert_eq!(s.tab(), SettingsTab::Parameters);
        s.next_tab();
        assert_eq!(s.tab(), SettingsTab::Rules);
        s.next_tab();
        assert_eq!(s.tab(), SettingsTab::Badges);
        s.next_tab();
        assert_eq!(s.tab(), SettingsTab::Thresholds);
        s.previous_tab();
        assert_eq!(s.tab(), SettingsTab::Badges);

        s.switch_tab(SettingsTab::Thresholds);
        s.start_edit(&cfg());
        s.next_tab();
        assert_eq!(
            s.tab(),
            SettingsTab::Thresholds,
            "tab must not move while an edit buffer is active"
        );
    }

    #[test]
    fn settings_tab_index_at_uses_rendered_label_boundaries() {
        // v1.51.0: labels now carry a numeric prefix ("1 Thresholds",
        // "2 Integrations", …) so the keyboard shortcut is visible
        // inside the tab strip itself. Hit-test column offsets shift
        // accordingly: each label gains 2 cells (digit + space).
        let tabs = Rect::new(10, 2, 80, 3);
        let inner_x = tabs.x + 1;

        // Layout (relative to inner_x):
        //   [0,13]  "1 Thresholds"  (12 chars + 2 padding)
        //   14      `│` divider
        //   [15,30] "2 Integrations" (14 chars + 2 padding)
        //   31      `│` divider
        //   [32,45] "3 Parameters"  (12 chars + 2 padding)
        //   46      `║` divider (editable→reference seam)
        //   [47,55] "4 Rules"       (7 chars + 2 padding)
        //   56      `│` divider
        //   [57,66] "5 Badges"      (8 chars + 2 padding)
        assert_eq!(settings_tab_index_at(tabs, inner_x + 2), Some(0));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 20), Some(1));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 35), Some(2));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 50), Some(3));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 60), Some(4));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 80), None);
        assert_eq!(settings_tab_index_at(tabs, tabs.x - 1), None);
    }

    #[test]
    fn settings_tab_strip_renders_inter_group_seam_between_parameters_and_rules() {
        // v1.51.0: tab strip uses `│` between same-group tabs and `║`
        // at the editable/reference seam. Pin both glyphs so a future
        // refactor can't silently drop the visual cue.
        let line = settings_tab_strip_line(SettingsTab::Thresholds);
        let rendered: String = line
            .spans
            .iter()
            .map(|s| s.content.clone().into_owned())
            .collect();
        assert!(rendered.contains("3 Parameters "), "rendered: {rendered}");
        assert!(
            rendered.contains("║"),
            "inter-group seam missing: {rendered}"
        );
        assert!(
            rendered.contains("│"),
            "intra-group divider missing: {rendered}"
        );
        // The seam must sit between Parameters and Rules — i.e., the
        // last `║` should appear before "4 Rules" and after "Parameters".
        let seam_at = rendered.find('║').expect("seam present");
        let params_at = rendered.find("Parameters").expect("Parameters present");
        let rules_at = rendered.find("Rules").expect("Rules present");
        assert!(seam_at > params_at, "seam must follow Parameters");
        assert!(seam_at < rules_at, "seam must precede Rules");
    }

    #[test]
    fn integration_tab_toggles_provider_setup_values() {
        let mut s = SettingsOverlay::new();
        let mut config = cfg();
        s.open();
        s.switch_tab(SettingsTab::Integrations);

        assert_eq!(s.selected_integration(), IntegrationField::ClaudeSidefile);
        let before_sidefile = config.provider_setup.claude_sidefile;
        s.toggle_integration(&mut config);
        assert_ne!(config.provider_setup.claude_sidefile, before_sidefile);
        assert!(s.is_dirty());
        assert!(matches!(s.status(), SettingsStatus::Dirty));

        s.next_integration();
        assert_eq!(s.selected_integration(), IntegrationField::CodexAppServer);
        let before_server = config.provider_setup.codex_app_server;
        s.toggle_integration(&mut config);
        assert_ne!(config.provider_setup.codex_app_server, before_server);
    }

    #[test]
    fn settings_integration_field_at_maps_body_rows() {
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = settings_modal_rects(viewport);
        let inner = rects.body.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });

        assert_eq!(
            settings_integration_field_at(rects.body, inner.x + 5, inner.y + 1),
            Some(IntegrationField::ClaudeSidefile)
        );
        assert_eq!(
            settings_integration_field_at(rects.body, inner.x + 5, inner.y + 2),
            Some(IntegrationField::CodexAppServer)
        );
        assert_eq!(
            settings_integration_field_at(rects.body, inner.x + 5, inner.y),
            None
        );
    }

    #[test]
    fn integrations_tab_shows_provider_setup_values_and_guidance() {
        let mut config = cfg();
        config.provider_setup.claude_sidefile = false;
        config.provider_setup.codex_app_server = true;
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Integrations);

        let rendered = rendered_text(&build_body_lines(&s, &config));

        assert!(rendered.contains("[provider_setup]"));
        assert!(rendered.contains("Claude sidefile export"));
        assert!(rendered.contains("OFF"));
        assert!(rendered.contains("Codex app-server"));
        assert!(rendered.contains("ON"));
        assert!(rendered.contains("Provider Setup (P) is read-only"));
        assert!(rendered.contains("press w to write TOML"));
    }

    #[test]
    fn parameters_tab_shows_custom_policy_inputs() {
        let mut config = cfg();
        config.token.quota_tight = true;
        config.cache.drift_min_samples = 6;
        config.reset.wait_eta_secs = 15 * 60;
        config.reset.snapshot_pressure_threshold = 0.65;
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);

        let rendered = rendered_text(&build_body_lines(&s, &config));

        assert!(rendered.contains("token quota_tight"));
        assert!(rendered.contains("on"));
        assert!(rendered.contains("default off"));
        assert!(rendered.contains("cache drift drop/samples"));
        assert!(rendered.contains("30%/6"));
        assert!(rendered.contains("reset wait pressure/eta"));
        assert!(rendered.contains("85%/15m"));
        assert!(rendered.contains("reset snapshot pressure/eta"));
        assert!(rendered.contains("65%/5m"));
    }

    #[test]
    fn parameters_tab_shows_hover_help_settings() {
        let mut config = cfg();
        config.ux.hover_help = false;
        config.ux.help_language = HelpLanguage::En;
        config.ux.hover_help_trigger = crate::app::config::HoverHelpTrigger::Row;
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);

        let rendered = rendered_text(&build_body_lines(&s, &config));

        assert!(rendered.contains("ux hover/language/trigger"));
        assert!(rendered.contains("off/en/row"));
        assert!(rendered.contains("default on/ko/label"));
    }

    #[test]
    fn parameters_tab_explains_selected_hover_help_parameter() {
        let config = cfg();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        s.select_parameter(ParameterField::UxHoverHelp);

        let rendered = rendered_text(&build_body_lines(&s, &config));

        assert!(rendered.contains("Selected parameter help"));
        assert!(rendered.contains("TOML: ux.hover_help"));
        assert!(rendered.contains("Shortcut: H"));
        assert!(rendered.contains("floating help"));
    }

    #[test]
    fn parameters_hint_mentions_hover_help_keys() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);

        let rendered = rendered_text(&hint_lines(&s));

        assert!(rendered.contains("H hover"));
        assert!(rendered.contains("L language"));
    }

    #[test]
    fn parameters_tab_uses_side_help_panel_on_wide_modal() {
        let config = cfg();
        let viewport = Rect::new(0, 0, 160, 44);
        let body = settings_modal_rects(viewport).body;
        let rects =
            settings_parameter_side_panel_rects(body).expect("wide Parameters body should split");
        assert!(rects.help.x > rects.list.x.saturating_add(rects.list.width));

        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        s.select_parameter(ParameterField::UxHoverHelp);

        let list_text = rendered_text(&build_parameter_list_lines(&s, &config));
        let help_text = rendered_text(&build_parameter_help_panel_lines(&s, &config));

        assert!(
            !list_text.contains("Selected parameter help"),
            "side-panel list must not inline the selected parameter help"
        );
        assert!(help_text.contains("Selected parameter help"));
        assert!(help_text.contains("TOML: ux.hover_help"));
    }

    #[test]
    fn settings_hint_reports_scroll_progress_and_end() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);

        let mid = rendered_text(&hint_lines_with_scroll(&s, Some((1, 3))));
        assert!(mid.contains("scroll 1/3"));
        assert!(mid.contains("more"));

        let end = rendered_text(&hint_lines_with_scroll(&s, Some((3, 3))));
        assert!(end.contains("scroll 3/3"));
        assert!(end.contains("END"));
    }

    #[test]
    fn parameters_scroll_end_uses_last_selectable_row_not_status_footer() {
        let config = cfg();
        let viewport = Rect::new(0, 0, 160, 44);
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        let last = *all_parameter_fields()
            .last()
            .expect("Parameters must expose at least one editable field");
        s.select_parameter(last);

        let visible = settings_parameter_visible_rows_for_viewport(viewport).max(1);
        let last_row = settings_parameter_field_line_index(&s, &config, last)
            .expect("last editable parameter must have a rendered row");
        let selectable_end = last_row.saturating_sub(visible.saturating_sub(1));

        assert_eq!(
            settings_max_scroll(&s, &config, viewport),
            selectable_end,
            "Parameters scroll end should match the last selectable row, not trailing blank/status rows"
        );
    }

    #[test]
    fn parameters_tab_exposes_editable_fields_for_all_config_sections() {
        let fields = all_parameter_fields();

        assert!(fields.contains(&ParameterField::TmuxSource));
        assert!(fields.contains(&ParameterField::ActionsMode));
        assert!(fields.contains(&ParameterField::RefreshPolicy));
        assert!(fields.contains(&ParameterField::LoggingSensitivity));
        assert!(fields.contains(&ParameterField::TokenQuotaTight));
        assert!(fields.contains(&ParameterField::StorageRoot));
        assert!(fields.contains(&ParameterField::InsightsIgnoredTtlSecs));
        assert!(fields.contains(&ParameterField::IdleStillnessPolls));
        assert!(fields.contains(&ParameterField::SecurityPostureAdvisories));
        assert!(fields.contains(&ParameterField::CacheHotRatioThreshold));
        assert!(fields.contains(&ParameterField::ResetAutoSnapshot));
        assert!(fields.contains(&ParameterField::AnomalyEnabled));
        assert!(fields.contains(&ParameterField::AnomalyPromoteSubagentSideEffect));
        assert!(fields.contains(&ParameterField::CostBudgetUsd));
        assert!(fields.contains(&ParameterField::ProfileSwitchWindowPolls));
        assert!(fields.contains(&ParameterField::UxConfirmActions));
        assert!(fields.contains(&ParameterField::UxHoverHelp));
        assert!(fields.contains(&ParameterField::UxHoverHelpTrigger));
        assert!(fields.contains(&ParameterField::UxHelpLanguage));
    }

    #[test]
    fn parameter_bool_enum_number_and_string_edits_update_config() {
        let mut s = SettingsOverlay::new();
        let mut config = cfg();
        s.open();
        s.switch_tab(SettingsTab::Parameters);

        // v1.51.0: anomaly.enabled default flipped to true; force a
        // known starting state so this test exercises the toggle
        // mechanic regardless of which way the default points.
        config.anomaly.enabled = false;
        s.select_parameter(ParameterField::AnomalyEnabled);
        s.activate_parameter(&mut config).expect("bool toggle");
        assert!(config.anomaly.enabled);

        s.select_parameter(ParameterField::AnomalyMinConfidence);
        s.activate_parameter(&mut config).expect("enum cycle");
        assert_eq!(config.anomaly.min_confidence, "high");

        s.select_parameter(ParameterField::ResetWaitEtaSecs);
        s.start_edit(&config);
        s.replace_edit_buffer_for_test("900");
        s.commit_edit(&mut config).expect("number edit");
        assert_eq!(config.reset.wait_eta_secs, 900);

        s.select_parameter(ParameterField::StorageRoot);
        s.start_edit(&config);
        s.replace_edit_buffer_for_test("/tmp/qmonster-root");
        s.commit_edit(&mut config).expect("string edit");
        assert_eq!(config.storage.root.as_deref(), Some("/tmp/qmonster-root"));
    }

    #[test]
    fn save_writes_parameter_values_into_existing_toml() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("qmonster.toml");
        let original = "# header\n[anomaly]\nenabled = false\n\n[reset]\nwait_eta_secs = 1800\n";
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(original.as_bytes()))
            .expect("write seed");

        let mut s = SettingsOverlay::new();
        let mut config = cfg();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        // v1.51.0: anomaly.enabled default flipped to true; toggling
        // from a known false starts the test consistently regardless
        // of the default's polarity.
        config.anomaly.enabled = false;
        s.select_parameter(ParameterField::AnomalyEnabled);
        s.activate_parameter(&mut config).expect("toggle enabled");
        s.select_parameter(ParameterField::ResetWaitEtaSecs);
        s.start_edit(&config);
        s.replace_edit_buffer_for_test("900");
        s.commit_edit(&mut config).expect("commit eta");

        s.save(&config, &path).expect("save ok");

        let saved = std::fs::read_to_string(&path).expect("read back");
        assert!(saved.contains("# header"));
        assert!(saved.contains("enabled = true"));
        assert!(saved.contains("wait_eta_secs = 900"));
        let reloaded: QmonsterConfig = toml::from_str(&saved).expect("parse");
        assert!(reloaded.anomaly.enabled);
        assert_eq!(reloaded.reset.wait_eta_secs, 900);
    }

    #[test]
    fn rules_tab_shows_dynamic_activation_conditions() {
        let mut config = cfg();
        config.cache.hot_ratio_threshold = 0.7;
        config.reset.wait_pressure_threshold = 0.75;
        config.reset.wait_eta_secs = 10 * 60;
        config.reset.snapshot_eta_secs = 90;
        config.security.identity_drift_findings = true;
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Rules);

        let rendered = rendered_text(&build_body_lines(&s, &config));

        assert!(rendered.contains("cache hot wait"));
        assert!(rendered.contains("cache > 70%"));
        assert!(rendered.contains("wait for reset"));
        assert!(rendered.contains("5h/weekly pressure >= 75% and eta <= 10m"));
        assert!(rendered.contains("snapshot before reset"));
        assert!(rendered.contains("any eta <= 1m30s and pressure >= 50%"));
        assert!(rendered.contains("reset sources"));
        assert!(rendered.contains("identity drift"));
        assert!(rendered.contains("security.identity_drift_findings = on"));
        // Phase H: annotation telling operators the rule has an actuation hook
        assert!(rendered.contains("auto when [reset] auto_snapshot = true"));
    }

    #[test]
    fn parameters_tab_shows_insights_config() {
        let mut config = cfg();
        config.insights.ignored_ttl_secs = 600;
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);

        let rendered = rendered_text(&build_body_lines(&s, &config));

        assert!(rendered.contains("insights ignored_ttl/default_window"));
        assert!(rendered.contains("10m/24h"));
        assert!(rendered.contains("default 30m/24h"));
    }

    #[test]
    fn rules_tab_shows_insights_ttl_rule() {
        let mut config = cfg();
        config.insights.ignored_ttl_secs = 45;
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Rules);

        let rendered = rendered_text(&build_body_lines(&s, &config));

        assert!(rendered.contains("insights ttl ignored"));
        assert!(rendered.contains("ignored after 45s without correlated outcome"));
    }

    #[test]
    fn parameters_tab_shows_reset_auto_snapshot_row() {
        // default: auto_snapshot = false
        let config_default = cfg();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        let rendered_default = rendered_text(&build_body_lines(&s, &config_default));
        assert!(
            rendered_default.contains("reset auto_snapshot"),
            "Parameters tab must have a reset auto_snapshot row"
        );
        assert!(
            rendered_default.contains("off"),
            "default auto_snapshot must render as 'off'"
        );

        // non-default: auto_snapshot = true
        let mut config_on = cfg();
        config_on.reset.auto_snapshot = true;
        let rendered_on = rendered_text(&build_body_lines(&s, &config_on));
        assert!(
            rendered_on.contains("reset auto_snapshot"),
            "Parameters tab must have a reset auto_snapshot row when enabled"
        );
        assert!(
            rendered_on.contains("on"),
            "enabled auto_snapshot must render as 'on'"
        );
        assert!(
            rendered_on.contains("default off"),
            "enabled auto_snapshot must show 'default off'"
        );
    }

    #[test]
    fn badges_tab_explains_cost_context_and_source_labels() {
        let s = {
            let mut s = SettingsOverlay::new();
            s.open();
            s.switch_tab(SettingsTab::Badges);
            s
        };

        let rendered = rendered_text(&build_body_lines(&s, &cfg()));

        assert!(rendered.contains("[Official]"));
        assert!(rendered.contains("CTX N%"));
        assert!(rendered.contains("context window used, not remaining"));
        assert!(rendered.contains("COST $N"));
        assert!(rendered.contains("Claude sidefile total_cost_usd"));
        assert!(rendered.contains("Codex/Gemini cost is Estimate"));
        assert!(rendered.contains("pricing.toml"));
        assert!(rendered.contains("zero-rate entries keep COST hidden"));
        assert!(rendered.contains("CALLS N"));
        assert!(rendered.contains("RESET model"));
    }

    // -----------------------------------------------------------------
    // Cluster B: read_field / effective_field.
    // -----------------------------------------------------------------

    #[test]
    fn read_field_returns_default_warning_for_cost() {
        let s = SettingsOverlay::new();
        let v = s.read_field(
            &cfg(),
            FieldId::new(Section::Cost, Scope::Default, Bound::Warning),
        );
        assert_eq!(v, Some(5.0));
    }

    #[test]
    fn read_field_returns_none_for_unset_per_provider_override() {
        let s = SettingsOverlay::new();
        let v = s.read_field(
            &cfg(),
            FieldId::new(Section::Context, Scope::Codex, Bound::Warning),
        );
        assert_eq!(
            v, None,
            "ContextConfig::default() leaves codex unset → read_field is None"
        );
    }

    #[test]
    fn effective_field_falls_through_to_default_when_unset() {
        let s = SettingsOverlay::new();
        let v = s.effective_field(
            &cfg(),
            FieldId::new(Section::Context, Scope::Codex, Bound::Warning),
        );
        assert!(
            (v - 0.75).abs() < 1e-6,
            "unset codex inherits 0.75 from default"
        );
    }

    #[test]
    fn quota_fields_split_claude_and_codex_5h_weekly_scopes() {
        let fields = all_fields();
        assert!(fields.contains(&FieldId::new(
            Section::Quota,
            Scope::Claude5h,
            Bound::Warning
        )));
        assert!(fields.contains(&FieldId::new(
            Section::Quota,
            Scope::ClaudeWeekly,
            Bound::Warning
        )));
        assert!(fields.contains(&FieldId::new(
            Section::Quota,
            Scope::Codex5h,
            Bound::Warning
        )));
        assert!(fields.contains(&FieldId::new(
            Section::Quota,
            Scope::CodexWeekly,
            Bound::Warning
        )));
        assert!(!fields.contains(&FieldId::new(
            Section::Cost,
            Scope::Claude5h,
            Bound::Warning
        )));
        assert_eq!(fields.len(), 28);
    }

    // -----------------------------------------------------------------
    // Cluster C: edit buffer / type_char / backspace / cancel.
    // -----------------------------------------------------------------

    #[test]
    fn start_edit_initializes_buffer_with_effective_value() {
        let mut s = SettingsOverlay::new();
        s.open();
        // Select first field (cost / default / warning) — value is 5.0.
        s.start_edit(&cfg());
        assert_eq!(s.edit_buffer(), Some("5"));
    }

    #[test]
    fn type_char_appends_digits_and_dot_only() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.start_edit(&cfg());
        // Buffer starts at "5"; clear then type "12.34a.5" expecting
        // "12.345" (the second '.' and 'a' are dropped).
        s.cancel_edit();
        s.edit_buffer = Some(String::new());
        for c in "12.34a.5".chars() {
            s.type_char(c);
        }
        assert_eq!(s.edit_buffer(), Some("12.345"));
    }

    #[test]
    fn backspace_pops_one_char() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.edit_buffer = Some("5.50".into());
        s.backspace();
        assert_eq!(s.edit_buffer(), Some("5.5"));
    }

    #[test]
    fn cancel_edit_clears_buffer_without_changing_config() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.start_edit(&cfg());
        s.type_char('9');
        s.cancel_edit();
        assert_eq!(s.edit_buffer(), None);
        // Verify config is unchanged via read_field.
        assert_eq!(
            s.read_field(
                &cfg(),
                FieldId::new(Section::Cost, Scope::Default, Bound::Warning),
            ),
            Some(5.0)
        );
    }

    // -----------------------------------------------------------------
    // Cluster D: commit_edit + validation.
    // -----------------------------------------------------------------

    #[test]
    fn commit_valid_cost_default_warning_updates_config() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.start_edit(&cfg());
        s.cancel_edit();
        s.edit_buffer = Some("7.5".into());
        let mut config = cfg();
        s.commit_edit(&mut config).expect("valid edit must commit");
        assert!((config.cost.warning_usd - 7.5).abs() < f64::EPSILON);
        assert!(s.is_dirty());
    }

    #[test]
    fn commit_unparseable_buffer_returns_error() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.edit_buffer = Some("abc".into());
        let mut config = cfg();
        let err = s.commit_edit(&mut config).unwrap_err();
        assert!(err.contains("not a number"), "got: {err}");
        assert!(matches!(s.status(), SettingsStatus::Error(_)));
        // Buffer must survive a validation failure so the operator can
        // see what they typed and either correct or cancel.
        assert!(s.edit_buffer().is_some());
    }

    #[test]
    fn commit_pct_outside_unit_range_returns_error() {
        let mut s = SettingsOverlay::new();
        s.open();
        // Walk to context / default / warning (8 next_field steps to skip
        // the 8 cost fields).
        for _ in 0..8 {
            s.next_field();
        }
        assert_eq!(
            s.selected(),
            FieldId::new(Section::Context, Scope::Default, Bound::Warning)
        );
        s.edit_buffer = Some("1.5".into());
        let mut config = cfg();
        let err = s.commit_edit(&mut config).unwrap_err();
        assert!(err.contains("[0.0, 1.0]"), "got: {err}");
    }

    #[test]
    fn commit_warning_above_critical_returns_error() {
        let mut s = SettingsOverlay::new();
        s.open();
        // First field is cost / default / warning. Default critical is 20.
        // Try to set warning to 25 → should reject.
        s.edit_buffer = Some("25".into());
        let mut config = cfg();
        let err = s.commit_edit(&mut config).unwrap_err();
        assert!(err.contains("warning"), "got: {err}");
        assert!(err.contains("critical"), "got: {err}");
    }

    #[test]
    fn commit_negative_cost_returns_error() {
        let mut s = SettingsOverlay::new();
        s.open();
        s.edit_buffer = Some("-1".into());
        let mut config = cfg();
        let err = s.commit_edit(&mut config).unwrap_err();
        assert!(err.contains("cost"), "got: {err}");
    }

    #[test]
    fn commit_creates_provider_override_when_editing_unset_provider_field() {
        let mut s = SettingsOverlay::new();
        s.open();
        // Walk to cost / claude / warning (2 next_field steps).
        s.next_field();
        s.next_field();
        assert_eq!(
            s.selected(),
            FieldId::new(Section::Cost, Scope::Claude, Bound::Warning)
        );
        let mut config = cfg();
        // Default cost config sets claude warning_usd = 10.0; we change it.
        s.edit_buffer = Some("12.5".into());
        s.commit_edit(&mut config).expect("valid edit must commit");
        let claude = config
            .cost
            .claude
            .as_ref()
            .expect("claude override present");
        assert!((claude.warning_usd - 12.5).abs() < f64::EPSILON);
        // Critical stays at the default-loaded 30.0 for claude.
        assert!((claude.critical_usd - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn commit_quota_codex_weekly_creates_split_override_only() {
        let mut s = SettingsOverlay::new();
        s.open();
        while s.selected() != FieldId::new(Section::Quota, Scope::CodexWeekly, Bound::Warning) {
            s.next_field();
        }
        let mut config = cfg();
        s.edit_buffer = Some("0.66".into());
        s.commit_edit(&mut config).expect("valid edit must commit");

        assert!(config.quota.codex_5h.is_none());
        let weekly = config
            .quota
            .codex_weekly
            .as_ref()
            .expect("weekly override present");
        assert!((weekly.warning_pct - 0.66).abs() < f32::EPSILON);
        assert!((weekly.critical_pct - config.quota.critical_pct).abs() < f32::EPSILON);
    }

    // -----------------------------------------------------------------
    // Cluster E: clear_override.
    // -----------------------------------------------------------------

    #[test]
    fn clear_override_on_provider_field_restores_default() {
        let mut s = SettingsOverlay::new();
        s.open();
        // Cost / claude / warning has an override out of the box (10.0).
        s.next_field(); // critical
        s.next_field(); // claude warning
        let mut config = cfg();
        assert!(config.cost.claude.is_some(), "default has claude override");
        s.clear_override(&mut config);
        assert!(
            config.cost.claude.is_none(),
            "clear must drop the entire claude provider override"
        );
        assert!(s.is_dirty());
        let v = s.effective_field(
            &config,
            FieldId::new(Section::Cost, Scope::Claude, Bound::Warning),
        );
        assert!(
            (v - config.cost.warning_usd).abs() < f64::EPSILON,
            "claude warning must now read the section default"
        );
    }

    #[test]
    fn clear_override_on_default_field_is_noop() {
        let mut s = SettingsOverlay::new();
        s.open(); // first field is default scope
        let mut config = cfg();
        s.clear_override(&mut config);
        assert!(!s.is_dirty(), "default-scope clear must not mark dirty");
    }

    // -----------------------------------------------------------------
    // Cluster F: save.
    // -----------------------------------------------------------------

    #[test]
    fn save_writes_toml_to_path_and_marks_clean() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("qmonster.toml");
        // Pre-create the file so write replaces it (not strictly needed,
        // but mirrors the real flow where the operator's file already exists).
        let _ = std::fs::File::create(&path).and_then(|mut f| f.write_all(b"# placeholder\n"));
        let mut s = SettingsOverlay::new();
        s.open();
        let mut config = cfg();
        // Make a small edit so dirty=true.
        s.edit_buffer = Some("6".into());
        s.commit_edit(&mut config).expect("commit ok");
        assert!(s.is_dirty());
        s.save(&config, &path).expect("save ok");
        assert!(!s.is_dirty(), "save clears dirty flag");
        assert!(matches!(s.status(), SettingsStatus::Saved(_)));
        // Round-trip parse to confirm the new value is on disk.
        let raw = std::fs::read_to_string(&path).expect("read back");
        let reloaded: QmonsterConfig = toml::from_str(&raw).expect("parse");
        assert!((reloaded.cost.warning_usd - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn save_writes_dirty_hover_help_parameters() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("qmonster.toml");
        let original = "[ux]\nconfirm_actions = \"always\"\n";
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(original.as_bytes()))
            .expect("write seed");

        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        let mut config = cfg();
        s.select_parameter(ParameterField::UxHoverHelp);
        s.activate_parameter(&mut config)
            .expect("toggle hover help");
        s.select_parameter(ParameterField::UxHelpLanguage);
        s.activate_parameter(&mut config).expect("toggle language");
        s.select_parameter(ParameterField::UxHoverHelpTrigger);
        s.activate_parameter(&mut config).expect("toggle trigger");

        s.save(&config, &path).expect("save ok");

        let raw = std::fs::read_to_string(&path).expect("read back");
        let reloaded: QmonsterConfig = toml::from_str(&raw).expect("parse");
        assert!(!reloaded.ux.hover_help);
        assert_eq!(reloaded.ux.help_language, HelpLanguage::En);
        assert_eq!(
            reloaded.ux.hover_help_trigger,
            crate::app::config::HoverHelpTrigger::Row
        );
        assert!(raw.contains("hover_help = false"), "raw: {raw}");
        assert!(raw.contains("help_language = \"en\""), "raw: {raw}");
        assert!(raw.contains("hover_help_trigger = \"row\""), "raw: {raw}");
    }

    #[test]
    fn save_preserves_top_level_comments_in_existing_file() {
        // Phase E E2 (v1.20.0): the operator's hand-written
        // `qmonster.toml` may begin with explanatory comments that
        // describe why specific thresholds were chosen. The previous
        // `toml::to_string_pretty` save path stripped them; this test
        // locks the new contract — top-level comments must survive
        // a round-trip through the settings overlay.
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("qmonster.toml");
        let original = "# Qmonster operator config\n# Edited by ops 2026-04-28\n\n[cost]\nwarning_usd = 5.0\ncritical_usd = 20.0\n";
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(original.as_bytes()))
            .expect("write seed");

        let mut s = SettingsOverlay::new();
        s.open();
        let mut config = cfg();
        // Bump the cost warning so save() actually writes a value.
        s.edit_buffer = Some("7.5".into());
        s.commit_edit(&mut config).expect("commit ok");

        s.save(&config, &path).expect("save ok");

        let saved = std::fs::read_to_string(&path).expect("read back");
        assert!(
            saved.contains("# Qmonster operator config"),
            "leading comment must survive save: {saved}"
        );
        assert!(
            saved.contains("# Edited by ops 2026-04-28"),
            "second comment line must survive save: {saved}"
        );
    }

    #[test]
    fn save_preserves_unrelated_section_comments_and_keys() {
        // Settings overlay edits its owned sections and preserves any
        // other section the operator has authored — `[tmux]`,
        // `[security]`, `[idle]`, custom comments, key order — after
        // save.
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("qmonster.toml");
        let original = "[cost]\nwarning_usd = 5.0\ncritical_usd = 20.0\n\n# tmux source preference; auto picks control-mode first\n[tmux]\nsource = \"control_mode\"\n\n[security]\n# raise this when a remote pair-programmer joins\nposture_advisories = true\n";
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(original.as_bytes()))
            .expect("write seed");

        let mut s = SettingsOverlay::new();
        s.open();
        let mut config = cfg();
        s.edit_buffer = Some("8".into());
        s.commit_edit(&mut config).expect("commit ok");
        s.save(&config, &path).expect("save ok");

        let saved = std::fs::read_to_string(&path).expect("read back");
        assert!(
            saved.contains("# tmux source preference; auto picks control-mode first"),
            "section comment must survive: {saved}"
        );
        assert!(
            saved.contains("source = \"control_mode\""),
            "[tmux] body unrelated to overlay must survive: {saved}"
        );
        assert!(
            saved.contains("# raise this when a remote pair-programmer joins"),
            "inline section comment must survive: {saved}"
        );
        assert!(
            saved.contains("posture_advisories = true"),
            "[security] body must survive: {saved}"
        );
    }

    #[test]
    fn save_updates_target_threshold_value_in_existing_file() {
        // The whole point of preserving the file is to also still
        // write the operator's edits. Lock the round-trip: the new
        // value reaches disk under the existing key path.
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("qmonster.toml");
        let original = "# header\n[cost]\nwarning_usd = 5.0\ncritical_usd = 20.0\n";
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(original.as_bytes()))
            .expect("write seed");

        let mut s = SettingsOverlay::new();
        s.open();
        let mut config = cfg();
        s.edit_buffer = Some("9.25".into());
        s.commit_edit(&mut config).expect("commit ok");
        s.save(&config, &path).expect("save ok");

        let saved = std::fs::read_to_string(&path).expect("read back");
        assert!(
            saved.contains("warning_usd = 9.25") || saved.contains("warning_usd = 9.25"),
            "new value must be present: {saved}"
        );
        let reloaded: QmonsterConfig = toml::from_str(&saved).expect("parse");
        assert!((reloaded.cost.warning_usd - 9.25).abs() < f64::EPSILON);
    }

    #[test]
    fn save_updates_provider_setup_values_in_existing_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("qmonster.toml");
        let original =
            "# header\n[provider_setup]\nclaude_sidefile = true\ncodex_app_server = false\n";
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(original.as_bytes()))
            .expect("write seed");

        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Integrations);
        let mut config = cfg();
        config.provider_setup.claude_sidefile = false;
        config.provider_setup.codex_app_server = true;

        s.save(&config, &path).expect("save ok");

        let saved = std::fs::read_to_string(&path).expect("read back");
        assert!(
            saved.contains("# header"),
            "leading comment must survive: {saved}"
        );
        assert!(
            saved.contains("claude_sidefile = false"),
            "sidefile setting must be written: {saved}"
        );
        assert!(
            saved.contains("codex_app_server = true"),
            "app-server setting must be written: {saved}"
        );
        let reloaded: QmonsterConfig = toml::from_str(&saved).expect("parse");
        assert!(!reloaded.provider_setup.claude_sidefile);
        assert!(reloaded.provider_setup.codex_app_server);
    }

    #[test]
    fn save_falls_back_to_pretty_serialize_when_file_does_not_exist() {
        // Fresh write path — no existing file, no comments to
        // preserve. The legacy `toml::to_string_pretty` scaffold
        // continues to work so an operator with no prior config
        // gets a sensible starting file.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("qmonster.toml");
        assert!(!path.exists());

        let mut s = SettingsOverlay::new();
        s.open();
        let mut config = cfg();
        s.edit_buffer = Some("11".into());
        s.commit_edit(&mut config).expect("commit ok");
        s.save(&config, &path).expect("save ok");

        let saved = std::fs::read_to_string(&path).expect("read back");
        let reloaded: QmonsterConfig = toml::from_str(&saved).expect("parse");
        assert!((reloaded.cost.warning_usd - 11.0).abs() < f64::EPSILON);
    }

    #[test]
    fn save_with_invalid_pair_returns_error_without_writing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("qmonster.toml");
        let mut s = SettingsOverlay::new();
        s.open();
        // Direct mutation of the config — bypass commit's per-field
        // validation to simulate a corrupted-on-disk start state, then
        // verify save() catches it on the second-pass full validation.
        let mut config = cfg();
        config.cost.warning_usd = 99.0; // > critical 20
        let err = s.save(&config, &path).unwrap_err();
        assert!(err.contains("warning"), "got: {err}");
        assert!(err.contains("critical"), "got: {err}");
        assert!(!path.exists(), "save must not write when validation fails");
    }

    // -----------------------------------------------------------------
    // Cluster G: close-button hit-test (v1.15.19).
    // -----------------------------------------------------------------

    #[test]
    fn close_button_rect_sits_at_top_right_of_body() {
        // Body of width 80 → close button starts at column 76 (80 - 4).
        let body = Rect::new(10, 5, 80, 20);
        let rect = settings_close_button_rect(body);
        assert_eq!(rect.x, 10 + 80 - 4);
        assert_eq!(rect.y, 5);
        assert_eq!(rect.width, 3);
        assert_eq!(rect.height, 1);
    }

    #[test]
    fn close_button_rect_stays_inside_body_bounds() {
        let body = Rect::new(0, 0, 60, 30);
        let rect = settings_close_button_rect(body);
        assert!(rect.x + rect.width <= body.x + body.width);
        assert!(rect.y + rect.height <= body.y + body.height);
    }

    #[test]
    fn close_button_rect_clamps_to_tiny_body() {
        // Degenerate body smaller than the [x] glyph — the rect must still
        // fit inside without underflowing.
        let body = Rect::new(0, 0, 2, 1);
        let rect = settings_close_button_rect(body);
        assert!(rect.x + rect.width <= body.x + body.width);
        assert!(rect.y + rect.height <= body.y + body.height);
    }

    // -----------------------------------------------------------------
    // Cluster: Phase 7 anomaly settings rows
    // -----------------------------------------------------------------

    #[test]
    fn parameters_tab_shows_anomaly_section() {
        let mut config = QmonsterConfig::defaults();
        config.anomaly.enabled = true;
        config.anomaly.window_polls = 25;
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        let lines = build_body_lines(&s, &config);
        let rendered = rendered_text(&lines);
        assert!(
            rendered.contains("anomaly enabled"),
            "missing enabled row: {rendered}"
        );
        assert!(
            rendered.contains("on"),
            "expected on for enabled=true: {rendered}"
        );
        assert!(
            rendered.contains("anomaly window_polls"),
            "missing window_polls row: {rendered}"
        );
        assert!(
            rendered.contains("25"),
            "expected 25 for window_polls: {rendered}"
        );
    }

    #[test]
    fn rules_tab_shows_four_anomaly_rows() {
        let config = QmonsterConfig::defaults();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Rules);
        let lines = build_body_lines(&s, &config);
        let rendered = rendered_text(&lines);
        assert!(
            rendered.contains("anomaly: IdentityChurn"),
            "missing IdentityChurn: {rendered}"
        );
        assert!(
            rendered.contains("anomaly: ErrorBurst"),
            "missing ErrorBurst: {rendered}"
        );
        assert!(
            rendered.contains("anomaly: CacheDiscontinuity"),
            "missing CacheDiscontinuity: {rendered}"
        );
        assert!(
            rendered.contains("anomaly: CrossPaneEditCluster"),
            "missing CrossPaneEditCluster: {rendered}"
        );
    }

    #[test]
    fn badges_tab_includes_anomalies_description() {
        let config = QmonsterConfig::defaults();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Badges);
        let lines = build_body_lines(&s, &config);
        let rendered = rendered_text(&lines);
        assert!(
            rendered.contains("ANOMALIES"),
            "missing ANOMALIES badge: {rendered}"
        );
    }

    #[test]
    fn rules_tab_anomaly_rows_show_promotion_annotation() {
        let config = QmonsterConfig::defaults();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Rules);
        let lines = build_body_lines(&s, &config);
        let rendered = rendered_text(&lines);
        let expected_phrase = "promotes to Recommendation when confidence >= high";
        let count = rendered.matches(expected_phrase).count();
        assert_eq!(
            count, 7,
            "expected 7 anomaly rules with promotion annotation; got {count}: {rendered}"
        );
    }

    #[test]
    fn parameters_tab_shows_v2_anomaly_thresholds() {
        let config = QmonsterConfig::defaults();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        let lines = build_body_lines(&s, &config);
        let rendered = rendered_text(&lines);
        assert!(
            rendered.contains("anomaly cost_slope_usd_per_hour"),
            "missing cost_slope_usd_per_hour row: {rendered}"
        );
        assert!(
            rendered.contains("20.00"),
            "expected 20.00 for cost_slope_usd_per_hour: {rendered}"
        );
        assert!(
            rendered.contains("anomaly token_slope_input_per_poll"),
            "missing token_slope_input_per_poll row: {rendered}"
        );
        assert!(
            rendered.contains("20000"),
            "expected 20000 for token_slope_input_per_poll: {rendered}"
        );
        assert!(
            rendered.contains("anomaly memory_growth_mb"),
            "missing memory_growth_mb row: {rendered}"
        );
        assert!(
            rendered.contains("1024"),
            "expected 1024 for memory_growth_mb: {rendered}"
        );
    }

    #[test]
    fn rules_tab_shows_v2_anomaly_rows() {
        let config = QmonsterConfig::defaults();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Rules);
        let lines = build_body_lines(&s, &config);
        let rendered = rendered_text(&lines);
        assert!(
            rendered.contains("anomaly: CostSlope"),
            "missing CostSlope: {rendered}"
        );
        assert!(
            rendered.contains("anomaly: TokenSlope"),
            "missing TokenSlope: {rendered}"
        );
        assert!(
            rendered.contains("anomaly: MemoryGrowth"),
            "missing MemoryGrowth: {rendered}"
        );
        assert!(
            rendered.contains("anomaly: SubagentSideEffect"),
            "missing SubagentSideEffect: {rendered}"
        );
    }

    #[test]
    fn parameters_tab_includes_anomaly_promote_section() {
        let config = QmonsterConfig::defaults();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        let lines = build_body_lines(&s, &config);
        let rendered = rendered_text(&lines);
        assert!(
            rendered.contains("Anomaly Promote (per-kind threshold)"),
            "section header missing: {rendered}"
        );
        assert!(
            rendered.contains("anomaly.promote subagent_side_effect"),
            "subagent_side_effect row missing"
        );
        assert!(
            rendered.contains("anomaly.promote cost_slope"),
            "cost_slope row missing"
        );
    }

    #[test]
    fn rules_tab_anomaly_rows_use_configured_threshold_value() {
        let config = QmonsterConfig::defaults();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Rules);
        let lines = build_body_lines(&s, &config);
        let rendered = rendered_text(&lines);
        // 7 slope-style detectors use the standard phrase with the dynamic threshold value
        assert!(
            rendered.contains("promotes to Recommendation when confidence >= high"),
            "default 'high' threshold missing in rules tab: {rendered}"
        );
        // SubagentSideEffect uses 'medium' default
        assert!(
            rendered.contains("(confidence >= medium)"),
            "subagent_side_effect medium default missing: {rendered}"
        );
    }

    #[test]
    fn parameters_tab_includes_anomaly_retention_days_row() {
        let config = QmonsterConfig::defaults();
        let mut s = SettingsOverlay::new();
        s.open();
        s.switch_tab(SettingsTab::Parameters);
        let lines = build_body_lines(&s, &config);
        let rendered = rendered_text(&lines);
        assert!(
            rendered.contains("anomaly retention_days"),
            "row label missing: {rendered}"
        );
        assert!(rendered.contains("30"), "default value 30 missing");
    }
}
