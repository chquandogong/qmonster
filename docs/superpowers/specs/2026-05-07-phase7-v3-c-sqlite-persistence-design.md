# Phase 7 v3 (c) — SQLite Persistence for Anomaly Events + History (v1.47.0) — Design Spec

**Date:** 2026-05-07
**Target version:** v1.47.0
**Predecessor:** v1.46.0 Phase 7 v3 (a+b) — per-kind promotion gate + `n` overlay
**Closes:** Phase 7 v3 (a/b/c). Phase 7 is feature-complete after v1.47.0.

---

## 1. Goal

Persist Phase 7 anomaly state across Qmonster restarts:

1. **`AnomalyEvent` log persistence** — every visible signal pushed to `AnomalyEventsRing` is also INSERTed into a new `anomaly_events` table.
2. **Lazy `AnomalyHistory` deque replay** — on first observation of a pane in a new session, hydrate per-pane deques from the most recent persisted snapshots so detectors don't need 20 ticks to warm up after a restart.
3. **Retention policy** — time-based primary (`[anomaly] retention_days = 30` default) + 100K-row emergency cap on `anomaly_events`; snapshots auto-purge at 4× the detection window.
4. **`n` overlay history view** — new `h` keybind toggles between the session-scoped in-memory ring (the v1.46 default) and a persistent history view that queries the last 200 events from disk.
5. **AnomalyHistory snapshot table** — one row per `(pane_id, tick_unix_secs)` capturing the per-tick observation that fed the in-memory deques.

## 2. Non-goals

- No `session_id` column. The session-scoped ring stays purely in-memory; the history view drops the session boundary.
- No audit-chain hash for `anomaly_events`. The existing `audit_events` table has no rolling hash either; persistence here is a plain log.
- No schema migration of _existing_ tables. v1.47 is purely additive — `audit_events`, `token_usage_samples`, `cost_usage_events`, `cost_budget_alerts` are unchanged.
- No `AnomalyKind`, `AnomalyConfidence`, `AnomalyEvent`, `AnomalyEventsRing`, `AnomalyHistory`, or detector-function changes. Persistence sits at the write boundary; detection consumes from in-memory deques unchanged.
- No SQLite `VACUUM` automation. If operators hit disk pressure, they can `VACUUM` manually; this is deferred.
- No pagination in the history view. A single `LIMIT 200` query feeds the overlay; ratatui scroll handles in-buffer pagination.
- No per-kind retention overrides. One global `retention_days`.
- No replay across `pane_id` reuse. Replay is keyed strictly on `pane_id`; if a pane id is recycled for a different provider mid-session, the replayed deques may be briefly inconsistent — detectors re-trim on subsequent ticks.
- No `[anomaly] enabled = false` data path changes. When detection is off, no events are pushed and no INSERTs happen. Retention purge still runs on startup.

## 3. Architecture overview

```
┌──────────────────┐       ┌──────────────────┐
│ eval_anomalies   │──────▶│ AnomalySignal[]  │
└──────────────────┘       └────────┬─────────┘
                                    │
                                    ▼
                       ┌──────────────────────────┐
                       │ promote gate (v1.46)     │
                       └────────────┬─────────────┘
                                    │
                ┌───────────────────┼─────────────────────┐
                ▼                   ▼                     ▼
       ┌────────────────┐  ┌────────────────────┐  ┌──────────────────────┐
       │ Recommendations│  │ AnomalyEventsRing  │  │ SqliteAnomalySink    │
       │  (v1.44 path)  │  │ (in-memory, 100)   │  │  .insert_anomaly_    │
       └────────────────┘  └────────────────────┘  │   event(&event)      │
                                                   └──────────┬───────────┘
                                                              │
                                                              ▼
                                                   ┌──────────────────────┐
                                                   │ anomaly_events table │
                                                   └──────────────────────┘

  AnomalyHistory push site (per pane per tick):
       in-memory deques  ──▶  SqliteAnomalySink.upsert_anomaly_history_snapshot(...)
                                              │
                                              ▼
                                anomaly_history_snapshots table

  On first-observation of a pane in this session:
       SqliteAnomalySink.fetch_recent_history_snapshots(pane_id, window_polls)
       ──▶ if newest snapshot < (window_polls × tick_seconds × 2) old
           ──▶ replay into in-memory AnomalyHistory deques

  On `n` overlay `h` key:
       AnomalyOverlay.view = History
       ──▶ SqliteAnomalySink.fetch_recent_anomaly_events(limit=200)
       ──▶ AnomalyOverlay.history_cache populated
       ──▶ render iterates history_cache instead of ring
```

