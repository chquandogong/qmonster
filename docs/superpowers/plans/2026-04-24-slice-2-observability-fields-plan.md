# Slice 2 — Observability Field Expansion (v1.12.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate `git_branch`, `worktree_path`, `reasoning_effort`, and Claude `model_name` on `SignalSet` so every Codex TUI pane shows its branch/path/effort and Claude panes show the model from `~/.claude/settings.json`.

**Architecture:** Extend the Slice 1 foundation — add 3 new `Option<MetricValue<String>>` fields to `SignalSet`, promote `ProviderParser::parse` from three positional args to a `&ParserContext` struct (the `ClaudeSettings` addition is the second cross-cutting input both Slice 1 reviewers said would trigger this migration), extend the Codex status-bar parser and add a second Codex surface parser for the `/status` box, read Claude `settings.json` once at startup as an operator-curated config file (same honesty class as `config/pricing.toml`), and split the TUI metric-badge line into two rows so the wider surface reads cleanly.

**Tech Stack:** Rust 2024 edition, existing `SignalSet` / `MetricValue` / `SourceKind` types, `serde = "1"` + `serde_json = "1"` + `toml = "0.8"` + `thiserror = "1"` + `tempfile = "3"` (all already in Cargo.toml).

**Spec:** `docs/superpowers/specs/2026-04-24-slice-2-observability-fields-design.md` (commit `bd36995`).

**Target version tag:** `v1.12.0` at the last commit in this plan.

---

## File Structure

**Create:**

- `src/policy/claude_settings.rs` — `ClaudeSettings`, `ClaudeSettingsError`, JSON loader, `model()` accessor
- `config/claude-settings.example.json` — operator-facing template (placeholder only)
- `tests/fixtures/codex.rs` — integration-test fixture module (`CODEX_STATUS_FIXTURE_V0_122_0` + `CODEX_STATUS_BOX_FIXTURE`); imported by `tests/event_loop_integration.rs` via `#[path = "fixtures/codex.rs"] mod codex_fixtures;`

**Modify:**

- `src/policy/mod.rs` — `pub mod claude_settings;` + re-export `ClaudeSettings`
- `src/domain/signal.rs` — add 3 fields: `git_branch`, `worktree_path`, `reasoning_effort`
- `src/domain/audit.rs` — add `AuditEventKind::ClaudeSettingsLoadFailed`; extend `as_str` + contract test
- `src/store/audit.rs` — extend `parse_kind` + round-trip test list; new SQLite round-trip test
- `src/adapters/common.rs` — add 3 new fields as `None` to the one exhaustive `SignalSet { ... }` struct-literal in `parse_common_signals`
- `src/adapters/mod.rs` — replace `ProviderParser::parse(.., identity, tail, pricing)` with `parse(&self, ctx: &ParserContext)`; update `parse_for` dispatch
- `src/adapters/claude.rs` — accept `ParserContext`, read `ctx.claude_settings.model()` for `model_name`
- `src/adapters/codex.rs` — accept `ParserContext`; extend `parse_codex_status_line` with worktree/branch extraction; add `parse_codex_reasoning_effort` helper; emit 3 new fields
- `src/adapters/gemini.rs` — accept `ParserContext` (mechanical)
- `src/adapters/qmonster.rs` — accept `ParserContext` (mechanical)
- `src/app/bootstrap.rs` — add `claude_settings: ClaudeSettings` field + `with_claude_settings` builder
- `src/app/event_loop.rs` — construct `ParserContext` and pass to `parse_for`
- `src/main.rs` — load `ClaudeSettings` at startup; chain `.with_claude_settings(...)` in the builder
- `src/ui/panels.rs` — change `metric_badge_line` return type to `Vec<Line<'static>>`; factor into `primary_metric_row` + `context_metric_row`; update `metric_row` and caller
- `.gitignore` — add `config/claude-settings.json`

**Test locations:**

- Unit tests inline in each modified source file's `mod tests`
- Integration tests in `tests/event_loop_integration.rs` + shared fixture module

---

## Task 1: `ClaudeSettings` module + audit kind + config template

**Files:**

- Create: `src/policy/claude_settings.rs`
- Modify: `src/policy/mod.rs`
- Create: `config/claude-settings.example.json`
- Modify: `.gitignore`
- Modify: `src/domain/audit.rs`
- Modify: `src/store/audit.rs`

- [ ] **Step 1: Write the failing tests (claude_settings)**

Create `src/policy/claude_settings.rs` with full module contents:

```rust
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ClaudeSettings {
    model: Option<String>,
    // Other settings.json keys are ignored — Slice 2 only surfaces `model`.
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudeSettingsError {
    #[error("claude settings not found at {0}")]
    NotFound(PathBuf),
    #[error("failed to read claude settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse claude settings: {0}")]
    Parse(#[from] serde_json::Error),
}

impl ClaudeSettings {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Standard location: `$HOME/.claude/settings.json`. Returns None
    /// when `HOME` is unset (uncommon but possible in sandboxes).
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/settings.json"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, ClaudeSettingsError> {
        let text = fs::read_to_string(path)?;
        let parsed: Self = serde_json::from_str(&text)?;
        Ok(parsed)
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_json(body: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", body).unwrap();
        f
    }

    #[test]
    fn claude_settings_empty_has_no_model() {
        assert!(ClaudeSettings::empty().model().is_none());
    }

    #[test]
    fn claude_settings_loads_model_from_json() {
        let f = write_json(r#"{"model": "claude-sonnet-4-6"}"#);
        let s = ClaudeSettings::load_from_path(f.path()).unwrap();
        assert_eq!(s.model(), Some("claude-sonnet-4-6"));
    }

    #[test]
    fn claude_settings_missing_model_key_returns_none() {
        let f = write_json(r#"{"other_key": "value"}"#);
        let s = ClaudeSettings::load_from_path(f.path()).unwrap();
        assert!(s.model().is_none());
    }

    #[test]
    fn claude_settings_missing_file_returns_io_not_found() {
        let result = ClaudeSettings::load_from_path(Path::new("/nonexistent/settings.json"));
        match result {
            Err(ClaudeSettingsError::Io(io)) => {
                assert_eq!(io.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected Io(NotFound), got {other:?}"),
        }
    }

    #[test]
    fn claude_settings_parse_error_surfaces_via_result() {
        let f = write_json(r#"not valid json at all"#);
        let result = ClaudeSettings::load_from_path(f.path());
        assert!(
            matches!(result, Err(ClaudeSettingsError::Parse(_))),
            "expected Parse error, got {result:?}"
        );
    }
}
```

- [ ] **Step 2: Register the module**

Edit `src/policy/mod.rs`. Current content:

```rust
pub mod engine;
pub mod gates;
pub mod pricing;
pub mod rules;

pub use engine::{Engine, EvalOutput, PaneView};
pub use gates::{PolicyGates, allow_aggressive, allow_provider_specific};
pub use pricing::{PricingRates, PricingTable};
pub use rules::eval_alerts;
```

Replace with:

```rust
pub mod claude_settings;
pub mod engine;
pub mod gates;
pub mod pricing;
pub mod rules;

pub use claude_settings::ClaudeSettings;
pub use engine::{Engine, EvalOutput, PaneView};
pub use gates::{PolicyGates, allow_aggressive, allow_provider_specific};
pub use pricing::{PricingRates, PricingTable};
pub use rules::eval_alerts;
```

- [ ] **Step 3: Run claude_settings tests — expect pass**

Run: `cargo test --lib policy::claude_settings::tests`

Expected: 5 tests pass.

- [ ] **Step 4: Create the operator-facing example**

Write `config/claude-settings.example.json`:

```json
{
  "_comment": "Qmonster reads the `model` key to populate the MODEL badge on Claude panes. Other keys are ignored. Copy this file to config/claude-settings.json and edit the model value to match your Claude Code settings. The file is gitignored so each operator owns their own copy.",
  "model": "claude-sonnet-4-6"
}
```

Note on the `_comment` field: Claude's `settings.json` is strict JSON (no `//` line comments). The leading underscore key is a common convention for in-file documentation that parsers ignore at the key-existence level (our `ClaudeSettings` struct has no `_comment` field, so serde ignores it by default). If a future operator complains, replace with a sibling `README` under `config/`.

- [ ] **Step 5: Extend .gitignore**

Append to `.gitignore`:

```
config/claude-settings.json
```

- [ ] **Step 6: Add the audit kind variant**

Edit `src/domain/audit.rs`. Find the line `PricingLoadFailed,` (around line 41) and add `ClaudeSettingsLoadFailed,` immediately after it:

```rust
    PricingLoadFailed,
    ClaudeSettingsLoadFailed,
```

Find the `as_str` match arm for `PricingLoadFailed` (around line 110):

```rust
            AuditEventKind::PricingLoadFailed => "PricingLoadFailed",
```

Add the analog immediately after:

```rust
            AuditEventKind::PricingLoadFailed => "PricingLoadFailed",
            AuditEventKind::ClaudeSettingsLoadFailed => "ClaudeSettingsLoadFailed",
```

