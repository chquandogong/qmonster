//! v1.15.18: in-TUI settings overlay for cost / context / quota
//! advisory thresholds.
//!
//! Lifts the v1.15.16 / v1.15.17 TOML-only configuration surface into
//! a keyboard-driven modal so the operator can re-tune thresholds
//! without leaving the dashboard. The state machine is config-mutating
//! but does not touch disk on its own — `save()` is an explicit
//! operator action gated on a known config path.
//!
//! Editable fields cover six sections × four scopes = 24 numeric
//! values: cost / context / quota × default / claude / codex / gemini.
//! Per-provider scopes that have no override read as inherited from
//! the section default and display as `(default)` until the operator
//! types a value.

use crate::app::config::{
    ContextConfig, CostConfig, CostProviderConfig, PressureProviderConfig, QmonsterConfig,
    QuotaConfig,
};
use crate::domain::recommendation::Severity;
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
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
    Codex,
    Gemini,
}

/// Warning vs critical end of the threshold pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Warning,
    Critical,
}

/// A single editable field — `(Section, Scope, Bound)` triple.
/// 3 × 4 × 2 = 24 fields total.
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
}

/// The 24 fields in the order they appear in the overlay. Iteration
/// order is `(section outer, scope middle, bound inner)` so a single
/// row of the rendered grid is one `(section, scope)` pair with the
/// warning + critical cells side by side, and adjacent sections sit
/// on consecutive rows of the same scope.
pub fn all_fields() -> Vec<FieldId> {
    let mut out = Vec::with_capacity(24);
    for section in [Section::Cost, Section::Context, Section::Quota] {
        for scope in [Scope::Default, Scope::Claude, Scope::Codex, Scope::Gemini] {
            for bound in [Bound::Warning, Bound::Critical] {
                out.push(FieldId::new(section, scope, bound));
            }
        }
    }
    out
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
    selected: FieldId,
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
            selected: FieldId::new(Section::Cost, Scope::Default, Bound::Warning),
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
        self.selected = FieldId::new(Section::Cost, Scope::Default, Bound::Warning);
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
        if self.edit_buffer.is_some() {
            return;
        }
        let fields = all_fields();
        let idx = fields.iter().position(|f| *f == self.selected).unwrap_or(0);
        self.selected = fields[(idx + 1) % fields.len()];
    }

    /// Move focus to the previous field, wrapping at the start.
    pub fn prev_field(&mut self) {
        if self.edit_buffer.is_some() {
            return;
        }
        let fields = all_fields();
        let idx = fields.iter().position(|f| *f == self.selected).unwrap_or(0);
        self.selected = fields[(idx + fields.len() - 1) % fields.len()];
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
        if self.edit_buffer.is_some() {
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
        let body = match toml::to_string_pretty(config) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("serialize: {e}");
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
                Scope::Default => unreachable!(),
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
            Scope::Claude => self.claude.as_ref(),
            Scope::Codex => self.codex.as_ref(),
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
    }
}

fn apply_quota(quota: &mut QuotaConfig, field: FieldId, value: f32) {
    match field.scope {
        Scope::Default => match field.bound {
            Bound::Warning => quota.warning_pct = value,
            Bound::Critical => quota.critical_pct = value,
        },
        Scope::Claude => apply_pct_provider(
            &mut quota.claude,
            quota.warning_pct,
            quota.critical_pct,
            field.bound,
            value,
        ),
        Scope::Codex => apply_pct_provider(
            &mut quota.codex,
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
                Scope::Default => {}
            }
        }
        Section::Context => {
            let ctx = &mut config.context;
            match scope {
                Scope::Claude => ctx.claude = None,
                Scope::Codex => ctx.codex = None,
                Scope::Gemini => ctx.gemini = None,
                Scope::Default => {}
            }
        }
        Section::Quota => {
            let q = &mut config.quota;
            match scope {
                Scope::Claude => q.claude = None,
                Scope::Codex => q.codex = None,
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
        Scope::Codex => "codex",
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
// (default + 3 providers) × (warning, critical) cells.
// -----------------------------------------------------------------

const MODAL_WIDTH_PERCENT: u16 = 76;
const MODAL_HEIGHT_PERCENT: u16 = 80;

pub struct SettingsModalRects {
    pub area: Rect,
    pub body: Rect,
    pub hint: Rect,
}

pub fn settings_modal_rects(viewport: Rect) -> SettingsModalRects {
    let area = centered_rect(MODAL_WIDTH_PERCENT, MODAL_HEIGHT_PERCENT, viewport);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(area);
    SettingsModalRects {
        area,
        body: chunks[0],
        hint: chunks[1],
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

    let body_lines = build_body_lines(overlay, config);
    frame.render_widget(
        Paragraph::new(body_lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(Span::styled(
                    " Settings — cost / context / quota thresholds ",
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

fn build_body_lines<'a>(overlay: &'a SettingsOverlay, config: &'a QmonsterConfig) -> Vec<Line<'a>> {
    let mut lines: Vec<Line<'a>> = Vec::new();
    for (i, section) in [Section::Cost, Section::Context, Section::Quota]
        .iter()
        .enumerate()
    {
        if i > 0 {
            lines.push(Line::from(""));
        }
        lines.push(section_header_line(*section));
        for scope in [Scope::Default, Scope::Claude, Scope::Codex, Scope::Gemini] {
            lines.push(scope_row_line(overlay, config, *section, scope));
        }
    }
    lines.push(Line::from(""));
    lines.push(status_line(overlay));
    lines
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
            "          warning      critical",
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
    let scope_label = format!("{:<10}", describe_scope(scope));

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
            Scope::Default => false,
        },
        Section::Context => match scope {
            Scope::Claude => config.context.claude.is_some(),
            Scope::Codex => config.context.codex.is_some(),
            Scope::Gemini => config.context.gemini.is_some(),
            Scope::Default => false,
        },
        Section::Quota => match scope {
            Scope::Claude => config.quota.claude.is_some(),
            Scope::Codex => config.quota.codex.is_some(),
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
    } else {
        "  ↑/↓ select · e or Enter edit · c clear override (provider rows) · w write · q or Esc close"
    };
    vec![
        Line::from(line1),
        Line::from("  Edits stay in memory until 'w' writes back to the loaded TOML."),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> QmonsterConfig {
        QmonsterConfig::defaults()
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
    fn open_sets_open_and_focuses_first_field() {
        let mut s = SettingsOverlay::new();
        s.open();
        assert!(s.is_open());
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
    fn next_field_advances_through_24_fields_and_wraps() {
        let mut s = SettingsOverlay::new();
        s.open();
        let start = s.selected();
        for _ in 0..24 {
            s.next_field();
        }
        assert_eq!(
            s.selected(),
            start,
            "24 next_field calls must wrap to start"
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
}
