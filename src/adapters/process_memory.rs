//! Phase F F-1: Linux process RSS reader for tmux pane PIDs.
//!
//! tmux exposes `#{pane_pid}` — the foreground shell PID. The actual AI
//! CLI is usually a descendant (Claude is a binary `claude`, Codex/Gemini
//! launch under `node`). This module walks `/proc/<pid>/task/<pid>/children`
//! recursively (capped at depth 5 to prevent runaway loops) and returns
//! the highest-RSS descendant's RSS in MiB, preferring children whose
//! `comm` matches a known CLI name. If the pane has no readable
//! descendant, the helper returns `None` (honesty rule: Qmonster does
//! not fabricate metrics).
//!
//! Note: `comm` is matched against `KNOWN_CLI_COMMS` by exact equality.
//! Linux truncates `comm` to 15 bytes (`TASK_COMM_LEN - 1`) — long
//! binary names will not match and will be classified as non-CLI.
//! Acceptable for v1 because all known AI CLI binaries are ≤ 7 chars;
//! revisit if this list grows.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const KNOWN_CLI_COMMS: &[&str] = &["claude", "codex", "gemini", "node", "python", "python3"];

/// BFS depth cap. Real shell→CLI trees are depth 1–3 (bash → claude,
/// or bash → node → gemini-cli child); 5 leaves headroom for unusual
/// wrappers (e.g. tmux → bash → asdf → node → cli) without admitting
/// pathological trees.
const MAX_DEPTH: usize = 5;

/// Default `/proc` root. Tests pass a tempdir-rooted alternative via
/// `read_descendant_rss_mb_with_proc_root`.
pub fn read_descendant_rss_mb(pane_pid: u32) -> Option<f64> {
    read_descendant_rss_mb_with_proc_root(pane_pid, Path::new("/proc"))
}

/// Test-friendly variant: pass an alternate `/proc` root (typically a
/// `tempdir`) so the descendant walk operates on a controlled tree.
#[doc(hidden)]
pub fn read_descendant_rss_mb_with_proc_root(pane_pid: u32, proc_root: &Path) -> Option<f64> {
    // Breadth-first walk, depth-capped. Pick the candidate with the
    // best class (CLI comm beats non-CLI) and within a class the highest
    // RSS. The shell PID itself is a candidate: a pane with no AI CLI
    // child still gets a number (its shell), conveyed honestly via
    // SourceKind::Heuristic upstream.
    let mut frontier: Vec<u32> = vec![pane_pid];
    let mut visited: HashSet<u32> = HashSet::new();
    visited.insert(pane_pid);
    let mut depth = 0;
    let mut best_rss_kb: Option<u64> = None;
    let mut best_is_cli_comm = false;

    while !frontier.is_empty() && depth < MAX_DEPTH {
        let mut next: Vec<u32> = Vec::new();
        for pid in &frontier {
            if let Some((rss_kb, is_cli_comm)) = read_pid_stats(*pid, proc_root) {
                let replace = match (best_is_cli_comm, is_cli_comm) {
                    (false, true) => true,
                    (true, false) => false,
                    _ => rss_kb > best_rss_kb.unwrap_or(0),
                };
                if replace {
                    best_rss_kb = Some(rss_kb);
                    best_is_cli_comm = is_cli_comm;
                }
            }
            for child in read_children(*pid, proc_root) {
                if visited.insert(child) {
                    next.push(child);
                }
            }
        }
        frontier = next;
        depth += 1;
    }

    best_rss_kb.map(|kb| (kb as f64) / 1024.0)
}

fn read_pid_stats(pid: u32, proc_root: &Path) -> Option<(u64, bool)> {
    let status_path: PathBuf = proc_root.join(pid.to_string()).join("status");
    let status = fs::read_to_string(&status_path).ok()?;
    let mut rss_kb: Option<u64> = None;
    let mut comm: Option<String> = None;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            rss_kb = rest
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u64>().ok());
        } else if let Some(rest) = line.strip_prefix("Name:") {
            comm = Some(rest.trim().to_string());
        }
    }
    let rss = rss_kb?;
    let is_cli_comm = comm
        .as_deref()
        .map(|c| KNOWN_CLI_COMMS.contains(&c))
        .unwrap_or(false);
    Some((rss, is_cli_comm))
}

fn read_children(pid: u32, proc_root: &Path) -> Vec<u32> {
    let path: PathBuf = proc_root
        .join(pid.to_string())
        .join("task")
        .join(pid.to_string())
        .join("children");
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    raw.split_whitespace()
        .filter_map(|s| s.parse::<u32>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_proc_pid(root: &Path, pid: u32, comm: &str, rss_kb: u64, children: &[u32]) {
        let dir = root.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("status"),
            format!("Name:\t{comm}\nVmRSS:\t{rss_kb} kB\n"),
        )
        .unwrap();
        let task_dir = dir.join("task").join(pid.to_string());
        fs::create_dir_all(&task_dir).unwrap();
        let kids = children
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        fs::write(task_dir.join("children"), kids).unwrap();
    }

    #[test]
    fn highest_rss_cli_descendant_wins_over_shell() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        // bash (12345) -> claude (12400) -> python helper (12410)
        write_proc_pid(root, 12345, "bash", 4_000, &[12400]);
        write_proc_pid(root, 12400, "claude", 250_000, &[12410]);
        write_proc_pid(root, 12410, "python3", 18_000, &[]);

        let mb = read_descendant_rss_mb_with_proc_root(12345, root).unwrap();
        // claude's 250_000 kB ≈ 244.14 MiB — bigger than python3's 17.58 MiB
        assert!((mb - (250_000.0 / 1024.0)).abs() < 0.001);
    }

    #[test]
    fn missing_pane_pid_returns_none() {
        let tmp = tempdir().unwrap();
        let mb = read_descendant_rss_mb_with_proc_root(99999, tmp.path());
        assert!(mb.is_none());
    }

    #[test]
    fn cli_comm_wins_over_bigger_unknown_comm() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        // bash -> some_unknown_huge (1 GB) AND claude (200 MB)
        // claude's known-CLI comm wins despite smaller RSS, so cards
        // never report unrelated processes (e.g. accidental htop child).
        write_proc_pid(root, 1, "bash", 4_000, &[2, 3]);
        write_proc_pid(root, 2, "htop_clone", 1_000_000, &[]);
        write_proc_pid(root, 3, "claude", 200_000, &[]);

        let mb = read_descendant_rss_mb_with_proc_root(1, root).unwrap();
        assert!((mb - (200_000.0 / 1024.0)).abs() < 0.001);
    }

    #[test]
    fn shell_only_pane_returns_shell_rss() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        // Plain shell with no AI CLI child — return shell RSS so the
        // operator still sees a number; SourceKind::Heuristic conveys
        // the imprecision.
        write_proc_pid(root, 1, "bash", 4_000, &[]);
        let mb = read_descendant_rss_mb_with_proc_root(1, root).unwrap();
        assert!((mb - (4_000.0 / 1024.0)).abs() < 0.001);
    }

    #[test]
    fn corrupted_status_file_returns_none_not_panic() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let dir = root.join("1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("status"), "garbage no fields here").unwrap();
        let task = dir.join("task").join("1");
        fs::create_dir_all(&task).unwrap();
        fs::write(task.join("children"), "").unwrap();

        let mb = read_descendant_rss_mb_with_proc_root(1, root);
        assert!(mb.is_none());
    }
}
