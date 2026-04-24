# Slice 2 — Observability Field Expansion (v1.12.0)

- Status: **Approved** (2026-04-24)
- Scope: observe-first → populate `git_branch`, `worktree_path`, `reasoning_effort`, and Claude `model_name` on `SignalSet` via Codex status-bar / `/status` box parsing and Claude `settings.json` read.
- Precedes: Slice 3 (Gemini observability + optional session-level caching)
- Builds on: v1.11.0 Slice 1 (`docs/superpowers/specs/2026-04-23-p0-1-slice-1-usage-hint-parsing-design.md`), v1.11.2 remediation, v1.11.3 confirm-archive.
- Version target: `v1.12.0`.

## 1. Motivation

The v1.10.9 audit identified that the TUI blanked out most of vision 4 ("작업 관측"). Slice 1 (v1.11.0..v1.11.3) closed `context_pressure / token_count / model_name / cost_usd` on Codex panes and added `token_count` on Claude panes. Four operator-visible fields remain blank on the shipped TUI:

- `git_branch` — Codex status bar already emits it (the `main` token); nothing reads it.
- `worktree_path` — Codex status bar emits `~/project-dir`; nothing reads it either.
- `reasoning_effort` — Codex `/status` box emits `(reasoning xhigh, ...)`; no parser touches it.
- Claude `model_name` — intentionally left `None` in Slice 1 because the Claude tail does not expose a model. This slice reads `~/.claude/settings.json` as an **operator config surface** (same honesty class as `config/pricing.toml`) and populates model from there.

Both v1.11.0 reviewers (Codex + Gemini) endorsed this expansion as the natural Slice 2 and both asked to promote `ProviderParser::parse(.., pricing: &PricingTable)` to a `ParserContext` struct once a second cross-cutting input arrived. Slice 2's `ClaudeSettings` is that second input — the trigger to make the refactor.

## 2. Empirical evidence

**Codex status bar** (`~/.qmonster/archive/2026-04-23/_65/...log`), `·`-delimited positions:

```
pos 0: Context 73% left
pos 1: ~/Qmonster                          ← worktree_path (NEW, starts with ~ or /)
pos 2: gpt-5.4                             ← model_name (Slice 1)
pos 3: Qmonster                            ← project name (not captured)
pos 4: main                                ← git_branch (NEW)
pos 5: Context 27% used                    ← context_pressure (Slice 1)
...
```

**Codex `/status` box** (same archive):

```
│  Model:                       gpt-5.4 (reasoning xhigh, summaries auto)                 │
```

Regex: `reasoning (xhigh|high|medium|low|auto)` — extract the effort value. Note the box appears only after the operator runs `/status`; it is stale-prone.

**Claude `settings.json`**: standard location `~/.claude/settings.json`. Claude Code writes it at install / init time. The `model` key is optional — many operators rely on CLI-flag overrides instead. When the key is present, it is the authoritative default for the session's model.

## 3. Scope

### 3.1 IN (Slice 2)

| Change                                                                                                         | Location                                                                           |
| -------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `SignalSet` gains `git_branch`, `worktree_path`, `reasoning_effort` fields (all `Option<MetricValue<String>>`) | `src/domain/signal.rs`                                                             |
| `ClaudeSettings` new module + JSON loader + `AuditEventKind::ClaudeSettingsLoadFailed`                         | `src/policy/claude_settings.rs` (new), `src/domain/audit.rs`, `src/store/audit.rs` |
| `ParserContext<'a>` struct replaces 3-positional args on `ProviderParser::parse`                               | `src/adapters/mod.rs`                                                              |
| Codex status-bar parser extension — worktree + branch via pattern-matched positions                            | `src/adapters/codex.rs`                                                            |
| Codex `/status` box parser — `reasoning_effort` with confidence 0.6 (stale-aware)                              | `src/adapters/codex.rs`                                                            |
| Claude adapter reads `ctx.claude_settings.model()` for `model_name`                                            | `src/adapters/claude.rs`                                                           |
| UI: `metric_row` + `metric_badge_line` render the 3 new fields; badge line splits into 2 rows                  | `src/ui/panels.rs`, `src/ui/labels.rs`                                             |
| `Context.claude_settings` field + `with_claude_settings` builder; `main.rs` loads default path                 | `src/app/bootstrap.rs`, `src/main.rs`                                              |
| Integration-test Codex fixture module (unit tests keep inline fixture intentionally — see §5.3)                | `tests/fixtures/codex.rs` (new)                                                    |
| Unit + integration tests                                                                                       | 각 모듈, `tests/event_loop_integration.rs`                                         |

