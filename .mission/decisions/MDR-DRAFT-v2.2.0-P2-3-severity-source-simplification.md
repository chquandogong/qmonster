# MDR DRAFT — P2-3: Severity / SourceKind label simplification

**Status:** DRAFT — operator-decision-pending. Do not promote until user explicitly approves a label-contract change.

**Authored:** 2026-05-11 (post v2.2.0 P1-3 implementation)

**Author:** Claude (post-evaluation improvement pass)

---

## Context

The v2.2.0 critical evaluation surfaced that the Severity × SourceKind label grid (5 × 4 = 20 cells) has higher cognitive cost than its discriminative value to a single operator. The current taxonomy:

- **Severity (5 levels):** Safe / Good / Concern / Warning / Risk
- **SourceKind (4 categories):** ProviderOfficial / ProjectCanonical / Heuristic / Estimated

Concrete frictions observed:

1. **Safe vs Good vs Concern:** Three "everything is fine or near-fine" levels operators routinely treat as identical. The `severity_letter` mapping `S / G / C / W / R` produces alert prefixes like `[G] [ProjectCanonical] %1 — cache: /compact is safe...` where the operator parses neither letter as discriminative.
2. **ProjectCanonical vs Heuristic ambiguity:** Both surface as Qmonster-derived but with different authority levels. Operators routinely conflate them because the visible label is just one word — no inline explanation.
3. **20-cell mental model:** Every render path (alerts, advisories, anomaly evidence, audit events, --once output) carries both labels. The combinatorial explosion in test fixtures alone is real friction (1491 lib tests of which ~200 lock specific severity/source combinations).

## Proposed simplification (sketch)

### Severity → 3 levels

| Old     | New                        | Rationale                                          |
| ------- | -------------------------- | -------------------------------------------------- |
| Safe    | (drop — implicit baseline) | Not surfacing "everything safe" reduces noise      |
| Good    | Info                       | Positive observations and successful checkpoints   |
| Concern | Info                       | Merge with Good — same operator response (nothing) |
| Warning | Warn                       | Unchanged meaning, shortened label                 |
| Risk    | Risk                       | Unchanged                                          |

Net: 5 → 3. `Notify` effect already gates on `≥ Warning`, so the merge of Safe/Good/Concern into Info changes no actuation behavior.

### SourceKind → 2 categories

| Old              | New      | Rationale                                           |
| ---------------- | -------- | --------------------------------------------------- |
| ProviderOfficial | Provider | Single category for "the CLI told us"               |
| ProjectCanonical | Qmonster | Qmonster derived from docs/ai rules + provider data |
| Heuristic        | Qmonster | Qmonster derived from tail pattern matching         |
| Estimated        | Qmonster | Qmonster derived from threshold comparison          |

Net: 4 → 2. Honesty preserved: "this number came from the provider" vs "Qmonster computed this". Sub-distinctions (heuristic vs estimated vs canonical) move from a label to a tooltip / hover help target.

## Cost of the change

This is a **contract change** affecting every layer:

| Layer                                     | Touch points                               | Estimated LOC                     | Risk                                 |
| ----------------------------------------- | ------------------------------------------ | --------------------------------- | ------------------------------------ |
| `domain::recommendation::Severity`        | enum + Ord + labels + tests                | ~150                              | high — every test that pins Severity |
| `domain::origin::SourceKind`              | enum + labels + Display                    | ~80                               | medium                               |
| `domain::audit::AuditEventKind` summaries | severity-bearing prose                     | ~40                               | low (mechanical)                     |
| `policy::*` rule outputs                  | every rule's severity assignment           | ~300 across ~14 files             | medium                               |
| `ui::*` rendering                         | severity color, badge, summary formatting  | ~500 across alerts/panels/metrics | high — visual regression risk        |
| `store::sqlite::*` schema                 | `severity` column already free-form string | 0                                 | low                                  |
| `app::event_loop` notify gate             | `severity >= Warning`                      | ~5                                | trivial                              |
| `tests/*` integration                     | many fixtures pin exact letters/labels     | ~400                              | high — mechanical sweep              |

Total: ~1500 LOC + 1491 lib tests + ~63 integration tests likely to need re-pinning.

**This is a 1–2 week commit.** Not a fit for an autonomous "while you're at it" pass.

## Why this needs operator approval

1. **Audit log retention.** `audit_events.severity` and `audit_events.source_kind` columns store the old strings. Existing rows would need a migration path (probably leave old labels intact and accept a v2.3.0 schema transition rather than a destructive ALTER).
2. **External consumers.** `--once` output may be piped to scripts; the new short labels could break downstream parsing. Needs a deprecation period or compatibility flag.
3. **Visual identity.** Operators have muscle memory for the current 5-letter S/G/C/W/R sidebar. Even if cognitively redundant, removing it is a UX disruption that should be explicit.
4. **Reversibility.** Severity is an ordered enum; collapsing levels and trying to recover them later requires re-classifying every historical rule by hand.

## Decision required

Before promoting this draft to an active MDR, the operator must answer:

1. **Do I find the current 5×4 grid friction-laden in practice?** If no — close this MDR as "no change".
2. **Am I willing to accept a v2.3.0 contract break + audit-log dual-label period?** If no — close this MDR as "deferred indefinitely".
3. **Do I want to keep the Safe/Good distinction for any specific surface?** (e.g., dashboard color tinting.) If yes — the proposal needs revision.

## Status

DRAFT — awaiting operator engagement with v2.2.0 + telemetry from P1-3 to determine if real cognitive cost matches the eval claim.

## Cross-reference

- v2.2.0 critical evaluation, §3 "표출" subsection: "SourceKind 4종을 운영자가 직관 구별 어려움" / "Severity letter S/G/C/W/R + word 'Warning' 양쪽 표기"
- Codex `improvement_plan.md` (2026-05-08) does not raise this point — it focused on identity/attribution and ROI measurement, which are now addressed in v2.0.0 + v2.2.0.
- This is the only P-tier item from v2.2.0 that is a label-contract change rather than additive instrumentation.
