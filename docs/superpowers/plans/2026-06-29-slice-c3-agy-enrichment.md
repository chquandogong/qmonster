# Slice C3 — agy structured/scrape enrichment — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface `model_name` + `context_pressure`/`context_window_size` +
`token_count` for agy (Antigravity CLI) panes via a hybrid structured-sidefile +
footer-scrape enricher, reusing existing `SignalSet` fields, opt-in, without
breaking the v2.4.0 ObserveOnly contract.

**Architecture:** A new agy enrichment branch in `parse_for_with_environment`
(mirroring the Slice B Codex-rollout branch) fills three existing `SignalSet`
fields. Footer scrape (`agy_footer::parse_agy_footer`, always pane-correct)
fills-when-absent; the structured sidefile (`agy_sidefile::read_agy_sidefile_for_path`,
mirroring `claude_sidefile`) overrides when present and unambiguous. Gated by a
new `[provider_setup] agy_enrichment` toggle (default false) + the existing
strict agy descendant-process confirm.

**Tech Stack:** Rust 2024 (rust-version 1.88), serde/serde_json, tmux
`capture-pane` (already wired via `ctx.tail`), `tempfile`/`filetime` (dev). No
new dependencies.

## Global Constraints

- **ObserveOnly preserved (byte-identical).** The v2.4.0 six-gate contract holds:
  agy stays excluded from anomalies (`AnomalyKind::supports_provider`),
  `provider_honesty` (Hidden), `profile_switch` (None), insights coverage
  ("unsupported"), and the token-sample filter. Populating `token_count` for
  display MUST NOT add agy to `token_rows_supported` or any token-sample / insights
  / anomaly / actuation surface.
- **Recommend-first / no config writes.** Qmonster never writes the user's agy
  `settings.json`; it only _shows_ a recommended `statusLine.command` block and
  _reads_ the resulting sidefile.
- **Reuse existing `SignalSet` fields only** — `model_name`, `context_pressure`,
  `context_window_size`, `token_count`. No new `SignalSet` field.
- **No fabrication** — a field is `Some` only when a real source provided it.
- **Opt-in** — `[provider_setup] agy_enrichment` default `false`.
- **SourceKind = `ProviderOfficial`**, `.with_confidence(0.9).with_provider(Provider::Antigravity)`
  for both paths (matches the Slice B Codex-rollout metric idiom).
- **Precedence** — footer fills-when-absent; sidefile overrides (sets even if
  footer/common already set the field) when present and ambiguity-guard passes.
- **Validation per task:** `cargo fmt --all --check` + `cargo clippy --all-targets
-- -D warnings -A clippy::uninlined_format_args` + `cargo test --all-targets`.
  Any task touching `ParserContext` or config (a shared struct) MUST run
  `--all-targets`, not just `--lib` (the `qmonster-review_flow` rule).

---

## File Structure

| File                                                             | Responsibility                                                                                                                                                                              | Change             |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ |
| `src/adapters/agy_footer.rs`                                     | Parse model/context/token from the agy pane's captured footer text.                                                                                                                         | **Create**         |
| `src/adapters/agy_sidefile.rs`                                   | Read the structured agy sidefile JSON (cwd-matched, distinct-conversation ambiguity guard).                                                                                                 | **Create**         |
| `src/adapters/mod.rs`                                            | Register the two modules; add the agy enrichment branch; add `ParserContext.agy_enrichment_enabled`; rename `agy_transcript_process_confirmed` → `agy_process_confirmed` (shared by C2+C3). | **Modify**         |
| `src/app/config.rs`                                              | `ProviderSetupConfig.agy_enrichment` (default false).                                                                                                                                       | **Modify**         |
| `src/app/event_loop.rs:221`                                      | Thread `agy_enrichment` config → `ParserContext`.                                                                                                                                           | **Modify**         |
| `src/adapters/agent_memory.rs`, `src/adapters/process_memory.rs` | Add `agy_enrichment_enabled: false` to their test `ParserContext` literals (struct-field ripple).                                                                                           | **Modify (tests)** |
| `src/ui/provider_setup.rs`                                       | agy tab: recommended `statusLine.command` block in `snippet_for_tab` + `append_copy_contract` + `render_tab_content`.                                                                       | **Modify**         |
| `docs/ai/ARCHITECTURE.md`, `docs/ai/UI_MANUAL.md`                | Provider-coverage matrix agy row + Provider Setup agy-tab note.                                                                                                                             | **Modify (docs)**  |

---

## Task 0: Spike — confirm the agy `statusLine.command` input contract

**This is an investigation task, not TDD.** It produces a findings note and a
go/no-go for the _structured_ sidefile path (Tasks 3 + the sidefile half of 4 +
the recommended-block jq paths in 5). The _scrape_ path (Tasks 1, 2, 4-footer, 5
display, 6) is unconditional and proceeds regardless.

**Files:** none in `src/`. Writes findings to
`.docs/claude/Qmonster-2026-06-29-agy-statusline-spike.md` (gitignored scratch).

- [ ] **Step 1: Pick a safe, auth-preserving instrumentation.** agy needs
      `~/.gemini/antigravity-cli/oauth_creds.json` to run, so a bare temp `HOME`
      fails. In order of preference: (a) an agy config-dir/settings override env var
      if one exists (`agy --help` / `env | grep -i gemini`); else (b) copy
      `~/.gemini` into an isolated dir, point agy at it, modify only that copy's
      `statusLine`; else (c) back up `~/.gemini/antigravity-cli/settings.json`, patch
      `statusLine`, run, restore under a `trap`. **Never** copy/commit
      `oauth_creds.json` into the repo or persistent scratch.

