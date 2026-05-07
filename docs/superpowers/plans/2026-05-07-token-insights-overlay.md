# Token Insights Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a read-only query layer and terminal UI overlay that visualizes token optimization and policy actions based on historical data.

**Architecture:** We will implement the `InsightsSnapshot` query layer in `src/store/insights.rs`, which queries existing SQLite tables (`token_usage_samples`, `audit_events`, `cost_usage_events`). We will then expose this in a TUI overlay triggered by the `i` key, managing state via `InsightsRuntimeState` in `src/app/insights_overlay.rs` and rendering in `src/ui/insights.rs`.

**Tech Stack:** Rust, Rusqlite, Ratatui, Qmonster App architecture.

---

### Task 1: Create the Read-Only Insights Query Layer

**Files:**
- Create: `src/store/insights.rs`
- Modify: `src/store/mod.rs`
- Test: `tests/store_insights_integration.rs`

- [ ] **Step 1: Write a failing integration test for the empty state query**

```rust
// tests/store_insights_integration.rs
use qmonster::store::audit::AuditDb;
use qmonster::store::insights::{InsightsWindow, InsightsStore, InsightsSnapshot};

#[test]
fn test_insights_query_empty_db_returns_zero_state() {
    let db = AuditDb::open_in_memory().unwrap();
    let window = InsightsWindow { since_ms: 0, until_ms: 1000000 };
    let snapshot = db.query_insights(window).unwrap();
    
    assert_eq!(snapshot.situation_summaries.len(), 0);
    assert_eq!(snapshot.cache_summary.hot_count, 0);
    assert_eq!(snapshot.timeline.len(), 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test store_insights_integration test_insights_query_empty_db_returns_zero_state`
Expected: FAIL due to missing `store::insights` module and `query_insights` method.

- [ ] **Step 3: Define structs and minimal implementation**

```rust
// src/store/insights.rs
use rusqlite::Connection;

#[derive(Debug, Clone, Copy)]
pub struct InsightsWindow {
    pub since_ms: u64,
    pub until_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SituationSummary {
    pub situation: String,
    pub current_count: u64,
    pub window_count: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CacheInsightSummary {
    pub hot_count: u64,
    pub cold_count: u64,
    pub drift_count: u64,
}

#[derive(Debug, Clone)]
pub struct RecommendationTimelineItem {
    pub ts: u64,
    pub situation: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Default)]
pub struct InsightsSnapshot {
    pub window: InsightsWindow,
    pub situation_summaries: Vec<SituationSummary>,
    pub cache_summary: CacheInsightSummary,
    pub timeline: Vec<RecommendationTimelineItem>,
}

pub trait InsightsStore {
    fn query_insights(&self, window: InsightsWindow) -> Result<InsightsSnapshot, rusqlite::Error>;
}
```

Modify `src/store/mod.rs` to expose it:
```rust
// In src/store/mod.rs
pub mod insights;
```

Modify `src/store/audit.rs` (or equivalent where `AuditDb` is defined) to implement the trait:
```rust
// Add to src/store/audit.rs
use crate::store::insights::{InsightsStore, InsightsSnapshot, InsightsWindow};

impl InsightsStore for AuditDb {
    fn query_insights(&self, window: InsightsWindow) -> Result<InsightsSnapshot, rusqlite::Error> {
        // Return an empty snapshot for now
        Ok(InsightsSnapshot {
            window,
            situation_summaries: vec![],
            cache_summary: Default::default(),
            timeline: vec![],
        })
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test store_insights_integration test_insights_query_empty_db_returns_zero_state`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add tests/store_insights_integration.rs src/store/insights.rs src/store/mod.rs src/store/audit.rs
git commit -m "feat(store): add empty InsightsStore and InsightsSnapshot structs"
```

### Task 2: Implement TUI Overlay State and Rendering

**Files:**
- Create: `src/app/insights_overlay.rs`
- Create: `src/ui/insights.rs`
- Modify: `src/app/mod.rs`, `src/ui/mod.rs`, `src/app/dashboard_state.rs`
- Test: `tests/insights_overlay_test.rs`

- [ ] **Step 1: Write a test for InsightsRuntimeState caching**

```rust
// tests/insights_overlay_test.rs
use qmonster::app::insights_overlay::InsightsRuntimeState;
use qmonster::store::insights::{InsightsSnapshot, InsightsWindow};

