# Phase 7 v3 (c) — SQLite Persistence for Anomaly Events + History (v1.47.0) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist Phase 7 anomaly state across Qmonster restarts: every visible `AnomalyEvent` lands in a new `anomaly_events` SQLite table; per-pane `AnomalyHistory` deque heads land in `anomaly_history_snapshots` and lazy-replay on first observation per session; the `n` overlay gains an `h` keybind toggling between the session ring (v1.46 default) and a persistent history view (last 200 from disk); operator-tunable retention via `[anomaly] retention_days` (default 30) + 100K-row hard cap.

**Architecture:** New `SqliteAnomalySink` (mirrors the existing `SqliteTokenUsageSink` / `SqliteCostUsageSink` pattern — wraps `AuditDb`, runs idempotent schema migrations on open). Two new tables added inside `AuditDb::open`'s migration block. Per-tick transaction wraps the existing ring push + new history snapshot upsert. Lazy replay gates on a staleness threshold of `window_polls × tick_seconds × 2`. The `n` overlay's `view: AnomalyOverlayView` enum (`Ring` / `History`) drives a render branch; `tui_loop` handles the disk fetch on `h` key.

**Tech Stack:** Rust 2021, ratatui, crossterm, rusqlite (already a dep via `AuditDb`), serde_json (already a dep) for `cross_pane_edit_paths_json`.

**Reference spec:** `docs/superpowers/specs/2026-05-07-phase7-v3-c-sqlite-persistence-design.md`

---

## File Structure (locked-in decomposition)

**New files:**

- `src/store/anomaly_history.rs` — `AnomalyHistorySnapshot` struct + `capture_tick` constructor + `push_snapshot_into_history` helper (replay).
- `src/store/anomaly_sink.rs` — `SqliteAnomalySink` (wraps `AuditDb`, exposes insert/upsert/fetch/with_transaction/run_retention).

**Modified files:**

- `src/store/sqlite.rs` — add two new schema constants (`ANOMALY_EVENTS_SCHEMA`, `ANOMALY_HISTORY_SNAPSHOTS_SCHEMA`); execute them inside `AuditDb::open`.
- `src/store/mod.rs` — register `anomaly_history` + `anomaly_sink` modules; re-export `SqliteAnomalySink` and `AnomalyHistorySnapshot`.
- `src/domain/anomaly.rs` — add `AnomalyKind::try_from_label`, `AnomalyConfidence::try_from_label` (used when reconstructing `AnomalyEvent` from a row).
- `src/domain/recommendation.rs` — add `Severity::label()` and `Severity::try_from_label()` if not already present.
- `src/app/config.rs` — `AnomalyConfig.retention_days: u64` (default 30).
- `src/app/bootstrap.rs` — `Context` gains `anomaly_sink: Option<SqliteAnomalySink>` and `anomaly_history_replayed: HashSet<String>`; new `with_anomaly_sink` builder.
- `src/app/event_loop.rs` — per-tick transaction wrapping ring push + history snapshot upsert; lazy replay gate before history push.
- `src/ui/anomaly_overlay.rs` — `AnomalyOverlayView` enum, `view` + `history_cache` fields on `AnomalyOverlay`, `toggle_view` method, render branch on view mode.
- `src/app/tui_loop.rs` — `'h'` key arm in the anomaly-overlay-open block; plug `with_anomaly_sink` at startup alongside the existing token/cost sink builders.
- `src/ui/settings.rs` — Parameters tab: 1 new row for `[anomaly] retention_days`.
- `tests/event_loop_integration.rs` — 2 new integration tests.
- Docs + version surfaces (Task 13).
- Mission ledger + tag (Task 14).

---

## Task 1: Add `AnomalyEvent` reconstruction helpers + Severity label

**Files:**

- Modify: `src/domain/anomaly.rs` (+ `try_from_label` for AnomalyKind and AnomalyConfidence)
- Modify: `src/domain/recommendation.rs` (+ `Severity::label` and `Severity::try_from_label` if not already present)

These helpers are pure inverse mappings of the existing `.label()` accessors. They're needed by the `fetch_recent_anomaly_events` SQL row mapping in Task 3.

- [ ] **Step 1: Inspect existing label methods**

Run: `grep -n "fn label" src/domain/anomaly.rs src/domain/recommendation.rs`

Verify which labels already exist. Confirmed by spec § 4.1: AnomalyKind.label() returns "CostSlope" etc., AnomalyConfidence.label() returns "low"/"medium"/"high". For `Severity`, check whether `label()` exists; if not, add it.

- [ ] **Step 2: Add `AnomalyKind::try_from_label` + tests**

In `src/domain/anomaly.rs`, after the existing `impl AnomalyKind` block, add:

```rust
impl AnomalyKind {
    pub fn try_from_label(s: &str) -> Option<Self> {
        Some(match s {
            "IdentityChurn" => AnomalyKind::IdentityChurn,
            "ErrorBurst" => AnomalyKind::ErrorBurst,
            "CacheDiscontinuity" => AnomalyKind::CacheDiscontinuity,
            "CrossPaneEditCluster" => AnomalyKind::CrossPaneEditCluster,
            "CostSlope" => AnomalyKind::CostSlope,
            "TokenSlope" => AnomalyKind::TokenSlope,
            "MemoryGrowth" => AnomalyKind::MemoryGrowth,
            "SubagentSideEffect" => AnomalyKind::SubagentSideEffect,
            _ => return None,
        })
    }
}
```

Add tests at the bottom of the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn anomaly_kind_label_roundtrip() {
    let kinds = [
        AnomalyKind::IdentityChurn,
        AnomalyKind::ErrorBurst,
        AnomalyKind::CacheDiscontinuity,
        AnomalyKind::CrossPaneEditCluster,
        AnomalyKind::CostSlope,
        AnomalyKind::TokenSlope,
        AnomalyKind::MemoryGrowth,
        AnomalyKind::SubagentSideEffect,
    ];
    for kind in kinds {
        assert_eq!(AnomalyKind::try_from_label(kind.label()), Some(kind));
    }
}

#[test]
fn anomaly_kind_try_from_label_rejects_garbage() {
    assert_eq!(AnomalyKind::try_from_label(""), None);
    assert_eq!(AnomalyKind::try_from_label("identitychurn"), None); // case-sensitive
    assert_eq!(AnomalyKind::try_from_label("Bogus"), None);
}
```

- [ ] **Step 3: Add `AnomalyConfidence::try_from_label` + test**

In the same file, add:

```rust
impl AnomalyConfidence {
    pub fn try_from_label(s: &str) -> Option<Self> {
        Some(match s {
            "low" => AnomalyConfidence::Low,
            "medium" => AnomalyConfidence::Medium,
            "high" => AnomalyConfidence::High,
            _ => return None,
        })
    }
}
```

Append test:

```rust
#[test]
fn anomaly_confidence_label_roundtrip() {
    for c in [AnomalyConfidence::Low, AnomalyConfidence::Medium, AnomalyConfidence::High] {
        assert_eq!(AnomalyConfidence::try_from_label(c.label()), Some(c));
    }
    assert_eq!(AnomalyConfidence::try_from_label("LOW"), None);
    assert_eq!(AnomalyConfidence::try_from_label(""), None);
}
```

- [ ] **Step 4: Inspect `Severity` and add `label` + `try_from_label` if needed**

Read `src/domain/recommendation.rs` around the `pub enum Severity` definition. If `label()` already exists, skip; otherwise add:

```rust
impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Concern => "concern",
            Severity::Warning => "warning",
            // include any other variants present — match the existing enum exhaustively
        }
    }

    pub fn try_from_label(s: &str) -> Option<Self> {
        Some(match s {
            "concern" => Severity::Concern,
            "warning" => Severity::Warning,
            // include any other variants present
            _ => return None,
        })
    }
}
```

(If `Severity` has variants other than `Concern` / `Warning`, mirror them in both methods. Read the enum first.)

Add tests in the existing `mod tests` block of `src/domain/recommendation.rs`:

```rust
#[test]
fn severity_label_roundtrip() {
    for s in [Severity::Concern, Severity::Warning] {
        assert_eq!(Severity::try_from_label(s.label()), Some(s));
    }
    assert_eq!(Severity::try_from_label("CONCERN"), None);
    assert_eq!(Severity::try_from_label(""), None);
}
```

- [ ] **Step 5: Run tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --lib domain::anomaly::tests::anomaly_kind_label_roundtrip domain::anomaly::tests::anomaly_kind_try_from_label_rejects_garbage domain::anomaly::tests::anomaly_confidence_label_roundtrip domain::recommendation::tests::severity_label_roundtrip`

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/domain/anomaly.rs src/domain/recommendation.rs
git commit -m "phase 7 v3 (c): label/try_from_label helpers for AnomalyKind/Confidence/Severity"
```

---

## Task 2: Add `AnomalyHistorySnapshot` module

**Files:**

- Create: `src/store/anomaly_history.rs`
- Modify: `src/store/mod.rs` (register module)

- [ ] **Step 1: Inspect existing AnomalyHistory push pattern**

Read `src/policy/rules/anomaly.rs` lines ~33-68 (the `AnomalyHistory` struct definition) and `src/app/event_loop.rs` lines ~470-510 (the per-tick AnomalyHistory push site). The Snapshot struct mirrors the deque heads. Note exactly which values are pushed per tick — the Snapshot must capture the same values so replay reconstructs identical state.

- [ ] **Step 2: Create `src/store/anomaly_history.rs`**

```rust
//! Phase 7 v3 (c) (v1.47.0): per-tick snapshot of the inputs the
//! detectors consumed. Mirrors `AnomalyHistory` deque heads, written
//! once per pane per tick. Used for restart-survival replay.
//!
//! Lives in `src/store/` rather than alongside `AnomalyHistory` itself
//! because this type is the persistence boundary — detectors continue
//! to consume from in-memory `AnomalyHistory` deques unchanged.

use crate::policy::rules::anomaly::AnomalyHistory;

#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyHistorySnapshot {
    /// `(provider, current_path)` snapshot, or `None` if the identity
    /// signal was absent that tick.
    pub identity: Option<(String, String)>,
    /// `signals.error_hint` per tick. `None` if the signal was absent.
    pub error_hint: Option<bool>,
    /// `cache_hit_ratio` per tick. `None` if the signal was absent.
    pub cache_hit_ratio: Option<f32>,
    /// Did F-7b cache-drift fire this tick?
    pub cache_drift_fire: bool,
    /// Paths of `ConcurrentFileEdit` findings this tick (possibly empty).
    pub cross_pane_edit_paths: Vec<String>,
    /// Cumulative `signals.cost_usd`. `None` if absent.
    pub cost_usd: Option<f64>,
    /// Cumulative `signals.input_tokens`. `None` if absent.
    pub input_tokens: Option<u64>,
    /// Cumulative `signals.output_tokens`. `None` if absent.
    pub output_tokens: Option<u64>,
    /// `signals.process_memory_mb` (RSS). `None` if absent.
    pub process_memory_mb: Option<f64>,
    /// `signals.agent_memory_bytes`. `None` if absent.
    pub agent_memory_bytes: Option<u64>,
    /// `signals.subagent_hint` per tick.
    pub subagent_hint: bool,
}