- [ ] **Step 2: Instrument `statusLine.command` with a dump script.** Set
      `statusLine` to `{ "type": "command", "command": "<dump>", "enabled": true }`
      where `<dump>` writes stdin + `env` + `$@` to a capture file, then echoes a
      dummy line so agy's status line still renders. Example dump script:

```bash
#!/usr/bin/env bash
cap="$HOME/agy-statusline-capture.txt"
{ echo "=== ARGV ==="; printf '%s\n' "$@"
  echo "=== ENV (gemini/agy/model/token/context) ==="; env | grep -iE 'gemini|agy|model|token|context|quota|session'
  echo "=== STDIN ==="; cat; } >> "$cap"
echo "spike"   # agy renders this as the status line
```

- [ ] **Step 3: Trigger the status line headlessly.** Run a single headless turn
      so the status line fires without touching the user's interactive panes:

Run: `agy --print "say hi" </dev/null`
Expected: completes; `~/agy-statusline-capture.txt` is non-empty.

- [ ] **Step 4: Inspect the capture and decide.** Record in the findings note:
      (a) the input channel (stdin JSON? argv? env?); (b) the exact JSON keys present
      and whether they include model / context-used / context-window / token-count;
      (c) **go/no-go**: if agy passes a parseable JSON carrying at least the model,
      the structured path is GO — note the exact key paths for the Task-5 recommended
      block's `jq` expressions and the Task-3 `AgySidefile` field mapping. If agy
      passes **no** usable data to the command, mark the structured path NO-GO:
      Tasks 3 and the sidefile half of Task 4 are SKIPPED, the Task-5 block is omitted,
      and the slice ships scrape-only (still complete and useful). Either way, restore
      any modified config and delete the capture file.

- [ ] **Step 5: Record findings.** Write the channel, key paths, and go/no-go
      decision to `.docs/claude/Qmonster-2026-06-29-agy-statusline-spike.md`. No
      `src/` commit (investigation only).

---

## Task 1: `agy_enrichment` toggle — config + ParserContext plumbing

**Files:**

- Modify: `src/app/config.rs:454-479` (`ProviderSetupConfig` + `Default`)
- Modify: `src/app/config.rs` (default-assertion test near line 1406)
- Modify: `src/adapters/mod.rs:42-50` (`ParserContext`), `:461-462` (`ctx()` test builder)
- Modify: `src/app/event_loop.rs:220-221` (config → ParserContext wiring)
- Modify: `src/adapters/agent_memory.rs:320-321,372-373` and
  `src/adapters/process_memory.rs:456-457,503-504,554-555` (test `ParserContext` literals)

**Interfaces:**

- Produces: `ProviderSetupConfig.agy_enrichment: bool` (default `false`);
  `ParserContext.agy_enrichment_enabled: bool`.

- [ ] **Step 1: Write the failing config default test.** Add to the
      `ProviderSetupConfig` tests in `src/app/config.rs` (next to the existing
      `agy_transcript` default assertion ~line 1421):

```rust
#[test]
fn provider_setup_agy_enrichment_defaults_off() {
    let absent = AppConfig::default();
    assert!(
        !absent.provider_setup.agy_enrichment,
        "agy_enrichment must default to false — opt-in scrape/sidefile enrichment"
    );
}
```

- [ ] **Step 2: Run it to confirm it fails.**

Run: `cargo test --lib provider_setup_agy_enrichment_defaults_off`
Expected: FAIL — `no field 'agy_enrichment' on type '&ProviderSetupConfig'`.

- [ ] **Step 3: Add the field + default.** In `src/app/config.rs`, add to
      `ProviderSetupConfig` (after `agy_transcript`):

```rust
    /// agy_enrichment defaults to false — opt-in. When true, agy panes are
    /// enriched with model / context% / token-count from the agy footer scrape
    /// and (if the operator applied the recommended statusLine.command block)
    /// the agy sidefile. Display-only; the v2.4.0 ObserveOnly gates are unchanged.
    pub agy_enrichment: bool,
```

And in `impl Default for ProviderSetupConfig` (after `agy_transcript: false,`):

```rust
            agy_enrichment: false,
```

- [ ] **Step 4: Add the ParserContext field.** In `src/adapters/mod.rs`
      `ParserContext` (after `agy_transcript_enabled`):

```rust
    /// Slice C3: operator toggle for agy footer/sidefile enrichment
    /// (`[provider_setup] agy_enrichment`). When false, no agy enrichment runs.
    pub agy_enrichment_enabled: bool,
```

Then set `agy_enrichment_enabled: false,` in the test `ctx()` builder
(`src/adapters/mod.rs` ~line 462), and in every other `ParserContext { … }`
literal so the crate compiles: `src/adapters/agent_memory.rs` (2 sites),
`src/adapters/process_memory.rs` (3 sites). Wire the real value in
`src/app/event_loop.rs` (after line 221):

```rust
            agy_enrichment_enabled: ctx.config.provider_setup.agy_enrichment,
```

- [ ] **Step 5: Run the full gate (shared-struct change → `--all-targets`).**

Run: `cargo test --all-targets provider_setup_agy_enrichment_defaults_off` then
`cargo build --all-targets`
Expected: PASS; crate compiles (all `ParserContext` literals updated).

