# P0-1 Slice 1 — Provider usage hint parsing (v1.11.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate `SignalSet.context_pressure`, `.token_count`, `.model_name`, `.cost_usd` from real Codex and Claude pane tails so the TUI shows actual operating numbers instead of blanks.

**Architecture:** Add an operator-curated `PricingTable` (loaded from `config/pricing.toml`); extend `ProviderParser::parse` to accept `&PricingTable`; Codex adapter parses its single-line status bar (context %, tokens, model, input/output split → cost via lookup); Claude adapter parses the working-status line for output token count; Claude `model_name`/`cost_usd` and Gemini all deferred to later slices with honesty rationale.

**Tech Stack:** Rust, existing `SignalSet` / `MetricValue<T>` / `SourceKind` types, `serde` + `toml = "0.8"` (already in Cargo.toml), ratatui UI renderer already wired for these fields.

**Spec:** `docs/superpowers/specs/2026-04-23-p0-1-slice-1-usage-hint-parsing-design.md` (commit `f1bf046`).

**Target version tag:** `v1.11.0` at the last commit in this plan.

---

## File Structure

**Create:**

- `src/policy/pricing.rs` — `PricingTable`, `PricingRates`, `PricingError`, TOML loader, lookup
- `config/pricing.example.toml` — operator-facing placeholder template

**Modify:**