### 3.2 OUT (deferred or permanent non-goal)

| Item                                                               | Target                 | Reason                                                                                                    |
| ------------------------------------------------------------------ | ---------------------- | --------------------------------------------------------------------------------------------------------- |
| Codex-specific settings file reader (e.g., `~/.codex/config.toml`) | Slice 3                | Status bar already gives us every Codex signal we need; no value yet                                      |
| `reasoning_effort` session-level cache (SessionState)              | Slice 3+               | Slice 2 accepts stale values at confidence 0.6; cache adds type without proven need                       |
| `SystemNotice`-based live reload of `settings.json`                | Slice 3+               | Load once at startup; operators who edit settings.json mid-session see change on next run (documented)    |
| Gemini observability fields                                        | Slice 3                | No stable archive samples                                                                                 |
| Shell-out to `git branch` / `git rev-parse` for branch             | **permanent non-goal** | Violates observe-first; Codex's status bar is the only authorised source                                  |
| Writing to `settings.json`                                         | **permanent non-goal** | Non-destructive observer; operator owns the file                                                          |
| Claude tail → `model_name`                                         | **permanent non-goal** | Claude tail does not expose it; `claude_adapter_never_populates_model_name_from_tail` regression locks it |

## 4. Design

### 4.1 `SignalSet` schema

`src/domain/signal.rs`:

```rust
pub struct SignalSet {
    // ... existing 13 fields ...
    pub git_branch: Option<MetricValue<String>>,
    pub worktree_path: Option<MetricValue<String>>,
    pub reasoning_effort: Option<MetricValue<String>>,
}
```

