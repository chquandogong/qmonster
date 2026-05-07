# Insights Overlay Loading Spinner — Design

**Status:** approved (operator confirmed 2026-05-07)
**Target version:** v1.50.0 (operator UX polish, follows v1.49.0)
**Author:** Claude Code (Qmonster main pane)
**Brainstorm date:** 2026-05-07

## Goal

When the operator presses `i` to open the Token Insights overlay or `r`
to refresh it, the SQLite snapshot path takes long enough (hundreds of
ms) that the TUI feels stalled. Render a center-aligned full-body
placeholder with a rotating spinner glyph until the snapshot is ready,
so the operator visually confirms the keypress was received and the
work is in progress.

## Scope

**In scope:**

- `i` Token Insights overlay open path
- `r` refresh path inside the same overlay
- Worker-thread fetch + main-loop channel polling for the insights
  snapshot
- Center-aligned loading placeholder rendered when no snapshot is
  available yet for the current request
- Spinner glyph rotation tied to the existing 100 ms `event::poll`
  cadence in `tui_loop`

**Out of scope (explicitly):**

- `s` snapshot, `n` Anomaly Events, `m` Metrics, `a` Pending Actions, or
  any other key path. The operator stated `s` already shows results in
  the alerts panel and is not part of this slice.
- Archive write, snapshot write, recommendation lifecycle write paths.
- Any cross-cutting "loading state" abstraction. This slice owns its
  own state on `InsightsOverlay` and a single dispatcher in
  `tui_loop`/bootstrap. Generalization is a future concern, not this
  release.
- New cancellation primitives. We use stale-request-id drop instead of
  thread cancellation (rusqlite blocking calls are not cancellable
  cheaply, and snapshot work is bounded).

## Architecture

### Worker thread per request

For each `i` open and each `r` refresh, spawn a fresh
`std::thread::spawn` that:

1. Opens the audit DB at `Context.insights_db_path()` via
   `SqliteInsightsStore::open_read_only`.
2. Calls `snapshot_with_ignored_ttl(window, ignored_ttl_secs)` using
   the same `InsightsWindow` and TTL the synchronous path uses today.
3. Sends `(request_id, Result<InsightsSnapshot, String>)` on a
   pre-existing `mpsc::Sender<InsightsLoadOutcome>`.
4. Returns. Thread exits naturally; no join from main.

If the DB cannot be opened, the worker sends a stringified error
instead of a snapshot. Operator sees `Failed: <reason>` in the same
center placeholder, replacing the spinner.

### Main-loop channel polling

`Context` gains:

```rust
pub struct InsightsLoadOutcome {
    pub request_id: u64,
    pub result: Result<InsightsSnapshot, String>,
}

// New fields on Context:
insights_load_tx: mpsc::Sender<InsightsLoadOutcome>,
insights_load_rx: mpsc::Receiver<InsightsLoadOutcome>,
next_insights_request_id: u64,
```

The `(tx, rx)` pair is initialized once in `Context::new`. Each `i`
open and each `r` refresh increments `next_insights_request_id`,
records the new id on the overlay via `overlay.mark_loading(id)`, and
spawns a worker thread cloning `tx` and the new id.

Each `event::poll` tick in `tui_loop`:

1. Drain `insights_load_rx` non-blocking (`while let Ok(outcome) =
rx.try_recv() { ... }`).
2. For each outcome, call `overlay.set_snapshot_for(outcome.request_id,
outcome.result)`. The overlay drops outcomes whose id is not the
   current `pending_request_id` (stale).
3. Call `overlay.advance_spinner()` if the overlay is open and
   loading. Phase wraps modulo 10.

### Stale request handling

When the operator presses `r` while a previous load is still in flight,
the new press increments `next_insights_request_id` to N+1. Any
outcome arriving with id ≤ N is dropped on `set_snapshot_for`. This
means:

- A slow first load followed by a fast second load shows the second
  load's result. The first thread's output is silently discarded.
- A fast first load followed by a slow second load shows the first
  load briefly, then the second load's result. The overlay never
  shows stale data overwriting newer data.

### Close-while-loading

If the operator presses `Esc` / `q` / `i` (toggle-off) while a load is
in flight:

- `overlay.close()` runs, `is_open() = false`.
- Worker thread keeps running until SQLite returns. Its outcome
  arrives at some later tick; `set_snapshot_for` is still called but
  the overlay's `pending_request_id` may now be 0 (no current load) or
  the id of a later open. The id check drops the stale outcome
  silently.
- If the operator re-opens with `i` while the old thread is still
  running, the new open spawns a new thread with id N+1. Both threads
  may finish in any order; only the matching id is accepted.

This is safe because `Sender::send` after the receiver is dropped
returns `Err`, but `Receiver` is owned by `Context` for the entire
lifetime of the run, so this case does not occur. If `Context` is
dropped (program shutdown) while threads are running, those threads
are abandoned (process exit cleans them up).

### Spinner cadence

