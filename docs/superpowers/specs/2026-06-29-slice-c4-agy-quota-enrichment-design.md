# Slice C4 — agy quota enrichment — Design Spec

**Status:** APPROVED (brainstorming, 2026-06-29). Next: `writing-plans`.

**Goal:** Map agy's structured quota → the existing `SignalSet`
`quota_5h_pressure` / `quota_weekly_pressure` (+ `quota_5h_resets_at` /
`quota_weekly_resets_at`), **active-model-aware**, so agy panes show quota
windows at parity with Claude. Builds directly on Slice C3 (v2.7.0); reuses the
C3 agy sidefile reader, enrichment branch, and recommended `statusLine.command`
block. Zero new `SignalSet` fields, opt-in, `ProviderOfficial`.

**Architecture:** The recommended agy `statusLine.command` block (operator-applied,
extended here) selects the active model's quota window and writes flat
quota fields to the agy sidefile; the C3 enrichment branch copies them onto the
`SignalSet`. Quota is **structured-only** (no footer fallback — the agy footer's
single aggregate `quota` item cannot disambiguate 5h vs weekly vs model).

**Tech Stack:** Rust 2024, serde/serde_json, the existing C3 adapters; the
recommended block uses `jq` (incl. `fromdateiso8601`, `ascii_downcase`). No new
dependencies.

## Global Constraints

- **Reuse existing `SignalSet` quota fields only** — `quota_5h_pressure`,
  `quota_weekly_pressure` (`f32`), `quota_5h_resets_at`, `quota_weekly_resets_at`
  (`u64` unix). No new `SignalSet` field.
- **ObserveOnly preserved — inherited from C3, no new gating.** The quota
  advisories (`quota_pressure_recommendations`, `src/policy/rules/advisories.rs`)
  live inside `eval_advisories`, which the C3 engine guard
  (`provider_is_observe_only` early-return in `Engine::evaluate`) returns BEFORE
  for agy. So populating agy quota pressures leaks ZERO recommendations. C4 adds
  NO new rec-gating; it EXTENDS the regression to prove quota stays inert.
- **No fabrication** — a quota field is `Some` only when the sidefile provided it.
- **Opt-in via the existing `[provider_setup] agy_enrichment` toggle** — no new toggle.
- **SourceKind `ProviderOfficial`**, `.with_confidence(0.9).with_provider(Antigravity)`.
- **Structured-only** — quota comes solely from the sidefile; no footer scrape path.
- Validation: `cargo fmt --all --check` + `cargo clippy --all-targets -- -D
warnings -A clippy::uninlined_format_args` + `cargo test --all-targets`.

---

## 1. Background — agy quota shape (C3 Task-0 spike, verified 2026-06-29)

agy feeds `statusLine.command` a session JSON on stdin whose `quota` object
carries four windows (no new spike needed — confirmed):

```json
"quota": {
  "gemini-5h":     { "remaining_fraction": 1.0,       "reset_time": "2026-06-29T17:32:54Z", "reset_in_seconds": 17996 },
  "gemini-weekly": { "remaining_fraction": 0.9967623, "reset_time": "2026-07-06T06:33:26Z", "reset_in_seconds": 583228 },
  "3p-5h":     { "remaining_fraction": 1.0, "reset_time": "…Z", "reset_in_seconds": … },
  "3p-weekly": { "remaining_fraction": 1.0, "reset_time": "…Z", "reset_in_seconds": … }
}
```

`gemini-*` = the Gemini-model quota; `3p-*` = third-party (Claude / GPT-OSS)
model quota — agy is multi-model (`agy models` lists Gemini 3.x, Claude Sonnet/
Opus 4.6, GPT-OSS). `remaining_fraction` is `[0,1]` remaining; `reset_time` is an
absolute ISO-8601 (Z) timestamp; `reset_in_seconds` is relative (NOT used —
relative to agy's refresh time, not Qmonster's read time, so the absolute
`reset_time` is preferred).

## 2. Mapping (decisions: Q1 = active-model-aware, Q2 = no plan_tier)

- **Window selection (active-model-aware):** if the session `model.id` starts
  with `gemini` (case-insensitive) → use `gemini-5h` / `gemini-weekly`; else
  (Claude / GPT-OSS / anything) → `3p-5h` / `3p-weekly`.
- **pressure = `1 - remaining_fraction`**, clamped `[0,1]` (defensive, in Rust).
- **resets_at = `reset_time` (ISO) → unix** via jq `fromdateiso8601` in the block;
  carried as `u64`. The implementation MUST verify jq parses agy's exact format
  (`"2026-06-29T17:32:54Z"`); if `fromdateiso8601` rejects the trailing `Z`, fall
  back to `(.reset_time | sub("Z$";"+0000") | strptime("%Y-%m-%dT%H:%M:%S%z") | mktime)`.
  The `?`-guarded form leaves `resets_at` null on parse failure (never blocks quota).
- `plan_tier`, agy session-cost, and footer-quota are OUT of scope (see §6).

## 3. Components (all extend C3 assets)

### Modify: recommended block `AGY_SIDEFILE_BLOCK` (`src/ui/provider_setup.rs`)