#[test]
fn test_insights_snapshot_refresh_not_in_ui_renderer() {
    let mut state = InsightsRuntimeState::new();
    assert!(state.snapshot().is_none());
    
    let snap = InsightsSnapshot {
        window: InsightsWindow { since_ms: 0, until_ms: 100 },
        situation_summaries: vec![],
        cache_summary: Default::default(),
        timeline: vec![],
    };
    state.update_snapshot(snap.clone());
    
    assert!(state.snapshot().is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test insights_overlay_test test_insights_snapshot_refresh_not_in_ui_renderer`
Expected: FAIL due to missing `insights_overlay` module.

- [ ] **Step 3: Write minimal implementation**

```rust
// src/app/insights_overlay.rs
use crate::store::insights::InsightsSnapshot;

pub struct InsightsRuntimeState {
    snapshot: Option<InsightsSnapshot>,
    last_refresh_ms: u64,
}

impl InsightsRuntimeState {
    pub fn new() -> Self {
        Self {
            snapshot: None,
            last_refresh_ms: 0,
        }
    }

    pub fn snapshot(&self) -> Option<&InsightsSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn update_snapshot(&mut self, snapshot: InsightsSnapshot) {
        self.snapshot = Some(snapshot);
        // Assuming we could set last_refresh_ms from a time source, omitted for brevity
    }
}
```

Modify `src/app/mod.rs`:
```rust
pub mod insights_overlay;
```

Modify `src/ui/insights.rs`:
```rust
// src/ui/insights.rs
use tui::widgets::Block; // example ratatui imports
use crate::store::insights::InsightsSnapshot;

pub fn draw_insights_overlay<B: tui::backend::Backend>(
    f: &mut tui::Frame<B>,
    area: tui::layout::Rect,
    snapshot: Option<&InsightsSnapshot>
) {
    let block = Block::default().title("Token Insights");
    f.render_widget(block, area);
}
```

Modify `src/ui/mod.rs`:
```rust
pub mod insights;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test insights_overlay_test test_insights_snapshot_refresh_not_in_ui_renderer`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add tests/insights_overlay_test.rs src/app/insights_overlay.rs src/app/mod.rs src/ui/insights.rs src/ui/mod.rs
git commit -m "feat(ui): add InsightsRuntimeState and empty insights overlay renderer"
```

### Task 3: Wire Overlay Key Dispatch

**Files:**
- Modify: `src/app/event_loop.rs` or `src/app/keymap.rs`
- Modify: `src/app/dashboard_state.rs`
- Test: `tests/insights_overlay_key_toggles.rs`

- [ ] **Step 1: Write a test for 'i' key toggling**

```rust
// tests/insights_overlay_key_toggles.rs
use qmonster::app::dashboard_state::DashboardState;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

#[test]
fn test_insights_overlay_key_toggles() {
    let mut state = DashboardState::new();
    assert!(!state.show_insights_overlay);
    
    // Process 'i' key
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::empty()));
    assert!(state.show_insights_overlay);
    
    // Process 'i' key again to close
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::empty()));
    assert!(!state.show_insights_overlay);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test insights_overlay_key_toggles`
Expected: FAIL due to missing `show_insights_overlay` property and `handle_key` not parsing `i`.

- [ ] **Step 3: Write minimal implementation**

Update `DashboardState` in `src/app/dashboard_state.rs` (or equivalent event handling location):
```rust
// Add `pub show_insights_overlay: bool` to the DashboardState struct
// In handle_key match block:
// match key.code {
//     KeyCode::Char('i') => self.show_insights_overlay = !self.show_insights_overlay,
//     // ...
// }
```
*(Exact location depends on the project's event loop structure, adjust according to existing key dispatch patterns).*

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test insights_overlay_key_toggles`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/app/dashboard_state.rs tests/insights_overlay_key_toggles.rs
git commit -m "feat(app): toggle insights overlay via 'i' key"
```