The existing `event::poll(Duration::from_millis(100))` at line 237 of
`tui_loop` provides the tick. On every tick (including the no-event
timeout case), the loop calls `advance_spinner` on the insights
overlay if it is open and loading. Spinner glyphs:

```rust
const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
```

Phase advances by 1 mod 10 per tick, so the visible rotation period is
1 second.

## State machine

```
Closed
  | i press                            (advance_spinner is no-op)
  v
Loading(request_id=N, phase=0)
  |
  | thread completes, set_snapshot_for(N, Ok(snap))
  |   -> Loaded(snap)
  | thread completes, set_snapshot_for(N, Err(msg))
  |   -> Failed(msg)
  | r press
  |   -> Loading(request_id=N+1, phase=0); previous N outcome (if it
  |      arrives) is dropped
  | Esc / q / i (toggle)
  |   -> Closed; previous N outcome (if it arrives) is dropped
  v
Loaded(snap) | Failed(msg)
  | r press                            -> Loading(N+1, 0)
  | Esc / q / i                        -> Closed
```

Spinner phase ticks at 100 ms cadence only while in `Loading`. In
`Loaded` and `Failed` the body renders the existing 6-panel /
error-text views.

## Render specification

### Loading view

The modal block (border, title, close button) renders as today,
unchanged. The body area is replaced with a centered placeholder:

- **Block title:** `Token Insights — loading` (replaces the existing
  refreshed-at title row when in Loading).
- **Body content:** one centered line, vertically positioned at the
  body's vertical midpoint:
  - `Aggregating insights {SPINNER_FRAMES[phase]}` styled
    `theme::BORDER_ACTIVE` with `Modifier::BOLD`.
- **Other panels:** none. The 6-situation aggregation, cache trend,
  action ledger, recommendation timeline panels are NOT rendered while
  loading.
- **Scrollbar:** suppressed. `line_count()` returns 0 while loading so
  scroll keys are no-ops.

### Failed view

Existing behavior: render `Failed: <reason>` in the body. No spinner.
The block title becomes `Token Insights — failed`.

### Loaded view

Existing 6-panel render is unchanged. Block title shows the existing
`Token Insights — refreshed: HH:MM:SS` line.

## Code changes by file

| File                             | Change                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `src/ui/insights.rs`             | `InsightsOverlay` gains `loading: bool`, `spinner_phase: u8`, `pending_request_id: u64`. New methods: `mark_loading(request_id)`, `set_snapshot_for(request_id, Result<InsightsSnapshot, String>)`, `advance_spinner()`, `is_loading()`, `pending_request_id()`. `render_insights_modal` adds a `loading` branch that draws the centered placeholder. `SPINNER_FRAMES` constant. `line_count()` returns 0 while loading.                                                                   |
| `src/app/insights_overlay.rs`    | No changes. `Refresh` action signal continues to flow to `tui_loop`.                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `src/app/insights_load.rs` (new) | `pub struct InsightsLoadOutcome { request_id, result }`. `pub fn spawn_insights_load(audit_path: PathBuf, window: InsightsWindow, ignored_ttl_secs: u64, sender: mpsc::Sender<InsightsLoadOutcome>, request_id: u64)` spawns the worker thread. The thread opens the read-only store and sends the outcome. Errors stringified.                                                                                                                                                            |
| `src/app/bootstrap.rs`           | `Context` gains `insights_load_tx: mpsc::Sender<InsightsLoadOutcome>`, `insights_load_rx: mpsc::Receiver<InsightsLoadOutcome>`, `next_insights_request_id: u64`. `Context::new` initializes the channel. The audit DB path is read from the existing `Context.audit_db_path` (already populated for the synchronous insights path); no new helper.                                                                                                                                         |
| `src/app/tui_loop.rs`            | (1) `i` open dispatch: increments `ctx.next_insights_request_id`, calls `overlay.mark_loading(id)`, calls `spawn_insights_load(...)` cloning `ctx.insights_load_tx`. (2) `Refresh` action handler: same as (1). (3) Tick body (right after `event::poll` returns): `while let Ok(outcome) = ctx.insights_load_rx.try_recv() { overlay.set_snapshot_for(outcome.request_id, outcome.result); }`. (4) Tick body: `if insights_overlay.is_loading() { insights_overlay.advance_spinner(); }`. |
| `src/lib.rs`                     | export `insights_load` module.                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `docs/ai/UI_MANUAL.md`           | §8.9: one paragraph noting that `i` / `r` fetch asynchronously and show a centered spinner placeholder while the snapshot is being aggregated.                                                                                                                                                                                                                                                                                                                                             |
| `docs/ai/ARCHITECTURE.md`        | v1.50.0 paragraph: worker-thread + mpsc channel fetch path for the `i` overlay.                                                                                                                                                                                                                                                                                                                                                                                                            |
| `docs/ai/VALIDATION.md`          | v1.50.0 row + evidence line for the loading-state render test.                                                                                                                                                                                                                                                                                                                                                                                                                             |

## Tests (TDD order)

Each task in the implementation plan starts with the failing test.

