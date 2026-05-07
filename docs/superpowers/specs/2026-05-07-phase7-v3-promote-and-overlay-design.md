# Phase 7 v3 (a+b) — Per-kind Promotion Gate + `n` Overlay (v1.46.0) — Design Spec

**Date:** 2026-05-07
**Target version:** v1.46.0
**Predecessor:** v1.45.0 Phase 7 v2 detectors
**Successor (deferred):** Phase 7 v3 (c) — SQLite persistence for anomaly history (v1.47+)

---

## 1. Goal

Ship two related Phase 7 v3 sub-features as a single v1.46.0 minor release:

1. **(a) Operator-tunable promotion gate.** Replace the hardcoded `severity ≥ Warning` filter inside `promote_anomalies_to_recommendations` with a per-`AnomalyKind` minimum-confidence threshold sourced from `AnomalyConfig`. Operators can tune which detector kinds promote and at which confidence floor.
2. **(b) `n` overlay (anomaly events).** Add an in-memory ring buffer of recent `AnomalyEvent` records and a new top-level overlay (`n` keybind) that visualizes them chronologically. Read-only observation surface for the signal stream that the m overlay's compact `ANOMALIES` row only summarizes.

The (c) sub-feature — SQLite persistence for anomaly history — is **deferred** to v1.47+ because it raises audit-grade / replay-semantics / retention questions that deserve standalone design attention, and (a)+(b) ship cleanly without it.

## 2. Non-goals

- No SQLite schema changes. The ring buffer is purely in-memory; it resets on every session start.
- No `AnomalyKind`, `AnomalySignal`, `AnomalyConfidence`, `AnomalyHistory`, or detector-function changes.
- No `severity_for(confidence)` change. Confidence → severity coupling stays honest (High → Warning, Medium/Low → Concern).
- No audit chain changes. `AnomalyEvent` records do not enter the audit log.
- No `SignalSet` schema changes.
- No provider adapter changes.
- No new sparkline rendering, no per-kind severity overrides (option 2 from brainstorming Q2 — deferred to v1.47+ if there's demand), no filters/search inside the `n` overlay, no operator-configurable ring buffer size.

## 3. Architecture overview

```
┌──────────────────┐    ┌──────────────────┐
│  detectors emit  │───▶│  AnomalySignal[] │
└──────────────────┘    └────────┬─────────┘
                                 │
                                 ▼
                  ┌────────────────────────────────────┐
                  │ promote_anomalies_to_recommendations│
                  │   gate: signal.confidence >=        │  ← (a) per-kind
                  │     gates.promote_min_confidence    │      threshold
                  │     (signal.kind)                   │
                  └────────────────────┬───────────────┘
                                       │
                          ┌────────────┴────────────┐
                          ▼                          ▼
                ┌──────────────────┐       ┌──────────────────┐
                │ Recommendations  │       │ AnomalyEventsRing│  ← (b) ring
                │  (existing path) │       │ (push for ALL    │      buffer
                └──────────────────┘       │  signals, with   │
                                           │  promoted bool)  │
                                           └────────┬─────────┘
                                                    │
                                                    ▼
                                           ┌──────────────────┐
                                           │  `n` overlay     │  ← (b) view
                                           └──────────────────┘
```

Two data flows run in parallel from the post-`eval_anomalies` signal vec inside `event_loop`:

- The existing recommendation pipeline gets a new gate check.
- A new ring buffer captures every signal (promoted or not) so the operator can see what their gate is filtering.

Both happen BEFORE `deliver_effects` and the audit_event loop — same sequencing pattern locked in by the v1.44.0 review fixup.

## 4. Two layers of `min_confidence` — keep them distinct

The codebase already has a `min_confidence` field. v1.46.0 adds another. Naming and semantics need to stay clean.

| Layer                                   | Field                      | Default                                                     | Effect                                                                                                                           |
| --------------------------------------- | -------------------------- | ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Visibility (existing, since Phase 7 v1) | `[anomaly] min_confidence` | `"medium"`                                                  | Detector signals below this confidence don't reach `PaneReport.anomalies` and aren't visible in the m overlay's `ANOMALIES` row. |
| Promotion (new, v1.46.0)                | `[anomaly.promote] <kind>` | `"high"` for 7 kinds, `"medium"` for `subagent_side_effect` | Visible signals are gated again before becoming Recommendations. Below the threshold = passive observation only.                 |

A signal can be visible-but-not-promoted (Medium-confidence CostSlope, default config). A signal cannot be promoted-but-not-visible — the visibility layer always runs first.

## 5. Data shapes

### 5.1 `AnomalyEvent` (new, in `src/domain/anomaly.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyEvent {
    pub timestamp: i64,                 // unix epoch seconds
    pub pane_id: String,
    pub kind: AnomalyKind,
    pub confidence: AnomalyConfidence,
    pub severity: Severity,             // mirrors signal.severity at push time
    pub promoted: bool,                 // promotion gate result
    pub reason: String,                 // brief evidence summary, ≤ ~100 chars
}
```

Lives next to `AnomalySignal` in the domain layer. The `reason` field is built at push time from `signal.evidence.first()` (or empty if none) — a one-line `"{metric_name}: {before} → {after}"` rendering. Not the full promote-matrix reason text (which is recommendation-prose); just enough for the `n` overlay row.

### 5.2 `AnomalyEventsRing` (new, in `src/app/anomaly_events_ring.rs`)

```rust
pub struct AnomalyEventsRing {
    events: VecDeque<AnomalyEvent>,
}

