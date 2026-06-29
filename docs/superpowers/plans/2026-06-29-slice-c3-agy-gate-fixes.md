# Slice C3 Agy Gate Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close all agy ObserveOnly recommendation leaks with a single engine-level guard, fix the footer parser to prefer the bottom-most match, strengthen regression tests, and update provider_setup.rs overlay text.

**Architecture:** Add a `provider_is_observe_only` helper in `src/policy/engine.rs` that gates ALL rec-emitting rule paths for `Provider::Antigravity`, add an engine-level test asserting recs are empty for an enriched agy pane, strengthen the existing advisories test to assert `.is_empty()`, add `context_pressure` to the adapter regression fixture, fix the footer bottom-match bug by overwriting on each iteration, and update the provider_setup.rs status/wiring text.

**Tech Stack:** Rust, cargo fmt, cargo clippy, cargo test

## Global Constraints

- Only `Provider::Antigravity` behavior changes — Claude/Codex/Gemini/Qmonster/Unknown output stays byte-identical
- No other production behavior change
- `cargo fmt --all --check` + `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args` + `cargo test --all-targets` must all pass
- Keep existing per-rule `context_pressure_warning`/`_critical` provider gates (defense-in-depth)
- Branch: `slice-c3-agy-enrichment`

---

### Task 1: Engine-level ObserveOnly guard + authoritative engine test

**Files:**

- Modify: `src/policy/engine.rs` lines 1-130 (production code) and lines 149-450 (tests)

**Interfaces:**

- Produces: `fn provider_is_observe_only(p: Provider) -> bool` — gates ONLY `Provider::Antigravity`
- Produces: test `agy_enriched_pane_recommendations_are_empty` asserting `out.recommendations.is_empty()` for an agy pane with `context_pressure = Some(0.95)`, High confidence, model_name, token_count

- [ ] **Step 1: Read the current engine.rs evaluate function to confirm the exact lines**

Read `/home/chquan/Qmonster/src/policy/engine.rs` lines 1-80. Confirm `evaluate` is at line 17, `eval_alerts` call is line 26, `recs.extend(eval_advisories...)` is line 27, `recs.extend(eval_profiles...)` is line 30.

- [ ] **Step 2: Add the helper and guard in engine.rs**

In `src/policy/engine.rs`, after the `use` imports (line 5 area), add:

```rust
fn provider_is_observe_only(p: crate::domain::identity::Provider) -> bool {
    matches!(p, crate::domain::identity::Provider::Antigravity)
}
```

Then in `Engine::evaluate`, immediately after `let mut recs = eval_alerts(id, signals);` (line 26), add:

```rust
        if provider_is_observe_only(id.identity.provider) {
            return EvalOutput {
                recommendations: vec![],
                effects: {
                    let mut efx = Vec::new();
                    if signals.log_storm {
                        efx.push(RequestedEffect::ArchiveLocal);
                    }
                    efx
                },
            };
        }
```

This preserves `ArchiveLocal` for log storms (non-recommendation effect) while emptying all recs for agy.

- [ ] **Step 3: Write the failing test first (TDD)**

Before building, add this test in the `#[cfg(test)]` mod in `src/policy/engine.rs`:

```rust
    #[test]
    fn agy_enriched_pane_recommendations_are_empty() {
        // ObserveOnly enforcement: Engine::evaluate for an agy/Antigravity pane
        // with a fully enriched SignalSet must return recommendations that are
        // EMPTY. This catches quota_tight_nudge and any current/future rec leak.
        use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, Role};
        use crate::domain::signal::{MetricValue, SignalSet};
        use crate::domain::origin::SourceKind;
        let agy_id = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Antigravity,
                instance: 1,
                role: Role::Main,
                pane_id: "%0".into(),
            },
            confidence: IdentityConfidence::High,
        };
        let signals = SignalSet {
            context_pressure: Some(MetricValue::new(0.95_f32, SourceKind::ProviderOfficial)),
            model_name: Some(
                MetricValue::new(
                    "Gemini 3.5 Flash (High)".to_string(),
                    SourceKind::ProviderOfficial,
                )
                .with_provider(Provider::Antigravity),
            ),
            token_count: Some(
                MetricValue::new(34_567_u64, SourceKind::ProviderOfficial)
                    .with_provider(Provider::Antigravity),
            ),
            ..SignalSet::default()
        };
        let gates = PolicyGates {
            identity_confidence: IdentityConfidence::High,
            ..PolicyGates::default()
        };
        let out = Engine.evaluate(&agy_id, &signals, &gates, None, &[], &[]);
        assert!(
            out.recommendations.is_empty(),
            "ObserveOnly: agy enriched pane must emit zero recommendations; got {:?}",
            out.recommendations
        );
    }
```

