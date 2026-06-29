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
        write(
            &agy_dir(tmp.path()),
            "a",
            r#"{"cwd":"/repo/a","conversation_id":"a","model":"Gemini 3.5 Flash (High)"}"#,
        );
        write(
            &agy_dir(tmp.path()),
            "b",
            r#"{"cwd":"/repo/b","conversation_id":"b"}"#,
        );
        let s = read_agy_sidefile_for_path(tmp.path(), "/repo/a").expect("cwd match");
        assert_eq!(s.conversation_id.as_deref(), Some("a"));
        assert_eq!(s.model.as_deref(), Some("Gemini 3.5 Flash (High)"));
    }

    #[test]
    fn none_for_concurrent_same_cwd_distinct_conversations() {
        let tmp = tempdir().unwrap();
        let a = write(
            &agy_dir(tmp.path()),
            "a",
            r#"{"cwd":"/repo","conversation_id":"a"}"#,
        );
        let b = write(
            &agy_dir(tmp.path()),
            "b",
            r#"{"cwd":"/repo","conversation_id":"b"}"#,
        );
        let now = SystemTime::now();
        filetime::set_file_mtime(
            &a,
            filetime::FileTime::from_system_time(now - Duration::from_secs(5)),
        )
        .unwrap();
        filetime::set_file_mtime(&b, filetime::FileTime::from_system_time(now)).unwrap();
        assert!(read_agy_sidefile_for_path(tmp.path(), "/repo").is_none());
    }

    #[test]
    fn newest_wins_when_same_conversation_within_window() {
        let tmp = tempdir().unwrap();
        let a = write(
            &agy_dir(tmp.path()),
            "old",
            r#"{"cwd":"/repo","conversation_id":"same","token_count":1}"#,
        );
        let b = write(
            &agy_dir(tmp.path()),
            "new",
            r#"{"cwd":"/repo","conversation_id":"same","token_count":2}"#,
        );
        let now = SystemTime::now();
        filetime::set_file_mtime(
            &a,
            filetime::FileTime::from_system_time(now - Duration::from_secs(5)),
        )
        .unwrap();
        filetime::set_file_mtime(&b, filetime::FileTime::from_system_time(now)).unwrap();
        let s =
            read_agy_sidefile_for_path(tmp.path(), "/repo").expect("same conversation → newest");
        assert_eq!(s.token_count, Some(2));
    }

    #[test]
    fn parses_full_shape_and_skips_malformed() {
        let tmp = tempdir().unwrap();
        write(&agy_dir(tmp.path()), "broken", "not json {");
        write(
            &agy_dir(tmp.path()),
            "ok",
            r#"{"cwd":"/repo","conversation_id":"ok","model":"Gemini 3.1 Pro (High)","context_used_percentage":37.5,"context_window_size":1048576,"token_count":34567,"quota_5h_pressure":0.0,"quota_5h_resets_at":1700000000,"quota_weekly_pressure":0.0032377,"quota_weekly_resets_at":1700600000}"#,
        );
        let s = read_agy_sidefile_for_path(tmp.path(), "/repo").expect("malformed must not block");
        assert_eq!(s.conversation_id.as_deref(), Some("ok"));
        assert_eq!(s.context_used_percentage, Some(37.5));
        assert_eq!(s.quota_5h_pressure, Some(0.0));
        assert_eq!(s.quota_5h_resets_at, Some(1700000000));
        assert_eq!(s.quota_weekly_pressure, Some(0.0032377));
        assert_eq!(s.quota_weekly_resets_at, Some(1700600000));
    }
}
