//! Slice C2: Antigravity `history.jsonl` transcript activity reader.
//!
//! Antigravity writes an append-only history of pane activity under
//! `<GEMINI_HOME>/antigravity-cli/history.jsonl`. This reader locates the newest entry
//! whose `workspace` (cwd) matches `current_path` and has a `conversationId`,
//! then reads the correlated transcript to extract last-activity timestamp and latest step.
//! Read-only; provides activity-only enrichment via `SourceKind::Heuristic`.

use std::fs;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgyActivity {
    pub conversation_id: String,
    pub last_activity_unix: Option<u64>,
    pub latest_step: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HistoryLine {
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
    #[serde(default)]
    timestamp: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TranscriptLine {
    #[serde(default)]
    #[serde(rename = "type")]
    kind: Option<String>,
}

/// Two distinct `conversationId`s in the same workspace whose newest timestamps fall
/// within this window are treated as concurrent active agy panes → ambiguous →
/// no enrichment. `workspace` (cwd) is not a pane-unique key, so rather than risk
/// attributing pane B's activity to pane A we fall back to returning None.
/// 60s comfortably separates a live session (actively appending) from a finished/older
/// same-workspace session. (Heuristic — mirrors codex_rollout ambiguity guard.)
const AMBIGUITY_WINDOW: Duration = Duration::from_secs(60);

/// Locate the newest agy history entry whose `workspace` matches `current_path`,
/// extract its `conversationId`, and read the transcript to obtain last-activity
/// timestamp and latest step kind. Returns None on empty path, missing history.jsonl,
/// no match, or ambiguity.
pub fn read_agy_activity(gemini_home: &Path, current_path: &str) -> Option<AgyActivity> {
    if current_path.is_empty() {
        return None;
    }
    let history_path = gemini_home.join("antigravity-cli/history.jsonl");
    let body = match fs::read_to_string(&history_path) {
        Ok(b) => b,
        Err(_) => return None,
    };

    // Parse all history lines and collect workspace-matching entries with conversationIds.
    let mut matches: Vec<(u64, String)> = Vec::new(); // (timestamp, conversationId)
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<HistoryLine>(line) else {
            continue;
        };
        if let (Some(ws), Some(cid), Some(ts)) =
            (parsed.workspace, parsed.conversation_id, parsed.timestamp)
            && ws == current_path
        {
            // history.jsonl is one small append-only file — collect ALL
            // matching entries and sort by timestamp below. No scan cap: a
            // file-order cap would drop the newest entries (appended last).
            matches.push((ts, cid));
        }
    }

    // Sort by timestamp desc → newest first.
    matches.sort_by(|a, b| b.0.cmp(&a.0));

    // Ambiguity guard: check if the top two *distinct* conversationIds
    // for this workspace are too close in time.
    let mut distinct_cids: Vec<(u64, String)> = Vec::new();
    for (ts, cid) in matches.iter() {
        if distinct_cids.is_empty() || distinct_cids.last().unwrap().1 != *cid {
            distinct_cids.push((*ts, cid.clone()));
            if distinct_cids.len() == 2 {
                break;
            }
        }
    }

    // If we have two distinct conversationIds and they're within the ambiguity window,
    // we can't tell which pane is active.
    if distinct_cids.len() == 2 {
        let newest_ms = distinct_cids[0].0;
        let second_newest_ms = distinct_cids[1].0;
        // Both are in milliseconds; compute the duration between them.
        let duration_ms = newest_ms.saturating_sub(second_newest_ms);
        if std::time::Duration::from_millis(duration_ms) < AMBIGUITY_WINDOW {
            return None; // Concurrent ambiguity
        }
    }

    // Pick the newest conversationId (which is the first after sorting).
    let conversation_id = matches.first()?.1.clone();

    // Read the transcript and extract last-activity + latest step.
    let transcript_dir = gemini_home.join(format!(
        "antigravity-cli/brain/{}/.system_generated/logs",
        conversation_id
    ));
    let transcript_path = transcript_dir.join("transcript.jsonl");

    // Extract mtime as unix seconds.
    let last_activity_unix = fs::metadata(&transcript_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    // Parse the last well-formed transcript line for its `type` field.
    let latest_step = if let Ok(body) = fs::read_to_string(&transcript_path) {
        body.lines().rev().find_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            serde_json::from_str::<TranscriptLine>(line)
                .ok()
                .and_then(|parsed| parsed.kind)
        })
    } else {
        None
    };

    Some(AgyActivity {
        conversation_id,
        last_activity_unix,
        latest_step,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // Write a minimal history.jsonl with the given entries.
    fn write_history(dir: &Path, entries: Vec<(&str, &str, u64)>) {
        let agy_dir = dir.join("antigravity-cli");
        fs::create_dir_all(&agy_dir).unwrap();
        let path = agy_dir.join("history.jsonl");
        let mut body = String::new();
        for (workspace, conversation_id, timestamp) in entries {
            body.push_str(&format!(
                r#"{{"workspace":"{}","conversationId":"{}","timestamp":{}}}"#,
                workspace, conversation_id, timestamp
            ));
            body.push('\n');
        }
        fs::write(&path, body).unwrap();
    }

    // Write a minimal transcript.jsonl with the given steps.
    fn write_transcript(dir: &Path, conversation_id: &str, steps: Vec<&str>) {
        let transcript_dir = dir.join(format!(
            "antigravity-cli/brain/{}/.system_generated/logs",
            conversation_id
        ));
        fs::create_dir_all(&transcript_dir).unwrap();
        let path = transcript_dir.join("transcript.jsonl");
        let mut body = String::new();
        for step in steps {
            body.push_str(&format!(
                r#"{{"type":"{}","created_at":"2026-06-29T12:00:00Z"}}"#,
                step
            ));
            body.push('\n');
        }
        fs::write(&path, body).unwrap();
    }

    #[test]
    fn reads_newest_conversation_with_transcript() {
        let tmp = tempdir().unwrap();
        write_history(
            tmp.path(),
            vec![
                ("/repo/qmonster", "uuid-aaa", 1000),
                ("/repo/qmonster", "uuid-bbb", 120_000), // 120 seconds later - unambiguous
                ("/repo/other", "uuid-ccc", 3000),
            ],
        );
        write_transcript(tmp.path(), "uuid-bbb", vec!["STEP_ONE", "STEP_TWO"]);
        let activity = read_agy_activity(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(activity.conversation_id, "uuid-bbb");
        assert_eq!(activity.latest_step.as_deref(), Some("STEP_TWO"));
        assert!(activity.last_activity_unix.is_some());
    }

    #[test]
    fn picks_newest_by_timestamp_even_when_appended_last() {
        // Append-only order: oldest first, newest LAST in the file. Newest
        // must still win — regression guard against a file-order scan cap.
        let tmp = tempdir().unwrap();
        write_history(
            tmp.path(),
            vec![
                ("/repo/qmonster", "uuid-old", 1000),
                ("/repo/qmonster", "uuid-new", 200_000), // newest, written last, >60s gap
            ],
        );
        write_transcript(tmp.path(), "uuid-new", vec!["LATEST"]);
        let activity = read_agy_activity(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(activity.conversation_id, "uuid-new");
    }

    #[test]
    fn returns_none_for_ambiguous_concurrent_conversations() {
        let tmp = tempdir().unwrap();
        // Two distinct conversation IDs, same workspace, timestamps within 60s (60,000ms)
        write_history(
            tmp.path(),
            vec![
                ("/repo/qmonster", "uuid-aaa", 10_000), // timestamp in ms
                ("/repo/qmonster", "uuid-bbb", 40_000), // 30s later (30,000ms) - within 60s window
            ],
        );
        write_transcript(tmp.path(), "uuid-bbb", vec!["STEP"]);
        let activity = read_agy_activity(tmp.path(), "/repo/qmonster");
        assert!(
            activity.is_none(),
            "two concurrent conversations in one workspace must be ambiguous"
        );
    }

    #[test]
    fn returns_none_for_non_matching_workspace() {
        let tmp = tempdir().unwrap();
        write_history(tmp.path(), vec![("/repo/other", "uuid-aaa", 1000)]);
        let activity = read_agy_activity(tmp.path(), "/repo/qmonster");
        assert!(activity.is_none());
    }

    #[test]
    fn returns_none_for_empty_current_path() {
        let tmp = tempdir().unwrap();
        write_history(tmp.path(), vec![("/repo/qmonster", "uuid-aaa", 1000)]);
        let activity = read_agy_activity(tmp.path(), "");
        assert!(activity.is_none());
    }

    #[test]
    fn returns_none_when_history_missing() {
        let tmp = tempdir().unwrap();
        let activity = read_agy_activity(tmp.path(), "/repo/qmonster");
        assert!(activity.is_none());
    }

    #[test]
    fn returns_some_without_transcript_file() {
        let tmp = tempdir().unwrap();
        write_history(tmp.path(), vec![("/repo/qmonster", "uuid-aaa", 1000)]);
        // Do NOT write transcript — expect graceful None fields
        let activity = read_agy_activity(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(activity.conversation_id, "uuid-aaa");
        assert!(activity.last_activity_unix.is_none());
        assert!(activity.latest_step.is_none());
    }

    #[test]
    fn ignores_entries_without_conversation_id() {
        let tmp = tempdir().unwrap();
        let history_path = tmp.path().join("antigravity-cli/history.jsonl");
        fs::create_dir_all(history_path.parent().unwrap()).unwrap();
        // Write entries: one without conversationId, one with
        fs::write(
            &history_path,
            r#"{"workspace":"/repo/qmonster","timestamp":1000}
{"workspace":"/repo/qmonster","conversationId":"uuid-aaa","timestamp":2000}
"#,
        )
        .unwrap();
        write_transcript(tmp.path(), "uuid-aaa", vec!["STEP"]);
        let activity = read_agy_activity(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(activity.conversation_id, "uuid-aaa");
    }

    #[test]
    fn skips_malformed_lines_defensively() {
        let tmp = tempdir().unwrap();
        let history_path = tmp.path().join("antigravity-cli/history.jsonl");
        fs::create_dir_all(history_path.parent().unwrap()).unwrap();
        fs::write(
            &history_path,
            "not valid json\n{\"workspace\":\"/repo/qmonster\",\"conversationId\":\"uuid-aaa\",\"timestamp\":1000}\n",
        )
        .unwrap();
        write_transcript(tmp.path(), "uuid-aaa", vec!["STEP"]);
        let activity = read_agy_activity(tmp.path(), "/repo/qmonster").expect("must match");
        assert_eq!(activity.conversation_id, "uuid-aaa");
    }
}
