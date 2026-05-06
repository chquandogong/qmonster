# Mouse Drag & Bulk Overlays Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add mouse drag to the m (Metrics) overlay and rebuild the a (Pending Actions) overlay with a left/right split, live Action Explainer panel, multi-select, and bulk dispatch keys.

**Architecture:** Two independent feature stacks landed sequentially. m drag = pure offset + clamp + drag state machine. a overlay = stable-key multi-select set + split layout helper + per-item bulk dispatch reusing existing single-item paths (no new audit event types). The dashboard's direct `p`/`d`/`y` keys and `[ux] confirm_actions` are untouched. Reference spec: `docs/superpowers/specs/2026-05-06-mouse-drag-and-bulk-overlays-design.md`.

**Tech Stack:** Rust 2021, ratatui (UI), crossterm (input), `cargo test --lib`, `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`, `cargo fmt --all --check`.

---

## File Map

- `src/ui/metrics.rs` — `MetricsOverlay` gains `offset_x`, `offset_y`, `drag_anchor`. `metrics_modal_rects` signature gains `offset_x: i16, offset_y: i16` and clamps.
- `src/app/metrics_overlay.rs` — drag state machine on title row; `=` reset extends to offset.
- `src/ui/pending_actions.rs` — `multi_selected: BTreeSet<String>` field; `pending_action_key`, group toggles, auto-prune; `pending_actions_modal_rects` split helper; row layout with checkbox + cursor; right-pane explainer renderer; modal size bumped to 80×65 (min 72×20); title + hint counters.
- `src/app/pending_actions_overlay.rs` — `PendingActionsOutcome` enum (replaces `PendingActionsKeyOutcome`); key handler with Space/P/Y/A/c/p/d/y/Enter; mouse handler with checkbox vs content hit-test, wheel, close; `accept_action_for` / `reject_action_for` / `copy_action_for` / `build_explainer_view_for_item` helpers consumed by both the live panel and the bulk dispatch.
- `src/app/tui_loop.rs` — wires `AcceptItems` / `ClearItems` / `CopyItem` through `confirm_pending_action` per item plus the alert-hide deadline path; deletes `dispatch_pending_action` and the `EnterSelected` branch; passes the new `PendingActionsRenderCtx` and the new offset args to `metrics_modal_rects`.
- `docs/ai/UI_MANUAL.md` — §8.5 m drag note, §8.7 a overlay rewrite.
- Help overlay text — Pending Actions key entries refreshed.

## Validation Gates (run after every task)

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

If any gate fails, fix before committing the task. Each task's "Commit" step assumes all three gates pass.

---

# Phase A — m overlay drag

## Task 1: m overlay offset state + clamped layout

**Files:**

- Modify: `src/ui/metrics.rs:102-185` (struct + impl), `src/ui/metrics.rs:194-210` (`metrics_modal_rects`), `src/ui/metrics.rs:817-825` (renderer call site), `src/ui/metrics.rs:887-963` (existing tests pinned to old signature).

- [ ] **Step 1: Write the failing offset + clamp tests**

Append inside the existing `#[cfg(test)] mod tests` block in `src/ui/metrics.rs`:

```rust
#[test]
fn metrics_modal_rects_zero_offset_matches_centered() {
    let viewport = Rect::new(0, 0, 200, 80);
    let with_offset = metrics_modal_rects(viewport, 95, 90, 0, 0);
    let baseline = crate::ui::dashboard::centered_rect(95, 90, viewport);
    assert_eq!(with_offset.area, baseline);
}

#[test]
fn metrics_modal_rects_positive_offset_moves_right_down() {
    let viewport = Rect::new(0, 0, 200, 80);
    let r0 = metrics_modal_rects(viewport, 95, 90, 0, 0);
    let r1 = metrics_modal_rects(viewport, 95, 90, 5, 3);
    assert_eq!(r1.area.x, r0.area.x + 5);
    assert_eq!(r1.area.y, r0.area.y + 3);
}

#[test]
fn metrics_modal_rects_negative_offset_moves_left_up() {
    let viewport = Rect::new(0, 0, 200, 80);
    let r0 = metrics_modal_rects(viewport, 95, 90, 0, 0);
    let r1 = metrics_modal_rects(viewport, 95, 90, -4, -2);
    assert_eq!(r1.area.x, r0.area.x.saturating_sub(4));
    assert_eq!(r1.area.y, r0.area.y.saturating_sub(2));
}

#[test]
fn metrics_modal_rects_right_clamp_keeps_4_cells_visible() {
    // Push the modal hard to the right; clamp must leave >= 4 cells on screen.
    let viewport = Rect::new(0, 0, 200, 80);
    let r = metrics_modal_rects(viewport, 50, 50, i16::MAX, 0);
    let visible_right = (viewport.x + viewport.width).saturating_sub(r.area.x);
    assert!(
        visible_right >= 4,
        "right clamp leaves {visible_right} cells visible (expected >= 4)"
    );
}

#[test]
fn metrics_modal_rects_left_clamp_keeps_4_cells_visible() {
    let viewport = Rect::new(0, 0, 200, 80);
    let r = metrics_modal_rects(viewport, 50, 50, i16::MIN, 0);
    let visible_left = (r.area.x + r.area.width).saturating_sub(viewport.x);
    assert!(
        visible_left >= 4,
        "left clamp leaves {visible_left} cells visible (expected >= 4)"
    );
}

#[test]
fn metrics_modal_rects_bottom_clamp_keeps_1_row_visible() {
    let viewport = Rect::new(0, 0, 200, 80);
    let r = metrics_modal_rects(viewport, 50, 50, 0, i16::MAX);
    let visible_height = (viewport.y + viewport.height).saturating_sub(r.area.y);
    assert!(
        visible_height >= 1,
        "bottom clamp leaves {visible_height} row(s) visible (expected >= 1)"
    );
}
```

- [ ] **Step 2: Run the new tests to confirm they fail**

```bash
cargo test --lib metrics::tests::metrics_modal_rects -- --nocapture
```

Expected: compilation error — `metrics_modal_rects` takes 3 args, tests pass 5.

- [ ] **Step 3: Update struct + impl with offset and drag anchor (no behavioral change yet for drag)**

Replace the existing `pub struct MetricsOverlay { ... }` and its `Default` / `impl` blocks (`src/ui/metrics.rs:102-185`) with:

```rust
#[derive(Debug, Clone)]
pub struct MetricsOverlay {
    open: bool,
    scroll: u16,
    width_pct: u16,
    height_pct: u16,
    offset_x: i16,
    offset_y: i16,
    drag_anchor: Option<DragAnchor>,
}

#[derive(Debug, Clone, Copy)]
pub struct DragAnchor {
    pub start_col: u16,
    pub start_row: u16,
    pub start_offset_x: i16,
    pub start_offset_y: i16,
}

impl Default for MetricsOverlay {
    fn default() -> Self {
        Self {
            open: false,
            scroll: 0,
            width_pct: Self::DEFAULT_WIDTH_PCT,
            height_pct: Self::DEFAULT_HEIGHT_PCT,
            offset_x: 0,
            offset_y: 0,
            drag_anchor: None,
        }
    }
}

impl MetricsOverlay {
    pub const SIZE_STEP: u16 = 5;
    pub const SIZE_MIN: u16 = 50;
    pub const SIZE_MAX: u16 = 99;
    pub const DEFAULT_WIDTH_PCT: u16 = 95;
    pub const DEFAULT_HEIGHT_PCT: u16 = 90;

    pub fn new() -> Self { Self::default() }

    pub fn open(&mut self) {
        self.open = true;
        self.scroll = 0;
        self.drag_anchor = None;
        // offset_x / offset_y intentionally preserved (mirrors size persistence)
    }

    pub fn close(&mut self) {
        self.open = false;
        self.scroll = 0;
        self.drag_anchor = None;
        // offset_x / offset_y preserved for next open
    }

    pub fn is_open(&self) -> bool { self.open }
    pub fn scroll(&self) -> u16 { self.scroll }
    pub fn scroll_up(&mut self) { self.scroll = self.scroll.saturating_sub(1); }
    pub fn scroll_down(&mut self, max: u16) { self.scroll = self.scroll.saturating_add(1).min(max); }

    pub fn width_pct(&self) -> u16 { self.width_pct }
    pub fn height_pct(&self) -> u16 { self.height_pct }
    pub fn offset_x(&self) -> i16 { self.offset_x }
    pub fn offset_y(&self) -> i16 { self.offset_y }
    pub fn drag_anchor(&self) -> Option<DragAnchor> { self.drag_anchor }

    pub fn set_offset(&mut self, x: i16, y: i16) {
        self.offset_x = x;
        self.offset_y = y;
    }

    pub fn begin_drag(&mut self, anchor: DragAnchor) {
        self.drag_anchor = Some(anchor);
    }

    pub fn end_drag(&mut self) {
        self.drag_anchor = None;
    }

    pub fn grow(&mut self) {
        self.width_pct = (self.width_pct + Self::SIZE_STEP).min(Self::SIZE_MAX);
        self.height_pct = (self.height_pct + Self::SIZE_STEP).min(Self::SIZE_MAX);
    }

    pub fn shrink(&mut self) {
        self.width_pct = self
            .width_pct
            .saturating_sub(Self::SIZE_STEP)
            .max(Self::SIZE_MIN);
        self.height_pct = self
            .height_pct
            .saturating_sub(Self::SIZE_STEP)
            .max(Self::SIZE_MIN);
    }

    pub fn reset_size(&mut self) {
        self.width_pct = Self::DEFAULT_WIDTH_PCT;
        self.height_pct = Self::DEFAULT_HEIGHT_PCT;
        self.offset_x = 0;
        self.offset_y = 0;
    }
}
```

- [ ] **Step 4: Update `metrics_modal_rects` signature with offset + clamp**

Replace the function in `src/ui/metrics.rs:194-210` with:

```rust
pub fn metrics_modal_rects(
    viewport: Rect,
    width_pct: u16,
    height_pct: u16,
    offset_x: i16,
    offset_y: i16,
) -> MetricsModalRects {
    let base = centered_rect(width_pct, height_pct, viewport);
    let area = apply_clamped_offset(base, viewport, offset_x, offset_y);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // banner
            Constraint::Min(6),    // body
            Constraint::Length(1), // hint
        ])
        .split(area);
    MetricsModalRects {
        area,
        banner: chunks[0],
        body: chunks[1],
        hint: chunks[2],
    }
}

/// Apply `offset_x`/`offset_y` to `base`, clamping so at least 4 cells
/// horizontally and 1 row vertically of the modal stay inside `viewport`.
/// Pure helper — used by `metrics_modal_rects` and tested directly.
fn apply_clamped_offset(base: Rect, viewport: Rect, offset_x: i16, offset_y: i16) -> Rect {
    let min_x = (viewport.x as i32) + 4 - (base.width as i32);
    let max_x = (viewport.x as i32) + (viewport.width as i32) - 4;
    let min_y = viewport.y as i32;
    let max_y = (viewport.y as i32) + (viewport.height as i32) - 1;

    let x = ((base.x as i32) + (offset_x as i32)).clamp(min_x, max_x.max(min_x));
    let y = ((base.y as i32) + (offset_y as i32)).clamp(min_y, max_y.max(min_y));

    Rect::new(
        x.max(0) as u16,
        y.max(0) as u16,
        base.width.min(viewport.width.saturating_sub((x - viewport.x as i32).max(0) as u16).max(1)),
        base.height.min(viewport.height.saturating_sub((y - viewport.y as i32).max(0) as u16).max(1)),
    )
}
```

Note: the width / height shrink in the `Rect::new` call is defensive — it prevents a sub-zero rect when the offset pushes the modal so far that the visible portion is smaller than `base`. It is not strictly required for the spec's clamp guarantee, but keeps `metrics_modal_rects` returning a valid `Rect`.

- [ ] **Step 5: Update existing call sites + tests to the new signature**

In `src/ui/metrics.rs:823`, change:

```rust
let rects = metrics_modal_rects(frame.area(), overlay.width_pct(), overlay.height_pct());
```

to:

```rust
let rects = metrics_modal_rects(
    frame.area(),
    overlay.width_pct(),
    overlay.height_pct(),
    overlay.offset_x(),
    overlay.offset_y(),
);
```

In `src/ui/metrics.rs:890`:

```rust
let r = metrics_modal_rects(viewport, 95, 90);
```

becomes:

```rust
let r = metrics_modal_rects(viewport, 95, 90, 0, 0);
```

Same change at `src/ui/metrics.rs:903`.

