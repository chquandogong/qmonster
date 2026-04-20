use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Snapshot of CLI tool versions known to Qmonster. Persisted to
/// `~/.qmonster/versions.json` in Phase 2 so drift can be detected
/// across restarts (not just within a single session).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionSnapshot {
    pub tools: BTreeMap<String, String>,
}

impl VersionSnapshot {
    /// Read a previously-saved snapshot. `Ok(None)` if the file does not
    /// exist; errors surface for real IO/parse problems so the caller
    /// can audit-log them.
    pub fn load_from(path: &Path) -> std::io::Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<VersionSnapshot>(&text) {
                Ok(snap) => Ok(Some(snap)),
                Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Persist this snapshot to disk (creates parent directory).
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
        std::fs::write(path, data)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionDiff {
    pub tool: String,
    pub before: String,
    pub after: String,
}

/// Capture versions for the four CLIs we observe. Missing tools are
/// recorded as "<missing>" so a later capture that adds one appears as
/// drift.
pub fn capture_versions() -> VersionSnapshot {
    let mut tools = BTreeMap::new();
    tools.insert("claude".into(), cli_version("claude", &["--version"]));
    tools.insert("codex".into(), cli_version("codex", &["--version"]));
    tools.insert("gemini".into(), cli_version("gemini", &["--version"]));
    tools.insert("tmux".into(), cli_version("tmux", &["-V"]));
    VersionSnapshot { tools }
}

fn cli_version(cmd: &str, args: &[&str]) -> String {
    match Command::new(cmd).args(args).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let first_line = stdout.lines().next().unwrap_or("").trim();
            if first_line.is_empty() {
                "<missing>".to_string()
            } else {
                first_line.to_string()
            }
        }
        _ => "<missing>".to_string(),
    }
}

/// Compare two snapshots; every tool whose version differs (including
/// additions or removals) becomes a `VersionDiff`. Returning an empty
/// vec means "no drift".
pub fn compare(before: &VersionSnapshot, after: &VersionSnapshot) -> Vec<VersionDiff> {
    let mut diffs = Vec::new();
    let mut keys: Vec<&String> = before
        .tools
        .keys()
        .chain(after.tools.keys())
        .collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        let b = before.tools.get(key).cloned().unwrap_or_else(|| "<missing>".to_string());
        let a = after.tools.get(key).cloned().unwrap_or_else(|| "<missing>".to_string());
        if a != b {
            diffs.push(VersionDiff {
                tool: key.clone(),
                before: b,
                after: a,
            });
        }
    }
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn compare_returns_none_on_identical_snapshots() {
        let a = snapshot([("claude", "1.0.0"), ("tmux", "3.5")]);
        let b = a.clone();
        let diffs = compare(&a, &b);
        assert!(diffs.is_empty());
    }

    #[test]
    fn compare_returns_drift_for_each_changed_key() {
        let a = snapshot([("claude", "1.0.0"), ("tmux", "3.5")]);
        let b = snapshot([("claude", "1.1.0"), ("tmux", "3.5")]);
        let diffs = compare(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].tool, "claude");
        assert_eq!(diffs[0].before, "1.0.0");
        assert_eq!(diffs[0].after, "1.1.0");
    }

    #[test]
    fn compare_reports_new_or_removed_tools() {
        let a = snapshot([("claude", "1.0.0")]);
        let b = snapshot([("claude", "1.0.0"), ("codex", "0.9")]);
        let diffs = compare(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].tool, "codex");
        assert_eq!(diffs[0].before, "<missing>");
        assert_eq!(diffs[0].after, "0.9");
    }

    fn snapshot<const N: usize>(entries: [(&str, &str); N]) -> VersionSnapshot {
        let mut map = BTreeMap::new();
        for (k, v) in entries {
            map.insert(k.to_string(), v.to_string());
        }
        VersionSnapshot { tools: map }
    }

    #[test]
    fn load_returns_none_when_file_missing() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("does-not-exist.json");
        let got = VersionSnapshot::load_from(&path).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn roundtrip_through_save_and_load() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("versions.json");
        let snap = snapshot([("claude", "1.0.0"), ("tmux", "3.5")]);
        snap.save_to(&path).unwrap();
        let got = VersionSnapshot::load_from(&path).unwrap().unwrap();
        assert_eq!(got, snap);
    }

    #[test]
    fn save_creates_parent_dir_if_missing() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("nested/deeper/versions.json");
        let snap = snapshot([("gemini", "0.1.0")]);
        snap.save_to(&path).unwrap();
        assert!(path.exists());
    }
}
