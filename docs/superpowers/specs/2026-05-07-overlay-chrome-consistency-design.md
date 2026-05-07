# 2026-05-07 - Overlay Chrome consistency (design)

Status: design accepted, implementation plan pending.
Audience: Qmonster TUI maintainers and Claude/Codex/Gemini implementers.
Baseline: `main` at `08a28a4` (Phase 8 footer/help/S/P/anomaly
discoverability pass).

This design standardizes how Qmonster modal overlays open, close, move,
resize, scroll, and expose close affordances. The goal is not to make
every overlay visually identical or equally heavy. The goal is to give
operators one reliable mental model and give maintainers one small set
of reusable layout and event helpers.

## 1. Goal

Create a shared "Overlay Chrome" contract for the TUI:

1. Every overlay has a visible `[x]` close affordance in the same
   top-right location.
2. Every persistent overlay closes with `Esc`, `q`, its entry key, and
   `[x]`.
3. Long overlays support keyboard and mouse scroll.
4. Large data overlays support size reset and resize.
5. Large persistent overlays support title-row drag to move.
6. Split overlays can resize their internal sections by key and mouse.
7. The help overlay and UI manual describe the contract once, then list
   per-overlay exceptions.

## 2. Product Decision

Use a tiered contract instead of forcing every modal to implement every
feature.

| Tier | Overlays | Required behavior |
| --- | --- | --- |
| Basic modal | Action Explainer and other short confirmations | `[x]`, `Esc`, and same-key cancel when meaningful. `q` is optional. No resize or drag required. |
| Scroll modal | Help, Git, Provider Setup, Settings, Token Insights, Anomaly Events | Basic modal + keyboard/wheel scroll. |
| Resizable modal | Metrics, Token Insights, Anomaly Events, Provider Setup, Settings, Help, Git | Scroll modal + `[` shrink, `]` grow, `=` reset size/position, title-row drag move. |
| Split modal | Pending Actions, future sectioned overlays | Resizable modal + internal separator resize with `,`/`.` and separator drag. |

Rationale:

- `m` Metrics and `a` Pending Actions already demonstrate the two
  mature patterns: resizable/movable modal and split-pane modal.
- Small confirmation modals become harder to operate if they gain
  unnecessary geometry state.
- `S` Settings needs edit-mode protection, so its entry-key close and
  resize keys must not fire while text editing is active.

## 3. Non-goals

- Persisting modal geometry to `qmonster.toml`. Geometry stays
  in-session only.
- Changing provider adapters, policy rules, audit schema, or
  recommendation behavior.
- Adding new overlay tabs, new data queries, or new token-optimization
  insights.
- Making all overlay content use the same layout. Only the chrome and
  input contract are standardized.
- Treating close as a destructive action. Close only dismisses the
  overlay unless the existing modal already documents a cancel/confirm
  path.

## 4. Current Overlay Inventory

| Overlay | Entry key | Current state | Target tier |
| --- | --- | --- | --- |
| Metrics | `m` | Has `[x]`, same-key close, resize, reset, title drag, scroll. | Resizable modal baseline |
| Pending Actions | `a` | Has `[x]`, same-key close, resize, reset, title drag, split resize, scroll. | Split modal baseline |
| Token Insights | `i` | Has `[x]`, same-key close, scroll, refresh. No resize/drag. | Resizable modal |
| Anomaly Events | `n` | Has `[x]`, same-key close, scroll, history toggle. No resize/drag. | Resizable modal |
| Provider Setup | `P` | Has `[x]`, same-key close, tab click, scroll. No resize/drag. | Resizable modal |
| Settings | `S` | Has `[x]`, same-key close when not editing, tab click, field selection. No resize/drag. | Resizable modal with edit guards |
| Help | `?` | Shared scroll modal, `[x]`, scroll. No resize/drag. | Resizable modal |
| Git | footer version click | Shared scroll modal, `[x]`, scroll. No resize/drag. | Resizable modal |
| Target picker | `t` | Has `[x]`, same-key close, list/preview layout. | Scroll modal; split resize optional later |
| Action Explainer | `p`/`d`/`y` flow | Has `[x]`, confirm/cancel semantics. | Basic modal |

## 5. Shared Interaction Contract

### 5.1 Open and Close

Every persistent overlay keeps its existing entry key. When the overlay
is already open, pressing the same key closes it:

| Overlay | Same-key close |
| --- | --- |
| Help | `?` |
| Settings | `S` when not editing |
| Provider Setup | `P` |
| Metrics | `m` |
| Anomaly Events | `n` |
| Pending Actions | `a` |
| Token Insights | `i` |
| Target picker | `t` |

All persistent overlays also close on `Esc`, `q`, and `[x]` click.

Settings exception: while editing a numeric field, printable keys and
editing keys belong to the editor. `Esc` first cancels edit mode and a
second close action dismisses the overlay, matching current behavior.

### 5.2 Scrolling

Scrollable overlays use:

- `Up` / `Down` and `k` / `j`: one row.
- `PageUp` / `PageDown`: page jump when the overlay has page-level
  scroll support.
- `Home` / `End`: jump to start/end when the overlay has bounded
  content.
