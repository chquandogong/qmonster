# Alerts Selected Tree Detail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Alerts as a dense row queue while rendering selected alert
details with tree branches.

**Architecture:** Update only the render string layer in
`src/ui/alerts.rs`. Keep `AlertEntry`, `AlertItem`, `AlertFlow`, sort,
hide, copy, and filter behavior unchanged.

**Tech Stack:** Rust, Ratatui, existing unit tests in `src/ui/alerts.rs`.

---

### Task 1: RED Tests For Selected Tree Detail

**Files:**
- Modify: `src/ui/alerts.rs`

- [ ] **Step 1: Update selected related render expectations**

In `renders_related_context_rail_without_collapsing_alerts`, expect tree
labels:

```rust
assert!(dump.contains("├ summary"), "{dump}");
assert!(dump.contains("├ related 2 on %1 · /compact path"), "{dump}");
assert!(dump.contains("├ rail [o] quota-pressure [ ] profile-switch"), "{dump}");
assert!(dump.contains("└ action y copy `/compact`"), "{dump}");
assert!(dump.contains("│ ├ quota-pressure: compact soon [Estimate]"), "{dump}");
assert!(dump.contains("│ └ profile-switch: compact profile [Qmonster]"), "{dump}");
```

- [ ] **Step 2: Update selected FLOW render expectations**

In `renders_context_recovery_flow_with_timeline_rail`, expect selected
FLOW tree labels:

```rust
assert!(dump.contains("├ summary"), "{dump}");
assert!(dump.contains("├ cause context pressure detected"), "{dump}");
assert!(dump.contains("├ evidence cache drift/discontinuity detected"), "{dump}");
assert!(dump.contains("├ prep snapshot before reset"), "{dump}");
assert!(dump.contains("├ included"), "{dump}");
assert!(dump.contains("└ action y copy `/compact`"), "{dump}");
```

- [ ] **Step 3: Verify RED**

Run:

```bash
cargo test --lib related_context
cargo test --lib context_recovery_flow
```

Expected: selected render tests fail because details still use flat
`label : value` lines.

### Task 2: Implement Tree Rendering For Selected Rows

**Files:**
- Modify: `src/ui/alerts.rs`
- Modify: `docs/ai/UI_MANUAL.md`

- [ ] **Step 1: Add small tree line helpers**

Add helpers near the existing related line helpers:

```rust
fn tree_detail_line(branch: &str, label: &str, body: &str) -> String {
    format!("{branch} {label:<8} {body}")
}

fn tree_child_line(branch: &str, body: &str) -> String {
    format!("│ {branch} {body}")
}
```

- [ ] **Step 2: Use tree lines for selected `AlertItem` details**

When `is_selected`, render summary/details/related/rail/action with
tree labels. Keep unselected rendering unchanged.

- [ ] **Step 3: Use tree lines for selected `AlertFlow` details**

When `is_selected`, render summary, timeline steps, action, and included
evidence with tree labels. Keep unselected FLOW rendering unchanged.

- [ ] **Step 4: Update manual**

Explain that Alerts remain rows, but selected rows use tree detail like
pane cards.

- [ ] **Step 5: Verify GREEN**

Run:

```bash
cargo test --lib related_context
cargo test --lib context_recovery_flow
cargo fmt -- --check
```

Expected: all commands pass.