impl AnomalyHistorySnapshot {
    /// Build a snapshot from the values that just got pushed onto an
    /// `AnomalyHistory` for the current tick. Reads the front of each
    /// deque (most-recent-first).
    pub fn capture_tick(history: &AnomalyHistory) -> Self {
        Self {
            identity: history.identity_snapshots.front().cloned(),
            error_hint: history.error_hints.front().copied(),
            cache_hit_ratio: history.cache_hit_ratios.front().copied().flatten(),
            cache_drift_fire: history.cache_drift_fires.front().copied().unwrap_or(false),
            cross_pane_edit_paths: history
                .cross_pane_edit_paths
                .front()
                .cloned()
                .unwrap_or_default(),
            cost_usd: history.cost_usd_samples.front().copied().flatten(),
            input_tokens: history.input_token_samples.front().copied().flatten(),
            output_tokens: history.output_token_samples.front().copied().flatten(),
            process_memory_mb: history.process_memory_samples.front().copied().flatten(),
            agent_memory_bytes: history.agent_memory_samples.front().copied().flatten(),
            subagent_hint: history.subagent_hint_samples.front().copied().unwrap_or(false),
        }
    }
}

/// Push one snapshot's fields into the corresponding `AnomalyHistory` deques
/// (newest-to-front, matching the existing per-tick push order in
/// `event_loop`). Used by the lazy replay path.
pub fn push_snapshot_into_history(history: &mut AnomalyHistory, snap: &AnomalyHistorySnapshot) {
    if let Some(id) = &snap.identity {
        history.identity_snapshots.push_front(id.clone());
    }
    if let Some(eh) = snap.error_hint {
        history.error_hints.push_front(eh);
    }
    history.cache_hit_ratios.push_front(snap.cache_hit_ratio);
    history.cache_drift_fires.push_front(snap.cache_drift_fire);
    history
        .cross_pane_edit_paths
        .push_front(snap.cross_pane_edit_paths.clone());
    history.cost_usd_samples.push_front(snap.cost_usd);
    history.input_token_samples.push_front(snap.input_tokens);
    history.output_token_samples.push_front(snap.output_tokens);
    history
        .process_memory_samples
        .push_front(snap.process_memory_mb);
    history
        .agent_memory_samples
        .push_front(snap.agent_memory_bytes);
    history.subagent_hint_samples.push_front(snap.subagent_hint);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_then_replay_roundtrips_basic_fields() {
        let mut h = AnomalyHistory::default();
        h.identity_snapshots
            .push_front(("Codex".to_string(), "/r".to_string()));
        h.error_hints.push_front(true);
        h.cache_hit_ratios.push_front(Some(0.5));
        h.cache_drift_fires.push_front(true);
        h.cross_pane_edit_paths
            .push_front(vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
        h.cost_usd_samples.push_front(Some(1.25));
        h.input_token_samples.push_front(Some(1000));
        h.output_token_samples.push_front(Some(500));
        h.process_memory_samples.push_front(Some(150.0));
        h.agent_memory_samples.push_front(Some(2_000_000));
        h.subagent_hint_samples.push_front(true);

        let snap = AnomalyHistorySnapshot::capture_tick(&h);

        let mut replayed = AnomalyHistory::default();
        push_snapshot_into_history(&mut replayed, &snap);

        assert_eq!(replayed.identity_snapshots.front(), h.identity_snapshots.front());
        assert_eq!(replayed.error_hints.front(), h.error_hints.front());
        assert_eq!(replayed.cache_hit_ratios.front(), h.cache_hit_ratios.front());
        assert_eq!(replayed.cache_drift_fires.front(), h.cache_drift_fires.front());
        assert_eq!(
            replayed.cross_pane_edit_paths.front(),
            h.cross_pane_edit_paths.front()
        );
        assert_eq!(replayed.cost_usd_samples.front(), h.cost_usd_samples.front());
        assert_eq!(replayed.input_token_samples.front(), h.input_token_samples.front());
        assert_eq!(replayed.output_token_samples.front(), h.output_token_samples.front());
        assert_eq!(
            replayed.process_memory_samples.front(),
            h.process_memory_samples.front()
        );
        assert_eq!(
            replayed.agent_memory_samples.front(),
            h.agent_memory_samples.front()
        );
        assert_eq!(
            replayed.subagent_hint_samples.front(),
            h.subagent_hint_samples.front()
        );
    }

    #[test]
    fn capture_handles_empty_history() {
        let h = AnomalyHistory::default();
        let snap = AnomalyHistorySnapshot::capture_tick(&h);
        assert_eq!(snap.identity, None);
        assert_eq!(snap.error_hint, None);
        assert_eq!(snap.cache_hit_ratio, None);
        assert!(!snap.cache_drift_fire);
        assert!(snap.cross_pane_edit_paths.is_empty());
        assert_eq!(snap.cost_usd, None);
        assert_eq!(snap.input_tokens, None);
        assert_eq!(snap.output_tokens, None);
        assert_eq!(snap.process_memory_mb, None);
        assert_eq!(snap.agent_memory_bytes, None);
        assert!(!snap.subagent_hint);
    }

    #[test]
    fn replay_skips_absent_optional_fields() {
        let snap = AnomalyHistorySnapshot {
            identity: None,
            error_hint: None,
            cache_hit_ratio: None,
            cache_drift_fire: false,
            cross_pane_edit_paths: Vec::new(),
            cost_usd: None,
            input_tokens: None,
            output_tokens: None,
            process_memory_mb: None,
            agent_memory_bytes: None,
            subagent_hint: false,
        };
        let mut h = AnomalyHistory::default();
        push_snapshot_into_history(&mut h, &snap);
        assert!(h.identity_snapshots.is_empty(), "no identity push when None");
        assert!(h.error_hints.is_empty(), "no error_hint push when None");
        // Non-Option fields always push:
        assert_eq!(h.cache_hit_ratios.front(), Some(&None));
        assert_eq!(h.cache_drift_fires.front(), Some(&false));
        assert_eq!(h.cross_pane_edit_paths.front(), Some(&Vec::<String>::new()));
        assert_eq!(h.subagent_hint_samples.front(), Some(&false));
    }
}
```

- [ ] **Step 3: Register module in `src/store/mod.rs`**

Run: `grep -n "^pub mod\|^pub use" src/store/mod.rs | head -10`

Add `pub mod anomaly_history;` in alphabetical position with the existing `pub mod` lines. Add `pub use anomaly_history::{AnomalyHistorySnapshot, push_snapshot_into_history};` near the existing `pub use` re-exports.

- [ ] **Step 4: Run tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --lib store::anomaly_history`

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/store/anomaly_history.rs src/store/mod.rs
git commit -m "phase 7 v3 (c): AnomalyHistorySnapshot + capture/replay helpers + 3 tests"
```

---

## Task 3: Add `SqliteAnomalySink` module + schemas

**Files:**

- Create: `src/store/anomaly_sink.rs`
- Modify: `src/store/sqlite.rs` (two new schema constants + execute_batch in `AuditDb::open`)
- Modify: `src/store/mod.rs` (register module + re-export)

- [ ] **Step 1: Add the two schema constants in `src/store/sqlite.rs`**

After the existing `COST_USAGE_SCHEMA` constant, add:

```rust
pub const ANOMALY_EVENTS_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS anomaly_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_unix_secs    INTEGER NOT NULL,
    pane_id         TEXT    NOT NULL,
    kind            TEXT    NOT NULL,
    confidence      TEXT    NOT NULL,
    severity        TEXT    NOT NULL,
    promoted        INTEGER NOT NULL,
    reason          TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_anomaly_events_ts
    ON anomaly_events(ts_unix_secs DESC);
CREATE INDEX IF NOT EXISTS idx_anomaly_events_pane_ts
    ON anomaly_events(pane_id, ts_unix_secs DESC);
";

pub const ANOMALY_HISTORY_SNAPSHOTS_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS anomaly_history_snapshots (
    pane_id                    TEXT    NOT NULL,
    tick_unix_secs             INTEGER NOT NULL,
    identity_provider          TEXT,
    identity_path              TEXT,
    error_hint                 INTEGER,
    cache_hit_ratio            REAL,
    cache_drift_fire           INTEGER,
    cross_pane_edit_paths_json TEXT,
    cost_usd                   REAL,
    input_tokens               INTEGER,
    output_tokens              INTEGER,
    process_memory_mb          REAL,
    agent_memory_bytes         INTEGER,
    subagent_hint              INTEGER,
    PRIMARY KEY (pane_id, tick_unix_secs)
);
CREATE INDEX IF NOT EXISTS idx_anomaly_history_pane_tick
    ON anomaly_history_snapshots(pane_id, tick_unix_secs DESC);
";
```

In `AuditDb::open`, after the existing `conn.execute_batch(COST_USAGE_SCHEMA)` line, add:

```rust
        conn.execute_batch(ANOMALY_EVENTS_SCHEMA)
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        conn.execute_batch(ANOMALY_HISTORY_SNAPSHOTS_SCHEMA)
            .map_err(|e| SqliteError::Query(e.to_string()))?;
```

- [ ] **Step 2: Add migration smoke test in `src/store/sqlite.rs`**

Append to the existing `#[cfg(test)] mod tests` block (or create one if absent):

```rust
#[test]
fn audit_db_open_creates_anomaly_tables() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("audit.db");
    let _db = AuditDb::open(&path).expect("open ok");
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table'
             AND name IN ('anomaly_events', 'anomaly_history_snapshots')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn audit_db_open_is_idempotent_for_anomaly_tables() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("audit.db");
    let _db1 = AuditDb::open(&path).expect("first open ok");
    let _db2 = AuditDb::open(&path).expect("second open ok (no error on reopen)");
}
```

If `tempfile` is not already a dev-dependency, check `Cargo.toml` first; if missing, this test should use whatever tmpdir helper the existing tests in this file use (`grep -n "tempfile\|TempDir" src/store/sqlite.rs Cargo.toml | head -5`).

- [ ] **Step 3a: Expose `current_unix_ms` as `pub`**

`src/app/event_loop.rs:662` defines `fn current_unix_ms() -> i64`. The new sink needs it. Change the signature in place:

```rust
pub fn current_unix_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

(One-word change: `fn` → `pub fn`. No callers break since `pub` is strictly looser visibility.)

- [ ] **Step 3: Create `src/store/anomaly_sink.rs`**

```rust
//! Phase 7 v3 (c) (v1.47.0): persistence sink for anomaly events and
//! per-pane AnomalyHistory deque snapshots. Wraps an `AuditDb`,
//! mirroring the existing `SqliteTokenUsageSink` and
//! `SqliteCostUsageSink` patterns. Schema migration runs inside
//! `AuditDb::open` and is idempotent.

use std::path::Path;

use crate::app::event_loop::current_unix_ms;
use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
use crate::domain::recommendation::Severity;
use crate::store::anomaly_history::AnomalyHistorySnapshot;
use crate::store::sqlite::{AuditDb, SqliteError};

pub struct SqliteAnomalySink {
    db: AuditDb,
}

impl SqliteAnomalySink {
    pub fn open(path: &Path) -> Result<Self, SqliteError> {
        Ok(Self {
            db: AuditDb::open(path)?,
        })
    }

    /// One-off INSERT into anomaly_events. Wraps its own transaction.
    /// For per-tick batching, prefer `with_transaction`.
    pub fn insert_anomaly_event(&self, event: &AnomalyEvent) -> Result<(), SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        conn.execute(
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
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(())
    }

    /// One-off UPSERT into anomaly_history_snapshots. Uses
    /// `INSERT OR REPLACE` keyed on `(pane_id, tick_unix_secs)`.
    pub fn upsert_anomaly_history_snapshot(
        &self,
        pane_id: &str,
        tick_unix_secs: u64,
        snap: &AnomalyHistorySnapshot,
    ) -> Result<(), SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let (identity_provider, identity_path) = match &snap.identity {
            Some((p, path)) => (Some(p.as_str()), Some(path.as_str())),
            None => (None, None),
        };
        let cross_paths_json = serde_json::to_string(&snap.cross_pane_edit_paths)
            .map_err(|e| SqliteError::Query(format!("json encode: {e}")))?;
        conn.execute(
            "INSERT OR REPLACE INTO anomaly_history_snapshots
             (pane_id, tick_unix_secs, identity_provider, identity_path,
              error_hint, cache_hit_ratio, cache_drift_fire,
              cross_pane_edit_paths_json, cost_usd, input_tokens, output_tokens,
              process_memory_mb, agent_memory_bytes, subagent_hint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                pane_id,
                tick_unix_secs as i64,
                identity_provider,
                identity_path,
                snap.error_hint.map(|b| b as i64),
                snap.cache_hit_ratio.map(|r| r as f64),
                snap.cache_drift_fire as i64,
                cross_paths_json,
                snap.cost_usd,
                snap.input_tokens.map(|n| n as i64),
                snap.output_tokens.map(|n| n as i64),
                snap.process_memory_mb,
                snap.agent_memory_bytes.map(|n| n as i64),
                snap.subagent_hint as i64,
            ],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(())
    }

    /// Run a closure inside a SQLite transaction, batching multiple
    /// writes into one fsync. Failure rolls back.
    pub fn with_transaction<F, R>(&self, f: F) -> Result<R, SqliteError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<R, SqliteError>,
    {
        let mut conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let result = f(&tx)?;
        tx.commit().map_err(|e| SqliteError::Query(e.to_string()))?;
        Ok(result)
    }

    pub fn fetch_recent_anomaly_events(&self, limit: usize) -> Vec<AnomalyEvent> {
        let conn = match self.db.connection().lock() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_anomaly_events lock: {e}");
                return Vec::new();
            }
        };
        let mut stmt = match conn.prepare(
            "SELECT ts_unix_secs, pane_id, kind, confidence, severity, promoted, reason
             FROM anomaly_events
             ORDER BY ts_unix_secs DESC, id DESC
             LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_anomaly_events prepare: {e}");
                return Vec::new();
            }
        };
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            let ts: i64 = row.get(0)?;
            let pane_id: String = row.get(1)?;
            let kind_str: String = row.get(2)?;
            let conf_str: String = row.get(3)?;
            let sev_str: String = row.get(4)?;
            let promoted_i: i64 = row.get(5)?;
            let reason: String = row.get(6)?;
            Ok((ts, pane_id, kind_str, conf_str, sev_str, promoted_i, reason))
        });
        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_anomaly_events query: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for row in rows.flatten() {
            let (ts, pane_id, kind_s, conf_s, sev_s, promoted_i, reason) = row;
            let Some(kind) = AnomalyKind::try_from_label(&kind_s) else {
                continue;
            };
            let Some(confidence) = AnomalyConfidence::try_from_label(&conf_s) else {
                continue;
            };
            let Some(severity) = Severity::try_from_label(&sev_s) else {
                continue;
            };
            out.push(AnomalyEvent {
                timestamp: ts as u64,
                pane_id,
                kind,
                confidence,
                severity,
                promoted: promoted_i != 0,
                reason,
            });
        }
        out
    }

    pub fn fetch_recent_history_snapshots(
        &self,
        pane_id: &str,
        window_polls: usize,
    ) -> Vec<(u64, AnomalyHistorySnapshot)> {
        let conn = match self.db.connection().lock() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_history_snapshots lock: {e}");
                return Vec::new();
            }
        };
        let mut stmt = match conn.prepare(
            "SELECT tick_unix_secs, identity_provider, identity_path, error_hint,
                    cache_hit_ratio, cache_drift_fire, cross_pane_edit_paths_json,
                    cost_usd, input_tokens, output_tokens, process_memory_mb,
                    agent_memory_bytes, subagent_hint
             FROM anomaly_history_snapshots
             WHERE pane_id = ?1
             ORDER BY tick_unix_secs DESC
             LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_history_snapshots prepare: {e}");
                return Vec::new();
            }
        };
        let rows = stmt.query_map(
            rusqlite::params![pane_id, window_polls as i64],
            |row| {
                let tick: i64 = row.get(0)?;
                let identity_provider: Option<String> = row.get(1)?;
                let identity_path: Option<String> = row.get(2)?;
                let error_hint: Option<i64> = row.get(3)?;
                let cache_hit_ratio: Option<f64> = row.get(4)?;
                let cache_drift_fire: i64 = row.get(5)?;
                let cross_paths_json: Option<String> = row.get(6)?;
                let cost_usd: Option<f64> = row.get(7)?;
                let input_tokens: Option<i64> = row.get(8)?;
                let output_tokens: Option<i64> = row.get(9)?;
                let process_memory_mb: Option<f64> = row.get(10)?;
                let agent_memory_bytes: Option<i64> = row.get(11)?;
                let subagent_hint: i64 = row.get(12)?;
                Ok((
                    tick,
                    identity_provider,
                    identity_path,
                    error_hint,
                    cache_hit_ratio,
                    cache_drift_fire,
                    cross_paths_json,
                    cost_usd,
                    input_tokens,
                    output_tokens,
                    process_memory_mb,
                    agent_memory_bytes,
                    subagent_hint,
                ))
            },
        );
        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                eprintln!("anomaly_sink fetch_recent_history_snapshots query: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for row in rows.flatten() {
            let (
                tick,
                provider,
                path,
                error_hint,
                cache_hit_ratio,
                cache_drift_fire,
                cross_paths_json,
                cost_usd,
                input_tokens,
                output_tokens,
                process_memory_mb,
                agent_memory_bytes,
                subagent_hint,
            ) = row;
            let identity = match (provider, path) {
                (Some(p), Some(pa)) => Some((p, pa)),
                _ => None,
            };
            let cross_pane_edit_paths: Vec<String> = cross_paths_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            out.push((
                tick as u64,
                AnomalyHistorySnapshot {
                    identity,
                    error_hint: error_hint.map(|i| i != 0),
                    cache_hit_ratio: cache_hit_ratio.map(|r| r as f32),
                    cache_drift_fire: cache_drift_fire != 0,
                    cross_pane_edit_paths,
                    cost_usd,
                    input_tokens: input_tokens.map(|i| i as u64),
                    output_tokens: output_tokens.map(|i| i as u64),
                    process_memory_mb,
                    agent_memory_bytes: agent_memory_bytes.map(|i| i as u64),
                    subagent_hint: subagent_hint != 0,
                },
            ));
        }
        out
    }

    /// Time-based purge (`retention_days`) + 100K-row hard cap for
    /// `anomaly_events`; 4×window auto-purge for `anomaly_history_snapshots`.
    pub fn run_retention(
        &self,
        retention_days: u64,
        window_polls: usize,
        tick_seconds: u64,
    ) -> Result<(), SqliteError> {
        let conn = self
            .db
            .connection()
            .lock()
            .map_err(|e| SqliteError::Query(e.to_string()))?;
        let now_secs = (current_unix_ms() / 1000) as u64;

        let events_threshold = now_secs.saturating_sub(retention_days * 86_400);
        conn.execute(
            "DELETE FROM anomaly_events WHERE ts_unix_secs < ?1",
            rusqlite::params![events_threshold as i64],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;

        conn.execute(
            "DELETE FROM anomaly_events
             WHERE id NOT IN (
                 SELECT id FROM anomaly_events ORDER BY id DESC LIMIT 100000
             )",
            [],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;

        let snapshots_threshold = now_secs
            .saturating_sub((window_polls as u64) * tick_seconds * 4);
        conn.execute(
            "DELETE FROM anomaly_history_snapshots WHERE tick_unix_secs < ?1",
            rusqlite::params![snapshots_threshold as i64],
        )
        .map_err(|e| SqliteError::Query(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
    use crate::domain::recommendation::Severity;

    fn temp_sink() -> (tempfile::TempDir, SqliteAnomalySink) {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("audit.db");
        let sink = SqliteAnomalySink::open(&path).unwrap();
        (tmp, sink)
    }

    fn fixture_event(ts: u64) -> AnomalyEvent {
        AnomalyEvent {
            timestamp: ts,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: format!("cost_usd at ts={ts}"),
        }
    }

    fn fixture_snapshot() -> AnomalyHistorySnapshot {
        AnomalyHistorySnapshot {
            identity: Some(("Codex".to_string(), "/r".to_string())),
            error_hint: Some(true),
            cache_hit_ratio: Some(0.42),
            cache_drift_fire: true,
            cross_pane_edit_paths: vec!["src/a.rs".to_string()],
            cost_usd: Some(2.5),
            input_tokens: Some(1234),
            output_tokens: Some(99),
            process_memory_mb: Some(128.5),
            agent_memory_bytes: Some(1_000_000),
            subagent_hint: false,
        }
    }

    #[test]
    fn insert_and_fetch_anomaly_event_roundtrip() {
        let (_tmp, sink) = temp_sink();
        let event = fixture_event(1_700_000_000);
        sink.insert_anomaly_event(&event).unwrap();
        let fetched = sink.fetch_recent_anomaly_events(10);
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0], event);
    }

    #[test]
    fn fetch_recent_anomaly_events_orders_newest_first() {
        let (_tmp, sink) = temp_sink();
        for ts in [1, 3, 2u64] {
            sink.insert_anomaly_event(&fixture_event(ts)).unwrap();
        }
        let events = sink.fetch_recent_anomaly_events(10);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].timestamp, 3);
        assert_eq!(events[1].timestamp, 2);
        assert_eq!(events[2].timestamp, 1);
    }

    #[test]
    fn fetch_recent_anomaly_events_respects_limit() {
        let (_tmp, sink) = temp_sink();
        for ts in 0..5 {
            sink.insert_anomaly_event(&fixture_event(ts)).unwrap();
        }
        let events = sink.fetch_recent_anomaly_events(2);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn upsert_history_snapshot_replaces_on_conflict() {
        let (_tmp, sink) = temp_sink();
        let mut snap = fixture_snapshot();
        sink.upsert_anomaly_history_snapshot("%1", 1_700_000_000, &snap)
            .unwrap();
        snap.cost_usd = Some(99.99);
        sink.upsert_anomaly_history_snapshot("%1", 1_700_000_000, &snap)
            .unwrap();
        let snaps = sink.fetch_recent_history_snapshots("%1", 10);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].1.cost_usd, Some(99.99));
    }

    #[test]
    fn fetch_recent_history_snapshots_filters_by_pane_id() {
        let (_tmp, sink) = temp_sink();
        let snap = fixture_snapshot();
        sink.upsert_anomaly_history_snapshot("%1", 100, &snap).unwrap();
        sink.upsert_anomaly_history_snapshot("%2", 200, &snap).unwrap();
        let only_pane1 = sink.fetch_recent_history_snapshots("%1", 10);
        assert_eq!(only_pane1.len(), 1);
        assert_eq!(only_pane1[0].0, 100);
    }

    #[test]
    fn snapshot_handles_absent_optional_fields() {
        let (_tmp, sink) = temp_sink();
        let snap = AnomalyHistorySnapshot {
            identity: None,
            error_hint: None,
            cache_hit_ratio: None,
            cache_drift_fire: false,
            cross_pane_edit_paths: Vec::new(),
            cost_usd: None,
            input_tokens: None,
            output_tokens: None,
            process_memory_mb: None,
            agent_memory_bytes: None,
            subagent_hint: false,
        };
        sink.upsert_anomaly_history_snapshot("%1", 100, &snap).unwrap();
        let fetched = sink.fetch_recent_history_snapshots("%1", 1);
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].1, snap);
    }

    #[test]
    fn run_retention_drops_events_older_than_days() {
        let (_tmp, sink) = temp_sink();
        // Insert one ancient event (timestamp 0) and one recent (timestamp = now).
        let now_secs = (current_unix_ms() / 1000) as u64;
        sink.insert_anomaly_event(&fixture_event(0)).unwrap();
        sink.insert_anomaly_event(&fixture_event(now_secs)).unwrap();
        sink.run_retention(30, 20, 2).unwrap();
        let events = sink.fetch_recent_anomaly_events(10);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].timestamp, now_secs);
    }

    #[test]
    fn run_retention_drops_old_history_snapshots() {
        let (_tmp, sink) = temp_sink();
        let now_secs = (current_unix_ms() / 1000) as u64;
        let snap = fixture_snapshot();
        sink.upsert_anomaly_history_snapshot("%1", 0, &snap).unwrap();
        sink.upsert_anomaly_history_snapshot("%1", now_secs, &snap).unwrap();
        sink.run_retention(30, 20, 2).unwrap();
        let fetched = sink.fetch_recent_history_snapshots("%1", 10);
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].0, now_secs);
    }

    #[test]
    fn with_transaction_commits_on_ok() {
        let (_tmp, sink) = temp_sink();
        sink.with_transaction(|tx| {
            tx.execute(
                "INSERT INTO anomaly_events
                 (ts_unix_secs, pane_id, kind, confidence, severity, promoted, reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    1i64, "%1", "CostSlope", "high", "warning", 1i64, "tx test"
                ],
            )
            .map_err(|e| SqliteError::Query(e.to_string()))?;
            Ok(())
        })
        .unwrap();
        let fetched = sink.fetch_recent_anomaly_events(10);
        assert_eq!(fetched.len(), 1);
    }
}
```

- [ ] **Step 4: Register module in `src/store/mod.rs`**

Add `pub mod anomaly_sink;` (alphabetical) and `pub use anomaly_sink::SqliteAnomalySink;` near the existing re-exports.

- [ ] **Step 5: Add `tempfile` to dev-dependencies if missing**

Run: `grep "tempfile" Cargo.toml`. If absent, add under `[dev-dependencies]`:

```toml
tempfile = "3"
```

(If already present, no edit.)

- [ ] **Step 6: Run tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --lib store::anomaly_sink store::sqlite::tests::audit_db_open_creates_anomaly_tables store::sqlite::tests::audit_db_open_is_idempotent_for_anomaly_tables`