Find the contract-test variant list around line 266:

```rust
            (AuditEventKind::PricingLoadFailed, "PricingLoadFailed"),
```

Add the analog:

```rust
            (AuditEventKind::PricingLoadFailed, "PricingLoadFailed"),
            (AuditEventKind::ClaudeSettingsLoadFailed, "ClaudeSettingsLoadFailed"),
```

- [ ] **Step 7: Extend parse_kind**

Edit `src/store/audit.rs`. Find the `parse_kind` arm around line 144:

```rust
        "PricingLoadFailed" => Some(AuditEventKind::PricingLoadFailed),
```

Add the analog immediately after:

```rust
        "PricingLoadFailed" => Some(AuditEventKind::PricingLoadFailed),
        "ClaudeSettingsLoadFailed" => Some(AuditEventKind::ClaudeSettingsLoadFailed),
```

Find the round-trip variant list around line 290:

```rust
            AuditEventKind::PricingLoadFailed,
```

Add the analog:

```rust
            AuditEventKind::PricingLoadFailed,
            AuditEventKind::ClaudeSettingsLoadFailed,
```

- [ ] **Step 8: Add SQLite round-trip test**

Find the `pricing_load_failed_audit_kind_roundtrips_through_sqlite` test around line 340 in `src/store/audit.rs`. Add a sibling test immediately after it (mirror the structure):

```rust
    #[test]
    fn claude_settings_load_failed_audit_kind_roundtrips_through_sqlite() {
        // ClaudeSettingsLoadFailed kind (new in v1.12.0) must survive a
        // write → read cycle so operators can post-hoc query why the
        // MODEL badge disappeared on Claude panes.
        let sink = InMemorySink::new();
        sink.record(sample(AuditEventKind::ClaudeSettingsLoadFailed));
        let rows = sink.snapshot();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, AuditEventKind::ClaudeSettingsLoadFailed);
        assert_eq!(
            AuditEventKind::ClaudeSettingsLoadFailed.as_str(),
            "ClaudeSettingsLoadFailed"
        );
    }
```

- [ ] **Step 9: Run all tests — expect pass**

Run: `cargo test --lib policy::claude_settings domain::audit store::audit`

Expected: 5 claude_settings + several audit variants + 1 new SQLite round-trip test all pass.

Run: `cargo test --lib`

Expected: full lib green. Baseline v1.11.3 = 277 lib → expect 283 (+6: 5 new claude_settings + 1 new SQLite round-trip). Note the audit contract-test expansion is data-driven (one test, more cases) so no net test count change there.

- [ ] **Step 10: Verify clippy + fmt**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`

Expected: clean.

- [ ] **Step 11: Commit**

```bash
git add src/policy/claude_settings.rs src/policy/mod.rs config/claude-settings.example.json .gitignore src/domain/audit.rs src/store/audit.rs
git commit -m "$(cat <<'EOF'
policy(v1.12.0-1): add ClaudeSettings module + settings.example.json + audit kind

Introduces a read-only loader for ~/.claude/settings.json so the
Claude adapter can surface `model` as the MODEL badge source
without parsing the pane tail (which does not expose model).
Errors route through ClaudeSettingsError (Io / Parse); file-absent
stays silent; parse failures will surface through the new
`AuditEventKind::ClaudeSettingsLoadFailed` variant when main.rs
wires the load in Task 3.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `SignalSet` schema — add 3 observability fields

**Files:**

- Modify: `src/domain/signal.rs`
- Modify: `src/adapters/common.rs`

- [ ] **Step 1: Write failing tests**

Append to `src/domain/signal.rs` `mod tests` (after the existing tests around line 97):

```rust
    #[test]
    fn default_signal_set_has_no_git_branch_or_worktree_or_effort() {
        let s = SignalSet::default();
        assert!(s.git_branch.is_none());
        assert!(s.worktree_path.is_none());
        assert!(s.reasoning_effort.is_none());
    }

    #[test]
    fn signal_set_can_carry_observability_fields() {
        let s = SignalSet {
            git_branch: Some(
                MetricValue::new("main".to_string(), SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Codex),
            ),
            worktree_path: Some(
                MetricValue::new("~/Qmonster".to_string(), SourceKind::ProviderOfficial)
                    .with_provider(Provider::Codex),
            ),
            reasoning_effort: Some(
                MetricValue::new("xhigh".to_string(), SourceKind::ProviderOfficial)
                    .with_confidence(0.6)
                    .with_provider(Provider::Codex),
            ),
            ..SignalSet::default()
        };
        assert_eq!(s.git_branch.as_ref().unwrap().value, "main");
        assert_eq!(s.worktree_path.as_ref().unwrap().value, "~/Qmonster");
        assert_eq!(s.reasoning_effort.as_ref().unwrap().value, "xhigh");
        assert_eq!(
            s.reasoning_effort.as_ref().unwrap().confidence,
            Some(0.6)
        );
    }
```

- [ ] **Step 2: Run test — expect compile failure**

Run: `cargo test --lib domain::signal::tests::default_signal_set_has_no_git_branch_or_worktree_or_effort`

Expected: compile error — `no field git_branch on SignalSet` (and similar).

- [ ] **Step 3: Add the fields**

Edit `src/domain/signal.rs`. Find the `SignalSet` struct (around line 52) and append three lines before the closing brace (after `model_name`):

```rust
#[derive(Debug, Clone, Default)]
pub struct SignalSet {
    pub waiting_for_input: bool,
    pub permission_prompt: bool,
    pub log_storm: bool,
    pub repeated_output: bool,
    pub verbose_answer: bool,
    pub error_hint: bool,
    pub subagent_hint: bool,
    pub output_chars: usize,
    pub task_type: TaskType,
    pub context_pressure: Option<MetricValue<f32>>,
    pub token_count: Option<MetricValue<u64>>,
    pub cost_usd: Option<MetricValue<f64>>,
    pub model_name: Option<MetricValue<String>>,
    pub git_branch: Option<MetricValue<String>>,
    pub worktree_path: Option<MetricValue<String>>,
    pub reasoning_effort: Option<MetricValue<String>>,
}
```

- [ ] **Step 4: Fix the cascade site in common.rs**

Edit `src/adapters/common.rs`. Find the `parse_common_signals` function around line 56 and locate the `SignalSet { ... }` literal (around line 69). The current final three fields are `token_count: None`, `cost_usd: None`, `model_name: None`. Append three more `None` lines before the closing brace:

```rust
    SignalSet {
        waiting_for_input: WAITING_MARKERS.iter().any(|m| lower.contains(m)),
        permission_prompt: PERMISSION_MARKERS.iter().any(|m| lower.contains(m)),
        log_storm,
        repeated_output: false,
        verbose_answer,
        error_hint: ERROR_MARKERS.iter().any(|m| lower.contains(m)),
        subagent_hint: SUBAGENT_MARKERS.iter().any(|m| lower.contains(m)),
        output_chars,
        task_type: detect_task_type(&lower),
        context_pressure: parse_context_pressure(&lower),
        token_count: None,
        cost_usd: None,
        model_name: None,
        git_branch: None,
        worktree_path: None,
        reasoning_effort: None,
    }
```

- [ ] **Step 5: Run tests — expect pass**

Run: `cargo test --lib domain::signal::tests`

Expected: all pass, including 2 new tests.

- [ ] **Step 6: Full lib tests green**

Run: `cargo test --lib`

Expected: 283 → 285 (+2). All existing tests still pass because every other `SignalSet { ... }` call site uses `..Default::default()` spread (Slice 1's v1.11.0-2 precedent).

- [ ] **Step 7: Clippy + fmt clean**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`

Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/domain/signal.rs src/adapters/common.rs
git commit -m "$(cat <<'EOF'
domain(v1.12.0-2): add git_branch + worktree_path + reasoning_effort fields to SignalSet

Extends SignalSet with three Option<MetricValue<String>> fields so
the Codex adapter (Tasks 4-5) can expose the information its
status bar and /status box already emit. parse_common_signals
(the sole exhaustive struct-literal site, deliberately kept
exhaustive per v1.11.0-2's reviewer feedback) gets three matching
`None` defaults. All ~50 other SignalSet { ... } construction
sites use ..Default::default() spread and cascade without touch.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Migrate `ProviderParser` to `ParserContext` struct

**Files:**

- Modify: `src/adapters/mod.rs`
- Modify: `src/adapters/claude.rs`
- Modify: `src/adapters/codex.rs`
- Modify: `src/adapters/gemini.rs`
- Modify: `src/adapters/qmonster.rs`
- Modify: `src/app/event_loop.rs`
- Modify: `src/app/bootstrap.rs`
- Modify: `src/main.rs`

This task is a mechanical signature migration. No behavior change yet — Tasks 4-6 add actual parsing.

- [ ] **Step 1: Replace `src/adapters/mod.rs`**

Current content is ~28 lines (from Slice 1's v1.11.0-3). Replace entirely with:

```rust
pub mod claude;
pub mod codex;
pub mod common;
pub mod gemini;
pub mod qmonster;

use crate::domain::identity::{Provider, ResolvedIdentity};
use crate::domain::signal::SignalSet;
use crate::policy::claude_settings::ClaudeSettings;
use crate::policy::pricing::PricingTable;

/// Inputs the adapter layer needs when producing a SignalSet from a
/// pane tail. The struct keeps the trait method signature stable as
/// Slice 3+ introduce more cross-cutting observability inputs.
pub struct ParserContext<'a> {
    pub identity: &'a ResolvedIdentity,
    pub tail: &'a str,
    pub pricing: &'a PricingTable,
    pub claude_settings: &'a ClaudeSettings,
}

