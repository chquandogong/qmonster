use crate::app::config::{QmonsterConfig, TmuxSourceMode};
use crate::tmux::{ControlModeSource, PollingSource, TmuxSource};

pub fn build_tmux_source(config: &QmonsterConfig) -> anyhow::Result<TmuxSource> {
    match config.tmux.source {
        TmuxSourceMode::Polling => Ok(TmuxSource::Polling(PollingSource::new(
            config.tmux.capture_lines,
        ))),
        TmuxSourceMode::ControlMode => Ok(TmuxSource::ControlMode(
            ControlModeSource::attach_current(config.tmux.capture_lines)
                .map_err(|e| anyhow::anyhow!("attach tmux control-mode source: {e}"))?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_config_builds_polling_source_without_touching_control_mode() {
        let config = QmonsterConfig::defaults();
        let source = build_tmux_source(&config).unwrap();

        assert!(matches!(source, TmuxSource::Polling(_)));
    }
}
