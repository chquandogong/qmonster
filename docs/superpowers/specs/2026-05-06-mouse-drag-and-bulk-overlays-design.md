# 2026-05-06 — m drag + a overlay live explainer & bulk clear (design)

Status: design accepted, plan completed (shipped in v1.40.0; reference plan: `docs/superpowers/plans/2026-05-06-mouse-drag-and-bulk-overlays.md`).
Audience: Qmonster TUI maintainers and the Claude/Codex/Gemini implementers.
Baseline: `main` at `9cc5bfe` (post-v1.39 polish, untagged at design time; v1.40.0 tag commit is `5b0e856`).

## 1. Goal

Two operator-visible improvements layered on top of the v1.39 overlay
surfaces. No provider adapter, policy gate, or `SignalSet` schema change.
No new audit event types.

1. **m overlay (Metrics)** — operator can drag the modal by its top
   border (title row) to reposition. Existing `[`/`]`/`=` resize keys
   retained; `=` becomes a combined size + position reset.
2. **a overlay (Pending Actions)** — modal grows a left/right split
   so the right pane shows a live Action Explainer for the cursor
   item, the list gains multi-select with bulk clear, and the per-key
   `p`/`d`/`y` dispatch happens inline. The separate Action Explainer
   modal is no longer opened from inside the a overlay; its content is
   mirrored in the right pane.

## 2. Non-goals

- Persisting m overlay position to `qmonster.toml` (matches existing
  size persistence: in-session only).
- Resizing the a overlay or making it draggable (m still owns drag).
- New audit event types. Bulk dispatch reuses single-item paths.
- Changing dashboard `p`/`d`/`y` direct-key behavior, including
  `[ux] confirm_actions` semantics outside the a overlay.
- Auto-cycling the cursor across items (no inertia / momentum scroll).
- Persisting multi-select across overlay close / reopen.

## 3. Architecture overview

| Layer                          | File                                 | Change                                                                                                                                                                                                                 |
| ------------------------------ | ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Metrics overlay state          | `src/ui/metrics.rs`                  | Add `offset_x: i16`, `offset_y: i16`, `drag_anchor: Option<DragAnchor>` to `MetricsOverlay`. Extend `metrics_modal_rects` to apply offset + clamp.                                                                     |
| Metrics dispatcher             | `src/app/metrics_overlay.rs`         | Drag state machine (`Down(Left)` on title row → `Drag(Left)` → `Up(Left)`). `=` resets size + offset.                                                                                                                  |
| Pending Actions state + render | `src/ui/pending_actions.rs`          | Add `multi_selected: BTreeSet<String>` (stable keys). Add `pending_actions_modal_rects` returning `{ area, list, explainer, hint }`. Bump default modal size. New row layout with `[ ]`/`[x]` checkbox column.         |
| Pending Actions dispatcher     | `src/app/pending_actions_overlay.rs` | Replace single `EnterSelected(idx)` outcome with `PendingActionsOutcome` enum (Closed / AcceptItems / ClearItems / CopyItem). Handle Space/P/Y/A/c/p/d/y; mouse hit-test on checkbox vs content; wheel scrolls cursor. |
| TUI loop wiring                | `src/app/tui_loop.rs`                | Stop opening Action Explainer modal from a overlay. Route new outcomes to existing accept/reject/hide/copy paths, looping per-item.                                                                                    |
| Help / docs                    | `docs/ai/UI_MANUAL.md`, help overlay | §8.5 (m drag) + §8.7 rewrite (a split + multi-select).                                                                                                                                                                 |

Reused unchanged:

- `crate::app::action_explainer::{build_accept_view, build_copy_view, render_explainer_lines}` — the right pane is a Paragraph rendered from the same lines as the modal explainer.
- `crate::app::prompt_send_actions::handle_prompt_send_action_for_proposal` — bulk accept calls this per item.
- `crate::ui::pending_actions::collect_pending_items` — item collection unchanged. Stable keys derived from `PendingItem` fields already present.
- v1.39 PendingAction snapshot validation: each per-item dispatch reuses the existing path and so the snapshot/vanish check applies per item.