/// Provider-specific parser. Each adapter receives a ParserContext
/// bundle and emits typed signals. Identity inference never happens
/// here (r2 non-negotiable; see ARCHITECTURE.md).
pub trait ProviderParser {
    fn parse(&self, ctx: &ParserContext) -> SignalSet;
}

/// Dispatch helper — pick the right adapter by provider.
pub fn parse_for(ctx: &ParserContext) -> SignalSet {
    match ctx.identity.identity.provider {
        Provider::Claude => claude::ClaudeAdapter.parse(ctx),
        Provider::Codex => codex::CodexAdapter.parse(ctx),
        Provider::Gemini => gemini::GeminiAdapter.parse(ctx),
        Provider::Qmonster => qmonster::QmonsterAdapter.parse(ctx),
        Provider::Unknown => common::parse_common_signals(ctx.tail),
    }
}

pub use common::parse_common_signals;
```

- [ ] **Step 2: Update `ClaudeAdapter` signature only (body unchanged this task)**

Edit `src/adapters/claude.rs`. The current impl signature is:

```rust
impl ProviderParser for ClaudeAdapter {
    fn parse(
        &self,
        _identity: &ResolvedIdentity,
        tail: &str,
        _pricing: &PricingTable,
    ) -> SignalSet {
```

Replace the signature block (keep the body unchanged — Task 6 changes the body):

```rust
impl ProviderParser for ClaudeAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let tail = ctx.tail;
        // ... existing body keeps working against the `tail` local ...
```

The body's `parse_common_signals(tail)` / `parse_context_percent_claude(&tail.to_lowercase())` / `parse_claude_output_tokens(tail)` calls stay unchanged — they all reference the `tail` local bound above.

Also remove any now-unused imports at the top — specifically `use crate::domain::identity::ResolvedIdentity;` and `use crate::policy::pricing::PricingTable;` can go away since the signature no longer names them directly. Verify with `cargo build` after — if the body still uses `ResolvedIdentity` somewhere, keep it.

- [ ] **Step 3: Update Claude tests to construct a ParserContext**

Inside `src/adapters/claude.rs::mod tests`, the current tests use a helper `fn id() -> ResolvedIdentity` and call `ClaudeAdapter.parse(&id(), tail, &PricingTable::empty())`. Rewrite each test to construct a `ParserContext` inline. Also add a reusable helper:

```rust
    use crate::adapters::ParserContext;
    use crate::policy::claude_settings::ClaudeSettings;

    fn ctx<'a>(
        id: &'a ResolvedIdentity,
        tail: &'a str,
        pricing: &'a PricingTable,
        settings: &'a ClaudeSettings,
    ) -> ParserContext<'a> {
        ParserContext {
            identity: id,
            tail,
            pricing,
            claude_settings: settings,
        }
    }

    #[test]
    fn claude_adapter_inherits_common_signals() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, "Press ENTER to continue", &pricing, &settings);
        let set = ClaudeAdapter.parse(&c);
        assert!(set.waiting_for_input);
    }

    #[test]
    fn claude_adapter_parses_claude_specific_percent() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, "claude context 88%", &pricing, &settings);
        let set = ClaudeAdapter.parse(&c);
        let m = set.context_pressure.expect("parsed");
        assert!((m.value - 0.88).abs() < 0.01);
    }
```

Apply the same `ctx(&id, tail, &pricing, &settings)` rewrite to every other Claude test (`claude_adapter_extracts_output_tokens_from_working_line`, `claude_adapter_prefers_subagent_done_line_over_working_line`, `claude_adapter_returns_none_token_count_when_no_marker`, `claude_adapter_never_populates_model_name_or_cost_in_slice_1`).

Rename `claude_adapter_never_populates_model_name_or_cost_in_slice_1` to `claude_adapter_never_populates_model_name_from_tail` in preparation for Task 6, but keep the assertions and body unchanged for now. The Task 6 commit will add the new `_from_settings` test that makes this rename necessary.

- [ ] **Step 4: Update Codex adapter signature (body unchanged this task)**

Edit `src/adapters/codex.rs`. The current Slice 1 post-remediation signature is:

```rust
impl ProviderParser for CodexAdapter {
    fn parse(
        &self,
        _identity: &ResolvedIdentity,
        tail: &str,
        pricing: &PricingTable,
    ) -> SignalSet {
```

Replace the signature block (keep the body unchanged):

```rust
impl ProviderParser for CodexAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let tail = ctx.tail;
        let pricing = ctx.pricing;
        // ... existing body keeps working against `tail` + `pricing` locals ...
```

Rewrite the Codex unit tests the same way Claude's were rewritten — construct a `ParserContext` inline, use a `ctx` helper. The `pricing_with_gpt_5_4() -> (PricingTable, NamedTempFile)` helper already exists from v1.11.2 and stays unchanged; just feed its `.0` into the new `ctx` helper.

- [ ] **Step 5: Update Gemini adapter**

Edit `src/adapters/gemini.rs`. Change:

```rust
impl ProviderParser for GeminiAdapter {
    fn parse(
        &self,
        _identity: &ResolvedIdentity,
        tail: &str,
        _pricing: &PricingTable,
    ) -> SignalSet {
        parse_common_signals(tail)
    }
}
```

To:

```rust
impl ProviderParser for GeminiAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        parse_common_signals(ctx.tail)
    }
}
```

And update the single test to use a ParserContext the same way.

- [ ] **Step 6: Update Qmonster adapter**

Same pattern for `src/adapters/qmonster.rs`:

```rust
impl ProviderParser for QmonsterAdapter {
    fn parse(&self, _ctx: &crate::adapters::ParserContext) -> SignalSet {
        SignalSet::default()
    }
}
```

Update the single test.

- [ ] **Step 7: Update the event-loop caller**

Edit `src/app/event_loop.rs` around line 94. Current:

```rust
let signals = crate::adapters::parse_for(&resolved, &pane.tail, &ctx.pricing);
```

Replace with:

```rust
let parse_ctx = crate::adapters::ParserContext {
    identity: &resolved,
    tail: &pane.tail,
    pricing: &ctx.pricing,
    claude_settings: &ctx.claude_settings,
};
let signals = crate::adapters::parse_for(&parse_ctx);
```

- [ ] **Step 8: Extend `Context` in bootstrap.rs**

Edit `src/app/bootstrap.rs`. Add the import near other `use crate::` lines:

```rust
use crate::policy::claude_settings::ClaudeSettings;
```

Add the field to `Context` (after `pricing`):

```rust
pub struct Context<P: PaneSource, N: NotifyBackend> {
    pub config: QmonsterConfig,
    pub source: P,
    pub notifier: N,
    pub sink: Box<dyn EventSink>,
    pub archive: Option<ArchiveWriter>,
    pub resolver: IdentityResolver,
    pub policy: Engine,
    pub lifecycle: PaneLifecycle,
    pub rate_limiter: RateLimiter,
    pub pricing: PricingTable,
    pub claude_settings: ClaudeSettings,
    known_pane_ids: Vec<String>,
}
```

Update `Context::new` to default the field:

```rust
    pub fn new(config: QmonsterConfig, source: P, notifier: N, sink: Box<dyn EventSink>) -> Self {
        Self {
            config,
            source,
            notifier,
            sink,
            archive: None,
            resolver: IdentityResolver::new(),
            policy: Engine,
            lifecycle: PaneLifecycle::new(),
            rate_limiter: RateLimiter::new(),
            pricing: PricingTable::empty(),
            claude_settings: ClaudeSettings::empty(),
            known_pane_ids: Vec::new(),
        }
    }
```

Add the builder method (after `with_pricing`):

```rust
    pub fn with_claude_settings(mut self, settings: ClaudeSettings) -> Self {
        self.claude_settings = settings;
        self
    }