Extend the C3 jq so the operator-applied block ALSO emits the four flat quota
fields, selecting the window by active model. Design (final jq confirmed in the
plan):

```bash
# inside the existing `jq '{ ... }'` that writes the sidefile, add a $fam binding:
#   (.model.id // "" | ascii_downcase | if startswith("gemini") then "gemini" else "3p" end) as $fam
# then add to the emitted object:
#   quota_5h_pressure:      (1 - (.quota[$fam + "-5h"].remaining_fraction // 1)),
#   quota_5h_resets_at:     (.quota[$fam + "-5h"].reset_time     | fromdateiso8601?),
#   quota_weekly_pressure:  (1 - (.quota[$fam + "-weekly"].remaining_fraction // 1)),
#   quota_weekly_resets_at: (.quota[$fam + "-weekly"].reset_time | fromdateiso8601?)
```

The active-model-aware selection lives here (the block is Qmonster-authored,
operator-applied). The sidefile stays a flat schema matching `SignalSet`.

### Modify: `AgySidefile` (`src/adapters/agy_sidefile.rs`)

Add four `#[serde(default)]` fields (no other change to the reader / ambiguity
guard / dir):

```rust
    pub quota_5h_pressure: Option<f64>,
    pub quota_5h_resets_at: Option<u64>,
    pub quota_weekly_pressure: Option<f64>,
    pub quota_weekly_resets_at: Option<u64>,
```

### Modify: agy enrichment branch (`src/adapters/mod.rs`)

In the existing C3 agy sidefile branch (after the model/context/token fills),
fill the four quota fields from the sidefile (same present-and-unambiguous gate;
override semantics, consistent with C3's sidefile fields). Use the same metric
idiom; clamp pressure in Rust:

```rust
    if let Some(p) = sf.quota_5h_pressure {
        signals.quota_5h_pressure = Some(metric_f32((p as f32).clamp(0.0, 1.0)));
    }
    if let Some(ts) = sf.quota_5h_resets_at {
        signals.quota_5h_resets_at = Some(metric_u64(ts));
    }
    // …weekly likewise…
```

### Modify: docs

`docs/ai/ARCHITECTURE.md` provider-coverage matrix agy row: add quota
(5h/weekly pressure + resets, structured-only, active-model-aware,
ProviderOfficial, opt-in).

## 4. Data flow

```
operator applies the (extended) recommended block → agy writes sidefile with
quota_5h/weekly_pressure + resets_at → C3 sidefile reader (cwd-matched,
ambiguity-guarded) → C3 enrichment branch fills the 4 SignalSet quota fields
(ProviderOfficial, clamped) → panels render "quota 5h X% · resets …" (parity).
```

No quota when the operator hasn't applied the block (structured-only; acceptable
for an opt-in enrichment).

## 5. ObserveOnly (inherited; regression extended)

No new gating. The C3 `Engine::evaluate` guard returns empty recommendations for
agy before `eval_advisories` runs, so `quota_pressure_recommendations` cannot
fire for agy regardless of quota pressure. C4 EXTENDS the existing
`agy_enriched_pane_recommendations_are_empty` engine test (and/or the six-gate
regression) to set `quota_5h_pressure = Some(0.95)` on the enriched agy
`SignalSet` and assert `Engine::evaluate` still returns ZERO recommendations and
agy stays off the six analytic gates.

## 6. Scope

**In scope:** agy `quota_5h_pressure` / `quota_weekly_pressure` /
`quota_5h_resets_at` / `quota_weekly_resets_at` (active-model-aware, structured-
only); the block jq extension; the `AgySidefile` + enrichment + regression +
docs.

**Out of scope (explicit):**

- **plan_tier** (Q2 = skip) — static/low-value; would need a new field or pollute
  `model_name`; defer.
- **session-cost** — not on the statusLine stdin (subscription tier, no per-token
  cost); `/usage` is interactive-only.
- **footer quota** — the footer's single aggregate `quota` item can't disambiguate
  5h/weekly/model; quota is structured-only.

## 7. Testing

- `AgySidefile` deserializes the four new quota fields (+ tolerates absence).
- Enrichment fills the four `SignalSet` quota fields from the sidefile (override;
  pressure clamped `[0,1]`).
- ObserveOnly regression extended: enriched agy + `quota_5h_pressure = 0.95` →
  `Engine::evaluate` recommendations empty + agy off the six gates.
- The block extension is verified at the snippet level (contains the quota jq +
  the `$fam` model-aware selection + `fromdateiso8601`).

## 8. Version & release gate

Feature minor → **v2.8.0**. Gate = Codex cross-review + human sign-off (Gemini
leg retired). Same subagent-driven TDD + opus whole-branch + Codex cycle as C3.

## 9. Decision log (brainstorming, 2026-06-29)

- **Q1 → B (active-model-aware):** `model.id` Gemini → `gemini-*`, else `3p-*`.
  The selection is done in the recommended block's jq.
- **Q2 → A (skip plan_tier):** quota-only; tier deferred (low-value, avoids a new
  field).
- Quota is **structured-only** (no footer fallback). ObserveOnly inherited from
  C3's engine guard (no new gating; regression extended).
