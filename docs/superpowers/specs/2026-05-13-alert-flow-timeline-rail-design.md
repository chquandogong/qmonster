# 2026-05-13 - Alert Flow Timeline Rail (design)

Status: design accepted, implementation plan pending.
Audience: Qmonster TUI maintainers and Claude/Codex/Gemini implementers.
Baseline: `main` at `6238a4b` (post-v2.2.0 dependency-bump cleanup).

This design adds an alert grouping layer for related recommendation
messages. The user need is not "more alerts"; it is making it obvious
when several alert rows are one continuing operational story.

## 1. Goal

Make related Alerts read as a single work flow when they share a topic
and a response path.

The first supported family is **Context Recovery**:

1. Context pressure is rising.
2. Cache drift or cache discontinuity explains why `/compact` may help.
3. Snapshot is the safe precondition.
4. `/compact` is the executable next action when available.

The rendered alert should answer:

- What is the current problem?
- What signal led to it?
- What should the operator do first?
- What command, if any, can be copied now?
- Which original alerts are included as evidence?

## 2. Product Decision

Use a **Timeline Rail** alert flow.

Default Alerts view shows one representative flow row instead of
scattering all included context/cache/snapshot alerts as independent
top-level rows.

```text
[14:23:08] WARNING  FLOW  Context recovery · 3 alerts · active
dismiss : [ ] click hide flow
summary : context pressure led to cache drift; snapshot before compact
  o CTX 86% crossed threshold
  | cache hit dropped 72% -> 38%
  o snapshot before reset
  | then run /compact
```

When selected or expanded, the flow shows the included source alerts:

```text
included:
  - context-pressure: checkpoint · %1 · [Estimate]
  - cache: drift detected - /compact will let cache rebuild · %1 · [Heur]
  - snapshot before 5h window resets · %1 · [Official]
```

Rationale:

- The timeline rail makes cause and next action visible without adding a
  heavy new overlay.
- The default list stays quiet: one flow row replaces several related
  top-level rows.
- Source alerts remain available in the selected/expanded view, so the
  UI does not hide evidence or make the operator trust a black box.

## 3. Non-goals

- Generic graph clustering for every alert type.
- New policy rules, provider adapters, persistence tables, or audit
  schema.
- New destructive or automatic actions.
- Changing recommendation production order or severity thresholds.
- Hiding unrelated Risk/Warning alerts behind a flow.
- Replacing the Pending Actions overlay.

## 4. Current Behavior

`src/ui/alerts.rs` collects system notices, strong recommendations,
cross-pane findings, and normal recommendations into `AlertItem`s, then
sorts by severity, freshness, timestamp, kind priority, and key.

Each row already has:

- title
- dismiss line
- summary
- optional details such as `next` and `run`
- `copy` line for selected alerts with a runnable `suggested_command`
- `anchor` / `others` for cross-pane findings

This is good for priority, but weak for continuity. Related
recommendations can appear as separate rows even when the operator
should read them as one response path.

## 5. Flow Family

### 5.1 Context Recovery

The initial flow family groups recommendation-class alerts on the same
pane when at least two of these categories are present. Matching is by
classifier, not by rendering text, because some current action strings
use Unicode punctuation.

| Category | Classifier |
| --- | --- |
| Context pressure | `action.starts_with("context-pressure:")` |
| Cache compact guidance | `action.starts_with("cache:")` and `action` or `reason` mentions `/compact` |
| Cache anomaly | `action == "anomaly: cache discontinuity detected"` |
| Snapshot precondition | `action.starts_with("snapshot before ")` |

The display name is `Context recovery`.

The first implementation should not group quota-only, cost-only,
identity-drift, profile-switch, or cross-pane findings into this flow.
Those can get their own families later if the UI pattern proves useful.

### 5.2 Action Normalization

Some action strings contain Unicode punctuation in the current codebase,
for example the cache drift action uses an em dash. The classifier
should avoid fragile exact-string matching where a prefix is enough:

