# Slice C4 — agy quota enrichment — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Map agy's structured quota (active-model-aware) onto the existing `SignalSet` `quota_5h_pressure` / `quota_weekly_pressure` + `quota_5h_resets_at` / `quota_weekly_resets_at`, so agy panes show quota windows at parity with Claude.

**Architecture:** Extend the C3 (v2.7.0) agy assets — the recommended `statusLine.command` block (adds active-model-aware quota jq), the `AgySidefile` reader struct (+4 quota fields), and the agy enrichment branch (+4 `SignalSet` fills). Quota is structured-only (sidefile); no footer path. ObserveOnly is inherited from the C3 `Engine::evaluate` guard (no new gating).

**Tech Stack:** Rust 2024, serde/serde_json; the recommended block uses `jq` (`ascii_downcase`, `fromdateiso8601`). No new dependencies.

## Global Constraints

- **Zero new `SignalSet` fields** — reuse `quota_5h_pressure` / `quota_weekly_pressure` (`f32`), `quota_5h_resets_at` / `quota_weekly_resets_at` (`u64`).
- **ObserveOnly inherited from C3 — NO new gating.** The C3 `provider_is_observe_only` guard in `Engine::evaluate` returns empty recommendations for agy before `eval_advisories` (which contains `quota_pressure_recommendations`). Populating agy quota pressures leaks zero recommendations. C4 only EXTENDS the regression to prove it.
- **No fabrication** — a quota field is `Some` only when the sidefile carried it.
- **Opt-in via the existing `[provider_setup] agy_enrichment` toggle** — no new toggle.
- **SourceKind `ProviderOfficial`**, `.with_confidence(0.9).with_provider(Provider::Antigravity)` — via the existing C3 `metric_f32` / `metric_u64` closures in the enrichment branch.
- **Structured-only** — quota from the sidefile alone; no footer scrape path.
- **Window selection (active-model-aware):** `model.id` (lowercased) starts with `gemini` → `gemini-*`; else → `3p-*`. Done in the block jq.
- **pressure = `1 - remaining_fraction`**, clamped `[0,1]` in Rust.
- Validation: `cargo fmt --all --check` + `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args` + `cargo test --all-targets`.

---

## File Structure

| File                           | Change                                                                                     |
| ------------------------------ | ------------------------------------------------------------------------------------------ |
| `src/adapters/agy_sidefile.rs` | **Modify** — `AgySidefile` gains 4 quota fields + parse test                               |
| `src/adapters/mod.rs`          | **Modify** — agy sidefile-fill block gains 4 `SignalSet` quota fills + enrichment test     |
| `src/ui/provider_setup.rs`     | **Modify** — `AGY_SIDEFILE_BLOCK` jq gains `$fam` selection + 4 quota fields; snippet test |
| `src/policy/engine.rs`         | **Modify** — extend `agy_enriched_pane_recommendations_are_empty` with quota               |
| `docs/ai/ARCHITECTURE.md`      | **Modify** — agy provider-coverage row: + quota                                            |

---

## Task 1: `AgySidefile` quota fields

**Files:** Modify `src/adapters/agy_sidefile.rs` (struct ~line 18; test `parses_full_shape_and_skips_malformed` ~line 182)

**Interfaces:**

- Produces: `AgySidefile.{quota_5h_pressure: Option<f64>, quota_5h_resets_at: Option<u64>, quota_weekly_pressure: Option<f64>, quota_weekly_resets_at: Option<u64>}` (Task 2 consumes these).

- [ ] **Step 1: Extend the parse test.** In `src/adapters/agy_sidefile.rs`, update `parses_full_shape_and_skips_malformed` — add the 4 quota fields to the `"ok"` sidefile JSON and assert they deserialize:

```rust
        write(
            &agy_dir(tmp.path()),
            "ok",
            r#"{"cwd":"/repo","conversation_id":"ok","model":"Gemini 3.1 Pro (High)","context_used_percentage":37.5,"context_window_size":1048576,"token_count":34567,"quota_5h_pressure":0.0,"quota_5h_resets_at":1700000000,"quota_weekly_pressure":0.0032377,"quota_weekly_resets_at":1700600000}"#,
        );
        let s = read_agy_sidefile_for_path(tmp.path(), "/repo").expect("malformed must not block");
        assert_eq!(s.conversation_id.as_deref(), Some("ok"));
        assert_eq!(s.context_used_percentage, Some(37.5));
        assert_eq!(s.quota_5h_pressure, Some(0.0));
        assert_eq!(s.quota_5h_resets_at, Some(1700000000));
        assert_eq!(s.quota_weekly_pressure, Some(0.0032377));
        assert_eq!(s.quota_weekly_resets_at, Some(1700600000));
```

