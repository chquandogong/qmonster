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
    ActionsMode, ConfirmActions, ContextConfig, CostConfig, CostProviderConfig, LogSensitivity,
    PressureProviderConfig, QmonsterConfig, QuotaConfig, RefreshPolicy,
};
use crate::domain::recommendation::Severity;
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap};
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
    edit_buffer: Option<String>,
    status: SettingsStatus,
    /// Tracks dirty-since-open so `is_dirty()` survives a transient
    /// `Saved` / `Error` status banner change.
    dirty: bool,
}

impl Default for SettingsOverlay {
    fn default() -> Self {
        Self {
            open: false,
            tab: SettingsTab::Thresholds,
            selected: FieldId::new(Section::Cost, Scope::Default, Bound::Warning),
            selected_integration: IntegrationField::ClaudeSidefile,
            edit_buffer: None,
            status: SettingsStatus::Idle,
            dirty: false,
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

    pub fn switch_tab(&mut self, tab: SettingsTab) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.tab = tab;
    }

    pub fn next_tab(&mut self) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.tab = self.tab.next();
    }

    pub fn previous_tab(&mut self) {
        if self.edit_buffer.is_some() {
            return;
        }
        self.tab = self.tab.previous();
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
        self.edit_buffer = None;
        self.status = SettingsStatus::Idle;
        self.dirty = false;
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
        if !self.open || self.tab != SettingsTab::Thresholds {
            return;
        }
        let v = self.effective_field(config, self.selected);
        // Strip trailing zeros so 5.0 reads back as "5" but 0.75 stays
        // "0.75". Operator who wants "5.00" can type the zeros.
        self.edit_buffer = Some(format_value_for_edit(v, self.selected.section));
        self.status = SettingsStatus::Idle;
    }

