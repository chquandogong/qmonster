# Insights Overlay Loading Spinner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the `i` Token Insights overlay open / `r` refresh path from a synchronous SQLite snapshot call to a worker-thread + mpsc-channel fetch, and render a centered rotating spinner placeholder while the snapshot is in flight.

**Architecture:** Each `i` open and `r` refresh increments a per-Context `next_insights_request_id` and spawns a `std::thread` that opens `SqliteInsightsStore` read-only, runs `snapshot_with_ignored_ttl`, and sends `(request_id, Result<InsightsSnapshot, String>)` on an `mpsc::Sender<InsightsLoadOutcome>` owned by `Context`. The TUI main loop drains the receiver each `event::poll` iteration and calls `overlay.set_snapshot_for(id, result)`; outcomes whose id is not the current `pending_request_id` are dropped. The overlay's render branch shows a centered `Aggregating insights ⠋` line with phase advancing on each iteration via `overlay.advance_spinner()`.

**Tech Stack:** Rust 2021, ratatui, crossterm, rusqlite (existing); `std::sync::mpsc` and `std::thread::spawn` for the worker pattern; `std::panic::catch_unwind` for thread-panic safety.

**Reference spec:** `docs/superpowers/specs/2026-05-07-insights-overlay-loading-spinner-design.md`

---

## File Structure

| File                             | Responsibility                                                                                                                                   |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `src/ui/insights.rs`             | `InsightsOverlay` state (loading flag, spinner phase, pending_request_id) + render branch + spinner glyph constant                               |
| `src/app/insights_load.rs` (new) | `InsightsLoadOutcome` struct + `spawn_insights_load()` worker spawner; isolated to keep thread/channel surface area visible                      |
| `src/app/bootstrap.rs`           | `Context` gains `insights_load_tx/rx` mpsc pair + `next_insights_request_id` counter                                                             |
| `src/app/tui_loop.rs`            | Replace synchronous `refresh_insights_overlay` with async dispatch (spawn worker, mark loading); add tick-time channel drain + `advance_spinner` |
| `src/lib.rs`                     | Export `insights_load` from the `app` module                                                                                                     |
| `docs/ai/UI_MANUAL.md`           | §8.9 — note async fetch + spinner placeholder                                                                                                    |
| `docs/ai/ARCHITECTURE.md`        | v1.50.0 paragraph — worker-thread fetch path                                                                                                     |
| `docs/ai/VALIDATION.md`          | v1.50.0 row + evidence line                                                                                                                      |

---

## Task 1: Loading state on `InsightsOverlay`

Adds the three new fields (`loading`, `pending_request_id`, `spinner_phase`) and the `mark_loading` entry point. No render change yet; tests assert state only.

**Files:**

- Modify: `src/ui/insights.rs`

- [ ] **Step 1: Add the failing tests**

Open `src/ui/insights.rs` and inside `mod tests` (the existing `#[cfg(test)] mod tests { ... }` block at the bottom of the file), add:

```rust
    #[test]
    fn mark_loading_sets_loading_flag_and_pending_id() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        assert!(!overlay.is_loading());
        overlay.mark_loading(7);
        assert!(overlay.is_loading());
        assert_eq!(overlay.pending_request_id(), 7);
    }

    #[test]
    fn mark_loading_clears_snapshot_and_error() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.set_snapshot(
            empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            }),
            "12:00:00".into(),
        );
        overlay.mark_loading(1);
        let joined = overlay.lines().join("\n");
        assert!(!joined.contains("Action Ledger"));
        assert!(!joined.contains("12:00:00"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib --no-fail-fast 'mark_loading_'
```

Expected: compile error `no method named mark_loading found`, `no method named is_loading found`, `no method named pending_request_id found`.

- [ ] **Step 3: Add fields and methods**

In the `pub struct InsightsOverlay` declaration (around line 11), add three new fields:

```rust
    loading: bool,
    spinner_phase: u8,
    pending_request_id: u64,
```

In `impl Default for InsightsOverlay::default()` (around line 21), add the new field initializers alongside the existing ones:

```rust
            loading: false,
            spinner_phase: 0,
            pending_request_id: 0,
```

In `impl InsightsOverlay`, add three new methods just after `pub fn close(&mut self)` (around line 64):

```rust
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn pending_request_id(&self) -> u64 {
        self.pending_request_id
    }

    pub fn mark_loading(&mut self, request_id: u64) {
        self.loading = true;
        self.spinner_phase = 0;
        self.pending_request_id = request_id;
        self.snapshot = None;
        self.error = None;
        self.refreshed_label = None;
        self.scroll = 0;
    }
```

Also extend `pub fn close(&mut self)` to clear the loading flag so a stale outcome arriving after close is silently dropped (the id check in Task 3 also covers this, but clearing here keeps the close→reopen path clean):

```rust
    pub fn close(&mut self) {
        self.open = false;
        self.scroll = 0;
        self.loading = false;
        self.geometry.end_drag();
    }
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib --no-fail-fast 'mark_loading_'
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/ui/insights.rs
git commit -m "feat(insights): add loading state + mark_loading on InsightsOverlay"
```