- `action.starts_with("context-pressure:")`
- `action.starts_with("cache:")` plus `/compact` in action or reason
- `action == "anomaly: cache discontinuity detected"`
- `action.starts_with("snapshot before ")`

The flow renderer can still display the original action string verbatim
in the included-alerts section.

## 6. Alert Model

Keep policy data unchanged. Build flows as a UI-level projection after
`AlertItem` collection and before rendering/sorting.

Introduce an internal render model similar to:

```rust
enum AlertEntry {
    Item(AlertItem),
    Flow(AlertFlow),
}

struct AlertFlow {
    key: String,
    family: AlertFlowFamily,
    pane_id: String,
    timestamp: String,
    timestamp_sort_key: u32,
    severity: Severity,
    source_kind: SourceKind,
    is_new: bool,
    hide_deadline: Option<Instant>,
    summary: String,
    steps: Vec<AlertFlowStep>,
    included: Vec<AlertItem>,
    suggested_command: Option<String>,
    command_identity: Option<(String, String)>, // (pane_id, action) for lifecycle ledger
}

struct AlertFlowStep {
    state: AlertFlowStepState,
    label: String,
    detail: String,
    command: Option<String>,
}
```

`AlertItem` can remain private to `ui::alerts`; this is an internal
rendering refactor, not a domain contract.

## 7. Grouping Rules

1. Collect normal alert items exactly as today.
2. Partition recommendation-class items by pane id.
3. For each pane, classify Context Recovery candidates.
4. If a pane has at least two Context Recovery categories, build one
   flow.
5. Remove included items from the top-level list.
6. Insert the flow into the same list and sort it with existing priority
   rules.
7. Leave non-included alerts unchanged.

The flow key should be stable across polling:

```text
flow|context-recovery|{pane_id}|{included_action_keys_joined}
```

The `included_action_keys_joined` component should be deterministic:
sort included recommendation keys before joining.

## 8. Severity, Source, and Freshness

- `severity`: maximum severity among included alerts.
- `source_kind`: highest-authority included source using the explicit
  order `ProviderOfficial > ProjectCanonical > Heuristic > Estimated`.
- `timestamp`: latest included alert timestamp.
- `is_new`: true when any included alert is new.
- `hide_deadline`: pending only when the flow key itself is pending.

Do not infer that hiding one included alert hides the flow. Flow hide
uses the flow key. When a flow key is pending or expired in the hide
map, the included source alerts stay suppressed for that same flow
composition; if the composition changes and a different flow key is
created, the new flow can appear as a fresh alert.

## 9. Timeline Steps

Use concise, deterministic steps:

| Step | Source | Label |
| --- | --- | --- |
| Context | strongest context-pressure item | `CTX {pct}% crossed threshold` when a percent is present, otherwise `context pressure detected` |
| Cache | cache guidance or cache anomaly | `cache drift/discontinuity detected` or the cache recommendation reason |
| Snapshot | snapshot recommendation or `next_step` mentioning snapshot | `snapshot before reset` |
| Command | first runnable `/compact` command in included alerts | `then run /compact` |

If data needed for a richer label is absent, prefer a plain label over
inventing numbers.

## 10. Rendering

Default flow row:

```text
[time] NEW WARNING FLOW Context recovery · N alerts · active ★y
dismiss : [ ] click hide flow
summary : context pressure led to cache drift; snapshot before compact
  o context pressure detected
  | cache drift detected
  o snapshot before reset
  | then run /compact
```

Selected or expanded flow appends:

```text
included: context-pressure: checkpoint · %1 · [Estimate]
          cache: drift detected - /compact will let cache rebuild · %1 · [Heur]
          snapshot before 5h window resets · %1 · [Official]
copy    : `/compact`  -> press y to copy
```

Use ASCII rail glyphs (`o`, `|`) unless the existing terminal theme
already proves Unicode line glyphs render reliably in this code path.
The mockup used visual dots, but the implementation should favor
terminal compatibility.

## 11. Interactions

### 11.1 Selection