## 4. m overlay — drag to move

### 4.1 State

```rust
pub struct MetricsOverlay {
    open: bool,
    scroll: u16,
    width_pct: u16,
    height_pct: u16,
    offset_x: i16,                 // NEW
    offset_y: i16,                 // NEW
    drag_anchor: Option<DragAnchor>, // NEW
}

struct DragAnchor {
    start_col: u16,
    start_row: u16,
    start_offset_x: i16,
    start_offset_y: i16,
}
```

`offset_x` / `offset_y` are signed integer cell deltas from the
centered base rect. Default `0` → modal renders centered (current
behavior). Both fields survive `close()` so the next `open()` reuses
the operator's chosen position. Neither field is persisted to
`qmonster.toml`; restart resets to `(0, 0)`. `drag_anchor` is `None`
outside an active drag and is cleared by any non-`Drag` mouse event.

### 4.2 Layout math

```rust
pub fn metrics_modal_rects(viewport: Rect, w_pct: u16, h_pct: u16,
                           offset_x: i16, offset_y: i16) -> MetricsModalRects {
    let base = centered_rect(w_pct, h_pct, viewport);
    let area = apply_clamped_offset(base, viewport, offset_x, offset_y);
    // banner / body / hint partition unchanged
    ...
}
```

Left and top edges are hard bounds — the modal cannot extend past them,
because Ratatui's u16-based `Rect` cannot represent a partial off-screen
render at negative coordinates. Right and bottom edges are soft bounds —
the modal may extend past, with at least 4 cells (horizontally) / 1 row
(vertically) remaining inside the viewport.

The clamp runs on every render so a terminal resize that shrinks the
viewport pulls the modal back into the safe area. Concretely:

```text
min_x = viewport.x as i32                                         // hard bound: left edge
max_x = viewport.x as i32 + viewport.width as i32 - 4            // soft bound: >= 4 cells visible
min_y = viewport.y as i32                                         // hard bound: top edge
max_y = viewport.y as i32 + viewport.height as i32 - 1            // soft bound: >= 1 row visible
```

### 4.3 Drag state machine (in `app::metrics_overlay`)

| Event                              | Position                                  | Effect                                                                                                                                          |
| ---------------------------------- | ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `Down(Left)`                       | `close_button_rect(area)`                 | Existing close path.                                                                                                                            |
| `Down(Left)`                       | top border row of `area`, excluding `[x]` | `drag_anchor = Some(DragAnchor { start_col, start_row, start_offset_x: offset_x, start_offset_y: offset_y })`.                                  |
| `Down(Left)`                       | elsewhere inside `area`                   | No drag start. Wheel/scroll handling unchanged.                                                                                                 |
| `Drag(Left)`                       | (anchor present)                          | `offset_x = anchor.start_offset_x + (event.column as i16 - anchor.start_col as i16)`; same for `offset_y`. Layout clamp applied at render time. |
| `Up(Left)` or any non-`Drag` event | (anchor present)                          | `drag_anchor = None`.                                                                                                                           |

The top border row is identified as `Rect::new(area.x, area.y,
area.width, 1)` minus `close_button_rect(area)`.

### 4.4 Reset & persistence

- `=` key: existing size reset extended to also set `offset_x = 0; offset_y = 0;`.
- `close()`: keep `width_pct`, `height_pct`, `offset_x`, `offset_y`. Clear `drag_anchor`. (Existing `scroll = 0` reset retained.)
- Restart: defaults restored (size 95×90, offset 0×0). No toml field added.

### 4.5 Tests (m drag)

- `metrics_modal_rects(_, _, _, 0, 0)` returns the same rect as the existing centered helper (regression).
- Positive offset moves area right/down; negative moves left/up (within clamp).
- Right clamp leaves ≥ 4 cells visible (soft bound); left clamp snaps flush to viewport.x (hard bound). Bottom clamp leaves ≥ 1 row visible (soft bound); top clamp snaps flush to viewport.y (hard bound).
- `Down(Left)` on title row (not `[x]`) sets anchor; subsequent `Drag(Left)` updates offset; `Up(Left)` clears anchor.
- `Down(Left)` on body / hint does NOT set anchor (wheel scroll still works).
- `Down(Left)` on `[x]` closes overlay (anchor untouched, overlay closed wins).
- `=` resets size AND offset.
- Reopen preserves offset (in-session).
- Viewport shrink between renders pulls offset back inside clamp.

