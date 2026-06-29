# Slice C3 — agy structured/scrape enrichment — Design Spec

**Status:** APPROVED (brainstorming, 2026-06-29). Next: `writing-plans`.

**Goal:** Lift agy (Antigravity CLI) panes out of pure ObserveOnly to display
`model_name` + `context_pressure`/`context_window_size` + `token_count`, using
the same structured-preferred-with-scrape-fallback enrichment that Claude
(Slice A sidefile) and Codex (Slice B rollout / Slice D scrape) already get —
reusing existing `SignalSet` fields (zero new fields), behind an opt-in toggle,
without breaking the v2.4.0 ObserveOnly contract.

**Architecture:** Hybrid. (1) _Structured_: operator applies a recommended agy
`statusLine.command` block (shown in the Provider Setup overlay) that dumps the
session JSON to a sidefile; Qmonster reads it (mirrors Slice A). (2) _Scrape_:
Qmonster parses model/context/token from the agy pane's own captured footer
text (mirrors Slice D Codex scrape). Structured overrides scrape when
unambiguously attributed; scrape is the always-pane-correct base.

**Tech Stack:** Rust, existing `src/adapters/` enrichment seam, serde_json,
tmux `capture-pane` (already used). No new dependencies.

## Global Constraints

- **ObserveOnly is preserved.** C3 is display-only enrichment. The v2.4.0
  six-gate contract MUST stay byte-identical: agy remains excluded from
  anomalies (`AnomalyKind::supports_provider`), `provider_honesty` (Hidden),
  `profile_switch` (None), insights coverage ("unsupported"), and the
  token-sample filter. Populating `token_count` for display MUST NOT re-include
  agy in token-sample collection or any analytic/actuation surface.
- **Recommend-first / no config writes.** Qmonster never writes the user's agy
  `settings.json`. It only _shows_ a recommended `statusLine.command` block (the
  operator applies it) and _reads_ the resulting sidefile — exactly as it does
  for the Claude sidefile today.
- **Reuse existing `SignalSet` fields** — `model_name`, `context_pressure`,
  `context_window_size`, `token_count`. No new `SignalSet` field (avoids the
  fixture ripple the v2.5.0/Slice D work was careful about).
- **No fabrication.** A field is `Some` only when a real source (sidefile or
  footer) provided it; otherwise `None`.
- **Opt-in.** Gated by `[provider_setup] agy_enrichment` (default `false`),
  consistent with the conservative agy stance of C2 (`agy_transcript`).
- **SourceKind = ProviderOfficial** for both paths (values originate from agy;
  scrape is transport, same basis as the Slice D Codex statusline scrape).
- Validation: `cargo fmt --all --check` + `cargo clippy --all-targets -- -D
warnings -A clippy::uninlined_format_args` + `cargo test --all-targets`.
  Shared-struct changes (`ParserContext`, config) verified with
  `--all-targets`, not just `--lib`.

---

## 1. Background — verified agy surfaces (2026-06-29, agy 1.0.13)

Re-probed against a live `agy --dangerously-skip-permissions` session and agy's
shipped docs. Findings (full trail in
`.docs/claude/Qmonster-2026-06-29-blocker-recheck-gemini-agy.md`):

- **`~/.gemini/antigravity-cli/settings.json`** carries
  `"statusLine": { "type": "", "command": "", "enabled": true }` — structurally
  identical to Claude Code's statusline (a `command` that emits the status line).
- **Official CLI docs**
  (`~/.gemini/antigravity-cli/builtin/skills/antigravity_guide/references/cli.md`):
  `/statusline` toggles the status line; `/usage` or `/quota` "Displays token
  usage and session cost statistics"; `statusLine` is a documented config
  object; `useG1Credits` toggles Google One quotas.
- **Footer items** (gemini-family `ui.footer.items`) include `model-name`,
  `quota`, `context-used`, `token-count`, `code-changes`, `session-id`, `auth` —
  i.e. model/context/token render as scrapeable text when enabled.
- **Live evidence:** the agy header + footer render `Gemini 3.5 Flash (High)` +
  account tier (`Google AI Pro`) right now → scrapeable today.
- **`~/.gemini/antigravity-cli/history.jsonl`** maps `workspace → conversationId`
  (keys: `conversationId, display, timestamp, type, workspace`); the C2 reader
  already correlates on it and handles `conversationId: null` rows.
