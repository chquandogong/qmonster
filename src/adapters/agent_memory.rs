//! Phase F F-2 (v1.23.0): agent-memory file scanner.
//!
//! Discovers provider-specific memory files associated with a pane:
//!   Claude: `<current_path>/CLAUDE.md`, `~/.claude/CLAUDE.md`, and
//!     all `.md` files inside `~/.claude/projects/<encoded>/memory/`
//!     where `<encoded>` is `current_path` with `/` replaced by `-`
//!     (Claude's project-directory encoding — see this project's
//!     own MEMORY.md path as the reference example).
//!   Codex: `<current_path>/AGENTS.md`, `~/.codex/AGENTS.md`,
//!     `~/.codex/AGENTS.override.md`.
//!   Gemini: `<current_path>/GEMINI.md`, `<current_path>/.gemini/GEMINI.md`,
//!     `~/.gemini/GEMINI.md`.
//!
//! Returns total bytes across the discovered set. Each file's size
//! contribution is capped at `MAX_FILE_BYTES` (1 MiB) so a single
//! pathological file can't dominate the count. Files that are absent
//! or unreadable contribute 0; the helper never errors. Returns
//! `None` when no files were found at all (honesty rule — distinct
//! from "found zero-byte files which sum to 0").
//!
//! `SourceKind::Heuristic` is the appropriate label upstream because
//! file existence is not proof the CLI actually loaded the bytes; it
//! is an observation that the bytes are available to be loaded.

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::identity::Provider;

/// Max bytes counted per file. Files larger than this contribute
/// exactly `MAX_FILE_BYTES` to the total. Real CLAUDE.md / AGENTS.md
/// / GEMINI.md are typically < 50 KB; a 1 MiB cap leaves headroom for
/// reasonably large rule files while preventing a 100 MiB log dump
/// in CLAUDE.md from breaking the metric.
const MAX_FILE_BYTES: u64 = 1_048_576;

/// Production entry. Resolves `home_dir` via
/// `directories::BaseDirs::new()` and dispatches to the test seam.
pub fn read_agent_memory_bytes(provider: Provider, current_path: &str) -> Option<u64> {
    let home = directories::BaseDirs::new().map(|bd| bd.home_dir().to_path_buf())?;
    read_agent_memory_bytes_with_filesystem(provider, Path::new(current_path), Some(&home))
}

/// Test-friendly variant. `current_path_root` is treated as the
/// project root; `home_dir` (when `Some`) overrides the home
/// resolution so tests can construct fake `~/.claude/`, `~/.codex/`,
/// `~/.gemini/` trees under a `tempdir`.
#[doc(hidden)]
pub fn read_agent_memory_bytes_with_filesystem(
    provider: Provider,
    current_path_root: &Path,
    home_dir: Option<&Path>,
) -> Option<u64> {
    let candidates = candidate_files(provider, current_path_root, home_dir);
    if candidates.is_empty() {
        return None;
    }
    let mut total: u64 = 0;
    let mut any_existed = false;
    for path in candidates {
        if let Some(bytes) = file_size_capped(&path) {
            any_existed = true;
            total = total.saturating_add(bytes);
        }
    }
    if any_existed { Some(total) } else { None }
}