impl AnomalyEventsRing {
    pub const CAPACITY: usize = 100;

    pub fn new() -> Self { ... }
    pub fn push(&mut self, event: AnomalyEvent) {
        if self.events.len() >= Self::CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
    pub fn iter(&self) -> impl Iterator<Item = &AnomalyEvent> + '_ {
        self.events.iter()
    }
    pub fn len(&self) -> usize { self.events.len() }
    pub fn is_empty(&self) -> bool { self.events.is_empty() }
}
```

Capacity is a `const` (100) — operator-configurable size is deferred. Newest-last by insertion order; the overlay reverses for display. Single global ring buffer (not per-pane); chronological merge across panes is the operator-useful view, with `pane_id` as a column.

### 5.3 `AnomalyPromoteConfig` (new, in `src/app/config.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnomalyPromoteConfig {
    pub identity_churn: String,             // default "high"
    pub error_burst: String,                // default "high"
    pub cache_discontinuity: String,        // default "high"
    pub cross_pane_edit_cluster: String,    // default "high"
    pub cost_slope: String,                 // default "high"
    pub token_slope: String,                // default "high"
    pub memory_growth: String,              // default "high"
    pub subagent_side_effect: String,       // default "medium"
}

impl Default for AnomalyPromoteConfig {
    fn default() -> Self {
        Self {
            identity_churn:          "high".to_string(),
            error_burst:             "high".to_string(),
            cache_discontinuity:     "high".to_string(),
            cross_pane_edit_cluster: "high".to_string(),
            cost_slope:              "high".to_string(),
            token_slope:             "high".to_string(),
            memory_growth:           "high".to_string(),
            subagent_side_effect:    "medium".to_string(),
        }
    }
}
```

Strings (not enums) for toml-friendliness, parsed at the `PolicyGates` boundary via the existing `parse_min_confidence` helper. New nested field on `AnomalyConfig`:

```rust
pub struct AnomalyConfig {
    // ... existing fields ...
    pub promote: AnomalyPromoteConfig,
}
```

`#[serde(default)]` on both `AnomalyConfig` and `AnomalyPromoteConfig` ensures pre-v1.46 toml files (no `[anomaly.promote]` section) get the new defaults automatically.

### 5.4 `PolicyGates` extensions (in `src/policy/gates.rs`)