- [ ] **Step 2: Run it to verify it fails.**

Run: `cargo test --lib agy_sidefile::tests::parses_full_shape_and_skips_malformed`
Expected: FAIL — `no field 'quota_5h_pressure' on type 'AgySidefile'`.

- [ ] **Step 3: Add the 4 fields** to the `AgySidefile` struct, after `token_count` (~line 30):

```rust
    #[serde(default)]
    pub quota_5h_pressure: Option<f64>,
    #[serde(default)]
    pub quota_5h_resets_at: Option<u64>,
    #[serde(default)]
    pub quota_weekly_pressure: Option<f64>,
    #[serde(default)]
    pub quota_weekly_resets_at: Option<u64>,
```

- [ ] **Step 4: Run to verify it passes.**

Run: `cargo test --lib agy_sidefile`
Expected: PASS (the existing reader/ambiguity-guard tests + the extended parse test).

- [ ] **Step 5: Commit.**

```bash
git add src/adapters/agy_sidefile.rs
git commit -m "feat(adapters): AgySidefile quota fields (5h/weekly pressure + resets)"
```

---

## Task 2: Fill agy quota onto `SignalSet`

**Files:** Modify `src/adapters/mod.rs` (the agy sidefile-fill block, inside `if let Some(sf) = agy_sidefile::read_agy_sidefile_for_path(...)`, currently ending ~line 298 after the `token_count` fill; enrichment test alongside the existing `agy_enrichment_*` tests)

**Interfaces:**

- Consumes: `AgySidefile.quota_*` (Task 1); the existing `metric_f32` (`|v: f32| MetricValue::new(v, ProviderOfficial).with_confidence(0.9).with_provider(Antigravity)`) and `metric_u64` closures already in this branch.
- Produces: `SignalSet.quota_5h_pressure` / `quota_weekly_pressure` / `quota_5h_resets_at` / `quota_weekly_resets_at` populated for agy.

- [ ] **Step 1: Write the failing test.** Add next to the existing `agy_enrichment_sidefile_overrides_footer` test in `src/adapters/mod.rs`, reusing the same scaffolding (`write_proc` with an `agy` descendant, `write_agy_sidefile`, the `ctx()` builder with an Antigravity identity + `pane_pid` + cwd `/repo`):

```rust
#[test]
fn agy_enrichment_fills_quota_from_sidefile() {
    let tmp = tempdir().unwrap();
    let proc = tmp.path().join("proc");
    write_proc(&proc, 100, "agy", &[]);
    let home = tmp.path().join("home");
    write_agy_sidefile(
        &home,
        "c1",
        r#"{"cwd":"/repo","conversation_id":"c1","quota_5h_pressure":0.25,"quota_5h_resets_at":1700000000,"quota_weekly_pressure":0.5,"quota_weekly_resets_at":1700600000}"#,
    );
    let mut c = ctx(/* Antigravity identity, cwd "/repo", pane_pid 100, any tail */);
    c.agy_enrichment_enabled = true;
    let signals = parse_for_with_environment(&c, &proc, Some(&home));
    assert_eq!(signals.quota_5h_pressure.as_ref().map(|m| m.value), Some(0.25_f32));
    assert_eq!(signals.quota_5h_resets_at.as_ref().map(|m| m.value), Some(1700000000));
    assert_eq!(signals.quota_weekly_pressure.as_ref().map(|m| m.value), Some(0.5_f32));
    assert_eq!(signals.quota_weekly_resets_at.as_ref().map(|m| m.value), Some(1700600000));
    assert_eq!(
        signals.quota_5h_pressure.as_ref().map(|m| m.source_kind),
        Some(crate::domain::origin::SourceKind::ProviderOfficial)
    );
}
```

(Fill the `ctx(...)` args by following the existing `agy_enrichment_sidefile_overrides_footer` test verbatim — same identity/pane_pid/cwd construction.)

- [ ] **Step 2: Run to verify it fails.**

Run: `cargo test --lib agy_enrichment_fills_quota_from_sidefile`
Expected: FAIL — `quota_5h_pressure` is `None`.

- [ ] **Step 3: Add the quota fills.** In `src/adapters/mod.rs`, inside the `if let Some(sf) = agy_sidefile::read_agy_sidefile_for_path(...)` block, after the `token_count` fill (~line 298), add:

```rust
            if let Some(p) = sf.quota_5h_pressure {
                signals.quota_5h_pressure = Some(metric_f32((p as f32).clamp(0.0, 1.0)));
            }
            if let Some(ts) = sf.quota_5h_resets_at {
                signals.quota_5h_resets_at = Some(metric_u64(ts));
            }
            if let Some(p) = sf.quota_weekly_pressure {
                signals.quota_weekly_pressure = Some(metric_f32((p as f32).clamp(0.0, 1.0)));
            }
            if let Some(ts) = sf.quota_weekly_resets_at {
                signals.quota_weekly_resets_at = Some(metric_u64(ts));
            }
```