## 5. a overlay — live explainer + multi-select

### 5.1 Modal size

Bump from `70% × 60% (min 60×16)` to **`80% × 65% (min 72×20)`**.
Reason: split layout needs more horizontal space for both the list and
the live explainer to read comfortably.

### 5.2 Split layout helper

```rust
pub struct PendingActionsModalRects {
    pub area: Rect,
    pub list: Rect,
    pub explainer: Rect,
    pub hint: Rect,
}

pub(crate) fn pending_actions_modal_rects(viewport: Rect)
    -> PendingActionsModalRects;
```

Body inside borders carved as:

| Section | Position                    |
| ------- | --------------------------- |
| `hint`  | bottom 1 row inside borders |
| `body`  | between top border and hint |

Inside `body`:

- **Wide mode** (`body.width >= 72`) — vertical split. List width `32`
  cells fixed; explainer = `body.width - 32 - 1` (the `1` is a
  visual separator drawn via `Block::default().borders(Borders::LEFT)`
  on the explainer rect).
- **Narrow mode** (`body.width < 72`) — horizontal split. List = top
  half of `body.height`; explainer = bottom half. Separator drawn via
  `Block::default().borders(Borders::TOP)` on the explainer rect.

### 5.3 State additions

```rust
pub struct PendingActionsOverlay {
    open: bool,
    selected: usize,
    multi_selected: BTreeSet<String>, // NEW
}
```

**Stable key derivation** (`pending_item_key(&PendingItem) -> String`):

| Item kind  | Key form                                                                                |
| ---------- | --------------------------------------------------------------------------------------- |
| `Proposal` | `format!("p:{proposal_id}")`                                                            |
| `Copy`     | `format!("y:{alert_title}\u{1F}{command}")` (unit separator U+001F to avoid collisions) |

`alert_idx` is intentionally **not** in the key — it shifts as alerts
are hidden between polls. Title + command is stable enough for
ergonomic multi-select.

**Auto-prune**: `pending_actions_lines` (or a `prune_to(items)` helper
called at the start of every render of the overlay) drops keys not in
the current items list. Stale entries vanish silently.

`open()` keeps `multi_selected` empty (default already empty).
`close()` clears `multi_selected` and resets `selected = 0`.

### 5.4 Row rendering

Each item row in the list pane (positions are relative to `list.x`, where `list.x` is the left edge of the list rect):

```
[x] ▶ [p]  CONCERN  · /compact · codex:1:review · %57
^^^ ^ ^^^^^^^^^^^^^^^ ^^^^^^^^^^ ^^^^^^^^^^^^^^^^^^^^
cb  c kind+severity   command    context
```

| Cols (offset from `list.x`) | Span                                                     |
| --------------------------- | -------------------------------------------------------- |
| `0..3`                      | `[x]` (multi-selected, severity-colored bg) or `[ ]`.    |
| `3`                         | Single space (separator).                                |
| `4..6`                      | Cursor mark — `▶ ` if `idx == selected`, `  ` otherwise. |
| `6..`                       | Existing `[p\|y] severity · command · context` styling.  |

### 5.5 Right-pane live explainer

For `items[selected]`:

- Build `ActionExplainView` via existing helpers — `build_accept_view(action)` for proposals, `build_copy_view(action)` for alerts.
- `action: PendingAction` constructed from the item via a new helper `pending_action_from_item(items, idx) -> Option<PendingAction>` (logic extracted from the current `enter_selected_pending_item` site in `tui_loop`, see Section 7).
- Render `render_explainer_lines(view, rects.explainer)` into a Paragraph. The Paragraph has no full Block; the visual separator from the list pane is provided by a tiny side widget — `Block::default().borders(Borders::LEFT)` (wide mode) or `Borders::TOP` (narrow mode) — rendered into `rects.explainer` _before_ the Paragraph so the border occupies col 0 (wide) or row 0 (narrow), and the Paragraph then renders into the inner area.
- Empty list → render dim `"Select an item to see what would happen."` placeholder.