```rust
pub struct PolicyGates {
    // ... existing anomaly fields ...
    pub anomaly_promote_identity_churn:          AnomalyConfidence,
    pub anomaly_promote_error_burst:             AnomalyConfidence,
    pub anomaly_promote_cache_discontinuity:     AnomalyConfidence,
    pub anomaly_promote_cross_pane_edit_cluster: AnomalyConfidence,
    pub anomaly_promote_cost_slope:              AnomalyConfidence,
    pub anomaly_promote_token_slope:             AnomalyConfidence,
    pub anomaly_promote_memory_growth:           AnomalyConfidence,
    pub anomaly_promote_subagent_side_effect:    AnomalyConfidence,
}

impl PolicyGates {
    pub fn promote_min_confidence(&self, kind: AnomalyKind) -> AnomalyConfidence {
        match kind {
            AnomalyKind::IdentityChurn         => self.anomaly_promote_identity_churn,
            AnomalyKind::ErrorBurst            => self.anomaly_promote_error_burst,
            AnomalyKind::CacheDiscontinuity    => self.anomaly_promote_cache_discontinuity,
            AnomalyKind::CrossPaneEditCluster  => self.anomaly_promote_cross_pane_edit_cluster,
            AnomalyKind::CostSlope             => self.anomaly_promote_cost_slope,
            AnomalyKind::TokenSlope            => self.anomaly_promote_token_slope,
            AnomalyKind::MemoryGrowth          => self.anomaly_promote_memory_growth,
            AnomalyKind::SubagentSideEffect    => self.anomaly_promote_subagent_side_effect,
        }
    }
}
```

Populated from `AnomalyConfig.promote.*` in `from_inputs`, parsed via `parse_min_confidence`. Defaults in `PolicyGates::default()` mirror the AnomalyConfig defaults (for the test fixture path that bypasses config parsing).

### 5.5 `AnomalyOverlay` (new, in `src/ui/anomaly_overlay.rs`)

```rust
pub struct AnomalyOverlay {
    open: bool,
    scroll: usize,
}

impl AnomalyOverlay {
    pub fn new() -> Self { Self { open: false, scroll: 0 } }
    pub fn is_open(&self) -> bool { self.open }
    pub fn open(&mut self) { self.open = true; self.scroll = 0; }
    pub fn close(&mut self) { self.open = false; }
    pub fn toggle(&mut self) { if self.open { self.close() } else { self.open() } }
    pub fn scroll_down(&mut self, max: usize) { self.scroll = (self.scroll + 1).min(max); }
    pub fn scroll_up(&mut self) { self.scroll = self.scroll.saturating_sub(1); }
    pub fn render(&self, frame: &mut Frame, area: Rect, ring: &AnomalyEventsRing) { ... }
}
```

State is `(open, scroll)` only — minimal. Render reads from the `&AnomalyEventsRing` passed by `dashboard_render`.

## 6. Promotion gate change

### 6.1 Code change in `src/policy/rules/anomaly.rs`

`promote_anomalies_to_recommendations` gains `gates: &PolicyGates` parameter. The gate at the top of the loop changes from:

```rust
// before (v1.45.0)
pub fn promote_anomalies_to_recommendations(signals: &[AnomalySignal]) -> Vec<Recommendation> {
    for sig in signals {
        if sig.severity < Severity::Warning {
            continue;
        }
        // ... promote matrix ...
    }
}
```

to:

```rust
// after (v1.46.0)
pub fn promote_anomalies_to_recommendations(
    signals: &[AnomalySignal],
    gates: &PolicyGates,
) -> Vec<Recommendation> {
    for sig in signals {
        if sig.confidence < gates.promote_min_confidence(sig.kind) {
            continue;
        }
        // ... promote matrix unchanged ...
    }
}
```

`severity_for(confidence)` is unchanged. The promote-matrix arms (action / reason / next_step builders) are unchanged. Only the gate predicate changes.

### 6.2 Behavior preservation for the 7 default-`"high"` kinds

