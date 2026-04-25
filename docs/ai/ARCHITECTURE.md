# ARCHITECTURE

- Version: v0.4.0
- Date: 2026-04-20 (round r2 reconciled) / 2026-04-25 (implementation sync through v1.14.1 + local doc/idle consistency follow-up)
- Status: canonical architecture reference; phase notes below describe the historical rollout and current invariants.

## One-line shape (r2 canonical)

```
tmux::RawPaneSnapshot
   → domain::IdentityResolver
   → adapters::ProviderParser
   → domain::SignalSet
   → policy::Engine
   → app::EffectRunner
   ↘                     ↘
  ui::ViewModel       store::EventSink
```

Two non-negotiable rules:

1. **Identity resolution precedes provider dispatch.** `adapters/`
   never infers `provider` or `role` — it receives a resolved identity
   from `domain/` and parses tails into signals.
2. **Policy is pure.** `policy/` performs no IO. It returns
   `Recommendation`s (advisory) and `RequestedEffect`s (gated by the
   actuation allow-list in `app::EffectRunner`).

## Core loop

```
observe → classify → recommend → archive → checkpoint → limited actuation
```

No step silently skips another. `limited actuation` is restricted to
the allow-list in `mission.yaml` constraints and `config [actions]`.
Phase 1 kept only an in-memory `EventSink`; Phase 2 added archive,
checkpoint, retention, and durable audit storage.

## Module layout (Rust crate)

```
src/
  main.rs      # CLI entry + current TUI event loop (still too large)
  app/         # bootstrap, config+safety-precedence, event loop, effect gate
  domain/      # pure types: identity, origin, signal, recommendation, audit, lifecycle
  tmux/        # polling first; control-mode-capable PaneSource trait
  adapters/    # per-provider tail parsers (no identity inference)
  policy/      # pure rules; Phase 1 = rules/alerts.rs;
               # Phase 3 adds rules/{advisories,concurrent}.rs;
               # Phase 4 adds rules/profiles.rs (provider-profile recommender)
  store/       # Phase 1: EventSink trait + NoopSink/InMemorySink
               # Phase 2: sqlite, archive_fs, audit (type-level raw split), snapshots, retention
  ui/          # ratatui widgets, alert queue, per-pane panels, theme
  notify/      # desktop / terminal bell; severity-aware rate limiting
```

The long-term target is still a thinner `src/main.rs`, but the current
implementation keeps the TUI event loop and modal orchestration there.
The invariant that matters is boundary purity: provider parsing stays in
`adapters/`, policy stays pure, storage stays out of `ui/`, and tmux
stays unaware of provider semantics.

## Module responsibilities

### `app/`

Owns startup, config load (with safety-precedence enforcement — see
"Safety precedence" below), polling cadence, event loop, shutdown.
Wires the other modules. Holds the top-level recommend-vs-actuate gate
in `effects.rs`. Does NOT contain business rules.

### `domain/`

Pure types, no IO:

- `PaneIdentity = { provider, instance, role, pane_id }`
- `IdentityConfidence = High | Medium | Low | Unknown`
- `ResolvedIdentity = { identity, confidence }`
- `IdentityResolver` — maps `RawPaneSnapshot` → `ResolvedIdentity`.
- `SourceKind = ProviderOfficial | ProjectCanonical | Heuristic | Estimated`
- `MetricValue<T> = { value, source_kind, confidence, provider }`
- `Signal`, `SignalSet`, `TaskType`, `Severity`,
  `Recommendation`, `RequestedEffect`, `AuditEvent`, `Finding`.
- `PaneLifecycle` — transitions (`pane_dead`, session re-attach) that
  drain alerts + reset per-pane pressure state.

### `tmux/`

A single trait `PaneSource` with a polling implementation first.
Control-mode implementation must be drop-in substitutable. Returns
`RawPaneSnapshot`, NOT `PaneSnapshot` — identity is resolved downstream.
Format strings for `list-panes` and `capture-pane` live here. Knows
nothing about providers, roles, or signal semantics.
`[ProviderOfficial: tmux wiki / Formats / Control Mode]` informs the
format strings and lifecycle assumptions.

### `adapters/`

Per-provider tail parsers: `claude.rs`, `codex.rs`, `gemini.rs`,
`qmonster.rs`. Each takes a `ResolvedIdentity` and the raw tail and
returns domain `Signal`s. No identity inference. No cross-provider
logic. No SQLite. No ratatui.

### `policy/`