Expected: SqliteAnomalySink module's 9 tests + 2 schema tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/store/anomaly_sink.rs src/store/sqlite.rs src/store/mod.rs Cargo.toml
git commit -m "phase 7 v3 (c): SqliteAnomalySink + two schema tables (anomaly_events, anomaly_history_snapshots) + 11 tests"
```

---

## Task 4: Add `AnomalyConfig.retention_days` field

**Files:**

- Modify: `src/app/config.rs` (add field + default + test)

- [ ] **Step 1: Add the field to `AnomalyConfig` struct**

Locate `pub struct AnomalyConfig` (around line 304). After the existing fields (after `memory_growth_mb` and the `promote: AnomalyPromoteConfig` field), add:

```rust
    /// Phase 7 v3 (c) (v1.47.0): retention horizon for `anomaly_events`
    /// rows. Older rows are deleted on Qmonster startup. Default 30
    /// days. A 100K-row hard cap also applies.
    pub retention_days: u64,
```

In `impl Default for AnomalyConfig`, add inside the `Self { ... }` literal (after the other defaults):

```rust
            retention_days: 30,
```

- [ ] **Step 2: Add tests**

In the existing `#[cfg(test)] mod tests` block of `src/app/config.rs`, append:

```rust
#[test]
fn anomaly_config_default_retention_is_30_days() {
    let c = AnomalyConfig::default();
    assert_eq!(c.retention_days, 30);
}

#[test]
fn anomaly_config_omitting_retention_uses_default() {
    let toml = r#"
        [anomaly]
        enabled = true
    "#;
    let cfg: Config = toml::from_str(toml).expect("parse");
    assert_eq!(cfg.anomaly.retention_days, 30);
}

#[test]
fn anomaly_config_retention_days_round_trips_via_toml() {
    let toml = r#"
        [anomaly]
        enabled = true
        retention_days = 7
    "#;
    let cfg: Config = toml::from_str(toml).expect("parse");
    assert_eq!(cfg.anomaly.retention_days, 7);
}
```