For `IdentityChurn`, `ErrorBurst`, `CacheDiscontinuity`, `CrossPaneEditCluster`, `CostSlope`, `TokenSlope`, `MemoryGrowth`:

- Today: `if severity < Warning continue` — Concern severity blocked.
- v1.46 default (`"high"`): `if confidence < High continue` — Medium/Low confidence blocked.
- Equivalent because `severity_for(High) = Warning` and `severity_for(Medium|Low) = Concern`.

### 6.3 Latent v1.45.0 dead-code fix for `SubagentSideEffect`

Today: SubagentSideEffect emits at `severity = severity_for(Medium) = Concern`. The Warning-gate filters it out. Its match arm in the promote matrix exists but is **unreachable**. The v1.45.0 ledger description ("Concern-severity Recommendation is promoted") and the v1.45.0 Settings UI annotation ("promotes when subagent_hint co-occurs with another anomaly") describe a behavior that never actually fired.

v1.46.0 default `subagent_side_effect = "medium"` makes the unreachable branch reachable at the threshold those docs already imply. This is a **dead-code-becomes-live fix**, framed honestly in release notes — not a behavior change disguised as a feature. The default matches the v1.45.0 documented intent.

### 6.4 Call site update in `src/app/event_loop.rs`

Single call site: `promote_anomalies_to_recommendations(&signals)` becomes `promote_anomalies_to_recommendations(&signals, gates)`. Existing `gates: &PolicyGates` is already in scope at that point (used by `eval_anomalies`).

### 6.5 Test fixture updates

`fixture_gates()` in `src/policy/rules/anomaly.rs` already uses the `..PolicyGates::default()` spread idiom, so the 8 new fields populate automatically from `PolicyGates::default()` (which mirrors `AnomalyPromoteConfig::default()`) — no explicit fixture edits required. Existing promote tests that relied on `severity < Warning` filtering continue passing because the equivalent confidence-based gate yields the same outcome at default thresholds. New tests cover:

- Per-kind threshold tunability (set `cost_slope = "medium"`, verify Medium CostSlope promotes).
- SubagentSideEffect default-medium behavior (Medium signal promotes at default config, no longer dead code).
- All 8 kinds at default threshold preserve v1.45.0 outcomes for High-confidence signals.

## 7. `n` overlay

### 7.1 Push site in `src/app/event_loop.rs`

Existing flow (v1.45.0):

```rust
let signals = eval_anomalies(...);                                   // ⓐ
let recs = promote_anomalies_to_recommendations(&signals);            // ⓑ (no gates yet)
// ... merge recs, deliver_effects, audit ...
```

v1.46.0 flow:

```rust
let signals = eval_anomalies(...);                                   // ⓐ
let recs = promote_anomalies_to_recommendations(&signals, gates);    // ⓑ (gated)
// New ⓒ: push every signal into the ring buffer with promoted bool
for sig in &signals {
    let promoted = sig.confidence >= gates.promote_min_confidence(sig.kind);
    ring.push(AnomalyEvent {
        timestamp: now,
        pane_id: pane_id.clone(),
        kind: sig.kind,
        confidence: sig.confidence,
        severity: sig.severity,
        promoted,
        reason: brief_reason_from_evidence(&sig.evidence),
    });
}
// ... deliver_effects, audit ...
```

Push runs BEFORE `deliver_effects` and the audit_event loop — same sequencing pattern locked in by v1.44.0. The ring buffer reflects the same gate decision the operator just made via config.

`brief_reason_from_evidence` is a small helper (could be inline) that builds `"{metric_name}: {before} → {after}"` from `evidence.first()`, or `""` if evidence is empty.

### 7.2 Key handler — `src/app/anomaly_overlay.rs`