In `src/app/metrics_overlay.rs:51`, change:

```rust
let rects = metrics_modal_rects(viewport, overlay.width_pct(), overlay.height_pct());
```

to:

```rust
let rects = metrics_modal_rects(
    viewport,
    overlay.width_pct(),
    overlay.height_pct(),
    overlay.offset_x(),
    overlay.offset_y(),
);
```

Also in `src/app/metrics_overlay.rs` test at line ~103:

```rust
let rects = metrics_modal_rects(viewport, o.width_pct(), o.height_pct());
```

becomes:

```rust
let rects = metrics_modal_rects(
    viewport,
    o.width_pct(),
    o.height_pct(),
    o.offset_x(),
    o.offset_y(),
);
```

Search for any remaining `metrics_modal_rects(` call sites with `grep -rn 'metrics_modal_rects(' src/ tests/` and apply the same 4-arg → 5-arg change.

- [ ] **Step 6: Add reset-size offset test inside `MetricsOverlay` impl tests**

Append inside the same `#[cfg(test)] mod tests` block:

```rust
#[test]
fn reset_size_also_zeros_offset() {
    let mut o = MetricsOverlay::new();
    o.set_offset(7, 4);
    assert_eq!(o.offset_x(), 7);
    assert_eq!(o.offset_y(), 4);
    o.reset_size();
    assert_eq!(o.offset_x(), 0);
    assert_eq!(o.offset_y(), 0);
    assert_eq!(o.width_pct(), MetricsOverlay::DEFAULT_WIDTH_PCT);
    assert_eq!(o.height_pct(), MetricsOverlay::DEFAULT_HEIGHT_PCT);
}

#[test]
fn close_preserves_offset_and_size() {
    let mut o = MetricsOverlay::new();
    o.open();
    o.set_offset(3, -2);
    o.grow();
    let (w, h, ox, oy) = (o.width_pct(), o.height_pct(), o.offset_x(), o.offset_y());
    o.close();
    o.open();
    assert_eq!(o.width_pct(), w);
    assert_eq!(o.height_pct(), h);
    assert_eq!(o.offset_x(), ox);
    assert_eq!(o.offset_y(), oy);
}
```

- [ ] **Step 7: Run tests + clippy + fmt**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib metrics
```

Expected: all tests pass (including the 8 new ones added in Steps 1 and 6). If any test fails because the offset clamp pushes the rect off-screen unexpectedly, re-check the clamp formula in Step 4 against §4.2 of the spec.

- [ ] **Step 8: Commit**

```bash
git add src/ui/metrics.rs src/app/metrics_overlay.rs
git commit -m "v1.40 m overlay: offset+clamp scaffolding for drag-to-move"
```

---

## Task 2: m overlay drag state machine + `=` reset wiring

**Files:**

- Modify: `src/app/metrics_overlay.rs:42-63` (`handle_metrics_overlay_mouse`).
- Add: tests in the same file.

- [ ] **Step 1: Write failing drag state machine tests**

Append inside the existing `#[cfg(test)] mod tests` block in `src/app/metrics_overlay.rs`:

```rust
fn mouse_event(
    kind: MouseEventKind,
    column: u16,
    row: u16,
) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::empty(),
    }
}

#[test]
fn drag_down_on_title_row_sets_anchor() {
    let mut o = MetricsOverlay::new();
    o.open();
    let viewport = Rect::new(0, 0, 200, 80);
    let rects = metrics_modal_rects(
        viewport,
        o.width_pct(),
        o.height_pct(),
        o.offset_x(),
        o.offset_y(),
    );
    let title_row = rects.area.y;
    // pick a title-bar column that's not the [x] close button
    let title_col = rects.area.x + rects.area.width / 2;

    let event = mouse_event(MouseEventKind::Down(MouseButton::Left), title_col, title_row);
    handle_metrics_overlay_mouse(&mut o, viewport, 0, event);

    let anchor = o.drag_anchor().expect("title-row click must set anchor");
    assert_eq!(anchor.start_col, title_col);
    assert_eq!(anchor.start_row, title_row);
    assert_eq!(anchor.start_offset_x, 0);
    assert_eq!(anchor.start_offset_y, 0);
}

#[test]
fn drag_event_updates_offset_relative_to_anchor() {
    let mut o = MetricsOverlay::new();
    o.open();
    let viewport = Rect::new(0, 0, 200, 80);
    let rects = metrics_modal_rects(
        viewport,
        o.width_pct(),
        o.height_pct(),
        o.offset_x(),
        o.offset_y(),
    );
    let title_row = rects.area.y;
    let title_col = rects.area.x + 5;

    handle_metrics_overlay_mouse(
        &mut o,
        viewport,
        0,
        mouse_event(MouseEventKind::Down(MouseButton::Left), title_col, title_row),
    );
    handle_metrics_overlay_mouse(
        &mut o,
        viewport,
        0,
        mouse_event(MouseEventKind::Drag(MouseButton::Left), title_col + 7, title_row + 3),
    );

    assert_eq!(o.offset_x(), 7);
    assert_eq!(o.offset_y(), 3);
    assert!(o.drag_anchor().is_some(), "anchor stays during active drag");
}

#[test]
fn up_event_clears_anchor() {
    let mut o = MetricsOverlay::new();
    o.open();
    let viewport = Rect::new(0, 0, 200, 80);
    let rects = metrics_modal_rects(
        viewport,
        o.width_pct(),
        o.height_pct(),
        o.offset_x(),
        o.offset_y(),
    );
    let title_row = rects.area.y;
    let title_col = rects.area.x + 5;

    handle_metrics_overlay_mouse(
        &mut o,
        viewport,
        0,
        mouse_event(MouseEventKind::Down(MouseButton::Left), title_col, title_row),
    );
    handle_metrics_overlay_mouse(
        &mut o,
        viewport,
        0,
        mouse_event(MouseEventKind::Up(MouseButton::Left), title_col + 2, title_row),
    );
    assert!(o.drag_anchor().is_none());
}

#[test]
fn drag_does_not_start_in_body() {
    let mut o = MetricsOverlay::new();
    o.open();
    let viewport = Rect::new(0, 0, 200, 80);
    let rects = metrics_modal_rects(
        viewport,
        o.width_pct(),
        o.height_pct(),
        o.offset_x(),
        o.offset_y(),
    );
    let body_col = rects.area.x + rects.area.width / 2;
    let body_row = rects.area.y + rects.area.height / 2;
    handle_metrics_overlay_mouse(
        &mut o,
        viewport,
        0,
        mouse_event(MouseEventKind::Down(MouseButton::Left), body_col, body_row),
    );
    assert!(o.drag_anchor().is_none());
}

#[test]
fn drag_does_not_start_on_close_button_and_close_wins() {
    let mut o = MetricsOverlay::new();
    o.open();
    let viewport = Rect::new(0, 0, 200, 80);
    let rects = metrics_modal_rects(
        viewport,
        o.width_pct(),
        o.height_pct(),
        o.offset_x(),
        o.offset_y(),
    );
    let close = crate::ui::dashboard::close_button_rect(rects.area);
    handle_metrics_overlay_mouse(
        &mut o,
        viewport,
        0,
        mouse_event(MouseEventKind::Down(MouseButton::Left), close.x, close.y),
    );
    assert!(!o.is_open(), "close-button click closes the overlay");
    assert!(o.drag_anchor().is_none());
}
```

- [ ] **Step 2: Run new tests to confirm they fail**

```bash
cargo test --lib app::metrics_overlay
```

Expected: at least one of `drag_down_on_title_row_sets_anchor`, `drag_event_updates_offset_relative_to_anchor`, `up_event_clears_anchor` fails because the existing handler doesn't manipulate drag state.

- [ ] **Step 3: Implement the drag state machine**

Replace `handle_metrics_overlay_mouse` (`src/app/metrics_overlay.rs:42-63`) with:

```rust
pub fn handle_metrics_overlay_mouse(
    overlay: &mut MetricsOverlay,
    viewport: Rect,
    _reports_len: usize,
    event: MouseEvent,
) {
    if !overlay.is_open() {
        return;
    }
    let rects = metrics_modal_rects(
        viewport,
        overlay.width_pct(),
        overlay.height_pct(),
        overlay.offset_x(),
        overlay.offset_y(),
    );
    let close = close_button_rect(rects.area);

    // 1. Close button always wins.
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && rect_contains(close, event.column, event.row)
    {
        overlay.close();
        return;
    }

    // 2. Drag start: Down(Left) on title row outside [x].
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && event.row == rects.area.y
        && event.column >= rects.area.x
        && event.column < rects.area.x + rects.area.width
        && !rect_contains(close, event.column, event.row)
    {
        overlay.begin_drag(crate::ui::metrics::DragAnchor {
            start_col: event.column,
            start_row: event.row,
            start_offset_x: overlay.offset_x(),
            start_offset_y: overlay.offset_y(),
        });
        return;
    }

    // 3. Drag update: Drag(Left) while anchored.
    if let MouseEventKind::Drag(MouseButton::Left) = event.kind {
        if let Some(anchor) = overlay.drag_anchor() {
            let dx = (event.column as i32) - (anchor.start_col as i32);
            let dy = (event.row as i32) - (anchor.start_row as i32);
            let new_x = (anchor.start_offset_x as i32 + dx).clamp(i16::MIN as i32, i16::MAX as i32);
            let new_y = (anchor.start_offset_y as i32 + dy).clamp(i16::MIN as i32, i16::MAX as i32);
            overlay.set_offset(new_x as i16, new_y as i16);
            return;
        }
    }

    // 4. Drag end: Up(Left) or any non-Drag mouse event clears anchor.
    if !matches!(event.kind, MouseEventKind::Drag(_)) {
        overlay.end_drag();
    }

    // 5. Wheel scroll (existing behavior, unchanged).
    match event.kind {
        MouseEventKind::ScrollUp => overlay.scroll_up(),
        MouseEventKind::ScrollDown => overlay.scroll_down(SCROLL_MAX),
        _ => {}
    }
}
```

Make sure the file's `use` block at the top includes `MouseButton`:

```rust
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
```

(The existing `use` already covers this — verify but do not add duplicates.)

- [ ] **Step 4: Run all tests**

```bash
cargo test --lib
```

Expected: drag tests pass, the existing `x_click_closes_overlay` test still passes, and the existing `arrows_scroll_body` / `bracket_keys_resize_overlay` tests still pass.

- [ ] **Step 5: Lint + format**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/app/metrics_overlay.rs
git commit -m "v1.40 m overlay: drag-to-move state machine on title row"
```

---

## Task 3: m overlay UI_MANUAL §8.5 update

**Files:**

- Modify: `docs/ai/UI_MANUAL.md` §8.5 (around line 645–650, after the existing `[`/`]`/`=` resize description).

- [ ] **Step 1: Locate the §8.5 section**

```bash
grep -n "운영자가 고른 크기는 close" docs/ai/UI_MANUAL.md
```

Expected: one hit on the line that ends “close 후 재오픈에도 유지.”

- [ ] **Step 2: Add the drag paragraph**

Open `docs/ai/UI_MANUAL.md` and replace the existing line:

```
- `[`: modal 5% 축소 (50%까지). `]`: 5% 확대 (99%까지). `=`: 기본
  95×90으로 reset. 폭·높이 동시 적용. 운영자가 고른 크기는 close 후
  재오픈에도 유지.
```

with:

```
- `[`: modal 5% 축소 (50%까지). `]`: 5% 확대 (99%까지). `=`: 기본
  95×90으로 reset하면서 드래그 위치도 함께 (0, 0)로 reset. 폭·높이
  동시 적용. 운영자가 고른 크기·위치는 close 후 재오픈에도 유지
  (Qmonster 재시작 시에는 기본값으로 초기화).
- 마우스로 모달 **상단 제목 줄**(`[x]` 닫기 버튼 영역 제외)을 드래그하면
  modal 위치를 옮길 수 있습니다. 드래그가 viewport를 벗어나도 가로 4셀 /
  세로 1셀은 항상 화면에 남도록 자동 클램프됩니다 (터미널 리사이즈 시에도
  같은 안전 영역으로 끌려옴). 드래그를 멈추면 (마우스 버튼을 떼면) 위치가
  고정됩니다.
