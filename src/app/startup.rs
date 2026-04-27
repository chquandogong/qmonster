use std::path::Path;

use anyhow::Context as _;

use crate::app::bootstrap::Context;
use crate::app::config::{QmonsterConfig, load_from_path};
use crate::app::path_resolution::{RootSource, default_config_path, pick_root};
use crate::app::safety_audit::apply_override_with_audit;
use crate::app::system_notice::{SystemNotice, record_startup_snapshot, route_version_drift};
use crate::app::tmux_source::build_tmux_source;
use crate::app::version_drift::{
    StartupLoad, VersionSnapshot, capture_versions, load_startup_snapshot,
};
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::notify::desktop::DesktopNotifier;
use crate::policy::claude_settings::{ClaudeSettings, ClaudeSettingsError};
use crate::policy::pricing::PricingTable;
use crate::store::{
    ArchiveWriter, EventSink, InMemorySink, QmonsterPaths, SnapshotWriter, SqliteAuditSink, sweep,
};
use crate::tmux::TmuxSource;

pub struct StartupOptions<'a> {
    pub config_path: Option<&'a Path>,
    pub root: Option<&'a Path>,
    pub set: &'a [String],
    pub env_root: Option<&'a str>,
}

pub struct StartupRuntime {
    pub ctx: Context<TmuxSource, DesktopNotifier>,
    pub paths: QmonsterPaths,
    pub root_source: RootSource,
    pub versions: VersionSnapshot,
    pub startup_notices: Vec<SystemNotice>,
    pub snapshot_writer: SnapshotWriter,
}

pub fn build_startup_runtime(options: StartupOptions<'_>) -> anyhow::Result<StartupRuntime> {
    let default_config_path = default_config_path(options.root, options.env_root);
    let loaded_config_path = options
        .config_path
        .map(|path| path.to_path_buf())
        .or_else(|| {
            if default_config_path.exists() {
                Some(default_config_path.clone())
            } else {
                None
            }
        });
    let config = match loaded_config_path.as_ref() {
        Some(path) => load_from_path(path).with_context(|| format!("loading {path:?}"))?,
        None => QmonsterConfig::defaults(),
    };
    let writable_config_path = options
        .config_path
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| default_config_path.clone());
    let pairs = parse_set_pairs(options.set)?;

    let resolved = pick_root(options.root, options.env_root, &config);
    let root_source = resolved.source.clone();
    let paths = resolved.into_paths();
    paths.ensure().context("ensure ~/.qmonster layout")?;

    let sink: Box<dyn EventSink> = match SqliteAuditSink::open(&paths.sqlite_path()) {
        Ok(db) => Box::new(db),
        Err(e) => {
            eprintln!(
                "qmonster: falling back to in-memory audit sink ({e}); events \
                 will not survive restart this session"
            );
            Box::new(InMemorySink::new())
        }
    };

    let source = build_tmux_source(&config)?;
    let notifier = DesktopNotifier;
    let archive = ArchiveWriter::new(paths.clone(), config.logging.big_output_chars);
    let pricing = load_pricing(&paths, &*sink);
    let claude_settings = load_claude_settings(&*sink);

    let mut ctx = Context::new(config, source, notifier, sink)
        .with_archive(archive)
        .with_pricing(pricing)
        .with_claude_settings(claude_settings)
        .with_config_path(writable_config_path);

    if !pairs.is_empty() {
        let refs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let stats = apply_override_with_audit(&mut ctx.config, &refs, &*ctx.sink);
        if stats.rejected + stats.unknown > 0 {
            eprintln!(
                "qmonster: {} override(s) rejected, {} unknown key(s); see audit log",
                stats.rejected, stats.unknown
            );
        }
    }

    sweep_retention(&paths, &ctx);
    let (versions, startup_notices) = capture_startup_versions(&paths, &ctx);
    let snapshot_writer = SnapshotWriter::new(paths.clone());

    Ok(StartupRuntime {
        ctx,
        paths,
        root_source,
        versions,
        startup_notices,
        snapshot_writer,
    })
}

fn parse_set_pairs(set: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    for kv in set {
        let Some((key, value)) = kv.split_once('=') else {
            anyhow::bail!("--set expects key=value, got {kv}");
        };
        pairs.push((key.trim().into(), value.trim().into()));
    }
    Ok(pairs)
}