```

- [ ] **Step 9: Wire Claude settings loading in main.rs**

Edit `src/main.rs`. Near the pricing import block (which should import `qmonster::policy::pricing::PricingTable` plus the pricing error), add:

```rust
use qmonster::policy::claude_settings::{ClaudeSettings, ClaudeSettingsError};
```

Find the pricing-load block added in v1.11.2 (around line 105). After it, add the analogous Claude-settings block:

```rust
let claude_settings = match ClaudeSettings::default_path() {
    Some(path) => match ClaudeSettings::load_from_path(&path) {
        Ok(s) => s,
        Err(ClaudeSettingsError::Io(io)) if io.kind() == std::io::ErrorKind::NotFound => {
            ClaudeSettings::empty()
        }
        Err(e) => {
            sink.record(qmonster::domain::audit::AuditEvent {
                kind: qmonster::domain::audit::AuditEventKind::ClaudeSettingsLoadFailed,
                pane_id: "n/a".into(),
                severity: qmonster::domain::recommendation::Severity::Warning,
                summary: format!(
                    "claude settings load failed at {}: {}",
                    path.display(),
                    e
                ),
                provider: None,
                role: None,
            });
            eprintln!(
                "qmonster: failed to load claude settings at {}: {e}; claude model badge disabled this session",
                path.display()
            );
            ClaudeSettings::empty()
        }
    },
    None => ClaudeSettings::empty(),
};
```

Then extend the existing `Context::new(...)...with_pricing(...)` builder chain with `.with_claude_settings(claude_settings)`:

```rust
let mut ctx = Context::new(config, source, notifier, sink)
    .with_archive(archive)
    .with_pricing(pricing)
    .with_claude_settings(claude_settings);
```

- [ ] **Step 10: Verify full build + tests**

Run: `cargo build`

Expected: clean compile. If you see `unused import` warnings for `ResolvedIdentity` / `PricingTable` in any adapter, remove them.

Run: `cargo test`

Expected: all tests green. Count unchanged from Task 2 (no new tests added this task; only signature migration + test rewrites). Baseline target: **~285 lib + 6 drift + 24 integration = 315 total**.

- [ ] **Step 11: Clippy + fmt clean**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`

Expected: clean.

- [ ] **Step 12: Commit**

```bash
git add src/adapters/mod.rs src/adapters/claude.rs src/adapters/codex.rs src/adapters/gemini.rs src/adapters/qmonster.rs src/app/event_loop.rs src/app/bootstrap.rs src/main.rs
git commit -m "$(cat <<'EOF'
adapters(v1.12.0-3): migrate ProviderParser to ParserContext struct

Replace ProviderParser::parse(&self, identity, tail, pricing) with
parse(&self, ctx: &ParserContext) so the trait surface stays stable
as Slice 3+ introduce more cross-cutting inputs. ParserContext
bundles identity, tail, pricing, and (new in this slice) a
reference to ClaudeSettings. The Claude, Codex, Gemini, and
Qmonster adapter impls all migrate to the new signature and their
tests construct a ParserContext inline via a local `ctx` helper.

Context gains `claude_settings: ClaudeSettings` (default empty()) +
with_claude_settings builder; main.rs loads
~/.claude/settings.json and routes parse errors through the new
AuditEventKind::ClaudeSettingsLoadFailed audit kind (added in
v1.12.0-1) while keeping eprintln as a dev/non-TUI secondary path,
matching the v1.11.2 pricing-load-failure pattern.

No behavior change in the adapter bodies — Tasks 4-6 add the new
parsing. The claude honesty regression test is renamed from
`_in_slice_1` to `_from_tail` in preparation for Task 6's
settings-backed model_name populate.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Codex status-bar worktree + branch extraction

**Files:**

- Modify: `src/adapters/codex.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/adapters/codex.rs::mod tests` (after the existing Slice 1 tests, using the same `ctx` helper established in Task 3):

```rust
    #[test]
    fn codex_adapter_extracts_worktree_and_branch_from_status_line() {
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, STATUS_LINE, &pricing, &settings);
        let set = CodexAdapter.parse(&c);

        let worktree = set.worktree_path.as_ref().expect("worktree parsed");
        assert_eq!(worktree.value, "~/Qmonster");
        assert_eq!(worktree.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(worktree.provider, Some(Provider::Codex));
        assert_eq!(worktree.confidence, Some(0.95));

        let branch = set.git_branch.as_ref().expect("branch parsed");
        assert_eq!(branch.value, "main");
        assert_eq!(branch.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(branch.provider, Some(Provider::Codex));
        assert_eq!(branch.confidence, Some(0.95));
    }

    #[test]
    fn codex_adapter_branch_extraction_skips_project_name() {
        // Position order in Codex 0.122.0: Context, worktree, model, project, branch.
        // Project ("Qmonster") and branch ("main") are both plain identifiers;
        // the parser must skip project (the token immediately after model)
        // and pick branch (the next plain identifier after that).
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, STATUS_LINE, &pricing, &settings);
        let set = CodexAdapter.parse(&c);

        let branch = set.git_branch.as_ref().expect("branch parsed");
        assert_eq!(
            branch.value, "main",
            "branch must be `main`, not `Qmonster` (the project-name token immediately after model)"
        );
    }

    #[test]
    fn codex_adapter_status_line_without_matching_worktree_token_leaves_worktree_none() {
        // Synthetic status-bar-shaped line with no ~/-prefixed cwd token.
        // Still has all four required fields so the status struct parses;
        // worktree just stays None (per-field independence per v1.11.2).
        let tail = "Context 30% left · no-slash · gpt-5.4 · proj · feat · Context 70% used · 5h 90% · weekly 80% · 0.122.0 · 100K window · 500K used · 400K in · 100K out · <rid> · gp";
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, tail, &pricing, &settings);
        let set = CodexAdapter.parse(&c);

        assert!(
            set.worktree_path.is_none(),
            "no `~/`- or `/`-prefixed token means worktree stays None"
        );
        // context/tokens/model still populate from the same line
        assert!(set.context_pressure.is_some());
        assert!(set.token_count.is_some());
        assert!(set.model_name.is_some());
    }
```

- [ ] **Step 2: Run tests — expect failure**

Run: `cargo test --lib adapters::codex::tests`

Expected: 3 new tests fail because `CodexStatus` has no `worktree_path` / `git_branch` fields yet and the parser doesn't emit them.

- [ ] **Step 3: Extend `CodexStatus` struct**

Edit `src/adapters/codex.rs`. Find the `CodexStatus` struct (the private one near the top of the file, around line 10):

```rust
struct CodexStatus {
    context_pct: u8,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    model: Option<String>,
}
```

Replace with:

```rust
struct CodexStatus {
    context_pct: u8,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    model: Option<String>,
    worktree_path: Option<String>,
    git_branch: Option<String>,
    reasoning_effort: Option<String>, // populated in Task 5
}
```

- [ ] **Step 4: Extend `parse_codex_status_line`**

Find `parse_codex_status_line` in `src/adapters/codex.rs`. The function already iterates `tail.lines().rev()`, picks the first shape-matching line, splits on `·`, and runs a per-token classifier with v1.11.2's newest-line-authoritative behavior.

Replace the function body's per-token loop. The updated code tracks `skip_next_plain_identifier` (one-shot flag) to exclude the project name that sits between model and branch:

```rust
fn parse_codex_status_line(tail: &str) -> Option<CodexStatus> {
    for line in tail.lines().rev() {
        if !(line.contains("Context") && line.contains("% used") && line.contains(" · ")) {
            continue;
        }

        let mut context_pct: Option<u8> = None;
        let mut total_tokens: Option<u64> = None;
        let mut input_tokens: Option<u64> = None;
        let mut output_tokens: Option<u64> = None;
        let mut model: Option<String> = None;
        let mut worktree_path: Option<String> = None;
        let mut git_branch: Option<String> = None;
        let mut skip_next_plain_identifier = false;

        for token in line.split(" · ").map(str::trim) {
            // worktree: first ~/- or /-prefixed token
            if worktree_path.is_none()
                && (token.starts_with('~') || token.starts_with('/'))
            {
                worktree_path = Some(token.to_string());
                continue;
            }
            // model: provider-known prefix
            if model.is_none()
                && (token.starts_with("gpt-")
                    || token.starts_with("claude-")
                    || token.starts_with("gemini-"))
            {
                model = Some(token.to_string());
                // Project name token sits immediately after model. Skip it.
                skip_next_plain_identifier = true;
                continue;
            }
            // Context pressure (Slice 1)
            if let Some(rest) = token.strip_prefix("Context ")
                && let Some(pct_str) = rest.strip_suffix("% used")
                && let Ok(pct) = pct_str.parse::<u8>()
            {
                context_pct = Some(pct);
                continue;
            }
            // Token counts (Slice 1)
            if total_tokens.is_none()
                && let Some(num) = token.strip_suffix(" used")
                && let Some(n) = parse_count_with_suffix(num.trim())
            {
                total_tokens = Some(n);
                continue;
            }
            if input_tokens.is_none()
                && let Some(num) = token.strip_suffix(" in")
                && let Some(n) = parse_count_with_suffix(num.trim())
            {
                input_tokens = Some(n);
                continue;
            }
            if output_tokens.is_none()
                && let Some(num) = token.strip_suffix(" out")
                && let Some(n) = parse_count_with_suffix(num.trim())
            {
                output_tokens = Some(n);
                continue;
            }
            // Plain identifier: either the project name (skip once) or the branch.
            if git_branch.is_none()
                && model.is_some()
                && is_plain_identifier(token)
            {
                if skip_next_plain_identifier {
                    skip_next_plain_identifier = false;
                    continue;
                }
                git_branch = Some(token.to_string());
                continue;
            }
        }

        // Newest-line authoritative (v1.11.2): stop at the first
        // shape-matching line whether or not every field parsed.
        if let (Some(c), Some(tot), Some(inp), Some(out)) =
            (context_pct, total_tokens, input_tokens, output_tokens)
        {
            return Some(CodexStatus {
                context_pct: c,
                total_tokens: tot,
                input_tokens: inp,
                output_tokens: out,
                model,
                worktree_path,
                git_branch,
                reasoning_effort: None, // populated in Task 5
            });
        }
        return None;
    }
    None
}

