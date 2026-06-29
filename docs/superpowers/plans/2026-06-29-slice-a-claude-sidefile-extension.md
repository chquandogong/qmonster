# Slice A — Claude sidefile structured-preferred extension — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Source Claude panes' context-pressure and 5h/weekly quota-pressure from the structured sidefile JSON Qmonster already reads (preferring it over the scraped statusline), and fill model/effort from the sidefile when the scrape didn't.

**Architecture:** Qmonster enriches a scraped `SignalSet` with the Claude statusline sidefile JSON in `apply_claude_sidefile()` (`src/adapters/mod.rs`). Today that fill uses `is_none()` guards and only touches token counts / cost / resets / identity. This slice (1) extends the `ClaudeSidefile` serde structs to deserialize `used_percentage` (context + rate windows) plus `model`/`effort`, and (2) makes the sidefile's `used_percentage` **override** the scraped pressure values (same provider-official number, less fragile than statusline parsing), while model/effort stay fill-when-absent to avoid display churn.

**Tech Stack:** Rust, `serde`/`serde_json`, the project's `MetricValue<T>` + `SourceKind` domain types. No new dependencies.

## Global Constraints

- Provider precedence (this slice): sidefile **overrides** scraped `context_pressure` / `quota_5h_pressure` / `quota_weekly_pressure`; sidefile **fills-when-absent** `model_name` / `reasoning_effort`. Mirrors the existing `cache_hit_ratio` override precedent in the same function.
- Every sidefile-derived value keeps `SourceKind::ProviderOfficial`, built via the existing local `metric<T>()` helper (`.with_confidence(0.95).with_provider(Provider::Claude)`).
- `permission_mode` is NOT in the sidefile → stays scrape-only (do not touch).
- No new `SignalSet` field in this slice. `context_window_size` surfacing is **deferred** (it would require a new `SignalSet` field rippling to ~12 fixture sites — out of scope for Slice A).
- No behavior change when the operator has no sidefile: `read_sidefile_for_path` returns `None` and everything falls back to scrape exactly as today.
- Gates that must stay green before any commit that ends a task: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`, `cargo test --all-targets`.
- ObserveOnly / safety-precedence / actuation policy: unchanged. No new audit kind, no SQLite change, no config key.

---

## File Structure

- `src/adapters/claude_sidefile.rs` — **Modify.** Extend the serde structs (`ClaudeSidefileContextWindow`, `ClaudeSidefileRateWindow`) and add `ClaudeSidefileModel` / `ClaudeSidefileEffort` + the `model` / `effort` / `version` fields on `ClaudeSidefile`. Extend its `mod tests`.
- `src/adapters/mod.rs` — **Modify.** In `apply_claude_sidefile()` (line ~175) add the pressure overrides + model/effort fills. Add unit tests to the existing `mod sidefile_integration_tests` (which can call the private `apply_claude_sidefile` directly via `use super::*`).
- `docs/ai/ARCHITECTURE.md` — **Modify.** Provider-coverage matrix: Claude `context_pressure` / `quota_*` rows note "sidefile JSON preferred, statusline fallback".

---

### Task 1: Extend `ClaudeSidefile` to deserialize `used_percentage`, `model`, `effort`

**Files:**

- Modify: `src/adapters/claude_sidefile.rs` (structs at lines 29-81; test `read_sidefile_parses_full_real_world_shape` at line 240)
- Test: same file, `mod tests`

**Interfaces:**

- Produces: `ClaudeSidefileContextWindow.used_percentage: Option<f64>`, `ClaudeSidefileContextWindow.context_window_size: Option<u64>`, `ClaudeSidefileRateWindow.used_percentage: Option<f64>`, `ClaudeSidefile.model: Option<ClaudeSidefileModel>` (`{ id, display_name }`), `ClaudeSidefile.effort: Option<ClaudeSidefileEffort>` (`{ level }`), `ClaudeSidefile.version: Option<String>`. Task 2 and Task 3 consume these.

- [ ] **Step 1: Write the failing test** — add assertions for the new fields to the existing real-shape test. Append these assertions at the end of `read_sidefile_parses_full_real_world_shape` (just before the closing `}` of the test, after the existing `rl.seven_day` assertion):

```rust
        // Slice A: fields previously dropped by serde must now deserialize.
        let cw = s.context_window.clone().unwrap();
        assert_eq!(cw.used_percentage, Some(5.0));
        assert_eq!(rl.five_hour.clone().unwrap().used_percentage, Some(30.0));
        assert_eq!(rl.seven_day.clone().unwrap().used_percentage, Some(10.0));
        let model = s.model.clone().unwrap();
        assert_eq!(model.id.as_deref(), Some("claude-x"));
        assert_eq!(model.display_name.as_deref(), Some("Claude X"));
