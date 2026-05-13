# Alert Related Context Rail Design

## 1. Goal

Show that separate Alerts are part of the same operating context without
collapsing them into a single top-level `FLOW` row.

The current `FLOW Context recovery` projection is intentionally narrow:
it only replaces source alerts when Qmonster has high confidence that
multiple alerts share one response path. That keeps the list safe, but
many related alerts still appear as isolated items. This design adds a
lighter relationship layer so operators can see continuity while keeping
each alert independently visible, sorted, copyable, and hideable.

## 2. Non-Goals

- Do not broaden `FLOW Context recovery` matching.
- Do not hide Risk/Warning alerts behind a weak relationship.
- Do not change recommendation policy generation or advisory severity.
- Do not persist related-group state in SQLite.
- Do not create a new navigation model or nested alert tree.

## 3. Relationship Model

Use two relationship strengths:

1. **Strong flow**: existing `AlertEntry::Flow`, used only when source
   alerts can be safely represented by one response path.
2. **Related context**: new UI-only metadata on individual alert rows,
   used when alerts share meaningful context but should stay separate.

Related context is computed after alert items are collected and before
rendering. It is a projection over the visible alert set, not a new
domain event.

## 4. Matching Rules

Only recommendation-class `AlertItem`s participate in the first slice.
System notices, cross-pane findings, and existing `FLOW` entries stay out
of related groups.

Two alerts are related when they share the same pane and at least one of
these connectors:

- Same `suggested_command`, such as `/compact`.
- Same broad action family, derived from the recommendation action key
  prefix before the first `:`.
- Same context-recovery category already used by flow matching.
- Same recovery path signal, such as snapshot-before-compact detail text.

A related group renders only when it has at least two visible items. The
existing strong flow suppression runs first; items consumed by a `FLOW`
do not also render as related top-level items.

## 5. Rendering

Default item rows remain unchanged unless the item has related siblings.
When related siblings exist, append compact context lines below the
normal `summary`/`next`/`run` block:

```text
related : 3 alerts on %56 · same /compact path
rail    : o context pressure | cache drift | snapshot before /compact
```

Rules:

- `related` reports total group size, pane id, and the strongest connector.
- `rail` shows up to four compact step labels in deterministic order.
- The current item's step uses marker `o`; sibling steps use `|`.
- If the row is not selected and vertical space is tight, `rail` may be
  omitted before truncating existing alert content.
- The line never changes hide/copy behavior. Enter/Space still hides only
  the selected alert item.

Selected related items append a short evidence block after existing
details:

```text
included: context-pressure: high [Heuristic]
included: cache: drift detected [Heuristic]
included: snapshot before /compact [Qmonster]
```

This mirrors selected `FLOW` evidence, but it does not create a separate
flow key.

## 6. Sorting And Lifecycle

Related context must not affect list sort order. Existing severity, NEW,
timestamp, kind priority, and key ordering continue to decide position.

Freshness stays per alert key. Hiding a related item hides only that item.
Bulk hide continues to count each related alert separately, because they
remain separate actionable rows.

`y` copies the selected alert's command. If multiple related alerts have
commands, the rail can mention that a shared command exists, but command
selection remains item-local.

## 7. Data Shape

Add a small render-facing metadata type near `AlertEntry`:

```rust
struct RelatedAlertContext {
    group_key: String,
    pane_id: String,
    connector_label: String,
    total: usize,
    steps: Vec<RelatedAlertStep>,
    evidence: Vec<RelatedAlertEvidence>,
}

struct RelatedAlertStep {
    marker: &'static str,
    label: String,
    is_current: bool,
}

struct RelatedAlertEvidence {
    action: String,
    source_label: String,
}
```

`AlertEntry::Item` can carry `Option<RelatedAlertContext>`, or the
renderer can keep a side map keyed by alert key. Prefer the smaller diff
that keeps public helper behavior clear and avoids duplicating item data.

## 8. UI Copy

Use plain operational labels:

- `related`
- `rail`
- `included`
- `same /compact path`
- `same pane recovery`
- `same action family`

Avoid saying "flow" for weak relationships. `FLOW` remains reserved for
the strong grouping row.

## 9. Tests

Add unit tests around the projection and renderer:

- Two visible same-pane recommendations with the same `/compact` command
  render related context while staying as two top-level items.
- Same connector on different panes does not relate.
- Items consumed by a `FLOW` do not also render related context.
- Related context does not change sort order.
- Hiding one related item does not hide siblings.
- Bulk hide counts related items individually.
- Selected related item renders evidence rows.
- Narrow height prefers existing alert content over optional `rail`.

## 10. Manual Check

Run Qmonster with a pane that emits context pressure plus cache/snapshot
recommendations that do not meet strong flow criteria. Confirm:

- Alerts remain individually visible.
- Each related item shows a concise `related` line.
- Selected item shows the sibling evidence.
- Enter/Space hides only the selected item.
- `y` copies the selected item command, not an inferred group command.

## 11. Risks

- Too much repeated rail text can make the list noisy. Mitigation: only
  show related context for groups of at least two and cap rail steps.
- Weak matching can imply a stronger causal relationship than exists.
  Mitigation: use connector labels such as `same action family`, not
  causal language.
- The alert renderer is already dense. Mitigation: keep related metadata
  render-facing and test line counts so narrow layouts degrade cleanly.