The right pane always reflects the **cursor**, never the multi-select
set. The `Mode now` line continues to surface `observe_only` /
`AutoSendOff` warnings per existing logic.

### 5.6 Title and bottom hint

- **Title** (top border): `"Pending Actions · {N} pending · {S} selected · a 다시로 닫기"` where the `· {S} selected` segment is omitted when `multi_selected` is empty.
- **Bottom hint** (the `hint` rect): single line, multi-aware:
  ```
  Space toggle · P/Y/A group · c clear-sel · p accept(N) · d clear(M) · y copy(K) · a/Esc close
  ```
  `(N)`, `(M)`, `(K)` reflect what the corresponding key would dispatch right now:
  - `multi_selected` non-empty: `N` = proposals in multi, `M` = `multi_selected.len()`, `K` = alerts in multi (capped at 1 since `y` only copies the first).
  - `multi_selected` empty: each count is `1` if the cursor item matches the action's allowed kind, else `0`.
  - When a count is `0` the entire `key (n)` segment renders dim so the operator sees “this key would do nothing right now.” The text shape stays the same so the hint width does not jiggle as selection changes.

### 5.7 Outcome contract

```rust
pub enum PendingActionsOutcome {
    None,
    Closed,
    AcceptItems(Vec<usize>), // proposal indices only (already filtered)
    ClearItems(Vec<usize>),  // mixed proposal + alert indices
    CopyItem(usize),         // single alert index
}
```

The dispatcher consumes this in `tui_loop`, looping per index for the
bulk variants and routing each item to the existing single-item
handler.

### 5.8 Key matrix

| Key               | Action                                                                                                                                                        | Outcome                       |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| `Esc` · `q` · `a` | Close overlay (clears `multi_selected`).                                                                                                                      | `Closed`                      |
| `↑` · `k`         | `select_prev` (clamp 0).                                                                                                                                      | `None`                        |
| `↓` · `j`         | `select_next` (clamp last).                                                                                                                                   | `None`                        |
| `Space`           | Toggle `multi_selected` membership for `items[selected]`.                                                                                                     | `None`                        |
| `P`               | Toggle proposal group (all-on ↔ all-off).                                                                                                                     | `None`                        |
| `Y`               | Toggle alert group.                                                                                                                                           | `None`                        |
| `A`               | Toggle full set.                                                                                                                                              | `None`                        |
| `c`               | `multi_selected.clear()`.                                                                                                                                     | `None`                        |
| `p`               | If `multi_selected` non-empty: filter its keys → indices that resolve to live proposals. Else: `vec![selected]` if cursor is proposal. Empty result → `None`. | `AcceptItems(idxs)` or `None` |
| `d`               | If `multi_selected` non-empty: keys → indices (proposals + alerts). Else: `vec![selected]`. Empty result → `None`.                                            | `ClearItems(idxs)` or `None`  |
| `y`               | Multi’s first alert index, else cursor if alert.                                                                                                              | `CopyItem(idx)` or `None`     |
| `Enter`           | Silently swallowed.                                                                                                                                           | `None`                        |
| Other             | Ignored.                                                                                                                                                      | `None`                        |

Group toggle (`P` / `Y` / `A`) semantics: if every item of the group is
already in `multi_selected`, remove them all; otherwise add all. Empty
group is a no-op (safe).

### 5.9 Mouse matrix

| Event         | Position (coordinates relative to `pending_actions_modal_rects`) | Effect                                                           |
| ------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `Down(Left)`  | inside `close_button_rect(area)`                                 | Close → `Closed`.                                                |
| `Down(Left)`  | inside `list`, row maps to item, cols `[list.x .. list.x + 4)`   | Toggle `multi_selected` for that item; cursor unchanged. `None`. |
| `Down(Left)`  | inside `list`, row maps to item, cols `>= list.x + 4`            | `selected = idx`. `None`.                                        |
| `Down(Left)`  | `list` but row outside item rows                                 | No-op. `None`.                                                   |
| `Down(Left)`  | `explainer`                                                      | No-op (read-only mirror). `None`.                                |
| `Down(Left)`  | `hint` or border or anywhere else inside `area`                  | No-op. `None`.                                                   |
| `ScrollUp`    | anywhere inside `area`                                           | `select_prev`. `None`.                                           |
| `ScrollDown`  | anywhere inside `area`                                           | `select_next`. `None`.                                           |
| Anything else | anywhere inside `area`                                           | Swallow (no leak to dashboard). `None`.                          |