```

- [ ] **Step 3: Verify Markdown still parses cleanly**

```bash
grep -n "마우스로 모달" docs/ai/UI_MANUAL.md
```

Expected: one hit on the new line.

- [ ] **Step 4: Commit**

```bash
git add docs/ai/UI_MANUAL.md
git commit -m "v1.40 docs: UI_MANUAL §8.5 m overlay drag-to-move"
```

---

# Phase B — a overlay state foundations

## Task 4: Stable item key + multi-select state + group toggles

**Files:**

- Modify: `src/ui/pending_actions.rs:32-70` (struct + impl), add free `pending_item_key` and group helpers.

- [ ] **Step 1: Write failing key + multi-select tests**

Append inside the existing `#[cfg(test)] mod tests` block in `src/ui/pending_actions.rs`:

```rust
#[test]
fn pending_item_key_proposal_uses_proposal_id() {
    use crate::domain::origin::SourceKind;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    assert_eq!(pending_item_key(&item), "p:%1:/compact");
}

#[test]
fn pending_item_key_copy_uses_title_command() {
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;
    let item = PendingItem::Copy {
        alert_idx: 0,
        command: "/clear".into(),
        alert_title: "context-pressure".into(),
        severity: Severity::Warning,
        source: SourceKind::Estimated,
        pane_idx: None,
    };
    let key = pending_item_key(&item);
    assert!(key.starts_with("y:context-pressure"));
    assert!(key.ends_with("/clear"));
}

#[test]
fn toggle_multi_adds_then_removes() {
    use crate::domain::origin::SourceKind;
    let mut o = PendingActionsOverlay::new();
    o.open();
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let key = pending_item_key(&item);
    o.toggle_multi(&item);
    assert!(o.multi_contains(&key));
    o.toggle_multi(&item);
    assert!(!o.multi_contains(&key));
}

#[test]
fn close_clears_multi_selected() {
    use crate::domain::origin::SourceKind;
    let mut o = PendingActionsOverlay::new();
    o.open();
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    o.toggle_multi(&item);
    o.close();
    assert_eq!(o.multi_len(), 0);
}

#[test]
fn group_toggle_p_selects_then_deselects_all_proposals() {
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;
    let proposal = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let copy = PendingItem::Copy {
        alert_idx: 0,
        command: "/clear".into(),
        alert_title: "context-pressure".into(),
        severity: Severity::Warning,
        source: SourceKind::Estimated,
        pane_idx: None,
    };
    let items = vec![proposal, copy];

    let mut o = PendingActionsOverlay::new();
    o.open();
    o.toggle_group_proposals(&items);
    assert_eq!(o.multi_len(), 1, "all proposals selected");
    o.toggle_group_proposals(&items);
    assert_eq!(o.multi_len(), 0, "all proposals deselected");
}

#[test]
fn group_toggle_a_selects_then_deselects_all_items() {
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;
    let proposal = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let copy = PendingItem::Copy {
        alert_idx: 0,
        command: "/clear".into(),
        alert_title: "context-pressure".into(),
        severity: Severity::Warning,
        source: SourceKind::Estimated,
        pane_idx: None,
    };
    let items = vec![proposal, copy];
    let mut o = PendingActionsOverlay::new();
    o.open();
    o.toggle_group_all(&items);
    assert_eq!(o.multi_len(), 2);
    o.toggle_group_all(&items);
    assert_eq!(o.multi_len(), 0);
}

#[test]
fn auto_prune_drops_keys_no_longer_in_items() {
    use crate::domain::origin::SourceKind;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let mut o = PendingActionsOverlay::new();
    o.open();
    o.toggle_multi(&item);
    assert_eq!(o.multi_len(), 1);
    // items now empty (the proposal vanished between polls)
    o.prune_to(&[]);
    assert_eq!(o.multi_len(), 0);
}
```

- [ ] **Step 2: Run new tests to confirm they fail**

```bash
cargo test --lib pending_actions
```

Expected: compile errors — `pending_item_key`, `toggle_multi`, `multi_contains`, `multi_len`, `toggle_group_proposals`, `toggle_group_all`, `prune_to` do not exist yet.

- [ ] **Step 3: Update struct + impl with multi-select set**

Replace the existing struct + impl in `src/ui/pending_actions.rs:32-70` with:

```rust
use std::collections::BTreeSet;

#[derive(Debug, Default, Clone)]
pub struct PendingActionsOverlay {
    open: bool,
    selected: usize,
    multi_selected: BTreeSet<String>,
}

impl PendingActionsOverlay {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
        // multi_selected stays empty (default already empty); explicit no-op.
    }
    pub fn close(&mut self) {
        self.open = false;
        self.selected = 0;
        self.multi_selected.clear();
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn set_selected(&mut self, idx: usize) {
        self.selected = idx;
    }
    pub fn select_next(&mut self, total: usize) {
        if total == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected.saturating_add(1);
        self.selected = next.min(total.saturating_sub(1));
    }
    pub fn select_prev(&mut self, _total: usize) {
        self.selected = self.selected.saturating_sub(1);
    }

    // --- multi-select ----------------------------------------------------

    pub fn multi_len(&self) -> usize {
        self.multi_selected.len()
    }
    pub fn multi_contains(&self, key: &str) -> bool {
        self.multi_selected.contains(key)
    }
    pub fn multi_keys(&self) -> impl Iterator<Item = &String> {
        self.multi_selected.iter()
    }
    pub fn clear_multi(&mut self) {
        self.multi_selected.clear();
    }
    pub fn toggle_multi(&mut self, item: &PendingItem) {
        let key = pending_item_key(item);
        if !self.multi_selected.remove(&key) {
            self.multi_selected.insert(key);
        }
    }

    /// Toggle all proposal items: select-all if not all are selected, otherwise clear-all.
    pub fn toggle_group_proposals(&mut self, items: &[PendingItem]) {
        self.toggle_group_filtered(items, |it| matches!(it, PendingItem::Proposal { .. }));
    }
    /// Toggle all alert (copy) items.
    pub fn toggle_group_alerts(&mut self, items: &[PendingItem]) {
        self.toggle_group_filtered(items, |it| matches!(it, PendingItem::Copy { .. }));
    }
    /// Toggle every item in the list.
    pub fn toggle_group_all(&mut self, items: &[PendingItem]) {
        self.toggle_group_filtered(items, |_| true);
    }

    fn toggle_group_filtered(
        &mut self,
        items: &[PendingItem],
        filter: impl Fn(&PendingItem) -> bool,
    ) {
        let group_keys: Vec<String> = items
            .iter()
            .filter(|it| filter(it))
            .map(pending_item_key)
            .collect();
        if group_keys.is_empty() {
            return;
        }
        let all_selected = group_keys.iter().all(|k| self.multi_selected.contains(k));
        if all_selected {
            for k in &group_keys {
                self.multi_selected.remove(k);
            }
        } else {
            for k in group_keys {
                self.multi_selected.insert(k);
            }
        }
    }

    /// Drop multi-select keys whose item is not in `items`.
    pub fn prune_to(&mut self, items: &[PendingItem]) {
        let live: BTreeSet<String> = items.iter().map(pending_item_key).collect();
        self.multi_selected.retain(|k| live.contains(k));
    }
}

/// Stable string key for a `PendingItem`, used to track multi-select
/// across polling refreshes. `\u{1F}` (unit separator) prevents `:`
/// collisions in alert titles that contain colons.
pub fn pending_item_key(item: &PendingItem) -> String {
    match item {
        PendingItem::Proposal { proposal_id, .. } => format!("p:{proposal_id}"),
        PendingItem::Copy {
            alert_title,
            command,
            ..
        } => format!("y:{alert_title}\u{1F}{command}"),
    }
}
```

Remove the now-redundant `use std::collections::{HashMap, HashSet};` if those are only used elsewhere — `grep -n "HashSet\|HashMap" src/ui/pending_actions.rs` and prune unused imports. (BTreeSet is the only new import.)

- [ ] **Step 4: Run tests**

```bash
cargo test --lib pending_actions
```

Expected: 7 new tests pass. Existing tests in the file still pass (open/close round-trip, select_clamps, etc).

- [ ] **Step 5: Lint + format**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

- [ ] **Step 6: Commit**

```bash
git add src/ui/pending_actions.rs
git commit -m "v1.40 a overlay: stable-key multi-select state + group toggles"
```

---

## Task 5: a overlay split-layout helper + modal size bump

**Files:**

- Modify: `src/ui/pending_actions.rs` — bump `pending_actions_modal_area`, add `pending_actions_modal_rects`.

- [ ] **Step 1: Write failing layout tests**

Append inside the existing `#[cfg(test)] mod tests` block in `src/ui/pending_actions.rs`:

```rust
#[test]
fn modal_area_uses_80x65_with_min_72_20() {
    let r = pending_actions_modal_area(Rect::new(0, 0, 200, 80));
    assert_eq!(r.width, 200 * 80 / 100);
    assert_eq!(r.height, 80 * 65 / 100);

    // Tiny viewport falls back to the minimum.
    let small = pending_actions_modal_area(Rect::new(0, 0, 60, 18));
    // viewport.width=60 < 72 min: clamps to viewport width
    assert_eq!(small.width, 60);
    // viewport.height=18 < 20 min: clamps to viewport height
    assert_eq!(small.height, 18);
}

#[test]
fn modal_rects_wide_mode_splits_vertically_with_32_col_list() {
    let viewport = Rect::new(0, 0, 200, 80);
    let rects = pending_actions_modal_rects(viewport);
    // Wide mode: list width = 32, separator = 1 col, explainer = rest.
    assert_eq!(rects.list.width, 32);
    let sep_col = rects.list.x + rects.list.width;
    assert_eq!(rects.explainer.x, sep_col + 1);
    assert_eq!(
        rects.explainer.x + rects.explainer.width,
        rects.area.x + rects.area.width - 1, // -1: right border
        "explainer extends to the right border"
    );
    // hint is the bottom row inside borders.
    assert_eq!(rects.hint.height, 1);
    assert_eq!(rects.hint.y, rects.area.y + rects.area.height - 2);
}

#[test]
fn modal_rects_narrow_mode_splits_horizontally() {
    // body.width < 72 → list on top, explainer on bottom.
    let viewport = Rect::new(0, 0, 80, 30);
    let rects = pending_actions_modal_rects(viewport);
    let body_width = rects.area.width - 2; // minus borders
    assert!(body_width < 72, "this test requires narrow body width");
    // list and explainer share the body height (minus hint).
    assert_eq!(rects.list.x, rects.explainer.x);
    assert_eq!(rects.list.width, rects.explainer.width);
    assert!(rects.list.height >= 1);
    assert!(rects.explainer.height >= 1);
    assert_eq!(rects.list.y + rects.list.height, rects.explainer.y);
}
```

- [ ] **Step 2: Run new tests to confirm they fail**

```bash
cargo test --lib pending_actions::tests::modal
```

Expected: compile failures — `pending_actions_modal_rects` and the new struct don't exist yet, and the size assertions don't match.

- [ ] **Step 3: Bump modal size + add the split helper**

In `src/ui/pending_actions.rs`, replace the existing `pending_actions_modal_area`:

```rust
pub(crate) fn pending_actions_modal_area(viewport: Rect) -> Rect {
    let width = (viewport.width * 80 / 100).max(72).min(viewport.width);
    let height = (viewport.height * 65 / 100).max(20).min(viewport.height);
    let x = viewport.x + viewport.width.saturating_sub(width) / 2;
    let y = viewport.y + viewport.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

/// Inner partition of the modal: list on the left (or top), explainer
/// on the right (or bottom), one-row hint at the very bottom inside
/// borders. Wide mode kicks in when the body width (modal width minus
/// 2 for left/right borders) is at least 72 cells.
#[derive(Debug, Clone, Copy)]
pub struct PendingActionsModalRects {
    pub area: Rect,
    pub list: Rect,
    pub explainer: Rect,
    pub hint: Rect,
}

const LIST_WIDTH_WIDE: u16 = 32;
const SPLIT_THRESHOLD_WIDE: u16 = 72;

pub(crate) fn pending_actions_modal_rects(viewport: Rect) -> PendingActionsModalRects {
    let area = pending_actions_modal_area(viewport);
    // Carve a 1-col / 1-row inset for the modal border (Borders::ALL).
    let inner_x = area.x + 1;
    let inner_y = area.y + 1;
    let inner_w = area.width.saturating_sub(2);
    let inner_h = area.height.saturating_sub(2);

    // hint = last row inside the inner area.
    let hint = Rect::new(inner_x, inner_y + inner_h.saturating_sub(1), inner_w, 1);
    let body_h = inner_h.saturating_sub(1);
    let body = Rect::new(inner_x, inner_y, inner_w, body_h);

    let (list, explainer) = if body.width >= SPLIT_THRESHOLD_WIDE {
        // Wide: list 32 cols, 1-col separator, explainer = rest.
        let list = Rect::new(body.x, body.y, LIST_WIDTH_WIDE, body.height);
        let exp_x = body.x + LIST_WIDTH_WIDE + 1;
        let exp_w = body.width.saturating_sub(LIST_WIDTH_WIDE + 1);
        let explainer = Rect::new(exp_x, body.y, exp_w, body.height);
        (list, explainer)
    } else {
        // Narrow: list top half, explainer bottom half.
        let half = body.height / 2;
        let list = Rect::new(body.x, body.y, body.width, half);
        let explainer = Rect::new(body.x, body.y + half, body.width, body.height - half);
        (list, explainer)
    };

    PendingActionsModalRects {
        area,
        list,
        explainer,
        hint,
    }
}
```