- [ ] **Step 4: Run the full gate** (enrichment branch is exercised by integration tests):

Run: `cargo test --all-targets agy_enrichment_fills_quota_from_sidefile` then `cargo test --all-targets`
Expected: PASS; whole suite green.

- [ ] **Step 5: Commit.**

```bash
git add src/adapters/mod.rs
git commit -m "feat(adapters): fill agy quota (5h/weekly pressure + resets) from sidefile"
```

---

## Task 3: Extend `AGY_SIDEFILE_BLOCK` jq (active-model-aware quota)

**Files:** Modify `src/ui/provider_setup.rs` (`AGY_SIDEFILE_BLOCK` const ~line 295; the `agy_snippet_*` test)

- [ ] **Step 1: Write the failing snippet test.** Add to the `provider_setup` tests:

```rust
#[test]
fn agy_snippet_carries_quota_jq() {
    let overlay = ProviderSetupOverlay {
        tab: ProviderSetupTab::Antigravity,
        agy_enrichment_enabled: true,
        ..Default::default()
    };
    let (_, text) = snippet_for_tab(&overlay);
    assert!(text.contains("quota_5h_pressure"), "block must emit quota_5h_pressure");
    assert!(text.contains("quota_weekly_resets_at"), "block must emit quota_weekly_resets_at");
    assert!(text.contains("fromdateiso8601"), "resets parsed from ISO");
    assert!(text.contains("startswith(\"gemini\")"), "active-model-aware window selection");
}
```

- [ ] **Step 2: Run to verify it fails.**

Run: `cargo test --lib agy_snippet_carries_quota_jq`
Expected: FAIL — the substrings are absent.

- [ ] **Step 3: Extend the block jq.** In `src/ui/provider_setup.rs`, replace the `jq '{ … }'` object in `AGY_SIDEFILE_BLOCK` (lines 307-314) so it binds `$fam` and emits the 4 quota fields. The new block body (lines 307-314 become):

```bash
  printf '%s' "$input" | jq '
    (.model.id // .model.display_name // "" | ascii_downcase
       | if startswith("gemini") then "gemini" else "3p" end) as $fam
    | {
    conversation_id: (.conversation_id // .session_id),
    cwd: (.cwd // .workspace.current_dir),
    model: (.model.display_name // .model.id),
    context_used_percentage: .context_window.used_percentage,
    context_window_size: .context_window.context_window_size,
    token_count: ((.context_window.total_input_tokens // 0) + (.context_window.total_output_tokens // 0)),
    quota_5h_pressure:      (1 - (.quota[$fam + "-5h"].remaining_fraction // 1)),
    quota_5h_resets_at:     (.quota[$fam + "-5h"].reset_time | fromdateiso8601?),
    quota_weekly_pressure:  (1 - (.quota[$fam + "-weekly"].remaining_fraction // 1)),
    quota_weekly_resets_at: (.quota[$fam + "-weekly"].reset_time | fromdateiso8601?)
  }' > "$dir/$cid.json"
```

