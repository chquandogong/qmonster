# Alerts Selected Tree Detail Design

## Goal

Keep the Alerts panel as a dense priority queue while making the selected
alert read more like the sectioned tree layout used by pane cards.

## Decision

Do not convert Alerts to cards. Alerts are volatile queue items, so row
density matters more than framed card presentation. Instead, keep the
existing row list and render tree-shaped detail only for the selected
row.

## Behavior

- Non-selected alerts keep the existing compact row style.
- Selected recommendation rows render detail labels as a small tree:
  `├ summary`, `├ next`, `├ related`, `├ rail`, `├ included`,
  and `└ action` as applicable. The selected tree uses `action` for
  copyable commands instead of repeating both `run` and `copy`.
- Selected related rows keep the compact `related` summary, then show
  nested sibling evidence under the selected row.
- Selected `FLOW Context recovery` rows render their timeline as tree
  branches: cause/evidence/prep/included/action.
- Unselected `FLOW` rows stay compact and continue hiding included/copy
  detail.
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
- `cargo fmt -- --check`
