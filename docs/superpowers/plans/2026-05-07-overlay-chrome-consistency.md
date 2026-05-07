# Overlay Chrome Consistency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Standardize the first overlay chrome slice by adding a shared modal close style, shared geometry helpers, and resizable/movable `i` Token Insights plus `n` Anomaly Events overlays.

**Architecture:** Keep the rollout incremental. First extract low-risk shared UI helpers, prove them against the existing `m` Metrics geometry, then migrate `i` and `n` to the same resize/drag contract. Do not touch provider adapters, policy rules, audit schema, or recommendation behavior.

**Tech Stack:** Rust, Ratatui, Crossterm mouse/key events, existing Qmonster TUI overlay modules, cargo test/clippy.

---

## Scope

This plan implements the first slice from
`docs/superpowers/specs/2026-05-07-overlay-chrome-consistency-design.md`:

- Add a central `[x]` style helper and apply it to all current overlay renderers.
- Add shared modal geometry helpers.
- Use the geometry helper to preserve current `m` Metrics layout behavior.
- Upgrade `i` Token Insights to resize, reset, and title-row drag.
- Upgrade `n` Anomaly Events to resize, reset, and title-row drag.
- Update Help/UI manual/architecture/validation for the first slice.

The remaining overlays (`S`, `P`, Help/Git geometry, Target picker split
resize) are intentionally left for the next implementation plan after
the helper and two read-only overlays are proven.

## File Structure

| File | Responsibility |
| --- | --- |
| `src/ui/theme.rs` | Add shared `modal_close_style()` and tests. |
| `src/ui/modal_chrome.rs` | New shared geometry/state helpers for modal size, offset, drag anchors, and clamped layout. |
| `src/ui/mod.rs` | Export `modal_chrome`. |
| `src/ui/metrics.rs` | Reuse shared clamped-offset helper without changing Metrics behavior. |
| `src/ui/insights.rs` | Add overlay geometry state, render with shared geometry, render `[x]` with shared style. |
| `src/app/insights_overlay.rs` | Add resize/reset/title-drag key and mouse handling for `i`. |
| `src/ui/anomaly_overlay.rs` | Add overlay geometry state, render with shared geometry, render `[x]` with shared style. |
| `src/app/anomaly_overlay.rs` | Add resize/reset/title-drag key and mouse handling for `n`. |
| `src/ui/dashboard.rs` | Update Help text to describe the shared first-slice overlay chrome contract. |
| `docs/ai/UI_MANUAL.md` | Add first-slice overlay chrome controls and exceptions. |
| `docs/ai/ARCHITECTURE.md` | Note shared UI chrome layer as UI-only architecture. |
| `docs/ai/VALIDATION.md` | Add validation evidence for first-slice overlay chrome. |

## Task 1: Centralize `[x]` Close Button Style

**Files:**
- Modify: `src/ui/theme.rs`
- Modify: `src/ui/dashboard.rs`
- Modify: `src/ui/metrics.rs`
- Modify: `src/ui/pending_actions.rs`
- Modify: `src/ui/action_explainer.rs`
- Modify: `src/ui/insights.rs`
- Modify: `src/ui/settings.rs`
- Modify: `src/ui/anomaly_overlay.rs`

- [ ] **Step 1: Write the failing style test**

Add this test to `src/ui/theme.rs` inside `mod tests`:

```rust
#[test]
fn modal_close_style_uses_active_border_not_severity() {
    let style = modal_close_style();
    assert_eq!(style.fg, Some(BORDER_ACTIVE));
    assert!(style.add_modifier.contains(Modifier::BOLD));
}
```

- [ ] **Step 2: Run the focused test to verify it fails**

Run:

```bash
cargo test ui::theme::tests::modal_close_style_uses_active_border_not_severity
```

Expected: fail with `cannot find function modal_close_style`.

- [ ] **Step 3: Add the style helper**

Add this function to `src/ui/theme.rs` after `label_style()`:

```rust
pub fn modal_close_style() -> Style {
    Style::default().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD)
}
```

- [ ] **Step 4: Apply the style helper to all rendered `[x]` close buttons**

Replace each overlay close button style block shaped like this:

```rust
Paragraph::new("[x]").style(
    Style::default()
        .fg(theme::TEXT_PRIMARY)
        .add_modifier(Modifier::BOLD),
)
```