- [ ] **Step 4: Run new tests**

```bash
cargo test --lib pending_actions::tests::modal
```

Expected: 3 new tests pass.

- [ ] **Step 5: Lint + format**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

- [ ] **Step 6: Commit**

```bash
git add src/ui/pending_actions.rs
git commit -m "v1.40 a overlay: 80x65 modal + split layout helper (list/explainer/hint)"
```

---

## Task 6: live-explainer view builder + per-key action constructors

**Files:**

- Modify: `src/app/pending_actions_overlay.rs` — add helpers.

**Background (read first)**: The actual `PendingAction` enum in `src/app/action_explainer.rs:23-42` has three variants and the build-view fns have specific signatures:

```rust
pub enum PendingAction {
    AcceptPromptSend { target_pane_id: String, slash_command: String, proposal_id: String },
    RejectPromptSend { target_pane_id: String, slash_command: String, proposal_id: String },
    CopyAlertCommand { command: String, alert_title: String },
}

pub fn build_accept_view(report: &PaneReport, slash: &str, mode: ActionsMode, allow_auto: bool) -> ActionExplainView;
pub fn build_reject_view(report: &PaneReport, slash: &str) -> ActionExplainView;
pub fn build_copy_view(alert_title: &str, command: &str, severity: Option<Severity>, source: SourceKind) -> ActionExplainView;
```

The live explainer view for a proposal cursor item therefore needs the live `&PaneReport` (looked up by `pane_idx`), plus `mode` and `allow_auto_prompt_send` for the "Mode now" warning. We thread those through from `tui_loop` at render time.

- [ ] **Step 1: Write failing tests for `build_explainer_view_for_item` and the per-key action constructors**

Append to the existing `#[cfg(test)] mod tests` block in `src/app/pending_actions_overlay.rs`:

```rust
#[test]
fn accept_action_for_proposal_returns_accept_variant() {
    use crate::app::action_explainer::PendingAction;
    use crate::domain::origin::SourceKind;
    use crate::ui::pending_actions::PendingItem;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let action = accept_action_for(&item).expect("proposal must produce an Accept action");
    match action {
        PendingAction::AcceptPromptSend { target_pane_id, slash_command, proposal_id } => {
            assert_eq!(target_pane_id, "%1");
            assert_eq!(slash_command, "/compact");
            assert_eq!(proposal_id, "%1:/compact");
        }
        _ => panic!("expected AcceptPromptSend"),
    }
}

#[test]
fn accept_action_for_alert_returns_none() {
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;
    use crate::ui::pending_actions::PendingItem;
    let item = PendingItem::Copy {
        alert_idx: 0,
        command: "/clear".into(),
        alert_title: "ctx".into(),
        severity: Severity::Warning,
        source: SourceKind::Estimated,
        pane_idx: None,
    };
    assert!(accept_action_for(&item).is_none());
}

#[test]
fn reject_action_for_proposal_returns_reject_variant() {
    use crate::app::action_explainer::PendingAction;
    use crate::domain::origin::SourceKind;
    use crate::ui::pending_actions::PendingItem;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let action = reject_action_for(&item).expect("proposal must produce a Reject action");
    assert!(matches!(action, PendingAction::RejectPromptSend { .. }));
}

#[test]
fn copy_action_for_alert_returns_copy_variant() {
    use crate::app::action_explainer::PendingAction;
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;
    use crate::ui::pending_actions::PendingItem;
    let item = PendingItem::Copy {
        alert_idx: 0,
        command: "/clear".into(),
        alert_title: "ctx".into(),
        severity: Severity::Warning,
        source: SourceKind::Estimated,
        pane_idx: None,
    };
    let action = copy_action_for(&item).expect("alert must produce a Copy action");
    match action {
        PendingAction::CopyAlertCommand { command, alert_title } => {
            assert_eq!(command, "/clear");
            assert_eq!(alert_title, "ctx");
        }
        _ => panic!("expected CopyAlertCommand"),
    }
}
```

The view-builder is exercised indirectly in Task 8's renderer tests — no separate unit test needed here.

- [ ] **Step 2: Run the new tests to confirm they fail**

```bash
cargo test --lib app::pending_actions_overlay::tests::accept_action_for_proposal_returns_accept_variant
```

Expected: compile error — `accept_action_for`, `reject_action_for`, `copy_action_for` do not exist.

- [ ] **Step 3: Add the helpers**

Append to `src/app/pending_actions_overlay.rs` (at the bottom of the module, before `#[cfg(test)]`):

```rust
use crate::app::action_explainer::{
    ActionExplainView, PendingAction, build_accept_view, build_copy_view,
};
use crate::app::config::ActionsMode;
use crate::app::event_loop::PaneReport;
use crate::ui::pending_actions::PendingItem;

/// Build the `AcceptPromptSend` action for a proposal item, or `None`
/// if the item is not a proposal. Used by the bulk `p` dispatch in
/// `tui_loop`.
pub fn accept_action_for(item: &PendingItem) -> Option<PendingAction> {
    match item {
        PendingItem::Proposal {
            target_pane_id,
            slash_command,
            proposal_id,
            ..
        } => Some(PendingAction::AcceptPromptSend {
            target_pane_id: target_pane_id.clone(),
            slash_command: slash_command.clone(),
            proposal_id: proposal_id.clone(),
        }),
        PendingItem::Copy { .. } => None,
    }
}

/// Build the `RejectPromptSend` action for a proposal item, or `None`
/// for non-proposal items. Used by the bulk `d` dispatch when the
/// item is a proposal (alerts use the hide path instead).
pub fn reject_action_for(item: &PendingItem) -> Option<PendingAction> {
    match item {
        PendingItem::Proposal {
            target_pane_id,
            slash_command,
            proposal_id,
            ..
        } => Some(PendingAction::RejectPromptSend {
            target_pane_id: target_pane_id.clone(),
            slash_command: slash_command.clone(),
            proposal_id: proposal_id.clone(),
        }),
        PendingItem::Copy { .. } => None,
    }
}

/// Build the `CopyAlertCommand` action for an alert item, or `None`
/// for proposals. Used by the bulk `y` dispatch.
pub fn copy_action_for(item: &PendingItem) -> Option<PendingAction> {
    match item {
        PendingItem::Copy {
            command,
            alert_title,
            ..
        } => Some(PendingAction::CopyAlertCommand {
            command: command.clone(),
            alert_title: alert_title.clone(),
        }),
        PendingItem::Proposal { .. } => None,
    }
}

/// Build an `ActionExplainView` for the right-pane live panel. For
/// proposals this is the accept-flavored view (the "primary" action a
/// `p` keypress would do); the operator can still press `d` to reject
/// without changing what the panel shows. Returns `None` when the
/// caller should render the empty-state placeholder.
pub fn build_explainer_view_for_item(
    item: &PendingItem,
    reports: &[PaneReport],
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
) -> Option<ActionExplainView> {
    match item {
        PendingItem::Proposal {
            pane_idx,
            slash_command,
            ..
        } => {
            let report = reports.get(*pane_idx)?;
            Some(build_accept_view(
                report,
                slash_command,
                mode,
                allow_auto_prompt_send,
            ))
        }
        PendingItem::Copy {
            command,
            alert_title,
            severity,
            source,
            ..
        } => Some(build_copy_view(
            alert_title,
            command,
            Some(*severity),
            *source,
        )),
    }
}
```

- [ ] **Step 4: Run the new tests**

```bash
cargo test --lib app::pending_actions_overlay
```

Expected: 4 new tests pass. Existing tests still pass.

- [ ] **Step 5: Lint + format**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

- [ ] **Step 6: Commit**

```bash
git add src/app/pending_actions_overlay.rs
git commit -m "v1.40 a overlay: per-key action constructors + live-explainer view builder"
```

---

# Phase C — a overlay rendering

## Task 7: New row layout — checkbox + cursor + content

**Files:**

- Modify: `src/ui/pending_actions.rs:279-345` (`pending_actions_lines`).

- [ ] **Step 1: Write failing row-layout tests**

Append to the test module:

```rust
#[test]
fn lines_render_checkbox_unchecked_for_non_multi() {
    use crate::domain::origin::SourceKind;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let overlay = PendingActionsOverlay::new();
    let lines = pending_actions_lines(&overlay, &[item]);
    let dump: String = lines.iter().map(|l| line_text(l) + "\n").collect();
    assert!(dump.contains("[ ]"), "non-multi row must render `[ ]`: {dump}");
    assert!(dump.contains("\u{25B6}"), "first row gets the cursor mark: {dump}");
}

#[test]
fn lines_render_checkbox_checked_when_multi_selected() {
    use crate::domain::origin::SourceKind;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let mut overlay = PendingActionsOverlay::new();
    overlay.toggle_multi(&item);
    let lines = pending_actions_lines(&overlay, &[item]);
    let dump: String = lines.iter().map(|l| line_text(l) + "\n").collect();
    assert!(dump.contains("[x]"), "multi-selected row must render `[x]`: {dump}");
}
```

(The existing `line_text` helper in the same `mod tests` is already used by other tests.)

- [ ] **Step 2: Run new tests to confirm they fail**

```bash
cargo test --lib pending_actions::tests::lines_render_checkbox
```

Expected: assertion failures — current code does not render `[ ]` / `[x]`.

- [ ] **Step 3: Replace the row construction in `pending_actions_lines`**

Replace the body of the per-item `for (idx, item)` loop in `pending_actions_lines` (around `src/ui/pending_actions.rs:295-334`) with:

```rust
let cursor = if idx == selected { "\u{25B6} " } else { "  " };
let checked = overlay.multi_contains(&pending_item_key(item));
let checkbox = if checked { "[x]" } else { "[ ]" };
let kind = item.kind_letter();
let sev_color = item
    .severity()
    .map(theme::severity_color)
    .unwrap_or(theme::TEXT_PRIMARY);
let sev_label = item.severity().map(severity_label).unwrap_or("\u{2014}");

let checkbox_style = if checked {
    Style::default()
        .fg(theme::TEXT_PRIMARY)
        .bg(sev_color)
        .add_modifier(Modifier::BOLD)
} else {
    Style::default().fg(theme::TEXT_DIM)
};

let spans: Vec<Span<'static>> = vec![
    Span::styled(checkbox.to_string(), checkbox_style),
    Span::raw(" "),
    Span::raw(cursor.to_string()),
    Span::styled(
        format!("[{kind}]"),
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    ),
    Span::raw(" "),
    Span::styled(
        format!(" {sev_label} "),
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .bg(sev_color)
            .add_modifier(Modifier::BOLD),
    ),
    Span::raw(" \u{B7} "),
    Span::styled(
        format!("`{}`", item.command()),
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    ),
    Span::raw(" \u{B7} "),
    Span::styled(
        item.context().to_string(),
        Style::default().fg(theme::TEXT_DIM),
    ),
];
out.push(Line::from(spans));
```

The new layout: `[x] ` (cols 0-3) → `▶ ` (cols 4-5) → kind/severity/command/context (cols 6+).

- [ ] **Step 4: Run pending_actions tests**

```bash
cargo test --lib pending_actions
```

Expected: 2 new tests pass + every existing rendering test still passes (existing tests check for substrings like `[p]` and `/compact` — those still appear).

- [ ] **Step 5: Lint + format**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

- [ ] **Step 6: Commit**

```bash
git add src/ui/pending_actions.rs
git commit -m "v1.40 a overlay: row layout adds [x]/[ ] checkbox + preserved cursor mark"
```

---

## Task 8: Title + bottom hint with multi-aware counts; modal renderer rewrite

**Files:**

- Modify: `src/ui/pending_actions.rs:239-272` (`render_pending_actions_modal`), `src/ui/pending_actions.rs:340-345` (`hint_line`).

- [ ] **Step 1: Write failing title + hint tests**

Append:

```rust
#[test]
fn title_includes_selected_count_when_multi_non_empty() {
    use crate::domain::origin::SourceKind;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let mut overlay = PendingActionsOverlay::new();
    overlay.toggle_multi(&item);
    let title = pending_actions_title(&overlay, std::slice::from_ref(&item));
    assert!(title.contains("1 pending"), "{title}");
    assert!(title.contains("1 selected"), "{title}");
}

#[test]
fn title_omits_selected_count_when_empty() {
    use crate::domain::origin::SourceKind;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let overlay = PendingActionsOverlay::new();
    let title = pending_actions_title(&overlay, std::slice::from_ref(&item));
    assert!(title.contains("1 pending"));
    assert!(!title.contains("selected"), "no selected segment when multi empty");
}

#[test]
fn hint_shows_action_counts_for_cursor_proposal_no_multi() {
    use crate::domain::origin::SourceKind;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let overlay = PendingActionsOverlay::new();
    let hint = pending_actions_hint_text(&overlay, std::slice::from_ref(&item));
    assert!(hint.contains("p accept(1)"), "{hint}");
    assert!(hint.contains("d clear(1)"), "{hint}");
    assert!(hint.contains("y copy(0)"), "cursor is proposal, not alert: {hint}");
}

#[test]
fn hint_shows_multi_counts() {
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;
    let p = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let y = PendingItem::Copy {
        alert_idx: 0,
        command: "/clear".into(),
        alert_title: "context-pressure".into(),
        severity: Severity::Warning,
        source: SourceKind::Estimated,
        pane_idx: None,
    };
    let items = vec![p, y];
    let mut overlay = PendingActionsOverlay::new();
    overlay.toggle_group_all(&items);
    let hint = pending_actions_hint_text(&overlay, &items);
    assert!(hint.contains("p accept(1)"), "1 proposal in multi: {hint}");
    assert!(hint.contains("d clear(2)"), "2 items in multi: {hint}");
    assert!(hint.contains("y copy(1)"), "1 alert in multi: {hint}");
}
```

- [ ] **Step 2: Run new tests to confirm they fail**

```bash
cargo test --lib pending_actions::tests::title pending_actions::tests::hint_shows
```

Expected: compile failures — `pending_actions_title` and `pending_actions_hint_text` don't exist yet.

- [ ] **Step 3: Add the title + hint text builders**

Insert just above `render_pending_actions_modal` in `src/ui/pending_actions.rs`:

```rust
/// Top-border title, e.g. `"Pending Actions · 5 pending · 3 selected · a 다시로 닫기"`.
/// The `· N selected` segment is omitted when multi-selected is empty.
pub fn pending_actions_title(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) -> String {
    if overlay.multi_len() == 0 {
        format!(
            "Pending Actions \u{B7} {} pending \u{B7} a 다시로 닫기",
            items.len()
        )
    } else {
        format!(
            "Pending Actions \u{B7} {} pending \u{B7} {} selected \u{B7} a 다시로 닫기",
            items.len(),
            overlay.multi_len(),
        )
    }
}

/// Bottom hint line text — multi-aware action counts.
/// Returns the plain string. The renderer wraps it in dim/severity styles
/// for `(0)` segments separately.
pub fn pending_actions_hint_text(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) -> String {
    let (n_accept, n_clear, n_copy) = pending_actions_counts(overlay, items);
    format!(
        "Space toggle \u{B7} P/Y/A group \u{B7} c clear-sel \u{B7} p accept({n_accept}) \u{B7} d clear({n_clear}) \u{B7} y copy({n_copy}) \u{B7} a/Esc close"
    )
}

fn pending_actions_counts(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) -> (usize, usize, usize) {
    if overlay.multi_len() == 0 {
        let cursor = items.get(overlay.selected());
        let n_accept = match cursor {
            Some(PendingItem::Proposal { .. }) => 1,
            _ => 0,
        };
        let n_clear = if cursor.is_some() { 1 } else { 0 };
        let n_copy = match cursor {
            Some(PendingItem::Copy { .. }) => 1,
            _ => 0,
        };
        return (n_accept, n_clear, n_copy);
    }
    let mut n_accept = 0;
    let mut n_clear = 0;
    let mut n_copy_present = 0;
    for item in items {
        let key = pending_item_key(item);
        if !overlay.multi_contains(&key) {
            continue;
        }
        match item {
            PendingItem::Proposal { .. } => {
                n_accept += 1;
                n_clear += 1;
            }
            PendingItem::Copy { .. } => {
                n_copy_present += 1;
                n_clear += 1;
            }
        }
    }
    let n_copy = n_copy_present.min(1); // y dispatches the first one only
    (n_accept, n_clear, n_copy)
}
```

- [ ] **Step 4: Rewrite `render_pending_actions_modal` to use the new layout**

The renderer now takes a context struct so it can build the live explainer view. Replace the existing function (`src/ui/pending_actions.rs:239-272`) with:

```rust
pub struct PendingActionsRenderCtx<'a> {
    pub overlay: &'a PendingActionsOverlay,
    pub items: &'a [PendingItem],
    pub reports: &'a [crate::app::event_loop::PaneReport],
    pub mode: crate::app::config::ActionsMode,
    pub allow_auto_prompt_send: bool,
}

pub fn render_pending_actions_modal(frame: &mut Frame<'_>, ctx: PendingActionsRenderCtx<'_>) {
    let PendingActionsRenderCtx {
        overlay,
        items,
        reports,
        mode,
        allow_auto_prompt_send,
    } = ctx;

    let viewport = frame.area();
    let rects = pending_actions_modal_rects(viewport);
    frame.render_widget(Clear, rects.area);

    let title = pending_actions_title(overlay, items);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));
    frame.render_widget(block, rects.area);

    // List pane.
    let list_lines = pending_actions_lines(overlay, items);
    frame.render_widget(
        Paragraph::new(list_lines).wrap(Wrap { trim: false }),
        rects.list,
    );

    // Separator + explainer pane. The separator block is rendered on
    // the explainer rect (Borders::LEFT in wide mode, Borders::TOP in
    // narrow mode); the inner area then carries the Paragraph.
    let sep_block = if rects.explainer.x > rects.list.x + rects.list.width {
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme::BORDER_ACTIVE))
    } else {
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme::BORDER_ACTIVE))
    };
    let inner_explainer = sep_block.inner(rects.explainer);
    frame.render_widget(sep_block, rects.explainer);
    let explainer_lines =
        explainer_lines_for_cursor(overlay, items, reports, mode, allow_auto_prompt_send, inner_explainer);
    frame.render_widget(
        Paragraph::new(explainer_lines).wrap(Wrap { trim: false }),
        inner_explainer,
    );

    // Hint row.
    let hint_text = pending_actions_hint_text(overlay, items);
    frame.render_widget(
        Paragraph::new(hint_text).style(Style::default().fg(theme::TEXT_DIM)),
        rects.hint,
    );

    // [x] close button on the top border.
    frame.render_widget(
        Paragraph::new("[x]").style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        close_button_rect(rects.area),
    );
}

/// Right-pane explainer lines for the cursor item. Reuses
/// `build_explainer_view_for_item` from `app::pending_actions_overlay`
/// and `render_explainer_lines` from `ui::action_explainer`. Falls back
/// to a dim placeholder for empty lists or when the report cannot be
/// resolved (proposal pointing at a vanished pane_idx).
fn explainer_lines_for_cursor(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
    reports: &[crate::app::event_loop::PaneReport],
    mode: crate::app::config::ActionsMode,
    allow_auto_prompt_send: bool,
    body: Rect,
) -> Vec<Line<'static>> {
    use crate::app::pending_actions_overlay::build_explainer_view_for_item;
    use crate::ui::action_explainer::render_explainer_lines;

    let Some(cursor_item) = items.get(overlay.selected()) else {
        return vec![Line::styled(
            "Select an item to see what would happen.".to_string(),
            Style::default().fg(theme::TEXT_DIM),
        )];
    };
    let Some(view) = build_explainer_view_for_item(cursor_item, reports, mode, allow_auto_prompt_send) else {
        return vec![Line::styled(
            "(no live report available for this item)".to_string(),
            Style::default().fg(theme::TEXT_DIM),
        )];
    };
    render_explainer_lines(&view, body)
}
```

If `build_accept_view` / `build_copy_view` actually take ownership instead of `&action`, adjust the call site to clone `action` (matching the existing tui_loop call site is the truth).

The `pending_actions_lines` function keeps emitting just the data rows (no leading blank, no trailing hint), since the new layout pushes the hint into its own `rects.hint` and the leading blank into the renderer if desired. To preserve the existing 1-row top padding the mouse handler relies on (Section 5.9 of the spec), keep the existing leading blank in `pending_actions_lines` as the first `Line::from("")`. Verify by re-running tests.

- [ ] **Step 5: Run all pending_actions tests**

```bash
cargo test --lib pending_actions
```

Expected: every test passes (the new title + hint tests + the existing render tests). The existing `pending_actions_overlay_renders_proposal_and_copy_rows` test asserts substrings like `[p]`, `/compact`, `Enter open explainer` — the old hint message is gone, so update that test's assertions:

```rust
assert!(dump.contains("[p]"), "modal must label proposal rows: {dump}");
// ...remove the `Enter open explainer` assertion (no longer in the list lines).
```

Also update `empty_modal_renders_no_pending_actions_message` — same removal of the old hint assertion.

- [ ] **Step 6: Lint + format**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

- [ ] **Step 7: Commit**

```bash
git add src/ui/pending_actions.rs
git commit -m "v1.40 a overlay: split-render with live explainer + multi-aware title/hint"
```

---

# Phase D — a overlay input

## Task 9: `PendingActionsOutcome` enum + selection / multi-toggle keys

**Files:**

- Modify: `src/app/pending_actions_overlay.rs:23-61` (outcome enum + key handler).

- [ ] **Step 1: Write failing tests for the new enum + selection keys**

Append to the existing `#[cfg(test)] mod tests` block in `src/app/pending_actions_overlay.rs`:

```rust
#[test]
fn space_toggles_multi_on_cursor_item() {
    use crate::domain::origin::SourceKind;
    use crate::ui::pending_actions::{PendingItem, pending_item_key};
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    let outcome = handle_pending_actions_overlay_key(
        &mut overlay,
        std::slice::from_ref(&item),
        KeyCode::Char(' '),
    );
    assert_eq!(outcome, PendingActionsOutcome::None);
    assert!(overlay.multi_contains(&pending_item_key(&item)));
}

#[test]
fn shift_p_toggles_proposal_group() {
    use crate::domain::origin::SourceKind;
    use crate::ui::pending_actions::PendingItem;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    handle_pending_actions_overlay_key(
        &mut overlay,
        std::slice::from_ref(&item),
        KeyCode::Char('P'),
    );
    assert_eq!(overlay.multi_len(), 1);
    handle_pending_actions_overlay_key(
        &mut overlay,
        std::slice::from_ref(&item),
        KeyCode::Char('P'),
    );
    assert_eq!(overlay.multi_len(), 0);
}

#[test]
fn c_clears_multi_only_keeps_cursor() {
    use crate::domain::origin::SourceKind;
    use crate::ui::pending_actions::PendingItem;
    let item = PendingItem::Proposal {
        pane_idx: 0,
        pane_label: "claude:1:main · %1".into(),
        slash_command: "/compact".into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: "%1:/compact".into(),
        target_pane_id: "%1".into(),
    };
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    overlay.set_selected(0);
    handle_pending_actions_overlay_key(
        &mut overlay,
        std::slice::from_ref(&item),
        KeyCode::Char(' '),
    );
    assert_eq!(overlay.multi_len(), 1);
    handle_pending_actions_overlay_key(
        &mut overlay,
        std::slice::from_ref(&item),
        KeyCode::Char('c'),
    );
    assert_eq!(overlay.multi_len(), 0);
    assert_eq!(overlay.selected(), 0, "c does not move the cursor");
}
```

Existing tests in this module still use the old `PendingActionsKeyOutcome` enum and the old `(items_len: usize)` handler signature. They will fail to compile in Step 3; update them as part of Step 3's edit.

- [ ] **Step 2: Run new tests to confirm they fail**

```bash
cargo test --lib app::pending_actions_overlay
```

Expected: compile errors — outcome variant `None` already exists but `PendingActionsKeyOutcome` is the wrong name; we need `PendingActionsOutcome`. Also the handler signature needs to take `items: &[PendingItem]` rather than `items_len: usize`.

- [ ] **Step 3: Replace the outcome enum and the key handler**