- [ ] **Step 4: Run test to verify it FAILS before fix (confirms it's a real guard)**

```bash
cd /home/chquan/Qmonster && cargo test -p qmonster agy_enriched_pane_recommendations_are_empty 2>&1 | tail -20
```

Expected: FAIL (the quota_tight_nudge leaks through). If it passes, quota_tight_nudge was already gated — re-check.

- [ ] **Step 5: Apply the engine guard (Edit the production code)**

Edit `src/policy/engine.rs` to add the `provider_is_observe_only` helper after the imports and the early-return guard inside `evaluate`.

- [ ] **Step 6: Run test to verify it PASSES**

```bash
cd /home/chquan/Qmonster && cargo test -p qmonster agy_enriched_pane_recommendations_are_empty 2>&1 | tail -10
```

Expected: `test policy::engine::tests::agy_enriched_pane_recommendations_are_empty ... ok`

- [ ] **Step 7: Run fmt + clippy to check no regressions**

```bash
cd /home/chquan/Qmonster && cargo fmt --all --check 2>&1 | head -20
cd /home/chquan/Qmonster && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args 2>&1 | tail -20
```

Expected: both PASS with no errors. If fmt fails, run `cargo fmt --all` first.

- [ ] **Step 8: Commit**

```bash
cd /home/chquan/Qmonster && git add src/policy/engine.rs && git commit -m "fix(policy): engine-level ObserveOnly rec guard — empty recs for Antigravity + authoritative engine test"
```

---

### Task 2: Strengthen advisories test to assert .is_empty()

**Files:**

- Modify: `src/policy/rules/advisories.rs` test at ~line 1067 (`context_pressure_advisory_does_not_fire_for_agy`)

**Interfaces:**

- Consumes: `eval_advisories` from the same file
- The existing test asserts no `/compact` and no `context-pressure` action. Strengthen to assert `recs.is_empty()` to catch `quota_tight_nudge` and any future advisor.

- [ ] **Step 1: Read the existing test**

Read `src/policy/rules/advisories.rs` lines 1047–1082. Confirm the test builds an agy identity with `context_pressure = Some(0.9)` and calls `eval_advisories`.

- [ ] **Step 2: Edit the test to add `.is_empty()` assertion**

After the two existing `assert!` blocks (lines 1068–1081), add:

```rust
        assert!(
            recs.is_empty(),
            "ObserveOnly: eval_advisories must return empty for agy (enriched); got {:?}",
            recs
        );
```

Also update the `context_pressure` value to `0.95` (to match the enriched scenario) and add model_name + token_count to the SignalSet to match the C3-enriched state:

```rust
        let s = SignalSet {
            context_pressure: Some(crate::domain::signal::MetricValue::new(
                0.95,
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            model_name: Some(
                crate::domain::signal::MetricValue::new(
                    "Gemini 3.5 Flash (High)".to_string(),
                    crate::domain::origin::SourceKind::ProviderOfficial,
                )
                .with_provider(crate::domain::identity::Provider::Antigravity),
            ),
            token_count: Some(
                crate::domain::signal::MetricValue::new(
                    34_567_u64,
                    crate::domain::origin::SourceKind::ProviderOfficial,
                )
                .with_provider(crate::domain::identity::Provider::Antigravity),
            ),
            ..SignalSet::default()
        };
```

- [ ] **Step 3: Run test to verify it passes**

```bash
cd /home/chquan/Qmonster && cargo test -p qmonster context_pressure_advisory_does_not_fire_for_agy 2>&1 | tail -10
```

Expected: `test policy::rules::advisories::tests::context_pressure_advisory_does_not_fire_for_agy ... ok`

- [ ] **Step 4: Commit**

```bash
cd /home/chquan/Qmonster && git add src/policy/rules/advisories.rs && git commit -m "test(policy): strengthen agy advisories test — assert .is_empty() for enriched pane"
```

---

### Task 3: Add context_pressure to the adapter regression fixture

**Files:**

- Modify: `src/adapters/mod.rs` function `agy_enriched_signals()` at ~line 1469

**Interfaces:**

- The fixture documents the C3-enriched shape. Add `context_pressure: Some(MetricValue::new(0.95, ProviderOfficial))` so the fixture actually represents context_pressure enrichment.

- [ ] **Step 1: Read the existing helper**

Read `src/adapters/mod.rs` lines 1467–1484. Confirm `agy_enriched_signals` only sets `model_name` and `token_count`.

- [ ] **Step 2: Add context_pressure to the fixture**

Edit the `agy_enriched_signals` function to add `context_pressure`:

```rust
    fn agy_enriched_signals() -> SignalSet {
        SignalSet {
            model_name: Some(
                MetricValue::new(
                    "Gemini 3.5 Flash (High)".to_string(),
                    SourceKind::ProviderOfficial,
                )
                .with_provider(Provider::Antigravity),
            ),
            token_count: Some(
                MetricValue::new(34_567_u64, SourceKind::ProviderOfficial)
                    .with_provider(Provider::Antigravity),
            ),
            context_pressure: Some(MetricValue::new(0.95_f32, SourceKind::ProviderOfficial)),
            ..SignalSet::default()
        }
    }
```

- [ ] **Step 3: Run all adapter tests to verify no regressions**

```bash
cd /home/chquan/Qmonster && cargo test -p qmonster agy_observeonly_regression 2>&1 | tail -20
```

Expected: all 6 gate tests pass.

- [ ] **Step 4: Commit**

```bash
cd /home/chquan/Qmonster && git add src/adapters/mod.rs && git commit -m "test(adapters): add context_pressure to agy_enriched_signals fixture — documents C3-enriched shape"
```

---

### Task 4: Fix agy_footer.rs — prefer bottom-most match for all fields

**Files:**

- Modify: `src/adapters/agy_footer.rs` function `parse_agy_footer` lines 25–47

**Interfaces:**

- Change the loop to OVERWRITE (not skip) on each subsequent match — drop the `is_none()` guards
- Add regression test: tail with early prose `> migrate to Gemini 3.1 Pro (High)` AND real footer `? for shortcuts    Gemini 3.5 Flash (High)` → returns `Gemini 3.5 Flash (High)`

- [ ] **Step 1: Write the failing regression test first**

Add this test to `src/adapters/agy_footer.rs` tests:

```rust
    #[test]
    fn prefers_bottom_most_model_over_prose_mention() {
        // IMPORTANT 3: the agy footer is at the BOTTOM of the captured tail.
        // Earlier conversation prose like "migrate to Gemini 3.1 Pro (High)"
        // must NOT win over the real footer line at the bottom.
        let tail = "> migrate to Gemini 3.1 Pro (High)\n\
                    ? for shortcuts                         Gemini 3.5 Flash (High)\n";
        let f = parse_agy_footer(tail);
        assert_eq!(
            f.model.as_deref(),
            Some("Gemini 3.5 Flash (High)"),
            "must return the bottom-most footer model, not prose mention"
        );
    }
```

- [ ] **Step 2: Run the test to verify it FAILS (confirms the bug)**

```bash
cd /home/chquan/Qmonster && cargo test -p qmonster prefers_bottom_most_model_over_prose_mention 2>&1 | tail -10
```

Expected: FAIL — returns `Gemini 3.1 Pro (High)` (first match wins due to `is_none()` guard).

- [ ] **Step 3: Fix parse_agy_footer to overwrite on each match**

Edit `src/adapters/agy_footer.rs` function `parse_agy_footer` to remove the `is_none()` guards and always overwrite:

```rust
pub fn parse_agy_footer(tail: &str) -> AgyFooter {
    let mut out = AgyFooter::default();
    for raw in tail.lines() {
        let line = raw.trim();
        // model: prefer the BOTTOM-most occurrence — overwrite with each later match.
        if let Some(m) = extract_model(line) {
            out.model = Some(m);
        }
        if let Some(p) = extract_labeled_pct(line, "context-used") {
            out.context_used_pct = Some(p);
        }
        if let Some(n) = extract_labeled_count(line, "token-count") {
            out.token_count = Some(n);
        }
    }
    out
}
```

- [ ] **Step 4: Run all footer tests to verify regression test now passes and no regressions**

```bash
cd /home/chquan/Qmonster && cargo test -p qmonster agy_footer 2>&1 | tail -15
```

Expected: all pass including `prefers_bottom_most_model_over_prose_mention`.

- [ ] **Step 5: Run clippy to check**

```bash
cd /home/chquan/Qmonster && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args 2>&1 | tail -10
```

- [ ] **Step 6: Commit**

```bash
cd /home/chquan/Qmonster && git add src/adapters/agy_footer.rs && git commit -m "fix(adapters): agy_footer prefers bottom-most match for all fields + regression test"
```

---

### Task 5: Update provider_setup.rs overlay text (MINOR fixes)

**Files:**

- Modify: `src/ui/provider_setup.rs` lines ~607-670

**Interfaces:**

- Change "not auto-detected — no documented status surface" to distinguish "no surface" from "enrichment is opt-in"
- Align snippet label and wiring text to say save as an executable SCRIPT FILE (e.g. `~/.local/share/ai-cli-status/agy-statusline.sh`) and set `statusLine.command` to that file path

- [ ] **Step 1: Read the current status/wiring section**

Read `src/ui/provider_setup.rs` lines 607–671. Note the current "not auto-detected" text and wiring section.

- [ ] **Step 2: Update the Current Status detail_row**

Change:

```rust
            out.push(detail_row(
                "agy CLI",
                "not auto-detected — no documented status surface",
            ));
            out.push(detail_row(
                "Documented surface",
                "agy is the launcher for the Antigravity IDE; no headless API yet",
            ));
```

To:

```rust
            out.push(detail_row(
                "agy CLI",
                "detected via tmux title / pane command — enrichment is opt-in",
            ));
            out.push(detail_row(
                "Enrichment surface",
                "statusLine.command stdin contract (C3); Qmonster writes no agy config",
            ));
```

- [ ] **Step 3: Update the Wiring section for script-file clarity**

Change the wiring section (lines ~659-670):

```rust
            section(&mut out, "Wiring (one-time, not copied by y)");
            out.push(
                "Set the copied script as statusLine.command in ~/.gemini/antigravity-cli/settings.json:"
                    .into(),
            );
            out.push(r#"  "statusLine": {"#.into());
            out.push(r#"    "type": "command","#.into());
            out.push(r#"    "command": "/path/to/agy_sidefile.sh""#.into());
            out.push(r#"  }"#.into());
            out.push(
                "  Save the copied script as an executable file then reference it above.".into(),
            );
```

To:

```rust
            section(&mut out, "Wiring (one-time, not copied by y)");
            out.push(
                "Save the copied script as an executable file, e.g.:".into(),
            );
            out.push(
                "  ~/.local/share/ai-cli-status/agy-statusline.sh  (chmod +x)".into(),
            );
            out.push(
                "Then set statusLine.command to that file path in ~/.gemini/antigravity-cli/settings.json:"
                    .into(),
            );
            out.push(r#"  "statusLine": {"#.into());
            out.push(r#"    "type": "command","#.into());
            out.push(r#"    "command": "/home/YOU/.local/share/ai-cli-status/agy-statusline.sh""#.into());
            out.push(r#"  }"#.into());
```

- [ ] **Step 4: Run provider_setup tests to verify no regressions**

```bash
cd /home/chquan/Qmonster && cargo test -p qmonster provider_setup 2>&1 | tail -20
```

Expected: all pass.

- [ ] **Step 5: Run fmt + clippy**

```bash
cd /home/chquan/Qmonster && cargo fmt --all --check 2>&1 | head -10
cd /home/chquan/Qmonster && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args 2>&1 | tail -10
```

- [ ] **Step 6: Commit**

```bash
cd /home/chquan/Qmonster && git add src/ui/provider_setup.rs && git commit -m "fix(ui): agy provider_setup status text + wiring to script-file path"
```

---

### Task 6: Full gate + final squash commit

**Files:**

- No new file changes; run the gate, write the report, create the final commit.

- [ ] **Step 1: Run the full gate**

```bash
cd /home/chquan/Qmonster && cargo fmt --all --check 2>&1 && echo "FMT OK"
cd /home/chquan/Qmonster && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args 2>&1 | tail -5 && echo "CLIPPY OK"
cd /home/chquan/Qmonster && cargo test --all-targets 2>&1 | tail -20 && echo "TEST OK"
```

Expected: all three pass.

- [ ] **Step 2: Write the codex-fix-report.md**

Write to `/home/chquan/Qmonster/.superpowers/sdd/codex-fix-report.md` with sections:

- CRITICAL: engine guard location (file:line), helper name, what it gates
- IMPORTANT 1: new engine test name, what it catches
- IMPORTANT 2: strengthened advisories test assertion, what it catches
- IMPORTANT 2 (fixture): context_pressure added to agy_enriched_signals
- IMPORTANT 3: footer fix — overwrite vs is_none, regression test name
- MINOR: provider_setup.rs text changes
- Gate output summary

- [ ] **Step 3: Create the squash/merge commit**

```bash
cd /home/chquan/Qmonster && git add .superpowers/sdd/codex-fix-report.md && git commit -m "fix(policy): engine-level ObserveOnly rec guard for agy + footer bottom-match + tests + overlay text"
```