- [ ] **Step 3: Run tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --lib app::config::tests::anomaly_config_default_retention_is_30_days app::config::tests::anomaly_config_omitting_retention_uses_default app::config::tests::anomaly_config_retention_days_round_trips_via_toml`

Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/app/config.rs
git commit -m "phase 7 v3 (c): AnomalyConfig.retention_days = 30 default + 3 tests"
```

---

## Task 5: Extend `Context` with `anomaly_sink` + `with_anomaly_sink` builder

**Files:**

- Modify: `src/app/bootstrap.rs` (add fields + builder method)

- [ ] **Step 1: Add fields to `Context<P, N>`**

In `src/app/bootstrap.rs`, locate the `pub struct Context<P: PaneSource, N: NotifyBackend>` definition. After the existing `cost_usage_sink: Option<SqliteCostUsageSink>` field, add:

```rust
    /// Phase 7 v3 (c) (v1.47.0): persistence sink for anomaly events
    /// and AnomalyHistory deque snapshots. None when persistence is
    /// disabled (test fixtures, operators without --config-with-db).
    pub anomaly_sink: Option<crate::store::SqliteAnomalySink>,
    /// Phase 7 v3 (c): per-session set of pane_ids whose AnomalyHistory
    /// has been replay-checked already. Prevents repeating the disk
    /// query every tick.
    pub anomaly_history_replayed: std::collections::HashSet<String>,
```

- [ ] **Step 2: Initialize fields in `Context::new`**

In `Context::new`, in the `Self { ... }` literal, add (alphabetical-ish position alongside the other defaults):

```rust
            anomaly_sink: None,
            anomaly_history_replayed: std::collections::HashSet::new(),
```

- [ ] **Step 3: Add `with_anomaly_sink` builder**

Find the existing `with_token_usage_sink` and `with_cost_usage_sink` builders. Add a parallel one:

```rust
    pub fn with_anomaly_sink(mut self, sink: crate::store::SqliteAnomalySink) -> Self {
        let tick_seconds = (self.config.tmux.poll_interval_ms / 1000).max(1);
        if let Err(e) = sink.run_retention(
            self.config.anomaly.retention_days,
            self.config.anomaly.window_polls,
            tick_seconds,
        ) {
            eprintln!("anomaly_sink retention purge failed at startup: {e}");
        }
        self.anomaly_sink = Some(sink);
        self
    }
```

- [ ] **Step 4: Add a smoke test**

Inspect existing tests in this file or in `src/store/anomaly_sink.rs` to see whether a `Context` test fixture pattern exists. If yes, add a smoke test in the appropriate test module:

```rust
#[test]
fn with_anomaly_sink_plumbs_and_runs_retention() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("audit.db");
    let sink = crate::store::SqliteAnomalySink::open(&path).unwrap();
    // Build a minimal Context. Reuse an existing fixture helper if
    // available; if not, this smoke test can be skipped (the integration
    // test in Task 12 covers the full plumbing).
    // ...
    // Verify ctx.anomaly_sink.is_some() after the builder runs.
}
```

If no Context fixture exists in this file, skip Step 4 — Task 12's integration test will cover the wiring end-to-end.