/// Returns the canonical candidate file list for a provider. The
/// caller filters out non-existent paths; this function is
/// pure-deterministic so the test fixtures know exactly which paths
/// to populate.
fn candidate_files(
    provider: Provider,
    current_path_root: &Path,
    home_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    match provider {
        Provider::Claude => {
            out.push(current_path_root.join("CLAUDE.md"));
            if let Some(home) = home_dir {
                out.push(home.join(".claude").join("CLAUDE.md"));
                let encoded = encode_claude_project_dir(current_path_root);
                let memory_dir = home
                    .join(".claude")
                    .join("projects")
                    .join(encoded)
                    .join("memory");
                if let Ok(entries) = fs::read_dir(&memory_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.extension().and_then(|s| s.to_str()) == Some("md") {
                            out.push(p);
                        }
                    }
                }
            }
        }
        Provider::Codex => {
            out.push(current_path_root.join("AGENTS.md"));
            if let Some(home) = home_dir {
                out.push(home.join(".codex").join("AGENTS.md"));
                out.push(home.join(".codex").join("AGENTS.override.md"));
            }
        }
        Provider::Gemini => {
            out.push(current_path_root.join("GEMINI.md"));
            out.push(current_path_root.join(".gemini").join("GEMINI.md"));
            if let Some(home) = home_dir {
                out.push(home.join(".gemini").join("GEMINI.md"));
            }
        }
        Provider::Qmonster | Provider::Unknown => {}
    }
    out
}

/// Encodes an absolute path as Claude does for `~/.claude/projects/`.
/// Replaces every `/` with `-`. For `/home/chquan/Qmonster` this
/// yields `-home-chquan-Qmonster`, matching the canonical example
/// path observed live (`~/.claude/projects/-home-chquan-Qmonster/...`).
fn encode_claude_project_dir(current_path_root: &Path) -> String {
    let s = current_path_root.to_string_lossy().to_string();
    s.replace('/', "-")
}

