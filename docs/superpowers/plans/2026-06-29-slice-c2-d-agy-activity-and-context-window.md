# Slices C2 + D — agy activity indicator + context_window_size — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use `- [ ]` checkboxes. Two independent mini-slices on one branch (`feat/slice-c2-d`), bundled into the next release (v2.5.1).

**Goal:** (C2) surface a live activity indicator for agy panes from the transcript (correlated via `history.jsonl`), keeping agy ObserveOnly; (D) surface the model context-window _size_ alongside `CTX %`, populated from the structured channels both Claude (sidefile) and Codex (rollout) already carry.

**Tech Stack:** Rust, serde/serde_json, existing `MetricValue`/`SourceKind`/`RuntimeFact`/`ParserContext`. No new deps.

## Global Constraints

- **C2 is opt-in + Heuristic + ObserveOnly-preserving.** agy transcript is reverse-engineered/undocumented → `SourceKind::Heuristic`, gated behind a new `[provider_setup] agy_transcript` toggle **default false**. The v2.4.0 six-gate ObserveOnly contract is untouched (no anomaly/cost/cache/profile surfaces) — C2 adds only an informational `RuntimeFact`.
- **Correlation:** `~/.gemini/antigravity-cli/history.jsonl` maps `workspace`(cwd)→`conversationId`. Pick the newest entry whose `workspace == current_path`; apply the **same 60s ambiguity guard** as codex_rollout / claude_sidefile (two distinct conversationIds recently active in one workspace → None). Then read `brain/<conversationId>/.system_generated/logs/transcript.jsonl`.
- **No token/model/cost from agy** — verified absent. C2 surfaces only last-activity recency + (optionally) the latest step kind. Do NOT claim quota/cost/context for agy.
- **D fill semantics:** `context_window_size` is `ProviderOfficial` (Claude sidefile `context_window.context_window_size`; Codex rollout `model_context_window`). Fill-when-absent. It is supplementary display only — does NOT change `context_pressure` computation.
- Shared-struct rule: D adds a `SignalSet` field → every task verifies with `cargo test --all-targets`, not `--lib` (per WORKFLOWS §6).
- Gates green before each commit: `cargo fmt --all --check` / `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args` / `cargo test --all-targets`. Baseline: 1588 lib + 70 integration.

---

## Slice C2 — agy activity indicator

### Task C2-1: `RuntimeFactKind::AgyActivity` + render

**Files:** `src/domain/signal.rs` (enum `RuntimeFactKind` ~line 76 + its tests), `src/ui/panels/mod.rs` (the `GROUPS` table ~line 2035 + any match needing the new variant).
**Interfaces produced:** `RuntimeFactKind::AgyActivity`.

- [ ] Write a failing test in `signal.rs` tests asserting a `RuntimeFact::new(RuntimeFactKind::AgyActivity, "...", SourceKind::Heuristic)` round-trips (kind match). RED (variant missing).
- [ ] Add `AgyActivity` to `RuntimeFactKind`. If the enum has a `Display`/label `match`, add the arm (label e.g. `"agy"`). Add it to the panels `GROUPS` table under the agy/runtime group so it renders.
- [ ] `cargo test --all-targets` green; commit `feat(domain): RuntimeFactKind::AgyActivity for agy transcript activity`.

### Task C2-2: `agy_transcript.rs` reader

**Files:** Create `src/adapters/agy_transcript.rs`; register `pub mod agy_transcript;` in `src/adapters/mod.rs`.
**Interfaces produced:** `pub struct AgyActivity { pub conversation_id: String, pub last_activity_unix: Option<u64>, pub latest_step: Option<String> }` and `pub fn read_agy_activity(gemini_home: &Path, current_path: &str) -> Option<AgyActivity>`.

