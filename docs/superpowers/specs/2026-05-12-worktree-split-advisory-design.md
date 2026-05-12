# Worktree Split Advisory — Design

_Author: Claude (post-v2.2.0 stabilization). Status: approved by operator on 2026-05-12._

## 1. Purpose

When two or more Main/Review panes are mutating the same `current_path +
git_branch` (or touching the same absolute file), Qmonster currently
emits a `ConcurrentMutatingWork` or `ConcurrentFileEdit` advisory whose
sole `suggested_command` is a tmux hint to coordinate via a research
pane. The advisory does not surface the second natural resolution path
— splitting one pane into a `git worktree` so the two operators (human
or agent) can work in parallel without divergent edits.

This design adds the worktree split as a **second hint line** inside
the existing `suggested_command` string. No new advisory variants, no
new gates, no new signals.

The proximate driver is the 2026-05-12 incident where a second
Claude / Codex agent edited the same Qmonster working tree
concurrently with the primary session, surfacing the gap.

## 2. Non-goals

- A new advisory variant `MultiAgentBranchContention` driven by commit
  cadence. Deferred — needs new signals and operator data.
- An `IdentityConfidence::Conflict` recovery hint. Unrelated:
  attribution drift is rarely solved by a worktree split.
- A new gate `worktree_split_suggestions`. The existing per-advisory
  gates (`cross_pane_file_findings`, defaults) already control whether
  the advisory fires at all. Toggling the worktree hint independently
  is not worth a new opt-in.
- Changing `CrossPaneFinding.suggested_command` to a `Vec<String>` or
  adding an `alternative_command` field. Schema unchanged; the second
  hint is appended with `\n` inside the existing single string.
- Modifying `CrossWindowConcurrentWork`. Its current
  `consolidate windows: tmux move-pane` suggestion is the opposite of
  "split via worktree" and remains intact.
- UI polish: parsing the two-line hint into separate clipboard targets
  in the pending-actions modal. Deferred to a follow-up.
- Actually executing `git worktree add`. Qmonster emits advisories;
  command execution stays operator-driven.

## 3. Scope summary

| Advisory                    | Today's hint                                       | After this change                                                                                                              |
| --------------------------- | -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `ConcurrentMutatingWork`    | `# coordinate via research pane: tmux select-pane` | Two-line hint: legacy line **plus** `# or split off <branch> into a new worktree: git worktree add -b <new-branch> <new-path>` |
| `ConcurrentFileEdit`        | Same as above                                      | Same two-line hint, with `<branch>` interpolated from the anchor pane's `signals.git_branch`                                   |
| `CrossWindowConcurrentWork` | `# consolidate windows: tmux move-pane`            | **Unchanged** (intentionally — semantically opposite to worktree split)                                                        |

## 4. Architecture

Single private helper in `src/policy/rules/concurrent.rs`:

```rust
fn build_concurrent_suggested_command(branch: &str) -> String {
    let branch_label = if branch.is_empty() { "<branch>" } else { branch };
    format!(
        "# coordinate via research pane:                tmux select-pane -t <research_pane_id>\n\
         # or split off {branch_label} into a new worktree:   git worktree add -b <new-branch> <new-path>"
    )
}
```

The helper is invoked from exactly two call sites:

1. **`eval_concurrent`** (`concurrent.rs:22`), `ConcurrentMutatingWork`
   emission path (currently `concurrent.rs:95-109`). Pass
   `&key.branch` — already in scope as the `ConcurrentKey` field that
   gated detector firing in the first place.

2. **`eval_concurrent_files`** (`concurrent.rs:146`), `ConcurrentFileEdit`
   emission path (currently `concurrent.rs:200-213`). The detector
   does not currently group by branch, so we read the anchor pane's
   `signals.git_branch.as_ref().map(|m| m.value.as_str()).unwrap_or("")`
   and pass that. An empty string falls back to the `<branch>`
   placeholder via the helper.

The `CrossWindowConcurrentWork` branch inside `eval_concurrent`
(`concurrent.rs:74-93`) is **not** modified.

## 5. Data flow