---

## Task 2: Spinner glyph + `advance_spinner`

Adds the `SPINNER_FRAMES` constant and the phase-advancement method. Render still doesn't use them; this just lands the rotation primitive.

**Files:**

- Modify: `src/ui/insights.rs`

- [ ] **Step 1: Add the failing tests**

Inside `mod tests` in `src/ui/insights.rs`, add:

```rust
    #[test]
    fn advance_spinner_cycles_through_braille_phases() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(1);
        assert_eq!(overlay.spinner_glyph(), '⠋');
        overlay.advance_spinner();
        assert_eq!(overlay.spinner_glyph(), '⠙');
        for _ in 0..8 {
            overlay.advance_spinner();
        }
        assert_eq!(overlay.spinner_glyph(), '⠋');
    }

    #[test]
    fn advance_spinner_is_no_op_when_not_loading() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        let before = overlay.spinner_glyph();
        overlay.advance_spinner();
        assert_eq!(overlay.spinner_glyph(), before);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib --no-fail-fast advance_spinner_
```

Expected: compile error `no method named spinner_glyph`, `no method named advance_spinner`.

- [ ] **Step 3: Add the constant and methods**

Just below the existing `use` block in `src/ui/insights.rs` (above `pub struct InsightsOverlay`), add:

```rust
const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
```

Inside `impl InsightsOverlay`, add two more methods adjacent to `mark_loading`:

```rust
    pub fn spinner_glyph(&self) -> char {
        SPINNER_FRAMES[(self.spinner_phase as usize) % SPINNER_FRAMES.len()]
    }

    pub fn advance_spinner(&mut self) {
        if !self.loading {
            return;
        }
        self.spinner_phase = (self.spinner_phase + 1) % SPINNER_FRAMES.len() as u8;
    }
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib --no-fail-fast advance_spinner_
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/ui/insights.rs
git commit -m "feat(insights): add SPINNER_FRAMES + advance_spinner phase helper"
```

---

## Task 3: `set_snapshot_for` accept/drop by request_id

The new entry point that the tui_loop calls when an outcome arrives via the channel. Stale outcomes (mismatched `request_id` or overlay closed) are silently dropped; matching outcomes swap the overlay to Loaded or Failed.

**Files:**

- Modify: `src/ui/insights.rs`

- [ ] **Step 1: Add the failing tests**

Inside `mod tests` in `src/ui/insights.rs`, add:

```rust
    #[test]
    fn set_snapshot_for_drops_stale_request_id() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(2);
        overlay.set_snapshot_for(
            1,
            Ok(empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            })),
            "stale".into(),
        );
        assert!(overlay.is_loading());
        assert_eq!(overlay.pending_request_id(), 2);
    }

    #[test]
    fn set_snapshot_for_accepts_matching_request_id_with_ok() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(2);
        overlay.set_snapshot_for(
            2,
            Ok(empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            })),
            "12:00:00".into(),
        );
        assert!(!overlay.is_loading());
        let joined = overlay.lines().join("\n");
        assert!(joined.contains("refreshed: 12:00:00"));
    }

    #[test]
    fn set_snapshot_for_accepts_matching_request_id_with_err() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(3);
        overlay.set_snapshot_for(3, Err("boom".into()), "n/a".into());
        assert!(!overlay.is_loading());
        let joined = overlay.lines().join("\n");
        assert!(joined.contains("error: boom"));
    }

    #[test]
    fn close_while_loading_drops_late_outcome() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(4);
        overlay.close();
        overlay.set_snapshot_for(
            4,
            Ok(empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            })),
            "12:00:00".into(),
        );
        overlay.open();
        let joined = overlay.lines().join("\n");
        assert!(!joined.contains("refreshed: 12:00:00"));
        assert!(!joined.contains("Action Ledger"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib --no-fail-fast set_snapshot_for_ close_while_loading_
```

Expected: compile error `no method named set_snapshot_for`.

- [ ] **Step 3: Add the method**

Inside `impl InsightsOverlay`, add this method just after the existing `set_error`:

