# WORKFLOWS

- Version: v0.4.0
- Date: 2026-04-20 (round r2 reconciled)

## 1. Planning loop (current round)

The round is `r<N>`; artefacts are named per the template in
`config/qmonster.example.toml`:
`{project}-{version}-{date}-{provider}-{kind}-r{round}.md`.

1. **Claude r1 — plan**  
   Claude (main pane) reads reference material in `.docs/init/` and
   produces `.docs/claude/Qmonster-v0.4.0-2026-04-20-claude-plan-r1.md`.
2. **Codex r1 — cross-check**  
   Codex pane (`codex:1:review`) reviews for structure, implementation
   risk, provider abstraction, scope creep. Output:
   `.docs/codex/Qmonster-v0.4.0-2026-04-20-codex-crosscheck-r1.md`.
3. **Gemini r1 — research / policy / safety**  
   Gemini pane (`gemini:1:research`) reinforces research, operations,
   auto-memory placement, audit / approval, missing scenarios. Output:
   `.docs/gemini/Qmonster-v0.4.0-2026-04-20-gemini-research-r1.md`.
4. **Claude r2 — final synthesis**  
   Claude classifies every reviewer item (now / Phase 1 / Phase 2+ /
   rejected), applies approved edits to canonical docs + mission +
   ledger, and publishes
   `.docs/final/Qmonster-v0.4.0-2026-04-20-claude-final-r2.md`.
5. **Implementation** begins only after r2 lands and the human approves
   the "begin Phase 1" gate in `mission.yaml`.

Canonical prompts for steps 2 and 3 live in
`.docs/init/Qmonster_ms_init_prompt_ko_v0.4.0_2026-04-20.txt`.

## 2. Handoff routine (between panes / sessions / models)

- **Same thread, same pane, different day** → Claude `/resume` or
  `claude --continue`. Resume restores full message history + tool
  state; it is NOT a memory feature.
- **Across panes or across models** → shared baseline is
  `mission.yaml`, `mission-history.yaml`, and the relevant `docs/ai/`
  files. If `.mission/CURRENT_STATE.md` exists locally, treat it as an
  optional local handoff note. `ms-context` can bundle the shared
  contract first and local notes second.
- **To a human reviewer** → point them at the round's `.docs/final/`
  entry and the relevant `docs/ai/` canonical docs.

## 3. Day-end routine

In this order, every day, **by a human** — Qmonster runtime never
writes any of these files:

1. Update `.mission/CURRENT_STATE.md` if you use a local handoff
   document. If a runtime snapshot under `~/.qmonster/snapshots/` is
   relevant, the human references or excerpts it; Qmonster does not
   auto-inline. This file is local-only and not part of shared clone
   acceptance.
2. If scope / constraints / done_when changed, update `mission.yaml`
   AND append a `mission-history.yaml` timeline entry.
3. Record any material decision as an MDR under `.mission/decisions/`.
4. If a human or LLM signed off on a verdict, record it under
   `.mission/evals/` as a structured mirror; the narrative review doc
   stays under `.docs/<model>/`.
5. Promote anything from `.docs/` that has stabilized into either
   `docs/ai/` (shared rule) or skills/workflows (reusable procedure).
6. **Optionally** save a stable local pattern to auto memory — never
   the day's state, never branch-specific handoff, never the only copy
   of an approval.

## 4. Day-start routine

1. `claude --continue` or `claude --resume` if you want to pick up a
   specific thread.
2. For a fresh shared clone: read `mission.yaml`,
   `mission-history.yaml`, then the relevant `docs/ai/` files.
3. If `.mission/CURRENT_STATE.md` exists locally, read it after the
   shared contract. Only then open `.docs/*` for round-specific
   context.

## 5. Cross-check and version-drift cadence

Re-run the planning loop when any of the following happen:

- A plan round lands with material scope changes (claude → codex →
  gemini → claude final).
- A new risk is surfaced in `CURRENT_STATE.md` under "Open questions".
- **A provider CLI, model, or config surface ships a change that might
  invalidate a `[ProviderOfficial]` label.** On a manual version-check
  refresh, Qmonster captures `claude --version` / `codex --version` /
  `gemini --version` / `tmux -V` and compares with the previous
  snapshot. On change, a `warning`-severity alert fires with:
  _"version drift detected — re-verify `[ProviderOfficial]` tags in
  `docs/ai/` and profile lever citations"_. The alert is informational;
  no auto-update happens (`refresh.policy = manual_only`).

## 6. `/compact`, `/clear`, `/memory`, cache — operating rules

- **`/compact`** — never automatic. Before `/compact`:
  1. Qmonster **offers** (does not force) a snapshot of large
     pane-local results into `~/.qmonster/snapshots/`.
  2. The operator captures open questions + next first action in
     their own notes or (if they choose) in `.mission/CURRENT_STATE.md`
     as a manual day-end action.
  3. Only then run `/compact`.
     After `/compact`: re-verify what MUST be retained.
     **`.mission/CURRENT_STATE.md` is never written by Qmonster runtime.**
     It is a day-end handoff document, not a runtime checkpoint sink.
- **`/clear`** — last-resort, manual only. Snapshot + archive first.
- **`/memory`** — not used for day-end state. Only for stable local
  patterns. Summaries and ledgers go to `.mission/` or `docs/ai/`.
- **Cache** — design for stable prefixes (thin routers, stable
  canonical docs) and put dynamic info late in the prompt. Qmonster
  surfaces cache-friendly structure as guidance `[ProjectCanonical]`;
  it does not toggle provider cache settings.

## 7. local-first with shared repo ledger

Tracked in shared repo:

```
mission.yaml
mission-history.yaml
docs/ai/*
.mission/evals/
```

Ignored locally:

```
/.docs/
/.mission/CURRENT_STATE.md
/.mission/snapshots/
/.mission/templates/
/.mission-spec/
CLAUDE.local.md
/.claude/
/logs/
/*.sqlite
```

The repository is already using this split. Local workflow files remain
useful, but shared verification must succeed without them.

## 8. Reference — round filename template

```
{project}-{version}-{date}-{provider}-{kind}-r{round}.md
```

Examples:

- `Qmonster-v0.4.0-2026-04-20-claude-plan-r1.md`
- `Qmonster-v0.4.0-2026-04-20-codex-crosscheck-r1.md`
- `Qmonster-v0.4.0-2026-04-20-gemini-research-r1.md`
- `Qmonster-v0.4.0-2026-04-20-claude-final-r2.md`
