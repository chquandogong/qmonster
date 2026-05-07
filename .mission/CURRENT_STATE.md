# CURRENT_STATE

_Last updated: 2026-05-07 (Claude, v1.49.0 publication verified)_

## Mission

- Title: Qmonster v1.49.0 — operator UX polish: pane CMD argv resolution + overlay chrome consistency + TokenUsageReadFailed audit + Settings tab body scroll.
- Version surfaces: mission ledger target `1.49.0`; npm package metadata `qmonster@1.49.0`; latest local Git tag `v1.49.0`.
- Branch / worktree at handoff start: `main`, tag `v1.49.0`.
- Release publication state: v1.49.0 is published. `Release and Package Mirror` workflow run `25498621086` (2026-05-07, 7m1s, success) created GitHub Release `v1.49.0` with full asset set (binary tarball, npm tarball, SBOM, sbom-diff, checksums) and published `qmonster@1.49.0` to npm + GitHub Packages mirror. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`), v1.41.0 (`25424418078`), v1.42.0 (`25472444159`), v1.43.0 (`25474748447`), v1.44.0 (`25476534645`), v1.45.0 (`25478893257`), v1.46.0 (`25485133895`), v1.47.0 (`25490555532`), v1.48.0 (`25491535211`) publications also remain live — `npm view qmonster versions` lists `1.37.0` through `1.49.0` with `dist-tags.latest = 1.49.0`; GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45,46,47,48,49}.0`.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, the v1.41 a-overlay polish round, Phase H opt-in auto-snapshot, Phase 7 v1 anomaly observation surface, Phase 7 v2 promotion, Phase 7 v2 detectors, Phase 7 v3 (a+b), Phase 7 v3 (c), Phase 8 v1 token insights query layer, and Phase 8 v2 recommendation lifecycle ledger are complete. v1.49.0 ships as an operator UX polish bundle on top of the Phase 8 baseline; no new phase is opened.

## v1.49.0 Feature State

v1.49.0 is an operator UX polish bundle on top of the v1.48.0 Token Insights Phase 8 baseline. It does not change provider adapters, the audit chain core, the `SignalSet` schema, or any of the v1.48.0 / v1.47.0 / Phase 7 v3 (c) artifacts beyond extending `AuditEventKind` with one additive variant. Four themes ship together:

1. **Pane CMD column wrapper-interpreter resolution** (`src/adapters/process_memory.rs`, `src/app/event_loop.rs`, `src/ui/panels.rs`):
   - `read_descendant_cmdline` (and `read_descendant_cmdline_with_proc_root` for tests) walk `/proc/<pid>/cmdline` BFS-style and pick the **shallowest known CLI** inside the descendant tree to render the pane CMD column with full argv. Rule: a CLI subprocess never overwrites a parent CLI's argv; a non-CLI wrapper (node / python) is upgraded once a CLI descendant is found.
   - `KNOWN_CLI_COMMS` extended with `qmonster` so the qmonster pane resolves correctly instead of being treated as a non-CLI at depth 1.
   - The within-class BFS rule changed from "deepest CLI wins" to "shallowest CLI wins", fixing the pane-swap bug where claude → node MCP-tool overrode claude's own argv.
   - CMD column **display goal** (shallowest CLI for legibility) is kept separate from RSS **attribution goal** (highest-RSS within class for memory accounting).
   - `enhance_command_with_descendant_cmdline` is wired at the `PaneReport` push site in the event loop.
   - The pane card's CMD column switches from `aligned_field` (single-line ellipsized) to `wrap_aligned_field` so the full argv stays visible at narrow widths; `display_command` no longer ellipsizes.