    /// Append a character to the edit buffer. No-op when not editing
    /// or when the character is not a digit / dot / minus.
    pub fn type_char(&mut self, c: char) {
        let Some(buf) = self.edit_buffer.as_mut() else {
            return;
        };
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

    /// Parse the edit buffer and apply it to the config if it
    /// validates. Returns `Ok(())` on success, `Err(reason)` on
    /// validation failure (status banner is also updated).
    pub fn commit_edit(&mut self, config: &mut QmonsterConfig) -> Result<(), String> {
        let Some(buf) = self.edit_buffer.take() else {
            return Err("no active edit".into());
        };
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

    let tab_titles: Vec<Line<'_>> = SETTINGS_TABS
        .iter()
        .map(|tab| Line::from(tab.label()))
        .collect();
    let tabs = Tabs::new(tab_titles)
        .select(overlay.tab().index())
        .block(
            Block::default()
                .title("Settings")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::BORDER_ACTIVE)),
        )
        .highlight_style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, rects.tabs);

    let body_lines = build_body_lines(overlay, config);
    frame.render_widget(
        Paragraph::new(body_lines).wrap(Wrap { trim: false }).block(
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

    // Mouse-clickable close button at the top-right of the body, mirroring
    // the help / git / target overlays. The companion click handler in the
    // main event loop hit-tests the same rectangle via
    // `settings_close_button_rect()`.
    frame.render_widget(
        Paragraph::new("[x]").style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        settings_close_button_rect(rects.body),
    );

    frame.render_widget(
        Paragraph::new(hint_lines(overlay))
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
    // Mirrors ratatui Tabs' left-aligned layout inside an ALL-borders
    // block: one border cell, then ` label `, a divider, and the next
    // padded label. Empty space to the right of the final label is
    // intentionally not clickable.
    let mut cursor = tabs.x.saturating_add(1);
    for (idx, tab) in SETTINGS_TABS.iter().enumerate() {
        let label_width = tab.label().chars().count() as u16;
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

pub fn settings_field_at(body: Rect, column: u16, row: u16) -> Option<FieldId> {
    let inner = body.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    if !rect_contains(inner, column, row) {
        return None;
    }
    let mut line_idx = row.saturating_sub(inner.y);
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
    let inner = body.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    if !rect_contains(inner, column, row) {
        return None;
    }
    match row.saturating_sub(inner.y) {
        1 => Some(IntegrationField::ClaudeSidefile),
        2 => Some(IntegrationField::CodexAppServer),
        _ => None,
    }
}

fn rect_contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn settings_body_title(tab: SettingsTab) -> &'static str {
    match tab {
        SettingsTab::Thresholds => " Settings — cost / context / quota thresholds ",
        SettingsTab::Integrations => " Settings — provider integrations ",
        SettingsTab::Parameters => " Settings — configured parameters ",
        SettingsTab::Rules => " Settings — rule conditions ",
        SettingsTab::Badges => " Settings — badge glossary ",
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
    vec![
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
        status_line(overlay),
    ]
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
                "fires when (provider, path) flips >= {} times in {} polls; promotes to Recommendation when confidence=High",
                config.anomaly.identity_churn_min_flips, config.anomaly.window_polls,
            ),
        ),
        rule_row(
            "anomaly: ErrorBurst",
            format!(
                "fires when error rate >= {:.0}% over {} polls; promotes to Recommendation when confidence=High",
                config.anomaly.error_burst_threshold * 100.0,
                config.anomaly.window_polls,
            ),
        ),
        rule_row(
            "anomaly: CacheDiscontinuity",
            format!(
                "fires when cache_hit_ratio drops >= {:.0}pp OR F-7b fires >= 2x in {} polls; promotes to Recommendation when confidence=High",
                config.anomaly.cache_discontinuity_drop * 100.0,
                config.anomaly.window_polls,
            ),
        ),
        rule_row(
            "anomaly: CrossPaneEditCluster",
            format!(
                "fires when >= {} ConcurrentFileEdit findings target the same path in {} polls; promotes to Recommendation when confidence=High",
                config.anomaly.cross_pane_cluster_min_findings, config.anomaly.window_polls,
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
            "cache_read / (input + cache_read); hidden when the provider does not expose cache reads",
        ),
        badge_row(
            "cache io",
            "expanded pane cache read/create token counts when a provider sidefile or stats panel exposes them",
        ),
        badge_row(
            "COST $N",
            "Claude sidefile total_cost_usd is Official; Codex cost is Estimate from input/output pricing",
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

fn hint_lines(overlay: &SettingsOverlay) -> Vec<Line<'static>> {
    let editing = overlay.edit_buffer().is_some();
    let line1 = if editing {
        "  EDIT — type digits/'.' · Enter commit · Esc cancel · Backspace delete"
    } else if overlay.tab() == SettingsTab::Thresholds {
        "  [1]-[5]/[Tab] tab · ↑/↓ select · e/Enter edit · c clear · w write · q/Esc close"
    } else if overlay.tab() == SettingsTab::Integrations {
        "  [1]-[5]/[Tab] tab · ↑/↓ select · Space/e/Enter toggle · w write · q/Esc close"
    } else {
        "  [1]-[5]/[Tab] tab · click tabs · w write dirty edits · q/Esc close"
    };
    let line2 =
        if overlay.tab() == SettingsTab::Thresholds || overlay.tab() == SettingsTab::Integrations {
            "  Edits stay in memory until 'w' writes back to the loaded TOML."
        } else {
            "  This tab is read-only; edit values on Thresholds or Integrations."
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
        let tabs = Rect::new(10, 2, 80, 3);
        let inner_x = tabs.x + 1;

        assert_eq!(settings_tab_index_at(tabs, inner_x + 2), Some(0));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 17), Some(1));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 29), Some(2));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 41), Some(3));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 50), Some(4));
        assert_eq!(settings_tab_index_at(tabs, inner_x + 70), None);
        assert_eq!(settings_tab_index_at(tabs, tabs.x - 1), None);
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
        assert!(rendered.contains("Codex cost is Estimate"));
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
        let expected_phrase = "promotes to Recommendation when confidence=High";
        let count = rendered.matches(expected_phrase).count();
        assert_eq!(
            count, 4,
            "expected 4 anomaly rules with promotion annotation; got {count}: {rendered}"
        );
    }
}
