//! Slice B (reduced): Codex `rollout-*.jsonl` structured backstop.
//!
//! Codex writes an append-only JSONL rollout per session under
//! `<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-<id>.jsonl`. This reader
//! locates the newest rollout whose `session_meta` has
//! `originator == "codex-tui"` (interactive panes only — `codex exec`
//! rollouts share the cwd and MUST be excluded) AND `cwd == current_path`,
//! and returns the latest cumulative token totals + model. Read-only;
//! best-effort enrichment used only when the status-line scrape is absent.

use std::cmp::Reverse;
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

/// Recursively collect `rollout-*.jsonl` paths with their mtime,
/// newest-first and bounded to `MAX_CANDIDATES`. Date-partitioned dir
/// names (`YYYY/MM/DD`) and timestamped rollout filenames both sort
/// chronologically, so descending name order visits the most recent
/// sessions first and lets us stop without scanning the whole tree.
fn collect_rollouts(dir: &Path, out: &mut Vec<(SystemTime, PathBuf)>) {
    if out.len() >= MAX_CANDIDATES {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| Reverse(e.file_name()));
    for entry in entries {
        if out.len() >= MAX_CANDIDATES {
            return;
        }
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
                let meta: SessionMeta = serde_json::from_value(parsed.payload).unwrap_or_default();
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // Write a minimal rollout JSONL (session_meta + turn_context + a token_count event_msg).
    fn write_rollout_with_tokens(
        dir: &Path,
        name: &str,
        originator: &str,
        cwd: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        let body = format!(
            concat!(
                r#"{{"type":"session_meta","payload":{{"originator":"{orig}","cwd":"{cwd}","cli_version":"0.142.2"}}}}"#,
                "\n",
                r#"{{"type":"turn_context","payload":{{"model":"gpt-5.5"}}}}"#,
                "\n",
                r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":1200000,"output_tokens":{output},"reasoning_output_tokens":1000,"total_tokens":{input_output}}},"last_token_usage":{{"input_tokens":2000,"output_tokens":50,"total_tokens":2050}},"model_context_window":258400}}}}}}"#,
                "\n",
            ),
            orig = originator,
            cwd = cwd,
            input = input_tokens,
            output = output_tokens,
            input_output = input_tokens + output_tokens,
        );
        fs::write(&path, body).unwrap();
        path
    }

    // Convenience wrapper with fixed token values.
    fn write_rollout(dir: &Path, name: &str, originator: &str, cwd: &str) -> PathBuf {
        write_rollout_with_tokens(dir, name, originator, cwd, 1_510_000, 20_400)
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
        write_rollout(
            &sessions,
            "rollout-exec.jsonl",
            "codex_exec",
            "/repo/qmonster",
        );
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

    #[test]
    fn returns_newest_matching_rollout_when_multiple_match() {
        let tmp = tempdir().unwrap();
        let sessions = tmp.path().join("sessions/2026/06/29");
        // Older-named rollout (visited later under descending-name order).
        write_rollout_with_tokens(
            &sessions,
            "rollout-2026-06-29T10-00-00-old.jsonl",
            "codex-tui",
            "/repo/qmonster",
            100,
            200,
        );
        // Newer-named rollout — must win.
        write_rollout_with_tokens(
            &sessions,
            "rollout-2026-06-29T20-00-00-new.jsonl",
            "codex-tui",
            "/repo/qmonster",
            999,
            888,
        );
        let s = read_rollout_for_path(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(s.input_tokens, Some(999));
        assert_eq!(s.output_tokens, Some(888));
    }
}