- `src/policy/mod.rs` — add `pub mod pricing;` + re-exports
- `src/domain/signal.rs` — add `model_name: Option<MetricValue<String>>` field to `SignalSet`
- `src/adapters/mod.rs` — extend `ProviderParser::parse` signature + `parse_for` to take `&PricingTable`
- `src/adapters/common.rs` — add `parse_count_with_suffix` helper; wire `model_name: None` into `parse_common_signals` default
- `src/adapters/claude.rs` — accept `_pricing`; add `parse_claude_output_tokens`; populate `token_count`
- `src/adapters/codex.rs` — accept `pricing`; add `parse_codex_status_line` + `CodexStatus` struct; populate 4 metrics incl. cost
- `src/adapters/gemini.rs` — accept `_pricing` (mechanical)
- `src/adapters/qmonster.rs` — accept `_pricing` (mechanical)
- `src/app/event_loop.rs` — load `PricingTable` once; pass `&pricing` to `parse_for`
- `src/app/bootstrap.rs` (or similar) — store `PricingTable` in `Context` (if that's where config lives; Task 3 verifies)
- `src/ui/labels.rs` — add `format_count_with_suffix`
- `src/ui/panels.rs` — render model badge in `metric_row` + `metric_badge_line`; use `format_count_with_suffix` for `token_count` display
- `.gitignore` — add `config/pricing.toml`

**Test:**

- Unit tests inline in each modified file's `mod tests`
- `tests/event_loop_integration.rs` — integration test

---

## Task 1: PricingTable module + config template

**Files:**

- Create: `src/policy/pricing.rs`
- Modify: `src/policy/mod.rs`
- Create: `config/pricing.example.toml`
- Modify: `.gitignore`

- [ ] **Step 1: Write the failing tests**

Add to top of new `src/policy/pricing.rs`:

```rust
use crate::domain::identity::Provider;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricingRates {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    #[error("pricing config not found at {0}")]
    NotFound(String),
    #[error("failed to read pricing config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse pricing config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("unknown provider in pricing entry: {0}")]
    UnknownProvider(String),
}

#[derive(Debug, Default, Clone)]
pub struct PricingTable {
    entries: HashMap<(Provider, String), PricingRates>,
}

#[derive(Debug, Deserialize)]
struct PricingFile {
    #[serde(default)]
    entries: Vec<PricingEntry>,
}

#[derive(Debug, Deserialize)]
struct PricingEntry {
    provider: String,
    model: String,
    #[serde(default)]
    input_per_1m: f64,
    #[serde(default)]
    output_per_1m: f64,
}

impl PricingTable {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn load_from_toml(path: &Path) -> Result<Self, PricingError> {
        let text = fs::read_to_string(path)?;
        let file: PricingFile = toml::from_str(&text)?;
        let mut entries = HashMap::new();
        for e in file.entries {
            let provider = parse_provider(&e.provider)?;
            entries.insert(
                (provider, e.model),
                PricingRates {
                    input_per_1m: e.input_per_1m,
                    output_per_1m: e.output_per_1m,
                },
            );
        }
        Ok(Self { entries })
    }

    pub fn load_from_toml_or_empty(path: &Path) -> Self {
        Self::load_from_toml(path).unwrap_or_else(|_| Self::empty())
    }

    pub fn lookup(&self, provider: Provider, model: &str) -> Option<&PricingRates> {
        self.entries
            .get(&(provider, model.to_string()))
            .filter(|r| r.input_per_1m > 0.0 || r.output_per_1m > 0.0)
    }
}

fn parse_provider(s: &str) -> Result<Provider, PricingError> {
    match s.to_lowercase().as_str() {
        "claude" => Ok(Provider::Claude),
        "codex" => Ok(Provider::Codex),
        "gemini" => Ok(Provider::Gemini),
        "qmonster" => Ok(Provider::Qmonster),
        _ => Err(PricingError::UnknownProvider(s.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_toml(body: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", body).unwrap();
        f
    }

    #[test]
    fn pricing_table_empty_has_no_entries() {
        let t = PricingTable::empty();
        assert!(t.lookup(Provider::Codex, "gpt-5.4").is_none());
    }

    #[test]
    fn pricing_table_loads_entries_from_toml() {
        let f = write_toml(
            r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.25
output_per_1m = 10.00

[[entries]]
provider = "claude"
model = "claude-sonnet-4-6"
input_per_1m = 3.00
output_per_1m = 15.00
"#,
        );
        let t = PricingTable::load_from_toml(f.path()).unwrap();
        let r = t.lookup(Provider::Codex, "gpt-5.4").unwrap();
        assert!((r.input_per_1m - 1.25).abs() < f64::EPSILON);
        assert!((r.output_per_1m - 10.00).abs() < f64::EPSILON);
        let r2 = t.lookup(Provider::Claude, "claude-sonnet-4-6").unwrap();
        assert!((r2.output_per_1m - 15.00).abs() < f64::EPSILON);
    }

    #[test]
    fn pricing_table_lookup_returns_none_for_missing_entry() {
        let f = write_toml(
            r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.0
output_per_1m = 10.0
"#,
        );
        let t = PricingTable::load_from_toml(f.path()).unwrap();
        assert!(t.lookup(Provider::Codex, "gpt-4o").is_none());
        assert!(t.lookup(Provider::Claude, "gpt-5.4").is_none());
    }

    #[test]
    fn pricing_table_treats_zero_rate_entries_as_unset() {
        let f = write_toml(
            r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 0.0
output_per_1m = 0.0
"#,
        );
        let t = PricingTable::load_from_toml(f.path()).unwrap();
        // Entry exists in file but zeros count as "operator has not filled in".
        assert!(t.lookup(Provider::Codex, "gpt-5.4").is_none());
    }

    #[test]
    fn pricing_table_load_from_toml_or_empty_falls_back_on_missing() {
        let t = PricingTable::load_from_toml_or_empty(Path::new("/nonexistent/pricing.toml"));
        assert!(t.lookup(Provider::Codex, "gpt-5.4").is_none());
    }
}
```

- [ ] **Step 2: Register the module**

Edit `src/policy/mod.rs` — add the line:

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

- [ ] **Step 3: Run tests — expect pass**

Run: `cargo test --lib policy::pricing::tests -- --nocapture`
Expected: 5 tests pass.

- [ ] **Step 4: Create operator config template**

Write `config/pricing.example.toml`:

```toml
# Qmonster pricing table (Estimated, operator-curated)
#
# Values are USD per 1 million tokens. Leave placeholders at 0.00 to skip cost
# estimation for a given (provider, model) pair -- Qmonster will render no
# "COST [Est]" badge for that combination.
#
# This table is ProjectCanonical. Qmonster does NOT fetch provider pricing
# pages. Refresh manually when prices change; the file is gitignored so each
# operator owns their own numbers. Cost estimates are for trend tracking,
# NOT for billing reconciliation.
#
# Last updated by operator: YYYY-MM-DD

[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 0.00    # TODO(operator): fill in from OpenAI pricing
output_per_1m = 0.00

[[entries]]
provider = "claude"
model = "claude-sonnet-4-6"
input_per_1m = 0.00    # TODO(operator): fill in from Anthropic pricing
output_per_1m = 0.00
```

- [ ] **Step 5: Extend .gitignore**

Append to `.gitignore`:

```
config/pricing.toml
```

- [ ] **Step 6: Verify build + clippy**

Run: `cargo fmt --check && cargo clippy --lib -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/policy/pricing.rs src/policy/mod.rs config/pricing.example.toml .gitignore
git commit -m "policy(v1.11.0-1): add pricing table module + example TOML config"
```

---

## Task 2: Add `model_name` field to `SignalSet`

**Files:**

- Modify: `src/domain/signal.rs:52-65`

- [ ] **Step 1: Write the failing test**

Append to `src/domain/signal.rs` `mod tests`:

```rust
    #[test]
    fn default_signal_set_has_no_model_name() {
        let s = SignalSet::default();
        assert!(s.model_name.is_none());
    }

    #[test]
    fn signal_set_can_carry_model_name_with_source_kind() {
        let mut s = SignalSet::default();
        s.model_name = Some(
            MetricValue::new("gpt-5.4".to_string(), SourceKind::ProviderOfficial)
                .with_provider(Provider::Codex),
        );
        let m = s.model_name.as_ref().unwrap();
        assert_eq!(m.value, "gpt-5.4");
        assert_eq!(m.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(m.provider, Some(Provider::Codex));
    }
```

- [ ] **Step 2: Run test — expect compile failure**

Run: `cargo test --lib domain::signal::tests::default_signal_set_has_no_model_name`
Expected: compile error `no field model_name on SignalSet`.

- [ ] **Step 3: Add the field**

Edit `src/domain/signal.rs` — in `SignalSet`, append after `cost_usd`:

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
}
```

Note: `Default` derive + `Option<_>` → new field defaults to `None` automatically. All 51 existing `SignalSet { ..Default::default() }` sites are unaffected.

Also add `Provider` import to the test if missing:

```rust
    use crate::domain::identity::Provider;
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib domain::signal::tests`
Expected: all pass.

- [ ] **Step 5: Run full lib tests to confirm no regressions**

Run: `cargo test --lib`
Expected: previous count + 2 (252 → 254 lib tests).

- [ ] **Step 6: Commit**

```bash
git add src/domain/signal.rs
git commit -m "domain(v1.11.0-2): add model_name field to SignalSet"
```

---

## Task 3: Extend `ProviderParser` trait + adapter signatures (mechanical)

**Files:**

- Modify: `src/adapters/mod.rs`
- Modify: `src/adapters/claude.rs`
- Modify: `src/adapters/codex.rs`
- Modify: `src/adapters/gemini.rs`
- Modify: `src/adapters/qmonster.rs`
- Modify: `src/app/event_loop.rs:94`
- Modify: `src/app/bootstrap.rs` (discover location)

This is a mechanical signature migration. No behavior change yet.

- [ ] **Step 1: Locate `Context` definition**

Run: `rg -n "pub struct Context" src/app/`
Read the file and confirm where `PricingTable` should live (expect `src/app/bootstrap.rs` with other ctx fields).

- [ ] **Step 2: Extend trait + dispatch signature in `src/adapters/mod.rs`**

Replace the file contents with:

```rust
pub mod claude;
pub mod codex;
pub mod common;
pub mod gemini;
pub mod qmonster;