- [ ] **Step 4: Verify the jq is correct against a realistic agy payload** (the spec's date-parse verification). Run this one-off check (NOT a committed test — a manual confirmation the jq parses the `Z` format and selects the active-model window):

```bash
echo '{"model":{"id":"Gemini 3.5 Flash (High)"},"conversation_id":"c","cwd":"/r","context_window":{"used_percentage":1,"context_window_size":1048576,"total_input_tokens":10,"total_output_tokens":2},"quota":{"gemini-5h":{"remaining_fraction":1,"reset_time":"2026-06-29T17:32:54Z"},"gemini-weekly":{"remaining_fraction":0.9967623,"reset_time":"2026-07-06T06:33:26Z"},"3p-5h":{"remaining_fraction":0.5,"reset_time":"2026-06-29T17:32:54Z"},"3p-weekly":{"remaining_fraction":0.5,"reset_time":"2026-07-06T06:33:26Z"}}}' \
  | jq '(.model.id // "" | ascii_downcase | if startswith("gemini") then "gemini" else "3p" end) as $fam | {q5: (1-(.quota[$fam+"-5h"].remaining_fraction//1)), r5: (.quota[$fam+"-5h"].reset_time|fromdateiso8601?), qw: (1-(.quota[$fam+"-weekly"].remaining_fraction//1)), rw: (.quota[$fam+"-weekly"].reset_time|fromdateiso8601?)}'
```

Expected: `{"q5":0, "r5":1782840774, "qw":0.0032377, "rw":1783348406}` (gemini window chosen; `fromdateiso8601` parsed both `Z` timestamps to unix). If `fromdateiso8601` errors on the `Z` form, switch the two `reset_time | fromdateiso8601?` to `reset_time | sub("Z$";"+0000") | strptime("%Y-%m-%dT%H:%M:%S%z") | mktime` and re-run. Record the working form in the block.

- [ ] **Step 5: Run the snippet test + gate.**

Run: `cargo test --lib agy_snippet` then `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
Expected: PASS; clean.

- [ ] **Step 6: Commit.**

```bash
git add src/ui/provider_setup.rs
git commit -m "feat(ui): agy statusLine.command block — active-model-aware quota jq"
```

---

## Task 4: ObserveOnly regression extension + docs

**Files:** Modify `src/policy/engine.rs` (`agy_enriched_pane_recommendations_are_empty` ~line 474); `docs/ai/ARCHITECTURE.md` (agy provider-coverage row)

- [ ] **Step 1: Extend the regression to include quota.** In `src/policy/engine.rs`, in `agy_enriched_pane_recommendations_are_empty`, add a populated quota pressure to the agy `SignalSet` so the test proves quota pressure does NOT leak a recommendation for agy. Add this field to the `SignalSet { … }` literal (alongside the existing `context_pressure` / `model_name` / `token_count`):

```rust
            quota_5h_pressure: Some(MetricValue::new(0.95_f32, SourceKind::ProviderOfficial)),
```

The existing assertion (`recs` is empty) now also covers the quota advisory.

- [ ] **Step 2: Run it.**

Run: `cargo test --lib agy_enriched_pane_recommendations_are_empty`
Expected: PASS — recommendations still empty (the C3 engine guard blocks `eval_advisories`, incl. `quota_pressure_recommendations`, for agy). If it FAILS, a quota rec leaked — STOP and report (a gating regression), do not weaken the test.

- [ ] **Step 3: Update docs.** In `docs/ai/ARCHITECTURE.md`, extend the agy provider-coverage row (the v2.7.0 Slice C3 entry) to note: agy now also surfaces **quota** (`quota_5h_pressure` / `quota_weekly_pressure` + resets, active-model-aware `gemini-*`/`3p-*`, structured-only via the sidefile, ProviderOfficial, opt-in) while still emitting zero recommendations (C3 engine guard). plan_tier + session-cost remain out of scope.

- [ ] **Step 4: Run the full gate.**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --all-targets`
Expected: all green.

- [ ] **Step 5: Commit.**

```bash
git add src/policy/engine.rs docs/ai/ARCHITECTURE.md
git commit -m "test(agy): quota in the ObserveOnly regression + provider-coverage docs"
```

---

## Release (after all tasks, operator-gated)

Bundle as **v2.8.0**: bump version surfaces (package.json / Cargo.toml / Cargo.lock / VERSION.md / README) + mission ledger (mission.yaml / mission-history change_sequence 221 / CURRENT_STATE / VALIDATION row) + the Codex eval mirror, then STOP for the operator's explicit "릴리스" go (tag + npm publish is irreversible). Gate = Codex + human.

---

## Self-Review (writing-plans)

**1. Spec coverage:**

- Active-model-aware window selection → Task 3 (block `$fam`). ✓
- pressure = 1-remaining_fraction, clamped → Task 2 (Rust clamp) + Task 3 (jq `1 - …`). ✓
- resets ISO→unix (fromdateiso8601 + fallback) → Task 3 Step 4 (verified + fallback). ✓
- 4 quota fields onto existing `SignalSet` (zero new) → Tasks 1+2. ✓
- ObserveOnly inherited, regression extended → Task 4. ✓
- structured-only / opt-in / ProviderOfficial → Tasks 2+3 + Global Constraints. ✓
- Out of scope (plan_tier / cost / footer-quota) → not implemented; documented in Task 4. ✓

**2. Placeholder scan:** No TBD/vague-error-handling. The `ctx(...)` arg fill in Tasks 2 is directed to the existing `agy_enrichment_sidefile_overrides_footer` test verbatim (concrete reuse, not a gap). Task 3 Step 4 is a concrete one-off jq check with expected output + a named fallback.

**3. Type consistency:** `AgySidefile.quota_*_pressure` is `Option<f64>` (Task 1) → cast `as f32` + clamp in the fill (Task 2) → `SignalSet.quota_*_pressure: f32`. `quota_*_resets_at` is `Option<u64>` → `u64` field. The block jq emits matching keys (`quota_5h_pressure`, `quota_5h_resets_at`, `quota_weekly_pressure`, `quota_weekly_resets_at`) = the `AgySidefile` field names (Task 3 ↔ Task 1). `metric_f32`/`metric_u64` are the existing C3 closures (Task 2). ✓