```rust
    pub fn set_snapshot_for(
        &mut self,
        request_id: u64,
        result: Result<InsightsSnapshot, String>,
        refreshed_label: String,
    ) {
        if !self.open || !self.loading || self.pending_request_id != request_id {
            return;
        }
        self.loading = false;
        match result {
            Ok(snapshot) => self.set_snapshot(snapshot, refreshed_label),
            Err(error) => self.set_error(error),
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib --no-fail-fast set_snapshot_for_ close_while_loading_
```

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/ui/insights.rs
git commit -m "feat(insights): add set_snapshot_for with request_id accept/drop"
```

---

## Task 4: Loading branch in `render_insights_modal`

Render a centered `Aggregating insights ⠋` line while loading. Other panels are suppressed. The block title gains a ` — loading` suffix.

**Files:**

- Modify: `src/ui/insights.rs`

- [ ] **Step 1: Add the failing test**

Inside `mod tests` in `src/ui/insights.rs`, add:

```rust
    #[test]
    fn loading_state_renders_centered_spinner_and_text() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let viewport = ratatui::layout::Rect::new(0, 0, 100, 40);
        let mut terminal =
            Terminal::new(TestBackend::new(viewport.width, viewport.height)).unwrap();
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(1);

        terminal
            .draw(|frame| render_insights_modal(frame, &overlay))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut joined = String::new();
        for y in 0..viewport.height {
            for x in 0..viewport.width {
                if let Some(cell) = buffer.cell((x, y)) {
                    joined.push_str(cell.symbol());
                }
            }
            joined.push('\n');
        }
        assert!(joined.contains("Aggregating insights"), "buffer:\n{joined}");
        assert!(joined.contains('⠋'), "buffer:\n{joined}");
        assert!(!joined.contains("Action Ledger"), "buffer:\n{joined}");
        assert!(!joined.contains("CACHE TREND"), "buffer:\n{joined}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --lib --no-fail-fast loading_state_renders_centered_spinner_and_text
```

Expected: assertion failure — buffer does not contain `Aggregating insights` (current render path falls through to the empty-snapshot branch which prints `No insights snapshot loaded.`).

- [ ] **Step 3: Add the loading render branch**

Add the following imports if not already present (top of `src/ui/insights.rs`):

```rust
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
```

(The existing imports include `ratatui::layout` via `Rect` references inside helper functions; these add `Alignment`, `Constraint`, `Direction`, `Layout` for the centering layout.)

Modify `pub fn render_insights_modal` (around line 187). Replace the existing function body. Find:

```rust
pub fn render_insights_modal(frame: &mut Frame<'_>, overlay: &InsightsOverlay) {
    let area = insights_modal_area_for(frame.area(), overlay);
    frame.render_widget(Clear, area);
    let title = " Token Insights [i/Esc/q close] [r refresh] ";
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(area),
    );

    let lines = overlay.lines();
    if lines.len() <= 4 && overlay.snapshot.is_none() {
        let paragraph = Paragraph::new(lines.join("\n")).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
        return;
    }

    let items: Vec<ListItem> = lines
        .into_iter()
        .skip(overlay.scroll as usize)
        .take(inner.height as usize)
        .map(ListItem::new)
        .collect();
    frame.render_widget(List::new(items), inner);
}
```

Replace with:

```rust
pub fn render_insights_modal(frame: &mut Frame<'_>, overlay: &InsightsOverlay) {
    let area = insights_modal_area_for(frame.area(), overlay);
    frame.render_widget(Clear, area);
    let title = if overlay.is_loading() {
        " Token Insights — loading [i/Esc/q close] "
    } else {
        " Token Insights [i/Esc/q close] [r refresh] "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(area),
    );

    if overlay.is_loading() {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);
        let line = format!("Aggregating insights {}", overlay.spinner_glyph());
        let paragraph = Paragraph::new(line)
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(theme::BORDER_ACTIVE)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(paragraph, rows[1]);
        return;
    }

    let lines = overlay.lines();
    if lines.len() <= 4 && overlay.snapshot.is_none() {
        let paragraph = Paragraph::new(lines.join("\n")).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
        return;
    }

    let items: Vec<ListItem> = lines
        .into_iter()
        .skip(overlay.scroll as usize)
        .take(inner.height as usize)
        .map(ListItem::new)
        .collect();
    frame.render_widget(List::new(items), inner);
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --lib --no-fail-fast loading_state_renders_centered_spinner_and_text
```

Expected: 1 test passes.

- [ ] **Step 5: Run full insights tests to confirm no regression**

```bash
cargo test --lib --no-fail-fast insights
```

Expected: all `ui::insights::tests::*` tests pass (including the existing `render_uses_active_border_style`, `snapshot_lines_include_action_ledger`, etc.).

- [ ] **Step 6: Commit**

```bash
git add src/ui/insights.rs
git commit -m "feat(insights): render centered spinner placeholder while loading"
```

---

## Task 5: `line_count` returns 0 while loading

Suppresses the scroll surface during loading so `j`/`k`/wheel are no-ops.

**Files:**

- Modify: `src/ui/insights.rs`

- [ ] **Step 1: Add the failing test**

Inside `mod tests` in `src/ui/insights.rs`, add:

```rust
    #[test]
    fn line_count_is_zero_while_loading() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.mark_loading(1);
        assert_eq!(overlay.line_count(), 0);
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --lib --no-fail-fast line_count_is_zero_while_loading
```

Expected: assertion failure — `line_count` currently returns the empty-state placeholder count (4 lines).

- [ ] **Step 3: Update `line_count`**

In `src/ui/insights.rs`, find:

```rust
    pub fn line_count(&self) -> usize {
        self.lines().len()
    }
