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
use std::time::Duration;
use std::time::SystemTime;

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexRolloutSignals {
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub context_window: Option<u64>,
    /// Reset ETAs (unix secs) from the rollout's `rate_limits` event. The Codex
    /// status line carries no reset time, so the rollout is the only source.
    /// `primary` window (≤720 min) → 5h, `secondary` (>720 min) → weekly.
    pub quota_5h_resets_at: Option<u64>,
    pub quota_weekly_resets_at: Option<u64>,
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

/// Two `codex-tui` rollouts matching the same cwd whose newest mtimes fall
/// within this window are treated as concurrent active panes → ambiguous →
/// no enrichment. `cwd` is not a pane-unique key, so rather than risk
/// attributing pane B's tokens to pane A we fall back to the scrape. 60s
/// comfortably separates a live session (its rollout is appended every turn)
/// from a finished/older same-cwd session. (Heuristic — Qmonster-chosen.)
const AMBIGUITY_WINDOW: Duration = Duration::from_secs(60);

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
    // mtime desc → newest (actively-appended) session first; cap bounds the
    // expensive content reads.
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    // Collect the cwd-matching codex-tui rollouts, newest first. Only the top
    // two are needed to decide ambiguity, so stop after the second match.
    let mut matches: Vec<(SystemTime, CodexRolloutSignals)> = Vec::new();
    for (mtime, path) in candidates.into_iter().take(MAX_CANDIDATES) {
        let body = match fs::read_to_string(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if let Some(sig) = parse_if_matching(&body, current_path) {
            matches.push((mtime, sig));
            if matches.len() == 2 {
                break;
            }
        }
    }
    let (newest_mtime, newest_sig) = matches.first()?;
    // Ambiguity guard: two concurrently-active same-cwd codex-tui panes can't
    // be told apart by cwd alone — don't attribute, fall back to the scrape.
    if let Some((second_mtime, _)) = matches.get(1)
        && newest_mtime
            .duration_since(*second_mtime)
            .unwrap_or(Duration::ZERO)
            < AMBIGUITY_WINDOW
    {
        return None;
    }
    Some(newest_sig.clone())
}

/// Recursively collect every `rollout-*.jsonl` path with its mtime.
/// Statting is cheap; the expensive step (reading + parsing file
/// contents) is bounded separately by `read_rollout_for_path`'s
/// mtime-sort + `MAX_CANDIDATES` take, so selection is always by true
/// newest-mtime (the actively-appended session), not by filename
/// (which encodes session-START time and can be older for a
/// long-lived pane).
fn collect_rollouts(dir: &Path, out: &mut Vec<(SystemTime, PathBuf)>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
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
                    && let Some(info) = parsed.payload.get("info")
                {
                    if let Some(total) = info.get("total_token_usage") {
                        sig.input_tokens = total.get("input_tokens").and_then(|v| v.as_u64());
                        sig.output_tokens = total.get("output_tokens").and_then(|v| v.as_u64());
                        sig.cached_input_tokens =
                            total.get("cached_input_tokens").and_then(|v| v.as_u64());
                    }
                    sig.context_window = info.get("model_context_window").and_then(|v| v.as_u64());
                    // `rate_limits` sits beside `info` in the same token_count
                    // payload; the status line carries no reset, so the rollout
                    // is the only reset-ETA source. Latest token_count event
                    // wins; classify each window by `window_minutes` (≤720 → 5h,
                    // else weekly) so a label/order change can't misattribute.
                    if let Some(rl) = parsed.payload.get("rate_limits") {
                        for key in ["primary", "secondary"] {
                            let Some(window) = rl.get(key) else {
                                continue;
                            };
                            let (Some(ts), Some(win)) = (
                                window.get("resets_at").and_then(|v| v.as_u64()),
                                window.get("window_minutes").and_then(|v| v.as_u64()),
                            ) else {
                                continue;
                            };
                            if win <= 720 {
                                sig.quota_5h_resets_at = Some(ts);
                            } else {
                                sig.quota_weekly_resets_at = Some(ts);
                            }
                        }
                    }
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
    fn returns_newest_by_mtime_even_when_filename_is_older() {
        let tmp = tempdir().unwrap();
        let sessions = tmp.path().join("sessions/2026/06/29");
        // NEWER filename, but we force it to an OLDER mtime → must LOSE.
        let loser = write_rollout_with_tokens(
            &sessions,
            "rollout-2026-06-29T20-00-00-newname.jsonl",
            "codex-tui",
            "/repo/qmonster",
            100,
            200,
        );
        // OLDER filename (long-lived session that started earlier),
        // forced to the NEWER mtime → must WIN (selection is by mtime).
        let winner = write_rollout_with_tokens(
            &sessions,
            "rollout-2026-06-29T08-00-00-oldname.jsonl",
            "codex-tui",
            "/repo/qmonster",
            999,
            888,
        );
        let older = std::time::SystemTime::now() - std::time::Duration::from_secs(300);
        filetime::set_file_mtime(&loser, filetime::FileTime::from_system_time(older)).unwrap();
        filetime::set_file_mtime(
            &winner,
            filetime::FileTime::from_system_time(std::time::SystemTime::now()),
        )
        .unwrap();
        let s = read_rollout_for_path(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(
            s.input_tokens,
            Some(999),
            "newest-mtime wins despite older filename"
        );
        assert_eq!(s.output_tokens, Some(888));
    }

    #[test]
    fn ambiguous_concurrent_same_cwd_codex_tui_returns_none() {
        // Two codex-tui rollouts, same cwd, touched within the ambiguity
        // window (concurrent active panes) → cannot attribute → None.
        let tmp = tempdir().unwrap();
        let sessions = tmp.path().join("sessions/2026/06/29");
        let a = write_rollout_with_tokens(
            &sessions,
            "rollout-2026-06-29T20-00-00-a.jsonl",
            "codex-tui",
            "/repo/qmonster",
            111,
            222,
        );
        let b = write_rollout_with_tokens(
            &sessions,
            "rollout-2026-06-29T20-00-05-b.jsonl",
            "codex-tui",
            "/repo/qmonster",
            999,
            888,
        );
        let now = std::time::SystemTime::now();
        filetime::set_file_mtime(
            &a,
            filetime::FileTime::from_system_time(now - std::time::Duration::from_secs(5)),
        )
        .unwrap();
        filetime::set_file_mtime(&b, filetime::FileTime::from_system_time(now)).unwrap();
        assert!(
            read_rollout_for_path(tmp.path(), "/repo/qmonster").is_none(),
            "two concurrent same-cwd codex-tui sessions must not cross-fill"
        );
    }

    #[test]
    fn reads_context_window_from_token_count_info() {
        let tmp = tempdir().unwrap();
        let sessions = tmp.path().join("sessions/2026/06/29");
        write_rollout(&sessions, "rollout-a.jsonl", "codex-tui", "/repo/qmonster");
        let s = read_rollout_for_path(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(s.context_window, Some(258400));
    }

    #[test]
    fn reads_rate_limit_resets_classified_by_window_minutes() {
        let tmp = tempdir().unwrap();
        let sessions = tmp.path().join("sessions/2026/06/30");
        fs::create_dir_all(&sessions).unwrap();
        // Raw JSONL (no format!) — `rate_limits` beside `info`; primary = 5h
        // window (300 min), secondary = weekly (10080 min).
        let body = concat!(
            r#"{"type":"session_meta","payload":{"originator":"codex-tui","cwd":"/repo/qmonster"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}},"rate_limits":{"primary":{"used_percent":1.0,"window_minutes":300,"resets_at":1782764339},"secondary":{"used_percent":2.0,"window_minutes":10080,"resets_at":1783313312}}}}"#,
            "\n",
        );
        fs::write(sessions.join("rollout-rl.jsonl"), body).unwrap();
        let s = read_rollout_for_path(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(
            s.quota_5h_resets_at,
            Some(1782764339),
            "primary window (300 min ≤ 720) → 5h reset"
        );
        assert_eq!(
            s.quota_weekly_resets_at,
            Some(1783313312),
            "secondary window (10080 min > 720) → weekly reset"
        );
    }

    #[test]
    fn rollout_without_rate_limits_yields_no_resets() {
        let tmp = tempdir().unwrap();
        let sessions = tmp.path().join("sessions/2026/06/30");
        write_rollout(&sessions, "rollout-a.jsonl", "codex-tui", "/repo/qmonster");
        let s = read_rollout_for_path(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(s.quota_5h_resets_at, None);
        assert_eq!(s.quota_weekly_resets_at, None);
    }
}
