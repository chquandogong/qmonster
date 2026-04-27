pub(crate) mod commands;
pub mod control_mode;
pub mod parity;
pub mod polling;
pub(crate) mod snapshots;
pub mod types;

pub use control_mode::ControlModeSource;
pub use polling::{PaneSource, PollingError, PollingSource};
pub use types::{PANE_LIST_FORMAT, RawPaneSnapshot, parse_list_panes_row};

#[derive(Debug)]
pub enum TmuxSource {
    Polling(PollingSource),
    ControlMode(ControlModeSource),
}

impl PaneSource for TmuxSource {
    fn list_panes(
        &self,
        target: Option<&types::WindowTarget>,
    ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
        match self {
            Self::Polling(source) => source.list_panes(target),
            Self::ControlMode(source) => source.list_panes(target),
        }
    }

    fn current_target(&self) -> Result<Option<types::WindowTarget>, PollingError> {
        match self {
            Self::Polling(source) => source.current_target(),
            Self::ControlMode(source) => source.current_target(),
        }
    }

    fn available_targets(&self) -> Result<Vec<types::WindowTarget>, PollingError> {
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