Row → item index calculation:

```text
let leading_pad: u16 = 1;        // 1-row top breathing space
let row_off = event.row.checked_sub(rects.list.y + leading_pad)?;
let idx = row_off as usize;
if idx < items.len() { Some(idx) } else { None }
```

### 5.10 Bulk dispatch + audit

Per-item routing through existing single-item paths:

- `AcceptItems(idxs)` → for each `idx`: build `PendingAction` from `items[idx]`, call `handle_prompt_send_action_for_proposal(target_pane_id, slash_command, proposal_id)`. Audit chain matches existing single accept (mode-dependent). Stale (`proposal_id` no longer matches live items) → vanish notice, skip.
- `ClearItems(idxs)` → for each `idx` in ascending order: proposal → reject path emitting `PromptSendRejected`; alert → existing `hide_alert_for(alert_idx)` (UI-only, no audit event). Idx ascending → deterministic audit ordering.
- `CopyItem(idx)` → existing `copy_alert_command_to_clipboard` path. Multi-alert selection copies only the first; clipboard is single-line.

After dispatch, **only dispatched keys are removed** from
`multi_selected`. Non-dispatched keys (e.g. alert keys when `p` is
pressed) remain selected so the operator can act on them next without
re-selecting.

### 5.11 `[ux] confirm_actions` interaction

- Dashboard direct `p`/`d`/`y`: behavior unchanged. `confirm_actions = always | first_time | never` continues to gate the standalone Action Explainer modal.
- Inside the a overlay: `confirm_actions` is **ignored**. The right-pane live explainer is the confirmation surface. UI_MANUAL §8.7 documents this.

### 5.12 Actuation mode display

The right-pane explainer renders `Mode now: observe_only ⚠ ...` /
`Mode now: AutoSendOff ⚠ ...` exactly as the modal explainer does
today (same `build_accept_view` / `build_copy_view` source). Operators
see mode warnings before pressing `p` in the overlay.

### 5.13 System notices

Per-item notices retained — bulk does not coalesce into a summary
notice. The existing `SystemNotice` queue / dim-after-N-seconds flow
already handles N items gracefully. (No `BulkActionSummary` notice
type added.)

### 5.14 Tests (a overlay)

State / collection:

- `multi_selected` round-trip on `open` / `close` (cleared on close).
- Stable key auto-prune: insert key not in current items → render call drops it.
- Group toggle: `P` with mixed selection of proposals → all selected; `P` again → all unselected.
- `c` empties `multi_selected` without touching `selected`.

Layout:

- `pending_actions_modal_rects(viewport)` partitions sum back to area minus borders.
- Wide vs narrow threshold transitions at 72-cell body width.
- Right pane renders cursor-item explainer; switches when cursor moves.
- Empty items → placeholder line in explainer panel.

Key handler:

- 13-key matrix × multi empty/non-empty × cursor p/y/none → outcome correctness.
- `Enter` returns `None` and does not close.
- `p` with mixed multi-select returns proposal indices only.
- `d` returns mixed indices in ascending order.
- `y` returns first alert index in multi (or cursor alert).

Mouse handler:

- Click checkbox cols: toggles multi without moving cursor.
- Click content cols: moves cursor.
- Click explainer / hint / borders: no-op.
- Wheel up/down: cursor up/down.
- `[x]` click closes.

Bulk dispatch (in `tui_loop` integration tests):