- [ ] **Step 5: Run tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --lib`

Expected: clean; existing test counts preserved (Task 4's 3 new tests already in baseline).

- [ ] **Step 6: Commit**

```bash
git add src/app/bootstrap.rs
git commit -m "phase 7 v3 (c): Context.anomaly_sink + anomaly_history_replayed + with_anomaly_sink builder"
```

---

## Task 6: Wire `event_loop` write site (per-tick transaction)

**Files:**

- Modify: `src/app/event_loop.rs` (per-tick transaction + history snapshot upsert + anomaly event INSERT)

- [ ] **Step 1: Locate the existing v1.46.0 ring push and AnomalyHistory push**

Run: `grep -n "ctx.anomaly_events_ring.push\|history.identity_snapshots.push\|trim(.*window_polls)" src/app/event_loop.rs`

Note the line numbers of:

- The AnomalyHistory push site (around line 470-510 — the per-tick observation push that builds and pushes onto history.identity_snapshots, error_hints, etc., then calls history.trim).
- The ring buffer push site (around line 530-555 from v1.46.0 — the `for sig in &anomalies` loop that builds `AnomalyEvent` and calls `ctx.anomaly_events_ring.push`).

- [ ] **Step 2: Add the per-tick transaction wrapper**

The simplest approach: don't change the in-memory push order. Add the persistence call AFTER the AnomalyHistory push (so we have the history values to snapshot) AND AFTER the ring push (so we have the events to persist). Wrap both DB writes in a single transaction.

Around the ring push site (after `ctx.anomaly_events_ring.push(event)` for all signals, before `deliver_effects`), add:

```rust
        // Phase 7 v3 (c) (v1.47.0): persist visible anomalies + history
        // snapshot for this pane this tick. Single transaction for fsync
        // batching. Failures degrade gracefully (logged, do not block).
        if let Some(anomaly_sink) = ctx.anomaly_sink.as_ref() {
            let snap = crate::store::AnomalyHistorySnapshot::capture_tick(&history);
            let result = anomaly_sink.with_transaction(|tx| {
                for sig_event in &events_pushed_this_tick {
                    tx.execute(
                        "INSERT INTO anomaly_events
                         (ts_unix_secs, pane_id, kind, confidence, severity, promoted, reason)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        rusqlite::params![
                            sig_event.timestamp as i64,
                            sig_event.pane_id,
                            sig_event.kind.label(),
                            sig_event.confidence.label(),
                            sig_event.severity.label(),
                            sig_event.promoted as i64,
                            sig_event.reason,
                        ],
                    )
                    .map_err(|e| crate::store::sqlite::SqliteError::Query(e.to_string()))?;
                }
                let cross_paths_json =
                    serde_json::to_string(&snap.cross_pane_edit_paths)
                        .map_err(|e| crate::store::sqlite::SqliteError::Query(format!("json: {e}")))?;
                let (id_provider, id_path) = match &snap.identity {
                    Some((p, path)) => (Some(p.as_str()), Some(path.as_str())),
                    None => (None, None),
                };
                tx.execute(
                    "INSERT OR REPLACE INTO anomaly_history_snapshots
                     (pane_id, tick_unix_secs, identity_provider, identity_path,
                      error_hint, cache_hit_ratio, cache_drift_fire,
                      cross_pane_edit_paths_json, cost_usd, input_tokens, output_tokens,
                      process_memory_mb, agent_memory_bytes, subagent_hint)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    rusqlite::params![
                        pane.pane_id,
                        now_secs as i64,
                        id_provider,
                        id_path,
                        snap.error_hint.map(|b| b as i64),
                        snap.cache_hit_ratio.map(|r| r as f64),
                        snap.cache_drift_fire as i64,
                        cross_paths_json,
                        snap.cost_usd,
                        snap.input_tokens.map(|n| n as i64),
                        snap.output_tokens.map(|n| n as i64),
                        snap.process_memory_mb,
                        snap.agent_memory_bytes.map(|n| n as i64),
                        snap.subagent_hint as i64,
                    ],
                )
                .map_err(|e| crate::store::sqlite::SqliteError::Query(e.to_string()))?;
                Ok(())
            });
            if let Err(e) = result {
                eprintln!("anomaly_sink persistence failed (pane={}): {e}", pane.pane_id);
            }
        }
```

`events_pushed_this_tick` is a new local `Vec<AnomalyEvent>` that the ring-push loop above accumulates alongside calling `ctx.anomaly_events_ring.push(...)`. The cleanest pattern: just before the existing push loop, declare `let mut events_pushed_this_tick: Vec<AnomalyEvent> = Vec::new();` and inside the loop push into both the ring and this Vec.

Locating `now_secs`: it's already computed in the v1.46.0 push block. The new persistence block reads it after that block runs.

`history`: in scope as the per-pane mutable reference used by the AnomalyHistory push. Verify it's still in scope at the ring-push site (it should be — both happen within the same `for pane in panes` body).

- [ ] **Step 3: Verify the existing v1.46 ring-push loop captures events**

Locate the v1.46 ring-push loop. Adjust:

```rust
let mut events_pushed_this_tick: Vec<AnomalyEvent> = Vec::new();
for sig in &anomalies {
    let promoted_bool =
        sig.confidence >= gates.promote_min_confidence(sig.kind);
    let reason = sig.evidence.first()
        .map(|e| format!("{}: {} → {}", e.metric_name, e.before, e.after))
        .unwrap_or_default();
    let event = crate::domain::anomaly::AnomalyEvent {
        timestamp: now_secs,
        pane_id: pane.pane_id.clone(),
        kind: sig.kind,
        confidence: sig.confidence,
        severity: sig.severity,
        promoted: promoted_bool,
        reason,
    };
    ctx.anomaly_events_ring.push(event.clone());
    events_pushed_this_tick.push(event);
}
```

(The clone is unavoidable: the ring takes ownership, the Vec needs the value too.)

- [ ] **Step 4: Run lib + integration tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --all-targets`

Expected: existing tests pass; no new test failures. The persistence path is exercised end-to-end in Task 12's integration test.

- [ ] **Step 5: Commit**

```bash
git add src/app/event_loop.rs
git commit -m "phase 7 v3 (c): event_loop per-tick transaction (anomaly_events INSERT + snapshot UPSERT)"
```

---

## Task 7: Wire `event_loop` lazy AnomalyHistory replay

**Files:**

- Modify: `src/app/event_loop.rs` (replay gate before AnomalyHistory push)

- [ ] **Step 1: Locate the AnomalyHistory push site**

Run: `grep -n "history.identity_snapshots.push_front\|history.trim" src/app/event_loop.rs`

Identify the per-tick block that pushes onto the live `AnomalyHistory`. The replay gate must run BEFORE that push so the deques are seeded first.

- [ ] **Step 2: Add the replay gate**

Just before the per-pane AnomalyHistory push site, add:

```rust
        // Phase 7 v3 (c) (v1.47.0): lazy hydrate AnomalyHistory deques
        // from persisted snapshots on first observation per pane this
        // session. Staleness gate: skip if newest snapshot > 2× window.
        if !ctx.anomaly_history_replayed.contains(&pane.pane_id) {
            if let Some(anomaly_sink) = ctx.anomaly_sink.as_ref() {
                let window_polls = gates.anomaly_window_polls;
                let snapshots =
                    anomaly_sink.fetch_recent_history_snapshots(&pane.pane_id, window_polls);
                let tick_seconds = (ctx.config.tmux.poll_interval_ms / 1000).max(1);
                let staleness_threshold =
                    (window_polls as u64) * tick_seconds * 2;
                let now_secs = (current_unix_ms() / 1000) as u64;
                if let Some((newest_ts, _)) = snapshots.first() {
                    if now_secs.saturating_sub(*newest_ts) <= staleness_threshold {
                        let history = ctx
                            .anomaly_history
                            .entry(pane.pane_id.clone())
                            .or_default();
                        // snapshots is newest-first; replay oldest-first so
                        // deque ordering matches live push semantics:
                        for (_, snap) in snapshots.iter().rev() {
                            crate::store::push_snapshot_into_history(history, snap);
                        }
                        history.trim(window_polls);
                    }
                }
            }
            ctx.anomaly_history_replayed.insert(pane.pane_id.clone());
        }
```

The `history` mutable reference inside the replay block uses `entry().or_default()` to create the AnomalyHistory if it didn't exist — same pattern as the existing live push.

- [ ] **Step 3: Run lib + integration tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --all-targets`

Expected: existing tests pass; no new test failures. End-to-end replay is exercised in Task 12.

- [ ] **Step 4: Commit**

```bash
git add src/app/event_loop.rs
git commit -m "phase 7 v3 (c): event_loop lazy AnomalyHistory replay (staleness-gated, per-pane per-session)"
```

---

## Task 8: Add `AnomalyOverlayView` enum + state extensions

**Files:**

- Modify: `src/ui/anomaly_overlay.rs` (`AnomalyOverlayView` enum + `view`/`history_cache` fields + `toggle_view` method + tests)

- [ ] **Step 1: Add the enum and extend state**

In `src/ui/anomaly_overlay.rs`, add near the top (after the imports, before `pub struct AnomalyOverlay`):

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

Update the `AnomalyOverlay` struct definition. The current state is:

```rust
#[derive(Debug, Clone)]
pub struct AnomalyOverlay {
    open: bool,
    scroll: u16,
}
```

Replace with:

```rust
#[derive(Debug, Clone)]
pub struct AnomalyOverlay {
    open: bool,
    scroll: u16,
    view: AnomalyOverlayView,
    history_cache: Vec<crate::domain::anomaly::AnomalyEvent>,
}

impl Default for AnomalyOverlay {
    fn default() -> Self {
        Self {
            open: false,
            scroll: 0,
            view: AnomalyOverlayView::Ring,
            history_cache: Vec::new(),
        }
    }
}
```

(The `Default` derive moves to a manual impl because `Vec::new()` initialization is more explicit and matches the existing T4 pattern from v1.46.0's AnomalyEventsRing.)

- [ ] **Step 2: Add accessor + toggle methods**

Inside `impl AnomalyOverlay`, after the existing `scroll_up` method (or at the end of the impl), add:

```rust
    pub fn view(&self) -> AnomalyOverlayView {
        self.view
    }

    /// Toggle between Ring and History views. When called with a
    /// non-empty `history_cache`, switch to History; otherwise switch
    /// to Ring (and clear the cache). The dispatch layer
    /// (`tui_loop.rs`) is responsible for fetching the cache from
    /// the persistence sink and passing it in.
    pub fn toggle_view(&mut self, history_cache: Vec<crate::domain::anomaly::AnomalyEvent>) {
        match self.view {
            AnomalyOverlayView::Ring => {
                self.view = AnomalyOverlayView::History;
                self.history_cache = history_cache;
                self.scroll = 0;
            }
            AnomalyOverlayView::History => {
                self.view = AnomalyOverlayView::Ring;
                self.history_cache.clear();
                self.scroll = 0;
            }
        }
    }

    pub fn history_cache(&self) -> &[crate::domain::anomaly::AnomalyEvent] {
        &self.history_cache
    }
```

Also extend `close()` to clear cache and reset view (defensive — overlay reopens cleanly):

```rust
    pub fn close(&mut self) {
        self.open = false;
        self.scroll = 0;
        self.view = AnomalyOverlayView::Ring;
        self.history_cache.clear();
    }
```

(If the existing `close` already exists, modify it in place; otherwise add. The current v1.46 `close()` body sets `self.open = false; self.scroll = 0;`.)

- [ ] **Step 3: Add tests**

In the existing `#[cfg(test)] mod tests` block, append:

```rust
#[test]
fn overlay_starts_in_ring_view() {
    let o = AnomalyOverlay::new();
    assert_eq!(o.view(), AnomalyOverlayView::Ring);
    assert!(o.history_cache().is_empty());
}

#[test]
fn toggle_view_to_history_with_cache() {
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
    use crate::domain::recommendation::Severity;
    let mut o = AnomalyOverlay::new();
    o.open();
    o.scroll_down(5);
    let event = AnomalyEvent {
        timestamp: 1,
        pane_id: "%1".to_string(),
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::High,
        severity: Severity::Warning,
        promoted: true,
        reason: "x".to_string(),
    };
    o.toggle_view(vec![event.clone()]);
    assert_eq!(o.view(), AnomalyOverlayView::History);
    assert_eq!(o.history_cache().len(), 1);
    assert_eq!(o.scroll(), 0); // toggle resets scroll
}

#[test]
fn toggle_view_back_to_ring_clears_cache() {
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
    use crate::domain::recommendation::Severity;
    let mut o = AnomalyOverlay::new();
    o.open();
    let event = AnomalyEvent {
        timestamp: 1,
        pane_id: "%1".to_string(),
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::High,
        severity: Severity::Warning,
        promoted: true,
        reason: "x".to_string(),
    };
    o.toggle_view(vec![event]);
    assert_eq!(o.view(), AnomalyOverlayView::History);
    o.toggle_view(Vec::new());
    assert_eq!(o.view(), AnomalyOverlayView::Ring);
    assert!(o.history_cache().is_empty());
}

#[test]
fn close_resets_view_and_clears_cache() {
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
    use crate::domain::recommendation::Severity;
    let mut o = AnomalyOverlay::new();
    o.open();
    o.toggle_view(vec![AnomalyEvent {
        timestamp: 1,
        pane_id: "%1".to_string(),
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::High,
        severity: Severity::Warning,
        promoted: true,
        reason: "x".to_string(),
    }]);
    o.close();
    assert!(!o.is_open());
    assert_eq!(o.view(), AnomalyOverlayView::Ring);
    assert!(o.history_cache().is_empty());
}
```

- [ ] **Step 4: Run targeted tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --lib ui::anomaly_overlay`

Expected: 4 new tests pass alongside the v1.46 state tests.

- [ ] **Step 5: Commit**

```bash
git add src/ui/anomaly_overlay.rs
git commit -m "phase 7 v3 (c): AnomalyOverlay gains view enum + history_cache + toggle_view + 4 tests"
```

---

## Task 9: Render branch for History view

**Files:**

- Modify: `src/ui/anomaly_overlay.rs` (render method respects `view`)

- [ ] **Step 1: Update the render method**

The current `AnomalyOverlay::render(frame, area, ring)` in `src/ui/anomaly_overlay.rs` iterates `ring.iter().rev()` and renders. Update to:

```rust
    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        ring: &crate::app::anomaly_events_ring::AnomalyEventsRing,
    ) {
        let popup_area = centered_rect(80, 70, area);
        frame.render_widget(Clear, popup_area);

        let (title, total_count) = match self.view {
            AnomalyOverlayView::Ring => (
                format!(
                    " ANOMALY EVENTS (last {} — newest first) [h: history] [n to close] ",
                    ring.len()
                ),
                ring.len(),
            ),
            AnomalyOverlayView::History => (
                format!(
                    " ANOMALY EVENTS [history view, last {}] [h: ring] [n to close] ",
                    self.history_cache.len()
                ),
                self.history_cache.len(),
            ),
        };
        let block = Block::default().borders(Borders::ALL).title(title);

        if total_count == 0 {
            let body_lines = match self.view {
                AnomalyOverlayView::Ring => vec![
                    Line::from("No anomaly events recorded this session."),
                    Line::from(""),
                    Line::from(
                        "Anomalies are recorded once detection is enabled in Settings: [anomaly] enabled = true.",
                    ),
                ],
                AnomalyOverlayView::History => vec![
                    Line::from("No persisted anomaly events."),
                    Line::from(""),
                    Line::from(
                        "Enable [anomaly] in Settings to start recording. Old rows are pruned by [anomaly] retention_days (default 30).",
                    ),
                ],
            };
            let body = Paragraph::new(body_lines).block(block).wrap(Wrap { trim: false });
            frame.render_widget(body, popup_area);
            return;
        }

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let header = format!(
            "{:<8}  {:<6}  {:<22}  {:<6}  {:<8}  {}",
            "Time", "Pane", "Kind", "Conf", "Promoted", "Reason"
        );

        let visible_rows = inner.height.saturating_sub(1) as usize;
        let items: Vec<ListItem> = match self.view {
            AnomalyOverlayView::Ring => ring
                .iter()
                .rev()
                .skip(self.scroll as usize)
                .take(visible_rows)
                .map(|e| ListItem::new(format_event_row(e, inner.width as usize)))
                .collect(),
            AnomalyOverlayView::History => self
                .history_cache
                .iter()
                .skip(self.scroll as usize)
                .take(visible_rows)
                .map(|e| ListItem::new(format_event_row(e, inner.width as usize)))
                .collect(),
        };

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        frame.render_widget(
            Paragraph::new(header).style(Style::default().add_modifier(Modifier::BOLD)),
            layout[0],
        );
        frame.render_widget(List::new(items), layout[1]);
    }
```

(Adjust the method body to fit the existing render code's exact shape — this skeleton mirrors v1.46.0's pattern with the new `match self.view` branches.)

- [ ] **Step 2: Add render tests**

Append to `mod tests`:

```rust
#[test]
fn render_history_view_shows_history_title_indicator() {
    use crate::app::anomaly_events_ring::AnomalyEventsRing;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
    use crate::domain::recommendation::Severity;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut overlay = AnomalyOverlay::new();
    overlay.open();
    overlay.toggle_view(vec![AnomalyEvent {
        timestamp: 1_700_000_000,
        pane_id: "%1".to_string(),
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::High,
        severity: Severity::Warning,
        promoted: true,
        reason: "from disk".to_string(),
    }]);
    let ring = AnomalyEventsRing::new();
    terminal.draw(|f| overlay.render(f, f.size(), &ring)).unwrap();
    let rendered = buf_to_string(terminal.backend().buffer());
    assert!(rendered.contains("history view"), "title missing 'history view': {rendered}");
    assert!(rendered.contains("[h: ring]"), "ring-toggle hint missing: {rendered}");
    assert!(rendered.contains("from disk"), "history row content missing");
}

#[test]
fn render_history_view_empty_shows_help_hint() {
    use crate::app::anomaly_events_ring::AnomalyEventsRing;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut overlay = AnomalyOverlay::new();
    overlay.open();
    overlay.toggle_view(Vec::new());
    let ring = AnomalyEventsRing::new();
    terminal.draw(|f| overlay.render(f, f.size(), &ring)).unwrap();
    let rendered = buf_to_string(terminal.backend().buffer());
    assert!(rendered.contains("No persisted anomaly events"), "history empty body missing");
    assert!(rendered.contains("retention_days"), "history empty hint missing retention reference");
}

#[test]
fn render_ring_view_still_uses_h_history_hint() {
    use crate::app::anomaly_events_ring::AnomalyEventsRing;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
    use crate::domain::recommendation::Severity;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut overlay = AnomalyOverlay::new();
    overlay.open();
    let mut ring = AnomalyEventsRing::new();
    ring.push(AnomalyEvent {
        timestamp: 1_700_000_000,
        pane_id: "%1".to_string(),
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::High,
        severity: Severity::Warning,
        promoted: true,
        reason: "ring view".to_string(),
    });
    terminal.draw(|f| overlay.render(f, f.size(), &ring)).unwrap();
    let rendered = buf_to_string(terminal.backend().buffer());
    assert!(rendered.contains("[h: history]"), "ring view title should advertise history toggle");
}
```

(`buf_to_string` already exists in this file from v1.46.0's render tests — no new helper needed.)

- [ ] **Step 3: Run targeted tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --lib ui::anomaly_overlay`

Expected: 3 new tests pass alongside existing ones.

- [ ] **Step 4: Commit**

```bash
git add src/ui/anomaly_overlay.rs
git commit -m "phase 7 v3 (c): AnomalyOverlay render branch for History view + 3 tests"
```

---

## Task 10: Wire `'h'` key + sink construction in `tui_loop.rs`

**Files:**

- Modify: `src/app/tui_loop.rs` (`'h'` key arm; sink wired into Context at startup)

- [ ] **Step 1: Plug the SqliteAnomalySink at startup**

Find the existing Context construction in `src/app/tui_loop.rs`. Search for `with_token_usage_sink` or `with_cost_usage_sink`:

```bash
grep -n "with_token_usage_sink\|with_cost_usage_sink\|Context::new" src/app/tui_loop.rs
```

Adjacent to those builder calls, add:

```rust
let ctx = ctx
    .with_anomaly_sink(crate::store::SqliteAnomalySink::open(&audit_db_path)?);
```

`audit_db_path` is the same path used by the existing token/cost sink builders. Reuse whatever local variable they use (verify by reading the surrounding lines).

If the token/cost sink construction is wrapped in a `match` or `if let` for opt-in behavior, the anomaly sink should follow the same gating pattern.

- [ ] **Step 2: Add the `'h'` key arm in the anomaly-overlay-open block**

Locate the existing block (T10 of the v1.46.0 plan landed it):

```rust
if anomaly_overlay.is_open() {
    let _ = crate::app::anomaly_overlay::handle_anomaly_overlay_key(
        &mut anomaly_overlay,
        ctx.anomaly_events_ring.len(),
        k.code,
    );
    continue;
}
```

Replace with:

```rust
if anomaly_overlay.is_open() {
    if matches!(k.code, KeyCode::Char('h')) {
        match anomaly_overlay.view() {
            crate::ui::anomaly_overlay::AnomalyOverlayView::Ring => {
                let events = ctx
                    .anomaly_sink
                    .as_ref()
                    .map(|sink| sink.fetch_recent_anomaly_events(200))
                    .unwrap_or_default();
                anomaly_overlay.toggle_view(events);
            }
            crate::ui::anomaly_overlay::AnomalyOverlayView::History => {
                anomaly_overlay.toggle_view(Vec::new());
            }
        }
        continue;
    }
    let _ = crate::app::anomaly_overlay::handle_anomaly_overlay_key(
        &mut anomaly_overlay,
        ctx.anomaly_events_ring.len(),
        k.code,
    );
    continue;
}
```

- [ ] **Step 3: Run lib + integration tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --all-targets`

Expected: existing tests pass; no regressions.

- [ ] **Step 4: Commit**

```bash
git add src/app/tui_loop.rs
git commit -m "phase 7 v3 (c): tui_loop wires SqliteAnomalySink + 'h' key history-view dispatch"
```

---

## Task 11: Settings UI — `[anomaly] retention_days` row

**Files:**

- Modify: `src/ui/settings.rs` (Parameters tab: 1 new row)

- [ ] **Step 1: Add the row**

Locate the existing `setting_row("anomaly memory_growth_mb", ...)` block. Right after it (before the `Line::from("")` separator that introduces `[anomaly.promote]`), add:

```rust
        setting_row(
            "anomaly retention_days",
            config.anomaly.retention_days.to_string(),
            defaults.anomaly.retention_days.to_string(),
        ),
```

- [ ] **Step 2: Add render test**

Append to the existing `mod tests` in this file:

```rust
#[test]
fn parameters_tab_includes_anomaly_retention_days_row() {
    let overlay = SettingsOverlay::new();
    let config = QmonsterConfig::default();
    let defaults = QmonsterConfig::default();
    let lines = build_settings_body_lines(&overlay, &config, &defaults);
    let rendered = lines_to_string(&lines);
    assert!(
        rendered.contains("anomaly retention_days"),
        "row label missing: {rendered}"
    );
    assert!(rendered.contains("30"), "default value 30 missing");
}
```

(The helper names `build_settings_body_lines` / `lines_to_string` should match what's in this file from v1.46.0 T11; if differing, mirror the existing test fixture pattern.)

- [ ] **Step 3: Run tests + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --lib ui::settings::tests::parameters_tab_includes_anomaly_retention_days_row`

