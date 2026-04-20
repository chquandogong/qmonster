use std::path::{Path, PathBuf};

/// All on-disk Qmonster locations under a single configurable root
/// (default: `~/.qmonster/`). Tests pass a `TempDir` root so no real
/// filesystem writes escape the test sandbox.
///
/// The runtime FS write boundary (r2 §3 / Codex CSF-1) is enforced
/// here: callers only ever resolve paths beneath `root()`.
#[derive(Debug, Clone)]
pub struct QmonsterPaths {
    root: PathBuf,
}

impl QmonsterPaths {
    pub fn at<P: Into<PathBuf>>(root: P) -> Self {
        Self { root: root.into() }
    }

    /// Default root: `~/.qmonster/`. Falls back to the current working
    /// directory joined with `.qmonster` if HOME is unset (edge case
    /// on some CI runners).
    pub fn default_root() -> Self {
        let root = dirs_home()
            .map(|h| h.join(".qmonster"))
            .unwrap_or_else(|| PathBuf::from(".qmonster"));
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn archive_dir(&self) -> PathBuf {
        self.root.join("archive")
    }

    pub fn snapshot_dir(&self) -> PathBuf {
        self.root.join("snapshots")
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.root.join("qmonster.db")
    }

    pub fn versions_path(&self) -> PathBuf {
        self.root.join("versions.json")
    }

    /// Create `root`, `archive/`, and `snapshots/` if missing.
    /// Idempotent; does not touch pre-existing contents.
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(self.archive_dir())?;
        std::fs::create_dir_all(self.snapshot_dir())?;
        Ok(())
    }
}

fn dirs_home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|bd| bd.home_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn paths_respect_custom_root() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        assert_eq!(paths.root(), td.path());
        assert_eq!(paths.archive_dir(), td.path().join("archive"));
        assert_eq!(paths.snapshot_dir(), td.path().join("snapshots"));
        assert_eq!(paths.sqlite_path(), td.path().join("qmonster.db"));
        assert_eq!(paths.versions_path(), td.path().join("versions.json"));
    }

    #[test]
    fn ensure_creates_all_expected_dirs() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        assert!(paths.root().is_dir());
        assert!(paths.archive_dir().is_dir());
        assert!(paths.snapshot_dir().is_dir());
    }

    #[test]
    fn ensure_is_idempotent() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        paths.ensure().unwrap(); // second call must succeed.
    }

    #[test]
    fn ensure_refuses_to_leave_the_root() {
        // Paranoia: the archive/snapshot dirs must be direct children.
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        let parent = td.path().parent().unwrap();
        assert!(paths.archive_dir().starts_with(td.path()));
        assert!(paths.snapshot_dir().starts_with(td.path()));
        assert!(!paths.archive_dir().starts_with(parent.join("..")));
    }
}
