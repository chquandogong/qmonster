# Alerts Stable Selection Detail Design

## Goal

Keep the Alerts panel as a dense priority queue while making selected rows
show extra structure without rewriting the base row the cursor landed on.

## Decision

Do not convert Alerts to cards. Alerts are volatile queue items, so row
density matters more than framed card presentation. Selection should be
additive: render the same base lines for selected and non-selected rows,
then append a small tree-shaped detail block only where extra context is
available.

## Behavior

- Non-selected alerts keep the existing compact row style.
- Selected recommendation rows keep the same `summary`, detail, `run`,
  and compact `related` summary lines as non-selected rows.
- Selected related rows append a small tree block for `rail`, nested
  sibling `included` evidence, and optional `action`.
- Selected `FLOW Context recovery` rows keep the same summary and timeline
  rail (`o` / `|`) as non-selected rows, then append `included` evidence
  and optional `action`.
- Selection changes vertical expansion only; it does not relabel existing
  base lines.
- No behavior changes to projection, sorting, hide, bulk counts, filters,
  or copy metadata.

## Non-Goals

- No card/chrome frame per alert.
- No new navigation model.
- No alert grouping changes.
- No persistence, audit, or policy changes.

## Verification

- `cargo test --lib related_context`
- `cargo test --lib context_recovery_flow`
- `cargo test --lib`
- `cargo fmt -- --check`
