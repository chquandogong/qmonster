# MDR — Retire the Gemini cross-review leg; cross-review is now Codex + human

**Status:** ACCEPTED — the decision is live in `docs/ai/WORKFLOWS.md §1/§6` and `docs/ai/REVIEW_GUIDE.md §1` as of v2.5.0. Recorded here for the decision trail + reinstatement criteria.

**Authored:** 2026-06-29 (v2.5.0 structured data-source migration cross-review gate)

**Author:** Claude (Opus 4.8) — confirmed by operator (human sign-off)

---

## Context

The project's documented cross-review contract was a two-reviewer round: **Codex r1** (structural cross-check) + **Gemini r1** (research / policy / safety), each producing a `.docs/<model>/` narrative + a `.mission/evals/` mirror (WORKFLOWS §1–2, REVIEW_GUIDE §1).

During the v2.5.0 release gate (2026-06-29) the Gemini leg could not run:

- `gemini -p` (gemini-cli 0.43.0-preview.1, individual tier) returns
  `IneligibleTierError: This client is no longer supported for Gemini Code Assist for individuals. To continue using Gemini, please migrate to the Antigravity suite.`
- This is Google's **2026-06-18 individual-tier Gemini CLI sunset** — the same event Qmonster v2.4.0 was built around (agy identification).
- `agy` (the Antigravity replacement) is an **interactive coding agent, not a diff reviewer**, so it cannot substitute for the Gemini r1 leg.

The v2.5.0 bundle therefore carried **Codex `approve-with-fixes` + operator human sign-off** as its gate, with the Gemini mirror recording `verdict: not_run` and the `IneligibleTier` blocked-reason.

## Decision

**Retire the Gemini r1 cross-review leg.** The standing cross-review contract is now **Codex r1 (structural) + human sign-off.** WORKFLOWS §1 step 3 is annotated RETIRED, WORKFLOWS §6 carries the amendment, and REVIEW_GUIDE §1 marks the Gemini role RETIRED. `codex --version` / `claude --version` / `tmux -V` version-drift checks continue; `gemini --version` drift is no longer gated.

## Consequences

- **Less automated reviewer diversity** — one automated structural reviewer (Codex) instead of two. Mitigated by: (a) the internal multi-agent review the controller already runs (per-task + an `opus` whole-branch pass per slice), and (b) the human sign-off gate, which is now load-bearing rather than a final rubber-stamp.
- **No safety/policy-specialized automated pass** — Gemini's emphasis (safety / approval / ops / missing scenarios) now falls to Codex's general review + the human. For ObserveOnly / no-actuation changes this is low-risk; for any future actuation-surface change the human reviewer should explicitly cover the safety dimension.
- Documentation is internally consistent (no stale "run Gemini" instruction left active).

## Reinstatement criteria (re-open triggers)

Re-add a second automated reviewer leg (and revert the WORKFLOWS/REVIEW_GUIDE annotations) when **either**:

1. **A non-individual-tier Gemini path is configured** — a Gemini Code Assist **Enterprise/Standard** license, or a **Gemini API key** driving a `gemini`/API review invocation that does not hit `IneligibleTierError`. Then restore the Gemini r1 leg verbatim.
2. **A substitute second-model reviewer is adopted** — e.g. a second high-capability model pass (a different model/provider than the Codex leg) wired into the same brief + artifact-split flow. Then rename the leg accordingly in WORKFLOWS/REVIEW_GUIDE and keep the two-reviewer contract.

Until then, **Codex + human is the contract.** This MDR is the authority for _why_ the second automated leg is absent.