```rust
pub fn handle_anomaly_overlay_key(
    overlay: &mut AnomalyOverlay,
    ring_len: usize,
    key: KeyCode,
) -> bool {
    match key {
        KeyCode::Char('n') => { overlay.toggle(); true }
        // close-only keys when open:
        KeyCode::Esc | KeyCode::Char('q') if overlay.is_open() => { overlay.close(); true }
        KeyCode::Down | KeyCode::Char('j') if overlay.is_open() => {
            overlay.scroll_down(ring_len.saturating_sub(1));
            true
        }
        KeyCode::Up | KeyCode::Char('k') if overlay.is_open() => {
            overlay.scroll_up();
            true
        }
        _ => false,
    }
}
```

`true` return = key consumed (matches `metrics_overlay` convention). `n` toggles open/close; `Esc`/`q` close-only when open; `Up`/`Down`/`j`/`k` scroll when open. No `g`/`G`/PageUp/PageDown for v1 — YAGNI.

### 7.3 Mouse handler — `src/app/anomaly_overlay.rs`

```rust
pub fn handle_anomaly_overlay_mouse(
    overlay: &mut AnomalyOverlay,
    viewport: Rect,
    ring_len: usize,
    event: MouseEvent,
) -> bool { ... }
```

Mirrors `handle_metrics_overlay_mouse` and `handle_pending_actions_overlay_mouse`: scroll-wheel up/down adjusts `overlay.scroll`; click-outside closes; click inside is a no-op for v1 (no row interactions). Same anchor-clear pattern.

### 7.4 Render — `src/ui/anomaly_overlay.rs`

Title: `"ANOMALY EVENTS (last {len} — newest first) [n to close]"`.

Empty state body: `"No anomaly events recorded this session."` followed by a help hint `"Anomalies are recorded once detection is enabled in Settings > [anomaly] enabled."`.

Populated state columns (left→right):

- `Time` — `HH:MM:SS` (timestamp formatted as local time, seconds-resolution since session-only).
- `Pane` — `pane_id` truncated to 6–8 chars.
- `Kind` — `AnomalyKind.label()` (existing helper).
- `Conf` — `confidence.label()` (existing helper).
- `Promoted` — `"yes"` / `"no"`.
- `Reason` — truncated to fit remaining width.

Iteration order: `ring.iter().rev()` (newest first). Scroll offset clips the visible window.

### 7.5 Wiring — `dashboard_render.rs` and `tui_loop.rs`

`dashboard_render.rs`:

- New `view.anomaly_overlay: &AnomalyOverlay` and `view.anomaly_events_ring: &AnomalyEventsRing` fields on the render-context struct.
- New branch after the existing metrics_overlay branch:
  ```rust
  if view.anomaly_overlay.is_open() {
      view.anomaly_overlay.render(frame, area, view.anomaly_events_ring);
  }
  ```

`tui_loop.rs`:

- Construct `AnomalyEventsRing::new()` and `AnomalyOverlay::new()` once per session, passed by mutable ref into the loop.
- Key dispatch: when no other overlay is currently open, `n` routes to `handle_anomaly_overlay_key`. Same precedence pattern as the existing `m` overlay — overlays don't stack.

### 7.6 Settings UI integration — `src/ui/settings.rs`

Parameters tab gains a new section header `[anomaly.promote]` followed by 8 rows (one per kind), each editing a `String` field with `"low"`/`"medium"`/`"high"` validation. Same edit-row pattern as the existing `[anomaly] min_confidence` row.

Rules tab: each of the 8 anomaly detector rows already exists from Phase 7 v1+v2. The annotation text on each row updates to read from the configured value (e.g., `"promotes when confidence >= medium"` or `"promotes when confidence >= high"`) instead of the hardcoded v1.45.0 strings.

## 8. AppState and lifecycle

### 8.1 New AppState fields

```rust
pub struct AppState {
    // ... existing fields ...
    pub anomaly_events_ring: AnomalyEventsRing,
    pub anomaly_overlay: AnomalyOverlay,
}
```

Initialized via `AnomalyEventsRing::new()` and `AnomalyOverlay::new()` in `AppState::new()`.

### 8.2 Lifecycle

