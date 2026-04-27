use crate::app::config::{QmonsterConfig, TmuxSourceMode};
use crate::app::system_notice::SystemNotice;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::tmux::{ControlModeSource, PollingSource, TmuxSource};

#[derive(Debug)]
pub struct TmuxSourceBuild {
    pub source: TmuxSource,
    pub startup_notice: Option<SystemNotice>,
}

pub fn build_tmux_source(config: &QmonsterConfig) -> anyhow::Result<TmuxSourceBuild> {
    build_tmux_source_with(config, control_mode_source)
}

fn build_tmux_source_with<F>(
    config: &QmonsterConfig,
    attach_control_mode: F,
) -> anyhow::Result<TmuxSourceBuild>
where
    F: FnOnce(usize) -> anyhow::Result<TmuxSource>,
{
    match config.tmux.source {
        TmuxSourceMode::Auto => match attach_control_mode(config.tmux.capture_lines) {
            Ok(source) => Ok(TmuxSourceBuild {
                source,
                startup_notice: None,
            }),
            Err(err) => Ok(TmuxSourceBuild {
                source: polling_source(config.tmux.capture_lines),
                startup_notice: Some(SystemNotice {
                    title: "tmux source fallback".into(),
                    body: format!(
                        "auto tmux source: control_mode attach failed; using polling this session: {err}"
                    ),
                    severity: Severity::Warning,
                    source_kind: SourceKind::ProjectCanonical,
                }),
            }),
        },
        TmuxSourceMode::Polling => Ok(TmuxSourceBuild {
            source: polling_source(config.tmux.capture_lines),
            startup_notice: None,
        }),
        TmuxSourceMode::ControlMode => Ok(TmuxSourceBuild {
            source: attach_control_mode(config.tmux.capture_lines)?,
            startup_notice: None,
        }),
    }
}

fn polling_source(capture_lines: usize) -> TmuxSource {
    TmuxSource::Polling(PollingSource::new(capture_lines))
}

fn control_mode_source(capture_lines: usize) -> anyhow::Result<TmuxSource> {
    Ok(TmuxSource::ControlMode(
        ControlModeSource::attach_current(capture_lines)
            .map_err(|e| anyhow::anyhow!("attach tmux control-mode source: {e}"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_config_builds_polling_source_without_touching_control_mode() {
        let mut config = QmonsterConfig::defaults();
        config.tmux.source = TmuxSourceMode::Polling;
        let build = build_tmux_source(&config).unwrap();

        assert!(matches!(build.source, TmuxSource::Polling(_)));
        assert!(build.startup_notice.is_none());
    }

    #[test]
    fn auto_config_uses_successful_attach_without_notice() {
        let config = QmonsterConfig::defaults();
        let build = build_tmux_source_with(&config, |capture_lines| {
            Ok(TmuxSource::Polling(PollingSource::new(capture_lines)))
        })
        .unwrap();

        assert_eq!(build.source.transport_label(), "polling");
        assert!(build.startup_notice.is_none());
    }

    #[test]
    fn auto_config_falls_back_to_polling_when_control_mode_attach_fails() {
        let config = QmonsterConfig::defaults();
        let build = build_tmux_source_with(&config, |_| {
            Err(anyhow::anyhow!(
                "attach tmux control-mode source: no sessions"
            ))
        })
        .unwrap();

        assert!(matches!(build.source, TmuxSource::Polling(_)));
        let notice = build.startup_notice.expect("fallback notice");
        assert_eq!(notice.title, "tmux source fallback");
        assert!(notice.body.contains("using polling this session"));
    }

    #[test]
    fn explicit_control_mode_attach_error_is_not_silently_downgraded() {
        let mut config = QmonsterConfig::defaults();
        config.tmux.source = TmuxSourceMode::ControlMode;
        let err = build_tmux_source_with(&config, |_| {
            Err(anyhow::anyhow!(
                "attach tmux control-mode source: no sessions"
            ))
        })
        .unwrap_err();

        assert!(err.to_string().contains("no sessions"));
    }
}