use crate::domain::identity::{Provider, ResolvedIdentity};
use crate::domain::signal::SignalSet;
use crate::policy::pricing::PricingTable;

pub trait ProviderParser {
    fn parse(
        &self,
        identity: &ResolvedIdentity,
        tail: &str,
        pricing: &PricingTable,
    ) -> SignalSet;
}

pub fn parse_for(
    identity: &ResolvedIdentity,
    tail: &str,
    pricing: &PricingTable,
) -> SignalSet {
    match identity.identity.provider {
        Provider::Claude => claude::ClaudeAdapter.parse(identity, tail, pricing),
        Provider::Codex => codex::CodexAdapter.parse(identity, tail, pricing),
        Provider::Gemini => gemini::GeminiAdapter.parse(identity, tail, pricing),
        Provider::Qmonster => qmonster::QmonsterAdapter.parse(identity, tail, pricing),
        Provider::Unknown => common::parse_common_signals(tail),
    }
}

pub use common::parse_common_signals;
```

- [ ] **Step 3: Update `ClaudeAdapter::parse` signature**

Edit `src/adapters/claude.rs` — change the impl to accept `_pricing: &PricingTable`:

```rust
use crate::adapters::ProviderParser;
use crate::adapters::common::parse_common_signals;
use crate::domain::identity::ResolvedIdentity;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, SignalSet};
use crate::policy::pricing::PricingTable;

pub struct ClaudeAdapter;

impl ProviderParser for ClaudeAdapter {
    fn parse(
        &self,
        _identity: &ResolvedIdentity,
        tail: &str,
        _pricing: &PricingTable,
    ) -> SignalSet {
        let mut set = parse_common_signals(tail);
        let lower = tail.to_lowercase();

        if set.context_pressure.is_none()
            && let Some(p) = parse_context_percent_claude(&lower)
        {
            set.context_pressure = Some(MetricValue::new(p / 100.0, SourceKind::Estimated));
        }
        set
    }
}
```

And update the existing tests at the bottom of that file to pass `&PricingTable::empty()` to each `.parse(...)` call:

```rust
    #[test]
    fn claude_adapter_inherits_common_signals() {
        let tail = "Press ENTER to continue";
        let set = ClaudeAdapter.parse(&id(), tail, &PricingTable::empty());
        assert!(set.waiting_for_input);
    }

    #[test]
    fn claude_adapter_parses_claude_specific_percent() {
        let tail = "claude context 88%";
        let set = ClaudeAdapter.parse(&id(), tail, &PricingTable::empty());
        let m = set.context_pressure.expect("parsed");
        assert!((m.value - 0.88).abs() < 0.01);
    }
```

Add `use crate::policy::pricing::PricingTable;` to the test module too.

- [ ] **Step 4: Update `CodexAdapter::parse` signature**

Edit `src/adapters/codex.rs`:

```rust
use crate::adapters::ProviderParser;
use crate::adapters::common::parse_common_signals;
use crate::domain::identity::ResolvedIdentity;
use crate::domain::signal::SignalSet;
use crate::policy::pricing::PricingTable;

pub struct CodexAdapter;

impl ProviderParser for CodexAdapter {
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

Update its test:

```rust
    #[test]
    fn codex_adapter_detects_permission_prompt() {
        let set = CodexAdapter.parse(&id(), "This action requires approval", &PricingTable::empty());
        assert!(set.permission_prompt);
    }
```

Add `use crate::policy::pricing::PricingTable;` in `mod tests`.

- [ ] **Step 5: Update Gemini + Qmonster adapters**

Edit `src/adapters/gemini.rs` similarly — signature + test call.

Edit `src/adapters/qmonster.rs` similarly — signature + test call.

- [ ] **Step 6: Update caller in `src/app/event_loop.rs:94`**

Change line 94 from:

```rust
let signals = crate::adapters::parse_for(&resolved, &pane.tail);
```

To:

```rust
let signals = crate::adapters::parse_for(&resolved, &pane.tail, &ctx.pricing);
```

- [ ] **Step 7: Add `pricing` field to `Context`**

Open `src/app/bootstrap.rs` (or the file discovered in Step 1 holding `struct Context`). Add:

```rust
use crate::policy::pricing::PricingTable;
```

Add to `Context` struct:

```rust
pub pricing: PricingTable,
```

Initialize it wherever `Context` is constructed — use `PricingTable::load_from_toml_or_empty(&root.join("config/pricing.toml"))`. The `root` path variable should already exist in bootstrap for `~/.qmonster/`. If only `QMONSTER_ROOT` data root is available and no project root, initialize `PricingTable::empty()` for now and add a follow-up TODO comment — Slice 2 can resolve config root discovery properly.

- [ ] **Step 8: Run all tests to confirm mechanical migration passed**

Run: `cargo build && cargo test`
Expected: everything compiles + all existing tests still pass (no net test count change yet).

- [ ] **Step 9: Clippy clean**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add src/adapters/mod.rs src/adapters/claude.rs src/adapters/codex.rs src/adapters/gemini.rs src/adapters/qmonster.rs src/app/event_loop.rs src/app/bootstrap.rs
git commit -m "adapters(v1.11.0-3): thread PricingTable through ProviderParser trait"
```

---

## Task 4: `parse_count_with_suffix` helper in `common.rs`

**Files:**

- Modify: `src/adapters/common.rs`

- [ ] **Step 1: Write failing tests**

Append to `mod tests` in `src/adapters/common.rs`:

```rust
    #[test]
    fn parse_count_with_suffix_handles_plain_integer() {
        assert_eq!(parse_count_with_suffix("4300"), Some(4300));
    }

