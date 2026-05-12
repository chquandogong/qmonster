# Worktree Split Advisory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `git worktree add` hint as a second line inside the existing `suggested_command` of `ConcurrentMutatingWork` and `ConcurrentFileEdit` advisories, with the current branch interpolated and a `<branch>` placeholder fallback when the signal is missing.

**Architecture:** Single private helper `build_concurrent_suggested_command(branch: &str) -> String` lives in `src/policy/rules/concurrent.rs` and is invoked from the two existing detector emission paths. `CrossWindowConcurrentWork` is intentionally left alone. No schema change to `CrossPaneFinding`, no new gates, no new signals.

**Tech Stack:** Rust 2024 edition, `policy/rules` module, `#[cfg(test)] mod tests` unit-test style already present in the file.

**Spec:** `docs/superpowers/specs/2026-05-12-worktree-split-advisory-design.md`

---

## File map

- Modify: `src/policy/rules/concurrent.rs`
  - Add private helper `build_concurrent_suggested_command` after `resolve_against`, before `#[cfg(test)] mod tests`.
  - Replace the literal `suggested_command` strings at the two emission sites (`eval_concurrent` ConcurrentMutatingWork branch ≈ line 105-107; `eval_concurrent_files` ≈ line 209-211).
  - Add 4 new unit tests inside the existing `mod tests` block.
- Modify: `src/ui/alerts.rs`
  - One test fixture at line 1671 hard-codes `"# coordinate via research pane"` as a literal `suggested_command` for assertion purposes. Assertions at lines 1685 and 1691 also key on that string. None of these need behavioral change — the alerts UI just needs to keep handling the legacy single-line form as a renderable string. Confirm-only: tests must continue to pass without modification because the test fixture supplies its own string rather than calling the detector.

## Task 1: Add helper + wire ConcurrentMutatingWork

**Files:**

- Modify: `src/policy/rules/concurrent.rs` (add helper, update one emission site, add one test)

- [ ] **Step 1: Add the failing test**

Insert at the end of the `#[cfg(test)] mod tests` block in `src/policy/rules/concurrent.rs` (after the last existing test, before the closing `}` of `mod tests`):

```rust
#[test]
fn concurrent_mutating_finding_includes_worktree_split_hint() {
    let id_a = mk_id(Role::Main, "%1");
    let id_b = mk_id(Role::Main, "%2");
    let s = busy_branch_signals("main");
    let views = vec![
        PaneView {
            identity: &id_a,
            signals: &s,
            current_path: "/repo",
            window_label: "",
        },
        PaneView {
            identity: &id_b,
            signals: &s,
            current_path: "/repo",
            window_label: "",
        },
    ];
    let findings = eval_concurrent(&views, &PolicyGates::default());
    assert_eq!(findings.len(), 1);
    let suggestion = findings[0]
        .suggested_command
        .as_ref()
        .expect("hint present");
    assert!(
        suggestion.contains("tmux select-pane"),
        "legacy research-pane hint must remain: {suggestion}"
    );
    assert!(
        suggestion.contains("git worktree add -b"),
        "worktree split hint must be added: {suggestion}"
    );
    assert!(
        suggestion.contains("split off main into a new worktree"),
        "branch must be interpolated: {suggestion}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p qmonster --lib concurrent_mutating_finding_includes_worktree_split_hint -- --nocapture`
Expected: FAIL on `assert!(suggestion.contains("git worktree add -b"))` because the current emission only writes the legacy tmux line.

- [ ] **Step 3: Add the helper fn**

Insert in `src/policy/rules/concurrent.rs` immediately after the `resolve_against` fn (currently ends near line 229) and before the `#[cfg(test)] mod tests` block opens:

```rust
/// Build the two-line suggested-command hint shared by
/// `ConcurrentMutatingWork` and `ConcurrentFileEdit`. The first line
/// keeps the historical "coordinate via research pane" tmux nudge; the
/// second line offers the alternative resolution path — split one pane
/// into a new git worktree. The current branch is interpolated when
/// known; an empty branch falls back to the `<branch>` placeholder so
/// the hint stays grammatical when `ConcurrentFileEdit` fires without
/// a `git_branch` signal.
fn build_concurrent_suggested_command(branch: &str) -> String {
    let branch_label = if branch.is_empty() { "<branch>" } else { branch };
    format!(
        "# coordinate via research pane:                tmux select-pane -t <research_pane_id>\n\
         # or split off {branch_label} into a new worktree:   git worktree add -b <new-branch> <new-path>"
    )
}
```