- `AcceptItems(N)` invokes `handle_prompt_send_action_for_proposal` N times with the right per-item args.
- `ClearItems(M proposals + K alerts)` invokes M reject + K hide; proposal idx ascending.
- Stale proposal_id in bulk → vanish notice for that item, others succeed.
- After `p` on mixed multi: dispatched proposal keys removed; alert keys remain.
- After `d` on full multi: all keys removed.
- `observe_only`: bulk accept emits `PromptSendBlocked` per item, no `PromptSendAccepted`.
- `AutoSendOff`: bulk accept emits `PromptSendAccepted + PromptSendBlocked` per item.
- `CopyItem` with two-alert multi copies only the first.

## 6. Documentation updates

- `docs/ai/UI_MANUAL.md` §8.5: add a paragraph after the existing `[`/`]`/`=` resize description: drag the top border (title row) to move; `=` resets both size and position; restart resets to defaults.
- `docs/ai/UI_MANUAL.md` §8.7: rewrite to cover the split layout, multi-select, bulk dispatch keys, mouse hit zones, and the `confirm_actions` ignore note inside the overlay.
- Help overlay (`?`): refresh Pending Actions entries to list new keys (`Space`/`P`/`Y`/`A`/`c`/`p`/`d`/`y`) and note that Enter is intentionally a no-op.
- `.mission/CURRENT_STATE.md` post-tag polish list to mention this work once landed.

## 7. Files touched (summary)

| File                                 | Reason                                                                                                                                                                                                                                                    |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/ui/metrics.rs`                  | Offset fields, drag anchor, modified `metrics_modal_rects`, `=` reset path.                                                                                                                                                                               |
| `src/app/metrics_overlay.rs`         | Drag state machine on title row; reset key extension.                                                                                                                                                                                                     |
| `src/ui/pending_actions.rs`          | Modal size bump, split layout helper, `multi_selected` field + stable keys + auto-prune, new row layout, right-pane explainer renderer, title + hint formatting.                                                                                          |
| `src/app/pending_actions_overlay.rs` | New `PendingActionsOutcome` enum; key + mouse handlers per Sections 5.8/5.9. Add `pending_action_from_item(items, idx) -> Option<PendingAction>` helper (logic extracted from the now-obsolete tui_loop site).                                            |
| `src/app/tui_loop.rs`                | Stop opening Action Explainer modal from the a overlay; wire `AcceptItems` / `ClearItems` / `CopyItem` to existing per-item paths. Delete the previous `enter_selected_pending_item` helper (its construction logic moves to `pending_action_from_item`). |
| `src/app/action_explainer.rs`        | No semantic change. `build_accept_view` / `build_copy_view` / `render_explainer_lines` are reused as-is by both the live panel and the dashboard direct-key Action Explainer modal.                                                                       |
| `docs/ai/UI_MANUAL.md`               | §8.5 / §8.7 updates.                                                                                                                                                                                                                                      |
| Help overlay text                    | Pending Actions key entries.                                                                                                                                                                                                                              |

## 8. Out of scope

- Persisting m overlay position between Qmonster runs.
- Persisting a overlay multi-select across overlay close / reopen.
- Bulk summary system notice (per-item notices retained).
- Resizing or dragging the a overlay (m only).
- Redesigning Action Explainer modal for dashboard direct keys — only the in-overlay path mirrors its content into the right pane.
- Per-pane mode differences (actuation mode is global; no per-item mode warnings beyond what the cursor item already shows).

## 9. Open questions / risks

- **Risk: `Drag(Left)` event delivery on terminals**. `crossterm` emits `MouseEventKind::Drag(button)` only when the terminal supports SGR mouse with button-tracking. The current Qmonster mouse capture already enables this (existing wheel + click handling), so no new terminfo work is expected.
- **Risk: very narrow terminals (< 72 inner-body width)**. Narrow-mode horizontal split shows the explainer below the list, halving each. On terminals < ~40 cols total the layout will wrap heavily; the `min(72, 20)` modal clamp protects only against extreme cases. Acceptable.
- **Risk: drag during active polling redraw**. The drag offset is updated in the mouse handler before the next render; the render reads the latest value via the overlay state. No race expected.
- **Open question (deferred)**: do operators want a "select all proposals of a single severity" key (e.g. `R` for risk-only)? Not included in v1; revisit after v1.40 lands if asked.