    #[test]
    fn parse_count_with_suffix_handles_k_suffix() {
        assert_eq!(parse_count_with_suffix("4.3k"), Some(4300));
        assert_eq!(parse_count_with_suffix("258K"), Some(258_000));
    }

    #[test]
    fn parse_count_with_suffix_handles_m_suffix() {
        assert_eq!(parse_count_with_suffix("1.53M"), Some(1_530_000));
        assert_eq!(parse_count_with_suffix("20.4K"), Some(20_400));
    }

    #[test]
    fn parse_count_with_suffix_returns_none_for_garbage() {
        assert_eq!(parse_count_with_suffix(""), None);
        assert_eq!(parse_count_with_suffix("xyz"), None);
    }
```

- [ ] **Step 2: Run — expect failure**

Run: `cargo test --lib adapters::common::tests::parse_count_with_suffix`
Expected: compile error `function parse_count_with_suffix not found`.

- [ ] **Step 3: Implement**

Append to `src/adapters/common.rs` (outside `mod tests`):

```rust
/// Parse a token count string like "4.3k", "258K", "1.53M", or "4300"
/// into a u64. Returns `None` if the input is not a recognisable number.
/// Case-insensitive for the suffix.
pub fn parse_count_with_suffix(s: &str) -> Option<u64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (num_part, multiplier) = match trimmed.chars().last()? {
        'k' | 'K' => (&trimmed[..trimmed.len() - 1], 1_000.0_f64),
        'm' | 'M' => (&trimmed[..trimmed.len() - 1], 1_000_000.0_f64),
        _ => (trimmed, 1.0_f64),
    };
    let value: f64 = num_part.parse().ok()?;
    Some((value * multiplier).round() as u64)
}
```

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test --lib adapters::common::tests`
Expected: 4 new tests pass, existing common tests still green.

- [ ] **Step 5: Commit**

```bash
git add src/adapters/common.rs
git commit -m "adapters(v1.11.0-4): parse_count_with_suffix helper for K/M token notation"
```

---

## Task 5: Codex status-line parser + 4-metric extraction + cost

**Files:**

- Modify: `src/adapters/codex.rs`

- [ ] **Step 1: Write the failing tests**

Replace the `mod tests` block in `src/adapters/codex.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};
    use crate::domain::origin::SourceKind;
    use crate::policy::pricing::{PricingRates, PricingTable};

    fn id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Codex,
                instance: 1,
                role: Role::Review,
                pane_id: "%2".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    // Redacted sample taken from ~/.qmonster/archive/2026-04-23/_65/
    // Codex CLI 0.122.0 status bar.
    const STATUS_LINE: &str = "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp";

    fn pricing_with_gpt_5_4() -> PricingTable {
        let mut t = PricingTable::empty();
        // Use a known-in-test rate so arithmetic is deterministic.
        t.insert_for_test(
            Provider::Codex,
            "gpt-5.4".into(),
            PricingRates {
                input_per_1m: 1.00,
                output_per_1m: 10.00,
            },
        );
        t
    }

    #[test]
    fn codex_adapter_detects_permission_prompt() {
        let set = CodexAdapter.parse(&id(), "This action requires approval", &PricingTable::empty());
        assert!(set.permission_prompt);
    }

    #[test]
    fn codex_adapter_extracts_four_metrics_from_status_line_with_pricing() {
        let set = CodexAdapter.parse(&id(), STATUS_LINE, &pricing_with_gpt_5_4());

        let ctx = set.context_pressure.as_ref().expect("context parsed");
        assert!((ctx.value - 0.27).abs() < 0.001);
        assert_eq!(ctx.source_kind, SourceKind::ProviderOfficial);

        let tokens = set.token_count.as_ref().expect("tokens parsed");
        assert_eq!(tokens.value, 1_530_000);
        assert_eq!(tokens.source_kind, SourceKind::ProviderOfficial);

        let model = set.model_name.as_ref().expect("model parsed");
        assert_eq!(model.value, "gpt-5.4");
        assert_eq!(model.source_kind, SourceKind::ProviderOfficial);

        let cost = set.cost_usd.as_ref().expect("cost computed");
        // 1.51M in × $1.00 / 1M + 20.4K out × $10.00 / 1M
        //   = 1.51 + 0.204 = 1.714
        assert!((cost.value - 1.714).abs() < 0.01, "got {}", cost.value);
        assert_eq!(cost.source_kind, SourceKind::Estimated);
    }

    #[test]
    fn codex_adapter_leaves_cost_none_when_pricing_table_empty() {
        let set = CodexAdapter.parse(&id(), STATUS_LINE, &PricingTable::empty());
        assert!(set.context_pressure.is_some());
        assert!(set.token_count.is_some());
        assert!(set.model_name.is_some());
        assert!(set.cost_usd.is_none());
    }

    #[test]
    fn codex_adapter_falls_back_to_common_when_status_line_absent() {
        let tail = "Press ENTER to continue\nno status bar here";
        let set = CodexAdapter.parse(&id(), tail, &PricingTable::empty());
        assert!(set.waiting_for_input);
        assert!(set.context_pressure.is_none());
        assert!(set.token_count.is_none());
        assert!(set.model_name.is_none());
    }
}
```