fn is_plain_identifier(s: &str) -> bool {
    if s.is_empty() || s.len() > 60 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '_' || c == '.' || c == '-')
}
```

- [ ] **Step 5: Emit worktree + branch MetricValues in `CodexAdapter::parse`**

Find the `CodexAdapter::parse` body (around the `set.context_pressure = ...` block). After the existing `model_name`/`cost_usd` assignments, append:

```rust
        set.worktree_path = status.worktree_path.map(|p| {
            MetricValue::new(p, SourceKind::ProviderOfficial)
                .with_confidence(0.95)
                .with_provider(Provider::Codex)
        });
        set.git_branch = status.git_branch.map(|b| {
            MetricValue::new(b, SourceKind::ProviderOfficial)
                .with_confidence(0.95)
                .with_provider(Provider::Codex)
        });
        // reasoning_effort set in Task 5
```

- [ ] **Step 6: Run tests — expect pass**

Run: `cargo test --lib adapters::codex::tests`

Expected: all tests pass including the 3 new ones. Existing Slice 1 tests continue passing.

- [ ] **Step 7: Clippy + fmt clean**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`

Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/adapters/codex.rs
git commit -m "$(cat <<'EOF'
adapters(v1.12.0-4): codex status bar worktree + branch extraction

Extends parse_codex_status_line to populate CodexStatus.worktree_path
and CodexStatus.git_branch from the Codex 0.122.0 status bar. The
per-token loop now uses:
- worktree: first `~/-` or `/-`-prefixed token
- branch: first plain identifier AFTER the model-matching token,
  skipping the project-name token that sits immediately after
  model (one-shot `skip_next_plain_identifier` flag)

Per-field independence from v1.11.2 is preserved: if the shape
heuristic matches but one of the new fields fails its per-token
test, only that field stays None — context, tokens, and model
still populate from the same line.

Both new MetricValues carry SourceKind::ProviderOfficial +
confidence 0.95 + Provider::Codex, matching the existing
model_name stamps.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Codex `/status` box reasoning-effort parser

**Files:**

- Modify: `src/adapters/codex.rs`

- [ ] **Step 1: Write failing tests**

Append to `src/adapters/codex.rs::mod tests`:

```rust
    const STATUS_BOX_SNIPPET: &str = "│  Model:                       gpt-5.4 (reasoning xhigh, summaries auto)                 │";

    const STATUS_LINE_WITH_BOX_ABOVE: &str = concat!(
        "│  Model:                       gpt-5.4 (reasoning xhigh, summaries auto)                 │\n",
        "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp"
    );

    #[test]
    fn codex_adapter_reasoning_effort_reads_xhigh_from_status_box() {
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, STATUS_LINE_WITH_BOX_ABOVE, &pricing, &settings);
        let set = CodexAdapter.parse(&c);

        let effort = set.reasoning_effort.as_ref().expect("effort parsed");
        assert_eq!(effort.value, "xhigh");
        assert_eq!(effort.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(effort.provider, Some(Provider::Codex));
        assert_eq!(
            effort.confidence,
            Some(0.6),
            "confidence 0.6 encodes the stale-risk of /status box tail retention"
        );
    }

    #[test]
    fn codex_adapter_reasoning_effort_falls_through_when_pattern_absent() {
        // Status bar present (so the line parses) but no `reasoning ...` text.
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, STATUS_LINE, &pricing, &settings);
        let set = CodexAdapter.parse(&c);

        assert!(
            set.reasoning_effort.is_none(),
            "no /status box snippet -> reasoning_effort stays None"
        );
        // status bar fields still populate
        assert!(set.context_pressure.is_some());
        assert!(set.token_count.is_some());
    }

    #[test]
    fn codex_adapter_reasoning_effort_absent_when_status_bar_missing() {
        // /status snippet present, but no status bar matches the shape.
        // Because reasoning_effort is populated inside parse_codex_status_line's
        // success path, the whole CodexStatus returns None and no field —
        // reasoning_effort included — is set.
        let id = id();
        let (pricing, _f) = pricing_with_gpt_5_4();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, STATUS_BOX_SNIPPET, &pricing, &settings);
        let set = CodexAdapter.parse(&c);

        assert!(set.reasoning_effort.is_none());
        assert!(set.context_pressure.is_none());
    }
```

- [ ] **Step 2: Run — expect failures**

Run: `cargo test --lib adapters::codex::tests::codex_adapter_reasoning_effort`

Expected: 3 new tests fail (no parser emits reasoning_effort yet).

- [ ] **Step 3: Add the `parse_codex_reasoning_effort` helper**

Add above `parse_codex_status_line` in `src/adapters/codex.rs`:

```rust
/// Scan `tail` for a Codex `/status` box line matching
/// `reasoning (xhigh|high|medium|low|auto)`. Return the captured
/// effort value on the first match; None otherwise. Called by
/// parse_codex_status_line's success path (only after the status
/// bar itself matches the shape heuristic).
fn parse_codex_reasoning_effort(tail: &str) -> Option<String> {
    for line in tail.lines() {
        // Match pattern `reasoning <effort>` where `<effort>` is one of
        // xhigh / high / medium / low / auto. The full /status box row
        // is e.g. `│  Model: gpt-5.4 (reasoning xhigh, summaries auto) │`.
        if let Some(idx) = line.find("reasoning ") {
            let after = &line[idx + "reasoning ".len()..];
            let effort = after
                .split(|c: char| !c.is_ascii_alphabetic())
                .next()
                .unwrap_or("");
            if matches!(effort, "xhigh" | "high" | "medium" | "low" | "auto") {
                return Some(effort.to_string());
            }
        }
    }
    None
}
```

- [ ] **Step 4: Wire the helper into `parse_codex_status_line`**

In `parse_codex_status_line`, change the `Some(CodexStatus { ... })` construction at the end of the success path. Before the `Some(CodexStatus { ... })`:

```rust
            let reasoning_effort = parse_codex_reasoning_effort(tail);
            return Some(CodexStatus {
                context_pct: c,
                total_tokens: tot,
                input_tokens: inp,
                output_tokens: out,
                model,
                worktree_path,
                git_branch,
                reasoning_effort,
            });
```

(Replace the earlier `reasoning_effort: None, // populated in Task 5` comment from Task 4.)

- [ ] **Step 5: Emit the MetricValue in `CodexAdapter::parse`**

In the `CodexAdapter::parse` body, after the `set.git_branch = ...` line from Task 4, add:

```rust
        set.reasoning_effort = status.reasoning_effort.map(|e| {
            MetricValue::new(e, SourceKind::ProviderOfficial)
                .with_confidence(0.6)  // stale-risk from /status box tail retention
                .with_provider(Provider::Codex)
        });
```

- [ ] **Step 6: Run tests — expect pass**

Run: `cargo test --lib adapters::codex::tests`

Expected: all tests pass including the 3 new reasoning-effort ones.