Expected: new test passes.

- [ ] **Step 4: Commit**

```bash
git add src/ui/settings.rs
git commit -m "phase 7 v3 (c): Settings Parameters [anomaly] retention_days row + 1 test"
```

---

## Task 12: Integration tests — write+roundtrip + replay

**Files:**

- Modify: `tests/event_loop_integration.rs` (2 new tests)

- [ ] **Step 1: Add the persistence write integration test**

Append at the bottom of `tests/event_loop_integration.rs`:

```rust
#[test]
fn event_loop_persists_anomaly_event_to_audit_db() {
    // Phase 7 v3 (c) Task 12: verify run_once_with_target writes a row
    // to anomaly_events when a signal fires and ctx.anomaly_sink is set.
    use qmonster::app::config::{AnomalyConfig, AnomalyPromoteConfig};
    use qmonster::app::event_loop::run_once_with_target;
    use qmonster::domain::anomaly::AnomalyKind;
    use qmonster::policy::rules::anomaly::AnomalyHistory;
    use qmonster::store::SqliteAnomalySink;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("audit.db");

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly = AnomalyConfig {
        enabled: true,
        window_polls: 5,
        min_confidence: "medium".to_string(),
        identity_churn_min_flips: 2,
        error_burst_threshold: 0.5,
        cache_discontinuity_drop: 0.30,
        cross_pane_cluster_min_findings: 3,
        cost_slope_usd_per_hour: 20.0,
        token_slope_input_per_poll: 20_000,
        memory_growth_mb: 1024.0,
        promote: AnomalyPromoteConfig::default(),
        retention_days: 30,
    };
    let sink = SqliteAnomalySink::open(&db_path).unwrap();
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()))
        .with_anomaly_sink(sink);

    let mut h = AnomalyHistory::default();
    h.identity_snapshots.push_back(("Codex".to_string(), "/b".to_string()));
    h.identity_snapshots.push_back(("Claude".to_string(), "/a".to_string()));
    h.identity_snapshots.push_back(("Codex".to_string(), "/b".to_string()));
    ctx.anomaly_history.insert("%99".to_string(), h);

    let _ = run_once_with_target(&mut ctx, std::time::Instant::now(), None).expect("run ok");

    // Re-open the DB independently and query — verifies the row was committed.
    let read_sink = SqliteAnomalySink::open(&db_path).unwrap();
    let events = read_sink.fetch_recent_anomaly_events(10);
    assert!(
        events
            .iter()
            .any(|e| e.kind == AnomalyKind::IdentityChurn && e.pane_id == "%99"),
        "expected persisted IdentityChurn event for %99; got {events:?}"
    );
}
```

- [ ] **Step 2: Add the replay integration test**

```rust
#[test]
fn event_loop_replays_anomaly_history_on_first_observation() {
    // Phase 7 v3 (c) Task 12: pre-populate anomaly_history_snapshots
    // for a pane; verify run_once_with_target hydrates the in-memory
    // AnomalyHistory deques on the first tick of a fresh Context.
    use qmonster::app::config::{AnomalyConfig, AnomalyPromoteConfig};
    use qmonster::app::event_loop::run_once_with_target;
    use qmonster::store::{AnomalyHistorySnapshot, SqliteAnomalySink};
    use qmonster::app::event_loop::current_unix_ms;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("audit.db");

    // Seed disk with 3 recent snapshots for pane %99.
    let writer_sink = SqliteAnomalySink::open(&db_path).unwrap();
    let now_secs = (current_unix_ms() / 1000) as u64;
    for offset in [4u64, 2, 0] {
        let snap = AnomalyHistorySnapshot {
            identity: Some(("Codex".to_string(), format!("/p{offset}"))),
            error_hint: Some(false),
            cache_hit_ratio: None,
            cache_drift_fire: false,
            cross_pane_edit_paths: Vec::new(),
            cost_usd: Some(1.0 + offset as f64 * 0.5),
            input_tokens: Some(100 + offset),
            output_tokens: Some(50 + offset),
            process_memory_mb: Some(120.0 + offset as f64),
            agent_memory_bytes: Some(1_000_000),
            subagent_hint: false,
        };
        writer_sink
            .upsert_anomaly_history_snapshot("%99", now_secs - offset, &snap)
            .unwrap();
    }
    drop(writer_sink); // release the lock before reopening

    let source = FixturePaneSource {
        panes: vec![pane("%99", "claude:1:main", "claude", "✦ Idle", false)],
    };
    let notifier = RecordingNotifier(Arc::new(Mutex::new(Vec::new())));
    let mut config = QmonsterConfig::defaults();
    config.anomaly = AnomalyConfig {
        enabled: true,
        window_polls: 20,
        min_confidence: "medium".to_string(),
        identity_churn_min_flips: 5, // high threshold so IdentityChurn doesn't fire
        error_burst_threshold: 0.5,
        cache_discontinuity_drop: 0.30,
        cross_pane_cluster_min_findings: 3,
        cost_slope_usd_per_hour: 20.0,
        token_slope_input_per_poll: 20_000,
        memory_growth_mb: 1024.0,
        promote: AnomalyPromoteConfig::default(),
        retention_days: 30,
    };

    let sink = SqliteAnomalySink::open(&db_path).unwrap();
    let mut ctx = Context::new(config, source, notifier, Box::new(InMemorySink::new()))
        .with_anomaly_sink(sink);

    // Fresh context: no in-memory history for %99.
    assert!(ctx.anomaly_history.get("%99").is_none());

    let _ = run_once_with_target(&mut ctx, std::time::Instant::now(), None).expect("run ok");

    // After one tick, the deques should contain the 3 replayed snapshots
    // PLUS the one observation from the live tick (4 total).
    let history = ctx
        .anomaly_history
        .get("%99")
        .expect("history was hydrated for %99");
    assert!(
        history.cost_usd_samples.len() >= 3,
        "expected >= 3 cost_usd samples after replay+live; got {}",
        history.cost_usd_samples.len()
    );
}
```

- [ ] **Step 3: Run integration tests**

Run:

- `cargo test --test event_loop_integration event_loop_persists_anomaly_event_to_audit_db event_loop_replays_anomaly_history_on_first_observation`

Expected: 2 passed.

- [ ] **Step 4: Run full test suite + fmt + clippy**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --all-targets`

Expected: lib unchanged (or ~+18 from prior tasks); integration count = 60 (was 58).

- [ ] **Step 5: Commit**

```bash
git add tests/event_loop_integration.rs
git commit -m "phase 7 v3 (c): event_loop integration tests — persist + replay (+2 integration)"
```

---

## Task 13: Docs + version surfaces

**Files:**

- Modify: `docs/ai/UI_MANUAL.md`
- Modify: `docs/ai/ARCHITECTURE.md`
- Modify: `docs/ai/VALIDATION.md`
- Modify: `docs/ai/PROJECT_BRIEF.md`
- Modify: `config/qmonster.example.toml`
- Modify: `README.md`
- Modify: `VERSION.md`
- Modify: `package.json`

- [ ] **Step 1: Extend `docs/ai/UI_MANUAL.md` §8.8 with `h` keybind**

Locate the v1.46.0 §8.8 "Anomaly Events Overlay" subsection (`grep -n "Anomaly Events Overlay" docs/ai/UI_MANUAL.md`). Add to the keybinds list:

```markdown
- `h`: toggle between Ring view (this session, in-memory) and History view (last 200 from disk).
```

Below the keybinds list, add a "View modes" paragraph:

```markdown
**View modes:**

- **Ring (default):** shows the in-memory ring buffer (capacity 100, this session only).
- **History:** queries `anomaly_events` SQLite table for the last 200 rows. Includes events from earlier sessions.

The persistent `anomaly_events` table is pruned on startup by `[anomaly] retention_days` (default 30) and an emergency 100K-row cap.
```

- [ ] **Step 2: Document `[anomaly] retention_days` in UI_MANUAL.md**

In the existing anomaly config section (near the v1.46.0 `[anomaly.promote]` documentation), add:

```markdown
**`[anomaly] retention_days`** — number of days to retain `anomaly_events`
rows on disk (v1.47.0+). Default 30. Older rows are deleted on Qmonster
startup. A 100K-row hard cap also applies regardless of this value, and
`anomaly_history_snapshots` are auto-pruned at 4× the detection window.
```

- [ ] **Step 3: Prepend v1.47.0 paragraph to `docs/ai/ARCHITECTURE.md`**

The codebase uses newest-first (reverse-chronological) ordering — verified by v1.46.0's commit pattern. Insert BEFORE the existing v1.46.0 paragraph:

```markdown
**v1.47.0 (Phase 7 v3 c — closes Phase 7):** SQLite persistence for
anomaly events and per-pane history. Two new tables in `audit.db`:
`anomaly_events` (every visible signal with its gate result) and
`anomaly_history_snapshots` (per-tick deque-head capture for replay).
New `SqliteAnomalySink` mirrors the existing `SqliteTokenUsageSink` /
`SqliteCostUsageSink` pattern. `Context.anomaly_sink: Option<...>` is
plugged in via `with_anomaly_sink(sink)`, which also runs the
retention purge (`[anomaly] retention_days = 30` default + 100K-row
hard cap; snapshots auto-purge at 4× the detection window). The `n`
overlay's new `h` key toggles between the session-scoped ring (v1.46
default) and a history view that queries the last 200 from disk.
Lazy AnomalyHistory replay on first-pane-observation per session,
gated by a staleness threshold of `window_polls × tick_seconds × 2`.
```

- [ ] **Step 4: Add row to `docs/ai/VALIDATION.md`**

Find the existing release-validation table (created in v1.46.0). Add a new row above the v1.46.0 row (newest-first):

```markdown
| v1.47.0 | Phase 7 v3 (c): SQLite persistence (closes Phase 7) | cargo fmt + clippy + test --all-targets (lib ~1180, integration 60); SBOM diff guard |
```

- [ ] **Step 5: Update `docs/ai/PROJECT_BRIEF.md`**

Phase line: append `Phase 7 v3 (c)` to the complete-phases list.
Date line: append v1.47.0 segment (e.g., `v1.47.0 (2026-05-07): Phase 7 v3 (c) — SQLite persistence for anomaly events + history; closes Phase 7`).

- [ ] **Step 6: Add `retention_days` to `config/qmonster.example.toml`**

Locate the existing `[anomaly]` block. After the existing fields and before the `[anomaly.promote]` block, add:

```toml
# Phase 7 v3 (c) (v1.47.0): retention horizon for anomaly_events table.
# Older rows are deleted on Qmonster startup. Default 30 days; a 100K-row
# hard cap also applies. Snapshots auto-purge at 4× window_polls.
# retention_days = 30
```

- [ ] **Step 7: Bump version surfaces**

- `README.md`: update the release table cells `v1.46.0` → `v1.47.0` and `qmonster@1.46.0` → `qmonster@1.47.0`.
- `VERSION.md`: replace `v1.46.0` with `v1.47.0` and `1.46.0` with `1.47.0` (keep Cargo crate at `0.1.0`).
- `package.json`: change `"version": "1.46.0"` to `"version": "1.47.0"`.

- [ ] **Step 8: Run full validation**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --all-targets`