```

Replace with:

```rust
    pub fn line_count(&self) -> usize {
        if self.loading {
            return 0;
        }
        self.lines().len()
    }
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --lib --no-fail-fast line_count_is_zero_while_loading
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/ui/insights.rs
git commit -m "feat(insights): line_count returns 0 while loading to disable scroll"
```

---

## Task 6: `src/app/insights_load.rs` — worker spawner

Creates the worker module: `InsightsLoadOutcome` struct + `spawn_insights_load` that runs the SQLite snapshot off-thread and emits the outcome on a channel. Includes a `catch_unwind` guard so a thread panic surfaces as `Err` instead of leaking.

**Files:**

- Create: `src/app/insights_load.rs`
- Modify: `src/app/mod.rs` (export the module)
- Test: same file (tests live inside the module)

- [ ] **Step 1: Create the module skeleton + failing tests**

Create `src/app/insights_load.rs` with this content:

```rust
//! Worker-thread fetch path for the `i` Token Insights overlay
//! (v1.50.0). Each `i` open / `r` refresh spawns one thread that
//! opens the audit DB read-only, runs `snapshot_with_ignored_ttl`,
//! and emits an `InsightsLoadOutcome` on the supplied channel. Stale
//! outcomes (whose `request_id` no longer matches the overlay's
//! pending id) are dropped on receipt by `InsightsOverlay::set_
//! snapshot_for`.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::store::{InsightsSnapshot, InsightsWindow, SqliteInsightsStore};

#[derive(Debug)]
pub struct InsightsLoadOutcome {
    pub request_id: u64,
    pub result: Result<InsightsSnapshot, String>,
}

pub fn spawn_insights_load(
    audit_path: PathBuf,
    window: InsightsWindow,
    ignored_ttl_secs: u64,
    sender: Sender<InsightsLoadOutcome>,
    request_id: u64,
) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_insights_load(&audit_path, window, ignored_ttl_secs)
        }))
        .unwrap_or_else(|_| Err("insights load thread panicked".into()));

        // Channel disconnect (Receiver dropped) means the runtime is
        // shutting down — drop the outcome silently.
        let _ = sender.send(InsightsLoadOutcome { request_id, result });
    });
}

