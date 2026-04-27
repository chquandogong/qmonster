pub mod bootstrap;
pub mod config;
pub mod dashboard_state;
pub mod effects;
pub mod event_loop;
pub mod git_info;
pub mod keymap;
pub mod modal_state;
pub mod operator_actions;
pub mod path_resolution;
pub mod runtime_refresh;
pub mod safety_audit;
pub mod settings_overlay;
pub mod system_notice;
pub mod target_picker;
pub mod version_drift;

pub use bootstrap::Context;
pub use config::{
    ActionsMode, LogSensitivity, QmonsterConfig, RefreshPolicy, SafetyOverride,
    apply_safety_override,
};
pub use effects::EffectRunner;
pub use event_loop::run_once;
pub use git_info::capture_repo_panel;
pub use path_resolution::{ResolvedRoot, pick_root};
pub use safety_audit::{OverrideStats, apply_override_with_audit};
pub use system_notice::{SystemNotice, record_startup_snapshot, route_version_drift};
pub use version_drift::{
    StartupLoad, VersionDiff, VersionSnapshot, capture_versions, compare, load_startup_snapshot,
};
