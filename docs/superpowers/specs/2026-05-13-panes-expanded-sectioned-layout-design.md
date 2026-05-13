# 2026-05-13 - Panes Expanded Sectioned Layout (design)

Status: design accepted, implementation plan pending.
Audience: Qmonster TUI maintainers and Claude/Codex/Gemini implementers.
Baseline: `main` at `877d29c` (post Alert Flow timeline rail merge).

This design reorganizes the Panes expanded state so the operator can
scan a selected pane without reading one long flat list. The user need
is not less information; it is clearer grouping when a pane is expanded.

## 1. Goal

Make the selected expanded pane in the Panes window read as a structured
operational summary.

The expanded pane should answer, in order:

1. What needs attention now?
2. Where is this pane and what is it running?
3. How much pressure is it under?
4. What runtime facts explain its behavior?
5. What recommendations are active?

The design is intentionally information-preserving. Existing fields
should remain visible when their source data exists, but they should be
grouped into predictable scan zones instead of one mixed sequence.

## 2. Product Decision

Use a **Sectioned Single Column** expanded pane layout.

Collapsed pane rows and selection behavior stay unchanged. Only the
selected expanded content is reorganized with lightweight section
headers:

```text
qmonster:0 - Codex review - CLI 0.122.0 [Official] - %57

NOW
state    : WAIT INPUT - 03:14
blocked  : waiting for input
signals  : subagent activity
proposal : /compact  -> press p to accept, d to reject

WHERE
path     : /home/chquan/Qmonster
cmd      : codex
status   : high confidence

PRESSURE
metrics  : CTX 86% [Official]  QUOTA 5H 47% [Official]
tokens   : TOKENS ▁▂▃▅▆█
token io : Main 1.51M in / 20.4K out [Official]
cache io : read 1.3M / create 22.1K [Official]

RUNTIME
modes    : ...
access   : ...
loaded   : ...
restrict : ...

RECOMMENDATIONS
CONCERN  : cache drift detected - /compact will let cache rebuild
detail   : ...
profile  : ...
```

Rationale:

- A single column preserves the current list navigation model.
- Section headers create visual anchors without adding a new overlay.
- The layout reduces noise without hiding audit details that operators
  already rely on.
- It can be implemented as a render-only transformation over the
  existing `PaneReport` data.

## 3. Non-goals

- Hiding existing pane facts in the expanded state.
- Adding a new pane overlay, tab model, or drilldown interaction.
- Changing provider adapters, signal collection, recommendations, or
  ranking logic.
- Changing collapsed pane rows.
- Changing Alerts, Alert Flow, or Pending Actions behavior.
- Recoloring the whole Panes window or introducing a new visual theme.

## 4. Current Behavior

`src/ui/panels/mod.rs` builds the selected expanded pane through
`pane_list_lines_with_flash(...)`. The current order is flat:

1. title
2. state rows
3. path
4. command
5. status
6. blocking signals
7. secondary signals
8. metrics
9. token and cache rows
10. runtime facts
11. proposal and recommendation details
12. separator

This preserves data, but it mixes identity, current action, pressure,
runtime facts, and recommendations in one continuous stream. The result
is hard to scan, especially when token rows, runtime badges, and
recommendation detail/profile rows are all present.

`panel_body_with_width(...)` has a similar flat body for pane panel
rendering. The same section order should be used there as well by
adapting the shared section rows into `ListItem`s. Recommendation counts
should remain unchanged per surface: the expanded list keeps its current
top 3, and the pane panel keeps its current top 6.

## 5. Section Model

### 5.1 NOW

Purpose: show the immediate operational state.

Rows:

- state rows from `render_pane_state_row_with_flash(...)`
- blocking signal rows such as `waiting for input` and `approval needed`
- secondary signal badges such as `log storm`, `verbose output`,
  `error hint`, and `subagent activity`
- prompt-send proposal, when present

Display rules:

- Show `NOW` only when at least one of these rows exists.
- Keep existing state flash and pulse behavior.
- Keep the proposal command text and accept/reject hint, but move it up
  into this section so it is not buried under metrics.

### 5.2 WHERE

Purpose: identify the pane and current process.

Rows:

- `path`
- `cmd`
- `status`

Display rules:

- Always show `WHERE` for expanded panes because these rows are always
  part of the current expanded body.
- Keep existing path ellipsizing and command wrapping behavior.

### 5.3 PRESSURE

Purpose: show resource and usage pressure in one place.

Rows:

- metric badges from `metric_badge_lines(...)`
- token sparkline status row when supported
- token IO row when available
- cache IO row when available

Display rules:

- Show `PRESSURE` when at least one row exists.
- Keep provider-specific token support checks.
- Keep token rows above recommendation details, preserving the current
  intent that pressure is not pushed off-screen by long recommendations.

### 5.4 RUNTIME

Purpose: collect runtime facts that explain the pane environment.

Rows:

- wrapped runtime badge lines from `runtime_badge_lines_wrapped(...)`

Display rules:

- Show `RUNTIME` only when runtime facts exist.
- Do not compress runtime facts in this slice. The accepted variant is
  information-preserving, so loaded/profile/source facts remain visible.

### 5.5 RECOMMENDATIONS

Purpose: keep active recommendations and audit details together.

Rows:

- up to the same number of recommendations currently shown in the
  expanded list
- recommendation detail lines
- provider profile lines
- `status: no active recommendations` when empty

Display rules:

- Show `RECOMMENDATIONS` in expanded panes even when empty, because the
  empty state tells the operator that there is no hidden action queue.
- Preserve severity labels, detail formatting, profile formatting, and
  recommendation ordering.

## 6. Rendering Contract

Section headers should be plain high-contrast text with subdued styling,
not boxed cards. The Panes list should remain a list, not cards inside
cards.

The row labels below each section should keep the existing aligned-field
format so hover help, wrapping, and muscle memory remain familiar.

Headers should not become new interactive rows. If hover help maps over
them, it should either explain the section in one short sentence or fall
back to the existing pane help topic. Existing row-level help topics
should continue to point at the underlying row meaning.

The separator between panes remains outside the sections.

## 7. Data Flow

No new domain data is required.

The renderer should continue to consume `PaneReport` and its existing
signals, recommendations, runtime facts, and recent token samples.

A small internal render helper can assemble section bodies before
flattening them back into `Vec<Line<'static>>` or `Vec<ListItem<'static>>`.
That keeps the change local to UI rendering while making the section
order explicit and testable.

## 8. Error Handling And Empty Data

Missing provider data should behave as it does today: omit rows rather
than inventing values.

Empty optional sections should be skipped, except for
`RECOMMENDATIONS`, which should retain the existing
`no active recommendations` empty state.

Narrow terminals should keep using existing wrapping and ellipsizing
helpers. Section headers must not introduce overflow or layout shifts.

## 9. Testing And Documentation

Implementation should include focused coverage for:

- expanded pane section order
- no loss of existing row categories when data exists
- empty optional section suppression
- the `RECOMMENDATIONS` empty state
- wrapped rows under narrow widths
- hover-help topic mapping for sectioned rows

`docs/ai/UI_MANUAL.md` should be updated after implementation to
describe the new section order under "Panes 읽는 법".

## 10. Success Criteria

- The selected expanded pane is grouped into `NOW`, `WHERE`,
  `PRESSURE`, `RUNTIME`, and `RECOMMENDATIONS`.
- Existing expanded pane facts remain visible when present.
- The default collapsed Panes list remains unchanged.
- The implementation is local to Panes rendering and help/docs updates.
- The layout remains readable on narrow terminal widths.