Note on the `\` continuation: Rust's string-literal `\<newline>` consumes the newline AND the leading whitespace on the next physical line, so the second line in the produced string starts at column 0 with `# or split off ...`. Verified pattern; matches the rest of the file's existing multi-line literals.

- [ ] **Step 4: Wire helper into the ConcurrentMutatingWork emission**

In `src/policy/rules/concurrent.rs`, locate the `eval_concurrent` block emitting `CrossPaneKind::ConcurrentMutatingWork` (currently lines 94-110). Replace the `suggested_command` field:

```rust
// BEFORE
suggested_command: Some(
    "# coordinate via research pane: tmux select-pane -t <research_pane_id>".into(),
),

// AFTER
suggested_command: Some(build_concurrent_suggested_command(&key.branch)),
```

Leave the `CrossWindowConcurrentWork` branch (lines 74-93) untouched.

- [ ] **Step 5: Run the new test, verify pass**

Run: `cargo test -p qmonster --lib concurrent_mutating_finding_includes_worktree_split_hint`
Expected: PASS (1 passed; 0 failed)

- [ ] **Step 6: Run the full concurrent module tests for regressions**

Run: `cargo test -p qmonster --lib policy::rules::concurrent::tests`
Expected: All existing tests in the module still pass. If any pre-existing test asserts on the literal `# coordinate via research pane: tmux select-pane -t <research_pane_id>` string (a single-line expectation), it will fail here — note the failing test names; they get fixed in Task 5.

- [ ] **Step 7: Commit**

```bash
git add src/policy/rules/concurrent.rs
git commit -m "feat(policy): add worktree split hint to ConcurrentMutatingWork advisory

Introduces build_concurrent_suggested_command helper that appends a
'git worktree add -b' line to the existing tmux coordination hint, with
the current branch interpolated.

Spec: docs/superpowers/specs/2026-05-12-worktree-split-advisory-design.md"
```

---

## Task 2: Wire ConcurrentFileEdit emission to the same helper

**Files:**

- Modify: `src/policy/rules/concurrent.rs` (update one emission site + add one test)

- [ ] **Step 1: Add the failing test**

Insert after the test from Task 1 inside `mod tests`:

```rust
#[test]
fn concurrent_file_edit_finding_includes_worktree_split_hint() {
    let id_a = mk_id(Role::Main, "%1");
    let id_b = mk_id(Role::Main, "%2");
    let signals = SignalSet {
        active_files: vec!["src/lib.rs".into()],
        git_branch: Some(MetricValue::new(
            "feature/abc".to_string(),
            SourceKind::ProviderOfficial,
        )),
        ..busy_signals()
    };
    let views = vec![
        PaneView {
            identity: &id_a,
            signals: &signals,
            current_path: "/repo",
            window_label: "",
        },
        PaneView {
            identity: &id_b,
            signals: &signals,
            current_path: "/repo",
            window_label: "",
        },
    ];
    let gates = PolicyGates {
        cross_pane_file_findings: true,
        ..PolicyGates::default()
    };
    let findings = eval_concurrent_files(&views, &gates);
    assert!(!findings.is_empty(), "file-edit finding must fire");
    assert_eq!(findings[0].kind, CrossPaneKind::ConcurrentFileEdit);
    let suggestion = findings[0]
        .suggested_command
        .as_ref()
        .expect("hint present");
    assert!(suggestion.contains("tmux select-pane"));
    assert!(suggestion.contains("git worktree add -b"));
    assert!(
        suggestion.contains("split off feature/abc into a new worktree"),
        "branch must be interpolated: {suggestion}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p qmonster --lib concurrent_file_edit_finding_includes_worktree_split_hint`
Expected: FAIL on the `git worktree add -b` assertion because `eval_concurrent_files` still emits only the legacy single-line hint.

- [ ] **Step 3: Wire helper into ConcurrentFileEdit emission**

In `src/policy/rules/concurrent.rs`, locate the `eval_concurrent_files` block emitting `CrossPaneKind::ConcurrentFileEdit` (currently lines 200-213). Before constructing the `CrossPaneFinding`, derive the anchor branch:

```rust
// Insert just before `out.push(CrossPaneFinding { … })`:
let anchor_branch = group[0]
    .signals
    .git_branch
    .as_ref()
    .map(|m| m.value.as_str())
    .unwrap_or("");
```