2. **F-3 long-tail TODO closure: `TokenUsageReadFailed` audit event** (`src/domain/audit.rs`, `src/store/audit.rs`, `src/app/event_loop.rs`, `src/app/bootstrap.rs`):
   - `AuditEventKind::TokenUsageReadFailed` variant added; `as_str()` and `parse_kind()` mappings updated; both contract tests (`audit_event_kind_as_str_contract_locks_every_variant_string` + `parse_kind_inverts_as_str_for_every_variant`) extended to lock the new variant string.
   - `Context.token_usage_read_failed_logged: HashSet<String>` per-pane dedup field added so a chronic provider read failure emits one audit event per pane per session, not on every poll tick.
   - Stale "Gemini UX TODO" attribution removed from `format_prompt_send_proposal_hides_accept_key_when_gate_denies` test comment in `src/ui/alerts.rs` (feature already shipped).

3. **Overlay chrome consistency slice** (`src/ui/insights.rs`, `src/ui/anomaly_overlay.rs`, shared modal-geometry helpers, `src/app/insights_overlay.rs`):
   - Shared modal-geometry helpers (close styling, clamp sizing, title-row drag) so the `i` Token Insights and `n` Anomaly Events overlays match the `m` Metrics and `a` Pending Actions chrome.
   - Both overlays adopt the `BORDER_ACTIVE` line color and gain resizable geometry via the same `[/]/=//,/.` + title-drag controls. New tests cover the active-border style and size persistence.
   - Drag anchors clear on all non-drag mouse events (no more stuck drag state).
   - Anomaly history scroll is bounded by the active view.

4. **Settings tab body scroll for read-only Parameters / Rules / Badges** (`src/ui/settings.rs`, `src/app/settings_overlay.rs`, `src/app/tui_loop.rs`, `src/ui/dashboard.rs`):
   - `SettingsOverlay.scroll_offset: u16` plus `scroll_up`, `scroll_down`, `page_up`, `page_down`, `scroll_top`, `scroll_bottom` helpers.
   - `settings_max_scroll`, `settings_visible_body_rows` compute geometry from the body rect.
   - `settings_field_at_with_scroll`, `settings_integration_field_at_with_scroll` handle mouse hit-testing with scroll offset (existing wrappers preserved for back-compat).
   - New `handle_settings_overlay_key_with_viewport` intercepts `↑/↓/j/k/wheel/PgUp/PgDn/Home/End` for read-only Parameters / Rules / Badges tabs only; Thresholds / Integrations preserve arrow / wheel **field-selection** semantics.
   - Tab switching / open / reset zero `scroll_offset` so each tab opens at row 0.
   - Help overlay text and `UI_MANUAL.md` document the new keys.

Reference docs in tree: `docs/superpowers/specs/2026-05-07-overlay-chrome-consistency-design.md`, `docs/superpowers/plans/2026-05-07-overlay-chrome-consistency.md`.

## Latest Release Notes

- `v1.49.0` is an operator UX polish release over the v1.48.0 Phase 8 baseline.
- Local tag: `v1.49.0` points at the v1.49.0 release ledger sync commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.49.0 release ledger sync`.
- Reference plan / spec: `docs/superpowers/plans/2026-05-07-overlay-chrome-consistency.md` + `docs/superpowers/specs/2026-05-07-overlay-chrome-consistency-design.md` for the parallel-track work; the pane CMD argv work + F-3 audit event closure landed via direct main-pane commits.

## Post-tag polish (on main, untagged)

No genuinely-post-v1.49.0 work has landed yet.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 / v1.45.0 / v1.46.0 / v1.47.0 / v1.48.0 / v1.49.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` / `25474748447` / `25476534645` / `25478893257` / `25485133895` / `25490555532` / `25491535211` / `25498621086` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45,46,47,48,49}.0`; `npm view qmonster versions` lists `1.37.0` through `1.49.0` with `dist-tags.latest = 1.49.0`.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
2. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.49.0 validation at the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1265 lib tests + 61 integration tests, all green (lib up from 1209 at v1.48.0 baseline as new Settings scroll / overlay chrome / cmdline walker / audit-variant tests landed).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v1.48.0 still apply when the v1.49.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. Phase 7 D3 stays provider-blocked; tag protection / ruleset activation is operator-side. Otherwise pick from upstream backlog (no formal next-phase planning queued at the moment).