/// Returns the file size in bytes, capped at `MAX_FILE_BYTES`. Returns
/// `None` when the file does not exist or is otherwise unreadable
/// (the metadata read failed). The caller treats `None` as "not in
/// the count" rather than an error.
fn file_size_capped(path: &Path) -> Option<u64> {
    let md = fs::metadata(path).ok()?;
    if !md.is_file() {
        return None;
    }
    Some(md.len().min(MAX_FILE_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_file(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn claude_sums_project_root_and_home_md_files() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        write_file(&project.join("CLAUDE.md"), &"a".repeat(40_000));
        write_file(&home.join(".claude").join("CLAUDE.md"), &"b".repeat(8_000));

        let total =
            read_agent_memory_bytes_with_filesystem(Provider::Claude, &project, Some(&home))
                .expect("Claude pane with two memory files should report Some");
        assert_eq!(total, 48_000);
    }

    #[test]
    fn claude_includes_project_memory_directory_md_files() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        write_file(&project.join("CLAUDE.md"), &"a".repeat(10_000));

        let encoded = encode_claude_project_dir(&project);
        let memory_dir = home
            .join(".claude")
            .join("projects")
            .join(encoded)
            .join("memory");
        write_file(&memory_dir.join("MEMORY.md"), &"c".repeat(20_000));
        write_file(&memory_dir.join("topic_one.md"), &"d".repeat(5_000));
        // Non-md siblings must NOT be summed.
        write_file(&memory_dir.join("ignore.txt"), &"x".repeat(99_999));

        let total =
            read_agent_memory_bytes_with_filesystem(Provider::Claude, &project, Some(&home))
                .unwrap();
        assert_eq!(total, 35_000);
    }

    #[test]
    fn codex_sums_project_agents_and_home_pair() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        write_file(&project.join("AGENTS.md"), &"a".repeat(12_000));
        write_file(&home.join(".codex").join("AGENTS.md"), &"b".repeat(3_000));
        write_file(
            &home.join(".codex").join("AGENTS.override.md"),
            &"c".repeat(1_500),
        );

        let total = read_agent_memory_bytes_with_filesystem(Provider::Codex, &project, Some(&home))
            .unwrap();
        assert_eq!(total, 16_500);
    }

    #[test]
    fn gemini_sums_project_root_dotdir_and_home() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        write_file(&project.join("GEMINI.md"), &"a".repeat(7_000));
        write_file(
            &project.join(".gemini").join("GEMINI.md"),
            &"b".repeat(2_000),
        );
        write_file(&home.join(".gemini").join("GEMINI.md"), &"c".repeat(11_000));

        let total =
            read_agent_memory_bytes_with_filesystem(Provider::Gemini, &project, Some(&home))
                .unwrap();
        assert_eq!(total, 20_000);
    }

    #[test]
    fn per_file_size_cap_clamps_pathological_files() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        // 5 MiB file — only MAX_FILE_BYTES (1 MiB) should count.
        write_file(&project.join("CLAUDE.md"), &"x".repeat(5 * 1_048_576));

        let total =
            read_agent_memory_bytes_with_filesystem(Provider::Claude, &project, Some(&home))
                .unwrap();
        assert_eq!(total, MAX_FILE_BYTES);
    }

    #[test]
    fn missing_files_returns_none() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        // No files written — every candidate path is absent.
        let total =
            read_agent_memory_bytes_with_filesystem(Provider::Claude, &project, Some(&home));
        assert_eq!(total, None);
    }

    #[test]
    fn qmonster_provider_returns_none() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        // Even with the right files in place, Qmonster monitor pane
        // has no memory surface — return None, not 0.
        write_file(&project.join("CLAUDE.md"), &"a".repeat(10_000));
        let total =
            read_agent_memory_bytes_with_filesystem(Provider::Qmonster, &project, Some(&home));
        assert_eq!(total, None);
    }

    #[test]
    fn unknown_provider_returns_none() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        write_file(&project.join("CLAUDE.md"), &"a".repeat(10_000));
        let total =
            read_agent_memory_bytes_with_filesystem(Provider::Unknown, &project, Some(&home));
        assert_eq!(total, None);
    }

    #[test]
    fn encode_claude_project_dir_replaces_slashes_with_dashes() {
        let path = Path::new("/home/chquan/Qmonster");
        assert_eq!(encode_claude_project_dir(path), "-home-chquan-Qmonster");
    }

    #[test]
    fn parse_for_fills_agent_memory_bytes_for_claude_when_files_exist() {
        use crate::adapters::ParserContext;
        use crate::adapters::common::PaneTailHistory;
        use crate::adapters::parse_for_with_environment;
        use crate::domain::identity::{
            IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
        };
        use crate::domain::origin::SourceKind;
        use crate::policy::claude_settings::ClaudeSettings;
        use crate::policy::pricing::PricingTable;
        use std::path::Path;
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        write_file(&project.join("CLAUDE.md"), &"a".repeat(15_000));
        write_file(&home.join(".claude").join("CLAUDE.md"), &"b".repeat(2_000));

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
        let project_str = project.to_string_lossy().to_string();
        let ctx = ParserContext {
            identity: &id,
            tail: "",
            pricing: &pricing,
            claude_settings: &settings,
            history: &history,
            pane_pid: None,
            current_path: &project_str,
        };

        let signals = parse_for_with_environment(&ctx, Path::new("/proc"), Some(&home));
        let mem = signals
            .agent_memory_bytes
            .expect("Claude pane with files should fill agent_memory_bytes");
        assert_eq!(mem.value, 17_000);
        assert_eq!(mem.source_kind, SourceKind::Heuristic);
    }

    #[test]
    fn parse_for_skips_agent_memory_when_current_path_is_empty() {
        use crate::adapters::ParserContext;
        use crate::adapters::common::PaneTailHistory;
        use crate::adapters::parse_for_with_environment;
        use crate::domain::identity::{
            IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
        };
        use crate::policy::claude_settings::ClaudeSettings;
        use crate::policy::pricing::PricingTable;
        use std::path::Path;
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let home = tmp.path().join("home");
        // Even with a global ~/.claude/CLAUDE.md present, an empty
        // current_path means we cannot attribute it to a pane — must
        // skip rather than fabricate.
        write_file(&home.join(".claude").join("CLAUDE.md"), &"a".repeat(99_000));

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
            pane_pid: None,
            current_path: "",
        };

        let signals = parse_for_with_environment(&ctx, Path::new("/proc"), Some(&home));
        assert!(signals.agent_memory_bytes.is_none());
    }
}