Then replace the `suggested_command` field:

```rust
// BEFORE
suggested_command: Some(
    "# coordinate via research pane: tmux select-pane -t <research_pane_id>".into(),
),

// AFTER
suggested_command: Some(build_concurrent_suggested_command(anchor_branch)),
```

- [ ] **Step 4: Run test, verify pass**

Run: `cargo test -p qmonster --lib concurrent_file_edit_finding_includes_worktree_split_hint`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/policy/rules/concurrent.rs
git commit -m "feat(policy): add worktree split hint to ConcurrentFileEdit advisory

Reuses build_concurrent_suggested_command; reads the anchor pane's
git_branch signal for interpolation, falling back to the <branch>
placeholder when missing."
```

---

## Task 3: Pin the missing-branch fallback

**Files:**

- Modify: `src/policy/rules/concurrent.rs` (add one test only — no code change expected)

- [ ] **Step 1: Add the test**

Insert after the test from Task 2:

```rust
#[test]
fn concurrent_file_edit_with_missing_branch_falls_back_to_placeholder() {
    let id_a = mk_id(Role::Main, "%1");
    let id_b = mk_id(Role::Main, "%2");
    let signals = SignalSet {
        active_files: vec!["src/lib.rs".into()],
        // NO git_branch — the fallback path.
        ..busy_signals()
    };
    let views = vec![
        PaneView {
            identity: &id_a,
            signals: &signals,
            current_path: "/repo",
            window_label: "",
        },
        PaneView {
            identity: &id_b,
            signals: &signals,
            current_path: "/repo",
            window_label: "",
        },
    ];
    let gates = PolicyGates {
        cross_pane_file_findings: true,
        ..PolicyGates::default()
    };
    let findings = eval_concurrent_files(&views, &gates);
    assert!(!findings.is_empty(), "file-edit finding must still fire without git_branch");
    let suggestion = findings[0]
        .suggested_command
        .as_ref()
        .expect("hint present");
    assert!(
        suggestion.contains("split off <branch> into a new worktree"),
        "missing branch must fall back to the <branch> placeholder: {suggestion}"
    );
    assert!(
        suggestion.contains("git worktree add -b"),
        "worktree command must still appear: {suggestion}"
    );
}
```

- [ ] **Step 2: Run test, expect PASS**

Run: `cargo test -p qmonster --lib concurrent_file_edit_with_missing_branch_falls_back_to_placeholder`
Expected: PASS — the helper from Task 1 already implements the fallback (`if branch.is_empty() { "<branch>" } else { branch }`).

- [ ] **Step 3: If unexpectedly fails, debug helper**

If `assert!(!findings.is_empty(), ...)` trips, the file-edit detector itself is not firing without git_branch. That would be a Task 2 wiring bug — re-inspect Task 2 step 3. If the suggestion assertions trip, the helper's empty-branch branch is wrong — re-check the `if branch.is_empty()` line in Task 1 step 3.

- [ ] **Step 4: Commit**

```bash
git add src/policy/rules/concurrent.rs
git commit -m "test(policy): pin <branch> placeholder fallback for ConcurrentFileEdit hint

Captures the invariant that ConcurrentFileEdit fires and surfaces a
grammatical worktree hint even when the anchor pane has no git_branch
signal."
```

---

## Task 4: Regression guard for CrossWindowConcurrentWork

**Files:**

- Modify: `src/policy/rules/concurrent.rs` (add one test only — no code change expected)

- [ ] **Step 1: Add the regression-guard test**

Insert after the test from Task 3:

```rust
#[test]
fn cross_window_concurrent_work_keeps_consolidate_suggestion() {
    let id_a = mk_id(Role::Main, "%1");
    let id_b = mk_id(Role::Main, "%2");
    let s = busy_branch_signals("main");
    let views = vec![
        PaneView {
            identity: &id_a,
            signals: &s,
            current_path: "/repo",
            window_label: "main-win",
        },
        PaneView {
            identity: &id_b,
            signals: &s,
            current_path: "/repo",
            window_label: "scratch-win",
        },
    ];
    let gates = PolicyGates {
        cross_window_findings: true,
        ..PolicyGates::default()
    };
    let findings = eval_concurrent(&views, &gates);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, CrossPaneKind::CrossWindowConcurrentWork);
    let suggestion = findings[0]
        .suggested_command
        .as_ref()
        .expect("hint present");
    assert!(
        suggestion.contains("consolidate windows"),
        "legacy consolidate-windows hint must remain on cross-window findings: {suggestion}"
    );
    assert!(
        !suggestion.contains("git worktree add"),
        "worktree split must NOT bleed into cross-window findings: {suggestion}"
    );
}
```

- [ ] **Step 2: Run test, expect PASS**

Run: `cargo test -p qmonster --lib cross_window_concurrent_work_keeps_consolidate_suggestion`
Expected: PASS — Task 1 step 4 explicitly leaves the cross-window branch untouched.

- [ ] **Step 3: Commit**

```bash
git add src/policy/rules/concurrent.rs
git commit -m "test(policy): guard CrossWindowConcurrentWork against worktree-hint bleed