In `src/app/pending_actions_overlay.rs`, replace the existing enum + `handle_pending_actions_overlay_key`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingActionsOutcome {
    None,
    Closed,
    AcceptItems(Vec<usize>),
    ClearItems(Vec<usize>),
    CopyItem(usize),
}

pub fn handle_pending_actions_overlay_key(
    overlay: &mut PendingActionsOverlay,
    items: &[PendingItem],
    code: KeyCode,
) -> PendingActionsOutcome {
    if !overlay.is_open() {
        return PendingActionsOutcome::None;
    }
    match code {
        KeyCode::Char('a') | KeyCode::Char('q') | KeyCode::Esc => {
            overlay.close();
            PendingActionsOutcome::Closed
        }
        KeyCode::Up | KeyCode::Char('k') => {
            overlay.select_prev(items.len());
            PendingActionsOutcome::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            overlay.select_next(items.len());
            PendingActionsOutcome::None
        }
        KeyCode::Char(' ') => {
            if let Some(item) = items.get(overlay.selected()) {
                overlay.toggle_multi(item);
            }
            PendingActionsOutcome::None
        }
        KeyCode::Char('P') => {
            overlay.toggle_group_proposals(items);
            PendingActionsOutcome::None
        }
        KeyCode::Char('Y') => {
            overlay.toggle_group_alerts(items);
            PendingActionsOutcome::None
        }
        KeyCode::Char('A') => {
            overlay.toggle_group_all(items);
            PendingActionsOutcome::None
        }
        KeyCode::Char('c') => {
            overlay.clear_multi();
            PendingActionsOutcome::None
        }
        KeyCode::Char('p') | KeyCode::Char('d') | KeyCode::Char('y') | KeyCode::Enter => {
            // Dispatch keys handled in Task 10.
            PendingActionsOutcome::None
        }
        _ => PendingActionsOutcome::None,
    }
}
```

Update the existing tests in this module to use the new outcome name and the new handler signature. Each call like:

```rust
let outcome = handle_pending_actions_overlay_key(&mut overlay, 3, KeyCode::Esc);
assert_eq!(outcome, PendingActionsKeyOutcome::Closed);
```

becomes:

```rust
let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Esc);
assert_eq!(outcome, PendingActionsOutcome::Closed);
```

For tests that exercise `select_next` / `select_prev` with a non-zero size, build a small `Vec<PendingItem>` from the existing fixture helper (see Task 4 tests for the proposal builder). Replace the old `EnterSelected(idx)` test with a placeholder for now (Task 10 will reinstate Enter→`None` semantics):

```rust
#[test]
fn enter_is_silently_swallowed() {
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Enter);
    assert_eq!(outcome, PendingActionsOutcome::None);
    assert!(overlay.is_open());
}
```

- [ ] **Step 4: Run all tests**

```bash
cargo test --lib app::pending_actions_overlay
```

Expected: all pass. The 3 new tests (`space_toggles_multi_on_cursor_item`, `shift_p_toggles_proposal_group`, `c_clears_multi_only_keeps_cursor`) plus the rewritten existing tests.

- [ ] **Step 5: Lint + format**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

- [ ] **Step 6: Commit**

```bash
git add src/app/pending_actions_overlay.rs
git commit -m "v1.40 a overlay: PendingActionsOutcome enum + multi-toggle key handlers"
```

---

## Task 10: Dispatch keys (`p` / `d` / `y`) returning `AcceptItems` / `ClearItems` / `CopyItem`

**Files:**

- Modify: `src/app/pending_actions_overlay.rs` — extend the `'p'` / `'d'` / `'y'` arm.

- [ ] **Step 1: Write failing dispatch-key tests**

Append:

```rust
fn fixture_proposal(id: &str, command: &str) -> crate::ui::pending_actions::PendingItem {
    use crate::domain::origin::SourceKind;
    use crate::ui::pending_actions::PendingItem;
    PendingItem::Proposal {
        pane_idx: 0,
        pane_label: format!("claude:1:main · {id}"),
        slash_command: command.into(),
        severity: None,
        source: SourceKind::ProjectCanonical,
        proposal_id: format!("{id}:{command}"),
        target_pane_id: id.into(),
    }
}

fn fixture_copy(title: &str, command: &str) -> crate::ui::pending_actions::PendingItem {
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Severity;
    use crate::ui::pending_actions::PendingItem;
    PendingItem::Copy {
        alert_idx: 0,
        command: command.into(),
        alert_title: title.into(),
        severity: Severity::Warning,
        source: SourceKind::Estimated,
        pane_idx: None,
    }
}

#[test]
fn p_no_multi_returns_cursor_proposal() {
    let items = vec![fixture_proposal("%1", "/compact")];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('p'));
    assert_eq!(outcome, PendingActionsOutcome::AcceptItems(vec![0]));
}

#[test]
fn p_no_multi_with_alert_cursor_returns_none() {
    let items = vec![fixture_copy("ctx", "/clear")];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('p'));
    assert_eq!(outcome, PendingActionsOutcome::None);
}

#[test]
fn p_with_multi_returns_proposal_indices_only() {
    let items = vec![
        fixture_proposal("%1", "/compact"),
        fixture_copy("ctx", "/clear"),
        fixture_proposal("%2", "/clear"),
    ];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    overlay.toggle_group_all(&items);
    let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('p'));
    assert_eq!(outcome, PendingActionsOutcome::AcceptItems(vec![0, 2]));
}

#[test]
fn d_no_multi_returns_cursor() {
    let items = vec![fixture_copy("ctx", "/clear")];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('d'));
    assert_eq!(outcome, PendingActionsOutcome::ClearItems(vec![0]));
}

#[test]
fn d_with_multi_returns_all_indices_ascending() {
    let items = vec![
        fixture_proposal("%1", "/compact"),
        fixture_copy("ctx", "/clear"),
        fixture_proposal("%2", "/clear"),
    ];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    overlay.toggle_group_all(&items);
    let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('d'));
    assert_eq!(outcome, PendingActionsOutcome::ClearItems(vec![0, 1, 2]));
}

#[test]
fn y_with_multi_picks_first_alert() {
    let items = vec![
        fixture_proposal("%1", "/compact"),
        fixture_copy("ctx-a", "/clear"),
        fixture_copy("ctx-b", "/compact"),
    ];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    overlay.toggle_group_all(&items);
    let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('y'));
    assert_eq!(outcome, PendingActionsOutcome::CopyItem(1));
}

#[test]
fn y_no_multi_with_proposal_cursor_returns_none() {
    let items = vec![fixture_proposal("%1", "/compact")];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('y'));
    assert_eq!(outcome, PendingActionsOutcome::None);
}
```

- [ ] **Step 2: Run new tests to confirm they fail**

```bash
cargo test --lib app::pending_actions_overlay::tests::p_no_multi_returns_cursor_proposal
```

Expected: assertion failure — current handler returns `None` for `p`/`d`/`y`.

- [ ] **Step 3: Replace the dispatch-key arm with the real logic**

In `handle_pending_actions_overlay_key`, replace:

```rust
KeyCode::Char('p') | KeyCode::Char('d') | KeyCode::Char('y') | KeyCode::Enter => {
    PendingActionsOutcome::None
}
```

with:

```rust
KeyCode::Char('p') => dispatch_accept(overlay, items),
KeyCode::Char('d') => dispatch_clear(overlay, items),
KeyCode::Char('y') => dispatch_copy(overlay, items),
KeyCode::Enter => PendingActionsOutcome::None,
```

And add these helpers below the handler:

```rust
fn dispatch_accept(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) -> PendingActionsOutcome {
    let idxs = if overlay.multi_len() == 0 {
        match items.get(overlay.selected()) {
            Some(PendingItem::Proposal { .. }) => vec![overlay.selected()],
            _ => Vec::new(),
        }
    } else {
        let mut out: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, it)| matches!(it, PendingItem::Proposal { .. }))
            .filter(|(_, it)| {
                overlay.multi_contains(&crate::ui::pending_actions::pending_item_key(it))
            })
            .map(|(idx, _)| idx)
            .collect();
        out.sort_unstable();
        out
    };
    if idxs.is_empty() {
        PendingActionsOutcome::None
    } else {
        PendingActionsOutcome::AcceptItems(idxs)
    }
}

fn dispatch_clear(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) -> PendingActionsOutcome {
    let idxs = if overlay.multi_len() == 0 {
        if items.get(overlay.selected()).is_some() {
            vec![overlay.selected()]
        } else {
            Vec::new()
        }
    } else {
        let mut out: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                overlay.multi_contains(&crate::ui::pending_actions::pending_item_key(it))
            })
            .map(|(idx, _)| idx)
            .collect();
        out.sort_unstable();
        out
    };
    if idxs.is_empty() {
        PendingActionsOutcome::None
    } else {
        PendingActionsOutcome::ClearItems(idxs)
    }
}

fn dispatch_copy(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) -> PendingActionsOutcome {
    if overlay.multi_len() == 0 {
        return match items.get(overlay.selected()) {
            Some(PendingItem::Copy { .. }) => PendingActionsOutcome::CopyItem(overlay.selected()),
            _ => PendingActionsOutcome::None,
        };
    }
    let first = items.iter().enumerate().find(|(_, it)| {
        matches!(it, PendingItem::Copy { .. })
            && overlay.multi_contains(&crate::ui::pending_actions::pending_item_key(it))
    });
    match first {
        Some((idx, _)) => PendingActionsOutcome::CopyItem(idx),
        None => PendingActionsOutcome::None,
    }
}
```

- [ ] **Step 4: Run all tests**

```bash
cargo test --lib app::pending_actions_overlay
```

Expected: 7 new tests pass.

- [ ] **Step 5: Lint + format**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

- [ ] **Step 6: Commit**

```bash
git add src/app/pending_actions_overlay.rs
git commit -m "v1.40 a overlay: p/d/y dispatch keys return AcceptItems/ClearItems/CopyItem"
```

---

## Task 11: Mouse handler — checkbox vs content + wheel + close

**Files:**

- Modify: `src/app/pending_actions_overlay.rs` — `handle_pending_actions_overlay_mouse`.

- [ ] **Step 1: Write failing mouse tests**

Append:

```rust
fn mouse_event(
    kind: MouseEventKind,
    column: u16,
    row: u16,
) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::empty(),
    }
}

#[test]
fn click_checkbox_toggles_multi_keeps_cursor() {
    use crate::ui::pending_actions::pending_actions_modal_rects;
    let items = vec![fixture_proposal("%1", "/compact")];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    overlay.set_selected(0);
    let viewport = Rect::new(0, 0, 200, 80);
    let rects = pending_actions_modal_rects(viewport);
    // Row 0 lives at rects.list.y + 1 (1-row top padding inside list pane).
    let row = rects.list.y + 1;
    let col = rects.list.x + 1; // inside [x] glyph (cols 0..3 from list.x)
    let outcome = handle_pending_actions_overlay_mouse(
        &mut overlay,
        viewport,
        &items,
        mouse_event(MouseEventKind::Down(MouseButton::Left), col, row),
    );
    assert_eq!(outcome, PendingActionsOutcome::None);
    assert_eq!(overlay.multi_len(), 1, "checkbox click toggles multi");
    assert_eq!(overlay.selected(), 0, "cursor unchanged");
}

#[test]
fn click_content_moves_cursor() {
    use crate::ui::pending_actions::pending_actions_modal_rects;
    let items = vec![
        fixture_proposal("%1", "/compact"),
        fixture_proposal("%2", "/clear"),
    ];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    overlay.set_selected(0);
    let viewport = Rect::new(0, 0, 200, 80);
    let rects = pending_actions_modal_rects(viewport);
    let row = rects.list.y + 1 + 1; // second item
    let col = rects.list.x + 10; // content area (>= 4)
    handle_pending_actions_overlay_mouse(
        &mut overlay,
        viewport,
        &items,
        mouse_event(MouseEventKind::Down(MouseButton::Left), col, row),
    );
    assert_eq!(overlay.selected(), 1);
    assert_eq!(overlay.multi_len(), 0);
}

#[test]
fn wheel_moves_cursor() {
    let items = vec![
        fixture_proposal("%1", "/compact"),
        fixture_proposal("%2", "/clear"),
    ];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    overlay.set_selected(0);
    let viewport = Rect::new(0, 0, 200, 80);
    handle_pending_actions_overlay_mouse(
        &mut overlay,
        viewport,
        &items,
        mouse_event(MouseEventKind::ScrollDown, 0, 0),
    );
    assert_eq!(overlay.selected(), 1);
    handle_pending_actions_overlay_mouse(
        &mut overlay,
        viewport,
        &items,
        mouse_event(MouseEventKind::ScrollUp, 0, 0),
    );
    assert_eq!(overlay.selected(), 0);
}