- [ ] **Step 6: Commit.**

```bash
git add src/app/config.rs src/adapters/mod.rs src/app/event_loop.rs src/adapters/agent_memory.rs src/adapters/process_memory.rs
git commit -m "feat(config): agy_enrichment toggle (default false) via ParserContext"
```

---

## Task 2: agy footer scrape parser (`agy_footer.rs`)

**Files:**

- Create: `src/adapters/agy_footer.rs`
- Modify: `src/adapters/mod.rs` (add `pub mod agy_footer;` near the other `pub mod` lines)

**Interfaces:**

- Produces: `pub struct AgyFooter { pub model: Option<String>, pub context_used_pct: Option<f32>, pub context_window: Option<u64>, pub token_count: Option<u64> }`
  and `pub fn parse_agy_footer(tail: &str) -> AgyFooter`.

- [ ] **Step 1: Write the failing tests.** Create `src/adapters/agy_footer.rs`
      with a `#[cfg(test)] mod tests` only (impl stubs return `Default`). Sample text
      matches the live agy footer (`? for shortcuts … Gemini 3.5 Flash (High)`) plus
      the optional `context-used` / `token-count` footer items when enabled:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_model_from_footer() {
        let tail = "> hello\n? for shortcuts                         Gemini 3.5 Flash (High)\n";
        let f = parse_agy_footer(tail);
        assert_eq!(f.model.as_deref(), Some("Gemini 3.5 Flash (High)"));
    }

    #[test]
    fn parses_context_and_token_items_when_present() {
        // Footer with context-used + token-count items enabled (showLabels on).
        let tail = "context-used 37%  token-count 34567  Gemini 3.1 Pro (High)\n";
        let f = parse_agy_footer(tail);
        assert_eq!(f.context_used_pct, Some(0.37));
        assert_eq!(f.context_window, None); // window size not shown in footer
        assert_eq!(f.token_count, Some(34567));
        assert_eq!(f.model.as_deref(), Some("Gemini 3.1 Pro (High)"));
    }

    #[test]
    fn absent_items_are_none() {
        let f = parse_agy_footer("> just a prompt, no footer line\n");
        assert!(f.model.is_none() && f.context_used_pct.is_none() && f.token_count.is_none());
    }
}
```

- [ ] **Step 2: Run to confirm failure.**

Run: `cargo test --lib agy_footer::tests`
Expected: FAIL — assertions fail (stub returns all `None`).

- [ ] **Step 3: Implement the parser.** Replace the stub with:

```rust
//! Slice C3: agy (Antigravity CLI) footer scrape.
//!
//! The agy TUI renders a configurable footer (model-name / context-used /
//! token-count items; see `~/.gemini/.../settings.json` `ui.footer.items`). The
//! captured pane tail therefore carries the model and, when those items are
//! enabled, context% and token-count as text. This parser extracts them; values
//! originate from agy (ProviderOfficial); each field is independent and best-effort.

/// Recognized Gemini model family prefixes shown in the agy footer/header
/// (e.g. "Gemini 3.5 Flash (High)", "Gemini 3.1 Pro (Low)").
const MODEL_PREFIXES: &[&str] = &["Gemini ", "Claude ", "GPT-OSS "];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgyFooter {
    pub model: Option<String>,
    pub context_used_pct: Option<f32>,
    pub context_window: Option<u64>,
    pub token_count: Option<u64>,
}

/// Parse the agy footer text out of a captured pane tail. Best-effort: any field
/// the footer does not carry stays `None`. `context_window` is never present in
/// the footer (the footer shows a percentage, not the window size) — it is only
/// ever filled by the sidefile path.
pub fn parse_agy_footer(tail: &str) -> AgyFooter {
    let mut out = AgyFooter::default();
    for raw in tail.lines() {
        let line = raw.trim();
        // model: the right-most occurrence of a known model family + "(level)".
        if out.model.is_none()
            && let Some(m) = extract_model(line)
        {
            out.model = Some(m);
        }
        if out.context_used_pct.is_none()
            && let Some(p) = extract_labeled_pct(line, "context-used")
        {
            out.context_used_pct = Some(p);
        }
        if out.token_count.is_none()
            && let Some(n) = extract_labeled_count(line, "token-count")
        {
            out.token_count = Some(n);
        }
    }
    out
}

