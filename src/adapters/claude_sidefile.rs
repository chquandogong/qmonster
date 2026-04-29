//! Phase F F-5b (v1.31.0): Claude statusLine sidefile JSON reader.
//!
//! When the operator's `~/.claude/statusline.sh` carries the
//! recommended sidefile-export block (see G-1's Provider Setup
//! overlay), every prompt cycle dumps the full Claude statusLine
//! JSON to:
//!
//!   `~/.local/share/ai-cli-status/claude/<session_id>.json`
//!
//! That dump is much richer than the visible statusline tail:
//! it carries raw `cache_read_input_tokens` / `input_tokens` /
//! `output_tokens` / `cache_creation_input_tokens`, the cumulative
//! `cost.total_cost_usd`, the `transcript_path`, and Unix
//! `resets_at` timestamps for the 5h and 7-day rate-limit windows.
//!
//! Qmonster's pane observation does not see `session_id` directly,
//! so this module matches a sidefile to a pane by the JSON's `cwd`
//! field equalling the pane's `current_path`. Multiple matches
//! (multiple Claude sessions in the same worktree) are resolved by
//! picking the most-recently-modified file. Read-only — Qmonster
//! never writes to the sidefile dir.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefile {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub cost: Option<ClaudeSidefileCost>,
    #[serde(default)]
    pub context_window: Option<ClaudeSidefileContextWindow>,
    #[serde(default)]
    pub rate_limits: Option<ClaudeSidefileRateLimits>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefileCost {
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefileContextWindow {
    #[serde(default)]
    pub current_usage: Option<ClaudeSidefileCurrentUsage>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefileCurrentUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefileRateLimits {
    #[serde(default)]
    pub five_hour: Option<ClaudeSidefileRateWindow>,
    #[serde(default)]
    pub seven_day: Option<ClaudeSidefileRateWindow>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeSidefileRateWindow {
    #[serde(default)]
    pub resets_at: Option<u64>,
}

/// Compute `cache_read / (cache_read + input_tokens)` from a parsed
/// sidefile when both raw counts are present. Used to override the
/// statusline's rounded-to-integer `cache N%` with a precise ratio.
pub fn cache_hit_ratio(sidefile: &ClaudeSidefile) -> Option<f64> {
    let usage = sidefile.context_window.as_ref()?.current_usage.as_ref()?;
    let cached = usage.cache_read_input_tokens? as f64;
    let input = usage.input_tokens.unwrap_or(0) as f64;
    let total = cached + input;
    if total <= 0.0 {
        return None;
    }
    Some(cached / total)
}

/// Locate the most-recently-modified sidefile JSON whose `cwd` field
/// matches `current_path`. Returns None when:
/// - The sidefile directory does not exist (operator hasn't applied
///   the recommended statusline yet).
/// - No sidefile in the dir has a `cwd` matching `current_path`.
/// - A matching file failed to read or parse (silently ignored —
///   this is best-effort enrichment, not a correctness gate).
pub fn read_sidefile_for_path(home: &Path, current_path: &str) -> Option<ClaudeSidefile> {
    if current_path.is_empty() {
        return None;
    }
    let dir = home.join(".local/share/ai-cli-status/claude");
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
    // Newest first so the first cwd match wins.
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in candidates {
        let body = match fs::read_to_string(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let sidefile: ClaudeSidefile = match serde_json::from_str(&body) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if sidefile.cwd.as_deref() == Some(current_path) {
            return Some(sidefile);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    fn write_sidefile(dir: &Path, sid: &str, body: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{sid}.json"));
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn read_sidefile_returns_none_when_dir_missing() {
        let tmp = tempdir().unwrap();
        assert!(read_sidefile_for_path(tmp.path(), "/some/path").is_none());
    }

    #[test]
    fn read_sidefile_returns_none_when_current_path_empty() {
        let tmp = tempdir().unwrap();
        // Even with a valid sidefile present, an empty current_path
        // must short-circuit so global / unattributed Claude sessions
        // don't bleed into a pane with no resolved cwd.
        let sub = tmp.path().join(".local/share/ai-cli-status/claude");
        write_sidefile(&sub, "abc", r#"{"cwd":"/foo","session_id":"abc"}"#);
        assert!(read_sidefile_for_path(tmp.path(), "").is_none());
    }

    #[test]
    fn read_sidefile_matches_by_cwd() {
        let tmp = tempdir().unwrap();
        let sub = tmp.path().join(".local/share/ai-cli-status/claude");
        write_sidefile(&sub, "aaa", r#"{"cwd":"/repo/a","session_id":"aaa"}"#);
        write_sidefile(&sub, "bbb", r#"{"cwd":"/repo/b","session_id":"bbb"}"#);
        let s = read_sidefile_for_path(tmp.path(), "/repo/b")
            .expect("matching cwd must return the file");
        assert_eq!(s.session_id.as_deref(), Some("bbb"));
    }

    #[test]
    fn read_sidefile_picks_most_recent_when_multiple_match() {
        let tmp = tempdir().unwrap();
        let sub = tmp.path().join(".local/share/ai-cli-status/claude");
        let older = write_sidefile(&sub, "old", r#"{"cwd":"/repo","session_id":"old"}"#);
        // Force older mtime to clearly precede the next write.
        let earlier = SystemTime::now() - Duration::from_secs(60);
        filetime::set_file_mtime(&older, filetime::FileTime::from_system_time(earlier)).unwrap();
        write_sidefile(&sub, "new", r#"{"cwd":"/repo","session_id":"new"}"#);
        let s = read_sidefile_for_path(tmp.path(), "/repo").expect("at least one match");
        assert_eq!(
            s.session_id.as_deref(),
            Some("new"),
            "newest-mtime sidefile must win when multiple sessions share a cwd"
        );
    }

    #[test]
    fn read_sidefile_skips_malformed_json() {
        let tmp = tempdir().unwrap();
        let sub = tmp.path().join(".local/share/ai-cli-status/claude");
        write_sidefile(&sub, "broken", "not json {");
        write_sidefile(&sub, "ok", r#"{"cwd":"/repo","session_id":"ok"}"#);
        let s = read_sidefile_for_path(tmp.path(), "/repo").expect("malformed file must not block");
        assert_eq!(s.session_id.as_deref(), Some("ok"));
    }

    #[test]
    fn cache_hit_ratio_from_raw_counts() {
        let s = ClaudeSidefile {
            context_window: Some(ClaudeSidefileContextWindow {
                current_usage: Some(ClaudeSidefileCurrentUsage {
                    input_tokens: Some(50_000),
                    cache_read_input_tokens: Some(150_000),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };
        let r = cache_hit_ratio(&s).expect("ratio must compute");
        assert!((r - 0.75).abs() < 1e-9);
    }

    #[test]
    fn cache_hit_ratio_returns_none_when_total_zero() {
        let s = ClaudeSidefile {
            context_window: Some(ClaudeSidefileContextWindow {
                current_usage: Some(ClaudeSidefileCurrentUsage {
                    input_tokens: Some(0),
                    cache_read_input_tokens: Some(0),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };
        assert!(cache_hit_ratio(&s).is_none());
    }

    #[test]
    fn read_sidefile_parses_full_real_world_shape() {
        // Verify deserialization tolerates the live Claude sidefile
        // shape (extra fields ignored thanks to serde defaults), and
        // that nested optionals remain accessible.
        let tmp = tempdir().unwrap();
        let sub = tmp.path().join(".local/share/ai-cli-status/claude");
        let body = r#"{
            "session_id": "abc",
            "transcript_path": "/home/u/.claude/projects/x/abc.jsonl",
            "cwd": "/home/u/repo",
            "model": {"id": "claude-x", "display_name": "Claude X"},
            "version": "2.1.0",
            "cost": {
                "total_cost_usd": 12.34,
                "total_duration_ms": 9999,
                "total_lines_added": 100
            },
            "context_window": {
                "total_input_tokens": 1000,
                "current_usage": {
                    "input_tokens": 50,
                    "output_tokens": 60,
                    "cache_creation_input_tokens": 70,
                    "cache_read_input_tokens": 80
                },
                "used_percentage": 5
            },
            "rate_limits": {
                "five_hour": {"used_percentage": 30, "resets_at": 1700000000},
                "seven_day": {"used_percentage": 10, "resets_at": 1700100000}
            },
            "extra_field": "ignored"
        }"#;
        write_sidefile(&sub, "abc", body);
        let s = read_sidefile_for_path(tmp.path(), "/home/u/repo").expect("must parse");
        assert_eq!(s.session_id.as_deref(), Some("abc"));
        assert_eq!(
            s.transcript_path.as_deref(),
            Some("/home/u/.claude/projects/x/abc.jsonl")
        );
        assert!((s.cost.unwrap().total_cost_usd.unwrap() - 12.34).abs() < 1e-9);
        let usage = s.context_window.unwrap().current_usage.unwrap();
        assert_eq!(usage.cache_read_input_tokens, Some(80));
        assert_eq!(usage.cache_creation_input_tokens, Some(70));
        let rl = s.rate_limits.unwrap();
        assert_eq!(rl.five_hour.unwrap().resets_at, Some(1700000000));
        assert_eq!(rl.seven_day.unwrap().resets_at, Some(1700100000));
    }
}