#[test]
fn click_close_button_closes() {
    use crate::ui::dashboard::close_button_rect;
    use crate::ui::pending_actions::pending_actions_modal_rects;
    let items = vec![fixture_proposal("%1", "/compact")];
    let mut overlay = PendingActionsOverlay::new();
    overlay.open();
    let viewport = Rect::new(0, 0, 200, 80);
    let rects = pending_actions_modal_rects(viewport);
    let close = close_button_rect(rects.area);
    let outcome = handle_pending_actions_overlay_mouse(
        &mut overlay,
        viewport,
        &items,
        mouse_event(MouseEventKind::Down(MouseButton::Left), close.x, close.y),
    );
    assert_eq!(outcome, PendingActionsOutcome::Closed);
    assert!(!overlay.is_open());
}
```

- [ ] **Step 2: Run new tests to confirm they fail**

```bash
cargo test --lib app::pending_actions_overlay::tests::click_checkbox
```

Expected: compile error — mouse handler still has the old signature `(overlay, viewport, event)`.

- [ ] **Step 3: Replace `handle_pending_actions_overlay_mouse`**

Replace the existing function with:

```rust
pub fn handle_pending_actions_overlay_mouse(
    overlay: &mut PendingActionsOverlay,
    viewport: Rect,
    items: &[PendingItem],
    event: MouseEvent,
) -> PendingActionsOutcome {
    if !overlay.is_open() {
        return PendingActionsOutcome::None;
    }
    let rects = pending_actions_modal_rects(viewport);

    // [x] close button
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && crate::app::keymap::rect_contains(
            close_button_rect(rects.area),
            event.column,
            event.row,
        )
    {
        overlay.close();
        return PendingActionsOutcome::Closed;
    }

    // List pane click: distinguish checkbox vs content.
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && rect_contains(rects.list, event.column, event.row)
    {
        let leading_pad: u16 = 1;
        if event.row >= rects.list.y + leading_pad {
            let row_off = event.row - rects.list.y - leading_pad;
            let idx = row_off as usize;
            if idx < items.len() {
                let col_off = event.column.saturating_sub(rects.list.x);
                if col_off < 4 {
                    // checkbox zone
                    if let Some(item) = items.get(idx) {
                        overlay.toggle_multi(item);
                    }
                } else {
                    // content / cursor zone
                    overlay.set_selected(idx);
                }
            }
        }
        return PendingActionsOutcome::None;
    }

    // Wheel: scroll the cursor.
    match event.kind {
        MouseEventKind::ScrollUp => overlay.select_prev(items.len()),
        MouseEventKind::ScrollDown => overlay.select_next(items.len()),
        _ => {}
    }

    // Swallow everything else (no leak to dashboard).
    PendingActionsOutcome::None
}

fn rect_contains(rect: Rect, col: u16, row: u16) -> bool {
    crate::app::keymap::rect_contains(rect, col, row)
}
```

(The local `rect_contains` wrapper exists only because the function is called twice and the long path is noisy.)

Make sure the `use` block at the top of the file has all imports needed:

```rust
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::ui::dashboard::close_button_rect;
use crate::ui::pending_actions::{
    PendingActionsOverlay, pending_actions_modal_rects,
};
use crate::ui::pending_actions::PendingItem;
```

(Drop the previous `pending_actions_modal_area` import if present — it lives behind `pending_actions_modal_rects.area` now.)

- [ ] **Step 4: Run all tests**

```bash
cargo test --lib app::pending_actions_overlay
```

Expected: 4 new mouse tests pass. The old `closed_overlay_swallows_keys` test (single arg version) should still pass — it predates mouse handling.

- [ ] **Step 5: Lint + format**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

- [ ] **Step 6: Commit**

```bash
git add src/app/pending_actions_overlay.rs
git commit -m "v1.40 a overlay: mouse handler with checkbox/content hit-test, wheel, close"
```

---

# Phase E — wiring

## Task 12: tui_loop — bulk dispatch + drop `dispatch_pending_action`

**Files:**

- Modify: `src/app/tui_loop.rs` — outcome dispatch (~line 320–360), `dispatch_pending_action` + `PendingActionDispatch` deletion (~line 770–855), key/mouse signature updates, `render_pending_actions_modal` call site, `metrics_modal_rects` call sites.

- [ ] **Step 1: Locate outcome dispatch and the helper**

```bash
grep -n "PendingActionsKeyOutcome\|EnterSelected\|dispatch_pending_action\|PendingActionDispatch\|handle_pending_actions_overlay_mouse\|handle_pending_actions_overlay_key" src/app/tui_loop.rs
```

Capture the exact line numbers — they will move slightly each commit, so resolve them at edit time.

- [ ] **Step 2: Update the `use` import**

Replace:

```rust
use crate::app::pending_actions_overlay::{
    PendingActionsKeyOutcome, handle_pending_actions_overlay_key,
    handle_pending_actions_overlay_mouse,
};
```

with:

```rust
use crate::app::pending_actions_overlay::{
    PendingActionsOutcome, accept_action_for, copy_action_for,
    handle_pending_actions_overlay_key, handle_pending_actions_overlay_mouse,
    reject_action_for,
};
```

- [ ] **Step 3: Add a `retain_multi` method on `PendingActionsOverlay`**

In `src/ui/pending_actions.rs`'s `impl PendingActionsOverlay`, add:

```rust
pub fn retain_multi(&mut self, mut pred: impl FnMut(&String) -> bool) {
    self.multi_selected.retain(|k| pred(k));
}
```

This is called from the bulk-dispatch helpers below to prune only the dispatched keys (per spec §5.10).

- [ ] **Step 4: Replace the key-outcome match in `tui_loop`**

Find the block that matches on `PendingActionsKeyOutcome` (currently around `tui_loop.rs:340`). Replace the inner `if let ... EnterSelected(idx)` arm with a full `match` that handles every variant of `PendingActionsOutcome`. The bulk dispatchers route each item through the existing `confirm_pending_action(...)` (proposals) or the alert-hide deadline path (alerts):

```rust
let outcome = handle_pending_actions_overlay_key(
    &mut pending_actions,
    &pending_items,
    k.code,
);
match outcome {
    PendingActionsOutcome::None | PendingActionsOutcome::Closed => {}
    PendingActionsOutcome::AcceptItems(idxs) => {
        dispatch_bulk_proposals(
            &idxs,
            &pending_items,
            &mut pending_actions,
            &mut dashboard.notices,
            &ctx.source,
            &*ctx.sink,
            &dashboard.reports,
            ctx.config.actions.mode,
            ctx.config.actions.allow_auto_prompt_send,
            DispatchKind::Accept,
        );
    }
    PendingActionsOutcome::ClearItems(idxs) => {
        dispatch_bulk_clear(
            &idxs,
            &pending_items,
            &mut pending_actions,
            &mut dashboard.notices,
            &mut dashboard.alert_hide_deadlines,
            &dashboard.reports,
            &ctx.source,
            &*ctx.sink,
            ctx.config.actions.mode,
            ctx.config.actions.allow_auto_prompt_send,
            Instant::now(),
        );
    }
    PendingActionsOutcome::CopyItem(idx) => {
        dispatch_bulk_copy(
            idx,
            &pending_items,
            &mut pending_actions,
            &mut dashboard.notices,
            &ctx.source,
            &*ctx.sink,
            &dashboard.reports,
            ctx.config.actions.mode,
            ctx.config.actions.allow_auto_prompt_send,
        );
    }
}
continue;
```

(`continue` mirrors the existing `EnterSelected` branch which also continued. Remove the previous `EnterSelected` block in the same edit.)

- [ ] **Step 5: Add the bulk dispatch helpers at the bottom of `tui_loop.rs`**

```rust
#[derive(Debug, Clone, Copy)]
enum DispatchKind {
    Accept,
    Reject,
}

#[allow(clippy::too_many_arguments)]
fn dispatch_bulk_proposals<P: crate::tmux::polling::PaneSource>(
    idxs: &[usize],
    items: &[crate::ui::pending_actions::PendingItem],
    overlay: &mut crate::ui::pending_actions::PendingActionsOverlay,
    notices: &mut Vec<SystemNotice>,
    source: &P,
    sink: &dyn crate::store::EventSink,
    reports: &[crate::app::event_loop::PaneReport],
    mode: crate::app::config::ActionsMode,
    allow_auto_prompt_send: bool,
    kind: DispatchKind,
) {
    use crate::app::pending_actions_overlay::{accept_action_for, reject_action_for};
    use crate::ui::pending_actions::pending_item_key;

    let mut dispatched_keys: Vec<String> = Vec::new();
    for &idx in idxs {
        let Some(item) = items.get(idx) else { continue; };
        let action = match kind {
            DispatchKind::Accept => accept_action_for(item),
            DispatchKind::Reject => reject_action_for(item),
        };
        let Some(action) = action else { continue; };
        let notice = confirm_pending_action(&action, source, sink, reports, mode, allow_auto_prompt_send);
        notices.push(notice);
        dispatched_keys.push(pending_item_key(item));
    }
    overlay.retain_multi(|k| !dispatched_keys.iter().any(|dk| dk == k));
}

#[allow(clippy::too_many_arguments)]
fn dispatch_bulk_clear<P: crate::tmux::polling::PaneSource>(
    idxs: &[usize],
    items: &[crate::ui::pending_actions::PendingItem],
    overlay: &mut crate::ui::pending_actions::PendingActionsOverlay,
    notices: &mut Vec<SystemNotice>,
    alert_hide_deadlines: &mut std::collections::HashMap<String, std::time::Instant>,
    reports: &[crate::app::event_loop::PaneReport],
    source: &P,
    sink: &dyn crate::store::EventSink,
    mode: crate::app::config::ActionsMode,
    allow_auto_prompt_send: bool,
    now: std::time::Instant,
) {
    use crate::app::pending_actions_overlay::reject_action_for;
    use crate::ui::pending_actions::{PendingItem, pending_item_key};

    let mut dispatched_keys: Vec<String> = Vec::new();
    // idxs is already sorted ascending by the dispatcher (see Task 10).
    for &idx in idxs {
        let Some(item) = items.get(idx) else { continue; };
        match item {
            PendingItem::Proposal { .. } => {
                if let Some(action) = reject_action_for(item) {
                    let notice = confirm_pending_action(
                        &action, source, sink, reports, mode, allow_auto_prompt_send,
                    );
                    notices.push(notice);
                    dispatched_keys.push(pending_item_key(item));
                }
            }
            PendingItem::Copy { alert_idx, .. } => {
                if let Some(key) = crate::app::dashboard_state::alert_key_at_index(
                    notices, reports, alert_hide_deadlines, now, *alert_idx,
                ) {
                    alert_hide_deadlines
                        .insert(key, now + crate::ui::alerts::ALERT_AUTO_HIDE_DELAY);
                }
                dispatched_keys.push(pending_item_key(item));
            }
        }
    }
    overlay.retain_multi(|k| !dispatched_keys.iter().any(|dk| dk == k));
}