The alert list cursor treats a flow as one row item. Up/down navigation
does not step through individual included alerts in the default view.

### 11.2 Expansion

Initial implementation can use "selected means expanded", matching the
current selected-alert copy detail pattern. A separate explicit expand
key can be added later if the selected view becomes too tall.

### 11.3 Copy

`y` on a selected flow copies `AlertFlow.suggested_command` when present.
For Context Recovery, prefer `/compact` over other commands. If no
runnable command exists, the existing no-copy behavior applies.
Lifecycle attribution should use `AlertFlow.command_identity`, pointing
at the included recommendation that supplied the copied command.

### 11.4 Dismiss

Enter/Space on a flow toggles hide for the flow key. It should not write
per-included hide keys in the first slice. This keeps undo simple and
avoids surprising the operator by hiding evidence rows that may later
leave the flow.

### 11.5 Bulk Hide

Bulk hide counts flows as one actionable alert at the flow severity.
Included source alerts are not counted separately while represented by
the flow.

### 11.6 Filter

Alert filter should match:

- flow title
- flow summary
- step labels/details
- included alert titles/headlines/details
- flow suggested command

If a filter matches only an included source alert, show the flow rather
than re-splitting the included item into the top-level list.

### 11.7 Hover Help

Add help text for `FLOW` rows:

- A flow groups related alerts that share one response path.
- The rail shows signal -> follow-on evidence -> recommended next step.
- The included section lists the original alerts used as evidence.
- `y` copies the current runnable command when present.

## 12. Architecture

Keep the change concentrated in `src/ui/alerts.rs`:

1. Preserve `collect_items` as the source of raw alerts.
2. Add a second projection function, for example
   `collect_entries(...) -> Vec<AlertEntry>`.
3. Update render, hit testing, selected-command lookup, bulk hide, copy
   count, filter, and pending-actions metadata to use entries.
4. Keep public helper behavior equivalent for non-flow alerts.

Avoid touching policy rules in this slice. The UI can read existing
recommendation action/reason/next/run fields.

## 13. Tests

Add unit tests in `src/ui/alerts.rs`:

- context/cache recommendations on the same pane become one flow.
- one matching item alone does not become a flow.
- different panes do not merge.
- flow severity is the max included severity.
- flow sort order follows existing severity/newness/timestamp rules.
- included alerts are suppressed from the top-level list while the flow
  is visible.
- selected flow with `/compact` returns `/compact` from selected command
  lookup.
- filter matches included alert text and keeps the flow visible.
- bulk hide counts the flow once.
- pending actions sees one copyable alert for the flow.
- non-flow recommendations keep existing rendering.

Manual smoke:

- Run the TUI with a fixture or live pane that triggers context pressure
  plus cache drift.
- Confirm the alert queue shows one `FLOW Context recovery` row.
- Select the flow and confirm included source alerts appear.
- Press `y` and confirm `/compact` copy behavior still goes through the
  Action Explainer / ledger path used by normal alerts.

## 14. Documentation

Update `docs/ai/UI_MANUAL.md` Alerts section with:

- What a `FLOW` row means.
- How to read the rail.
- Why included alerts are not duplicated as top-level rows.
- Copy/dismiss behavior for flows.

Update hover help glossary in Korean and English.

## 15. Risks

- Over-grouping could hide an unrelated alert. Mitigation: initial family
  is narrow and same-pane only.
- Dismiss semantics could surprise users. Mitigation: hide the flow key
  only; do not mutate included source hide keys in the first slice.
- The selected flow can become tall. Mitigation: keep included rows
  compact and defer explicit collapse controls until needed.
- Action-string matching can drift. Mitigation: use prefixes and focused
  tests around the current context/cache/snapshot actions.

## 16. Deferred Follow-ups

- Additional flow families such as Cost Control, Quota Reset, and
  Cross-Pane Conflict.
- Explicit expand/collapse key.
- Per-step selection inside the rail.
- Persisting flow lifecycle analytics in SQLite.
- Configurable grouping on/off switch.