fn run_insights_load(
    audit_path: &PathBuf,
    window: InsightsWindow,
    ignored_ttl_secs: u64,
) -> Result<InsightsSnapshot, String> {
    if !audit_path.exists() {
        return Ok(crate::insights_report::empty_insights_snapshot(window));
    }
    SqliteInsightsStore::open_read_only(audit_path)
        .and_then(|store| store.snapshot_with_ignored_ttl(window, ignored_ttl_secs))
        .map_err(|e| format!("open/query {} failed: {e}", audit_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AuditDb;
    use std::sync::mpsc;
    use std::time::Duration;

    fn recv_outcome(rx: &mpsc::Receiver<InsightsLoadOutcome>) -> InsightsLoadOutcome {
        rx.recv_timeout(Duration::from_secs(5))
            .expect("worker thread should send outcome within 5s")
    }

    #[test]
    fn spawn_insights_load_emits_outcome_with_matching_request_id() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("audit.db");
        // Initialize the schema by opening + dropping a writer.
        let _ = AuditDb::open(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        let window = InsightsWindow {
            since_ms: 0,
            until_ms: 1_000_000,
        };

        spawn_insights_load(path.clone(), window, 1800, tx, 42);

        let outcome = recv_outcome(&rx);
        assert_eq!(outcome.request_id, 42);
        assert!(outcome.result.is_ok(), "result was {:?}", outcome.result);
    }

    #[test]
    fn spawn_insights_load_emits_ok_for_missing_db() {
        let tmp = tempfile::TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist.db");
        let (tx, rx) = mpsc::channel();
        let window = InsightsWindow {
            since_ms: 0,
            until_ms: 1_000_000,
        };

        spawn_insights_load(missing, window, 1800, tx, 7);

        let outcome = recv_outcome(&rx);
        assert_eq!(outcome.request_id, 7);
        assert!(outcome.result.is_ok());
    }

    #[test]
    fn spawn_insights_load_emits_err_when_path_is_a_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        // A directory exists but is not a SQLite file -> open_read_only should error.
        let (tx, rx) = mpsc::channel();
        let window = InsightsWindow {
            since_ms: 0,
            until_ms: 1_000_000,
        };

        spawn_insights_load(tmp.path().to_path_buf(), window, 1800, tx, 9);

        let outcome = recv_outcome(&rx);
        assert_eq!(outcome.request_id, 9);
        assert!(outcome.result.is_err(), "result was {:?}", outcome.result);
    }
}
```

- [ ] **Step 2: Export the module**

Open `src/app/mod.rs` and add `pub mod insights_load;` next to the existing `pub mod insights_overlay;` (alphabetical order is fine). Find the existing line:

```bash
grep -n "pub mod insights" src/app/mod.rs
```

Add a new line `pub mod insights_load;` immediately above (or below) `pub mod insights_overlay;`.

- [ ] **Step 3: Run tests to verify they pass**

```bash
cargo test --lib --no-fail-fast spawn_insights_load
```

Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/app/insights_load.rs src/app/mod.rs
git commit -m "feat(insights): add insights_load worker module with mpsc fetch"
```

---

## Task 7: `Context` channel + request id

Adds the mpsc pair and the request id counter. The channel is created lazily — a single `mpsc::channel()` call in `Context::new`.

**Files:**

- Modify: `src/app/bootstrap.rs`

- [ ] **Step 1: Add the failing test**

Tests for `Context` use the integration-test fixtures in
`tests/event_loop_integration.rs` (`FixturePaneSource`,
`RecordingNotifier`, `InMemorySink`). The in-source `mod tests` in
`src/app/bootstrap.rs` does not have lightweight fixtures (see the
existing `auto_snapshot_dedup_type_is_accessible` fallback test
around line 285 — comment notes the same constraint).

Add the following test to `tests/event_loop_integration.rs` at the
end of the file (above any module-level closing braces):

```rust
#[test]
fn context_initializes_insights_load_channel_and_request_id() {
    let source = FixturePaneSource { panes: vec![] };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    assert_eq!(ctx.next_insights_request_id, 0);
    ctx.insights_load_tx
        .send(qmonster::app::insights_load::InsightsLoadOutcome {
            request_id: 99,
            result: Err("ping".into()),
        })
        .unwrap();
    let outcome = ctx.insights_load_rx.recv().unwrap();
    assert_eq!(outcome.request_id, 99);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --lib --no-fail-fast context_initializes_insights_load_channel
```

Expected: compile error — `next_insights_request_id`, `insights_load_tx`, `insights_load_rx` do not exist.

- [ ] **Step 3: Add fields and channel init**

In `src/app/bootstrap.rs`, find the `pub struct Context<P: PaneSource, N: NotifyBackend>` declaration (around line 28). Add three new public fields adjacent to the existing `insights_db_path: Option<std::path::PathBuf>,` field:

```rust
    pub insights_load_tx: std::sync::mpsc::Sender<crate::app::insights_load::InsightsLoadOutcome>,
    pub insights_load_rx: std::sync::mpsc::Receiver<crate::app::insights_load::InsightsLoadOutcome>,
    pub next_insights_request_id: u64,
```

In `impl<P: PaneSource, N: NotifyBackend> Context<P, N>`, modify `pub fn new(...)` to initialize the channel. Find the existing `pub fn new(...)` (around line 172) and add at the top of its body, before the `Self {` literal:

```rust
        let (insights_load_tx, insights_load_rx) = std::sync::mpsc::channel();
```

Then add to the `Self { ... }` literal initializer (after `insights_db_path: None,`):

```rust
            insights_load_tx,
            insights_load_rx,
            next_insights_request_id: 0,
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --test event_loop_integration context_initializes_insights_load_channel_and_request_id
```

Expected: 1 test passes.

- [ ] **Step 5: Run the full integration test suite**

```bash
cargo test --test event_loop_integration --no-fail-fast
```

Expected: all existing integration tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src/app/bootstrap.rs tests/event_loop_integration.rs
git commit -m "feat(insights): wire Context insights_load channel + request_id counter"
```

---

## Task 8: tui_loop async dispatch on `i` open and `r` refresh

Replaces the synchronous `refresh_insights_overlay` body with a worker-thread spawn. The function name stays for caller compatibility but its body now (1) computes the snapshot window, (2) increments `ctx.next_insights_request_id`, (3) calls `overlay.mark_loading(id)`, (4) spawns the worker via `insights_load::spawn_insights_load`.

**Files:**

- Modify: `src/app/tui_loop.rs`

- [ ] **Step 1: Identify the existing function and its callers**

```bash
grep -n "refresh_insights_overlay\|fn refresh_insights_overlay" src/app/tui_loop.rs
```

Confirm there are exactly two call sites: the `Refresh` action handler (around line 374) and the `i` open dispatch (around line 572). The function definition is around line 1021.

- [ ] **Step 2: Replace the function body**

In `src/app/tui_loop.rs`, find the function `fn refresh_insights_overlay<P, N>(...)` (around line 1021) and replace its entire body. The new function takes `ctx: &mut Context<P, N>` (mutable so we can bump the request id):

Find:

```rust
fn refresh_insights_overlay<P, N>(
    overlay: &mut crate::ui::insights::InsightsOverlay,
    ctx: &Context<P, N>,
) where
    P: PaneSource,
    N: NotifyBackend,
{
    let Some(path) = ctx.insights_db_path.as_ref() else {
        overlay.set_error("insights database path is unavailable in this runtime");
        return;
    };
    let now_ms = crate::app::event_loop::current_unix_ms();
    let window_ms = i64::try_from(u128::from(ctx.config.insights.default_window_secs) * 1000)
        .unwrap_or(i64::MAX);
    let window = crate::store::InsightsWindow {
        since_ms: now_ms.saturating_sub(window_ms),
        until_ms: now_ms,
    };
    let snapshot = if path.exists() {
        match crate::store::SqliteInsightsStore::open_read_only(path).and_then(|store| {
            store.snapshot_with_ignored_ttl(window, ctx.config.insights.ignored_ttl_secs)
        }) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                overlay.set_error(format!("open/query {} failed: {e}", path.display()));
                return;
            }
        }
    } else {
        crate::insights_report::empty_insights_snapshot(window)
    };
    let refreshed_label = chrono::Local::now().format("%H:%M:%S").to_string();
    overlay.set_snapshot(snapshot, refreshed_label);
}
```

Replace with:

```rust
fn refresh_insights_overlay<P, N>(
    overlay: &mut crate::ui::insights::InsightsOverlay,
    ctx: &mut Context<P, N>,
) where
    P: PaneSource,
    N: NotifyBackend,
{
    let Some(path) = ctx.insights_db_path.clone() else {
        overlay.set_error("insights database path is unavailable in this runtime");
        return;
    };
    let now_ms = crate::app::event_loop::current_unix_ms();
    let window_ms = i64::try_from(u128::from(ctx.config.insights.default_window_secs) * 1000)
        .unwrap_or(i64::MAX);
    let window = crate::store::InsightsWindow {
        since_ms: now_ms.saturating_sub(window_ms),
        until_ms: now_ms,
    };
    ctx.next_insights_request_id = ctx.next_insights_request_id.wrapping_add(1);
    let request_id = ctx.next_insights_request_id;
    overlay.mark_loading(request_id);
    crate::app::insights_load::spawn_insights_load(
        path,
        window,
        ctx.config.insights.ignored_ttl_secs,
        ctx.insights_load_tx.clone(),
        request_id,
    );
}
```

- [ ] **Step 3: Update the two call sites to pass `&mut ctx`**

Find the call site near line 374 (Refresh action handler):

```rust
                                    refresh_insights_overlay(&mut insights_overlay, ctx);