- **Session start:** Ring buffer is empty. Overlay is closed.
- **Each tick:** `event_loop` pushes per-pane events (one per signal). Capacity 100; oldest dropped on overflow.
- **Session end:** Buffer is dropped with the process. No persistence.

When `[anomaly] enabled = false`, `eval_anomalies` returns empty, so no events are pushed. The overlay opens but always shows the empty-state body.

## 9. Backwards compatibility

- `#[serde(default)]` on `AnomalyConfig` and the new `AnomalyPromoteConfig` ensures pre-v1.46 `qmonster.toml` files load cleanly with v1.46 defaults applied to the new section.
- All 8 default thresholds preserve v1.45.0 promotion behavior for the 7 `"high"`-default kinds. The `subagent_side_effect = "medium"` default activates a previously-unreachable promote branch — release notes call this out as a latent-bug fix matching the v1.45.0 documented intent.
- No data migration. Ring buffer is in-memory only.

## 10. Tests

### 10.1 Unit tests (target: ~+20 lib tests)

**Promotion gate (`src/policy/rules/anomaly.rs`):**

- `promote_gate_blocks_below_per_kind_threshold` — Medium CostSlope blocked at default `"high"` threshold.
- `promote_gate_admits_at_per_kind_threshold` — Medium CostSlope admitted when threshold is `"medium"`.
- `promote_gate_subagent_side_effect_default_medium_admits` — Medium SubagentSideEffect admitted at default `"medium"`, fixing the v1.45.0 dead-code branch.
- `promote_gate_subagent_side_effect_high_setting_blocks_medium` — operator override `"high"` blocks the previously-default Medium signal.
- `promote_gate_high_threshold_admits_high` — High signal admitted at any threshold (≥ High covers all).
- `promote_gate_low_threshold_admits_all` — `"low"` threshold admits Medium and Low signals.
- `promote_min_confidence_returns_configured_field_per_kind` — a single parametric test iterating all 8 `AnomalyKind` variants and asserting `gates.promote_min_confidence(kind)` returns the matching field.

**Ring buffer (`src/app/anomaly_events_ring.rs`):**

- `ring_push_below_capacity` — pushing < 100 events grows len.
- `ring_push_at_capacity_drops_oldest` — push 101st event evicts the first.
- `ring_iter_yields_insertion_order` — iter() returns events newest-last.
- `ring_starts_empty` — `new().is_empty()` is true.
- `ring_capacity_constant` — `CAPACITY == 100`.

**Overlay key/mouse/render (`src/ui/anomaly_overlay.rs` + `src/app/anomaly_overlay.rs`):**

- `n_toggles_open_close` — first `n` opens, second closes.
- `esc_closes_when_open_no_op_when_closed` — `Esc` only consumes when open.
- `q_closes_when_open` — alternative close key.
- `down_scrolls_within_bounds` — `Down` increments scroll; clamped at `ring_len-1`.
- `up_scrolls_within_bounds` — `Up` decrements; saturates at 0.
- `mouse_wheel_scrolls` — wheel events adjust scroll.
- `render_empty_shows_help_hint` — empty state body present.
- `render_populated_shows_columns` — column headers + at least one row visible.
- `render_truncates_long_reason` — long `reason` text fits in cell.

### 10.2 Integration tests (target: +1)

`tests/event_loop_integration.rs`:

- `event_loop_pushes_anomaly_events_for_visible_signals` — fixture pane source emits signals at mixed confidences; assert the ring buffer captures all visible signals (post-visibility-filter, pre-promotion-gate) with correct `promoted` bool.

### 10.3 Test count target

- Lib tests: 1123 (v1.45) → ~1144 (+~21).
  - Promotion gate: 7 (6 named scenarios + 1 parametric per-kind).
  - Ring buffer: 5.
  - Overlay key/mouse/render: 9.
- Integration tests: 57 (v1.45) → 58 (+1).

## 11. Validation gates

Same set that has guarded every release since v1.37:

