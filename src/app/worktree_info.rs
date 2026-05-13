use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeSplitSuggestion {
    pub command: String,
    pub branch_name: String,
    pub path: String,
}

pub(crate) fn suggest_worktree_split_command(
    current_path: &str,
    current_branch: &str,
) -> Option<WorktreeSplitSuggestion> {
    let current_path = current_path.trim();
    if current_path.is_empty() {
        return None;
    }

    let repo_root = repo_root(Path::new(current_path))?;
    if !is_clean(&repo_root)? {
        return None;
    }

    let current_branch = if current_branch.trim().is_empty() {
        run_git(&repo_root, &["branch", "--show-current"])?
    } else {
        current_branch.trim().to_string()
    };
    let branch_slug = slug_for_ref(&current_branch)?;
    let branch_name = first_available_branch_name(&repo_root, &format!("{branch_slug}-split"))?;
    let repo_name = repo_root
        .file_name()
        .and_then(|s| s.to_str())
        .map(slug_for_path_component)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "worktree".into());
    let parent = repo_root.parent()?;
    let path_branch = slug_for_path_component(&branch_name);
    let path = first_available_path(parent.join(format!("{repo_name}-{path_branch}")));

    Some(WorktreeSplitSuggestion {
        command: format!(
            "git -C {} worktree add -b {} {} HEAD",
            shell_quote_path(&repo_root),
            shell_quote(&branch_name),
            shell_quote_path(&path)
        ),
        branch_name,
        path: path.display().to_string(),
    })
}

fn repo_root(current_path: &Path) -> Option<PathBuf> {
    run_git(current_path, &["rev-parse", "--show-toplevel"]).map(|raw| PathBuf::from(raw.trim()))
}

fn is_clean(repo_root: &Path) -> Option<bool> {
    run_git(repo_root, &["status", "--porcelain"]).map(|raw| raw.trim().is_empty())
}

fn first_available_branch_name(repo_root: &Path, base: &str) -> Option<String> {
    for idx in 0..100 {
        let candidate = if idx == 0 {
            base.to_string()
        } else {
            format!("{base}-{idx}")
        };
        if !branch_exists(repo_root, &candidate) {
            return Some(candidate);
        }
    }
    None
}