Pins the contract that the worktree split hint is only added to the
same-window/same-file detectors; cross-window panes keep the original
'consolidate windows' guidance, which is semantically opposite."
```

---

## Task 5: Sweep legacy `suggested_command` assertions across the workspace

**Files:**

- Inspect (and possibly modify): every `.rs` file in the workspace that hard-codes the legacy single-line hint.

The pre-implementation grep already mapped four hits:

1. `src/policy/rules/concurrent.rs:100` — `reason` text, not `suggested_command`. NO change.
2. `src/policy/rules/concurrent.rs:106` — replaced in Task 1.
3. `src/policy/rules/concurrent.rs:210` — replaced in Task 2.
4. `src/ui/alerts.rs:1671, 1685, 1691` — a UI-test fixture that constructs its own `suggested_command` literal (not produced by the detector) and asserts the alert renderer handles it. The fixture remains valid; the renderer must continue to handle single-line hints because the production code can still emit single-line hints (e.g. for prompt-send proposals). NO change expected. Verify by running the alerts tests.

- [ ] **Step 1: Re-run the grep to catch anything added after spec write**

Run: `grep -rn "tmux select-pane\|coordinate via research pane" --include="*.rs" /home/chquan/Qmonster/`
Expected: same four hits as the pre-implementation map. If new hits appear, evaluate each — assertions on detector output need updating, fixture-only literals do not.

- [ ] **Step 2: Run the alerts-module tests**

Run: `cargo test -p qmonster --lib ui::alerts`
Expected: PASS unchanged. If a test fails because of a string mismatch, that test was asserting on detector-produced text. Update it to use `.contains(...)` instead of exact-string equality. Show the diff before committing.

- [ ] **Step 3: Run all integration tests**

Run: `cargo test -p qmonster --test '*'`
Expected: PASS. The `tests/event_loop_integration.rs` and other integration tests that wire through the detector now see two-line suggestions. If any of them assert on exact-string equality for a `suggested_command`, change the assertion to substring containment. Cite the file and line before editing.

- [ ] **Step 4: Commit (only if changes were needed)**

```bash
git add <files>
git commit -m "test: relax suggested_command assertions to substring containment

Two-line ConcurrentMutating/ConcurrentFileEdit hints now embed the
legacy tmux line plus a worktree alternative; downstream tests that
checked exact equality switch to contains() to remain stable across
future hint extensions."
```

If no files needed updating, skip the commit and document the finding in the task tracker.

---

## Task 6: Final validation gate

**Files:**

- None modified; verification only.

- [ ] **Step 1: Build**

Run: `cargo build`
Expected: clean.

- [ ] **Step 2: Format check**

Run: `cargo fmt --all --check`
Expected: clean. If it fails, run `cargo fmt --all` and amend the most recent commit (`git add -u && git commit --amend --no-edit`).

- [ ] **Step 3: Clippy**

Run: `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
Expected: clean.

- [ ] **Step 4: Full test suite**

Run: `cargo test --all-targets`
Expected: all tests pass. Test count should rise by exactly 4 (the four new tests from Tasks 1-4).

- [ ] **Step 5: Mission gate**

Run: `npx mission-spec eval`
Expected: `23/23 criteria passed — mission complete!`

- [ ] **Step 6: No-op commit only if validation surfaced fixes**

If any of steps 1-5 required code/test edits beyond Tasks 1-5, commit them as `chore: validation-gate fixes`. Otherwise nothing to commit.

---

## Out-of-scope follow-ups (named in spec §9, not implemented here)

- **F-1.** Single-line clipboard targeting in the pending-actions modal.
- **F-2.** `MultiAgentBranchContention` advisory using recent-commit cadence.
- **F-3.** Optional `[security] worktree_split_suggestions` gate.