- `cargo fmt --all --check`
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `cargo test --all-targets`
- `git diff --check`
- `scripts/release/dry-run.sh` + SBOM diff guard (inherited from CI).

## 12. Doc updates

- `docs/ai/UI_MANUAL.md` — new `n` overlay subsection (keybind, columns, scroll, empty state); promotion-gate config row in the existing anomaly subsection.
- `docs/ai/ARCHITECTURE.md` — v1.46.0 paragraph appended (two-layer min_confidence: visibility vs promotion; ring buffer scope = session-only; defer (c) note).
- `docs/ai/VALIDATION.md` — Phase 7 v3 (a+b) row added.
- `docs/ai/PROJECT_BRIEF.md` — Phase line + Date line updated with v1.46.0 segment.
- `config/qmonster.example.toml` — new `[anomaly.promote]` block with all 8 fields commented at their defaults; explanatory header noting visibility-vs-promotion distinction.
- `README.md` — release table bumped to v1.46.0.
- `VERSION.md` — bumped to v1.46.0.
- `package.json` — version 1.46.0.

## 13. Release notes call-outs

1. **Headline:** "Phase 7 v3 part 1: per-kind promotion thresholds + new `n` overlay for anomaly event history."
2. **Per-kind promotion gate:** explain the new `[anomaly.promote]` table; show example for stricter cost-slope or noisier identity-churn tuning.
3. **`n` overlay:** describe the keybind, what it shows, that it's session-only (deferred persistence).
4. **Latent-bug fix:** explicitly call out that v1.45.0's `SubagentSideEffect` promote-matrix branch was unreachable due to the hardcoded Warning gate; v1.46's default `subagent_side_effect = "medium"` makes that branch reachable, matching what the v1.45.0 ledger and Settings UI already described.
5. **Deferred:** SQLite persistence for anomaly history is the (c) sub-feature of Phase 7 v3 and ships separately when the audit-grade / replay / retention questions are designed.

## 14. Out of scope (explicit deferrals)

- **(c) SQLite persistence for anomaly history** — ring buffer survival across sessions, audit-grade `AnomalyEvent` table, retention policy, query API. Defer to v1.47+ standalone design.
- **Per-kind severity overrides** (option 2 from brainstorming Q2) — operator pinning Warning vs Concern regardless of confidence. Defer until there's demand.
- **Operator-configurable ring buffer size** — capacity 100 is a constant in v1.46. Defer to a config field if operators ask.
- **Filters / search inside the `n` overlay** — by kind, pane, promoted-status. Defer to v1.47+ if the chronological view proves insufficient.
- **Sparkline rendering of raw deque samples** — option 3 from brainstorming Q3. Different visualization shape; defer.
- **Per-pane ring buffers** — single global ring with `pane_id` column is the v1.46 choice. Defer split-by-pane to v1.47+ if the merged view becomes too noisy.

## 15. Topology summary (for mission ledger)

- Code touches: `src/domain/anomaly.rs` (+ `AnomalyEvent`), `src/app/config.rs` (+ `AnomalyPromoteConfig`), `src/policy/gates.rs` (+ 8 fields + `promote_min_confidence` helper), `src/policy/rules/anomaly.rs` (gate signature change, gate predicate change), `src/app/event_loop.rs` (gate call-site + ring-buffer push), `src/app/anomaly_events_ring.rs` (new), `src/app/anomaly_overlay.rs` (new — key + mouse handlers), `src/ui/anomaly_overlay.rs` (new — render), `src/app/dashboard_render.rs` (overlay branch), `src/app/tui_loop.rs` (construction + key dispatch), `src/ui/settings.rs` (Parameters tab section + Rules tab annotation update).
- No changes: provider adapters (`src/provider/*`), audit chain (`src/store/audit.rs`, `src/domain/audit.rs`), SignalSet schema, SQLite (`src/store/sqlite.rs`).
- Test counts: lib 1123 → ~1141, integration 57 → 58.

---

**End of design spec.**
