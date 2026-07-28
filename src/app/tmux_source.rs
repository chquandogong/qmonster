use crate::app::config::{MuxBackend, QmonsterConfig, TmuxSourceMode};
use crate::app::system_notice::SystemNotice;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::herdr::HerdrSource;
use crate::tmux::{ControlModeSource, PollingSource, TmuxSource};

#[derive(Debug)]
pub struct TmuxSourceBuild {
    pub source: TmuxSource,
    pub startup_notice: Option<SystemNotice>,
}

pub fn build_tmux_source(config: &QmonsterConfig) -> anyhow::Result<TmuxSourceBuild> {
    build_tmux_source_with(config, control_mode_source, |key| {
        std::env::var_os(key).is_some()
    })
}

fn build_tmux_source_with<F, E>(
    config: &QmonsterConfig,
    attach_control_mode: F,
    env_present: E,
) -> anyhow::Result<TmuxSourceBuild>
where
    F: FnOnce(usize) -> anyhow::Result<TmuxSource>,
    E: Fn(&str) -> bool,
{
    // `[mux] backend` sits above the `[tmux] source` transport choice:
    // herdr replaces the tmux transports wholesale; `auto` picks herdr
    // exactly when Qmonster itself runs inside a herdr pane.
    let herdr_selected = match config.mux.backend {
        MuxBackend::Herdr => true,
        MuxBackend::Tmux => false,
        MuxBackend::Auto => env_present("HERDR_ENV") || env_present("HERDR_SOCKET_PATH"),
    };
    if herdr_selected {
        return Ok(TmuxSourceBuild {
            source: TmuxSource::Herdr(HerdrSource::new(
                config.tmux.capture_lines,
                config.mux.include_shell_panes,
                std::env::var("HERDR_PANE_ID").ok(),
            )),
            startup_notice: None,
        });
    }
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

    /// Simulates a non-herdr environment (no HERDR_* variables).
    fn no_herdr_env(_key: &str) -> bool {
        false
    }

    #[test]
    fn polling_config_builds_polling_source_without_touching_control_mode() {
        let mut config = QmonsterConfig::defaults();
        config.tmux.source = TmuxSourceMode::Polling;
        let build = build_tmux_source_with(
            &config,
            |capture_lines| Ok(TmuxSource::Polling(PollingSource::new(capture_lines))),
            no_herdr_env,
        )
        .unwrap();

        assert!(matches!(build.source, TmuxSource::Polling(_)));
        assert!(build.startup_notice.is_none());
    }

    #[test]
    fn auto_config_uses_successful_attach_without_notice() {
        let config = QmonsterConfig::defaults();
        let build = build_tmux_source_with(
            &config,
            |capture_lines| Ok(TmuxSource::Polling(PollingSource::new(capture_lines))),
            no_herdr_env,
        )
        .unwrap();

        assert_eq!(build.source.transport_label(), "polling");
        assert!(build.startup_notice.is_none());
    }

    #[test]
    fn auto_config_falls_back_to_polling_when_control_mode_attach_fails() {
        let config = QmonsterConfig::defaults();
        let build = build_tmux_source_with(
            &config,
            |_| {
                Err(anyhow::anyhow!(
                    "attach tmux control-mode source: no sessions"
                ))
            },
            no_herdr_env,
        )
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
        let err = build_tmux_source_with(
            &config,
            |_| {
                Err(anyhow::anyhow!(
                    "attach tmux control-mode source: no sessions"
                ))
            },
            no_herdr_env,
        )
        .unwrap_err();

        assert!(err.to_string().contains("no sessions"));
    }

    #[test]
    fn auto_backend_selects_herdr_when_herdr_env_present() {
        let config = QmonsterConfig::defaults();
        let build = build_tmux_source_with(
            &config,
            |_| unreachable!("herdr selection must not touch control-mode attach"),
            |key| key == "HERDR_ENV",
        )
        .unwrap();

        assert_eq!(build.source.transport_label(), "herdr");
        assert!(build.startup_notice.is_none());
    }

    #[test]
    fn auto_backend_accepts_socket_path_as_herdr_evidence() {
        let config = QmonsterConfig::defaults();
        let build = build_tmux_source_with(
            &config,
            |_| unreachable!("herdr selection must not touch control-mode attach"),
            |key| key == "HERDR_SOCKET_PATH",
        )
        .unwrap();

        assert_eq!(build.source.transport_label(), "herdr");
    }

    #[test]
    fn explicit_tmux_backend_ignores_herdr_env() {
        let mut config = QmonsterConfig::defaults();
        config.mux.backend = MuxBackend::Tmux;
        config.tmux.source = TmuxSourceMode::Polling;
        let build = build_tmux_source_with(
            &config,
            |capture_lines| Ok(TmuxSource::Polling(PollingSource::new(capture_lines))),
            |_| true,
        )
        .unwrap();

        assert!(matches!(build.source, TmuxSource::Polling(_)));
    }

    #[test]
    fn explicit_herdr_backend_selects_herdr_even_without_env() {
        let mut config = QmonsterConfig::defaults();
        config.mux.backend = MuxBackend::Herdr;
        let build = build_tmux_source_with(
            &config,
            |_| unreachable!("herdr selection must not touch control-mode attach"),
            no_herdr_env,
        )
        .unwrap();

        assert_eq!(build.source.transport_label(), "herdr");
    }
}