fn load_pricing(paths: &QmonsterPaths, sink: &dyn EventSink) -> PricingTable {
    let pricing_path = paths.pricing_path();
    match PricingTable::load_from_toml(&pricing_path) {
        Ok(table) => table,
        Err(crate::policy::pricing::PricingError::Io(io_err))
            if io_err.kind() == std::io::ErrorKind::NotFound =>
        {
            PricingTable::empty()
        }
        Err(e) => {
            sink.record(AuditEvent {
                kind: AuditEventKind::PricingLoadFailed,
                pane_id: "n/a".into(),
                severity: Severity::Warning,
                summary: format!("pricing load failed at {}: {e}", pricing_path.display()),
                provider: None,
                role: None,
            });
            eprintln!(
                "qmonster: failed to load pricing table at {}: {e}; cost badges disabled this session",
                pricing_path.display()
            );
            PricingTable::empty()
        }
    }
}

fn load_claude_settings(sink: &dyn EventSink) -> ClaudeSettings {
    match ClaudeSettings::default_path() {
        Some(path) => match ClaudeSettings::load_from_path(&path) {
            Ok(settings) => settings,
            Err(ClaudeSettingsError::Io(io)) if io.kind() == std::io::ErrorKind::NotFound => {
                ClaudeSettings::empty()
            }
            Err(e) => {
                sink.record(AuditEvent {
                    kind: AuditEventKind::ClaudeSettingsLoadFailed,
                    pane_id: "n/a".into(),
                    severity: Severity::Warning,
                    summary: format!("claude settings load failed at {}: {}", path.display(), e),
                    provider: None,
                    role: None,
                });
                eprintln!(
                    "qmonster: failed to load claude settings at {}: {e}; claude model badge disabled this session",
                    path.display()
                );
                ClaudeSettings::empty()
            }
        },
        None => ClaudeSettings::empty(),
    }
}

fn sweep_retention(paths: &QmonsterPaths, ctx: &Context<TmuxSource, DesktopNotifier>) {
    match sweep(paths, ctx.config.logging.retention_days) {
        Ok(report) => {
            if report.files_removed > 0 {
                ctx.sink.record(AuditEvent {
                    kind: AuditEventKind::RetentionSwept,
                    pane_id: "n/a".into(),
                    severity: Severity::Safe,
                    summary: format!(
                        "retention: removed {} file(s), {} byte(s); kept {}",
                        report.files_removed, report.bytes_removed, report.files_kept
                    ),
                    provider: None,
                    role: None,
                });
            }
        }
        Err(e) => eprintln!("qmonster: retention sweep failed: {e}"),
    }
}

fn capture_startup_versions(
    paths: &QmonsterPaths,
    ctx: &Context<TmuxSource, DesktopNotifier>,
) -> (VersionSnapshot, Vec<SystemNotice>) {
    let startup = load_startup_snapshot(&*ctx.sink, &paths.versions_path());
    let may_save_fresh = startup.may_save_fresh();
    let fresh = capture_versions();
    let mut startup_notices = Vec::new();
    match &startup {
        StartupLoad::Previous(prev) => {
            startup_notices = route_version_drift(prev, &fresh, &*ctx.sink);
        }
        StartupLoad::Fresh => {}
        StartupLoad::Corrupted(_) => {
            startup_notices.push(SystemNotice {
                title: "versions.json corrupted".into(),
                body: format!(
                    "{} left in place for inspection; drift detection skipped this session",
                    paths.versions_path().display()
                ),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
            });
        }
    }
    record_startup_snapshot(&*ctx.sink, &fresh);
    if may_save_fresh && let Err(e) = fresh.save_to(&paths.versions_path()) {
        eprintln!("qmonster: could not persist version snapshot: {e}");
    }
    (fresh, startup_notices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_set_pairs_trims_key_and_value() {
        let pairs = parse_set_pairs(&[" actions.mode = observe_only ".to_string()]).unwrap();

        assert_eq!(pairs, vec![("actions.mode".into(), "observe_only".into())]);
    }

    #[test]
    fn parse_set_pairs_rejects_missing_equals() {
        let err = parse_set_pairs(&["actions.mode".to_string()]).unwrap_err();

        assert!(err.to_string().contains("--set expects key=value"));
    }
}