fn branch_exists(repo_root: &Path, branch: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn first_available_path(base: PathBuf) -> PathBuf {
    if !base.exists() {
        return base;
    }
    let parent = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = base
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("worktree")
        .to_string();
    for idx in 2..100 {
        let candidate = parent.join(format!("{stem}-{idx}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-next"))
}

fn slug_for_ref(value: &str) -> Option<String> {
    let mut slug = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("//") {
        slug = slug.replace("//", "/");
    }
    let slug = slug.trim_matches(&['-', '/'][..]).to_string();
    if slug.is_empty() || slug.starts_with('-') || slug.contains("..") {
        None
    } else {
        Some(slug)
    }
}

fn slug_for_path_component(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.display().to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_git(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Whether the pane's cwd is the primary checkout of a git repo or a
/// linked worktree created via `git worktree add`. None when the cwd
/// is not a git working tree at all, or when git is unavailable.
///
/// This is a *derived* fact about local git state — distinct from the
/// `signals.worktree_path` metric, which carries the value the
/// provider's statusline printed (and is stamped `ProviderOfficial`).
/// Keep the two off each other's source-kind contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorktreeRole {
    Primary,
    Linked { parent_repo_root: PathBuf },
}

/// Resolve the worktree role of `current_path`. Runs `git -C <path>
/// rev-parse --git-common-dir --git-dir` once; spawning git is the
/// only side effect. Returns `None` on empty input, non-existent
/// cwd, non-git cwd, git failure, or any parse anomaly.
pub(crate) fn resolve_worktree_role(current_path: &str) -> Option<WorktreeRole> {
    let trimmed = current_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cwd = Path::new(trimmed);
    if !cwd.exists() {
        return None;
    }

    let common_dir_raw = run_git(cwd, &["rev-parse", "--git-common-dir"])?;
    let git_dir_raw = run_git(cwd, &["rev-parse", "--git-dir"])?;

    let common_dir = canonicalize_git_path(cwd, &common_dir_raw)?;
    let git_dir = canonicalize_git_path(cwd, &git_dir_raw)?;

    if common_dir == git_dir {
        return Some(WorktreeRole::Primary);
    }

    let parent_repo_root = common_dir.parent()?.to_path_buf();
    Some(WorktreeRole::Linked { parent_repo_root })
}

/// `git rev-parse --git-{common-,}dir` returns paths relative to the
/// cwd it was invoked from. Canonicalize so Primary detection (path
/// equality) is robust against `.git` vs absolute spellings.
fn canonicalize_git_path(cwd: &Path, raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = Path::new(trimmed);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        cwd.join(candidate)
    };
    std::fs::canonicalize(&absolute).ok()
}

/// Default TTL for cached worktree-role lookups. At nominal 2 s tmux
/// poll cadence this covers ~5 ticks, so a six-pane setup spawns at
/// most ~6 git processes per 10 s instead of ~6 per tick. Tune via
/// `with_ttl` if real-world poll cadence diverges.
pub(crate) const WORKTREE_ROLE_TTL: Duration = Duration::from_secs(10);

/// Default cap; per-pane keys with cleanup at the LRU-by-insertion
/// edge are sufficient — operators rarely watch >128 distinct cwds.
pub(crate) const WORKTREE_ROLE_CACHE_CAPACITY: usize = 128;

#[derive(Debug)]
pub(crate) struct WorktreeRoleCache {
    entries: std::collections::HashMap<String, CachedRole>,
    insertion_order: std::collections::VecDeque<String>,
    ttl: Duration,
    capacity: usize,
    spawn_count: usize,
}

#[derive(Debug, Clone)]
struct CachedRole {
    role: Option<WorktreeRole>,
    cached_at: std::time::Instant,
}

impl Default for WorktreeRoleCache {
    fn default() -> Self {
        Self::with_capacity_and_ttl(WORKTREE_ROLE_CACHE_CAPACITY, WORKTREE_ROLE_TTL)
    }
}

impl WorktreeRoleCache {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "TTL-only constructor for callers that accept the default capacity; tests use it today and production wiring in Task 3 may swap to it if poll cadence diverges."
        )
    )]
    pub(crate) fn with_ttl(ttl: Duration) -> Self {
        Self::with_capacity_and_ttl(WORKTREE_ROLE_CACHE_CAPACITY, ttl)
    }

    pub(crate) fn with_capacity_and_ttl(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: std::collections::HashMap::with_capacity(capacity),
            insertion_order: std::collections::VecDeque::with_capacity(capacity),
            ttl,
            capacity: capacity.max(1),
            spawn_count: 0,
        }
    }

    pub(crate) fn lookup(&mut self, current_path: &str) -> Option<WorktreeRole> {
        self.lookup_at(current_path, std::time::Instant::now())
    }

    pub(crate) fn lookup_at(
        &mut self,
        current_path: &str,
        now: std::time::Instant,
    ) -> Option<WorktreeRole> {
        if let Some(cached) = self.entries.get(current_path)
            && now.saturating_duration_since(cached.cached_at) < self.ttl
        {
            return cached.role.clone();
        }
        let role = resolve_worktree_role(current_path);
        self.spawn_count += 1;
        self.insert(current_path.to_string(), role.clone(), now);
        role
    }

    fn insert(&mut self, key: String, role: Option<WorktreeRole>, now: std::time::Instant) {
        if !self.entries.contains_key(&key) {
            self.insertion_order.push_back(key.clone());
            while self.insertion_order.len() > self.capacity {
                if let Some(oldest) = self.insertion_order.pop_front() {
                    self.entries.remove(&oldest);
                }
            }
        }
        self.entries.insert(
            key,
            CachedRole {
                role,
                cached_at: now,
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn spawn_count(&self) -> usize {
        self.spawn_count
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("git command runs");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn clean_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        git(dir.path(), &["init", "-b", "main"]);
        git(
            dir.path(),
            &["config", "user.email", "qmonster@example.test"],
        );
        git(dir.path(), &["config", "user.name", "Qmonster Test"]);
        std::fs::write(dir.path().join("README.md"), "test\n").expect("write fixture");
        git(dir.path(), &["add", "README.md"]);
        git(dir.path(), &["commit", "-m", "init"]);
        dir
    }

    #[test]
    fn clean_repo_gets_executable_worktree_add_command() {
        let repo = clean_repo();

        let suggestion = suggest_worktree_split_command(repo.path().to_str().unwrap(), "main")
            .expect("clean repo should produce a command");

        assert_eq!(suggestion.branch_name, "main-split");
        assert!(
            suggestion.path.ends_with("main-split"),
            "path should be deterministic and sibling-safe: {suggestion:?}"
        );
        assert!(
            suggestion.command.starts_with("git -C "),
            "command must be executable shell, got: {}",
            suggestion.command
        );
        assert!(
            suggestion.command.contains(" worktree add -b "),
            "command must create a worktree, got: {}",
            suggestion.command
        );
        assert!(
            !suggestion.command.contains('<') && !suggestion.command.trim_start().starts_with('#'),
            "copyable command must not contain placeholders/comments: {}",
            suggestion.command
        );
    }

    #[test]
    fn dirty_repo_does_not_get_copyable_worktree_command() {
        let repo = clean_repo();
        std::fs::write(repo.path().join("dirty.txt"), "dirty\n").expect("dirty fixture");

        assert!(
            suggest_worktree_split_command(repo.path().to_str().unwrap(), "main").is_none(),
            "dirty repos need a next-step/checkpoint, not a copyable branch split command"
        );
    }

    #[test]
    fn empty_branch_falls_back_to_git_current_branch() {
        let repo = clean_repo();

        let suggestion = suggest_worktree_split_command(repo.path().to_str().unwrap(), "")
            .expect("git current branch should fill missing provider branch");

        assert_eq!(suggestion.branch_name, "main-split");
    }

    #[test]
    fn branch_slash_is_preserved_for_branch_but_flattened_for_path() {
        let repo = clean_repo();

        let suggestion =
            suggest_worktree_split_command(repo.path().to_str().unwrap(), "feat/worktree-copy")
                .expect("slash branch should still produce command");

        assert_eq!(suggestion.branch_name, "feat/worktree-copy-split");
        assert!(
            suggestion.path.ends_with("feat-worktree-copy-split"),
            "path must be a flat sibling directory, got: {suggestion:?}"
        );
    }

    #[test]
    fn resolve_worktree_role_returns_primary_for_main_checkout() {
        let repo = clean_repo();

        let role = resolve_worktree_role(repo.path().to_str().unwrap())
            .expect("clean repo should resolve as a git repo");

        assert!(
            matches!(role, WorktreeRole::Primary),
            "main checkout must resolve as Primary, got {role:?}"
        );
    }

    #[test]
    fn resolve_worktree_role_returns_linked_with_parent_root_for_added_worktree() {
        // Wrap both the primary checkout and the linked worktree inside a
        // single parent tempdir so both directories are cleaned on drop and
        // parallel `cargo test` runs do not race on a shared `/tmp/linked-wt`.
        let parent = tempfile::tempdir().expect("tempdir");
        let primary = parent.path().join("primary");
        std::fs::create_dir(&primary).expect("create primary dir");

        // Inline clean_repo() against the explicit `primary` path.
        git(&primary, &["init", "-b", "main"]);
        git(&primary, &["config", "user.email", "qmonster@example.test"]);
        git(&primary, &["config", "user.name", "Qmonster Test"]);
        std::fs::write(primary.join("README.md"), "test\n").expect("write fixture");
        git(&primary, &["add", "README.md"]);
        git(&primary, &["commit", "-m", "init"]);

        let worktree_dir = parent.path().join("linked");
        git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "feat-x",
                worktree_dir.to_str().unwrap(),
                "HEAD",
            ],
        );

        let role = resolve_worktree_role(worktree_dir.to_str().unwrap())
            .expect("linked worktree should resolve as a git repo");

        match role {
            WorktreeRole::Linked { parent_repo_root } => {
                let canonical_repo = std::fs::canonicalize(&primary).unwrap();
                let canonical_parent = std::fs::canonicalize(&parent_repo_root).unwrap();
                assert_eq!(
                    canonical_parent, canonical_repo,
                    "Linked.parent_repo_root must resolve to the primary checkout"
                );
            }
            other => panic!("linked worktree must resolve as Linked, got {other:?}"),
        }
    }

    #[test]
    fn resolve_worktree_role_returns_none_for_non_git_cwd() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            resolve_worktree_role(dir.path().to_str().unwrap()).is_none(),
            "non-git cwd must return None, not Primary"
        );
    }

    #[test]
    fn resolve_worktree_role_returns_none_for_empty_or_missing_cwd() {
        assert!(resolve_worktree_role("").is_none());
        assert!(resolve_worktree_role("   ").is_none());
        assert!(resolve_worktree_role("/this/path/does/not/exist/qmonster-plan").is_none());
    }

    #[test]
    fn worktree_role_cache_hits_within_ttl_and_misses_after() {
        use std::time::{Duration, Instant};
        let repo = clean_repo();
        let key = repo.path().to_str().unwrap().to_string();

        let mut cache = WorktreeRoleCache::with_ttl(Duration::from_secs(10));
        let t0 = Instant::now();
        let first = cache.lookup_at(&key, t0);
        let second = cache.lookup_at(&key, t0 + Duration::from_secs(5));
        let third = cache.lookup_at(&key, t0 + Duration::from_secs(11));

        assert_eq!(
            first, second,
            "within-TTL lookup must return the cached value"
        );
        assert_eq!(
            cache.spawn_count(),
            2,
            "TTL expiry must trigger a re-resolve"
        );
        assert!(matches!(third, Some(WorktreeRole::Primary)));
    }

    #[test]
    fn worktree_role_cache_caps_entries_and_evicts_oldest() {
        use std::time::{Duration, Instant};
        let mut cache = WorktreeRoleCache::with_capacity_and_ttl(2, Duration::from_secs(60));
        let t0 = Instant::now();

        let _ = cache.lookup_at("/nonexistent/a", t0);
        let _ = cache.lookup_at("/nonexistent/b", t0);
        let _ = cache.lookup_at("/nonexistent/c", t0);

        assert_eq!(cache.len(), 2, "cache must respect the capacity cap");
        assert!(
            !cache.contains_key("/nonexistent/a"),
            "oldest entry must be evicted when capacity is exceeded"
        );
    }

    #[test]
    fn worktree_role_cache_caches_none_results_too() {
        use std::time::{Duration, Instant};
        let mut cache = WorktreeRoleCache::with_ttl(Duration::from_secs(10));
        let t0 = Instant::now();

        let first = cache.lookup_at("/nonexistent/qmonster-plan", t0);
        let second = cache.lookup_at("/nonexistent/qmonster-plan", t0 + Duration::from_secs(1));

        assert!(first.is_none() && second.is_none());
        assert_eq!(
            cache.spawn_count(),
            1,
            "None results must be memoized too — otherwise every non-git pane spawns git every tick"
        );
    }
}
