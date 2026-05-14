# Alerts Related Visual Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce visual noise in the Alerts panel by making weak `related`
context quiet until the row is selected.

**Architecture:** Keep `RelatedAlertContext` projection unchanged and
adjust only the `src/ui/alerts.rs` renderer. Existing tests continue to
cover grouping, copy locality, sorting, and flow separation.

**Tech Stack:** Rust, Ratatui, existing `cargo test --lib` unit tests.

---

### Task 1: Lock the Desired Related Rendering

**Files:**
- Modify: `src/ui/alerts.rs`

- [ ] **Step 1: Update the selected related render test**

Change `renders_related_context_rail_without_collapsing_alerts` to expect
the compact summary text:

```rust
assert!(dump.contains("related : 2 on %1 · /compact path"), "{dump}");
```

- [ ] **Step 2: Update the unselected related render test**

Rename `unselected_related_context_omits_evidence_but_keeps_rail` to
`unselected_related_context_shows_summary_only`, then assert:

```rust
assert!(dump.contains("related : 2 on %1 · /compact path"), "{dump}");
assert!(!dump.contains("rail :"), "{dump}");
assert!(!dump.contains("included: quota-pressure"), "{dump}");
```

- [ ] **Step 3: Verify RED**

Run:

```bash
cargo test --lib related_context
```

Expected: the two render tests fail because the renderer still shows
`2 alerts on %1 · same /compact path` and still renders `rail` for
unselected related rows.

### Task 2: Implement Minimal Renderer Polish

**Files:**
- Modify: `src/ui/alerts.rs`
- Modify: `docs/ai/UI_MANUAL.md`

- [ ] **Step 1: Compact the related summary text**

In `related_summary_line`, strip a leading `same ` from the connector
label and render:

```rust
&format!("{} on {} · {}", related.total, related.pane_id, connector)
```

- [ ] **Step 2: Make rail selected-only**

In `alert_item_lines`, always render `related_summary_line(related)`,
but only render `related_rail_line(related)` and `included` evidence
inside `if is_selected`.

- [ ] **Step 3: Update UI manual wording**

Document that unselected related alerts show only the `related` summary,
while selected related alerts expand `rail` and `included` evidence.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test --lib related_context
cargo test --lib context_recovery_flow
cargo fmt -- --check
```

Expected: all commands pass.
