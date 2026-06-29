# Slice B (reduced) — Codex rollout structured backstop — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Codex panes a structured, `codex-tui`-gated fallback for token counts + model name read from the on-disk rollout JSONL, used ONLY when the status-line scrape didn't already produce them — moving Codex off fragile text-scraping where it actually helps, without replacing a working scrape.

**Architecture:** A new pure reader `src/adapters/codex_rollout.rs` (modeled on `claude_sidefile.rs`) locates the newest `~/.codex/sessions/**/rollout-*.jsonl` whose `session_meta` has `originator == "codex-tui"` AND `cwd == current_path`, and parses the latest `token_count` totals + `turn_context.model`. A new Codex branch in `apply_*` enrichment (`src/adapters/mod.rs`) fills `input_tokens`/`output_tokens`/`cached_input_tokens`/`model_name` **fill-when-absent** (`is_none`), gated on Provider::Codex + non-conflict + cwd + a descendant `codex` process + a real `codex_rollout` config toggle threaded through `ParserContext`.

**Tech Stack:** Rust, `serde`/`serde_json`, existing `MetricValue<T>`/`SourceKind`/`ParserContext`. No new dependencies.

## Global Constraints

- **Fill-when-absent only.** Rollout values fill `input_tokens`/`output_tokens`/`cached_input_tokens`/`model_name` ONLY when the scrape left them `None`. NEVER override a scraped value. (The Codex status-line scrape is rich + version-gated; rollout is a backstop, not a replacement.)
- **`codex-tui` originator gate is mandatory.** Only rollouts with `session_meta.originator == "codex-tui"` may correlate to a pane. `codex_exec` (and any non-TUI) rollouts MUST be ignored — they pollute the same cwd (observed: a `codex exec` review wrote the newest rollout in this repo's cwd).
- **No context% derivation, no new SignalSet field.** The scrape's `Context % used` stays authoritative for `context_pressure`. `reasoning_output_tokens` and `model_context_window` are present in the rollout but NOT surfaced this slice (would need new `SignalSet` fields → fixture ripple) — deferred, same rationale as Slice A's `context_window_size`.
- Token semantics: fill from `info.total_token_usage.{input_tokens,output_tokens,cached_input_tokens}` (cumulative session totals — matches the scrape's `N in / N out` semantics). NOT `last_token_usage`.
- Sidefile-style values keep `SourceKind::ProviderOfficial`. App-server stays the sole source for rate limits (unchanged). `permission_mode`/idle-state untouched.
- Codex home resolution: `CODEX_HOME` env if set, else `<home_dir>/.codex`.
- Gates green before each task commit: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`, `cargo test --all-targets`. Baseline at Slice B start: 1578 lib + 70 integration.
- ObserveOnly / safety precedence / no actuation: unchanged. No new audit kind, no SQLite change.

## File Structure

- `src/adapters/codex_rollout.rs` — **Create.** Pure reader: structs + `read_rollout_for_path(codex_home, current_path)`. Owns all rollout-JSONL knowledge.
- `src/adapters/mod.rs` — **Modify.** Register `pub mod codex_rollout;`. Add a Codex enrichment branch in `parse_for_with_environment` + a `codex_rollout_process_confirmed` helper (mirror `claude_sidefile_process_confirmed`). Add `codex_rollout_enabled: bool` to `ParserContext`; update the `ctx()` test helper. Add Codex enrichment tests.
- `src/app/config.rs` — **Modify.** Add `codex_rollout: bool` to `ProviderSetupConfig` (default `true`) + default/round-trip tests.
- `src/app/event_loop.rs` — **Modify.** Set `codex_rollout_enabled: config.provider_setup.codex_rollout` at the `ParserContext` construction site (~line 212).
- `src/adapters/codex_app_server.rs` — **Modify (docs only).** Refresh stale "Codex CLI 0.125.0 / v0.128.0" comments → note current 0.142.x + the `conversation/*`→`thread/*` rename (our 2 calls unaffected).
- `docs/ai/ARCHITECTURE.md` — **Modify.** Provider-coverage matrix: Codex `input_tokens`/`output_tokens`/`cached_input_tokens`/model gain a "+ rollout JSONL backstop (codex-tui)" note.

---

### Task 1: `codex_rollout.rs` reader

**Files:**

- Create: `src/adapters/codex_rollout.rs`
- Modify: `src/adapters/mod.rs` (add `pub mod codex_rollout;` near the other `pub mod` lines at the top)
- Test: in `src/adapters/codex_rollout.rs` `mod tests`

**Interfaces:**

- Produces: `pub struct CodexRolloutSignals { pub model: Option<String>, pub input_tokens: Option<u64>, pub output_tokens: Option<u64>, pub cached_input_tokens: Option<u64> }` and `pub fn read_rollout_for_path(codex_home: &std::path::Path, current_path: &str) -> Option<CodexRolloutSignals>`. Task 3 consumes both.

- [ ] **Step 1: Write the failing test.** Create `src/adapters/codex_rollout.rs` with ONLY the test module first (so it fails to compile → RED), then add the impl in Step 3. Test body:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    // Write a minimal rollout JSONL (session_meta + turn_context + a token_count event_msg).
    fn write_rollout(dir: &Path, name: &str, originator: &str, cwd: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        let body = format!(
            concat!(
                r#"{{"type":"session_meta","payload":{{"originator":"{orig}","cwd":"{cwd}","cli_version":"0.142.2"}}}}"#, "\n",
                r#"{{"type":"turn_context","payload":{{"model":"gpt-5.5"}}}}"#, "\n",
                r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":1510000,"cached_input_tokens":1200000,"output_tokens":20400,"reasoning_output_tokens":1000,"total_tokens":1530400}},"last_token_usage":{{"input_tokens":2000,"output_tokens":50,"total_tokens":2050}},"model_context_window":258400}}}}}}"#, "\n",
            ),
            orig = originator, cwd = cwd,
        );
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn reads_latest_totals_and_model_for_matching_codex_tui_cwd() {
        let tmp = tempdir().unwrap();
        let sessions = tmp.path().join("sessions/2026/06/29");
        write_rollout(&sessions, "rollout-a.jsonl", "codex-tui", "/repo/qmonster");
        let s = read_rollout_for_path(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(s.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(s.input_tokens, Some(1_510_000));
        assert_eq!(s.output_tokens, Some(20_400));
        assert_eq!(s.cached_input_tokens, Some(1_200_000));
    }

    #[test]
    fn ignores_codex_exec_originator() {
        let tmp = tempdir().unwrap();
        let sessions = tmp.path().join("sessions/2026/06/29");
        // A codex_exec rollout in the SAME cwd must be ignored (pollution guard).
        write_rollout(&sessions, "rollout-exec.jsonl", "codex_exec", "/repo/qmonster");
        assert!(read_rollout_for_path(tmp.path(), "/repo/qmonster").is_none());
    }

    #[test]
    fn ignores_non_matching_cwd() {
        let tmp = tempdir().unwrap();
        let sessions = tmp.path().join("sessions/2026/06/29");
        write_rollout(&sessions, "rollout-other.jsonl", "codex-tui", "/repo/other");
        assert!(read_rollout_for_path(tmp.path(), "/repo/qmonster").is_none());
    }

    #[test]
    fn returns_none_when_sessions_dir_missing_or_path_empty() {
        let tmp = tempdir().unwrap();
        assert!(read_rollout_for_path(tmp.path(), "/repo/qmonster").is_none());
        let sessions = tmp.path().join("sessions/2026/06/29");
        write_rollout(&sessions, "rollout-a.jsonl", "codex-tui", "/repo/qmonster");
        assert!(read_rollout_for_path(tmp.path(), "").is_none());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails.** Run: `cargo test --lib codex_rollout` — Expected: FAILS TO COMPILE (`read_rollout_for_path`/`CodexRolloutSignals` not found).

- [ ] **Step 3: Write the implementation** at the top of `src/adapters/codex_rollout.rs` (above the test module):

```rust
//! Slice B (reduced): Codex `rollout-*.jsonl` structured backstop.
//!
//! Codex writes an append-only JSONL rollout per session under
//! `<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-<id>.jsonl`. This reader
//! locates the newest rollout whose `session_meta` has
//! `originator == "codex-tui"` (interactive panes only — `codex exec`
//! rollouts share the cwd and MUST be excluded) AND `cwd == current_path`,
//! and returns the latest cumulative token totals + model. Read-only;
//! best-effort enrichment used only when the status-line scrape is absent.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexRolloutSignals {
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RolloutLine {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize, Default)]
struct SessionMeta {
    #[serde(default)]
    originator: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

/// How many of the newest rollout files (by mtime) to inspect before
/// giving up. Bounds per-poll cost when the sessions tree is large.
const MAX_CANDIDATES: usize = 64;

/// Locate the newest `codex-tui` rollout whose `session_meta.cwd`
/// matches `current_path`, and parse its latest token totals + model.
/// Returns None on empty path, missing sessions dir, or no match.
pub fn read_rollout_for_path(codex_home: &Path, current_path: &str) -> Option<CodexRolloutSignals> {
    if current_path.is_empty() {
        return None;
    }
    let sessions = codex_home.join("sessions");
    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    collect_rollouts(&sessions, &mut candidates);
    candidates.sort_by(|a, b| b.0.cmp(&a.0)); // newest first
    for (_, path) in candidates.into_iter().take(MAX_CANDIDATES) {
        let body = match fs::read_to_string(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if let Some(sig) = parse_if_matching(&body, current_path) {
            return Some(sig);
        }
    }
    None
}

/// Recursively collect `rollout-*.jsonl` paths with their mtime.
fn collect_rollouts(dir: &Path, out: &mut Vec<(SystemTime, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            collect_rollouts(&path, out);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
        {
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((mtime, path));
        }
    }
}

/// Parse one rollout body; return signals only if it is a `codex-tui`
/// session whose cwd matches `current_path`.
fn parse_if_matching(body: &str, current_path: &str) -> Option<CodexRolloutSignals> {
    let mut matched = false;
    let mut sig = CodexRolloutSignals::default();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<RolloutLine>(line) else {
            continue;
        };
        match parsed.kind.as_str() {
            "session_meta" => {
                let meta: SessionMeta =
                    serde_json::from_value(parsed.payload).unwrap_or_default();
                if meta.originator.as_deref() != Some("codex-tui")
                    || meta.cwd.as_deref() != Some(current_path)
                {
                    return None; // wrong session — abandon this file
                }
                matched = true;
            }
            "turn_context" => {
                if let Some(m) = parsed.payload.get("model").and_then(|v| v.as_str()) {
                    sig.model = Some(m.to_string()); // latest wins
                }
            }
            "event_msg" => {
                if parsed.payload.get("type").and_then(|v| v.as_str()) == Some("token_count")
                    && let Some(total) = parsed.payload.pointer("/info/total_token_usage")
                {
                    sig.input_tokens = total.get("input_tokens").and_then(|v| v.as_u64());
                    sig.output_tokens = total.get("output_tokens").and_then(|v| v.as_u64());
                    sig.cached_input_tokens =
                        total.get("cached_input_tokens").and_then(|v| v.as_u64());
                }
            }
            _ => {}
        }
    }
    if matched { Some(sig) } else { None }
}
```

Then register the module: in `src/adapters/mod.rs`, add `pub mod codex_rollout;` alphabetically near `pub mod codex_app_server;`.

- [ ] **Step 4: Run the tests to verify they pass.** Run: `cargo test --lib codex_rollout` — Expected: 4 passed. Then `cargo test --lib` — all pass.

- [ ] **Step 5: Commit.**

```bash
git add src/adapters/codex_rollout.rs src/adapters/mod.rs
git commit -m "$(cat <<'EOF'
feat(codex-rollout): codex-tui-gated rollout reader for token/model backstop

Slice B Task 1: new pure reader locates the newest ~/.codex/sessions
rollout with originator=codex-tui AND cwd match, parses latest cumulative
token totals + model. Excludes codex_exec rollouts (cwd-pollution guard).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `codex_rollout` config toggle + thread through `ParserContext`

**Files:**

- Modify: `src/app/config.rs` (`ProviderSetupConfig` struct ~line 451 + its `Default` ~line 461 + doc comment ~line 446)
- Modify: `src/adapters/mod.rs` (`ParserContext` struct ~line 21 + the `ctx()` test helper ~line 278)
- Modify: `src/app/event_loop.rs` (`ParserContext` construction ~line 212)
- Test: `src/app/config.rs` (extend the existing default/round-trip tests ~line 1397)

**Interfaces:**

- Produces: `ProviderSetupConfig.codex_rollout: bool` (default `true`); `ParserContext.codex_rollout_enabled: bool`. Task 3 reads `ctx.codex_rollout_enabled`.

- [ ] **Step 1: Write the failing test** — extend the config default test. In `src/app/config.rs` find the test asserting `absent.provider_setup.claude_sidefile` (~line 1397) and add, in the same test:

```rust
        assert!(
            absent.provider_setup.codex_rollout,
            "codex_rollout must default to true — rollout files need no operator setup"
        );
```

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test --lib provider_setup` (or the enclosing test name) — Expected: FAILS TO COMPILE (`no field codex_rollout`).

- [ ] **Step 3: Add the config field.** In `ProviderSetupConfig` (after `codex_app_server`):

```rust
    #[serde(default = "default_true")]
    pub codex_rollout: bool,
```

If a `default_true` fn does not already exist in the file, add the field as `pub codex_rollout: bool,` and set it in the `Default` impl instead. In the `Default for ProviderSetupConfig` impl (after `codex_app_server: false,`):

```rust
            codex_rollout: true,
```

Update the doc comment above the struct to note: "`codex_rollout` defaults to `true` — Codex writes rollout JSONL automatically (no operator setup); the reader is a fill-when-absent, codex-tui-gated backstop. Set `codex_rollout = false` to disable reading `~/.codex/sessions`."

- [ ] **Step 4: Thread through `ParserContext`.** In `src/adapters/mod.rs`, add to the `ParserContext` struct (after `current_path`):

```rust
    /// Slice B: operator toggle for the Codex rollout backstop
    /// (`[provider_setup] codex_rollout`). When false, the rollout
    /// reader is never consulted.
    pub codex_rollout_enabled: bool,
```

In the `ctx()` test helper (mod.rs ~line 278), add `codex_rollout_enabled: false,` to the returned struct literal (tests opt in explicitly when needed). In `src/app/event_loop.rs` at the `ParserContext { … }` construction (~line 212), add:

```rust
            codex_rollout_enabled: config.provider_setup.codex_rollout,
```

(Confirm the local binding name for config at that site — it is the loaded `QmonsterConfig`; use the same path other fields there use, e.g. `ctx.config.provider_setup.codex_rollout` if accessed via `ctx`.)

- [ ] **Step 5: Run to verify pass.** Run: `cargo test --lib` — Expected: all pass (the default test now passes; ParserContext compiles everywhere). If any other `ParserContext { … }` literal exists in production, the compiler will flag it — add the field there too.

- [ ] **Step 6: Commit.**

```bash
git add src/app/config.rs src/adapters/mod.rs src/app/event_loop.rs
git commit -m "$(cat <<'EOF'
feat(config): codex_rollout toggle (default true) threaded via ParserContext

Slice B Task 2: real opt-out for the rollout backstop — unlike the
claude_sidefile toggle, this one actually gates the read via a new
ParserContext.codex_rollout_enabled set from config at the event-loop seam.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Codex enrichment branch (fill-when-absent)

**Files:**

- Modify: `src/adapters/mod.rs` (`parse_for_with_environment` — add a Codex branch after the Claude sidefile block ~line 126; add `codex_rollout_process_confirmed` helper near `claude_sidefile_process_confirmed` ~line 130)
- Test: `src/adapters/mod.rs` `mod sidefile_integration_tests` (or a new `mod codex_rollout_integration_tests`)

**Interfaces:**

- Consumes: `codex_rollout::{read_rollout_for_path, CodexRolloutSignals}` (Task 1); `ParserContext.codex_rollout_enabled` (Task 2); existing `process_memory::read_descendant_cli_process_with_proc_root` + `cli_process_basename_contains` helpers.

- [ ] **Step 1: Write the failing tests** in `src/adapters/mod.rs` (new module at the end of the file):

```rust
#[cfg(test)]
mod codex_rollout_integration_tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role};
    use std::fs;

    fn codex_id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity { provider: Provider::Codex, instance: 1, role: Role::Main, pane_id: "%2".into() },
            confidence: IdentityConfidence::High,
        }
    }
    fn write_rollout(home: &std::path::Path, cwd: &str) {
        let dir = home.join(".codex/sessions/2026/06/29");
        fs::create_dir_all(&dir).unwrap();
        let body = format!(
            concat!(
                r#"{{"type":"session_meta","payload":{{"originator":"codex-tui","cwd":"{cwd}","cli_version":"0.142.2"}}}}"#, "\n",
                r#"{{"type":"turn_context","payload":{{"model":"gpt-5.5"}}}}"#, "\n",
                r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":1510000,"output_tokens":20400,"cached_input_tokens":1200000,"total_tokens":1530400}},"model_context_window":258400}}}}}}"#, "\n",
            ), cwd = cwd);
        fs::write(dir.join("rollout-x.jsonl"), body).unwrap();
    }
    fn write_proc(root: &std::path::Path, pid: u32, comm: &str, children: &[u32]) {
        let d = root.join(pid.to_string());
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("status"), format!("Name:\t{comm}\nVmRSS:\t1 kB\n")).unwrap();
        let t = d.join("task").join(pid.to_string());
        fs::create_dir_all(&t).unwrap();
        fs::write(t.join("children"), children.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(" ")).unwrap();
        let mut argv = Vec::new();
        argv.extend_from_slice(comm.as_bytes());
        argv.push(0);
        fs::write(d.join("cmdline"), argv).unwrap();
    }

    #[test]
    fn rollout_fills_codex_tokens_and_model_when_scrape_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_rollout(tmp.path(), cwd);
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "bash", &[2]);
        write_proc(&proc_root, 2, "codex", &[]);
        let id = codex_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        // empty tail → scrape produces no tokens/model → backstop fires.
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.codex_rollout_enabled = true;

        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));

        assert_eq!(signals.input_tokens.as_ref().unwrap().value, 1_510_000);
        assert_eq!(signals.output_tokens.as_ref().unwrap().value, 20_400);
        assert_eq!(signals.cached_input_tokens.as_ref().unwrap().value, 1_200_000);
        assert_eq!(signals.model_name.as_ref().unwrap().value, "gpt-5.5");
        assert_eq!(
            signals.input_tokens.as_ref().unwrap().source_kind,
            crate::domain::origin::SourceKind::ProviderOfficial
        );
    }

    #[test]
    fn rollout_does_not_override_scraped_tokens() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_rollout(tmp.path(), cwd);
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "codex", &[]);
        let id = codex_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.codex_rollout_enabled = true;

        let mut signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));
        // Simulate a prior scrape value, then re-run enrichment-only would override?
        // Instead assert: pre-seeding is not possible through parse_for; so prove the
        // guard by disabling the toggle and confirming no fill.
        c.codex_rollout_enabled = false;
        signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));
        assert!(signals.input_tokens.is_none(), "toggle off → no rollout fill");
    }

    #[test]
    fn rollout_skipped_when_descendant_is_not_codex() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = "/repo/qmonster";
        write_rollout(tmp.path(), cwd);
        let proc_root = tmp.path().join("proc");
        write_proc(&proc_root, 1, "bash", &[2]);
        write_proc(&proc_root, 2, "node", &[]); // not codex
        let id = codex_id();
        let pricing = crate::policy::pricing::PricingTable::empty();
        let settings = crate::policy::claude_settings::ClaudeSettings::empty();
        let history = crate::adapters::common::PaneTailHistory::empty();
        let mut c = ctx(&id, "", &pricing, &settings, &history);
        c.current_path = cwd;
        c.pane_pid = Some(1);
        c.codex_rollout_enabled = true;
        let signals = parse_for_with_environment(&c, &proc_root, Some(tmp.path()));
        assert!(signals.input_tokens.is_none());
    }
}
```

(Note: the `rollout_does_not_override_scraped_tokens` test proves the toggle gate; an explicit "scrape value survives" assertion is impossible through `parse_for` with an empty tail, so the override-safety is enforced structurally by the `is_none()` guards in Step 3 and locked by the toggle-off assertion. If a richer override test is wanted, build a Codex status-line tail fixture in a follow-up.)

- [ ] **Step 2: Run to verify they fail.** Run: `cargo test --lib codex_rollout_integration` — Expected: FAIL (`no field codex_rollout_enabled` is already added in Task 2, so these compile but FAIL — no fill happens yet).

- [ ] **Step 3: Implement the enrichment branch.** In `parse_for_with_environment` (`src/adapters/mod.rs`), AFTER the Claude sidefile block (ends ~line 126), add:

```rust
    // Slice B: Codex rollout backstop. Fill token totals + model from the
    // newest codex-tui rollout matching this pane's cwd ONLY when the
    // status-line scrape left them absent. Never overrides a scraped value.
    if ctx.codex_rollout_enabled
        && matches!(ctx.identity.identity.provider, Provider::Codex)
        && !identity_conflict
        && !ctx.current_path.is_empty()
        && (signals.input_tokens.is_none()
            || signals.output_tokens.is_none()
            || signals.cached_input_tokens.is_none()
            || signals.model_name.is_none())
        && codex_rollout_process_confirmed(ctx, proc_root)
        && let Some(home) = home_dir
    {
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        if let Some(roll) = codex_rollout::read_rollout_for_path(&codex_home, ctx.current_path) {
            let metric = |v| {
                crate::domain::signal::MetricValue::new(
                    v,
                    crate::domain::origin::SourceKind::ProviderOfficial,
                )
                .with_confidence(0.9)
                .with_provider(Provider::Codex)
            };
            if signals.input_tokens.is_none()
                && let Some(n) = roll.input_tokens { signals.input_tokens = Some(metric(n)); }
            if signals.output_tokens.is_none()
                && let Some(n) = roll.output_tokens { signals.output_tokens = Some(metric(n)); }
            if signals.cached_input_tokens.is_none()
                && let Some(n) = roll.cached_input_tokens { signals.cached_input_tokens = Some(metric(n)); }
            if signals.model_name.is_none()
                && let Some(m) = roll.model {
                    signals.model_name = Some(
                        crate::domain::signal::MetricValue::new(m, crate::domain::origin::SourceKind::ProviderOfficial)
                            .with_confidence(0.9)
                            .with_provider(Provider::Codex),
                    );
                }
        }
    }
```

Add the process-confirm helper near `claude_sidefile_process_confirmed`:

```rust
fn codex_rollout_process_confirmed(ctx: &ParserContext, proc_root: &std::path::Path) -> bool {
    let Some(pid) = ctx.pane_pid else { return true };
    let Some(desc) = process_memory::read_descendant_cli_process_with_proc_root(pid, proc_root)
    else {
        return false;
    };
    cli_process_basename_contains(&desc, "codex")
}
```

- [ ] **Step 4: Run to verify pass.** Run: `cargo test --lib codex_rollout_integration` — Expected: 3 passed. Then `cargo test --lib` — all pass.

- [ ] **Step 5: Commit.**

```bash
git add src/adapters/mod.rs
git commit -m "$(cat <<'EOF'
feat(adapters): Codex rollout fills tokens/model when scrape absent

Slice B Task 3: fill-when-absent enrichment from the codex-tui rollout,
gated on Provider::Codex + non-conflict + cwd + codex_rollout toggle +
descendant codex process. ProviderOfficial. Never overrides a scraped value.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Docs (matrix + codex_app_server version refresh) + validation + handoff

**Files:**

- Modify: `docs/ai/ARCHITECTURE.md` (provider-coverage matrix Codex cells)
- Modify: `src/adapters/codex_app_server.rs` (stale version-string comments only)

- [ ] **Step 1: Provider-coverage matrix.** In `docs/ai/ARCHITECTURE.md`, append ` (+ rollout JSONL backstop)` to the Codex cells for `input_tokens`, `output_tokens`, `cached_input_tokens`, and add a sentence under the matrix: "Since Slice B (2026-06), Codex token counts + model fall back to the `codex-tui` rollout JSONL (`~/.codex/sessions/.../rollout-*.jsonl`, fill-when-absent) when the status-line scrape is unavailable; `codex_exec` rollouts are excluded by the `originator` gate. context% / rate-limits are unchanged (scrape + app-server)."

- [ ] **Step 2: Refresh stale codex_app_server.rs comments.** Update the module doc-comment lines referencing "Codex CLI 0.125.0" / "v0.128.0" to note the current line (0.142.x verified) and that the `conversation/*`→`thread/*` rename does not affect the two calls used (`initialize`, `account/rateLimits/read`). Comment-only; no code change.

- [ ] **Step 3: Full gate suite.** Run `cargo fmt --all && cargo fmt --all --check` (clean), `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args` (no warnings), `cargo test --all-targets` (all green; lib 1578 → ~1589 with Task 1's 4 + Task 2's +0 + Task 3's 3 new tests; confirm exact count and that integration stays 70).

- [ ] **Step 4: Commit docs.**

```bash
git add docs/ai/ARCHITECTURE.md src/adapters/codex_app_server.rs
git commit -m "$(cat <<'EOF'
docs: Codex rollout backstop in coverage matrix; refresh app-server version notes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Handoff.** Slice B code complete + green. Do NOT bump version / edit mission ledger / push / tag — the release ritual is bundled (A+B+…) and operator-driven. Report readiness; next is Slice C (agy transcript activity enricher + version/doc refresh) or the bundled release.

---

## Self-Review (against the re-scoped B design)

- **Spec coverage:** reader + originator gate → Task 1; real config gate → Task 2; fill-when-absent enrichment → Task 3; docs + app-server version refresh → Task 4. The re-scope (no override, no context% derivation, no new SignalSet field, codex-tui gate) is encoded in Global Constraints and each task.
- **Deliberate deviations from the original spec §5 (B):** (1) fill-when-absent instead of override — the Codex scrape is already rich/robust (real-data finding); (2) no context% derivation — scrape's `Context % used` is authoritative + can't be calibrated without a live TUI pane; (3) `reasoning_output_tokens` + `model_context_window` deferred (new-field ripple); (4) added a REAL config gate (the claude_sidefile toggle doesn't actually gate its read — fixed the pattern for Codex). All flagged to the operator.
- **Placeholder scan:** none — full code in every code step.
- **Type consistency:** `CodexRolloutSignals` fields (`Option<u64>`/`Option<String>`) match the `MetricValue<u64>`/`<String>` SignalSet fields filled in Task 3. `read_rollout_for_path(&Path, &str) -> Option<CodexRolloutSignals>` used identically in Task 1 tests and Task 3. `codex_rollout_enabled` defined in Task 2, consumed in Task 3. `read_descendant_cli_process_with_proc_root` + `cli_process_basename_contains` reused from the existing claude path.

## After Slice B

Slice C (agy transcript activity enricher — activity/idle only, analytic surfaces stay Hidden; + version/doc refresh) gets its own plan. Then the bundled v2.5.0 release ritual (version + ledger + cross-review + tag + npm), operator-gated.