```

(Note: the test body's JSON already contains `"used_percentage"`, `"rate_limits": {... "used_percentage" ...}`, and `"model": {"id": "claude-x", "display_name": "Claude X"}` — serde currently drops them. `rl` and `s` are already bound earlier in the test.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib claude_sidefile::tests::read_sidefile_parses_full_real_world_shape`
Expected: **FAILS TO COMPILE** — `no field 'used_percentage' on type 'ClaudeSidefileContextWindow'`, `no field 'model' on ClaudeSidefile`.

- [ ] **Step 3: Extend the structs.** Replace the `ClaudeSidefileContextWindow` struct (lines 51-55) with:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefileContextWindow {
    #[serde(default)]
    pub current_usage: Option<ClaudeSidefileCurrentUsage>,
    #[serde(default)]
    pub used_percentage: Option<f64>,
    #[serde(default)]
    pub context_window_size: Option<u64>,
}
```

Replace the `ClaudeSidefileRateWindow` struct (lines 77-81) with:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefileRateWindow {
    #[serde(default)]
    pub resets_at: Option<u64>,
    #[serde(default)]
    pub used_percentage: Option<f64>,
}
```

Add the `model` / `effort` / `version` fields to `ClaudeSidefile` (inside the struct at lines 29-43, after `pub rate_limits: ...`):

```rust
    #[serde(default)]
    pub model: Option<ClaudeSidefileModel>,
    #[serde(default)]
    pub effort: Option<ClaudeSidefileEffort>,
    #[serde(default)]
    pub version: Option<String>,
```