1. `mark_loading_sets_loading_flag_and_pending_id` — overlay starts
   not-loading; after `mark_loading(7)` it reports loading=true and
   `pending_request_id() == 7`.
2. `loading_state_renders_centered_spinner_and_text` — render after
   `mark_loading(1)` shows `Aggregating insights ⠋` at the body's
   vertical midpoint; the 6-panel labels ("CACHE TREND",
   "ACTION LEDGER", etc.) do not appear.
3. `advance_spinner_cycles_through_braille_phases` — phase advances
   0→1→…→9→0 across 11 calls. Idempotent if `is_loading() == false`.
4. `set_snapshot_for_drops_stale_request_id` — `mark_loading(2)`,
   `set_snapshot_for(1, Ok(...))` keeps overlay in loading state,
   pending_id unchanged.
5. `set_snapshot_for_accepts_matching_request_id_with_ok` — `mark_
loading(2)`, `set_snapshot_for(2, Ok(snap))` swaps to Loaded;
   loading=false; snapshot Some.
6. `set_snapshot_for_accepts_matching_request_id_with_err` — same as
   #5 but Err path: error Some, snapshot None, loading=false.
7. `refresh_increments_request_id_and_clears_snapshot` — sequence: id
   1 load completes, then `mark_loading(2)`. snapshot becomes None,
   loading=true, pending_id=2.
8. `close_while_loading_drops_late_outcome` — `mark_loading(3)`,
   `close()`, `set_snapshot_for(3, Ok(snap))` is a no-op (overlay
   closed, snapshot stays None when reopened).
9. `line_count_is_zero_while_loading` — `mark_loading(1)` →
   `line_count() == 0` so scroll keys cannot move.
10. Integration test in `src/app/insights_load.rs`:
    `spawn_insights_load_emits_outcome_with_matching_request_id` —
    create a temp audit DB, spawn the load, recv on the channel, assert
    `outcome.request_id == 42` and `result.is_ok()`.
11. Integration test:
    `spawn_insights_load_emits_err_for_missing_db` — spawn against a
    non-existent path, recv outcome, assert `result.is_err()`.

## Edge cases and risks

- **Thread panic:** wrap the worker body in `catch_unwind` and send
  `Err("load thread panicked")` if caught. Otherwise the channel
  receiver waits forever (overlay stays in Loading). Risk: low (the
  read-only path is well-tested), but the wrap is cheap.
- **Channel disconnected:** can't happen during runtime — `Context`
  owns the receiver for the entire process lifetime.
- **`i` press while loading:** treated as toggle-close (existing
  behavior). Re-opening with `i` again increments id and respawns. The
  prior thread's outcome is dropped on the id check.
- **Multiple rapid `r` presses:** each press spawns a new thread and
  bumps the id. All but the latest outcome are dropped. Threads run to
  completion; bounded by SQLite query cost.
- **Test for spinner rotation across ticks:** unit-test against
  `advance_spinner` directly (#3). Don't try to test real-time
  rotation across `event::poll` ticks — that's integration-test
  territory and brittle.
- **Existing synchronous tests:** any test that relied on the
  synchronous "open `i`, snapshot is immediately Some" pattern will
  break. We update those tests to call `set_snapshot_for(id,
Ok(test_snapshot))` directly to simulate thread completion.

## Validation gates

The standard Qmonster gates from `docs/ai/VALIDATION.md`:

- `cargo fmt --all --check`
- `cargo test --all-targets` — expect lib test count to grow by ~10
  (the tests above) plus 2 integration tests; existing
  insights-related tests are updated to the new model rather than
  added.
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

## Performance considerations

- Spawning a thread per `i`/`r` press is fine for an interactive TUI.
  The press cadence is bounded by the operator (well below 1 Hz in
  practice).
- The audit DB read-only open is a fresh connection per request. This
  matches the existing `qmonster insights --since` CLI path which also
  opens a read-only connection per invocation.
- Spinner advancement is a single u8 increment per tick; negligible.
- Channel `try_recv` per tick is non-blocking and amortized O(1) when
  empty.

## Out-of-scope cleanup deferred

- Generalizing the "loading state + worker thread + channel" pattern
  into a reusable abstraction. v1.50 keeps it inline in
  `InsightsOverlay` + `tui_loop`. If a future slice (e.g. an `n`
  history view fetch) repeats the pattern, that's the time to lift it.
- Operator-tunable spinner cadence. v1.50 hardcodes the 100 ms tick
  (matches the event-loop poll). Not a config surface.

## References

- `src/ui/insights.rs` (current `InsightsOverlay` definition)
- `src/app/insights_overlay.rs` (current key/mouse handler)
- `src/app/tui_loop.rs:237` (existing `event::poll(Duration::from_
millis(100))` cadence used as the spinner tick source)
- `src/store/insights.rs` (existing `SqliteInsightsStore` +
  `snapshot_with_ignored_ttl` API)
- v1.49.0 mission ledger entry (`mission-history.yaml` change_sequence 192) — most recent ancestor release
