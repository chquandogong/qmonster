use crate::tmux::control_mode::ControlModeSource;
use crate::tmux::polling::{PaneSource, PollingError, PollingSource};
use crate::tmux::types::{RawPaneSnapshot, WindowTarget};

#[derive(Debug)]
pub enum TmuxSource {
    Polling(PollingSource),
    ControlMode(ControlModeSource),
}

impl TmuxSource {
    pub fn transport_label(&self) -> &'static str {
        match self {
            Self::Polling(_) => "polling",
            Self::ControlMode(_) => "control_mode",
        }
    }
}

impl PaneSource for TmuxSource {
    fn list_panes(
        &self,
        target: Option<&WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        match self {
            Self::Polling(source) => source.list_panes(target),
            Self::ControlMode(source) => source.list_panes(target),
        }
    }

    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        match self {
            Self::Polling(source) => source.current_target(),
            Self::ControlMode(source) => source.current_target(),
        }
    }

    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        match self {
            Self::Polling(source) => source.available_targets(),
            Self::ControlMode(source) => source.available_targets(),
        }
    }

    fn capture_tail(&self, pane_id: &str, lines: usize) -> Result<String, PollingError> {
        match self {
            Self::Polling(source) => source.capture_tail(pane_id, lines),
            Self::ControlMode(source) => source.capture_tail(pane_id, lines),
        }
    }

    fn send_keys(&self, pane_id: &str, text: &str) -> Result<(), PollingError> {
        match self {
            Self::Polling(source) => source.send_keys(pane_id, text),
            Self::ControlMode(source) => source.send_keys(pane_id, text),
        }
    }

    fn send_key(&self, pane_id: &str, key: &str) -> Result<(), PollingError> {
        match self {
            Self::Polling(source) => source.send_key(pane_id, key),
            Self::ControlMode(source) => source.send_key(pane_id, key),
        }
    }
}