- [ ] **Step 7: Clippy + fmt clean**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`

Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/adapters/codex.rs
git commit -m "$(cat <<'EOF'
adapters(v1.12.0-5): codex /status box reasoning effort parser

Adds parse_codex_reasoning_effort(tail) -> Option<String>. Matches
Codex /status box lines like `│ Model: gpt-5.4 (reasoning xhigh,
summaries auto) │` with a cheap index-based search; accepted
effort values: xhigh / high / medium / low / auto. Called from
parse_codex_status_line's success path so reasoning_effort only
populates when the status bar itself parses — isolating us from
drift when /status box content is present without a valid status
bar.

The resulting MetricValue is SourceKind::ProviderOfficial (Codex
prints it) with confidence 0.6 (not 0.95). The lower confidence
encodes stale-risk: `/status` output sits in the tail until
scrolled off, so the value may not reflect the currently-active
reasoning setting if the operator ran /status minutes ago.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Claude model_name from `ClaudeSettings`

**Files:**

- Modify: `src/adapters/claude.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/adapters/claude.rs::mod tests`:

```rust
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn settings_with_model(m: &str) -> (ClaudeSettings, NamedTempFile) {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"model": "{}"}}"#, m).unwrap();
        let s = ClaudeSettings::load_from_path(f.path()).unwrap();
        (s, f)
    }

    #[test]
    fn claude_adapter_populates_model_name_when_settings_has_model() {
        let id = id();
        let pricing = PricingTable::empty();
        let (settings, _f) = settings_with_model("claude-sonnet-4-6");
        let c = ctx(&id, "any tail", &pricing, &settings);
        let set = ClaudeAdapter.parse(&c);

        let m = set.model_name.as_ref().expect("model populated from settings");
        assert_eq!(m.value, "claude-sonnet-4-6");
        assert_eq!(m.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(m.provider, Some(Provider::Claude));
        assert_eq!(
            m.confidence,
            Some(0.9),
            "confidence 0.9 < Codex's 0.95 because CLI flags can override settings"
        );
    }

    #[test]
    fn claude_adapter_leaves_cost_usd_none_regardless_of_settings() {
        // Honesty regression: Claude cost requires input-token data
        // which Claude's tail does not expose. Settings presence must
        // not accidentally unlock cost computation.
        let id = id();
        let pricing = PricingTable::empty();
        let (settings, _f) = settings_with_model("claude-sonnet-4-6");
        let c = ctx(&id, "✶ Working… (↓ 100 tokens)", &pricing, &settings);
        let set = ClaudeAdapter.parse(&c);

        assert!(set.model_name.is_some(), "model populates from settings");
        assert!(
            set.cost_usd.is_none(),
            "cost must stay None — no input-token source on Claude tail"
        );
    }
```

Also **rename and re-scope** the existing Slice 1 regression test. The current test name (after Task 3's rename) is `claude_adapter_never_populates_model_name_from_tail`. Update its body if needed to reflect the new contract: tail-only → model_name None; settings empty in this test:

```rust
    #[test]
    fn claude_adapter_never_populates_model_name_from_tail() {
        // Honesty regression: with EMPTY settings, the Claude adapter
        // must not populate model_name from the tail alone. Claude's
        // tail does not expose the model; only the settings-read path
        // (tested separately) may populate this field.
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let c = ctx(&id, "✶ Working… (↓ 100 tokens)", &pricing, &settings);
        let set = ClaudeAdapter.parse(&c);
        assert!(
            set.model_name.is_none(),
            "Claude tail must not surface model_name without a ClaudeSettings source"
        );
    }
```

- [ ] **Step 2: Run — expect failures**

Run: `cargo test --lib adapters::claude::tests`

Expected: 2 new tests fail, the renamed regression still passes (still asserting None because the Claude adapter doesn't yet consult `ctx.claude_settings`).

- [ ] **Step 3: Update `ClaudeAdapter::parse` to read settings**

In `src/adapters/claude.rs`, find `ClaudeAdapter::parse` (post-Task-3 signature is `fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet`). The body already handles common signals + Claude context-pressure heuristic + output-token parsing. Append the model-from-settings branch **at the end** of the function, just before `set`:

```rust
        // Slice 2: model from external ~/.claude/settings.json (not tail).
        // Confidence 0.9 (< Codex's 0.95) because CLI flags can override
        // the settings value at invocation time.
        if let Some(m) = ctx.claude_settings.model() {
            set.model_name = Some(
                MetricValue::new(m.to_string(), SourceKind::ProviderOfficial)
                    .with_confidence(0.9)
                    .with_provider(Provider::Claude),
            );
        }

        set
    }
```

Remove the old final `set` return if you moved it. (Match the structure: the existing body ends with `set`, your insertion goes immediately above that line.)

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test --lib adapters::claude::tests`

Expected: all 3 new tests + the renamed regression + all Slice 1 Claude tests pass.

- [ ] **Step 5: Clippy + fmt clean**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/adapters/claude.rs
git commit -m "$(cat <<'EOF'
adapters(v1.12.0-6): claude model_name from ClaudeSettings

ClaudeAdapter::parse now consults ctx.claude_settings.model() and
populates set.model_name with SourceKind::ProviderOfficial +
confidence 0.9 + Provider::Claude when the settings file provided
a value. The Claude tail still does not surface the model — the
honesty regression test (renamed in Task 3 from
`_never_populates_..._in_slice_1` to `_never_populates_model_name_from_tail`)
continues to lock that property, now with an explicit empty
ClaudeSettings in the test context to make the contract precise.