**Cascade impact**: `parse_common_signals` in `src/adapters/common.rs` uses an exhaustive struct literal (the v1.11.0 "one cascade site" — deliberate per that round's reviewer feedback). The Slice 2 commit adds `git_branch: None`, `worktree_path: None`, `reasoning_effort: None` to that literal. All ~50 other `SignalSet { ..Default::default() }` sites cascade through `Default` derivation without touch.

### 4.2 `ClaudeSettings` module

`src/policy/claude_settings.rs` (new, ~120 LOC):

```rust
use crate::domain::identity::Provider;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ClaudeSettings {
    model: Option<String>,
    // other settings keys are ignored — we only surface `model` in Slice 2
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudeSettingsError {
    #[error("claude settings not found at {0}")]
    NotFound(PathBuf),
    #[error("failed to read claude settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse claude settings: {0}")]
    Parse(#[from] serde_json::Error),
}

impl ClaudeSettings {
    pub fn empty() -> Self { Self::default() }

    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/settings.json"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, ClaudeSettingsError> {
        let text = fs::read_to_string(path)?;
        let parsed: Self = serde_json::from_str(&text)?;
        Ok(parsed)
    }

    pub fn model(&self) -> Option<&str> { self.model.as_deref() }
}
```

No `load_*_or_empty` convenience helper is shipped (v1.11.2's pricing remediation learned that silent-swallow wrappers hide parse errors from the audit breadcrumb). `main.rs` performs the explicit error match shown below so parse failures route to the audit sink while file-absent stays silent.

**Loading from main.rs** — same pattern as pricing load (post-v1.11.2):

```rust
let claude_settings = match ClaudeSettings::default_path() {
    Some(path) => match ClaudeSettings::load_from_path(&path) {
        Ok(s) => s,
        Err(ClaudeSettingsError::Io(io)) if io.kind() == std::io::ErrorKind::NotFound => {
            ClaudeSettings::empty()  // silent default — not having settings.json is normal
        }
        Err(e) => {
            sink.record(AuditEvent {
                kind: AuditEventKind::ClaudeSettingsLoadFailed,
                pane_id: "n/a".into(),
                severity: Severity::Warning,
                summary: format!("claude settings load failed at {}: {}", path.display(), e),
                provider: None,
                role: None,
            });
            eprintln!(
                "qmonster: failed to load {}: {e}; claude model badge disabled this session",
                path.display()
            );
            ClaudeSettings::empty()
        }
    },
    None => ClaudeSettings::empty(),  // no $HOME
};
```

### 4.3 Audit kind addition

`src/domain/audit.rs`:

- Add `AuditEventKind::ClaudeSettingsLoadFailed` after `PricingLoadFailed`.
- Extend `as_str()` arm: `"ClaudeSettingsLoadFailed"`.
- No SQLite schema change (metadata-only).

`src/store/audit.rs`:

- Extend `parse_kind` inverse for round-trip symmetry.
- Extend exhaustive round-trip test (`parse_kind_inverts_as_str_for_every_variant`).
- Add `claude_settings_load_failed_audit_kind_roundtrips_through_sqlite` (mirrors v1.11.2's `pricing_load_failed_...` test).

### 4.4 `ParserContext` struct + trait migration

`src/adapters/mod.rs`:

```rust
use crate::policy::claude_settings::ClaudeSettings;
use crate::policy::pricing::PricingTable;
use crate::domain::identity::{Provider, ResolvedIdentity};
use crate::domain::signal::SignalSet;

pub struct ParserContext<'a> {
    pub identity: &'a ResolvedIdentity,
    pub tail: &'a str,
    pub pricing: &'a PricingTable,
    pub claude_settings: &'a ClaudeSettings,
}

pub trait ProviderParser {
    fn parse(&self, ctx: &ParserContext) -> SignalSet;
}

pub fn parse_for(ctx: &ParserContext) -> SignalSet {
    match ctx.identity.identity.provider {
        Provider::Claude => claude::ClaudeAdapter.parse(ctx),
        Provider::Codex => codex::CodexAdapter.parse(ctx),
        Provider::Gemini => gemini::GeminiAdapter.parse(ctx),
        Provider::Qmonster => qmonster::QmonsterAdapter.parse(ctx),
        Provider::Unknown => common::parse_common_signals(ctx.tail),
    }
}
```

**Caller update** (`src/app/event_loop.rs`):

```rust
let parse_ctx = crate::adapters::ParserContext {
    identity: &resolved,
    tail: &pane.tail,
    pricing: &ctx.pricing,
    claude_settings: &ctx.claude_settings,
};
let signals = crate::adapters::parse_for(&parse_ctx);
```

**Context extension** (`src/app/bootstrap.rs`):

```rust
pub struct Context<P: PaneSource, N: NotifyBackend> {
    // ... existing ...
    pub pricing: PricingTable,          // Slice 1
    pub claude_settings: ClaudeSettings, // NEW Slice 2
    known_pane_ids: Vec<String>,
}

impl<...> Context<P, N> {
    pub fn new(...) -> Self {
        Self {
            // ... existing ...
            claude_settings: ClaudeSettings::empty(),
            known_pane_ids: Vec::new(),
        }
    }

    pub fn with_claude_settings(mut self, s: ClaudeSettings) -> Self {
        self.claude_settings = s;
        self
    }
}
```

**`main.rs` wiring**: chain `.with_claude_settings(claude_settings)` after existing builder calls.

**Adapter signature migration** — all 4 adapters change `fn parse(&self, identity, tail, pricing)` to `fn parse(&self, ctx: &ParserContext)`. Body accesses `ctx.tail`, `ctx.pricing`, `ctx.claude_settings` as needed (or ignores). Unit tests construct a `ParserContext` inline.

### 4.5 Codex parser extension

`src/adapters/codex.rs`:

**`CodexStatus` struct** (existing + 3 new fields):

```rust
struct CodexStatus {
    context_pct: u8,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    model: Option<String>,            // Slice 1
    worktree_path: Option<String>,    // NEW Slice 2
    git_branch: Option<String>,       // NEW Slice 2
    reasoning_effort: Option<String>, // NEW Slice 2 (from /status box, not status bar)
}
```

**`parse_codex_status_line` extension** — still scans bottom-up for first line matching the shape heuristic (Context + `% used` + `·`). The token loop walks `line.split(" · ")` left-to-right (pos 0, 1, 2, ... in order of appearance) and uses the following per-token classifier, with **earliest-wins per field** and **project-name exclusion**:

- **`worktree_path`**: the first token matching `^[~/].*` (starts with `~` or absolute slash). Slice 1 already has this token in position 1; Slice 2 captures it.
- **`model`** (Slice 1, unchanged): first token whose first character-run matches `^(gpt-|claude-|gemini-)`.
- **`git_branch`**: the first token that (a) matches `^[a-zA-Z0-9/_.-]{1,60}$`, (b) appears in the token stream AFTER the token that matched `model`, (c) is NOT itself a model/cwd/context-pressure/quota/version/token-count/session-id token. Practically: after the model token matches, track a boolean `past_model`; on each subsequent plain-identifier token, the FIRST one that is not the project-name position (position immediately after model — skipped by setting a one-shot skip flag) populates `git_branch`.

**Project-name exclusion rule** (concrete implementation): once the `model` match succeeds, set `skip_next_plain_identifier = true`. On the next token, if it is a plain identifier AND `skip_next_plain_identifier` is true, consume the flag (set it to false) and do NOT populate `git_branch`. The subsequent plain-identifier token (position 4 in the current Codex format) populates `git_branch`.

If any of the 3 new fields fail their per-token test, they stay `None` on the `CodexStatus`. Per-field independence (v1.11.2 rule) preserved.

**`parse_codex_reasoning_effort` helper** — scans `tail` for any line matching the regex `reasoning (xhigh|high|medium|low|auto)` and returns the captured effort value (first match wins). This helper is called **inside `parse_codex_status_line`'s success path**, after the status bar tokens have populated their fields: `reasoning_effort` gets assigned to `status.reasoning_effort` before `Some(status)` is returned. When the status bar itself does not match the shape heuristic, `parse_codex_reasoning_effort` is not called at all — the whole `CodexStatus` returns `None` and `CodexAdapter::parse` falls through to `parse_common_signals`. Confidence **0.6** on the final `MetricValue` encodes stale-risk from `/status` block tail retention.

**`CodexAdapter::parse`** now emits:

```rust
set.worktree_path = status.worktree_path.map(|p| {
    MetricValue::new(p, SourceKind::ProviderOfficial)
        .with_confidence(0.95)
        .with_provider(Provider::Codex)
});
set.git_branch = status.git_branch.map(|b| {
    MetricValue::new(b, SourceKind::ProviderOfficial)
        .with_confidence(0.95)
        .with_provider(Provider::Codex)
});
set.reasoning_effort = status.reasoning_effort.map(|e| {
    MetricValue::new(e, SourceKind::ProviderOfficial)
        .with_confidence(0.6)  // stale-risk from /status box
        .with_provider(Provider::Codex)
});
```

### 4.6 Claude adapter extension

`src/adapters/claude.rs`:

```rust
impl ProviderParser for ClaudeAdapter {
    fn parse(&self, ctx: &ParserContext) -> SignalSet {
        let mut set = parse_common_signals(ctx.tail);

        // Claude-specific context_pressure heuristic (v1.11.1 stamps preserved)
        if let Some(p) = parse_context_percent_claude(&ctx.tail.to_lowercase()) {
            set.context_pressure = Some(
                MetricValue::new(p / 100.0, SourceKind::Estimated)
                    .with_confidence(0.6)
                    .with_provider(Provider::Claude),
            );
        }

        // v1.11.0 token_count from working line (unchanged)
        if let Some(n) = parse_claude_output_tokens(ctx.tail) {
            set.token_count = Some(
                MetricValue::new(n, SourceKind::ProviderOfficial)
                    .with_confidence(0.85)
                    .with_provider(Provider::Claude),
            );
        }

        // NEW Slice 2: model from external settings file, not from tail
        if let Some(m) = ctx.claude_settings.model() {
            set.model_name = Some(
                MetricValue::new(m.to_string(), SourceKind::ProviderOfficial)
                    .with_confidence(0.9)  // settings can be overridden by CLI flag
                    .with_provider(Provider::Claude),
            );
        }

        set
    }
}
```

**Confidence 0.9 < 0.95**: Claude settings can be overridden by CLI flag at invocation time (rare but documented). Codex model is rendered every frame on the status bar — higher confidence. Claude model read once at startup.

### 4.7 UI render

`src/ui/panels.rs`:

**`metric_row`** (text, `--once` surface) — extend with 3 new branches in order CTX → TOKENS → COST → MODEL → BRANCH → PATH → EFFORT:

```rust
if let Some(m) = s.git_branch.as_ref() {
    parts.push(format!("branch {} [{}]", m.value, source_kind_label(m.source_kind)));
}
if let Some(m) = s.worktree_path.as_ref() {
    parts.push(format!("path {} [{}]", m.value, source_kind_label(m.source_kind)));
}
if let Some(m) = s.reasoning_effort.as_ref() {
    parts.push(format!("effort {} [{}]", m.value, source_kind_label(m.source_kind)));
}
```

**`metric_badge_line`** — return type changes `Option<Line<'static>>` → `Vec<Line<'static>>`. Row 1 keeps existing 4 metrics (CTX/TOKENS/COST/MODEL); Row 2 (new) carries BRANCH/PATH/EFFORT when any is `Some`.

Implementation sketch:

```rust
fn metric_badge_line(signals: &SignalSet) -> Vec<Line<'static>> {
    let mut rows = Vec::with_capacity(2);
    if let Some(line) = primary_metric_row(signals) { rows.push(line); }
    if let Some(line) = context_metric_row(signals) { rows.push(line); }
    rows
}

fn primary_metric_row(signals: &SignalSet) -> Option<Line<'static>> {
    // existing body, factored out — CTX / TOKENS / COST / MODEL spans
}

fn context_metric_row(signals: &SignalSet) -> Option<Line<'static>> {
    // new — BRANCH / PATH / EFFORT spans, same theme::label_style() for non-severity metrics
}
```

**Caller update** (`render_pane` around line 282): iterate rows:

```rust
for row in metric_badge_line(&report.signals) {
    items.push(ListItem::new(row));
}
```

**Width awareness**: the existing v1.10.9 pre-wrap code does not run on `metric_badge_line` (it's for the help overlay). Row 2 at narrow viewports may wrap awkwardly, but each row is independent so the visual grouping stays readable even when wrapped. No additional pre-wrap work in this slice.

## 5. Testing

### 5.1 Unit tests

| Module                                       | Tests | Covers                                                                                                                                                                                                                                                |
| -------------------------------------------- | ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/policy/claude_settings.rs`              | 5     | `empty()`, `load_from_path` happy, missing `model` key, file absent, parse error + audit emitted                                                                                                                                                      |
| `src/domain/signal.rs`                       | 2     | 3 new fields default to None; `MetricValue<String>` round-trip for each                                                                                                                                                                               |
| `src/domain/audit.rs` + `src/store/audit.rs` | 2     | `ClaudeSettingsLoadFailed` `as_str`/`parse_kind` round-trip + SQLite round-trip                                                                                                                                                                       |
| `src/adapters/codex.rs`                      | 4     | (a) status bar populates worktree+branch, (b) pattern-mismatch → per-field None, (c) `/status` box populates reasoning_effort with confidence 0.6, (d) missing `/status` box → reasoning_effort None                                                  |
| `src/adapters/claude.rs`                     | 3     | (a) settings with `model` populates model_name (PO, 0.9, Claude), (b) `claude_adapter_never_populates_model_name_from_tail` regression (rename of v1.11.0 test; still locks tail-based detection absence), (c) cost stays None regardless of settings |
| `src/adapters/gemini.rs`, `qmonster.rs`      | 0     | Mechanical migration only                                                                                                                                                                                                                             |
| `src/ui/panels.rs`                           | 3     | `metric_row` renders 3 new fields; `metric_badge_line` returns 2 rows when context fields present; single-row fallback when only primary fields present                                                                                               |

### 5.2 Integration tests

`tests/event_loop_integration.rs`:

- `codex_status_line_end_to_end_populates_seven_metrics` — status bar + `/status` box in same tail. Asserts context, token_count, model_name, cost_usd, git_branch, worktree_path, reasoning_effort all Some with expected labels.
- `claude_adapter_end_to_end_reads_model_from_claude_settings` — `ClaudeSettings::load_from_path` via tempfile with `{"model": "claude-sonnet-4-6"}`; parse_for emits `model_name` with 0.9 confidence + ProviderOfficial.

### 5.3 Integration-test fixture module

New file `tests/fixtures/codex.rs` (integration-test only — `src/` unit tests keep their own inline fixtures intentionally so `src/adapters/codex.rs::mod tests` stays self-contained):

```rust
pub const CODEX_STATUS_FIXTURE_V0_122_0: &str = "\
Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · ...";

pub const CODEX_STATUS_BOX_FIXTURE: &str = "\
│  Model:                       gpt-5.4 (reasoning xhigh, summaries auto)                 │";
```

The two copies (unit-side inline fixture in `src/adapters/codex.rs` + integration fixture here) must be kept in sync when Codex CLI formats drift. The Slice 2 commit message for commit 8 calls this out; Slice 3 may promote to a single `pub(crate)` const re-exported into integration scope if the drift becomes a maintenance burden.

### 5.4 Expected test count

- v1.11.3 baseline: 307 tests (277 lib + 6 drift + 24 integration)
- Slice 2 additions: ~21 new tests (5 claude_settings + 2 signal + 2 audit + 4 codex + 3 claude + 3 panels + 2 integration)
- Target: **~328 tests green** at v1.12.0 ship

## 6. Version + Commit plan

**Target tag**: `v1.12.0` — minor bump (SignalSet schema + trait signature change + 4 newly populated fields).

**Commits** (8, TDD):

| #   | Message                                                                                           | Scope                                                                                                                                                  |
| --- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | `policy(v1.12.0-1): add ClaudeSettings module + settings.example.json + audit kind`               | `src/policy/claude_settings.rs`, `config/claude-settings.example.json`, `.gitignore`, `src/policy/mod.rs`, `src/domain/audit.rs`, `src/store/audit.rs` |
| 2   | `domain(v1.12.0-2): add git_branch + worktree_path + reasoning_effort fields to SignalSet`        | `src/domain/signal.rs`, `src/adapters/common.rs` (cascade update)                                                                                      |
| 3   | `adapters(v1.12.0-3): migrate ProviderParser to ParserContext struct`                             | `src/adapters/mod.rs`, `src/adapters/{claude,codex,gemini,qmonster}.rs`, `src/app/event_loop.rs`, `src/app/bootstrap.rs`, `src/main.rs`                |
| 4   | `adapters(v1.12.0-4): codex status bar worktree + branch extraction`                              | `src/adapters/codex.rs`                                                                                                                                |
| 5   | `adapters(v1.12.0-5): codex /status box reasoning effort parser`                                  | `src/adapters/codex.rs`                                                                                                                                |
| 6   | `adapters(v1.12.0-6): claude model_name from ClaudeSettings`                                      | `src/adapters/claude.rs`                                                                                                                               |
| 7   | `ui(v1.12.0-7): 2-row metric_badge_line + render new fields`                                      | `src/ui/panels.rs`, `src/ui/labels.rs`                                                                                                                 |
| 8   | `test(v1.12.0-8): integration tests for end-to-end seven-metric populate + claude settings model` | `tests/event_loop_integration.rs`, `tests/fixtures/codex.rs` (new)                                                                                     |

**Annotated tag**: `v1.12.0` at commit 8.

**State update**: `.mission/CURRENT_STATE.md` Mission/Phase lines updated (gitignored).

**Review cycle** — project convention applies. Expected sequence:

- v1.12.0 ship → Codex + Gemini cross-check → likely `approve-with-fixes` (larger slice than v1.11.0; 1-3 must-fix expected).
- v1.12.1 remediation → re-review → `approve`.
- v1.12.2 confirm-archive (ledger-only).

## 7. Acceptance criteria

- [ ] `cargo fmt --check` clean
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] 전체 테스트 green (~328 expected)
- [ ] `config/claude-settings.example.json` 배포 + `.gitignore`에 `config/claude-settings.json` 추가
- [ ] 실세션 Codex pane TUI에 Row 1 (CTX/TOKENS/COST/MODEL) + Row 2 (BRANCH/PATH/EFFORT) 표시 확인 (operator 육안)
- [ ] 실세션 Claude pane TUI에 `MODEL` 배지 등장 (settings.json에 model 키 있을 때) — 없으면 honest None (우린 mechanism만 검증, 실제 키 존재는 operator 환경 의존)
- [ ] settings.json parse 실패 시 SQLite audit에 `ClaudeSettingsLoadFailed` 기록 (smoke test)
- [ ] `.mission/CURRENT_STATE.md` v1.12.0 요약으로 갱신
- [ ] Codex + Gemini 둘 다 `approve` (remediation 라운드 통과 포함)

## 8. Risks / unknowns

1. **`~/.claude/settings.json` `model` 키 실제 존재 여부 불확실** — 많은 운영자가 CLI flag override만 쓰고 `model`을 settings에 고정하지 않음. 최악의 경우 Slice 2 후에도 Claude `model_name`이 대부분 환경에서 None (v1.11.2와 동일 체감). Mechanism은 구축됨 — operator가 settings 편집하면 동작. Acceptance criteria 5번째 item에서 명시적으로 수용.

2. **Codex `/status` 박스 포맷 drift** — v0.122.0 기준 정규식 매치. CLI 업데이트 시 silent degradation (reasoning_effort = None). `codex_adapter_reasoning_effort_falls_through_when_pattern_absent` 테스트가 regression 가드.

3. **`git_branch` 포지션 drift** — Codex 상태 바에서 branch는 pos 4. CLI가 토큰 하나 끼워넣으면 project name이 branch로 잘못 잡힐 수 있음. 다행히 project name은 `main`이 아니라 repo 이름이라 의미적으로 틀림. 첫 번째 리뷰 라운드에서 flag될 수 있는 약점 — "Slice 2 remediation에서 패턴-기반 branch detection으로 강화"가 예상 remediation 방향.

4. **`metric_badge_line` 반환 타입 변경 → caller migration** — `render_pane`의 기존 `.map(|line| items.push(ListItem::new(line)))` 체인이 있다면 `Vec` iteration으로 전환. 기존 unit 테스트 `.unwrap()` → `.first()` 전환.

5. **`AuditEventKind` 22번째 변형** — 추가됨. SQLite schema 변경 없음 (metadata text). v1.11.2의 `PricingLoadFailed` 패턴 그대로.

6. **Stale `reasoning_effort` confidence 0.6 의미** — operator에게 "자주 stale될 수 있음"을 어떻게 전달? UI 배지에서 `[Official]` 라벨이 일정해서 confidence 숫자는 노출 안 됨. 향후 리뷰에서 "stale source_kind 별도 추가해야 하나" 논점 가능. Slice 2는 confidence 값으로만 encode; Slice 3에서 UI 세부 갱신 여부 재검토.

## 9. References

- 이전 슬라이스: `docs/superpowers/specs/2026-04-23-p0-1-slice-1-usage-hint-parsing-design.md` (v1.11.0 Slice 1)
- v1.11.0 리뷰 라운드: `.mission/evals/Qmonster-v0.4.0-2026-04-24-v1.11.0-review.result.yaml` (Codex), `...-v1.11.0-gemini-review.result.yaml` (Gemini)
- v1.11.2 remediation confirm: `.mission/evals/Qmonster-v0.4.0-2026-04-24-v1.11.2-confirm-*.result.yaml`
- 실세션 아카이브: `~/.qmonster/archive/2026-04-23/_65/`
- Codex 상태 바 + `/status` 박스 포맷 샘플: §2 위
- 기존 UI 렌더 함수: `src/ui/panels.rs::metric_row`, `::metric_badge_line`
- `MetricValue<T>` + `SourceKind` 분류: `src/domain/signal.rs`, `src/domain/origin.rs`
- Similar schema-expansion slice precedent: Phase 4 P4-1 profile field addition, Slice 1's `model_name` field addition