```

Confirm `ctx` is already `&mut` in scope (it is in the run loop). If the existing line passes a non-mut `ctx`, change it. Modern Rust will compile if `ctx` is bound `let mut ctx = ...` — verify by searching upward for the binding:

```bash
grep -n "let .*ctx" src/app/tui_loop.rs | head -5
```

Around line 67 you should see `let mut ctx = ctx_in;` or similar. The two existing call sites (`refresh_insights_overlay(&mut insights_overlay, ctx);` at lines 374 and 572) should now compile with the new `&mut ctx` requirement because the borrow already permits it. If a borrow-checker error fires, the call sites remain on the same lines — re-emit them as `refresh_insights_overlay(&mut insights_overlay, ctx);` (the borrow of `ctx` is mutable now because the function takes `&mut Context`).

- [ ] **Step 4: Run lib tests to confirm the codebase still compiles**

```bash
cargo build --lib
```

Expected: clean build. If the call sites fail to borrow `ctx` mutably, ensure `ctx` is bound `let mut ctx` at the loop entry (around line 67); search via `grep -n "fn run_event_loop\|fn run_tui_loop\|fn run<" src/app/tui_loop.rs` and inspect.

- [ ] **Step 5: Run the relevant lib tests**

```bash
cargo test --lib --no-fail-fast tui_loop insights
```

Expected: all `tui_loop` and `insights` tests pass. Existing tests for `refresh_insights_overlay` (if any) may need updating in the next task to match the async model — if they assert `overlay.snapshot.is_some()` synchronously after `refresh_insights_overlay`, they will now fail. Note any failures and fix in Task 10.

- [ ] **Step 6: Commit**

```bash
git add src/app/tui_loop.rs
git commit -m "feat(insights): dispatch i open + r refresh through worker thread"
```

---

## Task 9: tui_loop tick polling + spinner advancement

Drains `ctx.insights_load_rx` non-blocking on each iteration of the event loop, calls `overlay.set_snapshot_for(id, result, refreshed_label)` for each outcome, and advances the spinner when the overlay is loading.

**Files:**

- Modify: `src/app/tui_loop.rs`

- [ ] **Step 1: Locate the loop body**

```bash
grep -n "event::poll(Duration::from_millis(100))" src/app/tui_loop.rs
```

You should find the call around line 237. The drain + spinner advance must run on every iteration of the loop, regardless of whether `event::poll` returned an event. We add it just before the existing `if event::poll(...)?` block so the channel is drained once per iteration before the next event handling.

- [ ] **Step 2: Add the drain + advance block**

In `src/app/tui_loop.rs`, just above `if event::poll(Duration::from_millis(100))? {` (around line 237), add:

```rust
                while let Ok(outcome) = ctx.insights_load_rx.try_recv() {
                    let label = chrono::Local::now().format("%H:%M:%S").to_string();
                    insights_overlay.set_snapshot_for(outcome.request_id, outcome.result, label);
                }
                if insights_overlay.is_loading() {
                    insights_overlay.advance_spinner();
                }
```

(Indentation must match the surrounding lines — use the same number of spaces as the existing `if event::poll(...)` line.)

- [ ] **Step 3: Add an integration test that simulates an outcome arriving on the channel**

This test exercises the drain logic via the public API on `Context`.
Add to `tests/event_loop_integration.rs` (same fixtures as Task 7's
test):

```rust
#[test]
fn insights_load_channel_supports_try_recv_drain() {
    let source = FixturePaneSource { panes: vec![] };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let sink = Box::new(InMemorySink::new());
    let ctx = Context::new(QmonsterConfig::defaults(), source, notifier, sink);

    ctx.insights_load_tx
        .send(qmonster::app::insights_load::InsightsLoadOutcome {
            request_id: 5,
            result: Err("nope".into()),
        })
        .unwrap();

    let outcome = ctx
        .insights_load_rx
        .try_recv()
        .expect("queued outcome should be available without blocking");
    assert_eq!(outcome.request_id, 5);
    assert!(outcome.result.is_err());
    assert!(ctx.insights_load_rx.try_recv().is_err());
}
```

- [ ] **Step 4: Run the test + the full suite**

```bash
cargo test --test event_loop_integration insights_load_channel_supports_try_recv_drain
cargo test --all-targets --no-fail-fast
```

Expected: new integration test passes; full suite stays green. (Some existing tests may fail if they relied on the synchronous `refresh_insights_overlay` setting `overlay.snapshot` immediately — those are addressed in Task 10.)

- [ ] **Step 5: Commit**

```bash
git add src/app/tui_loop.rs tests/event_loop_integration.rs
git commit -m "feat(insights): drain insights_load channel + advance spinner per tick"
```

---

## Task 10: Migrate or rewrite tests that depended on synchronous refresh

Identifies and updates any existing test that asserts state set by the old synchronous `refresh_insights_overlay`. The replacement pattern is to call `overlay.set_snapshot_for(id, Ok(snapshot), label)` directly to simulate the worker thread completing.

**Files:**

- Modify: any test file that depends on synchronous insights refresh.

- [ ] **Step 1: Identify failing tests**

```bash
cargo test --lib --no-fail-fast 2>&1 | tail -80
```

Note any failure whose message includes "snapshot" or "Action Ledger" or "refresh" inside an `insights`/`tui_loop`-adjacent test. Example failure pattern: `expected snapshot to be Some, was None` after calling `refresh_insights_overlay`.

- [ ] **Step 2: For each failing test, replace synchronous refresh assertion**

The replacement always follows the same shape. If a test had:

```rust
refresh_insights_overlay(&mut overlay, &ctx);
assert!(overlay.snapshot.is_some());
```

…rewrite as:

```rust
refresh_insights_overlay(&mut overlay, &mut ctx);
let outcome = ctx
    .insights_load_rx
    .recv_timeout(std::time::Duration::from_secs(5))
    .expect("worker should send outcome");
overlay.set_snapshot_for(outcome.request_id, outcome.result, "12:00:00".into());
assert!(overlay.snapshot.is_some());
```

(`overlay.snapshot` is private; if the existing test reaches into a private field, switch to `assert!(!overlay.lines().join("\n").contains("No insights snapshot loaded."))` or expose a `pub fn has_snapshot(&self) -> bool` helper if you find more than one such test.)

- [ ] **Step 3: Run tests until green**

```bash
cargo test --lib --no-fail-fast
```

Expected: full lib suite green. If a test's assertion inspects internal state that is no longer addressable, prefer adding a small public predicate to `InsightsOverlay` over making the field `pub` — e.g.:

```rust
    pub fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }
```

- [ ] **Step 4: Commit**

```bash
git add src/ src/app
git commit -m "test(insights): migrate sync-refresh tests to async worker model"
```

(Skip the commit if no tests required updates and the suite was already green at the end of Task 9.)

---

## Task 11: Documentation

UI_MANUAL paragraph + ARCHITECTURE v1.50.0 paragraph + VALIDATION row.

**Files:**

- Modify: `docs/ai/UI_MANUAL.md`
- Modify: `docs/ai/ARCHITECTURE.md`
- Modify: `docs/ai/VALIDATION.md`

- [ ] **Step 1: UI_MANUAL §8.9**

Open `docs/ai/UI_MANUAL.md` and find §8.9 (Token Insights overlay):

```bash
grep -n "§8\.9\|Token Insights" docs/ai/UI_MANUAL.md | head -5
```

Add a new paragraph at the end of the §8.9 subsection (just before the next §8.X subsection or document break):

```markdown
v1.50.0 부터 `i` open / `r` refresh 는 worker thread 로 SQLite snapshot
을 비동기 fetch 합니다. 결과가 도착하기 전까지 본문 가운데에
`Aggregating insights ⠋` placeholder 가 회전 글리프 (10 프레임
braille, 100ms 간격) 와 함께 표시됩니다. 로딩 중에는 6-패널 본문이
숨겨지고 scroll keys (↑/↓/j/k/wheel) 가 일시적으로 비활성됩니다.
`r` 을 빠르게 두 번 누르면 첫 번째 결과는 자동으로 드롭되고 두 번째
결과만 반영됩니다 (request_id 기반 stale-drop).
```

- [ ] **Step 2: ARCHITECTURE v1.50.0 paragraph**

Open `docs/ai/ARCHITECTURE.md`. Find the existing v1.49.0 paragraph (search for `v1.49.0`). Insert a new paragraph immediately above it (so newest releases stay at top):

```markdown
**v1.50.0 (operator UX polish):** the `i` Token Insights overlay
moves from synchronous SQLite snapshot to a worker-thread fetch. Each
`i` open / `r` refresh increments `Context.next_insights_request_id`
and spawns a `std::thread` that opens `SqliteInsightsStore` read-only
and emits an `InsightsLoadOutcome` on the per-Context
`insights_load_tx` channel. The TUI main loop drains the receiver
each `event::poll` iteration and calls
`InsightsOverlay::set_snapshot_for(id, result, label)`; outcomes whose
id is not the current `pending_request_id` are dropped silently. While
the overlay is loading, `render_insights_modal` shows a centered
`Aggregating insights ⠋` line with phase rotating across 10 braille
glyphs at 100 ms cadence (the existing event-loop poll period). The
6-situation panels are suppressed during load. No provider adapter,
audit chain, or SignalSet changes; the SQLite read-only path is
unchanged. New module `src/app/insights_load.rs` isolates the spawn
logic with a `catch_unwind` guard so a worker-thread panic surfaces
as `Err` instead of leaving the overlay stuck in Loading.
```

- [ ] **Step 3: VALIDATION row**

Open `docs/ai/VALIDATION.md`. Find the existing v1.49.0 row (search for `v1.49.0`). Insert a new row above it:

```markdown
- v1.50.0 (operator UX polish): `i` Token Insights overlay moved to
  worker-thread fetch. Evidence: `loading_state_renders_centered_
spinner_and_text` (renders `Aggregating insights ⠋` in the body
  midpoint, suppresses panel labels);
  `set_snapshot_for_drops_stale_request_id` (stale outcomes ignored);
  `spawn_insights_load_emits_outcome_with_matching_request_id`
  (worker delivers via channel). `cargo fmt --all --check`,
  `cargo test --all-targets`, `cargo clippy --all-targets -- -D
warnings -A clippy::uninlined_format_args`, `git diff --check` all
  green at the release commit.
