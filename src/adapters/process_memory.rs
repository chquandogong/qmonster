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

const KNOWN_CLI_COMMS: &[&str] = &[
    "claude", "codex", "gemini", "qmonster", "node", "python", "python3",
];

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

/// Walk the descendant tree from `pane_pid` and return the full
/// `/proc/<pid>/cmdline` of the most-likely AI CLI child. Used to
/// resolve generic interpreter wrappers (e.g. tmux reports `node`
/// for a Codex pane because Codex is `node /usr/bin/codex`) into the
/// argv string operators recognise.
///
/// Returns `None` when `pane_pid` is unreadable, when no descendant
/// exists, or when every descendant is a generic shell. The caller
/// is responsible for falling back to `pane_current_command` in that
/// case.
pub fn read_descendant_cmdline(pane_pid: u32) -> Option<String> {
    read_descendant_cmdline_with_proc_root(pane_pid, Path::new("/proc"))
}

/// Test-friendly variant: pass an alternate `/proc` root.
#[doc(hidden)]
pub fn read_descendant_cmdline_with_proc_root(pane_pid: u32, proc_root: &Path) -> Option<String> {
    // BFS-with-class-priority. Class priority: a descendant whose
    // `comm` is in `KNOWN_CLI_COMMS` beats one that isn't. Within
    // each class, keep the SHALLOWEST match — that descendant is
    // the pane's operator-meaningful identity (claude in a Claude
    // pane, node-running-codex in a Codex pane). Deeper subprocesses
    // are tools the CLI launched, not the pane's identity.
    // The pane shell itself is NOT a candidate — we want the CLI
    // descendant, not the shell prompt.
    let mut frontier: Vec<u32> = vec![pane_pid];
    let mut visited: HashSet<u32> = HashSet::new();
    visited.insert(pane_pid);
    let mut depth = 0;
    let mut best: Option<(u32, bool)> = None;

    while !frontier.is_empty() && depth < MAX_DEPTH {
        let mut next: Vec<u32> = Vec::new();
        for pid in &frontier {
            // Skip the pane shell itself — only descendants are candidates.
            if depth > 0
                && let Some((_, is_cli_comm)) = read_pid_stats(*pid, proc_root)
            {
                let replace = match (best.map(|(_, c)| c), is_cli_comm) {
                    // First candidate ever — take it.
                    (None, _) => true,
                    // Upgrade non-CLI → CLI (deeper CLI beats shallower
                    // non-CLI: e.g. `bash → asdf → node /usr/bin/codex`
                    // picks node, not asdf).
                    (Some(false), true) => true,
                    // Don't downgrade CLI → non-CLI; don't replace
                    // same-class (shallowest wins) — once we have a
                    // CLI match, deeper CLI subprocesses are the
                    // CLI's tool calls, not the pane's identity.
                    _ => false,
                };
                if replace {
                    best = Some((*pid, is_cli_comm));
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

    let (best_pid, _) = best?;
    let cmdline_path = proc_root.join(best_pid.to_string()).join("cmdline");
    let raw = fs::read(cmdline_path).ok()?;
    if raw.is_empty() {
        return None;
    }
    // /proc/<pid>/cmdline is null-separated argv with a trailing null.
    // Replace nulls with spaces and trim trailing whitespace.
    let mut out = String::with_capacity(raw.len());
    for byte in raw {
        if byte == 0 {
            out.push(' ');
        } else {
            out.push(byte as char);
        }
    }
    let trimmed = out.trim_end().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
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

    const GEMINI_IDLE_FIXTURE: &str = include_str!("../../tests/fixtures/real/gemini_idle.txt");

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

    /// Write a /proc/<pid>/cmdline fixture (null-separated argv with
    /// trailing null, matching real kernel format).
    fn write_proc_cmdline(root: &Path, pid: u32, argv: &[&str]) {
        let dir = root.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        for (i, arg) in argv.iter().enumerate() {
            if i > 0 {
                bytes.push(0);
            }
            bytes.extend_from_slice(arg.as_bytes());
        }
        bytes.push(0); // trailing null per kernel format
        fs::write(dir.join("cmdline"), bytes).unwrap();
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

    #[test]
    fn parser_context_carries_pane_pid_field() {
        use crate::adapters::ParserContext;
        use crate::adapters::common::PaneTailHistory;
        use crate::domain::identity::{
            IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
        };
        use crate::policy::claude_settings::ClaudeSettings;
        use crate::policy::pricing::PricingTable;

        let id = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        };
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let ctx = ParserContext {
            identity: &id,
            tail: "",
            pricing: &pricing,
            claude_settings: &settings,
            history: &history,
            pane_pid: Some(42),
            current_path: "", // F-2: test fixture; production wires from snapshot.current_path
        };
        assert_eq!(ctx.pane_pid, Some(42));
    }

    // -----------------------------------------------------------------
    // Phase F F-1 Task 4: parse_for_with_proc_root wiring tests
    // -----------------------------------------------------------------

    #[test]
    fn parse_for_fills_process_memory_mb_for_claude_when_adapter_left_it_none() {
        use crate::adapters::ParserContext;
        use crate::adapters::common::PaneTailHistory;
        use crate::adapters::parse_for_with_proc_root;
        use crate::domain::identity::{
            IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
        };
        use crate::domain::origin::SourceKind;
        use crate::policy::claude_settings::ClaudeSettings;
        use crate::policy::pricing::PricingTable;

        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 1, "bash", 4_000, &[2]);
        write_proc_pid(root, 2, "claude", 300_000, &[]);

        let id = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        };
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let ctx = ParserContext {
            identity: &id,
            tail: "",
            pricing: &pricing,
            claude_settings: &settings,
            history: &history,
            pane_pid: Some(1),
            current_path: "", // F-2: test fixture; production wires from snapshot.current_path
        };

        let signals = parse_for_with_proc_root(&ctx, root);
        let mem = signals
            .process_memory_mb
            .expect("F-1: claude pane should get RSS-derived MEM");
        assert!((mem.value - (300_000.0 / 1024.0)).abs() < 0.001);
        assert_eq!(mem.source_kind, SourceKind::Heuristic);
    }

    #[test]
    fn parse_for_does_not_overwrite_gemini_provider_official_memory() {
        use crate::adapters::ParserContext;
        use crate::adapters::common::PaneTailHistory;
        use crate::adapters::parse_for_with_proc_root;
        use crate::domain::identity::{
            IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
        };
        use crate::domain::origin::SourceKind;
        use crate::policy::claude_settings::ClaudeSettings;
        use crate::policy::pricing::PricingTable;

        let tail = GEMINI_IDLE_FIXTURE;

        let id = ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Gemini,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        };
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();

        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 99, "node", 999_000, &[]);

        let ctx = ParserContext {
            identity: &id,
            tail,
            pricing: &pricing,
            claude_settings: &settings,
            history: &history,
            pane_pid: Some(99),
            current_path: "", // F-2: test fixture; production wires from snapshot.current_path
        };
        let signals = parse_for_with_proc_root(&ctx, root);

        // If the Gemini adapter recognized the fixture's memory column, it
        // populated process_memory_mb with ProviderOfficial. Task 4's /proc
        // fill MUST NOT clobber it with the 999_000 kB Heuristic value.
        let mem = signals
            .process_memory_mb
            .expect("Gemini status-table memory column should be parsed");
        assert_eq!(mem.source_kind, SourceKind::ProviderOfficial);
        // Defense in depth: tie the test to the fixture's known content. If
        // gemini_idle.txt is intentionally updated, this assertion fails first
        // and points the reader at the fixture; the distance check below stays
        // as a backup that catches Heuristic-overwrite regressions.
        assert!(
            (mem.value - 118.8).abs() < 0.5,
            "Gemini fixture should report 118.8 MB; got {} (drift in fixture?)",
            mem.value
        );
        // Don't assert the exact MB — depends on the fixture. Just ensure
        // /proc's 999_000 kB ≈ 975.6 MiB did NOT win.
        assert!(
            (mem.value - (999_000.0 / 1024.0)).abs() > 100.0,
            "Gemini value {} MB should not equal /proc-derived ~975 MB",
            mem.value
        );
    }

    #[test]
    fn cmdline_returns_node_descendant_argv() {
        // Mirrors a real Codex pane: bash (pane shell) → node /usr/bin/codex
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 100, "bash", 4_000, &[200]);
        write_proc_pid(root, 200, "node", 80_000, &[]);
        write_proc_cmdline(root, 200, &["node", "/usr/bin/codex"]);

        let cmdline = read_descendant_cmdline_with_proc_root(100, root).unwrap();
        assert_eq!(cmdline, "node /usr/bin/codex");
    }

    #[test]
    fn cmdline_skips_pane_shell_itself() {
        // Pane shell with no descendant — nothing to enhance.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 1, "bash", 4_000, &[]);
        write_proc_cmdline(root, 1, &["bash"]);
        assert!(read_descendant_cmdline_with_proc_root(1, root).is_none());
    }

    #[test]
    fn cmdline_prefers_known_cli_comm_over_unknown() {
        // bash → htop_clone (unknown comm) AND node (CLI comm)
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 1, "bash", 4_000, &[2, 3]);
        write_proc_pid(root, 2, "htop_clone", 5_000, &[]);
        write_proc_cmdline(root, 2, &["htop_clone", "--no-color"]);
        write_proc_pid(root, 3, "node", 80_000, &[]);
        write_proc_cmdline(root, 3, &["node", "/usr/local/bin/gemini"]);

        let cmdline = read_descendant_cmdline_with_proc_root(1, root).unwrap();
        assert_eq!(cmdline, "node /usr/local/bin/gemini");
    }

    #[test]
    fn cmdline_returns_none_when_pane_pid_missing() {
        let tmp = tempdir().unwrap();
        assert!(read_descendant_cmdline_with_proc_root(99999, tmp.path()).is_none());
    }

    #[test]
    fn cmdline_returns_none_for_empty_cmdline_file() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 1, "bash", 4_000, &[2]);
        write_proc_pid(root, 2, "node", 80_000, &[]);
        // /proc/<pid>/cmdline can be empty for kernel threads.
        let dir = root.join("2");
        fs::write(dir.join("cmdline"), b"").unwrap();

        assert!(read_descendant_cmdline_with_proc_root(1, root).is_none());
    }

    #[test]
    fn cmdline_handles_argv_with_null_separators() {
        // Real kernel format: argv joined by NUL bytes with a trailing NUL.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 1, "bash", 4_000, &[2]);
        write_proc_pid(root, 2, "python3", 50_000, &[]);
        write_proc_cmdline(
            root,
            2,
            &["python3", "-m", "agent.run", "--config", "x.toml"],
        );

        let cmdline = read_descendant_cmdline_with_proc_root(1, root).unwrap();
        assert_eq!(cmdline, "python3 -m agent.run --config x.toml");
    }

    #[test]
    fn cmdline_keeps_shallowest_cli_when_cli_spawns_cli_tool() {
        // Realistic Claude pane: bash → claude → node (a tool subprocess
        // claude spawned for an MCP server, e.g.). The pane's identity
        // is `claude`, NOT the deeper node tool.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 1, "bash", 4_000, &[2]);
        write_proc_pid(root, 2, "claude", 200_000, &[3]);
        write_proc_cmdline(root, 2, &["claude", "--print", "review"]);
        write_proc_pid(root, 3, "node", 80_000, &[]);
        write_proc_cmdline(root, 3, &["node", "/tmp/mcp-server.js"]);

        let cmdline = read_descendant_cmdline_with_proc_root(1, root).unwrap();
        assert_eq!(
            cmdline, "claude --print review",
            "deeper node tool must NOT replace shallower claude (pane identity)"
        );
    }

    #[test]
    fn cmdline_resolves_qmonster_pane() {
        // Qmonster pane: bash → qmonster. qmonster is in KNOWN_CLI_COMMS
        // so it wins as the pane's identity.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 1, "bash", 4_000, &[2]);
        write_proc_pid(root, 2, "qmonster", 30_000, &[3]);
        write_proc_cmdline(
            root,
            2,
            &["qmonster", "--config", "/home/u/.qmonster/qmonster.toml"],
        );
        // qmonster might spawn `tmux` or `git` as a child, but those
        // are not in KNOWN_CLI_COMMS and even if they were the
        // shallowest-CLI rule keeps qmonster.
        write_proc_pid(root, 3, "tmux", 5_000, &[]);
        write_proc_cmdline(root, 3, &["tmux", "capture-pane", "-p"]);

        let cmdline = read_descendant_cmdline_with_proc_root(1, root).unwrap();
        assert_eq!(
            cmdline, "qmonster --config /home/u/.qmonster/qmonster.toml",
            "qmonster pane must resolve to qmonster, not its tmux subprocess"
        );
    }

    #[test]
    fn cmdline_upgrades_non_cli_wrapper_to_deeper_cli() {
        // bash → asdf (not in KNOWN_CLI_COMMS) → node (in KNOWN_CLI_COMMS).
        // Real-world Node version managers: depth 1 is the wrapper,
        // depth 2 is the actual interpreter we want to surface.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_proc_pid(root, 1, "bash", 4_000, &[2]);
        write_proc_pid(root, 2, "asdf", 10_000, &[3]);
        write_proc_cmdline(root, 2, &["asdf", "exec", "node"]);
        write_proc_pid(root, 3, "node", 80_000, &[]);
        write_proc_cmdline(root, 3, &["node", "/usr/bin/gemini"]);

        let cmdline = read_descendant_cmdline_with_proc_root(1, root).unwrap();
        assert_eq!(
            cmdline, "node /usr/bin/gemini",
            "non-CLI wrapper at depth 1 must be replaced by deeper CLI at depth 2"
        );
    }
}
