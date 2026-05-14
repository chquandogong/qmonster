# Alerts Stable Selection Detail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Alerts as a dense row queue while keeping selected and
non-selected base rows structurally identical. Selection appends only extra
tree detail for related rail, included evidence, and copy action.

**Architecture:** Update only the render string layer in
`src/ui/alerts.rs`. Keep `AlertEntry`, `AlertItem`, `AlertFlow`, sort,
hide, copy, and filter behavior unchanged.

**Tech Stack:** Rust, Ratatui, existing unit tests in `src/ui/alerts.rs`.

---

### Task 1: RED Tests For Stable Selection Detail

**Files:**
- Modify: `src/ui/alerts.rs`

- [ ] **Step 1: Update selected related render expectations**

In `renders_related_context_rail_without_collapsing_alerts`, expect the base
lines to remain flat and the extra selected detail to be appended as a tree:

```rust
assert!(dump.contains("summary : [Estimate] quota pressure can recover after compact"), "{dump}");
assert!(dump.contains("run : `/compact`"), "{dump}");
assert!(dump.contains("related : 2 on %1 · /compact path"), "{dump}");
assert!(dump.contains("├ rail [o] quota-pressure [ ] profile-switch"), "{dump}");
assert!(dump.contains("└ action y copy `/compact`"), "{dump}");
assert!(dump.contains("│ ├ quota-pressure: compact soon [Estimate]"), "{dump}");
assert!(dump.contains("│ └ profile-switch: compact profile [Qmonster]"), "{dump}");
```

- [ ] **Step 2: Update selected FLOW render expectations**

In `renders_context_recovery_flow_with_timeline_rail`, expect the base FLOW
summary/timeline to remain flat and only included/action to be appended as a
tree:

```rust
assert!(dump.contains("summary : context pressure led to cache drift"), "{dump}");
assert!(dump.contains("o context pressure detected"), "{dump}");
assert!(dump.contains("| cache drift/discontinuity detected"), "{dump}");
assert!(dump.contains("o snapshot before reset"), "{dump}");
assert!(dump.contains("| then run /compact"), "{dump}");
assert!(dump.contains("├ included"), "{dump}");
assert!(dump.contains("└ action y copy `/compact`"), "{dump}");
```

- [ ] **Step 3: Verify RED**

Run:

```bash
cargo test --lib related_context
cargo test --lib context_recovery_flow
```

Expected: selected render tests fail if selection still rewrites summary,
related, or FLOW timeline lines into tree rows.

### Task 2: Implement Additive Tree Rendering For Selected Rows

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

- [ ] **Step 2: Keep `AlertItem` base lines stable**

Render summary/details/related before the selection branch. When
`is_selected`, append only related `rail`, nested `included` evidence, and
optional `action` with tree labels.

- [ ] **Step 3: Keep `AlertFlow` base lines stable**

Render summary and timeline steps before the selection branch. When
`is_selected`, append only `included` evidence and optional `action` with
tree labels.

- [ ] **Step 4: Update manual**

Explain that Alerts remain rows, selected rows keep the same base lines, and
tree glyphs are used only for appended detail.

- [ ] **Step 5: Verify GREEN**

Run:

```bash
cargo test --lib related_context
cargo test --lib context_recovery_flow
cargo test --lib
cargo fmt -- --check
```

Expected: all commands pass.