- Mouse wheel over the modal body: scroll.

Each overlay keeps domain-specific keys, for example `r` refresh in
Token Insights and `h` Ring/History toggle in Anomaly Events.

### 5.3 Resize and Reset

Resizable overlays use:

- `[`: shrink modal.
- `]`: grow modal.
- `=`: reset size and position.

Resize changes width and height together for single-pane overlays,
matching `m` Metrics. Split overlays can additionally resize an
internal section:

- `,`: narrow the primary/list section.
- `.`: widen the primary/list section.
- Separator drag: direct mouse resize.

The implementation should keep the current in-session behavior:
geometry survives close/reopen and resets on process restart.

### 5.4 Move

Resizable overlays can be moved by dragging the title row, excluding
the `[x]` hit target. The clamp rules should match `m` Metrics and
`a` Pending Actions:

- Left/top edges are hard-bounded inside the viewport.
- Right/bottom edges are soft-bounded so a small grip remains visible.
- `=` returns to centered position.

### 5.5 Mouse Priority

Mouse dispatch order is consistent:

1. `[x]` close wins over every other mouse action.
2. Title-row drag start, excluding `[x]`.
3. Split separator drag start for split overlays.
4. Body click behavior, such as tab switch or row selection.
5. Wheel scroll.
6. Any non-drag event clears active drag anchors.

## 6. Close Button Style

Do not color `[x]` as Risk/Warning by default. Closing an overlay is not
destructive, and red/orange would compete with Qmonster's severity
palette.

Add a shared style helper:

```rust
pub fn modal_close_style() -> Style {
    Style::default()
        .fg(theme::BORDER_ACTIVE)
        .add_modifier(Modifier::BOLD)
}
```

This intentionally makes `[x]` visible without implying danger. It also
ties the close affordance to the active modal border instead of the main
text color. If contrast is insufficient in a terminal theme, the helper
can be adjusted centrally without touching each overlay renderer.

## 7. Architecture

Introduce a small reusable chrome layer rather than copying the
Metrics/Pending Actions geometry logic again.

| Component | Proposed file | Responsibility |
| --- | --- | --- |
| `ModalGeometry` | `src/ui/modal_chrome.rs` | Open-independent geometry state: width, height, offset, drag anchor, optional split width. |
| `ModalChromeConfig` | `src/ui/modal_chrome.rs` | Defaults/min/max, title, tier flags, and soft-bound clamp policy. |
| Layout helpers | `src/ui/modal_chrome.rs` | `modal_area`, `apply_clamped_offset`, `close_button_rect`, title-row hit testing, split rect calculation. |
| Event helpers | `src/app/modal_chrome.rs` | Shared key/mouse handlers for close, resize, move, scroll, split resize. |
| Style helper | `src/ui/theme.rs` | `modal_close_style()` or equivalent central style. |

The helpers should be additive. Existing overlays keep their local state
types at first, then migrate field-by-field to avoid a broad rewrite.

Recommended migration order:

1. Extract common style and hit-test helpers without behavior change.
2. Migrate `i` Token Insights and `n` Anomaly Events to resizable modal.
3. Migrate Help and Git shared scroll modal.
4. Migrate `P` Provider Setup.
5. Migrate `S` Settings with edit-mode guard tests.
6. Consider `t` Target picker split resize only after the core overlay
   set is stable.

## 8. Documentation and Help

Update:

- `docs/ai/UI_MANUAL.md`: add one "Overlay chrome contract" subsection.
- `?` Help: replace repeated close/resize prose with a short common
  line plus overlay-specific keys.
- `docs/ai/ARCHITECTURE.md`: note that overlay geometry is a shared UI
  layer, not a policy or data model change.
- `docs/ai/VALIDATION.md`: add acceptance checks for the contract.

The help/footer should avoid becoming too long. Footer should continue
to show entry keys, while Help documents advanced geometry controls.

## 9. Tests

Add tests before implementation for each migration slice:

- Same-key close works for every persistent overlay.
- `[x]` hit target closes every overlay and wins over drag/selection.
- `modal_close_style` is used by each overlay renderer.
- `[` / `]` / `=` resize/reset work for migrated resizable overlays.
- Title-row drag changes offset and `=` resets it.
- Wheel and arrow scroll behavior still clamps.
- Split overlays keep `,` / `.` and separator drag behavior.
- Settings edit mode consumes editing keys before overlay chrome keys.
- Help text and UI manual list the common contract and exceptions.

For the first implementation slice, run:

- Targeted overlay unit tests for the changed overlays.
- `cargo fmt --all --check`
- `git diff --check`
- `cargo test --all-targets`
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`

## 10. Rollout Recommendation

Implement this as a small UI consistency phase, not as part of token
policy work. The first useful slice is:

1. Add `modal_close_style` and apply it everywhere.
2. Add shared geometry helpers while preserving `m` and `a` behavior.
3. Upgrade `i` and `n` to resizable/movable overlays.
4. Update Help/UI manual and validate.

That slice gives immediate operator value and proves the abstraction
before touching the more complex `S`, `P`, Help/Git, and target picker
surfaces.