Confidence 0.9 (< Codex's status-bar 0.95) encodes the risk that
a CLI invocation-time `--model` flag can override whatever is in
settings.json.

Two new tests cover the populate path (`_populates_model_name_when_settings_has_model`)
and the honesty-regression-independent-of-settings property
(`_leaves_cost_usd_none_regardless_of_settings`).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: UI — 2-row `metric_badge_line` + new field renders

**Files:**

- Modify: `src/ui/panels.rs`

- [ ] **Step 1: Write failing tests**

Find the `mod tests` block at the bottom of `src/ui/panels.rs`. Append:

```rust
    #[test]
    fn metric_row_renders_git_branch_and_worktree_and_effort() {
        let s = crate::domain::signal::SignalSet {
            git_branch: Some(crate::domain::signal::MetricValue::new(
                "main".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            worktree_path: Some(crate::domain::signal::MetricValue::new(
                "~/Qmonster".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            reasoning_effort: Some(crate::domain::signal::MetricValue::new(
                "xhigh".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            ..crate::domain::signal::SignalSet::default()
        };
        let row = metric_row(&s);
        assert!(row.contains("branch main"), "row: {row}");
        assert!(row.contains("path ~/Qmonster"), "row: {row}");
        assert!(row.contains("effort xhigh"), "row: {row}");
    }

    #[test]
    fn metric_badge_line_returns_two_rows_when_context_fields_present() {
        let s = crate::domain::signal::SignalSet {
            token_count: Some(crate::domain::signal::MetricValue::new(
                1_530_000_u64,
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            git_branch: Some(crate::domain::signal::MetricValue::new(
                "main".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            ..crate::domain::signal::SignalSet::default()
        };
        let rows = metric_badge_line(&s);
        assert_eq!(
            rows.len(),
            2,
            "TOKENS on row 1 + BRANCH on row 2 → exactly two rows"
        );
    }

    #[test]
    fn metric_badge_line_returns_single_row_when_only_primary_fields_present() {
        let s = crate::domain::signal::SignalSet {
            token_count: Some(crate::domain::signal::MetricValue::new(
                1_530_000_u64,
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            ..crate::domain::signal::SignalSet::default()
        };
        let rows = metric_badge_line(&s);
        assert_eq!(rows.len(), 1, "primary fields only → one row");
    }

    #[test]
    fn metric_badge_line_returns_empty_vec_when_no_fields() {
        let rows = metric_badge_line(&crate::domain::signal::SignalSet::default());
        assert!(
            rows.is_empty(),
            "no fields populated → empty Vec (not a single empty Line)"
        );
    }
```

- [ ] **Step 2: Run — expect compile failure**

Run: `cargo test --lib ui::panels`

Expected: compile error — `metric_row` has no git_branch/worktree_path/reasoning_effort branches; `metric_badge_line` returns `Option<Line>` but tests expect a `Vec`.

- [ ] **Step 3: Extend `metric_row`**

Find `metric_row` in `src/ui/panels.rs` (around line 341). The current body handles context_pressure, token_count, cost_usd, model_name. Append three more branches before the `parts.join("  ")` line:

```rust
pub fn metric_row(s: &SignalSet) -> String {
    let mut parts = Vec::new();
    if let Some(m) = s.context_pressure.as_ref() {
        parts.push(format!(
            "context {:.0}% [{}]",
            m.value * 100.0,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.token_count.as_ref() {
        parts.push(format!(
            "tokens {} [{}]",
            format_count_with_suffix(m.value),
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.cost_usd.as_ref() {
        parts.push(format!(
            "cost ${:.2} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.model_name.as_ref() {
        parts.push(format!(
            "model {} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.git_branch.as_ref() {
        parts.push(format!(
            "branch {} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.worktree_path.as_ref() {
        parts.push(format!(
            "path {} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.reasoning_effort.as_ref() {
        parts.push(format!(
            "effort {} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    parts.join("  ")
}
```

- [ ] **Step 4: Refactor `metric_badge_line` to return `Vec<Line>`**

Find `metric_badge_line` in `src/ui/panels.rs` (around line 425). Current signature: `fn metric_badge_line(signals: &SignalSet) -> Option<Line<'static>>`. Replace the function with these three functions (the outer one plus two row helpers):

```rust
fn metric_badge_line(signals: &SignalSet) -> Vec<Line<'static>> {
    let mut rows = Vec::with_capacity(2);
    if let Some(line) = primary_metric_row(signals) {
        rows.push(line);
    }
    if let Some(line) = context_metric_row(signals) {
        rows.push(line);
    }
    rows
}

fn primary_metric_row(signals: &SignalSet) -> Option<Line<'static>> {
    let mut spans = vec![Span::raw(format!("{:<8}: ", "metrics"))];
    let mut has_any = false;

    if let Some(metric) = signals.context_pressure.as_ref() {
        has_any = true;
        spans.push(Span::styled(
            format!(" CTX {:.0}% ", metric.value * 100.0),
            theme::severity_badge_style(context_metric_severity(metric.value)),
        ));
    }
    if let Some(metric) = signals.token_count.as_ref() {
        if has_any {
            spans.push(Span::raw(" "));
        }
        has_any = true;
        spans.push(Span::styled(
            format!(
                " TOKENS {} [{}] ",
                format_count_with_suffix(metric.value),
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        ));
    }
    if let Some(metric) = signals.cost_usd.as_ref() {
        if has_any {
            spans.push(Span::raw(" "));
        }
        has_any = true;
        spans.push(Span::styled(
            format!(
                " COST ${:.2} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        ));
    }
    if let Some(metric) = signals.model_name.as_ref() {
        if has_any {
            spans.push(Span::raw(" "));
        }
        has_any = true;
        spans.push(Span::styled(
            format!(
                " MODEL {} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        ));
    }

    has_any.then(|| Line::from(spans))
}

fn context_metric_row(signals: &SignalSet) -> Option<Line<'static>> {
    let mut spans = vec![Span::raw(format!("{:<8}: ", "context"))];
    let mut has_any = false;

    if let Some(metric) = signals.git_branch.as_ref() {
        has_any = true;
        spans.push(Span::styled(
            format!(
                " BRANCH {} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        ));
    }
    if let Some(metric) = signals.worktree_path.as_ref() {
        if has_any {
            spans.push(Span::raw(" "));
        }
        has_any = true;
        spans.push(Span::styled(
            format!(
                " PATH {} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        ));
    }
    if let Some(metric) = signals.reasoning_effort.as_ref() {
        if has_any {
            spans.push(Span::raw(" "));
        }
        has_any = true;
        spans.push(Span::styled(
            format!(
                " EFFORT {} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        ));
    }

    has_any.then(|| Line::from(spans))
}
```

- [ ] **Step 5: Update the caller in `render_pane`**

Find the caller of `metric_badge_line` in `src/ui/panels.rs` (around line 282). Current pattern is `if let Some(line) = metric_badge_line(&report.signals) { items.push(ListItem::new(line)); }`. Replace with:

```rust
        for row in metric_badge_line(&report.signals) {
            items.push(ListItem::new(row));
        }
```

- [ ] **Step 6: Run tests — expect pass**

Run: `cargo test --lib ui::panels`

Expected: all UI tests pass including the 4 new ones. Verify that the existing `metric_row_renders_model_name_line_when_populated` and `metric_row_uses_count_suffix_for_tokens` tests (from Slice 1 v1.11.0-7) still pass with the extended 7-branch `metric_row`.

- [ ] **Step 7: Full test run**

Run: `cargo test`

Expected: full suite green. Cumulative count: 285 + 2 (Task 2) + 4 (Task 4) + 3 (Task 5) + 3 (Task 6 — 2 new + 1 renamed) + 4 (Task 7) = ~301 lib, plus existing 6 drift and 24 integration = **~331 total**. Integration count still 24 — Task 8 adds the new integration tests.

- [ ] **Step 8: Clippy + fmt clean**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`

Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add src/ui/panels.rs
git commit -m "$(cat <<'EOF'
ui(v1.12.0-7): 2-row metric_badge_line + render new fields

metric_badge_line now returns Vec<Line<'static>> instead of
Option<Line<'static>>. Row 1 (primary_metric_row) keeps the
existing CTX/TOKENS/COST/MODEL order; Row 2 (context_metric_row,
new) carries BRANCH/PATH/EFFORT when any of the three Slice 2
fields populates. Each row independently gates on having at
least one Some(_) field, so a Claude pane with only tokens still
produces exactly one row and a Codex pane with the full seven
fields produces two. render_pane's caller iterates the Vec.

metric_row (text surface for --once) adds three matching
branches after the existing MODEL branch, preserving the order
CTX → TOKENS → COST → MODEL → BRANCH → PATH → EFFORT.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Integration tests + fixture module

**Files:**

- Create: `tests/fixtures/codex.rs`
- Modify: `tests/event_loop_integration.rs`

- [ ] **Step 1: Create the fixture file**

Write `tests/fixtures/codex.rs`:

```rust
//! Shared Codex status-bar + /status-box fixtures for integration tests.
//!
//! Unit tests in `src/adapters/codex.rs::mod tests` keep their own inline
//! fixtures so `src/` stays self-contained; this module is for external
//! integration tests under `tests/`. When Codex CLI formats drift, both
//! copies must be updated together.

pub const CODEX_STATUS_FIXTURE_V0_122_0: &str = "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp";

pub const CODEX_STATUS_BOX_FIXTURE: &str = "│  Model:                       gpt-5.4 (reasoning xhigh, summaries auto)                 │";
```

Cargo's integration test harness compiles every `tests/*.rs` top-level file as a separate binary but treats files under `tests/<subdir>/` as non-binary modules that can be imported. We expose `tests/fixtures/codex.rs` to the integration binary via a `#[path]` attribute below — no `tests/fixtures/mod.rs` is needed.

- [ ] **Step 2: Write failing integration tests**

Append to `tests/event_loop_integration.rs`:

```rust
#[path = "fixtures/codex.rs"]
mod codex_fixtures;

#[test]
fn codex_status_line_end_to_end_populates_seven_metrics() {
    use qmonster::adapters::{ParserContext, parse_for};
    use qmonster::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use qmonster::domain::origin::SourceKind;
    use qmonster::policy::claude_settings::ClaudeSettings;
    use qmonster::policy::pricing::PricingTable;
    use std::io::Write;
    use tempfile::NamedTempFile;

    let identity = ResolvedIdentity {
        identity: PaneIdentity {
            provider: Provider::Codex,
            instance: 1,
            role: Role::Review,
            pane_id: "%9".into(),
        },
        confidence: IdentityConfidence::High,
    };

    // Tail has the /status box AND the status bar (newest line at bottom).
    let tail = format!(
        "{}\n{}",
        codex_fixtures::CODEX_STATUS_BOX_FIXTURE,
        codex_fixtures::CODEX_STATUS_FIXTURE_V0_122_0
    );

    // Pricing: operator-supplied $1/M input, $10/M output for gpt-5.4.
    let mut pricing_toml = NamedTempFile::new().unwrap();
    write!(
        pricing_toml,
        r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.00
output_per_1m = 10.00
"#
    )
    .unwrap();
    let pricing = PricingTable::load_from_toml(pricing_toml.path()).unwrap();
    let claude_settings = ClaudeSettings::empty();

    let ctx = ParserContext {
        identity: &identity,
        tail: &tail,
        pricing: &pricing,
        claude_settings: &claude_settings,
    };

    let signals = parse_for(&ctx);

    // Slice 1 fields (still populated, provider-official)
    assert_eq!(
        signals.context_pressure.as_ref().unwrap().source_kind,
        SourceKind::ProviderOfficial
    );
    assert_eq!(signals.token_count.as_ref().unwrap().value, 1_530_000);
    assert_eq!(signals.model_name.as_ref().unwrap().value, "gpt-5.4");
    let cost = signals.cost_usd.as_ref().unwrap();
    assert!((cost.value - 1.714).abs() < 0.01);
    assert_eq!(cost.source_kind, SourceKind::Estimated);

    // Slice 2 fields — the new three
    let branch = signals.git_branch.as_ref().expect("branch populated");
    assert_eq!(branch.value, "main");
    assert_eq!(branch.source_kind, SourceKind::ProviderOfficial);

    let worktree = signals.worktree_path.as_ref().expect("worktree populated");
    assert_eq!(worktree.value, "~/Qmonster");
    assert_eq!(worktree.source_kind, SourceKind::ProviderOfficial);

    let effort = signals.reasoning_effort.as_ref().expect("effort populated");
    assert_eq!(effort.value, "xhigh");
    assert_eq!(effort.source_kind, SourceKind::ProviderOfficial);
    assert_eq!(effort.confidence, Some(0.6));
}

#[test]
fn claude_adapter_end_to_end_reads_model_from_claude_settings() {
    use qmonster::adapters::{ParserContext, parse_for};
    use qmonster::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use qmonster::domain::origin::SourceKind;
    use qmonster::policy::claude_settings::ClaudeSettings;
    use qmonster::policy::pricing::PricingTable;
    use std::io::Write;
    use tempfile::NamedTempFile;

    let identity = ResolvedIdentity {
        identity: PaneIdentity {
            provider: Provider::Claude,
            instance: 1,
            role: Role::Main,
            pane_id: "%1".into(),
        },
        confidence: IdentityConfidence::High,
    };

    let mut settings_json = NamedTempFile::new().unwrap();
    write!(settings_json, r#"{{"model": "claude-sonnet-4-6"}}"#).unwrap();
    let claude_settings = ClaudeSettings::load_from_path(settings_json.path()).unwrap();
    let pricing = PricingTable::empty();

    let tail = "✶ Working… (1m · ↓ 500 tokens)";
    let ctx = ParserContext {
        identity: &identity,
        tail,
        pricing: &pricing,
        claude_settings: &claude_settings,
    };

    let signals = parse_for(&ctx);

    // Slice 2: model comes from settings, not tail.
    let model = signals.model_name.as_ref().expect("model populated from settings");
    assert_eq!(model.value, "claude-sonnet-4-6");
    assert_eq!(model.source_kind, SourceKind::ProviderOfficial);
    assert_eq!(model.confidence, Some(0.9));
    assert_eq!(model.provider, Some(Provider::Claude));

    // Claude tail still populates token_count (Slice 1 behavior).
    assert_eq!(signals.token_count.as_ref().unwrap().value, 500);

    // Honesty: cost stays None (no input tokens on Claude tail).
    assert!(signals.cost_usd.is_none());
}
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test --test event_loop_integration`

Expected: 24 existing + 2 new = 26 integration tests pass.

- [ ] **Step 4: Full verification**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`

Expected totals:

- lib: ~301 (Task 2 +2, Task 4 +3, Task 5 +3, Task 6 +2, Task 7 +4; Task 1's +5 already counted after Task 1)
- drift: 6 (unchanged)
- integration: 26 (+2 from Task 8)
- Grand total ≈ **333 tests green**

Exact lib count depends on whether `cargo test --lib` reports the renamed Claude test. The rename is a rename-in-place (no net new test), so total = baseline 307 + 26 new = **333 green, +26 net vs v1.11.3**.

- [ ] **Step 5: Commit**

```bash
git add tests/fixtures/codex.rs tests/event_loop_integration.rs
git commit -m "$(cat <<'EOF'
test(v1.12.0-8): integration tests for end-to-end seven-metric populate + claude settings model

Two end-to-end tests pin the Slice 2 observability contract at
the public parse_for API:

1. codex_status_line_end_to_end_populates_seven_metrics —
   a tail containing both the /status box (with reasoning xhigh)
   and the status bar (Codex 0.122.0 fixture). Asserts all seven
   Codex fields populate: context_pressure, token_count,
   model_name, cost_usd (Estimated via operator-supplied TOML
   pricing), git_branch, worktree_path, reasoning_effort.

2. claude_adapter_end_to_end_reads_model_from_claude_settings —
   a tempfile ClaudeSettings with `{"model":"claude-sonnet-4-6"}`
   feeds through parse_for. Asserts model_name populates at
   ProviderOfficial / 0.9 confidence, token_count still parses
   from the Claude working-line format, and cost_usd honestly
   stays None (Claude input-token count is not exposed in tail).

tests/fixtures/codex.rs introduces CODEX_STATUS_FIXTURE_V0_122_0
and CODEX_STATUS_BOX_FIXTURE as shared constants for integration
tests. src/adapters/codex.rs unit tests keep their own inline
fixtures intentionally so the test module stays self-contained.
When Codex CLI formats drift, both copies must be updated.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Verification, state update, annotated tag

**Files:**

- Modify: `.mission/CURRENT_STATE.md` (gitignored)
- Git: annotated tag `v1.12.0`

- [ ] **Step 1: Final verification**

Run each of these in order, each expected clean:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
cargo test --test event_loop_integration
cargo test
```

Expected totals:

- lib: ~301 pass
- drift (main.rs): 6 pass (unchanged)
- integration: 26 pass
- Total ≈ **333 green** (+26 vs v1.11.3 baseline 307)

- [ ] **Step 2: Update `.mission/CURRENT_STATE.md`**

Replace the first ~11 lines of `.mission/CURRENT_STATE.md` with the following (actual counts confirmed by Step 1):

```markdown
# CURRENT_STATE

_Last updated: 2026-04-24 (claude:1:main, after observability field expansion v1.12.0 Slice 2)_

## Mission

- Title: Qmonster v0.4.0 — observability field expansion v1.12.0 Slice 2 (SignalSet gains git_branch + worktree_path + reasoning_effort; Claude model_name now sourced from ~/.claude/settings.json via new ClaudeSettings module; ProviderParser::parse migrates from 3 positional args to a &ParserContext struct; 2-row metric_badge_line in the TUI; 333 tests green +26 vs v1.11.3)
- Version: 1.12.0
- Branch / worktree: `main` + annotated tags `v1.10.7`..`v1.10.9`, `v1.11.0`..`v1.11.3`, `v1.12.0`
- Phase: **Slice 2 shipped** — closes the v1.11.0 reviewer follow-ups on both SignalSet field expansion and ParserContext struct migration. Slice 1 (P0-1 provider usage-hint parsing) extended with the three remaining observability fields and Claude model surfacing from an operator config file. ProviderParser::parse now takes a single &ParserContext struct bundling identity, tail, pricing (Slice 1), and claude_settings (new). Codex status-bar parser tracks a `skip_next_plain_identifier` one-shot flag so the project-name token between model and branch is excluded; parse_codex_reasoning_effort scans the /status box inside the success path and encodes stale-risk via confidence 0.6. Claude adapter populates model_name from ctx.claude_settings.model() when present (ProviderOfficial / 0.9) while the `_never_populates_model_name_from_tail` regression test still locks tail-based absence. ClaudeSettings loading failures route through AuditEventKind::ClaudeSettingsLoadFailed (22nd variant) matching v1.11.2's PricingLoadFailed pattern; eprintln retained as dev/non-TUI secondary path. UI's metric_badge_line returns Vec<Line> (row 1: CTX/TOKENS/COST/MODEL; row 2: BRANCH/PATH/EFFORT); caller iterates. 333 tests green (+26 net); clippy + fmt clean. Commits `v1.12.0-1..-8`, tag `v1.12.0`. Review cycle follow-up expected per project convention.
```

Replace the rest of the file per the existing structure — or leave as-is if the file appends-only.

- [ ] **Step 3: Create the annotated tag**

```bash
git tag -a v1.12.0 -m "$(cat <<'EOF'
v1.12.0 — observability field expansion Slice 2

Closes the Slice 1 reviewer follow-ups on SignalSet field expansion
and the ParserContext struct migration. Both were flagged as Slice 2
priorities in the v1.11.0 Codex + Gemini cross-check.

Schema: SignalSet gains git_branch, worktree_path, reasoning_effort
(all Option<MetricValue<String>>). Claude model_name populates from
~/.claude/settings.json via the new ClaudeSettings module.

Trait: ProviderParser::parse moves from (identity, tail, pricing)
positional args to a single &ParserContext struct that also carries
a reference to ClaudeSettings. Codex is the only adapter that needs
all four; Claude consumes tail + claude_settings; Gemini + Qmonster
ignore the rest.

Parsing: parse_codex_status_line extended with a per-token
skip_next_plain_identifier one-shot flag to exclude the project
name between model and branch; parse_codex_reasoning_effort
reads the /status box inside the status-bar success path and
stamps confidence 0.6 to encode stale-risk.

Audit: new AuditEventKind::ClaudeSettingsLoadFailed for parse
errors on settings.json (matches v1.11.2's PricingLoadFailed
pattern).

UI: metric_badge_line returns Vec<Line>; row 1 primary metrics
(CTX/TOKENS/COST/MODEL), row 2 context metrics
(BRANCH/PATH/EFFORT). render_pane iterates.

Tests: 333 green (+26 net vs v1.11.3). 8 commits v1.12.0-1..-8.
clippy + fmt clean.

Next: Codex + Gemini cross-check review round per the confirm-archive
convention; expect approve-with-fixes given this slice's larger
surface.
EOF
)"
```

- [ ] **Step 4: Verify describe**

Run: `git describe --tags --always --dirty`

Expected: `v1.12.0`.

- [ ] **Step 5: State file is gitignored — no additional commit needed**

`.mission/CURRENT_STATE.md` is gitignored per project convention (confirmed in v1.11.x rounds). The tag itself is the canonical marker.

---

## Out-of-plan follow-ups

- **Codex + Gemini cross-check** — this slice's review round is expected to mirror the v1.11.0 pattern: both reviewers will likely return `approve-with-fixes` given the larger surface (trait signature change, new external file reader, new parser surface, 3-row UI). Expected must-fix topics: (a) Codex branch-extraction drift risk (the `skip_next_plain_identifier` heuristic depends on Codex 0.122.0 position layout); (b) `~/.claude/settings.json` path-resolution edge cases (operators on non-HOME-env sandboxes); (c) reasoning_effort stale-risk communication to operators (confidence 0.6 isn't rendered in the UI; reviewers may ask for a distinct label). Track as a v1.12.1 remediation round per convention.

- **Ledger catch-up** — v1.10.9, v1.11.0, v1.11.1 individual change_sequence entries remain open from v1.11.3's follow-up list. Consider bundling a ledger-only backfill commit before or after the v1.12.0 review round so the mission-history narrative is complete.

- **`pricing.example.toml` header `COST [Estimate]` alignment** — carried forward from v1.11.2. The Slice 2 commits don't touch it; if it still says `COST [Est]` in any operator-facing doc, clean up in the v1.12.x remediation.

- **Fixture dedup** — the Codex unit-test inline fixture + `tests/fixtures/codex.rs` hold two copies of the same status-bar string. If maintenance becomes burdensome, Slice 3 can promote to a single `pub(crate)` const re-exported into integration scope.

- **Slice 3 preview** — Gemini observability fields (pending archive samples), reasoning_effort session-level caching if the 0.6-confidence approach proves insufficient, optional `CodexSettings` or `GeminiSettings` modules mirroring `ClaudeSettings` if operator-curated config surfaces grow, and `metric_badge_line` width-awareness (v1.10.9 pre-wrap heuristic) if Row 2 begins wrapping on narrow terminals.