- [ ] Write failing tests (real masked fixture from this machine's `~/.gemini/antigravity-cli/history.jsonl` + a `brain/<id>/.../transcript.jsonl`): (a) matches newest `workspace==cwd` → returns its conversation_id + last_activity_unix (from the latest transcript step `created_at` ISO8601→unix); (b) two distinct conversationIds for the same workspace within 60s → None (ambiguity); (c) no workspace match / missing history → None; (d) empty cwd → None.
- [ ] Implement (mirror `codex_rollout.rs`): parse history.jsonl lines `{workspace, conversationId, timestamp}`, filter `workspace==current_path` with a conversationId, sort by `timestamp` desc; ambiguity guard on the top-two **distinct** conversationIds within `AMBIGUITY_WINDOW = Duration::from_secs(60)`; for the chosen conversation, read its transcript, take the last well-formed step's `created_at` → unix + its `type`. Defensive parsing; `gemini_home` = `<home>/.gemini`.
- [ ] `cargo test --all-targets` green; commit `feat(agy-transcript): history.jsonl-correlated activity reader`.

### Task C2-3: `agy_transcript` toggle + ParserContext thread

**Files:** `src/app/config.rs` (`ProviderSetupConfig` + default false + doc + default test), `src/adapters/mod.rs` (`ParserContext.agy_transcript_enabled: bool` + `ctx()` helper), `src/app/event_loop.rs` (set from config). Mirror the `codex_rollout` toggle exactly.

- [ ] Extend the config default test asserting `codex_rollout` → also assert `!provider_setup.agy_transcript` (default false). RED.
- [ ] Add `codex_rollout`-pattern field + ParserContext thread; fix all compiler-flagged construction sites (`--all-targets` surfaces test-file sites too). Commit `feat(config): agy_transcript toggle (default false) via ParserContext`.

### Task C2-4: enricher + docs + validation

**Files:** `src/adapters/mod.rs` (`parse_for_with_environment` Antigravity branch + `agy_transcript_process_confirmed` helper mirroring `codex_rollout_process_confirmed` with needle `"agy"`), `docs/ai/ARCHITECTURE.md` (agy note: activity-only enricher shipped, analytics still Hidden).

- [ ] Write integration tests in `mod.rs` (new `mod agy_transcript_integration_tests`): with toggle on + Provider::Antigravity + cwd + descendant `agy` process + a fixture history/transcript, `parse_for_with_environment` pushes a `RuntimeFact{kind:AgyActivity, source_kind:Heuristic}`; toggle off → no fact; non-agy descendant → no fact.
- [ ] Implement the branch: gated on `ctx.agy_transcript_enabled && Provider::Antigravity && !identity_conflict && !cwd.empty && agy_transcript_process_confirmed`; resolve `gemini_home` from `home_dir`; on `Some(activity)`, push `RuntimeFact::new(RuntimeFactKind::AgyActivity, format!("active … (transcript)"), SourceKind::Heuristic).with_provider(Provider::Antigravity)`. The six v2.4.0 ObserveOnly gates are untouched (no anomaly/cost/cache/profile/insights/token-sample change).
- [ ] Update `docs/ai/ARCHITECTURE.md` agy paragraph (activity enricher shipped; analytics remain Hidden). `cargo test --all-targets` green; commit.

---

## Slice D — context_window_size surfacing

### Task D-1: `SignalSet.context_window_size` field + backfill

**Files:** `src/domain/signal.rs` (add field), every `SignalSet { … }` literal the compiler flags (production + tests).
**Interfaces produced:** `SignalSet.context_window_size: Option<MetricValue<u64>>`.

- [ ] Add `pub context_window_size: Option<MetricValue<u64>>` to `SignalSet` (next to `context_pressure`). Build; the compiler flags every literal missing it — add `context_window_size: None,` to each (mirror the `codex_rollout_enabled` backfill; `--all-targets` surfaces `tests/*.rs` sites).
- [ ] `cargo test --all-targets` green (count unchanged); commit `feat(domain): SignalSet.context_window_size field`.

### Task D-2: populate from both structured channels

**Files:** `src/adapters/mod.rs` (`apply_claude_sidefile`: set from `sidefile.context_window.context_window_size`), `src/adapters/codex_rollout.rs` + its mod.rs enricher (carry `model_context_window` through `CodexRolloutSignals` → fill `context_window_size`).

- [ ] Tests: Claude sidefile with `context_window_size: 1000000` → `signals.context_window_size == Some(1_000_000)` ProviderOfficial; Codex rollout with `model_context_window: 258400` → filled. RED.
- [ ] Implement: `apply_claude_sidefile` fills it (fill-when-absent, `metric()` helper); extend `CodexRolloutSignals` with `context_window: Option<u64>` (from `info.model_context_window`) and the Codex enricher fills `signals.context_window_size`. ProviderOfficial. `cargo test --all-targets` green; commit.

### Task D-3: UI surface + docs + validation

**Files:** `src/ui/panels/mod.rs` (the `CTX {:.0}%` render ~line 1380) and/or `src/ui/metrics.rs`; `docs/ai/ARCHITECTURE.md` matrix.

- [ ] Test: the pane/metrics line renders the window size when present (e.g. `CTX 21% of 1.0M`); absent → just `CTX 21%` (no regression). RED.
- [ ] Implement a small helper that appends ` of <human(size)>` to the CTX line when `context_window_size` is Some (reuse any existing human-number formatter; else `1_000_000 → "1.0M"`). Update the ARCHITECTURE matrix note. `cargo test --all-targets` green; commit.

---

## Self-Review

- C2: opt-in + Heuristic + ObserveOnly-preserving + ambiguity-guarded correlation → consistent with the project's honesty rules; no analytics claimed for agy. D: supplementary ProviderOfficial display, fill-when-absent, no context_pressure change.
- Type consistency: `AgyActivity`/`read_agy_activity` (C2-2) consumed in C2-4; `agy_transcript_enabled` (C2-3) consumed in C2-4; `context_window_size: Option<MetricValue<u64>>` (D-1) populated in D-2, rendered in D-3; `CodexRolloutSignals.context_window` added in D-2.
- Shared-struct (D-1) + ParserContext (C2-3) changes verify with `--all-targets`.

## After C2 + D

Bundle with the unreleased sidefile guard (`26021e8`) → one **v2.5.1** release: ledger sync (mission.yaml/history/CURRENT_STATE/VALIDATION) + external Codex cross-review + human sign-off + tag + npm. Operator-gated (npm publish on explicit go).