```
PaneView.signals.git_branch ──┐
                              ├──► ConcurrentKey.branch ──┐
PaneView.current_path ────────┘                            │
                                                           ├──► build_concurrent_suggested_command(&branch)
                                                           │              │
PaneView.signals.active_files ──► by_file groups            │              ▼
                                  ──► anchor.git_branch ───┘     CrossPaneFinding.suggested_command
```

No new signals, no new gates, no schema change.

## 6. Error handling and edge cases

| Case                                                                     | Behaviour                                                                                                                                                                                        |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ConcurrentMutatingWork` fires with empty `key.branch`                   | Unreachable — `concurrent_key` returns `None` for empty branch and the pane never qualifies.                                                                                                     |
| `ConcurrentFileEdit` anchor pane has `git_branch = None`                 | Helper receives `""`, emits `# or split off <branch> into a new worktree: …` with the literal placeholder. Operator fills in.                                                                    |
| Both detectors fire on the same poll                                     | Two findings appear in the alert queue, each carrying the worktree hint independently. Dedup is out of scope for this spec.                                                                      |
| Anchor and other pane disagree on `git_branch` (ConcurrentFileEdit only) | Anchor's branch wins. Detector already uses lexicographic anchor; consistent choice.                                                                                                             |
| Branch name contains shell-meta characters                               | Embedded verbatim in the hint text. The hint is a `#`-prefixed comment with placeholders; operator is expected to edit before running. Same policy as existing `<research_pane_id>` placeholder. |
| Operator copies the suggested command via `y`-press                      | Whole multi-line string copies. Single-line clipboard targeting is a follow-up polish, out of scope.                                                                                             |

## 7. Tests

Four units to add in `src/policy/rules/concurrent.rs`'s
`#[cfg(test)] mod tests`. Existing tests that assert on
`suggested_command` text are updated as part of implementation; a grep
during implementation will surface them.

1. **`concurrent_mutating_finding_includes_worktree_split_hint`**
   Two Main panes on same `current_path` + branch `main`. Assertions:
   - `suggested_command` contains `tmux select-pane` (legacy hint kept).
   - Contains `git worktree add -b`.
   - Contains `split off main into a new worktree` (branch interpolated).

2. **`concurrent_file_edit_finding_includes_worktree_split_hint`**
   Two Main panes touching the same absolute file, both reporting
   `git_branch = "feature/abc"`, `cross_pane_file_findings` gate on.
   Same three assertions with `feature/abc` interpolated.

3. **`concurrent_file_edit_with_missing_branch_falls_back_to_placeholder`**
   Two panes touching the same file but anchor has
   `git_branch = None`. Assertion: hint contains the literal
   `split off <branch> into a new worktree`, and `git worktree add -b`
   is still present.

4. **`cross_window_concurrent_work_keeps_consolidate_suggestion`** —
   regression guard. Two panes on same path+branch but in different
   tmux windows, `cross_window_findings` gate on. Assertions:
   - Contains `consolidate windows` (legacy hint preserved).
   - Does **not** contain `git worktree add`.

Existing tests outside `concurrent.rs` (notably
`tests/event_loop_integration.rs`) that compare against the old
suggested-command string will need updating; the implementation step
runs a final `grep -r 'tmux select-pane' --include='*.rs'` to surface
any missed sites.

## 8. Validation gate

Run after implementation lands, before marking the work complete:

- `cargo build`
- `cargo fmt --all --check`
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --all-targets`
- `npx mission-spec eval` — 23 / 23 must still pass.

## 9. Out-of-scope follow-ups

These appear in this spec only as named pointers so a future reader can
trace the trail of decisions; they are **not** implemented here:

- **F-1.** Single-line clipboard targeting in the pending-actions modal:
  parse the two-line `suggested_command` into separately-copyable rows.
- **F-2.** `MultiAgentBranchContention` advisory using recent-commit
  cadence per pane to fire even when same-path / same-branch
  concurrent mutating activity hasn't started yet.
- **F-3.** Optional `[security] worktree_split_suggestions` gate to
  let operators with non-worktree workflows hide the second hint line
  while keeping the parent advisory.