fn extract_model(line: &str) -> Option<String> {
    let start = MODEL_PREFIXES
        .iter()
        .filter_map(|p| line.find(p))
        .min()?;
    let candidate = line[start..].trim();
    // Require the "(level)" suffix the agy footer always renders so we don't grab
    // prose mentioning "Gemini".
    if candidate.contains('(') && candidate.ends_with(')') {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn extract_labeled_pct(line: &str, label: &str) -> Option<f32> {
    let idx = line.find(label)? + label.len();
    let rest = line[idx..].trim_start();
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num.is_empty() {
        return None;
    }
    let pct: f32 = num.parse().ok()?;
    Some((pct / 100.0).clamp(0.0, 1.0))
}

fn extract_labeled_count(line: &str, label: &str) -> Option<u64> {
    let idx = line.find(label)? + label.len();
    let rest = line[idx..].trim_start();
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num.is_empty() {
        return None;
    }
    num.parse().ok()
}
```

- [ ] **Step 4: Register the module.** In `src/adapters/mod.rs`, add near the
      other `pub mod` declarations:

```rust
pub mod agy_footer;
```

- [ ] **Step 5: Run to confirm pass.**

Run: `cargo test --lib agy_footer::tests`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit.**

```bash
git add src/adapters/agy_footer.rs src/adapters/mod.rs
git commit -m "feat(adapters): agy footer scrape parser (model/context/token)"
```

---

## Task 3: agy structured sidefile reader (`agy_sidefile.rs`)

> **Spike-gated:** implement only if Task 0 returned GO (agy passes usable data
> to `statusLine.command`). If NO-GO, skip this task and the sidefile half of
> Task 4; the slice ships scrape-only. The `AgySidefile` output schema below is
> Qmonster's own (the recommended block writes it), so it is fixed regardless of
> agy's input contract; Task 0 only confirms the recommended block _can_ populate it.

**Files:**

- Create: `src/adapters/agy_sidefile.rs`
- Modify: `src/adapters/mod.rs` (add `pub mod agy_sidefile;`)

**Interfaces:**

- Produces: `pub struct AgySidefile { conversation_id, cwd, model, context_used_percentage, context_window_size, token_count }`
  (all `Option`) and `pub fn read_agy_sidefile_for_path(home: &Path, current_path: &str) -> Option<AgySidefile>`.

- [ ] **Step 1: Write the failing tests** (mirror `claude_sidefile.rs` tests —
      cwd match, distinct-conversation ambiguity → None, newest-wins, dir-missing →
      None, malformed skipped). Create `src/adapters/agy_sidefile.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};
    use tempfile::tempdir;

    fn write(dir: &Path, cid: &str, body: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let p = dir.join(format!("{cid}.json"));
        fs::write(&p, body).unwrap();
        p
    }
    fn agy_dir(home: &Path) -> PathBuf {
        home.join(".local/share/ai-cli-status/agy")
    }

    #[test]
    fn none_when_dir_missing() {
        let tmp = tempdir().unwrap();
        assert!(read_agy_sidefile_for_path(tmp.path(), "/repo").is_none());
    }

    #[test]
    fn matches_by_cwd() {
        let tmp = tempdir().unwrap();
        write(&agy_dir(tmp.path()), "a", r#"{"cwd":"/repo/a","conversation_id":"a","model":"Gemini 3.5 Flash (High)"}"#);
        write(&agy_dir(tmp.path()), "b", r#"{"cwd":"/repo/b","conversation_id":"b"}"#);
        let s = read_agy_sidefile_for_path(tmp.path(), "/repo/a").expect("cwd match");
        assert_eq!(s.conversation_id.as_deref(), Some("a"));
        assert_eq!(s.model.as_deref(), Some("Gemini 3.5 Flash (High)"));
    }

    #[test]
    fn none_for_concurrent_same_cwd_distinct_conversations() {
        let tmp = tempdir().unwrap();
        let a = write(&agy_dir(tmp.path()), "a", r#"{"cwd":"/repo","conversation_id":"a"}"#);
        let b = write(&agy_dir(tmp.path()), "b", r#"{"cwd":"/repo","conversation_id":"b"}"#);
        let now = SystemTime::now();
        filetime::set_file_mtime(&a, filetime::FileTime::from_system_time(now - Duration::from_secs(5))).unwrap();
        filetime::set_file_mtime(&b, filetime::FileTime::from_system_time(now)).unwrap();
        assert!(read_agy_sidefile_for_path(tmp.path(), "/repo").is_none());
    }

    #[test]
    fn newest_wins_when_same_conversation_within_window() {
        let tmp = tempdir().unwrap();
        let a = write(&agy_dir(tmp.path()), "old", r#"{"cwd":"/repo","conversation_id":"same","token_count":1}"#);
        let b = write(&agy_dir(tmp.path()), "new", r#"{"cwd":"/repo","conversation_id":"same","token_count":2}"#);
        let now = SystemTime::now();
        filetime::set_file_mtime(&a, filetime::FileTime::from_system_time(now - Duration::from_secs(5))).unwrap();
        filetime::set_file_mtime(&b, filetime::FileTime::from_system_time(now)).unwrap();
        let s = read_agy_sidefile_for_path(tmp.path(), "/repo").expect("same conversation → newest");
        assert_eq!(s.token_count, Some(2));
    }

    #[test]
    fn parses_full_shape_and_skips_malformed() {
        let tmp = tempdir().unwrap();
        write(&agy_dir(tmp.path()), "broken", "not json {");
        write(&agy_dir(tmp.path()), "ok", r#"{"cwd":"/repo","conversation_id":"ok","model":"Gemini 3.1 Pro (High)","context_used_percentage":37.5,"context_window_size":1048576,"token_count":34567}"#);
        let s = read_agy_sidefile_for_path(tmp.path(), "/repo").expect("malformed must not block");
        assert_eq!(s.conversation_id.as_deref(), Some("ok"));
        assert_eq!(s.context_used_percentage, Some(37.5));
        assert_eq!(s.context_window_size, Some(1048576));
        assert_eq!(s.token_count, Some(34567));
    }
}
```

- [ ] **Step 2: Run to confirm failure.**

Run: `cargo test --lib agy_sidefile::tests`
Expected: FAIL — `read_agy_sidefile_for_path` not defined.

- [ ] **Step 3: Implement the reader** (cwd-matched, distinct-`conversation_id`
      ambiguity guard with `AMBIGUITY_WINDOW = 60s`, newest-by-mtime — the same
      structure as `claude_sidefile::read_sidefile_for_path`):

```rust
//! Slice C3: agy structured sidefile reader.
//!
//! When the operator applies the recommended agy `statusLine.command` block
//! (shown in the Provider Setup overlay), every status-line refresh dumps a
//! per-conversation JSON to `~/.local/share/ai-cli-status/agy/<conversation_id>.json`
//! in the schema below. Qmonster does not see the conversation id directly, so a
//! sidefile is matched to a pane by its `cwd` equalling the pane's current_path.
//! Two concurrent same-cwd agy conversations are ambiguous → None (no
//! cross-attribution). Read-only; mirrors `claude_sidefile`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct AgySidefile {
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub context_used_percentage: Option<f64>,
    #[serde(default)]
    pub context_window_size: Option<u64>,
    #[serde(default)]
    pub token_count: Option<u64>,
}

const AMBIGUITY_WINDOW: Duration = Duration::from_secs(60);

pub fn read_agy_sidefile_for_path(home: &Path, current_path: &str) -> Option<AgySidefile> {
    if current_path.is_empty() {
        return None;
    }
    let dir = home.join(".local/share/ai-cli-status/agy");
    let entries = fs::read_dir(&dir).ok()?;
    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        candidates.push((mtime, path));
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0)); // newest first
    // Collect up to 2 cwd-matching sidefiles with DISTINCT conversation_ids
    // (None counts as always-distinct), newest first.
    let mut distinct: Vec<(SystemTime, AgySidefile)> = Vec::new();
    for (mtime, path) in candidates {
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(sf) = serde_json::from_str::<AgySidefile>(&body) else {
            continue;
        };
        if sf.cwd.as_deref() == Some(current_path) {
            let is_distinct = if let Some(cid) = sf.conversation_id.as_deref() {
                !distinct
                    .iter()
                    .any(|(_, s)| s.conversation_id.as_deref() == Some(cid))
            } else {
                true
            };
            if is_distinct {
                distinct.push((mtime, sf));
                if distinct.len() == 2 {
                    break;
                }
            }
        }
    }
    let (newest_mtime, newest) = distinct.first()?;
    if let Some((second_mtime, _)) = distinct.get(1)
        && newest_mtime
            .duration_since(*second_mtime)
            .unwrap_or(Duration::ZERO)
            < AMBIGUITY_WINDOW
    {
        return None;
    }
    Some(newest.clone())
}
```

- [ ] **Step 4: Register the module.** In `src/adapters/mod.rs` add:

```rust
pub mod agy_sidefile;
```

- [ ] **Step 5: Run to confirm pass.**

Run: `cargo test --lib agy_sidefile::tests`
Expected: PASS (5 tests).

- [ ] **Step 6: Commit.**

```bash
git add src/adapters/agy_sidefile.rs src/adapters/mod.rs
git commit -m "feat(adapters): agy structured sidefile reader (cwd-matched, ambiguity-guarded)"
```

---

## Task 4: Wire the agy enrichment branch into `parse_for_with_environment`

**Files:**

- Modify: `src/adapters/mod.rs` (enrichment branch after the agy_transcript branch
  ~line 229; rename `agy_transcript_process_confirmed` → `agy_process_confirmed`
  and update the C2 call site at line 212; add tests)

**Interfaces:**

- Consumes: `agy_footer::parse_agy_footer`, `agy_sidefile::read_agy_sidefile_for_path`,
  `ctx.agy_enrichment_enabled`, `agy_process_confirmed`.
- Produces: enriched `SignalSet` for Antigravity panes
  (`model_name`/`context_pressure`/`context_window_size`/`token_count`,
  `ProviderOfficial`).

- [ ] **Step 1: Write the failing tests.** Add to `src/adapters/mod.rs` tests,
      mirroring the `rollout_*` enrichment tests (reuse the `ctx()` builder, the
      `write_proc` helper that builds a `/proc` tree with a named descendant, and add
      a small `write_agy_sidefile` helper). Provide the agy footer text via the tail
      on the `ctx`:

```rust
fn write_agy_sidefile(home: &std::path::Path, cid: &str, body: &str) {
    let dir = home.join(".local/share/ai-cli-status/agy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{cid}.json")), body).unwrap();
}

#[test]
fn agy_enrichment_fills_model_and_token_from_footer_when_enabled() {
    let tmp = tempdir().unwrap();
    let proc = tmp.path().join("proc");
    write_proc(&proc, 100, "agy", &[]); // pane_pid 100 → agy descendant
    let home = tmp.path().join("home");
    let mut c = ctx(/* Antigravity identity, tail with footer, cwd "/repo", pane_pid 100 */);
    c.agy_enrichment_enabled = true;
    let signals = parse_for_with_environment(&c, &proc, Some(&home));
    assert_eq!(
        signals.model_name.as_ref().map(|m| m.value.as_str()),
        Some("Gemini 3.5 Flash (High)")
    );
    assert!(signals.token_count.is_some());
    assert_eq!(
        signals.model_name.as_ref().map(|m| m.source_kind),
        Some(crate::domain::origin::SourceKind::ProviderOfficial)
    );
}

#[test]
fn agy_enrichment_sidefile_overrides_footer() {
    let tmp = tempdir().unwrap();
    let proc = tmp.path().join("proc");
    write_proc(&proc, 100, "agy", &[]);
    let home = tmp.path().join("home");
    write_agy_sidefile(&home, "c1", r#"{"cwd":"/repo","conversation_id":"c1","model":"Gemini 3.1 Pro (High)","context_window_size":1048576}"#);
    let mut c = ctx(/* Antigravity, tail footer model "Gemini 3.5 Flash (High)", cwd "/repo", pid 100 */);
    c.agy_enrichment_enabled = true;
    let signals = parse_for_with_environment(&c, &proc, Some(&home));
    assert_eq!(
        signals.model_name.as_ref().map(|m| m.value.as_str()),
        Some("Gemini 3.1 Pro (High)"),
        "sidefile model must override the footer-scraped model"
    );
    assert_eq!(signals.context_window_size.as_ref().map(|m| m.value), Some(1048576));
}

#[test]
fn agy_enrichment_skipped_when_toggle_disabled() {
    let tmp = tempdir().unwrap();
    let proc = tmp.path().join("proc");
    write_proc(&proc, 100, "agy", &[]);
    let home = tmp.path().join("home");
    let mut c = ctx(/* Antigravity, tail footer, cwd "/repo", pid 100 */);
    c.agy_enrichment_enabled = false; // off
    let signals = parse_for_with_environment(&c, &proc, Some(&home));
    assert!(signals.model_name.is_none(), "no enrichment when toggle off");
}

#[test]
fn agy_enrichment_skipped_when_descendant_is_not_agy() {
    let tmp = tempdir().unwrap();
    let proc = tmp.path().join("proc");
    write_proc(&proc, 100, "bash", &[]); // no agy descendant
    let home = tmp.path().join("home");
    let mut c = ctx(/* Antigravity, tail footer, cwd "/repo", pid 100 */);
    c.agy_enrichment_enabled = true;
    let signals = parse_for_with_environment(&c, &proc, Some(&home));
    assert!(signals.model_name.is_none(), "strict agy process-confirm gates enrichment");
}
```

(Fill the `ctx(...)` arguments by following the existing `rollout_*` tests'
construction of an Antigravity-identity `ParserContext` with `pane_pid` and a
footer-bearing `tail`.)

- [ ] **Step 2: Run to confirm failure.**

Run: `cargo test --lib agy_enrichment_`
Expected: FAIL — `model_name` is `None` (no branch yet).

- [ ] **Step 3: Rename the shared process-confirm helper.** Rename
      `agy_transcript_process_confirmed` → `agy_process_confirmed` (it is now shared
      by C2 + C3; behavior unchanged — strict `false` on missing `pane_pid`, checks
      `"agy"` descendant). Update the C2 call site (`src/adapters/mod.rs:212`).

- [ ] **Step 4: Add the enrichment branch** after the agy_transcript branch
      (~line 229), mirroring the Slice B Codex-rollout branch:

```rust
    // Slice C3: agy footer/sidefile enrichment. Footer scrape fills model /
    // context% / token-count (always pane-correct) when absent; the structured
    // sidefile OVERRIDES when present and unambiguously attributed by cwd. Both
    // ProviderOfficial. Display-only — the six v2.4.0 ObserveOnly gates are
    // untouched (agy is NOT added to token_rows_supported / token-sample / insights).
    if ctx.agy_enrichment_enabled
        && matches!(ctx.identity.identity.provider, Provider::Antigravity)
        && !identity_conflict
        && !ctx.current_path.is_empty()
        && agy_process_confirmed(ctx, proc_root)
    {
        use crate::domain::identity::Provider;
        use crate::domain::origin::SourceKind;
        use crate::domain::signal::MetricValue;
        let metric_str = |v: String| {
            MetricValue::new(v, SourceKind::ProviderOfficial)
                .with_confidence(0.9)
                .with_provider(Provider::Antigravity)
        };
        let metric_f32 = |v: f32| {
            MetricValue::new(v, SourceKind::ProviderOfficial)
                .with_confidence(0.9)
                .with_provider(Provider::Antigravity)
        };
        let metric_u64 = |v: u64| {
            MetricValue::new(v, SourceKind::ProviderOfficial)
                .with_confidence(0.9)
                .with_provider(Provider::Antigravity)
        };

        // (a) Footer scrape — fill-when-absent (always the pane's own text).
        let footer = agy_footer::parse_agy_footer(ctx.tail);
        if signals.model_name.is_none()
            && let Some(m) = footer.model
        {
            signals.model_name = Some(metric_str(m));
        }
        if signals.context_pressure.is_none()
            && let Some(p) = footer.context_used_pct
        {
            signals.context_pressure = Some(metric_f32(p));
        }
        if signals.token_count.is_none()
            && let Some(n) = footer.token_count
        {
            signals.token_count = Some(metric_u64(n));
        }

        // (b) Structured sidefile — OVERRIDE when present + unambiguous.
        if let Some(home) = home_dir
            && let Some(sf) = agy_sidefile::read_agy_sidefile_for_path(home, ctx.current_path)
        {
            if let Some(m) = sf.model {
                signals.model_name = Some(metric_str(m));
            }
            if let Some(pct) = sf.context_used_percentage {
                signals.context_pressure = Some(metric_f32((pct / 100.0).clamp(0.0, 1.0) as f32));
            }
            if let Some(n) = sf.context_window_size {
                signals.context_window_size = Some(metric_u64(n));
            }
            if let Some(n) = sf.token_count {
                signals.token_count = Some(metric_u64(n));
            }
        }
    }
```

- [ ] **Step 5: Run the full gate.**

Run: `cargo test --all-targets agy_enrichment_` then `cargo test --all-targets`
Expected: PASS (4 new tests + the whole suite green; the `agy_transcript` rename
compiles its one C2 call site).

- [ ] **Step 6: Commit.**

```bash
git add src/adapters/mod.rs
git commit -m "feat(adapters): agy enrichment branch — footer fill + sidefile override"
```

---

## Task 5: Provider Setup overlay — recommended agy `statusLine.command` block

> **Spike-gated content:** the `jq` field paths inside the recommended block come
> from Task 0. If Task 0 was NO-GO, implement only the _display_ half (state +
> footer-items guidance) and omit the copyable sidefile block.

**Files:**

- Modify: `src/ui/provider_setup.rs` (`snippet_for_tab` agy arm line 324;
  `append_copy_contract` agy arm line 385; `render_tab_content` agy arm; add an
  `AGY_SIDEFILE_BLOCK` const)

**Interfaces:**

- Consumes: `overlay.agy_enrichment_enabled` (add this `bool` to
  `ProviderSetupOverlay`, mirrored from `overlay.claude_sidefile_enabled`; sourced
  from `config.provider_setup.agy_enrichment` where the overlay is built).

- [ ] **Step 1: Write the failing test.** In `src/ui/provider_setup.rs` tests:

```rust
#[test]
fn agy_snippet_carries_sidefile_block_when_enabled() {
    let mut overlay = ProviderSetupOverlay::default();
    overlay.tab = ProviderSetupTab::Antigravity;
    overlay.agy_enrichment_enabled = true;
    let (label, text) = snippet_for_tab(&overlay);
    assert!(label.contains("agy"), "label names the agy target");
    assert!(text.contains("ai-cli-status/agy"), "block writes the agy sidefile dir");
    assert!(text.contains("statusLine"), "block configures statusLine.command");
}

#[test]
fn agy_snippet_empty_when_disabled() {
    let mut overlay = ProviderSetupOverlay::default();
    overlay.tab = ProviderSetupTab::Antigravity;
    overlay.agy_enrichment_enabled = false;
    let (_, text) = snippet_for_tab(&overlay);
    assert!(text.is_empty(), "no copyable block until the operator opts in");
}
```

- [ ] **Step 2: Run to confirm failure.**

Run: `cargo test --lib agy_snippet`
Expected: FAIL — no field `agy_enrichment_enabled` / empty agy snippet.

- [ ] **Step 3: Add the overlay flag + const + arms.** Add
      `pub agy_enrichment_enabled: bool` to `ProviderSetupOverlay` (default false in
      its constructor, sourced from `config.provider_setup.agy_enrichment` at the
      build site — mirror `claude_sidefile_enabled`). Add the block const (jq paths
      per Task 0):

```rust
/// Slice C3: recommended agy statusLine.command sidefile-export. The operator
/// pastes this into `~/.gemini/antigravity-cli/settings.json` `statusLine.command`
/// (type "command"). It dumps a per-conversation JSON Qmonster reads, then echoes
/// a status line so the agy footer still renders. The `jq` input paths below are
/// confirmed by the Task-0 spike against agy's statusLine.command stdin contract.
const AGY_SIDEFILE_BLOCK: &str = r#"#!/usr/bin/env bash
# Qmonster agy sidefile export. Set as agy statusLine.command (type: "command").
input=$(cat)
dir="$HOME/.local/share/ai-cli-status/agy"; mkdir -p "$dir"
cid=$(printf '%s' "$input" | jq -r '.conversationId // .session_id // "default"')
printf '%s' "$input" | jq '{conversation_id: (.conversationId // .session_id),
  cwd: .workspace, model: .model,
  context_used_percentage: .contextUsedPercentage,
  context_window_size: .contextWindowSize, token_count: .tokenCount}' > "$dir/$cid.json"
printf '%s' "$input" | jq -r '.model // ""'   # keep the agy status line populated
"#;
```

Replace the `snippet_for_tab` agy arm (line 324):

```rust
        ProviderSetupTab::Antigravity => {
            if overlay.agy_enrichment_enabled {
                ("agy statusLine.command", String::from(AGY_SIDEFILE_BLOCK))
            } else {
                ("", String::new())
            }
        }
```

Fill the `append_copy_contract` agy arm (line 385):

```rust
        ProviderSetupTab::Antigravity => {
            out.push(detail_row("Target", "~/.gemini/antigravity-cli/settings.json"));
            out.push(detail_row("Content", "statusLine.command sidefile-export script"));
            out.push(detail_row(
                "Optional included",
                format!("agy enrichment {} (from Settings)", on_off(overlay.agy_enrichment_enabled)),
            ));
        }
```

Extend the `render_tab_content` agy arm with a Current Status + Settings +
footer-items note (mirror the Claude arm structure: `section`/`detail_row`,
then `append_copy_contract` + `append_copied_preview`), e.g. note that
`provider_setup.agy_enrichment` is read-only here (change in Settings), that
enabling the `context-used`/`token-count` footer items in agy improves scrape
coverage, and that the sidefile block overrides the footer when applied.

- [ ] **Step 4: Run to confirm pass.**

Run: `cargo test --lib agy_snippet` then `cargo test --all-targets` (overlay flag
is a shared-ish struct change)
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add src/ui/provider_setup.rs src/app/event_loop.rs
git commit -m "feat(ui): Provider Setup agy tab — recommended statusLine.command block"
```

---

## Task 6: ObserveOnly six-gate regression + docs

**Files:**

- Modify: `src/adapters/mod.rs` tests (or the existing ObserveOnly gate test
  module) — regression that a fields-populated agy pane stays off the six surfaces
- Modify: `docs/ai/ARCHITECTURE.md` (provider-coverage matrix agy row)
- Modify: `docs/ai/UI_MANUAL.md` (Provider Setup agy-tab note)

- [ ] **Step 1: Write the regression test.** Add a test asserting that an agy
      `SignalSet` enriched by C3 (model_name + token_count populated) is still
      excluded from the six v2.4.0 gates. Co-locate it with the existing agy
      ObserveOnly gate tests (find them: `rg -n "supports_provider|ObserveOnly|Antigravity" src/domain/anomaly.rs src/domain/`):

```rust
#[test]
fn agy_enrichment_does_not_re_enable_token_rows_or_anomaly_gates() {
    // C3 populates model_name + token_count for agy, but agy must stay off every
    // analytic surface: token_rows_supported false, no AnomalyKind supports agy,
    // provider_honesty Hidden, profile_switch None, insights "unsupported".
    use crate::domain::identity::Provider;
    assert!(!crate::ui::panels::token_rows_supported(Provider::Antigravity));
    assert!(
        crate::domain::anomaly::AnomalyKind::ALL
            .iter()
            .all(|k| !k.supports_provider(Provider::Antigravity)),
        "no anomaly kind may support agy after C3"
    );
    // (Add the provider_honesty / profile_switch / insights assertions using the
    // same helpers the v2.4.0 six-gate tests use — locate via the rg above.)
}
```

(Make `token_rows_supported` reachable from the test — it is currently private
in `src/ui/panels/mod.rs`; either add a `#[cfg(test)] pub(crate)` re-export or
assert via an existing public wrapper. Do NOT change its return for agy.)

- [ ] **Step 2: Run to confirm it passes (gates already hold) — this is a
      guard-rail, so it should pass immediately; if it fails, C3 broke a gate and
      must be fixed before proceeding.**

Run: `cargo test --all-targets agy_enrichment_does_not_re_enable`
Expected: PASS.

- [ ] **Step 3: Update docs.** In `docs/ai/ARCHITECTURE.md`, update the
      provider-coverage matrix agy row: agy now gets model / context% / token-count
      via C3 (footer scrape + optional sidefile, ProviderOfficial, opt-in
      `agy_enrichment`), while staying ObserveOnly on the six analytic gates; cost +
      quota remain out of scope. In `docs/ai/UI_MANUAL.md`, note the Provider Setup
      agy tab now offers a recommended `statusLine.command` block when
      `agy_enrichment` is on.

- [ ] **Step 4: Run the full gate.**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --all-targets`
Expected: all green.

- [ ] **Step 5: Commit.**

```bash
git add src/adapters/mod.rs docs/ai/ARCHITECTURE.md docs/ai/UI_MANUAL.md
git commit -m "test(agy): ObserveOnly six-gate regression for C3 + provider-coverage docs"
```

---

## Release (after all tasks, operator-gated)

Not a task — the release ritual. Bundle as **v2.7.0**: bump version surfaces
(package.json / Cargo.toml / Cargo.lock / VERSION.md / README), mission ledger
(mission.yaml / mission-history.yaml change_sequence 220 / CURRENT_STATE /
VALIDATION row), run the Codex cross-review (`codex exec` medium effort), write
the `.mission/evals` mirror, then STOP for the operator's explicit "릴리스" go
(tag + npm publish is irreversible). Gemini leg stays retired (Codex + human).

---

## Self-Review (writing-plans)

**1. Spec coverage:**

- Hybrid (footer + sidefile, sidefile overrides) → Tasks 2, 3, 4. ✓
- Core 3 fields, existing `SignalSet`, zero new fields → Tasks 4, 6 (`model_name`,
  `context_pressure`, `context_window_size`, `token_count`). ✓
- Opt-in `agy_enrichment` default false → Task 1. ✓
- SourceKind ProviderOfficial both paths → Task 4 (`metric_*` closures). ✓
- ObserveOnly six gates intact → Task 6 regression + Global Constraints. ✓
- Provider Setup recommended block → Task 5. ✓
- Task-0 spike (agy statusLine.command contract) → Task 0, gating Tasks 3 + 5-block. ✓
- Out of scope (quota/cost/protobuf/main-dispatch) → not implemented; documented in Task 6 docs. ✓

**2. Placeholder scan:** No "TBD"/"add error handling". The only deferred items
are (a) the `ctx(...)` test-arg fill in Task 4 (explicitly directed to the
existing `rollout_*` test pattern) and (b) the `jq` field paths + provider_honesty/
profile_switch/insights assertion helpers (explicitly directed to Task 0 output
and an `rg` locator) — both are "follow this existing pattern", not vague gaps.

**3. Type consistency:** `AgyFooter` fields (`model`/`context_used_pct`/
`context_window`/`token_count`) and `AgySidefile` fields
(`model`/`context_used_percentage`/`context_window_size`/`token_count`) map
consistently onto `SignalSet.{model_name, context_pressure, context_window_size,
token_count}` in Task 4. `parse_agy_footer(&str) -> AgyFooter` and
`read_agy_sidefile_for_path(&Path, &str) -> Option<AgySidefile>` are used with
those exact signatures in Task 4. `agy_process_confirmed` (renamed) is referenced
consistently in Task 4. ✓