Pure: consumes `(ResolvedIdentity, SignalSet)` and emits
`(Recommendation | RequestedEffect)[]`. Reads thresholds from config.
Gates `aggressive_mode` behind the `quota_tight` flag AND the
`IdentityConfidence` gate (low-confidence panes suppress
provider-specific recommendations). Every rule attaches a
`SourceKind` to its output. Phase 1 ships `rules/alerts.rs` only;
the A–G canonical situations (log storm / code exploration / context
pressure / verbose output / permission wait / quota-tight /
repeated output) land in Phase 3. Phase 4 adds `rules/profiles.rs` —
a provider-profile recommender that bundles ProviderOfficial CLI
flags / settings / env vars into named `ProjectCanonical` profiles
(e.g. `claude-default`) with per-lever citations, consumed via
`Engine::evaluate` alongside alerts and advisories.

### `store/`

- **Phase 1:** `EventSink` trait + `NoopSink` + `InMemorySink`. No
  durable storage, no SQLite, no archive writer.
- **Phase 2:**
  - `sqlite.rs` — audit DB (metadata only).
  - `archive_fs.rs` — raw tail archive with preview/full split.
  - `audit.rs` — audit writer **whose type signature cannot accept
    raw bytes**. Type-level isolation prevents raw tail from bleeding
    into the audit log (Codex CSF-2 + Gemini G-8).
  - `snapshots.rs` — runtime checkpoint writer to
    `~/.qmonster/snapshots/`. Never writes `.mission/CURRENT_STATE.md`.
  - `retention.rs` — retention job (default 14 days, config-driven).

### `ui/`

Ratatui widgets. Current operator surfaces:

1. Severity-first alert queue with timestamps, `NEW` highlighting,
   per-alert auto-hide toggles, and severity bulk-hide chips.
2. Per-pane list with inline expansion for the selected pane's
   recommendations, provider-profile payload, metrics, and runtime
   facts (`modes`, `access`, `loaded`, `restrict`).
3. Overlays for target selection (session -> window), help/legend, and
   Git status from the bottom-right version badge.
4. Source labels rendered in long form (`[Official]`, `[Qmonster]`,
   `[Heur]`, `[Estimate]`) rather than two-letter abbreviations.

Palette: low-saturation, grey/navy/blue. Color only on state
transitions, always paired with a numeric % or severity letter.
UI consumes already-classified signals; it never re-parses tails.
Provider runtime facts are produced by adapter-local parsers from
provider status/slash output and readable provider config sources. The
TUI key `u` sends the selected provider's read-only runtime slash
commands with Enter. If the pane is active or only heuristically stale,
Qmonster uses only commands verified to run without waiting: Claude
`/status`, Codex `/status`, and Gemini `/stats session`, `/stats model`,
`/stats tools`. If Claude is explicitly idle, waiting, or limited,
Qmonster sends the fuller Claude set: `/status`, `/context`, `/config`,
`/stats`, `/usage`. The next poll parses the resulting official output.
Claude `/btw` is not used as a runtime fact source because it has no
tool or internal-state access. Unknown or unexposed fields stay absent
rather than inferred.

### `notify/`

Desktop notification (`notify-send` or equivalent) + optional terminal
bell. Severity-aware rate limiting so log storms do not spam.

## Cross-cutting rules

### Actuation policy (enforced at `app::EffectRunner`)

- `observe_only` — no outbound actions at all.
- `recommend_only` — **default**. Allowed auto actions: notifications,
  runtime-local archive writes under the resolved Qmonster root, and
  display-layer prompt-send proposals. Disallowed: unconfirmed prompt
  send, `/compact`, `/clear`, memory mutation, provider
  reconfiguration, any destructive mutation.
- `safe_auto` — accepted as a non-`observe_only` mode, but it does not
  create autonomous destructive behavior. Real prompt send still
  requires the operator `p` confirmation path plus
  `allow_auto_prompt_send = true`.

### Safety precedence (resolves the r1 contradiction)

Current runtime config loading is explicit: `--config PATH` or
defaults. Safer-only runtime overrides may be passed with `--set
KEY=VALUE`. Storage-root resolution has its own implemented
precedence: `QMONSTER_ROOT > --root > config.storage.root > default
(~/.qmonster/)`.

Asymmetry for these four flags — env/CLI may only move them TOWARD
safer behavior; any attempt to move them toward more permissive is
ignored and logged as a `risk`-severity audit event:

- `actions.mode` (safer: `observe_only` > `recommend_only` > `safe_auto`)
- `allow_auto_prompt_send` (safer: `false`)
- `allow_destructive_actions` (safer: `false`)
- `refresh.policy` (safer: `manual_only`)

Runtime code does NOT toggle these flags; they are set at startup by
config + safer-only env/CLI overrides.

### Data-shape rule

- Config is TOML (static, stable). The runtime-parsed subset is the one
  defined in `src/app/config.rs`; `config/qmonster.example.toml`
  documents only those live keys.
- Runtime state is in-memory structs + (Phase 2+) SQLite. UI must
  never treat runtime state as config or vice versa.

### Storage split

- `qmonster.db` = `<qmonster-root>/qmonster.db` (Phase 2+; audit
  metadata - indices + summaries). **Never contains raw tail bytes.**