- **Not used by C3:** `conversations/<id>.db` is undocumented protobuf;
  `brain/<uuid>/.system_generated/logs/transcript.jsonl` carries no
  token/model/cost (C2's ObserveOnly call stands). `/usage` session-_cost_ is
  interactive-only (not passively scrapeable) → out of scope.

## 2. Components & files

### New: `src/adapters/agy_sidefile.rs`

Structured reader, mirroring `src/adapters/claude_sidefile.rs`.

- `pub struct AgySidefile { … }` — fields finalized by the Task-0 spike (§5).
  Expected to carry at minimum a session/conversation id, `cwd`, model, and
  (if agy passes them) context-used / context-window / token counts.
- `pub fn read_agy_sidefile_for_path(sidefile_dir: &Path, current_path: &str) ->
Option<AgySidefile>` — collect cwd-matching sidefiles newest-first, keep up to
  two with **distinct session ids**, return `None` if the two newest distinct
  sessions fall within the ambiguity window (reuse Slice A's distinct-session
  guard verbatim; `None` id counts as always-distinct). Returns `None` when the
  dir is absent (operator hasn't applied the block) — self-gating.
- The recommended sidefile dir mirrors Claude's:
  `~/.local/share/ai-cli-status/agy/<id>.json`.

### New: `src/adapters/agy_footer.rs`

Scrape parser over the pane's captured text.

- `pub struct AgyFooter { model: Option<String>, context_used: Option<f32>,
context_window: Option<u64>, token_count: Option<u64> }`.
- `pub fn parse_agy_footer(captured_text: &str) -> AgyFooter` — parse the
  agy header/footer (e.g. `Gemini 3.5 Flash (High)`; `context-used`/`token-count`
  footer items when enabled). Each field independently `Some`/`None`. agy's main
  dispatch stays on `common::parse_common_signals` (v2.4.0 untouched); footer
  parsing runs only in the enrichment step.

### Modify: `src/adapters/mod.rs`

agy enrichment branch in `parse_for_with_environment` (alongside
`apply_claude_sidefile` / the codex_rollout / agy_transcript branches):

- Gate: `ctx.agy_enrichment_enabled` AND the agy descendant-process confirm
  (reuse C2's strict `pane_pid` check — returns `false` on missing `pane_pid`,
  since agy correlation is workspace-based and weaker than Claude `session_id` /
  Codex cwd+rollout).
- Fill order: `parse_agy_footer` first (always pane-correct) → then
  `read_agy_sidefile_for_path`; if the sidefile is present AND passes the
  ambiguity guard, it **overrides** the footer values; otherwise the footer
  values stand. Set `SourceKind::ProviderOfficial` on populated fields.

### Modify: `src/adapters/mod.rs` `ParserContext`

Add `pub agy_enrichment_enabled: bool` (next to `codex_rollout_enabled` /
`agy_transcript_enabled`; default `false` in all constructors/fixtures).

### Modify: `src/app/config.rs`

Add `pub agy_enrichment: bool` to `ProviderSetupConfig` (default `false`, with a
doc comment mirroring `agy_transcript`'s "opt-in" note). Thread it into
`ParserContext.agy_enrichment_enabled`.

### Modify: Provider Setup overlay (agy tab)

Extend the agy tab's `snippet_for_tab` / `render_tab_content` (the
`ProviderSetupTab::ALL`-driven content from v2.4.1) with a **recommended agy
`statusLine.command` sidefile-export block** the operator copies into
`~/.gemini/antigravity-cli/settings.json`. Exact block content is produced by
the Task-0 spike (§5).

## 3. Data flow & attribution

```
pane capture ─▶ provider detect (agy, v2.4.0) ─▶ [agy_enrichment ON
              + agy descendant confirmed] ─▶ parse_agy_footer(pane text)   ── always pane-correct
                                            └▶ read_agy_sidefile(cwd)      ── overrides IFF unambiguous
              ─▶ SignalSet { model_name, context_pressure,
                             context_window_size, token_count } (ProviderOfficial)
              ─▶ dashboard renders agy pane like Claude/Codex
```

**Attribution asymmetry (key insight):**

- _Footer scrape_ reads the pane's **own** captured text → attribution is always
  exact (no cross-pane ambiguity). This is the base layer.
- _Sidefile_ is matched by `cwd` → two same-cwd agy panes are ambiguous → guarded
  by C2's `history.jsonl` correlation + the distinct-session ambiguity window.
  The sidefile overrides the footer **only when** the guard passes; otherwise the
  always-correct footer values stand.

## 4. Error handling

- Sidefile dir missing / no cwd match → structured path is a no-op; footer
  scrape (or `None`) stands.
- Sidefile ambiguous (two distinct sessions in window) → ignore sidefile, keep
  footer values.
- Footer item absent/unparseable → that field is `None`.
- Toggle off OR no confirmed agy descendant → no enrichment (current v2.4.0
  ObserveOnly behavior, unchanged).

## 5. Task-0 spike — confirm the agy `statusLine.command` contract

The single real unknown: agy's docs don't specify what data `statusLine.command`
receives. Resolve empirically before finalizing `AgySidefile` + the recommended
block.

- **Method:** instrument `statusLine.command` with a dump script (writes
  stdin + env + argv to a capture file), run **`agy --print "<prompt>"`** (the
  documented headless mode) once, and inspect what was passed. The spike's
  FIRST sub-step is choosing a safe instrumentation that both leaves the user's
  live config undisturbed AND preserves auth (agy needs
  `~/.gemini/antigravity-cli/oauth_creds.json` to run, so a bare temp `HOME`
  fails before the status line fires): prefer an agy config-dir / settings
  override if one exists; else copy `~/.gemini` into an isolated dir (carrying
  auth) and modify only that copy's `statusLine`; else back-up → modify →
  restore the live `settings.json` under a guard. Never persist or commit
  `oauth_creds.json` into the repo/scratchpad.
- **Outputs:** (a) the input channel (expected: session JSON on stdin,
  Claude-style); (b) which fields it carries (model? context-used?
  context-window? token counts?); (c) the finalized `AgySidefile` serde schema;
  (d) the exact recommended `statusLine.command` block for the overlay.
- **Branch:** if agy does NOT pass context/token to the command (only model),
  the structured path carries model only and the footer scrape supplies
  context/token — no design change (the hybrid already covers this). The spike
  result is recorded; it cannot block the scrape path.

## 6. Testing strategy

- **agy_sidefile reader:** cwd match; distinct-session ambiguity → `None`;
  newest-by-mtime among distinct sessions; dir-absent → `None`; field parse.
- **agy_footer parser:** sample agy header/footer text → expected model /
  context / token; absent footer items → `None` per field.
- **Precedence:** sidefile overrides footer when guard passes; footer stands when
  sidefile ambiguous or absent; both absent → `None`.
- **Gating:** `agy_enrichment` off → no enrichment; on + no agy descendant
  (missing `pane_pid`) → no enrichment.
- **ObserveOnly regression (critical):** with `model_name`/`token_count`
  populated for an agy pane, the six v2.4.0 gates still hold — agy stays out of
  anomalies, `provider_honesty` Hidden, `profile_switch` None, insights
  "unsupported", and token-sample collection.
- **SourceKind:** populated agy fields carry `ProviderOfficial`.

## 7. Scope

**In scope:** `model_name`, `context_pressure` + `context_window_size`,
`token_count` for agy panes; the structured reader + footer parser + enrichment
wiring + `agy_enrichment` toggle + the Provider Setup recommended block.

**Out of scope (explicit):**

- **quota / tier** — agy's quota model (Google AI Pro tier + `useG1Credits`)
  doesn't map to Claude's 5h/weekly windows; needs its own mapping design → a
  later slice.
- **session cost** — only via the interactive `/usage` command; not passively
  scrapeable under ObserveOnly.
- **agy main dispatch** — stays on `common::parse_common_signals` (v2.4.0);
  C3 adds enrichment only.
- The `conversations/<id>.db` protobuf surface (undocumented, fragile).

## 8. Version & release gate

Feature minor → **v2.7.0**. Release gate = Codex cross-review + human sign-off
(current contract; Gemini leg retired, re-checked 2026-06-29 still blocked).

## 9. Decision log (brainstorming Q&A, 2026-06-29)

- **Q1 → C (hybrid):** structured sidefile preferred + footer-scrape fallback.
- **Q2 → A (core 3):** model + context% + token-count; reuse existing fields;
  quota/cost deferred/out.
- **Q3 → all three defaults confirmed:** (1) `agy_enrichment` default `false`;
  (2) structured overrides scrape when unambiguous, scrape fills otherwise,
  `None` if neither; (3) SourceKind ProviderOfficial for both paths (correcting
  an earlier "Heuristic" note for footer scrape — the value is agy's own;
  attribution stays Heuristic-guarded via the ambiguity window).
