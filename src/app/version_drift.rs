use std::collections::BTreeMap;
use std::process::Command;

/// Snapshot of CLI tool versions known to Qmonster. Kept in-memory in
/// Phase 1 (no persistence); later phases can load/save from disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionSnapshot {
    pub tools: BTreeMap<String, String>,
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
}