Also add a test-only helper to `PricingTable`. Edit `src/policy/pricing.rs`, add inside `impl PricingTable` as a plain `pub fn` (no `cfg(test)` gate, so Task 8's integration test can also use it from the external test crate):

```rust
    /// Test-only helper: insert a pricing entry directly without going
    /// through TOML. Do NOT call from production code paths — production
    /// must go through `load_from_toml_or_empty` so operator-curated
    /// values are the source of truth.
    pub fn insert_for_test(&mut self, provider: Provider, model: String, rates: PricingRates) {
        self.entries.insert((provider, model), rates);
    }
```

- [ ] **Step 2: Run tests — expect failures**

Run: `cargo test --lib adapters::codex`
Expected: compile error or test failures because status line parser doesn't exist yet.

- [ ] **Step 3: Implement**

Replace the body of `src/adapters/codex.rs` (keeping tests) with:

```rust
use crate::adapters::ProviderParser;
use crate::adapters::common::{parse_common_signals, parse_count_with_suffix};
use crate::domain::identity::{Provider, ResolvedIdentity};
use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, SignalSet};
use crate::policy::pricing::PricingTable;

pub struct CodexAdapter;

struct CodexStatus {
    context_pct: u8,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    model: String,
}

impl ProviderParser for CodexAdapter {
    fn parse(
        &self,
        _identity: &ResolvedIdentity,
        tail: &str,
        pricing: &PricingTable,
    ) -> SignalSet {
        let mut set = parse_common_signals(tail);
        let Some(status) = parse_codex_status_line(tail) else {
            return set;
        };

        set.context_pressure = Some(
            MetricValue::new(
                status.context_pct as f32 / 100.0,
                SourceKind::ProviderOfficial,
            )
            .with_confidence(0.95)
            .with_provider(Provider::Codex),
        );
        set.token_count = Some(
            MetricValue::new(status.total_tokens, SourceKind::ProviderOfficial)
                .with_confidence(0.95)
                .with_provider(Provider::Codex),
        );
        set.model_name = Some(
            MetricValue::new(status.model.clone(), SourceKind::ProviderOfficial)
                .with_confidence(0.95)
                .with_provider(Provider::Codex),
        );
        set.cost_usd = pricing
            .lookup(Provider::Codex, &status.model)
            .map(|rates| {
                let cost = (status.input_tokens as f64 * rates.input_per_1m
                    + status.output_tokens as f64 * rates.output_per_1m)
                    / 1_000_000.0;
                MetricValue::new(cost, SourceKind::Estimated)
                    .with_confidence(0.7)
                    .with_provider(Provider::Codex)
            });

        set
    }
}

fn parse_codex_status_line(tail: &str) -> Option<CodexStatus> {
    // bottom-up — prefer the most recent frame's status bar over the
    // /status command's bordered box output (which goes stale).
    for line in tail.lines().rev() {
        if !(line.contains("Context") && line.contains("% used") && line.contains(" · ")) {
            continue;
        }
        let tokens: Vec<&str> = line.split(" · ").map(str::trim).collect();

        let mut context_pct: Option<u8> = None;
        let mut total_tokens: Option<u64> = None;
        let mut input_tokens: Option<u64> = None;
        let mut output_tokens: Option<u64> = None;
        let mut model: Option<String> = None;

        for tok in &tokens {
            // "Context 27% used"
            if let Some(rest) = tok.strip_prefix("Context ")
                && let Some(pct_str) = rest.strip_suffix("% used")
                && let Ok(pct) = pct_str.parse::<u8>()
            {
                context_pct = Some(pct);
                continue;
            }
            // "1.53M used"
            if let Some(num) = tok.strip_suffix(" used")
                && let Some(n) = parse_count_with_suffix(num)
                && !tok.contains("% used")
            {
                total_tokens = Some(n);
                continue;
            }
            // "1.51M in"
            if let Some(num) = tok.strip_suffix(" in")
                && let Some(n) = parse_count_with_suffix(num)
            {
                input_tokens = Some(n);
                continue;
            }
            // "20.4K out"
            if let Some(num) = tok.strip_suffix(" out")
                && let Some(n) = parse_count_with_suffix(num)
            {
                output_tokens = Some(n);
                continue;
            }
            // Model name: known provider prefixes
            if model.is_none()
                && (tok.starts_with("gpt-")
                    || tok.starts_with("claude-")
                    || tok.starts_with("gemini-"))
            {
                model = Some((*tok).to_string());
                continue;
            }
        }

        // Require all four to consider the line a valid status bar.
        if let (Some(c), Some(tot), Some(i), Some(o), Some(m)) =
            (context_pct, total_tokens, input_tokens, output_tokens, model)
        {
            return Some(CodexStatus {
                context_pct: c,
                total_tokens: tot,
                input_tokens: i,
                output_tokens: o,
                model: m,
            });
        }
    }
    None
}
```

Note on the `%` edge case: the "1.53M used" vs "Context 27% used" ambiguity is resolved by checking `context_pct` pattern FIRST (with prefix `Context `) and guarding `total_tokens` with `!tok.contains("% used")`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib adapters::codex`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/adapters/codex.rs src/policy/pricing.rs
git commit -m "adapters(v1.11.0-5): codex status line parser with 4-metric extraction and cost computation"
```

---

## Task 6: Claude working-line output token parser

**Files:**

- Modify: `src/adapters/claude.rs`

- [ ] **Step 1: Write failing tests**

Replace `mod tests` in `src/adapters/claude.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};
    use crate::domain::origin::SourceKind;
    use crate::policy::pricing::PricingTable;

    fn id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    #[test]
    fn claude_adapter_inherits_common_signals() {
        let tail = "Press ENTER to continue";
        let set = ClaudeAdapter.parse(&id(), tail, &PricingTable::empty());
        assert!(set.waiting_for_input);
    }

    #[test]
    fn claude_adapter_parses_claude_specific_percent() {
        let tail = "claude context 88%";
        let set = ClaudeAdapter.parse(&id(), tail, &PricingTable::empty());
        let m = set.context_pressure.expect("parsed");
        assert!((m.value - 0.88).abs() < 0.01);
    }

    #[test]
    fn claude_adapter_extracts_output_tokens_from_working_line() {
        let tail = "✶ Exploring adapter parsing surface… (1m 34s · ↓ 4.3k tokens · thought for 11s)";
        let set = ClaudeAdapter.parse(&id(), tail, &PricingTable::empty());
        let m = set.token_count.expect("output tokens parsed");
        assert_eq!(m.value, 4_300);
        assert_eq!(m.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(m.provider, Some(Provider::Claude));
    }

    #[test]
    fn claude_adapter_prefers_subagent_done_line_over_working_line() {
        let tail = "\
✽ Exploring… (2m · ↓ 8.6k tokens)
  ⎿  Done (27 tool uses · 95.1k tokens · 1m 21s)";
        let set = ClaudeAdapter.parse(&id(), tail, &PricingTable::empty());
        let m = set.token_count.expect("tokens parsed");
        assert_eq!(m.value, 95_100);
    }

    #[test]
    fn claude_adapter_returns_none_token_count_when_no_marker() {
        let tail = "regular claude output with no token marker";
        let set = ClaudeAdapter.parse(&id(), tail, &PricingTable::empty());
        assert!(set.token_count.is_none());
    }

    #[test]
    fn claude_adapter_never_populates_model_name_or_cost_in_slice_1() {
        let tail = "✶ Working… (↓ 100 tokens)";
        let set = ClaudeAdapter.parse(&id(), tail, &PricingTable::empty());
        assert!(set.model_name.is_none(), "Claude model is not parseable in Slice 1");
        assert!(set.cost_usd.is_none(), "Claude cost requires input tokens which Claude tail does not expose");
    }
}
```

- [ ] **Step 2: Run — expect failures**

Run: `cargo test --lib adapters::claude`
Expected: multiple test failures (token_count not populated).

- [ ] **Step 3: Implement**

Replace body of `src/adapters/claude.rs` (keep tests):

```rust
use crate::adapters::ProviderParser;
use crate::adapters::common::{parse_common_signals, parse_count_with_suffix};
use crate::domain::identity::{Provider, ResolvedIdentity};
use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, SignalSet};
use crate::policy::pricing::PricingTable;

pub struct ClaudeAdapter;

impl ProviderParser for ClaudeAdapter {
    fn parse(
        &self,
        _identity: &ResolvedIdentity,
        tail: &str,
        _pricing: &PricingTable,
    ) -> SignalSet {
        let mut set = parse_common_signals(tail);
        let lower = tail.to_lowercase();

        if set.context_pressure.is_none()
            && let Some(p) = parse_context_percent_claude(&lower)
        {
            set.context_pressure = Some(MetricValue::new(p / 100.0, SourceKind::Estimated));
        }

        if let Some(n) = parse_claude_output_tokens(tail) {
            set.token_count = Some(
                MetricValue::new(n, SourceKind::ProviderOfficial)
                    .with_confidence(0.85)
                    .with_provider(Provider::Claude),
            );
        }

        set
    }
}

fn parse_context_percent_claude(lower: &str) -> Option<f32> {
    for line in lower.lines() {
        if line.contains("claude") && line.contains('%') {
            let mut digits = String::new();
            let mut seen_dot = false;
            for ch in line.chars() {
                if ch.is_ascii_digit() {
                    digits.push(ch);
                } else if ch == '.' && !seen_dot {
                    digits.push(ch);
                    seen_dot = true;
                } else if ch == '%' {
                    if let Ok(v) = digits.parse::<f32>() {
                        return Some(v);
                    }
                    digits.clear();
                    seen_dot = false;
                } else {
                    digits.clear();
                    seen_dot = false;
                }
            }
        }
    }
    None
}

fn parse_claude_output_tokens(tail: &str) -> Option<u64> {
    // Priority 1: `Done (… · N[kM] tokens · …)` — subagent finished, cumulative.
    for line in tail.lines().rev() {
        if let Some(n) = extract_done_tokens(line) {
            return Some(n);
        }
    }
    // Priority 2: `↓ N[kM] tokens` — live working line.
    for line in tail.lines().rev() {
        if let Some(n) = extract_arrow_tokens(line) {
            return Some(n);
        }
    }
    None
}

fn extract_done_tokens(line: &str) -> Option<u64> {
    // match substring: "· <count> tokens" where the line also contains "Done ("
    if !line.contains("Done (") {
        return None;
    }
    extract_tokens_after_middot(line)
}

fn extract_arrow_tokens(line: &str) -> Option<u64> {
    // match "↓ <count> tokens"
    let idx = line.find('↓')?;
    let rest = &line[idx + '↓'.len_utf8()..];
    extract_tokens_substring(rest)
}

fn extract_tokens_after_middot(line: &str) -> Option<u64> {
    // Look for " · <count> tokens"
    for segment in line.split(" · ") {
        if let Some(n) = extract_tokens_substring(segment) {
            return Some(n);
        }
    }
    None
}

fn extract_tokens_substring(s: &str) -> Option<u64> {
    // Split on whitespace, find a "[number][suffix?]" immediately before "tokens"
    let words: Vec<&str> = s.split_whitespace().collect();
    for w in words.windows(2) {
        if w[1] == "tokens" {
            if let Some(n) = parse_count_with_suffix(w[0]) {
                return Some(n);
            }
        }
    }
    None
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib adapters::claude`
Expected: all 6 pass.

- [ ] **Step 5: Commit**

```bash
git add src/adapters/claude.rs
git commit -m "adapters(v1.11.0-6): claude working-line output token parser"
```

---

## Task 7: UI — model badge + count-suffix formatter

**Files:**

- Modify: `src/ui/labels.rs`
- Modify: `src/ui/panels.rs:341-365, 425-466`

- [ ] **Step 1: Write failing tests in labels.rs**

Append to `src/ui/labels.rs` (add `#[cfg(test)] mod tests { ... }` if not present):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_count_with_suffix_handles_plain_integers() {
        assert_eq!(format_count_with_suffix(0), "0");
        assert_eq!(format_count_with_suffix(999), "999");
    }

    #[test]
    fn format_count_with_suffix_handles_k_boundary() {
        assert_eq!(format_count_with_suffix(1_000), "1.0K");
        assert_eq!(format_count_with_suffix(4_300), "4.3K");
        assert_eq!(format_count_with_suffix(999_999), "1000.0K");
    }

    #[test]
    fn format_count_with_suffix_handles_m_boundary() {
        assert_eq!(format_count_with_suffix(1_000_000), "1.00M");
        assert_eq!(format_count_with_suffix(1_530_000), "1.53M");
    }
}
```

- [ ] **Step 2: Implement the formatter**

Append to `src/ui/labels.rs`:

```rust
/// Human-friendly token count: `999 -> "999"`, `4_300 -> "4.3K"`,
/// `1_530_000 -> "1.53M"`. Used by metric rows so raw u64 token counts
/// do not dominate the UI.
pub fn format_count_with_suffix(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
```

- [ ] **Step 3: Run**

Run: `cargo test --lib ui::labels::tests`
Expected: 3 tests pass.

- [ ] **Step 4: Write failing UI-panel tests**

Run: `rg -n "mod tests" src/ui/panels.rs | head -5` to find the test block.

Append inside that `mod tests`:

```rust
    #[test]
    fn metric_row_renders_model_name_line_when_populated() {
        let mut s = crate::domain::signal::SignalSet::default();
        s.model_name = Some(
            crate::domain::signal::MetricValue::new(
                "gpt-5.4".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            ),
        );
        let row = metric_row(&s);
        assert!(row.contains("model gpt-5.4"));
        assert!(row.contains("Official"));
    }

    #[test]
    fn metric_row_uses_count_suffix_for_tokens() {
        let mut s = crate::domain::signal::SignalSet::default();
        s.token_count = Some(
            crate::domain::signal::MetricValue::new(
                1_530_000,
                crate::domain::origin::SourceKind::ProviderOfficial,
            ),
        );
        let row = metric_row(&s);
        assert!(row.contains("tokens 1.53M"), "got: {row}");
    }
```

- [ ] **Step 5: Run — expect failures**

Run: `cargo test --lib ui::panels`
Expected: new tests fail (no model_name rendering; tokens show raw integer).

- [ ] **Step 6: Update `metric_row`**

Edit `src/ui/panels.rs:341-365` — replace `metric_row`:

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
    parts.join("  ")
}
```

Add `format_count_with_suffix` to the existing `use` block in `src/ui/panels.rs`:

```rust
use crate::ui::labels::{format_count_with_suffix, source_kind_label};
```

(Merge with whatever `source_kind_label` line already imports from `labels`.)

- [ ] **Step 7: Update `metric_badge_line`**

Edit `src/ui/panels.rs:425-466` — after the existing `cost_usd` `if let Some(metric)` branch, append the model branch and swap the tokens branch to use the formatter:

```rust
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
    // ... (cost_usd branch unchanged)
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
```

(Replace the existing `tokens` span with the suffix-formatted version; keep the same order: CTX → TOKENS → COST → MODEL.)

- [ ] **Step 8: Run**

Run: `cargo test --lib ui::panels`
Expected: all pass including the 2 new ones.

- [ ] **Step 9: Commit**

```bash
git add src/ui/labels.rs src/ui/panels.rs
git commit -m "ui(v1.11.0-7): model badge + count-suffix formatter for token rows"
```

---

## Task 8: Integration test — end-to-end Codex status line

**Files:**

- Modify: `tests/event_loop_integration.rs`

- [ ] **Step 1: Locate a representative harness**

Run: `rg -n "run_once|parse_for" tests/event_loop_integration.rs | head -10`

Identify the existing pattern for constructing a pane with a Codex tail and calling through `parse_for` or `run_once`.

- [ ] **Step 2: Add the integration test**

Append to `tests/event_loop_integration.rs`:

```rust
#[test]
fn codex_status_line_end_to_end_populates_four_metrics() {
    use qmonster::adapters::parse_for;
    use qmonster::domain::identity::{IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role};
    use qmonster::domain::origin::SourceKind;
    use qmonster::policy::pricing::{PricingRates, PricingTable};

    let identity = ResolvedIdentity {
        identity: PaneIdentity {
            provider: Provider::Codex,
            instance: 1,
            role: Role::Review,
            pane_id: "%9".into(),
        },
        confidence: IdentityConfidence::High,
    };
    let tail = "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp";

    let mut pricing = PricingTable::empty();
    pricing.insert_for_test(
        Provider::Codex,
        "gpt-5.4".into(),
        PricingRates {
            input_per_1m: 1.00,
            output_per_1m: 10.00,
        },
    );

    let signals = parse_for(&identity, tail, &pricing);

    assert_eq!(
        signals
            .context_pressure
            .as_ref()
            .unwrap()
            .source_kind,
        SourceKind::ProviderOfficial
    );
    assert_eq!(signals.token_count.as_ref().unwrap().value, 1_530_000);
    assert_eq!(signals.model_name.as_ref().unwrap().value, "gpt-5.4");
    let cost = signals.cost_usd.as_ref().unwrap();
    assert!((cost.value - 1.714).abs() < 0.01);
    assert_eq!(cost.source_kind, SourceKind::Estimated);
}

#[test]
fn codex_status_line_end_to_end_without_pricing_populates_three_metrics() {
    use qmonster::adapters::parse_for;
    use qmonster::domain::identity::{IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role};
    use qmonster::policy::pricing::PricingTable;

    let identity = ResolvedIdentity {
        identity: PaneIdentity {
            provider: Provider::Codex,
            instance: 1,
            role: Role::Review,
            pane_id: "%9".into(),
        },
        confidence: IdentityConfidence::High,
    };
    let tail = "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp";

    let signals = parse_for(&identity, tail, &PricingTable::empty());

    assert!(signals.context_pressure.is_some());
    assert!(signals.token_count.is_some());
    assert!(signals.model_name.is_some());
    assert!(signals.cost_usd.is_none());
}
```

`PricingTable::insert_for_test` is already a plain `pub fn` (Task 5 Step 1 enforces this) — no `cfg` gate, so integration tests in `tests/` can import and call it directly. No Cargo feature flags needed.

- [ ] **Step 3: Run integration test**

Run: `cargo test --test event_loop_integration`
Expected: all existing (22) + 2 new = 24 pass.

- [ ] **Step 4: Commit**

```bash
git add tests/event_loop_integration.rs src/policy/pricing.rs
git commit -m "test(v1.11.0-8): end-to-end codex status line populates four metrics"
```

---

## Task 9: Verification, state, tag

**Files:**

- Modify: `.mission/CURRENT_STATE.md` (gitignored; local only)
- Git: annotated tag `v1.11.0`

- [ ] **Step 1: Full verification**

Run each in sequence:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
cargo test --test event_loop_integration
cargo test
```

Expected totals:

- Lib: ~267 pass (252 prior + 15 new)
- Drift (main.rs): 6 unchanged
- Integration: 24 pass (22 + 2 new)
- Total ≈ 297 green (+17 net)

- [ ] **Step 2: Update CURRENT_STATE.md**

Replace the first 11 lines of `.mission/CURRENT_STATE.md` with (actual values as observed in verification):

```markdown
# CURRENT_STATE

_Last updated: 2026-04-23 (claude:1:main, after usage-hint parsing v1.11.0)_

## Mission

- Title: Qmonster v0.4.0 — usage-hint parsing v1.11.0 (Codex status-line parser populates context %, token_count, model_name, cost_usd [via operator-supplied PricingTable]; Claude working-line parser populates output token_count; Claude model/cost and Gemini honest None; SignalSet gains model_name field; UI renders MODEL badge + K/M suffix on token counts)
- Version: 1.11.0
- Branch / worktree: `main` + annotated tags `v1.10.7`, `v1.10.8`, `v1.10.9`, `v1.11.0`
- Phase: **Usage-hint parsing Slice 1 shipped** — closes the honest-gap audit finding that v1.10.9's TUI showed blanks for `누가 얼마나 쓰는지`. New `src/policy/pricing.rs` module loads `config/pricing.toml` (operator-curated, gitignored). `ProviderParser::parse` now takes `&PricingTable` as 3rd arg. Codex adapter parses its single-line status bar (`Context N% used`, `<total>M used`, `<input>M in`, `<output>K out`, `gpt-X.Y` model) and computes `cost_usd` via pricing lookup (SourceKind::Estimated, confidence 0.7). Claude adapter parses `↓ Nk tokens` working line (or `Done (… · Nk tokens)` subagent completion line when present, preferred as more accurate). Claude `model_name` and `cost_usd` explicitly remain `None` — tail does not expose these honestly in Slice 1. Gemini unchanged (Slice 3). 267 lib + 6 drift + 24 integration = ~297 tests green (+17 net); clippy + fmt clean. v1.10.x UX polish arc closed. Layered on v1.10.9 (`337f80c`); commits are the 7 `v1.11.0-1` .. `-7` + `-8` sequence plus this tag commit — obtain via `git log --oneline v1.10.9..v1.11.0` and inline the short SHAs here at update time.
```

- [ ] **Step 3: Create annotated tag**

```bash
git tag -a v1.11.0 -m "v1.11.0 — provider usage-hint parsing Slice 1

First slice of the P0-1 observability gap closure. Codex status-line
parser populates context %, tokens, model, and cost (via operator-supplied
pricing table, Estimated label). Claude working-line parser populates
output token count. Claude model/cost and Gemini remain honestly None
per deferred slices.

Schema: SignalSet gains model_name: Option<MetricValue<String>>.
Trait: ProviderParser::parse gains &PricingTable 3rd arg.
UI: MODEL badge in metric_badge_line, K/M suffix in token display.
+17 tests, ~297 green total."
```

- [ ] **Step 4: Verify describe**

Run: `git describe --tags --always --dirty`
Expected: `v1.11.0`

- [ ] **Step 5: Final summary commit (if CURRENT_STATE were tracked, but it's gitignored so no commit)**

Skip — state file is gitignored per project convention.

---

## Out-of-plan follow-ups

- **Codex/Gemini reviewer gate** — this slice changes observation surface + introduces `Estimated` cost. The project's confirm-archive pattern applies. After verification, request Codex + Gemini reviews targeting `v1.11.0` per `docs/ai/REVIEW_GUIDE.md`. Not a step in this plan because it is an external-agent action.
- **Slice 2 preview** — SignalSet `git_branch` / `reasoning_effort` / `worktree_path` fields; Claude `model_name` via `settings.json` or session-start banner (if reliable); Gemini status line investigation. Separate spec + plan when reached.
- **Slice 3 preview** — Gemini CLI status surface. Requires real archive samples first.
