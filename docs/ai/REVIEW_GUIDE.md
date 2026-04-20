# REVIEW_GUIDE

- Version: v0.4.0
- Date: 2026-04-20

This is the contract that every reviewer (Codex cross-check, Gemini
research/policy/safety, human sign-off) works to. It defines what is
a must-fix vs nice-to-have, how to label evidence, and what verdicts are
allowed.

## 1. Reviewer roles

### Claude (main)

Author + final synthesizer. NOT the cross-check reviewer of their own
plan.

### Codex (review)

Structural cross-check. Focus:

- Are the module boundaries actually implementable?
- Is the MVP scope too large?
- Does provider abstraction hold, or does provider-specific knowledge
  leak into `tmux/`, `policy/`, `ui/`, or `store/`?
- Is token optimization baked into the structure, or bolted on?
- Do `docs/ai/`, mission ledger, and memory roles overlap?
- Is actuation risk calibrated to `recommend_only`?
- Are `(official)` vs `(heuristic)` claims clean?

### Gemini (research)

Research / policy / safety / ops reinforcement. Focus:

- Doc hierarchy and handoff ergonomics.
- Safe placement of auto memory in each CLI.
- How token optimization integrates into ops, not just architecture.
- Approval / sandbox / policy / audit design.
- Manual-refresh policy and version-drift handling.
- Missing scenarios and edge cases.

### Human (operator)

Final approval gate on planning artifacts and any transition between
phases. Holds veto authority.

## 2. Severity taxonomy

Used by reviewers in their output docs and in the Qmonster audit log.

| Severity  | Meaning                                             |
| --------- | --------------------------------------------------- |
| `safe`    | Normal operating state.                             |
| `good`    | Positive observation (not required, but reinforce). |
| `concern` | Worth noting; does not block.                       |
| `warning` | Should be addressed before next phase.              |
| `risk`    | Blocks the phase gate until fixed.                  |

## 3. Must-fix categories

A reviewer MUST flag the following as at least `warning`:

1. Any canonical doc or router that exceeds its line budget.
2. Any recommendation, threshold, or profile lever that lacks an
   `(official)` or `(heuristic)`/`(estimated)` label.
3. Any proposal to auto-run a destructive action in `recommend_only`
   mode.
4. Any state duplication between `mission.yaml`, `CURRENT_STATE.md`,
   and auto memory.
5. Any implementation content leaking into a thin router.
6. Any phase-2+ feature smuggled into the phase-1 MVP scope.
7. Any missing abstraction boundary (tmux knows providers, adapters
   know SQLite, etc.).
8. Any approval / sandbox / audit gap relative to
   `mission.yaml` constraints.

## 4. Output format (Codex / Gemini rounds)

Each reviewer produces a markdown file under their pane's `.docs/<model>/`
directory using the filename template in `WORKFLOWS.md`. Required
sections:

- **Executive summary** (≤ 10 lines, human-readable).
- **What the plan got right** (receipts, not flattery).
- **Must-fix before next phase** (numbered, each with file + line +
  proposed change).
- **Scope-reduction suggestions** (concrete: what to defer, why).
- **Token-optimization architecture review.**
- **Doc / mission / memory separation review.**
- **Provider abstraction + tmux integration review.** (Codex emphasis)
- **Safety / audit / approval reinforcement.** (Gemini emphasis)
- **Official-vs-heuristic cleanup list.**
- **Concrete TODOs for Claude** (the next author).
- **Final verdict**: one of
  `approve` | `approve-with-fixes` | `rework`.

## 5. Promotion rule

- A claim survives in `docs/ai/` only after at least one cross-check
  round does not flag it as must-fix.
- A change to `docs/ai/` that widens scope or changes a constraint MUST
  be paired with a `mission-history.yaml` entry.
- MDRs belong in `.mission/decisions/`, not in canonical docs.

## 6. Evidence handling

- Raw logs stay in `~/.qmonster/archive/` (never inlined into the
  review doc).
- Numbers cited in a review carry `(official)` / `(heuristic)` /
  `(estimated)` labels.
- Screenshots / UI evidence go under `.docs/final/` for the round.

## 7. What reviewers should NOT do

- Do not rewrite canonical docs during a cross-check round. Propose
  changes; the author performs the promotion in `docs/ai/`.
- Do not silently drop an `(official)` lever because it "looks risky" —
  flag it, cite the provider doc, let the author decide.
- Do not move state from `mission.yaml` into memory, ever.

## 8. Verdict meanings

- `approve` — next phase gate may open; no fix required.
- `approve-with-fixes` — next phase gate opens once listed must-fix
  items are resolved and re-checked.
- `rework` — the plan needs another author round before cross-check
  can land.
