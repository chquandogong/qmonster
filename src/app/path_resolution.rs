use std::path::{Path, PathBuf};

use crate::app::config::QmonsterConfig;
use crate::store::paths::QmonsterPaths;

/// How the storage root was chosen, for audit / logging purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootSource {
    Env,
    Cli,
    Config,
    Default,
}

#[derive(Debug, Clone)]
pub struct ResolvedRoot {
    pub root: PathBuf,
    pub source: RootSource,
}

impl ResolvedRoot {
    pub fn into_paths(self) -> QmonsterPaths {
        QmonsterPaths::at(self.root)
    }
}

/// Pure precedence resolver. `docs/ai/ARCHITECTURE.md §Safety
/// precedence` defines the standard order as
/// `env > CLI > user TOML > project TOML > defaults`. Codex Phase-2
/// finding #3 caught an inverted implementation; this helper exists
/// so the rule is unit-testable without shelling out to the env.
pub fn pick_root(
    cli_root: Option<&Path>,
    env_root: Option<&str>,
    config: &QmonsterConfig,
) -> ResolvedRoot {
    if let Some(env) = env_root
        && !env.is_empty()
    {
        return ResolvedRoot {
            root: PathBuf::from(env),
            source: RootSource::Env,
        };
    }
    if let Some(cli) = cli_root {
        return ResolvedRoot {
            root: cli.to_path_buf(),
            source: RootSource::Cli,
        };
    }
    if let Some(cfg_root) = config.storage.root.as_deref()
        && !cfg_root.is_empty()
    {
        return ResolvedRoot {
            root: PathBuf::from(cfg_root),
            source: RootSource::Config,
        };
    }
    ResolvedRoot {
        root: QmonsterPaths::default_root().root().to_path_buf(),
        source: RootSource::Default,
    }
}

pub fn default_config_path(cli_root: Option<&Path>, env_root: Option<&str>) -> PathBuf {
    let root = if let Some(env) = env_root
        && !env.is_empty()
    {
        PathBuf::from(env)
    } else if let Some(cli) = cli_root {
        cli.to_path_buf()
    } else {
        QmonsterPaths::default_root().root().to_path_buf()
    };
    QmonsterPaths::at(root).config_path()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config::{QmonsterConfig, StorageConfig};
    use std::path::PathBuf;

    fn cfg_with_root(root: Option<&str>) -> QmonsterConfig {
        let mut c = QmonsterConfig::defaults();
        c.storage = StorageConfig {
            root: root.map(|s| s.to_string()),
        };
        c
    }

    #[test]
    fn env_wins_when_both_env_and_cli_set() {
        let cli = Some(PathBuf::from("/cli-root"));
        let env = Some("/env-root".to_string());
        let result = pick_root(cli.as_deref(), env.as_deref(), &cfg_with_root(None));
        assert_eq!(
            result.root,
            PathBuf::from("/env-root"),
            "docs/ai/ARCHITECTURE.md defines env > CLI precedence"
        );
    }

    #[test]
    fn cli_wins_over_config_when_env_absent() {
        let cli = Some(PathBuf::from("/cli-root"));
        let result = pick_root(cli.as_deref(), None, &cfg_with_root(Some("/cfg-root")));
        assert_eq!(result.root, PathBuf::from("/cli-root"));
    }

    #[test]
    fn config_wins_over_default_when_env_and_cli_absent() {
        let result = pick_root(None, None, &cfg_with_root(Some("/cfg-root")));
        assert_eq!(result.root, PathBuf::from("/cfg-root"));
    }

    #[test]
    fn empty_env_is_treated_as_absent() {
        let result = pick_root(None, Some(""), &cfg_with_root(Some("/cfg-root")));
        assert_eq!(
            result.root,
            PathBuf::from("/cfg-root"),
            "empty env must fall through per POSIX-ish convention"
        );
    }

    #[test]
    fn default_config_path_uses_env_root_before_cli_root() {
        let cli_root = PathBuf::from("/cli-qmonster");
        let path = default_config_path(Some(&cli_root), Some("/env-qmonster"));
        assert_eq!(path, PathBuf::from("/env-qmonster/config/qmonster.toml"));
    }

    #[test]
    fn default_config_path_uses_cli_root_when_env_absent() {
        let cli_root = PathBuf::from("/cli-qmonster");
        let path = default_config_path(Some(&cli_root), None);
        assert_eq!(path, PathBuf::from("/cli-qmonster/config/qmonster.toml"));
    }
}