with:

```rust
Paragraph::new("[x]").style(theme::modal_close_style())
```

Apply the replacement in:

- `src/ui/dashboard.rs`
- `src/ui/metrics.rs`
- `src/ui/pending_actions.rs`
- `src/ui/action_explainer.rs`
- `src/ui/insights.rs`
- `src/ui/settings.rs`
- `src/ui/anomaly_overlay.rs`

If a file only imported `Modifier` for the close button, remove that
unused import after the replacement.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test ui::theme::tests::modal_close_style_uses_active_border_not_severity
cargo test ui::dashboard::tests::help_documents_modal_x_close
cargo test app::anomaly_overlay::tests::mouse_click_on_close_button_closes
cargo test app::insights_overlay::tests::mouse_left_on_close_button_closes_overlay
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/ui/theme.rs src/ui/dashboard.rs src/ui/metrics.rs src/ui/pending_actions.rs src/ui/action_explainer.rs src/ui/insights.rs src/ui/settings.rs src/ui/anomaly_overlay.rs
git commit -m "ui: centralize modal close styling"
```

## Task 2: Add Shared Modal Geometry Helpers

**Files:**
- Create: `src/ui/modal_chrome.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/ui/metrics.rs`

- [ ] **Step 1: Create the failing helper tests**

Create `src/ui/modal_chrome.rs` with tests first:

```rust
use ratatui::layout::Rect;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_area_with_zero_offset_matches_centered_rect() {
        let viewport = Rect::new(0, 0, 100, 40);
        let geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        let area = geometry.area(viewport);
        assert_eq!(area, crate::ui::dashboard::centered_rect(80, 70, viewport));
    }

    #[test]
    fn modal_area_applies_positive_offset() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        let base = geometry.area(viewport);
        geometry.set_offset(5, 3);
        let moved = geometry.area(viewport);
        assert_eq!(moved.x, base.x + 5);
        assert_eq!(moved.y, base.y + 3);
    }

    #[test]
    fn modal_area_left_top_clamps_hard() {
        let viewport = Rect::new(10, 5, 100, 40);
        let mut geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        geometry.set_offset(i16::MIN, i16::MIN);
        let area = geometry.area(viewport);
        assert_eq!(area.x, viewport.x);
        assert_eq!(area.y, viewport.y);
    }

    #[test]
    fn modal_area_right_bottom_leaves_visible_grip() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        geometry.set_offset(i16::MAX, i16::MAX);
        let area = geometry.area(viewport);
        assert!(area.x < viewport.x + viewport.width);
        assert!(area.y < viewport.y + viewport.height);
        assert!(viewport.x + viewport.width - area.x >= 4);
        assert!(viewport.y + viewport.height - area.y >= 1);
    }

    #[test]
    fn shrink_grow_and_reset_geometry() {
        let mut geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        geometry.grow();
        assert_eq!(geometry.width_pct(), 85);
        assert_eq!(geometry.height_pct(), 75);
        geometry.shrink();
        geometry.shrink();
        assert_eq!(geometry.width_pct(), 75);
        assert_eq!(geometry.height_pct(), 65);
        geometry.set_offset(4, 2);
        geometry.reset();
        assert_eq!(geometry.width_pct(), 80);
        assert_eq!(geometry.height_pct(), 70);
        assert_eq!(geometry.offset_x(), 0);
        assert_eq!(geometry.offset_y(), 0);
    }
}
```

- [ ] **Step 2: Wire the module and verify the test fails on missing types**

Add this to `src/ui/mod.rs`:

```rust
pub mod modal_chrome;
```

Run:

```bash
cargo test ui::modal_chrome::tests::modal_area_with_zero_offset_matches_centered_rect
```

Expected: fail with missing `ModalGeometry`.

- [ ] **Step 3: Implement `ModalGeometry` and `DragAnchor`**

Replace the top of `src/ui/modal_chrome.rs` with this implementation,
keeping the tests below it:

```rust
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragAnchor {
    pub start_col: u16,
    pub start_row: u16,
    pub start_offset_x: i16,
    pub start_offset_y: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalGeometry {
    width_pct: u16,
    height_pct: u16,
    default_width_pct: u16,
    default_height_pct: u16,
    min_pct: u16,
    max_pct: u16,
    step_pct: u16,
    offset_x: i16,
    offset_y: i16,
    drag_anchor: Option<DragAnchor>,
}

impl ModalGeometry {
    pub fn new(default_width_pct: u16, default_height_pct: u16, min_pct: u16, max_pct: u16, step_pct: u16) -> Self {
        Self {
            width_pct: default_width_pct,
            height_pct: default_height_pct,
            default_width_pct,
            default_height_pct,
            min_pct,
            max_pct,
            step_pct,
            offset_x: 0,
            offset_y: 0,
            drag_anchor: None,
        }
    }

    pub fn width_pct(&self) -> u16 {
        self.width_pct
    }

    pub fn height_pct(&self) -> u16 {
        self.height_pct
    }

    pub fn offset_x(&self) -> i16 {
        self.offset_x
    }

    pub fn offset_y(&self) -> i16 {
        self.offset_y
    }

    pub fn drag_anchor(&self) -> Option<DragAnchor> {
        self.drag_anchor
    }

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

    pub fn shrink(&mut self) {
        self.width_pct = self.width_pct.saturating_sub(self.step_pct).max(self.min_pct);
        self.height_pct = self.height_pct.saturating_sub(self.step_pct).max(self.min_pct);
    }

    pub fn grow(&mut self) {
        self.width_pct = self.width_pct.saturating_add(self.step_pct).min(self.max_pct);
        self.height_pct = self.height_pct.saturating_add(self.step_pct).min(self.max_pct);
    }

    pub fn reset(&mut self) {
        self.width_pct = self.default_width_pct;
        self.height_pct = self.default_height_pct;
        self.offset_x = 0;
        self.offset_y = 0;
        self.drag_anchor = None;
    }

    pub fn area(&self, viewport: Rect) -> Rect {
        modal_area(viewport, self.width_pct, self.height_pct, self.offset_x, self.offset_y)
    }
}

pub fn modal_area(viewport: Rect, width_pct: u16, height_pct: u16, offset_x: i16, offset_y: i16) -> Rect {
    let base = crate::ui::dashboard::centered_rect(width_pct, height_pct, viewport);
    apply_clamped_offset(base, viewport, offset_x, offset_y)
}

pub fn apply_clamped_offset(base: Rect, viewport: Rect, offset_x: i16, offset_y: i16) -> Rect {
    let min_x = viewport.x as i32;
    let min_y = viewport.y as i32;
    let max_x = viewport
        .x
        .saturating_add(viewport.width)
        .saturating_sub(4) as i32;
    let max_y = viewport
        .y
        .saturating_add(viewport.height)
        .saturating_sub(1) as i32;
    let x = (base.x as i32 + offset_x as i32).clamp(min_x, max_x.max(min_x));
    let y = (base.y as i32 + offset_y as i32).clamp(min_y, max_y.max(min_y));
    Rect { x: x as u16, y: y as u16, ..base }
}

pub fn title_row_contains(area: Rect, col: u16, row: u16) -> bool {
    row == area.y && col >= area.x && col < area.x.saturating_add(area.width)
}
```

- [ ] **Step 4: Prove Metrics layout parity with the helper**

In `src/ui/metrics.rs`, change `metrics_modal_rects` so the area is
computed with the shared helper:

```rust
let area = crate::ui::modal_chrome::modal_area(
    viewport,
    width_pct,
    height_pct,
    offset_x,
    offset_y,
);
```

Keep the rest of `metrics_modal_rects` unchanged.

- [ ] **Step 5: Run helper and Metrics geometry tests**

Run:

```bash
cargo test ui::modal_chrome::tests
cargo test ui::metrics::tests::modal_area_zero_offset_centers_modal
cargo test ui::metrics::tests::modal_area_right_clamp_keeps_4_cells_visible
cargo test app::metrics_overlay::tests::drag_title_row_updates_offset
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/ui/mod.rs src/ui/modal_chrome.rs src/ui/metrics.rs
git commit -m "ui: add shared modal geometry helpers"
```

## Task 3: Upgrade Token Insights Overlay Geometry

**Files:**
- Modify: `src/ui/insights.rs`
- Modify: `src/app/insights_overlay.rs`

- [ ] **Step 1: Add failing state and key tests**

Add these tests to `src/app/insights_overlay.rs`:

```rust
#[test]
fn bracket_keys_resize_insights_overlay() {
    let mut overlay = InsightsOverlay::new();
    overlay.open();
    let (w0, h0) = (overlay.width_pct(), overlay.height_pct());
    assert_eq!(handle_insights_overlay_key(&mut overlay, KeyCode::Char(']')), InsightsOverlayAction::None);
    assert!(overlay.width_pct() > w0);
    assert!(overlay.height_pct() > h0);
    assert_eq!(handle_insights_overlay_key(&mut overlay, KeyCode::Char('[')), InsightsOverlayAction::None);
    assert_eq!(handle_insights_overlay_key(&mut overlay, KeyCode::Char('=')), InsightsOverlayAction::None);
    assert_eq!(overlay.width_pct(), InsightsOverlay::DEFAULT_WIDTH_PCT);
    assert_eq!(overlay.height_pct(), InsightsOverlay::DEFAULT_HEIGHT_PCT);
}

#[test]
fn title_drag_moves_insights_overlay_and_reset_clears_offset() {
    let viewport = Rect::new(0, 0, 100, 40);
    let mut overlay = InsightsOverlay::new();
    overlay.open();
    let area = crate::ui::insights::insights_modal_area_for(viewport, &overlay);
    handle_insights_overlay_mouse(
        &mut overlay,
        viewport,
        mouse(MouseEventKind::Down(MouseButton::Left), area.x + 2, area.y),
    );
    handle_insights_overlay_mouse(
        &mut overlay,
        viewport,
        mouse(MouseEventKind::Drag(MouseButton::Left), area.x + 7, area.y + 3),
    );
    assert_eq!(overlay.offset_x(), 5);
    assert_eq!(overlay.offset_y(), 3);
    assert_eq!(handle_insights_overlay_key(&mut overlay, KeyCode::Char('=')), InsightsOverlayAction::None);
    assert_eq!(overlay.offset_x(), 0);
    assert_eq!(overlay.offset_y(), 0);
}
```

- [ ] **Step 2: Run the focused tests to verify they fail**

Run:

```bash
cargo test app::insights_overlay::tests::bracket_keys_resize_insights_overlay
cargo test app::insights_overlay::tests::title_drag_moves_insights_overlay_and_reset_clears_offset
```

Expected: each test fails on missing geometry methods.

- [ ] **Step 3: Add geometry state to `InsightsOverlay`**

In `src/ui/insights.rs`, import the shared types:

```rust
use crate::ui::modal_chrome::{DragAnchor, ModalGeometry};
```

Change the struct to:

```rust
#[derive(Debug, Clone)]
pub struct InsightsOverlay {
    open: bool,
    scroll: u16,
    geometry: ModalGeometry,
    snapshot: Option<InsightsSnapshot>,
    error: Option<String>,
    refreshed_label: Option<String>,
}
```

Add this `Default` implementation and remove `#[derive(Default)]`:

```rust
impl Default for InsightsOverlay {
    fn default() -> Self {
        Self {
            open: false,
            scroll: 0,
            geometry: ModalGeometry::new(
                Self::DEFAULT_WIDTH_PCT,
                Self::DEFAULT_HEIGHT_PCT,
                Self::SIZE_MIN,
                Self::SIZE_MAX,
                Self::SIZE_STEP,
            ),
            snapshot: None,
            error: None,
            refreshed_label: None,
        }
    }
}
```

Inside `impl InsightsOverlay`, add:

```rust
pub const SIZE_STEP: u16 = 5;
pub const SIZE_MIN: u16 = 50;
pub const SIZE_MAX: u16 = 99;
pub const DEFAULT_WIDTH_PCT: u16 = 86;
pub const DEFAULT_HEIGHT_PCT: u16 = 78;

pub fn width_pct(&self) -> u16 {
    self.geometry.width_pct()
}

pub fn height_pct(&self) -> u16 {
    self.geometry.height_pct()
}

pub fn offset_x(&self) -> i16 {
    self.geometry.offset_x()
}

pub fn offset_y(&self) -> i16 {
    self.geometry.offset_y()
}

pub fn drag_anchor(&self) -> Option<DragAnchor> {
    self.geometry.drag_anchor()
}

pub fn shrink(&mut self) {
    self.geometry.shrink();
}

pub fn grow(&mut self) {
    self.geometry.grow();
}

pub fn reset_geometry(&mut self) {
    self.geometry.reset();
}

pub fn begin_drag(&mut self, anchor: DragAnchor) {
    self.geometry.begin_drag(anchor);
}

pub fn set_offset(&mut self, x: i16, y: i16) {
    self.geometry.set_offset(x, y);
}

pub fn end_drag(&mut self) {
    self.geometry.end_drag();
}
```

In `open()` and `close()`, clear active drag state:

```rust
self.geometry.end_drag();
```

- [ ] **Step 4: Render Token Insights with shared geometry and close style**

Replace `insights_modal_area` in `src/ui/insights.rs` with:

```rust
pub fn insights_modal_area(viewport: ratatui::layout::Rect) -> ratatui::layout::Rect {
    crate::ui::modal_chrome::modal_area(
        viewport,
        InsightsOverlay::DEFAULT_WIDTH_PCT,
        InsightsOverlay::DEFAULT_HEIGHT_PCT,
        0,
        0,
    )
}

pub fn insights_modal_area_for(
    viewport: ratatui::layout::Rect,
    overlay: &InsightsOverlay,
) -> ratatui::layout::Rect {
    crate::ui::modal_chrome::modal_area(
        viewport,
        overlay.width_pct(),
        overlay.height_pct(),
        overlay.offset_x(),
        overlay.offset_y(),
    )
}
```

In `render_insights_modal`, change:

```rust
let area = insights_modal_area(frame.area());
```

to:

```rust
let area = insights_modal_area_for(frame.area(), overlay);
```

Change the `[x]` renderer to:

```rust
frame.render_widget(
    Paragraph::new("[x]").style(theme::modal_close_style()),
    close_button_rect(area),
);
```

- [ ] **Step 5: Add key and mouse geometry handling**

In `src/app/insights_overlay.rs`, add imports:

```rust
use crate::app::keymap::rect_contains;
use crate::ui::dashboard::close_button_rect;
use crate::ui::modal_chrome::{DragAnchor, title_row_contains};
```

In `handle_insights_overlay_key`, add these cases before close/scroll cases:

```rust
KeyCode::Char('[') => {
    overlay.shrink();
    InsightsOverlayAction::None
}
KeyCode::Char(']') => {
    overlay.grow();
    InsightsOverlayAction::None
}
KeyCode::Char('=') => {
    overlay.reset_geometry();
    InsightsOverlayAction::None
}
```

Replace the `MouseEventKind::Down(MouseButton::Left)` branch with:

```rust
MouseEventKind::Down(MouseButton::Left) => {
    let area = crate::ui::insights::insights_modal_area_for(viewport, overlay);
    let close = close_button_rect(area);
    if rect_contains(close, event.column, event.row)
        || !rect_contains(area, event.column, event.row)
    {
        overlay.close();
        return InsightsOverlayAction::None;
    }
    if title_row_contains(area, event.column, event.row) {
        overlay.begin_drag(DragAnchor {
            start_col: event.column,
            start_row: event.row,
            start_offset_x: overlay.offset_x(),
            start_offset_y: overlay.offset_y(),
        });
    }
    InsightsOverlayAction::None
}
MouseEventKind::Drag(MouseButton::Left) => {
    if let Some(anchor) = overlay.drag_anchor() {
        let dx = event.column as i32 - anchor.start_col as i32;
        let dy = event.row as i32 - anchor.start_row as i32;
        overlay.set_offset(
            (anchor.start_offset_x as i32 + dx).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            (anchor.start_offset_y as i32 + dy).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        );
    }
    InsightsOverlayAction::None
}
MouseEventKind::Up(MouseButton::Left) => {
    overlay.end_drag();
    InsightsOverlayAction::None
}
```

Keep the existing scroll branches.

- [ ] **Step 6: Run Token Insights focused tests**

Run:

```bash
cargo test app::insights_overlay::tests
cargo test ui::insights::tests
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/ui/insights.rs src/app/insights_overlay.rs
git commit -m "ui: make token insights overlay resizable"
```

## Task 4: Upgrade Anomaly Events Overlay Geometry

**Files:**
- Modify: `src/ui/anomaly_overlay.rs`
- Modify: `src/app/anomaly_overlay.rs`

- [ ] **Step 1: Add failing resize/drag tests**

Add these tests to `src/app/anomaly_overlay.rs`:

```rust
#[test]
fn bracket_keys_resize_anomaly_overlay() {
    let mut overlay = AnomalyOverlay::new();
    overlay.open();
    let (w0, h0) = (overlay.width_pct(), overlay.height_pct());
    assert!(handle_anomaly_overlay_key(&mut overlay, 0, KeyCode::Char(']')));
    assert!(overlay.width_pct() > w0);
    assert!(overlay.height_pct() > h0);
    assert!(handle_anomaly_overlay_key(&mut overlay, 0, KeyCode::Char('[')));
    assert!(handle_anomaly_overlay_key(&mut overlay, 0, KeyCode::Char('=')));
    assert_eq!(overlay.width_pct(), AnomalyOverlay::DEFAULT_WIDTH_PCT);
    assert_eq!(overlay.height_pct(), AnomalyOverlay::DEFAULT_HEIGHT_PCT);
}

#[test]
fn title_drag_moves_anomaly_overlay_and_reset_clears_offset() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    let mut overlay = AnomalyOverlay::new();
    overlay.open();
    let viewport = Rect::new(0, 0, 100, 40);
    let area = crate::ui::anomaly_overlay::anomaly_modal_area_for(viewport, &overlay);
    let down = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x + 2,
        row: area.y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let drag = MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: area.x + 8,
        row: area.y + 4,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(handle_anomaly_overlay_mouse(&mut overlay, viewport, 5, down));
    assert!(handle_anomaly_overlay_mouse(&mut overlay, viewport, 5, drag));
    assert_eq!(overlay.offset_x(), 6);
    assert_eq!(overlay.offset_y(), 4);
    assert!(handle_anomaly_overlay_key(&mut overlay, 0, KeyCode::Char('=')));
    assert_eq!(overlay.offset_x(), 0);
    assert_eq!(overlay.offset_y(), 0);
}
```

- [ ] **Step 2: Run the focused tests to verify they fail**

Run each test separately:

```bash
cargo test app::anomaly_overlay::tests::bracket_keys_resize_anomaly_overlay
cargo test app::anomaly_overlay::tests::title_drag_moves_anomaly_overlay_and_reset_clears_offset
```

Expected: fail on missing `width_pct`, `height_pct`, or
`anomaly_modal_area_for`.

- [ ] **Step 3: Add geometry state to `AnomalyOverlay`**

In `src/ui/anomaly_overlay.rs`, import:

```rust
use crate::ui::modal_chrome::{DragAnchor, ModalGeometry};
```

Change `AnomalyOverlay` to include:

```rust
geometry: ModalGeometry,
```

Set it in `Default`:

```rust
geometry: ModalGeometry::new(
    Self::DEFAULT_WIDTH_PCT,
    Self::DEFAULT_HEIGHT_PCT,
    Self::SIZE_MIN,
    Self::SIZE_MAX,
    Self::SIZE_STEP,
),
```

Inside `impl AnomalyOverlay`, add:

```rust
pub const SIZE_STEP: u16 = 5;
pub const SIZE_MIN: u16 = 50;
pub const SIZE_MAX: u16 = 99;
pub const DEFAULT_WIDTH_PCT: u16 = 80;
pub const DEFAULT_HEIGHT_PCT: u16 = 70;

pub fn width_pct(&self) -> u16 {
    self.geometry.width_pct()
}

pub fn height_pct(&self) -> u16 {
    self.geometry.height_pct()
}

pub fn offset_x(&self) -> i16 {
    self.geometry.offset_x()
}

pub fn offset_y(&self) -> i16 {
    self.geometry.offset_y()
}

pub fn drag_anchor(&self) -> Option<DragAnchor> {
    self.geometry.drag_anchor()
}

pub fn shrink(&mut self) {
    self.geometry.shrink();
}

pub fn grow(&mut self) {
    self.geometry.grow();
}

pub fn reset_geometry(&mut self) {
    self.geometry.reset();
}

pub fn begin_drag(&mut self, anchor: DragAnchor) {
    self.geometry.begin_drag(anchor);
}

pub fn set_offset(&mut self, x: i16, y: i16) {
    self.geometry.set_offset(x, y);
}

pub fn end_drag(&mut self) {
    self.geometry.end_drag();
}
```

Call `self.geometry.end_drag();` in both `open()` and `close()`.

- [ ] **Step 4: Render Anomaly Events with shared geometry and close style**

Replace `anomaly_modal_area` with:

```rust
pub fn anomaly_modal_area(viewport: Rect) -> Rect {
    crate::ui::modal_chrome::modal_area(
        viewport,
        AnomalyOverlay::DEFAULT_WIDTH_PCT,
        AnomalyOverlay::DEFAULT_HEIGHT_PCT,
        0,
        0,
    )
}

pub fn anomaly_modal_area_for(viewport: Rect, overlay: &AnomalyOverlay) -> Rect {
    crate::ui::modal_chrome::modal_area(
        viewport,
        overlay.width_pct(),
        overlay.height_pct(),
        overlay.offset_x(),
        overlay.offset_y(),
    )
}
```

In `render`, change:

```rust
let popup_area = anomaly_modal_area(area);
```

to:

```rust
let popup_area = anomaly_modal_area_for(area, self);
```

Change close button rendering to:

```rust
let close = Paragraph::new("[x]").style(theme::modal_close_style());
```

- [ ] **Step 5: Add key and mouse geometry handling**

In `src/app/anomaly_overlay.rs`, import:

```rust
use crate::app::keymap::rect_contains;
use crate::ui::dashboard::close_button_rect;
use crate::ui::modal_chrome::{DragAnchor, title_row_contains};
```

In `handle_anomaly_overlay_key`, add cases before close/scroll:

```rust
KeyCode::Char('[') => {
    overlay.shrink();
    true
}
KeyCode::Char(']') => {
    overlay.grow();
    true
}
KeyCode::Char('=') => {
    overlay.reset_geometry();
    true
}
```

In `handle_anomaly_overlay_mouse`, replace the left-down branch with:

```rust
MouseEventKind::Down(MouseButton::Left) => {
    let area = crate::ui::anomaly_overlay::anomaly_modal_area_for(viewport, overlay);
    let close = close_button_rect(area);
    if rect_contains(close, event.column, event.row) {
        overlay.close();
        return true;
    }
    if title_row_contains(area, event.column, event.row) {
        overlay.begin_drag(DragAnchor {
            start_col: event.column,
            start_row: event.row,
            start_offset_x: overlay.offset_x(),
            start_offset_y: overlay.offset_y(),
        });
    }
    true
}
MouseEventKind::Drag(MouseButton::Left) => {
    if let Some(anchor) = overlay.drag_anchor() {
        let dx = event.column as i32 - anchor.start_col as i32;
        let dy = event.row as i32 - anchor.start_row as i32;
        overlay.set_offset(
            (anchor.start_offset_x as i32 + dx).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            (anchor.start_offset_y as i32 + dy).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        );
    }
    true
}
MouseEventKind::Up(MouseButton::Left) => {
    overlay.end_drag();
    true
}
```

Keep existing wheel scroll behavior.

- [ ] **Step 6: Run Anomaly focused tests**

Run:

```bash
cargo test app::anomaly_overlay::tests
cargo test ui::anomaly_overlay::tests
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/ui/anomaly_overlay.rs src/app/anomaly_overlay.rs
git commit -m "ui: make anomaly events overlay resizable"
```

## Task 5: Update Help and Docs for First-Slice Contract

**Files:**
- Modify: `src/ui/dashboard.rs`
- Modify: `docs/ai/UI_MANUAL.md`
- Modify: `docs/ai/ARCHITECTURE.md`
- Modify: `docs/ai/VALIDATION.md`

- [ ] **Step 1: Add failing Help tests**

In `src/ui/dashboard.rs`, add this test near the existing help tests:

```rust
#[test]
fn help_documents_overlay_chrome_contract() {
    let lines: Vec<String> = help_lines().into_iter().map(line_text).collect();
    let dump = lines.join("\n");
    assert!(
        dump.contains("[/] resize") && dump.contains("title drag move") && dump.contains("= reset geometry"),
        "Help should document the shared overlay chrome contract; got:\n{dump}"
    );
    assert!(
        dump.contains("i") && dump.contains("n") && dump.contains("resizable"),
        "Help should mark Token Insights and Anomaly Events as resizable overlays; got:\n{dump}"
    );
}
```

- [ ] **Step 2: Run the failing Help test**

Run:

```bash
cargo test ui::dashboard::tests::help_documents_overlay_chrome_contract
```

Expected: fail because Help does not yet use the contract wording.

- [ ] **Step 3: Update Help wording**

In `help_lines_for_width`, add a row after `Modal [x]`:

```rust
(
    "Overlay chrome",
    "large overlays use [/] resize, = reset geometry, title drag move, wheel/↑/↓ scroll, same entry key/Esc/q/[x] close",
),
```

Update the `m`, `n`, and `i` help rows so they contain:

```text
resizable
```

and do not contradict the shared row.

- [ ] **Step 4: Update `docs/ai/UI_MANUAL.md`**

Add this subsection under the controls/overlays area:

```markdown
### Overlay chrome contract

Large persistent overlays share the same chrome controls:
`[` / `]` resize, `=` resets size and position, title-row drag moves
the modal, mouse wheel over the modal body and `↑` / `↓` scroll, the
same entry key closes the overlay, and `[x]` / `Esc` / `q` close where
the overlay is not in an edit sub-mode. In the first
chrome-consistency slice this applies to `m` Metrics, `a` Pending
Actions, `i` Token Insights, and `n` Anomaly Events. `S` Settings keeps
edit-mode guards, and short confirmation modals intentionally remain
non-resizable.
```

- [ ] **Step 5: Update `docs/ai/ARCHITECTURE.md`**

Add this paragraph near the Phase 8/overlay UI notes:

```markdown
The overlay chrome consistency slice adds a shared UI-only geometry
layer for modal size, offset, close styling, and title-row dragging.
It does not change provider adapters, policy evaluation, audit schema,
or recommendation semantics.
```

- [ ] **Step 6: Update `docs/ai/VALIDATION.md`**

Add this checklist item near the Phase 8 UI validation items:

```markdown
- [x] Overlay chrome first slice: shared `[x]` style uses active-border
      color rather than severity color; `i` Token Insights and `n`
      Anomaly Events support `[`/`]` resize, `=` geometry reset,
      title-row drag, same-key close, body-local wheel/arrow scroll,
      `[x]` close parity with `m`/`a`, and footer `focus: overlay`
      while any key-owning overlay is open.
```

- [ ] **Step 7: Run docs/help tests**

Run:

```bash
cargo test ui::dashboard::tests::help_documents_overlay_chrome_contract
cargo test ui::dashboard::tests::help_documents_modal_x_close
git diff --check
```

Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add src/ui/dashboard.rs docs/ai/UI_MANUAL.md docs/ai/ARCHITECTURE.md docs/ai/VALIDATION.md
git commit -m "docs: document overlay chrome contract"
```

## Task 6: Full Verification

**Files:**
- No source edits unless verification exposes a real issue.

- [ ] **Step 1: Run formatter**

Run:

```bash
cargo fmt --all
cargo fmt --all --check
```

Expected: both commands exit 0.

- [ ] **Step 2: Run full test suite**

Run:

```bash
cargo test --all-targets
```

Expected: exit 0 with all lib and integration tests passing.

- [ ] **Step 3: Run clippy**

Run:

```bash
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

Expected: exit 0.

- [ ] **Step 4: Run diff and status checks**

Run:

```bash
git diff --check
git status --short --branch
```

Expected: `git diff --check` exits 0. `git status --short --branch`
shows only intended committed work or a clean tree ahead of origin.

- [ ] **Step 5: Final verification commit if needed**

If formatting or docs-only fixes were required after Task 5, commit
them:

```bash
git add src docs
git commit -m "chore: verify overlay chrome consistency"
```

If no files changed after Task 5, do not create an empty commit.

## Self-Review Notes

- Spec coverage: this plan covers the accepted first slice: close style,
  shared geometry helper, `i`/`n` resize+drag, and docs/help/validation.
  The spec's later migration targets (`S`, `P`, Help/Git geometry,
  Target picker split resize) remain out of this first implementation
  slice by design and require a follow-up plan.
- Placeholder scan: no task relies on unspecified test names, file paths,
  or undefined helper names. Every new helper used by later tasks is
  defined in Task 2 or in the target overlay task before use.
- Type consistency: the plan consistently uses
  `ModalGeometry`, `DragAnchor`, `modal_area`, `title_row_contains`,
  `modal_close_style`, `insights_modal_area_for`, and
  `anomaly_modal_area_for`.