- `archive_dir = <qmonster-root>/archive/` (Phase 2+; raw tails with
  preview/full split).
- `snapshot_dir = <qmonster-root>/snapshots/` (Phase 2+; runtime
  checkpoints before a user-requested compact). **Never overlaps with
  `.mission/CURRENT_STATE.md`.**
- `.mission/CURRENT_STATE.md` is a **day-end handoff document** written
  by the human day-end routine (`docs/ai/WORKFLOWS.md` §3). Qmonster
  runtime never writes it.

### Filesystem write boundary

Qmonster runtime writes only within the resolved Qmonster root
(default `~/.qmonster/`, or the configured `QMONSTER_ROOT` / `--root`
/ `storage.root`). Project-directory files (`CURRENT_STATE.md`,
`mission.yaml`, `mission-history.yaml`, `docs/ai/*`, provider config
files, source files) are read-only to Qmonster across Phases 1–5. Any
runtime write elsewhere is a bug.

### Abstraction boundaries (do not violate)

- `tmux/` knows nothing about providers, roles, or signals.
- `domain/` is pure, no IO.
- `adapters/` know nothing about SQLite, ratatui, or identity
  inference.
- `policy/` is pure; returns values only. No IO.
- `store/` knows nothing about ratatui; `ui/` knows nothing about
  storage shape.
- `audit.rs` cannot accept raw bytes — type-level enforcement, not
  comment-level.

## SourceKind taxonomy

Every metric, threshold, and recommendation carries a `SourceKind`:

| SourceKind         | Meaning                                                                                                               |
| ------------------ | --------------------------------------------------------------------------------------------------------------------- |
| `ProviderOfficial` | Cited to a provider/tool's own documentation (Anthropic / OpenAI / Google / tmux).                                    |
| `ProjectCanonical` | Qmonster project-local rule in `docs/ai/` or `config/qmonster.example.toml`.                                          |
| `Heuristic`        | Community-tool derived (RTK, Context Mode, Token Savior, Caveman, claude-token-efficient, token-optimizer-mcp, etc.). |
| `Estimated`        | Qmonster-derived inference or default chosen without external citation.                                               |

Promotion rule: nothing promotes to `ProviderOfficial` without a direct
provider-doc citation. Heuristic thresholds stay `Estimated` until
Phase-1 fixture measurement justifies re-labeling.

Display rule: every UI surface shows the `SourceKind` next to the
value. Color is never used alone.

## Token-optimization architecture (five layers)

1. **Provider-native profile** (Phase 4 implementation). Pick the right
   CLI flags/settings per `(provider, role, situation)` using
   `ProviderOfficial` levers (Claude/Codex/Gemini). Profile names are
   project-local (`ProjectCanonical`).
2. **Observation** (Phase 1). Polling pane tail, provider parsers,
   repeated-output / log-storm / verbose-answer / context-pressure /
   token / cost signal extraction, plus provider runtime facts for
   permission mode, auto/yolo mode, sandbox, allowed directories,
   loaded tools/skills/plugins, and restricted tools when exposed by
   provider status/config sources. **Phase 1 surfaces these as
   display-only metrics/facts (each with `SourceKind`), not as gating
   signals.**
3. **Archive + checkpoint** (Phase 2). Raw tails → archive;
   preview/full split on screen; runtime snapshots pre-compact →
   `<qmonster-root>/snapshots/`. Never `.mission/CURRENT_STATE.md`.
4. **Policy + recommendation** (Phase 1 = alerts only; Phase 3 =
   A–G situations; Phase 4 = profile recommendations). Aggressive mode
   gated by `quota_tight` flag.
5. **Limited actuation** (see "Actuation policy"). Destructive code
   paths are not created until the phase that owns them is approved.

## MVP reference code — warning

`.docs/init/qmonster_adaptive_token_optimizer_mvp.rs` is a **signal
catalog** (useful) and a **prototype-level reference** (NOT an
architecture template). In particular:

- its `collect_snapshots()` fills `provider`/`role` inside the
  tmux-facing code — violates "`tmux/` knows no providers".
- its `recommend()` mixes provider-specific profile logic into the
  rules — violates "`policy/` is pure".

Copy the markers and parse functions when needed. Do NOT mirror its
module shape.

## Deferred for later phases

- Control-mode tmux adapter.
- Manual prompt-send helper with user confirmation.
- Subagent token accounting (Phase 1 ships a detection-only warning).
- Cross-window / cross-project correlation.
- Anomaly detection on pane identity drift (Phase 1 logs transitions).
- Concurrent-work warning across panes (Phase 3A ships a project-level
  proxy at `same current_path`; file-level and git-branch-level
  detection deferred to Phase 3B / Phase 4+).
- Copy-pasteable command snippets with recommendations (Phase 3+).
- Side-effect warnings on high-compression profiles (Phase 4+).