And add these two new structs immediately after the `ClaudeSidefile` struct definition (after line 43):

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefileModel {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefileEffort {
    #[serde(default)]
    pub level: Option<String>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib claude_sidefile::tests::read_sidefile_parses_full_real_world_shape`
Expected: PASS (`test result: ok. 1 passed`).

- [ ] **Step 5: Commit**

```bash
git add src/adapters/claude_sidefile.rs
git commit -m "$(cat <<'EOF'
feat(claude-sidefile): deserialize used_percentage, model, effort fields

Slice A Task 1: extend ClaudeSidefile structs to read context_window
used_percentage/context_window_size, rate-window used_percentage, and
model/effort/version — fields the live sidefile already carries but the
struct previously dropped.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Sidefile `used_percentage` overrides scraped pressures

**Files:**

- Modify: `src/adapters/mod.rs` — `apply_claude_sidefile()` (the `rate_limits` block at lines 227-238)
- Test: `src/adapters/mod.rs` — `mod sidefile_integration_tests`

**Interfaces:**

- Consumes (from Task 1): `ClaudeSidefileContextWindow.used_percentage`, `ClaudeSidefileRateWindow.used_percentage`.
- Consumes (existing): private `apply_claude_sidefile(signals: &mut SignalSet, sidefile: claude_sidefile::ClaudeSidefile)`; local `metric<T>()` helper; `SignalSet.{context_pressure, quota_5h_pressure, quota_weekly_pressure}: Option<MetricValue<f32>>`.

- [ ] **Step 1: Write the failing test** — add to `mod sidefile_integration_tests` (after the last existing test, before the module's closing `}`):

```rust
    #[test]
    fn sidefile_used_percentage_overrides_scraped_pressure() {
        // Pre-seed as if the scrape produced stale/rounded values.
        let mut signals = crate::adapters::common::parse_common_signals("");
        signals.context_pressure = Some(
            crate::domain::signal::MetricValue::new(
                0.50_f32,
                crate::domain::origin::SourceKind::ProviderOfficial,
            ),
        );
        signals.quota_5h_pressure = Some(
            crate::domain::signal::MetricValue::new(
                0.50_f32,
                crate::domain::origin::SourceKind::ProviderOfficial,
            ),
        );
        let sidefile: claude_sidefile::ClaudeSidefile = serde_json::from_str(
            r#"{
                "context_window": {"used_percentage": 21},
                "rate_limits": {
                    "five_hour": {"used_percentage": 9},
                    "seven_day": {"used_percentage": 20}
                }
            }"#,
        )
        .unwrap();

        apply_claude_sidefile(&mut signals, sidefile);

        assert!((signals.context_pressure.unwrap().value - 0.21).abs() < 1e-6);
        assert!((signals.quota_5h_pressure.unwrap().value - 0.09).abs() < 1e-6);
        assert!((signals.quota_weekly_pressure.unwrap().value - 0.20).abs() < 1e-6);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib sidefile_used_percentage_overrides_scraped_pressure`
Expected: FAIL — `context_pressure` stays `0.50` (override not implemented), assertion `0.50 ≈ 0.21` fails.

- [ ] **Step 3: Implement the overrides.** In `apply_claude_sidefile()`, replace the existing `rate_limits` block (lines 227-238) with the version below — it adds the context-pressure override before the block and the two quota-pressure overrides inside it (unconditional assignment, like the `cache_hit_ratio` override above it):

```rust
    // Slice A: prefer the sidefile's structured used_percentage over the
    // scraped statusline pressure — same ProviderOfficial number, but not
    // dependent on statusline text parsing. Override (not is_none) so the
    // structured value wins whenever present.
    if let Some(pct) = sidefile
        .context_window
        .as_ref()
        .and_then(|cw| cw.used_percentage)
    {
        signals.context_pressure = Some(metric((pct / 100.0) as f32));
    }
    if let Some(rl) = sidefile.rate_limits.as_ref() {
        if let Some(pct) = rl.five_hour.as_ref().and_then(|w| w.used_percentage) {
            signals.quota_5h_pressure = Some(metric((pct / 100.0) as f32));
        }
        if let Some(pct) = rl.seven_day.as_ref().and_then(|w| w.used_percentage) {
            signals.quota_weekly_pressure = Some(metric((pct / 100.0) as f32));
        }
        if signals.quota_5h_resets_at.is_none()
            && let Some(ts) = rl.five_hour.as_ref().and_then(|w| w.resets_at)
        {
            signals.quota_5h_resets_at = Some(metric(ts));
        }
        if signals.quota_weekly_resets_at.is_none()
            && let Some(ts) = rl.seven_day.as_ref().and_then(|w| w.resets_at)
        {
            signals.quota_weekly_resets_at = Some(metric(ts));
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib sidefile_used_percentage_overrides_scraped_pressure`
Expected: PASS.
Run also (no regression on the existing resets_at test): `cargo test --lib sidefile_enriches_claude_pane_with_raw_token_counts_cost_and_resets_at`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/adapters/mod.rs
git commit -m "$(cat <<'EOF'
feat(adapters): prefer sidefile used_percentage over scraped pressure

Slice A Task 2: context_pressure + quota_5h/weekly_pressure now come from
the sidefile JSON's structured used_percentage when present, overriding
the scraped statusline value (ProviderOfficial both ways). resets_at
fills unchanged. No change when no sidefile is present.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Fill `model_name` / `reasoning_effort` from sidefile when scrape absent

**Files:**

- Modify: `src/adapters/mod.rs` — `apply_claude_sidefile()` (add after the `transcript_path` runtime-fact block, before the function's closing `}` at line 261)
- Test: `src/adapters/mod.rs` — `mod sidefile_integration_tests`

**Interfaces:**

- Consumes (from Task 1): `ClaudeSidefile.model` (`ClaudeSidefileModel { id, display_name }`), `ClaudeSidefile.effort` (`ClaudeSidefileEffort { level }`).
- Consumes (existing): `SignalSet.model_name: Option<MetricValue<String>>`, `SignalSet.reasoning_effort: Option<MetricValue<String>>`, local `metric<T>()`.

- [ ] **Step 1: Write the failing tests** — add to `mod sidefile_integration_tests`:

```rust
    #[test]
    fn sidefile_fills_model_and_effort_when_scrape_absent() {
        let mut signals = crate::adapters::common::parse_common_signals("");
        let sidefile: claude_sidefile::ClaudeSidefile = serde_json::from_str(
            r#"{
                "model": {"id": "claude-opus-4-8[1m]", "display_name": "Opus 4.8 (1M context)"},
                "effort": {"level": "max"}
            }"#,
        )
        .unwrap();

        apply_claude_sidefile(&mut signals, sidefile);

        assert_eq!(
            signals.model_name.as_ref().unwrap().value,
            "Opus 4.8 (1M context)"
        );
        assert_eq!(signals.reasoning_effort.as_ref().unwrap().value, "max");
    }

    #[test]
    fn sidefile_does_not_override_scraped_model() {
        let mut signals = crate::adapters::common::parse_common_signals("");
        signals.model_name = Some(crate::domain::signal::MetricValue::new(
            "scraped-model".to_string(),
            crate::domain::origin::SourceKind::ProviderOfficial,
        ));
        let sidefile: claude_sidefile::ClaudeSidefile =
            serde_json::from_str(r#"{"model":{"id":"x","display_name":"Y"}}"#).unwrap();

        apply_claude_sidefile(&mut signals, sidefile);

        assert_eq!(signals.model_name.as_ref().unwrap().value, "scraped-model");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib sidefile_fills_model_and_effort_when_scrape_absent`
Expected: FAIL — `model_name` is `None` (fill not implemented), `.unwrap()` panics.

- [ ] **Step 3: Implement the fills.** In `apply_claude_sidefile()`, add immediately before the function's closing brace (after the `transcript_path` block ending at line 260):

```rust
    // Slice A: fill model/effort from the sidefile only when the scrape
    // didn't already produce them (is_none) — the statusline is the
    // authoritative display string when present, so we avoid churning it.
    if signals.model_name.is_none()
        && let Some(m) = sidefile.model.as_ref()
        && let Some(name) = m.display_name.clone().or_else(|| m.id.clone())
    {
        signals.model_name = Some(metric(name));
    }
    if signals.reasoning_effort.is_none()
        && let Some(level) = sidefile.effort.as_ref().and_then(|e| e.level.clone())
    {
        signals.reasoning_effort = Some(metric(level));
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib sidefile_fills_model_and_effort`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add src/adapters/mod.rs
git commit -m "$(cat <<'EOF'
feat(adapters): fill model/effort from sidefile when scrape absent

Slice A Task 3: model_name + reasoning_effort fall back to the sidefile's
structured model.display_name/id + effort.level when the scraped
statusline didn't produce them. is_none guard so a live statusline keeps
its authoritative display string.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Doc-matrix note + full validation + release/review handoff

**Files:**

- Modify: `docs/ai/ARCHITECTURE.md` (provider-coverage matrix, the `context_pressure` and `quota_5h_pressure` / `quota_weekly_pressure` Claude cells around lines 721-728)

**Interfaces:** none (docs + validation only).

- [ ] **Step 1: Update the provider-coverage matrix.** In `docs/ai/ARCHITECTURE.md`, change the Claude cell for `context_pressure` from `✅ statusline + USAGE block` to `✅ sidefile JSON (preferred) + statusline fallback`, and for the `quota_5h_pressure` / `quota_weekly_pressure` Claude cells append ` (used_percentage)` so they read `✅ sidefile JSON (used_percentage)`. Add one sentence under the matrix noting: "Since Slice A (2026-06), Claude `context_pressure` / `quota_*_pressure` prefer the sidefile's structured `used_percentage` over the scraped statusline; `permission_mode` remains scrape-only (absent from the sidefile)."

- [ ] **Step 2: Run the full gate suite**

Run: `cargo fmt --all --check`
Expected: clean (no diff). If it reports drift, run `cargo fmt --all` and re-stage.

Run: `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
Expected: `Finished` with no warnings.

Run: `cargo test --all-targets`
Expected: all green; lib test count = prior baseline **+4** (Task 1 added assertions to an existing test = +0 tests; Task 2 = +1; Task 3 = +2; → +3 new test fns). Confirm no integration regressions.

- [ ] **Step 3: Commit the docs**

```bash
git add docs/ai/ARCHITECTURE.md
git commit -m "$(cat <<'EOF'
docs(architecture): Claude pressure/quota prefer sidefile used_percentage

Slice A Task 4: provider-coverage matrix reflects the structured-preferred
sourcing; permission_mode stays scrape-only.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 4: Handoff to the project release/review ritual (operator-driven).** Do NOT self-assign a version or edit the 130 KB mission ledger inside this plan. Hand off to the operator's normal flow:
  - Version bump (next feature minor; **v2.5.0 expected**) in `package.json`, `Cargo.toml`, `README.md`, `VERSION.md`.
  - `mission.yaml` + `mission-history.yaml` + `.mission/CURRENT_STATE.md` ledger sync.
  - Cross-review gate per `docs/ai/WORKFLOWS.md` (Codex + Gemini), with `.docs/<model>/` narratives + `.mission/evals/` mirrors.
  - Tag + `Release and Package Mirror` workflow verification.
    Report to the operator: tasks 1-3 code complete + green, doc-matrix updated, ready for the version-bump/review ritual.

---

## Self-Review (run against the spec §4 + §3)

- **Spec coverage:** §4.1 (struct extension) → Task 1. §4.2 (structured-preferred context%/5h%/7d%) → Task 2. §4.3 (permission_mode stays scraped) → Global Constraints + not touched. §4.4 (graceful no-sidefile fallback) → preserved (read path unchanged) + existing `sidefile_skipped_*` tests still cover it. model/effort (§4 step 2) → Task 3 (as fill-when-absent; see deviation note). §4 fixtures → Task 1 extends the real-shape test. Doc updates (§6 C1 partial) → Task 4.
- **Deliberate deviation from spec §3/§4:** model_name + reasoning_effort are **fill-when-absent**, not override, because the scrape produces the authoritative display string (`status.model`) and the sidefile gives no accuracy gain there — overriding risks display churn (id vs display_name format). The numeric pressure fields — the actual fragile-parse, high-value targets of the user's intent — ARE overridden. `context_window_size` surfacing deferred (new SignalSet field → fixture ripple). **Flag both to the operator at handoff.**
- **Placeholder scan:** none — every code/step is concrete.
- **Type consistency:** `used_percentage: Option<f64>` → `(pct / 100.0) as f32` matches `context_pressure/quota_*: MetricValue<f32>`. `metric<T>()` is generic, used for f32 / String / u64 alike. `apply_claude_sidefile` private fn reachable from the child test module via `use super::*`. `claude_sidefile::ClaudeSidefile`, `serde_json`, `crate::adapters::common::parse_common_signals`, `crate::domain::signal::MetricValue`, `crate::domain::origin::SourceKind` all confirmed in-tree.

## After Slice A

Slices B (Codex rollout tailer + context% calibration) and C (agy activity enricher + doc/version refresh) get their own plans, written once A lands — B's context% calibration depends on comparing the rollout `last_token_usage`/`model_context_window` against the live scraped "Context %", which is best done with A's groundwork merged.
