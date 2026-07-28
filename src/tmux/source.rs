use crate::herdr::HerdrSource;
use crate::tmux::control_mode::ControlModeSource;
use crate::tmux::polling::{PaneSource, PollingError, PollingSource};
use crate::tmux::types::{RawPaneSnapshot, WindowTarget};

#[derive(Debug)]
pub enum TmuxSource {
    Polling(PollingSource),
    ControlMode(ControlModeSource),
    /// herdr workspace-manager backend (v3.2.0). Lives behind the same
    /// enum so every caller keeps a single source type; the enum name
    /// predates multi-backend support and is kept to avoid a
    /// mechanical rename this slice.
    Herdr(HerdrSource),
}

impl TmuxSource {
    pub fn transport_label(&self) -> &'static str {
        match self {
            Self::Polling(_) => "polling",
            Self::ControlMode(_) => "control_mode",
            Self::Herdr(_) => "herdr",
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
            Self::Herdr(source) => source.list_panes(target),
        }
    }

    fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
        match self {
            Self::Polling(source) => source.current_target(),
            Self::ControlMode(source) => source.current_target(),
            Self::Herdr(source) => source.current_target(),
        }
    }

    fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
        match self {
            Self::Polling(source) => source.available_targets(),
            Self::ControlMode(source) => source.available_targets(),
            Self::Herdr(source) => source.available_targets(),
        }
    }

    fn capture_tail(&self, pane_id: &str, lines: usize) -> Result<String, PollingError> {
        match self {
            Self::Polling(source) => source.capture_tail(pane_id, lines),
            Self::ControlMode(source) => source.capture_tail(pane_id, lines),
            Self::Herdr(source) => source.capture_tail(pane_id, lines),
        }
    }

    fn send_keys(&self, pane_id: &str, text: &str) -> Result<(), PollingError> {
        match self {
            Self::Polling(source) => source.send_keys(pane_id, text),
            Self::ControlMode(source) => source.send_keys(pane_id, text),
            Self::Herdr(source) => source.send_keys(pane_id, text),
        }
    }

    fn send_key(&self, pane_id: &str, key: &str) -> Result<(), PollingError> {
        match self {
            Self::Polling(source) => source.send_key(pane_id, key),
            Self::ControlMode(source) => source.send_key(pane_id, key),
            Self::Herdr(source) => source.send_key(pane_id, key),
        }
    }

    fn prefers_global_default(&self) -> bool {
        match self {
            Self::Polling(source) => source.prefers_global_default(),
            Self::ControlMode(source) => source.prefers_global_default(),
            Self::Herdr(source) => source.prefers_global_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_label_covers_herdr() {
        let source = TmuxSource::Herdr(HerdrSource::new(24, false, None));
        assert_eq!(source.transport_label(), "herdr");
    }
}
