# Alerts Related Visual Polish Design

## Goal

Make the Alerts panel look less crowded when weakly related alerts are
visible, without changing alert production, sorting, hide semantics, or
copy semantics.

## Scope

This polish applies only to `related` context rows. `FLOW Context
recovery` keeps its existing grouping behavior and timeline rail.

## Behavior

- Unselected related alerts show one compact metadata line:
  `related : 2 on %1 · /compact path`.
- Selected related alerts expand the optional context:
  `rail` shows sibling steps, and `included` shows sibling action/source
  evidence.
- Related alerts remain independent top-level items. Severity sorting,
  bulk counts, hide behavior, and `y` copy behavior stay item-local.
- Connector matching remains unchanged; this is a rendering-only polish.

## Verification

- `cargo test --lib related_context`
- `cargo test --lib context_recovery_flow`
- `cargo fmt -- --check`