Two parallel persistent paths (mirroring v1.46.0's parallel ring + recommendations from `event_loop`):

- **Event path:** every push to the in-memory ring also INSERTs into `anomaly_events`. The ring stays session-scoped (the v1.46 spec's "session-only" framing is preserved); disk is the cross-session truth.
- **History path:** every push to the in-memory `AnomalyHistory` also UPSERTs into `anomaly_history_snapshots`. On first-observation per pane per session, lazy replay reconstructs deques from the persisted snapshots if recent enough.

Both paths run BEFORE `deliver_effects` and the audit_event loop — same sequencing pattern locked in by v1.44.0.

## 4. Schema

Both tables are declared inside `AuditDb::open` alongside the existing `AUDIT_SCHEMA`, `TOKEN_USAGE_SCHEMA`, `COST_USAGE_SCHEMA` constants. The reads/writes go through a new `SqliteAnomalySink` (at `src/store/anomaly_sink.rs`) that owns its own `AuditDb` instance — mirroring the existing `SqliteTokenUsageSink` (`src/store/token_usage.rs`) and `SqliteCostUsageSink` (`src/store/cost_usage.rs`) pattern. Because `AuditDb::open` is idempotent (`CREATE TABLE IF NOT EXISTS`), multiple sinks pointed at the same file share the schema migration cleanly.

### 4.1 `anomaly_events`

```sql
CREATE TABLE IF NOT EXISTS anomaly_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_unix_secs    INTEGER NOT NULL,
    pane_id         TEXT    NOT NULL,
    kind            TEXT    NOT NULL,        -- AnomalyKind.label() ("CostSlope", etc.)
    confidence      TEXT    NOT NULL,        -- AnomalyConfidence.label() ("low"/"medium"/"high")
    severity        TEXT    NOT NULL,        -- "concern"/"warning"
    promoted        INTEGER NOT NULL,        -- 0 or 1
    reason          TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_anomaly_events_ts
    ON anomaly_events(ts_unix_secs DESC);
CREATE INDEX IF NOT EXISTS idx_anomaly_events_pane_ts
    ON anomaly_events(pane_id, ts_unix_secs DESC);
```

`severity` is stored as a string label rather than an integer, matching the existing `audit_events.severity TEXT` precedent. The mapping is `Severity::Concern → "concern"`, `Severity::Warning → "warning"` (we add a `Severity::label() -> &'static str` helper if not already present).

### 4.2 `anomaly_history_snapshots`

```sql
CREATE TABLE IF NOT EXISTS anomaly_history_snapshots (
    pane_id                    TEXT    NOT NULL,
    tick_unix_secs             INTEGER NOT NULL,
    identity_provider          TEXT,           -- NULL when identity_snapshots was absent that tick
    identity_path              TEXT,
    error_hint                 INTEGER,        -- 0/1, NULL if absent
    cache_hit_ratio            REAL,           -- NULL if absent
    cache_drift_fire           INTEGER,        -- 0/1
    cross_pane_edit_paths_json TEXT,           -- JSON array (empty array "[]" if absent)
    cost_usd                   REAL,
    input_tokens               INTEGER,
    output_tokens              INTEGER,
    process_memory_mb          REAL,
    agent_memory_bytes         INTEGER,
    subagent_hint              INTEGER,        -- 0/1
    PRIMARY KEY (pane_id, tick_unix_secs)
);
CREATE INDEX IF NOT EXISTS idx_anomaly_history_pane_tick
    ON anomaly_history_snapshots(pane_id, tick_unix_secs DESC);
```

The PRIMARY KEY `(pane_id, tick_unix_secs)` lets us use `INSERT OR REPLACE` (rusqlite's `execute` with `ON CONFLICT REPLACE`) — defensive against any double-push within a single tick.

The `cross_pane_edit_paths_json` column stores a JSON array of strings (`["src/foo.rs", "src/bar.rs"]`), serialized via `serde_json::to_string` at write time and parsed at replay time. This is the only field that doesn't fit a primitive SQL type cleanly — single column with JSON keeps the schema simple.

Boolean fields use `INTEGER` (0/1) following the existing SQLite convention in this codebase.

## 5. Data shapes

### 5.1 `AnomalyHistorySnapshot` (new, in `src/store/anomaly_history.rs`)

```rust
//! Phase 7 v3 (c) (v1.47.0): per-tick snapshot of the inputs the
//! detectors consumed. Mirrors `AnomalyHistory` deque heads, written
//! once per pane per tick. Used for restart-survival replay.

#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyHistorySnapshot {
    pub identity: Option<(String, String)>,
    pub error_hint: Option<bool>,
    pub cache_hit_ratio: Option<f32>,
    pub cache_drift_fire: bool,
    pub cross_pane_edit_paths: Vec<String>,
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub process_memory_mb: Option<f64>,
    pub agent_memory_bytes: Option<u64>,
    pub subagent_hint: bool,
}
```

This struct is the write/read boundary type. It does NOT replace `AnomalyHistory` — that stays the in-memory rolling deque structure detectors consume. `AnomalyHistorySnapshot` represents one tick's contribution to those deques.

A small constructor + helper live alongside it:

```rust
impl AnomalyHistorySnapshot {
    /// Build from the values that just got pushed onto AnomalyHistory
    /// in event_loop's per-pane block.
    pub fn capture_tick(...) -> Self;
}

/// Push one snapshot's fields into the corresponding AnomalyHistory deques.
/// Used for replay. Matches the existing per-tick push order in event_loop.
pub fn push_snapshot_into_history(history: &mut AnomalyHistory, snap: &AnomalyHistorySnapshot);
```

`push_snapshot_into_history` mirrors the existing per-tick push logic in `event_loop` (search for the line that pushes onto `history.identity_snapshots`). Reusing the same push semantics keeps replay consistent with live detection.

### 5.2 `AnomalyOverlayView` (new enum, in `src/ui/anomaly_overlay.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyOverlayView {
    Ring,
    History,
}

impl Default for AnomalyOverlayView {
    fn default() -> Self {
        Self::Ring
    }
}
```

`AnomalyOverlay` extended:

```rust
pub struct AnomalyOverlay {
    open: bool,
    scroll: u16,
    view: AnomalyOverlayView,
    history_cache: Vec<AnomalyEvent>,
}
```

Cache is populated when `view` flips to `History`; cleared when flipping back to `Ring` or on overlay close. New methods:

```rust
impl AnomalyOverlay {
    pub fn view(&self) -> AnomalyOverlayView;
    pub fn toggle_view(&mut self, history_cache: Vec<AnomalyEvent>);  // populates cache for History
    fn clear_cache(&mut self);
}
```

`toggle_view` takes the freshly-fetched events because the overlay shouldn't talk to `AuditDb` directly — the `tui_loop.rs` layer fetches and passes the result, mirroring the existing pattern where overlay state is pure and dispatch is wired externally.

### 5.3 `AnomalyConfig.retention_days` (new field, in `src/app/config.rs`)

```rust
pub struct AnomalyConfig {
    // ... existing fields ...
    /// Phase 7 v3 (c) (v1.47.0): retention horizon for `anomaly_events`
    /// rows. Older rows are deleted on Qmonster startup. Default 30.
    /// A 100K-row hard cap also applies regardless of this value.
    pub retention_days: u64,
}
```

Default 30. `#[serde(default)]` already on the parent ensures pre-v1.47 toml files load with the default applied.

## 6. Write path

### 6.1 New `SqliteAnomalySink` (in `src/store/anomaly_sink.rs`)

Mirrors `SqliteTokenUsageSink` / `SqliteCostUsageSink`: thin wrapper around an `AuditDb` instance, with anomaly-specific methods.

```rust
pub struct SqliteAnomalySink {
    db: AuditDb,
}

impl SqliteAnomalySink {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self { db: AuditDb::open(path)? })
    }

    /// INSERT one row into anomaly_events. Failures are returned;
    /// callers ignore the Err (degrade gracefully — never block detection).
    pub fn insert_anomaly_event(
        &self,
        event: &crate::domain::anomaly::AnomalyEvent,
    ) -> Result<(), SqliteError>;

    /// INSERT OR REPLACE one row into anomaly_history_snapshots.
    pub fn upsert_anomaly_history_snapshot(
        &self,
        pane_id: &str,
        tick_unix_secs: u64,
        snap: &crate::store::anomaly_history::AnomalyHistorySnapshot,
    ) -> Result<(), SqliteError>;

    /// Run a closure inside a SQLite transaction. Used by event_loop
    /// to batch all per-tick writes into a single fsync.
    pub fn with_transaction<F, R>(&self, f: F) -> Result<R, SqliteError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<R, SqliteError>;

    /// Read methods (used by overlay history view + replay).
    pub fn fetch_recent_anomaly_events(&self, limit: usize) -> Vec<AnomalyEvent>;
    pub fn fetch_recent_history_snapshots(
        &self,
        pane_id: &str,
        window_polls: usize,
    ) -> Vec<(u64, AnomalyHistorySnapshot)>;

    /// Retention purge. Called from Context::new (via with_anomaly_sink helper).
    pub fn run_retention(
        &self,
        retention_days: u64,
        window_polls: usize,
        tick_seconds: u64,
    ) -> Result<(), SqliteError>;
}
```

The `with_transaction` callback receives `&Transaction<'_>` so per-tick batches can run multiple writes through `tx.execute(...)` directly without going back through the `&self` API.

### 6.2 `event_loop` write site changes (in `src/app/event_loop.rs`)

Existing v1.46.0 layout (around lines 530-555):

```rust
let now_secs: u64 = (current_unix_ms() / 1000) as u64;
for sig in &anomalies {
    let promoted_bool = sig.confidence >= gates.promote_min_confidence(sig.kind);
    let reason = /* build reason from evidence */;
    ctx.anomaly_events_ring.push(AnomalyEvent { ... });
}
```

v1.47.0 wraps both the existing ring push AND the existing AnomalyHistory push site (the one slightly earlier that builds the per-tick observation) inside a single per-tick transaction. Pseudocode (the actual structure depends on where the `AnomalyHistory` push lives — verify at impl time):

```rust
if let Some(anomaly_sink) = ctx.anomaly_sink.as_ref() {
    let _ = anomaly_sink.with_transaction(|tx| {
        // Write each visible signal:
        for sig in &anomalies {
            let promoted_bool = sig.confidence >= gates.promote_min_confidence(sig.kind);
            let reason = /* ... */;
            let event = AnomalyEvent { /* ... */ };
            ctx.anomaly_events_ring.push(event.clone());
            tx.execute(
                "INSERT INTO anomaly_events
                 (ts_unix_secs, pane_id, kind, confidence, severity, promoted, reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    event.timestamp as i64,
                    event.pane_id,
                    event.kind.label(),
                    event.confidence.label(),
                    event.severity.label(),
                    event.promoted as i64,
                    event.reason,
                ],
            )?;
        }
        // Write the history snapshot for this pane (built from the
        // values already pushed into history this tick):
        let snap = AnomalyHistorySnapshot::capture_tick(/* ... */);
        // (similar tx.execute INSERT OR REPLACE for anomaly_history_snapshots)
        Ok(())
    });
}
```

Note: the per-pane block is already inside the `for pane in panes` loop, so each pane gets its own per-tick transaction. Cross-pane atomicity isn't required — each pane's row is independent.

If `ctx.anomaly_sink` is `None` (test fixtures or operator running without persistence), the `if let` guard skips the writes entirely. The in-memory ring push and AnomalyHistory push always happen — persistence is purely additive.

### 6.3 Lazy AnomalyHistory replay (in `src/app/event_loop.rs`)

New per-pane gate before the existing AnomalyHistory push site:

```rust
if !ctx.anomaly_history_replayed.contains(&pane.pane_id) {
    if let Some(anomaly_sink) = ctx.anomaly_sink.as_ref() {
        let snapshots = anomaly_sink
            .fetch_recent_history_snapshots(&pane.pane_id, gates.anomaly_window_polls);
        let staleness_threshold =
            (gates.anomaly_window_polls as u64) * tick_seconds * 2;
        let now_secs = (current_unix_ms() / 1000) as u64;
        if let Some((newest_ts, _)) = snapshots.first() {
            if now_secs.saturating_sub(*newest_ts) <= staleness_threshold {
                let history = ctx.anomaly_history.entry(pane.pane_id.clone()).or_default();
                for (_, snap) in snapshots.iter().rev() {
                    push_snapshot_into_history(history, snap);
                }
                history.trim(gates.anomaly_window_polls);
            }
        }
    }
    ctx.anomaly_history_replayed.insert(pane.pane_id.clone());
}
```

`tick_seconds` is derived from the existing `[tmux] poll_interval_ms` config:

```rust
let tick_seconds = (ctx.config.tmux.poll_interval_ms / 1000).max(1);
```

The default `poll_interval_ms = 2000` gives `tick_seconds = 2`, so with the default `window_polls = 20` the staleness threshold is `20 × 2 × 2 = 80 seconds` — anything older than 80s means cold-start.

### 6.4 Retention purge (`SqliteAnomalySink::run_retention`)

The retention method on the new sink runs the three DELETEs and is called once from the runtime startup path (e.g., `Context::new` or wherever the sink is plugged in via a builder method). `AuditDb::open` itself stays config-agnostic — it only owns schema migration.

```rust
impl SqliteAnomalySink {
    pub fn run_retention(
        &self,
        retention_days: u64,
        window_polls: usize,
        tick_seconds: u64,
    ) -> Result<(), SqliteError> {
        let conn = self.db.connection().lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let now_secs = (current_unix_ms() / 1000) as u64;

        // Time-based primary purge.
        let events_threshold = now_secs.saturating_sub(retention_days * 86400);
        conn.execute(
            "DELETE FROM anomaly_events WHERE ts_unix_secs < ?1",
            rusqlite::params![events_threshold as i64],
        ).map_err(|e| SqliteError::Query(e.to_string()))?;

        // Hard cap: keep last 100K rows by id (newest).
        conn.execute(
            "DELETE FROM anomaly_events
             WHERE id NOT IN (
                 SELECT id FROM anomaly_events ORDER BY id DESC LIMIT 100000
             )",
            [],
        ).map_err(|e| SqliteError::Query(e.to_string()))?;

        // Snapshots: 4× window auto-purge.
        let snapshots_threshold = now_secs.saturating_sub(
            (window_polls as u64) * tick_seconds * 4,
        );
        conn.execute(
            "DELETE FROM anomaly_history_snapshots WHERE tick_unix_secs < ?1",
            rusqlite::params![snapshots_threshold as i64],
        ).map_err(|e| SqliteError::Query(e.to_string()))?;

        Ok(())
    }
}
```

Called from a new `Context::with_anomaly_sink(sink)` builder method (mirroring the existing `with_token_usage_sink` / `with_cost_usage_sink` builders), which also stores the sink on `Context` and triggers the retention purge:

```rust
impl<P, N> Context<P, N> {
    pub fn with_anomaly_sink(mut self, sink: SqliteAnomalySink) -> Self {
        let _ = sink.run_retention(
            self.config.anomaly.retention_days,
            self.config.anomaly.window_polls,
            (self.config.tmux.poll_interval_ms / 1000).max(1),
        );
        self.anomaly_sink = Some(sink);
        self
    }
}
```

Failures from `run_retention` are logged via `eprintln!` inside the method (or via `let _ =` here) — a failed purge degrades gracefully into "table grows further this session"; it does not block startup.

## 7. Read path

### 7.1 `SqliteAnomalySink::fetch_recent_anomaly_events(limit)`

```rust
impl SqliteAnomalySink {
    pub fn fetch_recent_anomaly_events(&self, limit: usize) -> Vec<AnomalyEvent>;
}
```

Query: `SELECT ts_unix_secs, pane_id, kind, confidence, severity, promoted, reason FROM anomaly_events ORDER BY ts_unix_secs DESC, id DESC LIMIT ?1`. Returns newest-first. Empty vec on DB read error (logged via `eprintln!`).

The kind/confidence/severity strings are parsed back into enum variants via existing `from_label` helpers (or new `try_from_label` constructors as needed; small additions to `domain/anomaly.rs` and `domain/recommendation.rs`).

### 7.2 `SqliteAnomalySink::fetch_recent_history_snapshots(pane_id, window_polls)`

```rust
impl SqliteAnomalySink {
    pub fn fetch_recent_history_snapshots(
        &self,
        pane_id: &str,
        window_polls: usize,
    ) -> Vec<(u64, AnomalyHistorySnapshot)>;
}
```

Query: `SELECT tick_unix_secs, identity_provider, identity_path, ... FROM anomaly_history_snapshots WHERE pane_id = ?1 ORDER BY tick_unix_secs DESC LIMIT ?2`. Returns newest-first; replay caller iterates `.rev()` to push oldest-first into the deques.

### 7.3 `n` overlay history-view toggle

`handle_anomaly_overlay_key` (in `src/app/anomaly_overlay.rs`) gets a new arm for `'h'`:

```rust
KeyCode::Char('h') if overlay.is_open() => {
    // The handler has access to `&mut AnomalyOverlay` and `ring_len`,
    // but not to AuditDb. The actual fetch happens at the dispatch
    // layer (tui_loop.rs) when it sees `h` was consumed.
    // ...
}
```

The cleanest split: `handle_anomaly_overlay_key` returns a richer signal than just `bool`. Two options:

**Option A — Return a new enum:**

```rust
pub enum AnomalyOverlayKeyResult {
    Ignored,
    Consumed,
    NeedsHistoryFetch,
    NeedsClearCache,
}
```

The dispatch in `tui_loop.rs` matches on this and calls `audit_db.fetch_recent_anomaly_events(200)` when needed.

**Option B — Move the toggle one level up:**

`handle_anomaly_overlay_key` ignores `'h'`. `tui_loop.rs::run` adds an explicit `KeyCode::Char('h')` arm when the anomaly overlay is open: it fetches from `audit_db` and calls `anomaly_overlay.toggle_view(events)`.

Option B is closer to the existing pattern in this codebase (other overlay-specific keys with side effects live at the dispatch layer). Choose Option B; the handler keeps its simple `-> bool` contract.

`tui_loop.rs` change (inside the existing "consumes all keys while open" block for the anomaly overlay):

```rust
if anomaly_overlay.is_open() {
    if matches!(k.code, KeyCode::Char('h')) {
        match anomaly_overlay.view() {
            AnomalyOverlayView::Ring => {
                let events = ctx
                    .anomaly_sink
                    .as_ref()
                    .map(|sink| sink.fetch_recent_anomaly_events(200))
                    .unwrap_or_default();
                anomaly_overlay.toggle_view(events);
            }
            AnomalyOverlayView::History => {
                anomaly_overlay.toggle_view(Vec::new());  // back to Ring; cache cleared
            }
        }
        continue;
    }
    // existing all-keys-consumed dispatch:
    let _ = handle_anomaly_overlay_key(&mut anomaly_overlay, ctx.anomaly_events_ring.len(), k.code);
    continue;
}
```

### 7.4 Render branch for History view

`AnomalyOverlay::render` reads `self.view`:

```rust
let events: Vec<&AnomalyEvent> = match self.view {
    AnomalyOverlayView::Ring => ring.iter().rev().collect(),
    AnomalyOverlayView::History => self.history_cache.iter().collect(),
};
```

Title bar reflects mode:

- Ring: `format!(" ANOMALY EVENTS (last {} — newest first) [h: history] [n to close] ", ring.len())`
- History: `format!(" ANOMALY EVENTS [history view, last {}] [h: ring] [n to close] ", self.history_cache.len())`

Empty-state message in History view: `"No persisted anomaly events. Enable [anomaly] in Settings to start recording."`

## 8. Lifecycle integration

### 8.1 `Context<P, N>` extensions (in `src/app/bootstrap.rs`)

```rust
pub struct Context<P: PaneSource, N: NotifyBackend> {
    // ... existing fields ...
    pub anomaly_sink: Option<SqliteAnomalySink>,                   // new
    pub anomaly_history_replayed: std::collections::HashSet<String>,  // new
}
```

`anomaly_sink` mirrors the existing `token_usage_sink: Option<SqliteTokenUsageSink>` and `cost_usage_sink: Option<SqliteCostUsageSink>` fields. `Context::new` initializes both new fields to `None` / `HashSet::new()`. The `with_anomaly_sink(sink)` builder method (Section 6.4) plugs the sink in and runs retention.

`anomaly_history_replayed` is never reset during a session — it's a "once per pane per session" guard.

### 8.2 `Config` plumbing

`AnomalyConfig.retention_days: u64` (default 30) is added alongside the existing `AnomalyConfig` fields. `tick_seconds` is derived from the existing `[tmux] poll_interval_ms` field via `(poll_interval_ms / 1000).max(1)` — same source the rest of the runtime uses for poll cadence.

### 8.3 Behavior when `[anomaly] enabled = false`

- Detection returns empty → ring stays empty → no INSERTs.
- `AnomalyHistory` is still pushed onto each tick (the existing in-memory push doesn't gate on `enabled`); but the new write block is gated on `enabled` to avoid unnecessary disk I/O for the off-state.
- Replay attempts on first-observation still query the DB but find no recent rows (or snapshots are too stale), so cold-start is the result.
- Retention purge runs unconditionally on `Context::new`.
- Overlay opens, ring is empty, history toggle queries the (possibly populated from prior enabled session) DB.

## 9. Backwards compatibility

- `CREATE TABLE IF NOT EXISTS` is idempotent — pre-v1.47 SQLite files load cleanly with the new tables added on open.
- Pre-v1.47 toml files without `[anomaly] retention_days` get the v1.47 default (30) via `#[serde(default)]` at `AnomalyConfig`.
- Existing tables (`audit_events`, `token_usage_samples`, `cost_usage_events`, `cost_budget_alerts`) are unchanged.
- The DB file path is unchanged.

## 10. Tests

### 10.1 Unit tests (target: ~+18 lib tests)

**`src/store/sqlite.rs`:**

- `anomaly_events_schema_creates_idempotently` — open in-memory DB twice, no error.
- `insert_and_fetch_anomaly_event_roundtrip` — insert one event, fetch returns it with all 7 fields preserved.
- `fetch_recent_anomaly_events_orders_newest_first` — insert 3 with descending timestamps, fetch returns reverse order.
- `fetch_recent_anomaly_events_respects_limit` — insert 250, fetch with limit=200 returns 200.
- `anomaly_history_snapshots_upsert_replaces_on_conflict` — upsert twice for same (pane_id, tick), second wins.
- `fetch_recent_history_snapshots_filters_by_pane_id`.
- `retention_purge_drops_events_older_than_days` — insert old + new, purge with 30 days, only new remains.
- `retention_purge_enforces_100k_hard_cap` — insert 100,001, purge keeps newest 100,000.
- `retention_purge_drops_snapshots_older_than_4x_window`.

**`src/store/anomaly_history.rs`:**

- `snapshot_capture_and_replay_roundtrip` — capture from a synthetic `AnomalyHistory`, replay into a fresh `AnomalyHistory`, fields match.
- `snapshot_handles_absent_optional_fields` — None values round-trip as NULL.
- `snapshot_serializes_empty_cross_pane_paths_as_empty_array`.

**`src/ui/anomaly_overlay.rs`:**

- `toggle_view_switches_to_history_with_cache`.
- `toggle_view_back_to_ring_clears_cache`.
- `render_history_view_shows_history_title_indicator`.
- `render_history_view_empty_shows_help_hint`.

**`src/app/anomaly_overlay.rs` (or `src/app/tui_loop.rs` if dispatch lives there):**

- `h_key_in_ring_view_triggers_history_fetch` — uses an in-memory AuditDb fixture.
- `h_key_in_history_view_returns_to_ring`.

### 10.2 Integration tests (target: +2)

`tests/event_loop_integration.rs`:

- `event_loop_persists_anomaly_event_to_audit_db` — fixture with ctx.audit_db set; run one tick that fires IdentityChurn; assert the row is in the DB and the in-memory ring has it.
- `event_loop_replays_anomaly_history_on_first_observation` — pre-populate `anomaly_history_snapshots` for a pane; start with empty in-memory `AnomalyHistory`; run one tick; verify the deques were hydrated.

### 10.3 Test count target

- Lib tests: 1162 (v1.46) → ~1180 (+~18).
- Integration tests: 58 (v1.46) → 60 (+2).

## 11. Validation gates

Same set as v1.46:

- `cargo fmt --all --check`
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --all-targets`
- `git diff --check`
- `scripts/release/dry-run.sh` + SBOM diff guard (inherited from CI).

## 12. Doc updates

- `docs/ai/UI_MANUAL.md` — extend §8.8 (the v1.46 Anomaly Events Overlay subsection): document `h` keybind, the two view modes, and the History-view query semantics (last 200 from disk).
- `docs/ai/ARCHITECTURE.md` — append v1.47.0 paragraph (two new tables, per-tick transaction batching, lazy replay with staleness gate, retention).
- `docs/ai/VALIDATION.md` — add Phase 7 v3 (c) row.
- `docs/ai/PROJECT_BRIEF.md` — Phase line + Date line updated.
- `config/qmonster.example.toml` — `# retention_days = 30` (commented at default).
- `README.md` release table → `v1.47.0`.
- `VERSION.md` → `v1.47.0` / `1.47.0`.
- `package.json` → `1.47.0`.

## 13. Release notes call-outs

1. **Headline:** Phase 7 v3 (c): SQLite persistence for anomaly events + history. Phase 7 closes after this release.
2. **Schema additions:** two new tables (`anomaly_events`, `anomaly_history_snapshots`), purely additive via `CREATE TABLE IF NOT EXISTS`. No migration of existing tables.
3. **History view:** `h` inside the `n` overlay toggles between session ring (v1.46 default) and persistent history (last 200 from disk).
4. **Retention:** operator-tunable `[anomaly] retention_days` (default 30) + 100K-row hard cap on events. Snapshots auto-purge at 4× the detection window.
5. **Lazy replay:** on first observation of a pane in a session, deques hydrate from the most recent persisted snapshots if the newest is < 2× window-old. Otherwise cold-start.
6. **Phase 7 closes.** v3(a) per-kind promotion gate, v3(b) `n` overlay, v3(c) persistence — all shipped. D3 per-subagent token attribution remains the only outstanding Phase 7 follow-up; it stays provider-blocked.

## 14. Out of scope (explicit deferrals)

- `session_id` column / cross-session analytics
- Audit-chain hashing for `anomaly_events`
- History-view pagination beyond `LIMIT 200` (within-buffer scroll covers it)
- Per-kind retention overrides
- SQLite `VACUUM` automation
- Replay across `pane_id` reuse / provider changes
- Snapshot persistence when `[anomaly] enabled = false`
- Stored / parameterized queries (rusqlite handles parameter binding fine)

## 15. Topology summary (for mission ledger)

- New files:
  - `src/store/anomaly_history.rs` — `AnomalyHistorySnapshot` struct + `push_snapshot_into_history` helper.
  - `src/store/anomaly_sink.rs` — `SqliteAnomalySink` (write/read methods + `run_retention` + `with_transaction`).
- Modified files:
  - `src/store/sqlite.rs` — two new `CREATE TABLE` schemas appended to `AuditDb::open`'s migration block.
  - `src/store/mod.rs` — register `anomaly_history` and `anomaly_sink` modules; re-export `SqliteAnomalySink`.
  - `src/app/bootstrap.rs` — Context gains `anomaly_sink: Option<SqliteAnomalySink>` and `anomaly_history_replayed: HashSet<String>`; new `with_anomaly_sink(sink)` builder method that runs retention.
  - `src/app/event_loop.rs` — per-tick transaction wrapper around the existing ring push and AnomalyHistory push sites; lazy replay gate before the AnomalyHistory push; reads `tick_seconds = (config.tmux.poll_interval_ms / 1000).max(1)`.
  - `src/app/config.rs` — `AnomalyConfig.retention_days: u64` (default 30).
  - `src/ui/anomaly_overlay.rs` — `AnomalyOverlayView` enum, `view` + `history_cache` fields on `AnomalyOverlay`, `toggle_view` method, render branch on view mode, title-bar mode indicator, empty-state in History view.
  - `src/app/anomaly_overlay.rs` — no change (handler keeps `-> bool` contract; `'h'` dispatch lives in `tui_loop.rs`).
  - `src/app/tui_loop.rs` — `'h'` arm inside the anomaly-overlay-open block; calls `anomaly_sink.fetch_recent_anomaly_events(200)` and `anomaly_overlay.toggle_view(events)`. Also: where the runtime constructs `Context`, plug `with_anomaly_sink(SqliteAnomalySink::open(path)?)` alongside the existing `with_token_usage_sink` / `with_cost_usage_sink` builders.
  - `src/domain/anomaly.rs` — possibly add `AnomalyKind::try_from_label` and `AnomalyConfidence::try_from_label` if not already present (for parsing event rows back from disk).
  - `src/domain/recommendation.rs` — possibly add `Severity::label` / `try_from_label` if not already present.
- Tests: ~+18 lib + 2 integration → 1180 lib + 60 integration.

---

**End of design spec.**