```

- [ ] **Step 4: Commit**

```bash
git add docs/ai/UI_MANUAL.md docs/ai/ARCHITECTURE.md docs/ai/VALIDATION.md
git commit -m "docs: document insights overlay worker-thread fetch (v1.50.0)"
```

---

## Task 12: Final validation gates

Pre-release validation. The release ledger sync (mission.yaml / mission-history.yaml / .mission/CURRENT_STATE.md / version surfaces / tag / push) is the operator's call after this plan completes successfully — it follows the established v1.49.0 pattern and is intentionally outside this plan's scope.

**Files:** none modified in this task; validation only.

- [ ] **Step 1: Run rustfmt check**

```bash
cargo fmt --all --check
```

Expected: clean (no diff). If the check reports formatting drift, run `cargo fmt --all` and commit the formatting fix:

```bash
git add -A
git commit -m "style: apply rustfmt"
```

- [ ] **Step 2: Run the full test suite**

```bash
cargo test --all-targets
```

Expected: all tests pass. Lib test count should be ~1265 + ~13 (the new tests in Tasks 1-7 + the bootstrap test in Task 9) = ~1278. Integration test count unchanged (we kept new tests in `mod tests` blocks alongside their modules per existing Qmonster convention).

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

Expected: clean. Common drift on this surface: `clippy::collapsible_if` if the `set_snapshot_for` early-return guard ends up nested. Fix inline if it fires.

- [ ] **Step 4: Whitespace check**

```bash
git diff --check origin/main..HEAD
```

Expected: no output (no whitespace errors).

- [ ] **Step 5: Smoke test in a TUI**

Start the dev TUI:

```bash
cargo build --release --bin qmonster
./target/release/qmonster
```

In the TUI:

1. Press `i` to open Token Insights. Verify the centered `Aggregating insights ⠋` line appears for at least a few frames before the snapshot replaces it.
2. With the overlay open, press `r` to refresh. Verify the body clears to the loading placeholder and then re-populates.
3. Press `r` rapidly twice. Verify the second refresh's result wins (no flicker of stale data after the placeholder).
4. Press `Esc` while loading. Verify the overlay closes cleanly. Press `i` again — the new load shows a fresh placeholder.
5. Press `j` / `k` / wheel during load. Verify scroll is suppressed (line_count is 0).

Document any operator-visible regressions and address before tagging. Type-check / test pass alone is necessary but not sufficient for UI changes.

- [ ] **Step 6: Confirm plan is complete**

All preceding tasks are checked, all validation gates green, smoke test passed. Ready for the operator's release sequence (version bump, mission ledger sync, tag, push, watch publication, verify).