#[allow(clippy::too_many_arguments)]
fn dispatch_bulk_copy<P: crate::tmux::polling::PaneSource>(
    idx: usize,
    items: &[crate::ui::pending_actions::PendingItem],
    overlay: &mut crate::ui::pending_actions::PendingActionsOverlay,
    notices: &mut Vec<SystemNotice>,
    source: &P,
    sink: &dyn crate::store::EventSink,
    reports: &[crate::app::event_loop::PaneReport],
    mode: crate::app::config::ActionsMode,
    allow_auto_prompt_send: bool,
) {
    use crate::app::pending_actions_overlay::copy_action_for;
    use crate::ui::pending_actions::pending_item_key;

    let Some(item) = items.get(idx) else { return; };
    let Some(action) = copy_action_for(item) else { return; };
    let notice = confirm_pending_action(&action, source, sink, reports, mode, allow_auto_prompt_send);
    notices.push(notice);
    let key = pending_item_key(item);
    overlay.retain_multi(|k| k != &key);
}
```

There is no per-item snapshot validation work to add — `confirm_pending_action` already runs `resolve_accept_target` internally, surfacing the `"proposal vanished"` `SystemNotice` per item when a proposal disappears between selection and dispatch. Reusing it is what keeps the spec's "no new audit event types" promise.

For the `AcceptItems` arm, ensure the call passes `DispatchKind::Accept`. The same helper handles `RejectPromptSend` for proposals inside `dispatch_bulk_clear`; alerts route to the hide-deadline path instead. (Note: the routing for `d` therefore uses the dedicated `dispatch_bulk_clear` helper rather than reusing `dispatch_bulk_proposals`. Don't pass `DispatchKind::Reject` to `dispatch_bulk_proposals` from the `ClearItems` arm.)

If the `notices: &mut Vec<SystemNotice>` field on `dashboard` is wrapped (e.g. behind a `push_notice` method), substitute that call shape in place of `notices.push(notice)`. The structure of the bulk dispatch is unchanged.

- [ ] **Step 6: Update mouse handler call site**

Find the existing call to `handle_pending_actions_overlay_mouse(&mut pending_actions, viewport, event)` and update it:

```rust
let outcome = handle_pending_actions_overlay_mouse(
    &mut pending_actions,
    viewport,
    &pending_items,
    event,
);
match outcome {
    PendingActionsOutcome::None | PendingActionsOutcome::Closed => {}
    PendingActionsOutcome::AcceptItems(_)
    | PendingActionsOutcome::ClearItems(_)
    | PendingActionsOutcome::CopyItem(_) => {
        // Mouse never produces dispatch outcomes today; future-proof
        // by debug-asserting to flag a contract drift.
        debug_assert!(false, "mouse outcomes should not include dispatch variants");
    }
}
```

- [ ] **Step 7: Delete `dispatch_pending_action` and the `EnterSelected` branch**

The previous wiring routed `EnterSelected(idx)` through `dispatch_pending_action(...)` (currently `tui_loop.rs:340-353` and `tui_loop.rs:776-855`), which opened the Action Explainer modal. With Enter as a no-op and the live panel rendering inline, that path is dead. Delete:

- The `if let PendingActionsKeyOutcome::EnterSelected(idx) = outcome { ... }` block (already replaced in Step 4).
- The `struct PendingActionDispatch<'a>` and `fn dispatch_pending_action(...)` definitions.

Verify with:

```bash
grep -n "PendingActionsKeyOutcome\|EnterSelected\|dispatch_pending_action\b" src/
```

Expected: no hits.

- [ ] **Step 8: Update the `render_pending_actions_modal` call site**

The renderer now takes a `PendingActionsRenderCtx<'_>` (Task 8). Find the existing call (the renderer is invoked once per frame inside the `pending_actions.is_open()` render branch) and replace its argument list:

```rust
crate::ui::pending_actions::render_pending_actions_modal(
    f,
    crate::ui::pending_actions::PendingActionsRenderCtx {
        overlay: &pending_actions,
        items: &pending_items,
        reports: &dashboard.reports,
        mode: ctx.config.actions.mode,
        allow_auto_prompt_send: ctx.config.actions.allow_auto_prompt_send,
    },
);
```

- [ ] **Step 9: Auto-prune multi-select on every frame**

Right after `collect_pending_items(...)` returns the `pending_items` vector for the frame, call:

```rust
pending_actions.prune_to(&pending_items);
```

so stale keys (proposal accepted last tick / alert hidden last tick) drop before the next render paints.

- [ ] **Step 10: Run the full validation gates**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
cargo test --all-targets
```

Expected: every test passes. If any integration or doc test fails because the `EnterSelected` outcome is gone, update it to assert the new outcome variants or delete it as obsolete.

- [ ] **Step 11: Commit**

```bash
git add src/app/tui_loop.rs src/ui/pending_actions.rs src/app/pending_actions_overlay.rs
git commit -m "v1.40 a overlay: bulk dispatch wiring through confirm_pending_action + remove dispatch_pending_action"
```

---

# Phase F — Documentation

## Task 13: UI_MANUAL §8.7 rewrite + help overlay refresh

**Files:**

- Modify: `docs/ai/UI_MANUAL.md` §8.7 (around lines 686-712).
- Modify: help overlay key list — locate via `grep -n "Pending Actions\|action_explainer\|render_help" src/ui/dashboard.rs src/app/*.rs`.

- [ ] **Step 1: Rewrite UI_MANUAL §8.7**

Locate §8.7 — find the line that starts `### 8.7 Pending Actions Overlay` in `docs/ai/UI_MANUAL.md` and replace the entire section (until the next `## 9. 운영 파일`) with:

```
### 8.7 Pending Actions Overlay

`a` 키로 토글합니다. v1.39에서 추가된 디스커버리 layer가
v1.40에서 좌/우 split + 멀티 선택 + 라이브 explainer + 벌크
dispatch로 확장되었습니다.

다음 항목을 한 화면에 보여줍니다:

- pending prompt-send proposal을 보유한 모든 pane (operator가
  `p`/`d`로 처리할 수 있는 항목)
- `suggested_command`가 있는 모든 alert (operator가 `y`로 복사할
  수 있는 항목)

본 overlay 외에도 항목 존재는 두 가지 다른 surface로 노출됩니다:

- **Header chip** — pane card 제목에 `★p`, alert 제목에 `★y`가
  붙고 severity 색으로 강조됩니다.
- **Footer counter** — 화면 하단에 `★p:N · ★y:M` 카운터가 항상
  표시됩니다 (0이면 dim, 양수면 severity 색).

**모달 layout**

- 좌측 (또는 좁은 폭에서는 상단): 항목 리스트
- 우측 (또는 좁은 폭에서는 하단): 커서 항목의 라이브 Action
  Explainer 패널 — `Why now`, `Severity`, `Audit chain`, `Mode now`
  등 기존 Action Explainer 모달과 동일한 정보를 그대로 표시합니다.
- 하단 1줄: 키 안내 + 멀티 선택 카운트
- 모달 폭 ≥ 72셀이면 좌/우 split, 그 미만이면 상/하 split

**행 형식**: `[x|space] ▶ [p|y] severity · command · context`

- 좌측 `[x]` / `[ ]` = 멀티 선택 체크박스 (severity 색)
- `▶` = 현재 커서 행
- `[p]` proposal / `[y]` copyable alert
- `command`는 backtick으로 감싼 슬래시 명령
- `context`는 pane 식별자 또는 alert 제목

**제목**: `"Pending Actions · {N} pending · {S} selected · a 다시로 닫기"`
(멀티 선택이 비어있으면 `· {S} selected` 부분 생략)

**조작 — 키보드**

| 키                            | 동작                                                              |
|------------------------------|------------------------------------------------------------------|
| `↑/↓` · `j/k`                | 커서 이동 → 우측 explainer 즉시 갱신                              |
| `Space`                      | 커서 항목 멀티 선택 토글                                          |
| `P` / `Y` / `A`              | proposal / alert / 전체 그룹 토글 (모두 선택 ↔ 모두 해제)          |
| `c`                          | 멀티 선택 비움 (커서는 유지)                                      |
| `p`                          | accept — 멀티에 proposal이 있으면 일괄, 없으면 커서 (proposal일 때) |
| `d`                          | clear — proposal=reject, alert=hide. 멀티 우선, 없으면 커서        |
| `y`                          | copy — 멀티의 첫 alert 또는 커서 alert (클립보드는 한 줄)          |
| `Enter`                      | silently no-op (실수 입력 방지)                                  |
| `Esc` · `q` · `a` · `[x]` 클릭 | overlay 닫기 (멀티 선택 비움)                                    |

**조작 — 마우스**

| 이벤트                          | 동작                                                              |
|--------------------------------|------------------------------------------------------------------|
| 행 좌측 `[ ]` 영역 (cols 0–3)  | 멀티 선택 토글, 커서 변경 없음                                    |
| 행 cols 4 이상 (커서/내용)     | 커서를 그 행으로 이동, explainer 갱신                              |
| 휠                              | 커서 이동                                                         |
| `[x]` 클릭                     | 닫기                                                              |

**dispatch 후 처리**

- 멀티 선택의 dispatch된 key**만** 제거됩니다. 예: `p` 누름 시
  proposal key만 빠지고 alert key는 그대로 남아 다음에 `d`나 `y`로
  처리할 수 있습니다.
- 폴링 사이에 사라진 key는 자동 prune됩니다 (다음 render에서).

**`[ux] confirm_actions` 무시**

a 오버레이 안의 `p`/`d`/`y`는 `confirm_actions = always | first_time | never`
설정과 무관하게 즉시 dispatch됩니다 — 우측 라이브 explainer 패널이
confirmation 역할을 합니다. 대시보드 직접 키(`p`/`d`/`y`)는 기존처럼
설정값에 따라 Action Explainer 모달을 띄웁니다.

**Mode 차단 표시**

`observe_only` / `AutoSendOff`와 같이 actuation mode가 차단 중일
때는 우측 explainer 패널의 `Mode now` 줄에 ⚠ 경고가 표시되어,
operator가 dispatch 전에 확인할 수 있습니다.

항목이 없을 때는 `Select an item to see what would happen.` 라는
dim 안내 줄이 explainer 패널에 표시됩니다.
```

- [ ] **Step 2: Refresh help overlay key entries**

```bash
grep -n "★p\|★y\|Pending Actions\|render_help_overlay\|fn help_lines" src/ src/ui/ src/app/ -R | head
```

Locate the help-overlay text builder. Add or update entries for the Pending Actions section. Replace the previous Enter-opens-explainer line with:

```text
a key:
  Space toggle · P/Y/A group · c clear-sel
  p accept · d clear · y copy · Enter no-op · Esc/q/a close
```

If the help overlay uses a structured `(label, description)` list, encode each as one row:

```rust
("a", "open Pending Actions overlay (live explainer + multi-select bulk dispatch)"),
("Space", "toggle multi-select on cursor item (a overlay)"),
("P / Y / A", "toggle proposal / alert / all group selection (a overlay)"),
("c", "clear multi-select set (a overlay)"),
```

Update existing entries that mentioned "Enter open explainer" — that text is gone.

- [ ] **Step 3: Run a quick build to ensure nothing imports moved**

```bash
cargo build
```

Expected: clean build.

- [ ] **Step 4: Manual smoke (optional but recommended)**

```bash
scripts/run-qmonster.sh
```

Open `a`, scroll, Space-toggle, press `P`, press `d`, watch audit log + system notices. Close. Open `m`, drag the title bar, press `=`. (Mouse drag manual smoke is hard to TDD; this manual step is the spec's compliance check for §4.3.)

- [ ] **Step 5: Commit**

```bash
git add docs/ai/UI_MANUAL.md src/
git commit -m "v1.40 docs: UI_MANUAL §8.7 rewrite + help overlay key entries"
```

---

# Self-Review Checklist

Before declaring the plan complete, verify:

1. **Spec coverage**:
   - §3 architecture overview — Tasks 1, 4, 5, 6 (state), 9–11 (dispatch), 12 (wiring).
   - §4 m drag — Tasks 1, 2, 3.
   - §5.1–5.3 a layout + state — Tasks 5 (size/split), 4 (state).
   - §5.4 row rendering — Task 7.
   - §5.5 right-pane explainer — Tasks 6, 8.
   - §5.6 title + hint — Task 8.
   - §5.7 Outcome enum — Task 9.
   - §5.8 key matrix — Tasks 9, 10.
   - §5.9 mouse matrix — Task 11.
   - §5.10 bulk dispatch + audit — Task 12.
   - §5.11 confirm_actions interaction — Task 12 (no separate Action Explainer modal opens from a overlay) + Task 13 (documented).
   - §5.12 actuation mode display — Task 8 (right-pane shows `Mode now` line via reused helpers).
   - §5.13 system notices — Task 12 (per-item; no summary type added).
   - §5.14 / §6 tests + docs — Tasks 1–13 each ship their own.

2. **No placeholders**: every `/* ... */` in Task 12 is replaced with the concrete arg list at edit time. No "TBD" / "TODO" / "fill in details" remains in any task.

3. **Type / signature consistency**:
   - `PendingActionsOutcome` (Task 9) matches the variants Task 12 destructures.
   - `pending_actions_modal_rects` returns the same `PendingActionsModalRects` struct used by both Task 8 (renderer) and Task 11 (mouse hit-test).
   - `accept_action_for` / `reject_action_for` / `copy_action_for` (Task 6) and `build_explainer_view_for_item` (Task 6) are the only constructors used by both Task 8's renderer and Task 12's bulk dispatchers — variant names and field shapes match `src/app/action_explainer.rs:23-42`.
   - `metrics_modal_rects` 5-arg signature (Task 1) is consistent at every call site updated in Task 1 Step 5.

If any inconsistency surfaces during execution, fix in place and re-run the validation gates.

---

## Execution

Plan complete. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task with two-stage review.
2. **Inline Execution** — sequential tasks in this session with checkpoints.

Pick one before starting Task 1.