Expected: clean; lib ~1180, integration 60.

- [ ] **Step 9: Commit**

```bash
git add docs/ai/UI_MANUAL.md docs/ai/ARCHITECTURE.md docs/ai/VALIDATION.md docs/ai/PROJECT_BRIEF.md config/qmonster.example.toml README.md VERSION.md package.json
git commit -m "phase 7 v3 (c): docs (UI_MANUAL h-key + retention_days, ARCHITECTURE v1.47.0 paragraph, VALIDATION row, example.toml + version surfaces)"
```

---

## Task 14: Release ledger sync + tag

**Files:**

- Modify: `mission.yaml`
- Modify: `mission-history.yaml`
- Modify: `.mission/CURRENT_STATE.md`
- New: annotated git tag `v1.47.0`

- [ ] **Step 1: Refresh `.mission/CURRENT_STATE.md`**

Replace the v1.46.0 banner with a v1.47.0 banner. Mirror the v1.46.0 release-ledger-sync structure:

- `_Last updated: 2026-05-07 (Claude, v1.47.0 release ledger sync)_`
- Mission section: title, version surfaces, branch/tag, release publication state (v1.46.0 published; v1.47.0 publication pending), Current phase line gains `Phase 7 v3 (c)`.
- Replace `## v1.46.0 Feature State` section with `## v1.47.0 Feature State` covering 12 enumerated changes:
  1. anomaly_events SQLite table schema (src/store/sqlite.rs)
  2. anomaly_history_snapshots SQLite table schema (src/store/sqlite.rs)
  3. AnomalyHistorySnapshot struct + push_snapshot_into_history helper (src/store/anomaly_history.rs)
  4. SqliteAnomalySink (src/store/anomaly_sink.rs) — open/insert/upsert/fetch/with_transaction/run_retention
  5. AnomalyConfig.retention_days = 30 default (src/app/config.rs)
  6. Context.anomaly_sink + anomaly_history_replayed + with_anomaly_sink builder (src/app/bootstrap.rs)
  7. event_loop per-tick transaction wrapping ring push + snapshot upsert (src/app/event_loop.rs)
  8. event_loop lazy AnomalyHistory replay on first-pane-observation (src/app/event_loop.rs)
  9. AnomalyOverlayView enum + AnomalyOverlay state extensions (src/ui/anomaly_overlay.rs)
  10. AnomalyOverlay render branch for History view (src/ui/anomaly_overlay.rs)
  11. tui_loop wiring: SqliteAnomalySink construction + 'h' key dispatch (src/app/tui_loop.rs)
  12. Settings UI: anomaly retention_days row (src/ui/settings.rs); label/try_from_label helpers for AnomalyKind/Confidence/Severity (src/domain/anomaly.rs, src/domain/recommendation.rs)
- Update Latest Release Notes (plan + spec paths).
- Reset Post-tag polish to "No genuinely-post-v1.47.0 work has landed yet."
- Known External State: include v1.46.0 in the all-published list (note: v1.47.0 publication pending).
- Active Follow-Ups:
  1. D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters (Phase 7's only remaining follow-up; Phase 7 is otherwise feature-complete).
  2. Tag protection / ruleset activation remains a GitHub Settings task.
- Validation Baseline:
  - `cargo test --all-targets` — 1180 lib tests + 60 integration tests, all green (lib up from 1162 at v1.46.0; integration up from 58).
- Next First Action: Phase 7 D3 is provider-blocked; pick from upstream backlog (token observability v2, etc.) or address operator-side follow-ups (tag protection).

- [ ] **Step 2: Append v1.47.0 entry to `mission-history.yaml`**

Mirror the v1.46.0 timeline entry (around lines 17368-17467 — read it first for exact structure). New entry:

- `change_id: "v1.47.0-phase-7-v3-c-sqlite-persistence"`
- `semantic_version: "1.47.0"`
- `change_sequence: 190`
- `date: "2026-05-07"`
- `author: "Claude Sonnet 4.6"` (or appropriate model name)
- `summary` and `intent_ko` describing Phase 7 v3 (c): SQLite persistence
- `triggered_by: "Operator scoped Phase 7 v3 (c) as a 14-task implementation sequence (Tasks 1-13 = impl commits, Task 14 = ledger sync). Closes Phase 7 v3 cycle (a/b shipped in v1.46.0, c ships now)."`
- `changes.added`: list new files + types + functions (anomaly_events / anomaly_history_snapshots tables, AnomalyHistorySnapshot, SqliteAnomalySink, retention_days, anomaly_sink/anomaly_history_replayed Context fields, AnomalyOverlayView enum, label/try_from_label helpers).
- `changes.modified`: src/store/sqlite.rs, src/store/mod.rs, src/domain/anomaly.rs, src/domain/recommendation.rs, src/app/config.rs, src/app/bootstrap.rs, src/app/event_loop.rs, src/ui/anomaly_overlay.rs, src/app/tui_loop.rs, src/ui/settings.rs, docs files, example.toml, README.md, VERSION.md, package.json, mission ledger files.
- `observable_effects`:
  - "Anomaly events persisted across restarts in `~/.qmonster/audit.db` (anomaly_events table); accessible via `n` overlay `h` key."
  - "Per-pane AnomalyHistory deques replay from disk on first observation per session (staleness gate: ≤2× window)."
  - "Retention runs lazily on Context::new: time-based (`retention_days`, default 30) + 100K-row emergency cap."
  - "Settings Parameters tab gains `anomaly retention_days` row."
- `impact_scope`:
  - `runtime`: per-tick SQLite transaction (1 INSERT per visible signal + 1 UPSERT per pane). Lazy replay query on first observation per pane per session.
  - `tests`: 1180 lib tests + 60 integration tests passing at the v1.47.0 release commit (+18 lib vs v1.46 baseline 1162; +2 integration vs 58).
  - `ledger`: 1 new timeline entry (sequence 190); meta total_revisions 189 → 190, latest_version 1.46.0 → 1.47.0.
  - `npm`: package.json version 1.47.0; CI publication of v1.47.0 follows the same workflow as v1.37 through v1.46.
- `breaking: false`
- `risk: "low — additive: no migration of existing tables; CREATE TABLE IF NOT EXISTS is idempotent. Detection logic unchanged. When ctx.anomaly_sink is None, behavior is identical to v1.46.0."`
- `related_commits`: list every Phase 7 v3 (c) commit by SHA (run `git log --oneline 2edfd29..HEAD --no-merges` and filter to phase 7 v3 (c) commits, excluding any unrelated parallel-track commits).
- `follow_up`:
  - "Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters (only outstanding Phase 7 follow-up)."
  - "Tag protection/ruleset activation remains a GitHub Settings task."
  - "v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 / v1.45.0 / v1.46.0 / v1.47.0 CI publication verification (GitHub Release + npm package) remains an external follow-up until each release workflow completes."

Update meta:

- `meta.mission_title`: replace v1.46.0 description with v1.47.0 description
- `meta.total_revisions`: 189 → 190
- `meta.latest_version`: "1.46.0" → "1.47.0"

Update `evolution_summary`:

- `current_phase`: replace v1.46.0 description with v1.47.0 description (closes Phase 7 v3)
- `next_phase`: focus on upstream backlog or remaining provider-blocked Phase 7 D3

- [ ] **Step 3: Update `mission.yaml`**

In `execution_hints.topology`, replace v1.46.0 description with v1.47.0 description: "Current working state is v1.47.0. Phases 1-5, ..., Phase 7 v3 (a+b), Phase 7 v3 (c) are complete. v1.47.0 ships SQLite persistence for anomaly events + history. Two new tables (anomaly_events, anomaly_history_snapshots) added to audit.db via SqliteAnomalySink (mirrors SqliteTokenUsageSink/SqliteCostUsageSink pattern). Per-tick transaction batching, lazy AnomalyHistory replay on first-pane-observation (staleness ≤ 2× window), 30-day default retention + 100K-row hard cap on events, snapshots auto-purge at 4× window. n overlay gains `h` key for History view. Phase 7 closes after this release; D3 per-subagent token attribution remains the only outstanding Phase 7 follow-up (provider-blocked)."

In `execution_hints.suggested_approach`, update for v1.47.0 baseline.

In `budget_hint`:

- `priority`: "post-v1.47.0 — pick from upstream backlog. Phase 7 D3 stays provider-blocked."
- `estimated_sessions`: 1
- `scope`: summarize v1.47.0 (SQLite persistence) and Phase 7 closure
- `followups`:
  - "Phase 7 D3 per-subagent token attribution (provider-blocked)"
  - "Tag protection/ruleset activation (GitHub Settings task)"

- [ ] **Step 4: Run validation gates**

Run:

- `cargo fmt --all --check`
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --all-targets`
- `git diff --check`

Expected: clean; lib 1180 + integration 60.

- [ ] **Step 5: Commit the ledger sync**

```bash
git add mission.yaml mission-history.yaml .mission/CURRENT_STATE.md
git commit -m "v1.47.0 release ledger sync"
```

- [ ] **Step 6: Create annotated v1.47.0 tag**

```bash
git tag -a v1.47.0 -m "v1.47.0 — Phase 7 v3 (c): SQLite persistence; closes Phase 7"
```

- [ ] **Step 7: Verify tag**

Run:

- `git rev-parse v1.47.0^{commit}`
- `git log --oneline -1`

Both should print the same SHA.

- [ ] **Step 8: STOP — do NOT push**

Push + publication is operator-controlled (matches v1.42-v1.46 pattern). Report back that v1.47.0 is locally tagged and awaits push.

---

## Validation summary (cumulative across all tasks)

After Task 14:

- **Lib tests:** 1162 (v1.46 baseline) → ~1180 (+~18).
  - +4 (Task 1: kind/conf/severity label roundtrips + garbage rejection)
  - +3 (Task 2: snapshot roundtrip, empty handling, replay absent fields)
  - +11 (Task 3: 9 sink tests + 2 schema tests)
  - +3 (Task 4: retention_days default + omit + roundtrip)
  - 0 (Tasks 5-7: integration covered by Task 12)
  - +4 (Task 8: state extensions)
  - +3 (Task 9: render branch)
  - 0 (Task 10: dispatch wiring; integration covers it)
  - +1 (Task 11: settings)
  - Total ~+29; tighter implementations may land closer to ~+22.
- **Integration tests:** 58 → 60 (+2 from Task 12).
- **`cargo fmt`, `cargo clippy --lib --tests`, `git diff --check`** clean.
- **Local tag:** `v1.47.0` annotated, points at the release ledger sync commit.
- **Push + publication:** operator-controlled follow-up.

---

**End of plan.**
