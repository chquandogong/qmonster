use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use crate::app::keymap::{
    ScrollDir, list_row_at, move_selection, page_selection, rect_contains, select_first,
    select_last,
};
use crate::app::system_notice::SystemNotice;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::tmux::polling::PaneSource;
use crate::tmux::types::{RawPaneSnapshot, WindowTarget};
use crate::ui::dashboard::{close_button_rect, target_picker_rects};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPickerStage {
    Session,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetChoiceValue {
    AllSessions,
    Session(String),
    Window(WindowTarget),
}

#[derive(Debug, Clone)]
pub struct TargetChoice {
    pub label: String,
    pub value: TargetChoiceValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetPickerOutcome {
    AdvanceToWindows(String),
    Close(String),
}

pub struct TargetPickerController<'a> {
    pub open: &'a mut bool,
    pub stage: &'a mut TargetPickerStage,
    pub session: &'a mut Option<String>,
    pub state: &'a mut ListState,
    pub choices: &'a mut Vec<TargetChoice>,
    pub preview_title: &'a mut String,
    pub preview_lines: &'a mut Vec<String>,
    pub selected_target: &'a mut Option<WindowTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetPickerAction {
    None,
    TargetSwitched(String),
}

impl TargetPickerController<'_> {
    fn refresh_choices<P: PaneSource>(&mut self, source: &P) {
        refresh_target_choices(
            source,
            *self.stage,
            self.session.as_deref(),
            self.choices,
            self.state,
            self.selected_target.as_ref(),
        );
        self.refresh_preview(source);
    }

    fn refresh_preview<P: PaneSource>(&mut self, source: &P) {
        refresh_target_preview(
            source,
            self.choices,
            self.state,
            self.preview_title,
            self.preview_lines,
        );
    }

    fn move_selection<P: PaneSource>(&mut self, source: &P, step: isize) {
        move_selection(self.state, self.choices.len(), step);
        self.refresh_preview(source);
    }

    fn page_selection<P: PaneSource>(&mut self, source: &P, dir: ScrollDir) {
        page_selection(self.state, self.choices.len(), 6, dir);
        self.refresh_preview(source);
    }

    fn back_to_sessions<P: PaneSource>(&mut self, source: &P) {
        if *self.stage == TargetPickerStage::Window {
            *self.stage = TargetPickerStage::Session;
            *self.session = None;
            self.refresh_choices(source);
        }
    }

    fn apply_selected<P: PaneSource>(&mut self, source: &P) -> TargetPickerAction {
        match apply_target_choice(
            *self.stage,
            self.session.as_deref(),
            self.choices,
            self.state,
            self.selected_target,
        ) {
            Some(TargetPickerOutcome::AdvanceToWindows(session_name)) => {
                *self.stage = TargetPickerStage::Window;
                *self.session = Some(session_name);
                self.refresh_choices(source);
                TargetPickerAction::None
            }
            Some(TargetPickerOutcome::Close(label)) => {
                *self.open = false;
                TargetPickerAction::TargetSwitched(label)
            }
            None => TargetPickerAction::None,
        }
    }
}

pub fn open_target_picker<P: PaneSource>(source: &P, mut picker: TargetPickerController<'_>) {
    *picker.stage = TargetPickerStage::Session;
    *picker.session = None;
    picker.refresh_choices(source);
    *picker.open = true;
}

pub fn handle_target_picker_key<P: PaneSource>(
    source: &P,
    mut picker: TargetPickerController<'_>,
    key: KeyCode,
) -> TargetPickerAction {
    match key {
        KeyCode::Esc | KeyCode::Char('t') => {
            *picker.open = false;
            TargetPickerAction::None
        }
        KeyCode::Left | KeyCode::Backspace => {
            picker.back_to_sessions(source);
            TargetPickerAction::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            picker.move_selection(source, -1);
            TargetPickerAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            picker.move_selection(source, 1);
            TargetPickerAction::None
        }
        KeyCode::PageUp => {
            picker.page_selection(source, ScrollDir::Up);
            TargetPickerAction::None
        }
        KeyCode::PageDown => {
            picker.page_selection(source, ScrollDir::Down);
            TargetPickerAction::None
        }
        KeyCode::Home => {
            select_first(picker.state, picker.choices.len());
            picker.refresh_preview(source);
            TargetPickerAction::None
        }
        KeyCode::End => {
            select_last(picker.state, picker.choices.len());
            picker.refresh_preview(source);
            TargetPickerAction::None
        }
        KeyCode::Enter => picker.apply_selected(source),
        _ => TargetPickerAction::None,
    }
}

pub fn handle_target_picker_mouse<P: PaneSource>(
    source: &P,
    mut picker: TargetPickerController<'_>,
    viewport: Rect,
    event: MouseEvent,
) -> TargetPickerAction {
    let rects = target_picker_rects(viewport);
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && rect_contains(close_button_rect(rects.list), event.column, event.row)
    {
        *picker.open = false;
        return TargetPickerAction::None;
    }

    match event.kind {
        MouseEventKind::ScrollUp if rect_contains(rects.list, event.column, event.row) => {
            picker.move_selection(source, -1);
            TargetPickerAction::None
        }
        MouseEventKind::ScrollDown if rect_contains(rects.list, event.column, event.row) => {
            picker.move_selection(source, 1);
            TargetPickerAction::None
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let Some(row) = list_row_at(rects.list, event) else {
                return TargetPickerAction::None;
            };
            let Some(idx) = target_choice_index_at_row(picker.choices, picker.state, row) else {
                return TargetPickerAction::None;
            };
            picker.state.select(Some(idx));
            picker.refresh_preview(source);
            picker.apply_selected(source)
        }
        _ => TargetPickerAction::None,
    }
}

pub fn refresh_target_choices<P: PaneSource>(
    source: &P,
    stage: TargetPickerStage,
    session_name: Option<&str>,
    choices: &mut Vec<TargetChoice>,
    state: &mut ListState,
    selected: Option<&WindowTarget>,
) {
    let targets = source.available_targets().unwrap_or_default();
    *choices = match stage {
        TargetPickerStage::Session => build_session_choices(&targets),
        TargetPickerStage::Window => {
            build_window_choices(&targets, session_name.unwrap_or_default())
        }
    };
    sync_target_choice_selection(state, stage, session_name, choices, selected);
}

pub fn refresh_target_preview<P: PaneSource>(
    source: &P,
    choices: &[TargetChoice],
    state: &ListState,
    preview_title: &mut String,
    preview_lines: &mut Vec<String>,
) {
    let Some(choice) = state.selected().and_then(|idx| choices.get(idx)) else {
        *preview_title = "Panes".into();
        preview_lines.clear();
        return;
    };

    match &choice.value {
        TargetChoiceValue::AllSessions => {
            *preview_title = "All Sessions".into();
            *preview_lines = vec![
                "all sessions".into(),
                "choose a specific session to inspect its windows and panes".into(),
            ];
        }
        TargetChoiceValue::Session(session_name) => {
            *preview_title = format!("Session · {session_name}");
            *preview_lines = build_session_preview(source, session_name);
        }
        TargetChoiceValue::Window(target) => {
            *preview_title = format!("Window · {}", target.label());
            *preview_lines = build_window_preview(source, target);
        }
    }
}

pub fn apply_target_choice(
    stage: TargetPickerStage,
    session_name: Option<&str>,
    choices: &[TargetChoice],
    state: &ListState,
    selected_target: &mut Option<WindowTarget>,
) -> Option<TargetPickerOutcome> {
    let idx = state.selected()?;
    let choice = choices.get(idx)?;
    match (&choice.value, stage) {
        (TargetChoiceValue::AllSessions, TargetPickerStage::Session) => {
            *selected_target = None;
            Some(TargetPickerOutcome::Close("all sessions".into()))
        }
        (TargetChoiceValue::Session(session), TargetPickerStage::Session) => {
            Some(TargetPickerOutcome::AdvanceToWindows(session.clone()))
        }
        (TargetChoiceValue::Window(target), TargetPickerStage::Window) => {
            if let Some(session) = session_name
                && target.session_name != session
            {
                return None;
            }
            *selected_target = Some(target.clone());
            Some(TargetPickerOutcome::Close(target.label()))
        }
        _ => None,
    }
}

pub fn target_choice_index_at_row(
    choices: &[TargetChoice],
    state: &ListState,
    row: u16,
) -> Option<usize> {
    let mut remaining = row;
    for (idx, choice) in choices.iter().enumerate().skip(state.offset()) {
        let height = choice.label.lines().count().max(1) as u16;
        if remaining < height {
            return Some(idx);
        }
        remaining = remaining.saturating_sub(height);
    }
    None
}

pub fn target_picker_title(stage: TargetPickerStage, session_name: Option<&str>) -> String {
    match (stage, session_name) {
        (TargetPickerStage::Session, _) => "Choose Session".into(),
        (TargetPickerStage::Window, Some(session)) => format!("Choose Window · {session}"),
        (TargetPickerStage::Window, None) => "Choose Window".into(),
    }
}

pub fn target_picker_hint(stage: TargetPickerStage) -> &'static str {
    match stage {
        TargetPickerStage::Session => {
            "click select · click [x] close · wheel scroll · ↑/↓ item · PgUp/PgDn page · Home/End · Enter open · Esc close"
        }
        TargetPickerStage::Window => {
            "click watch · click [x] close · wheel scroll · ↑/↓ item · PgUp/PgDn page · Home/End · Enter watch · ←/Backspace sessions · Esc close"
        }
    }
}

pub fn target_label(target: Option<&WindowTarget>) -> String {
    target
        .map(WindowTarget::label)
        .unwrap_or_else(|| "all sessions".into())
}

pub fn initial_target<P: PaneSource>(source: &P) -> Option<WindowTarget> {
    source
        .current_target()
        .ok()
        .flatten()
        .or_else(|| source.available_targets().ok()?.into_iter().next())
}

pub fn target_switched_notice(label: &str) -> SystemNotice {
    SystemNotice {
        title: "target switched".into(),
        body: format!("now watching {label}"),
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
    }
}

fn build_session_preview<P: PaneSource>(source: &P, session_name: &str) -> Vec<String> {
    let mut targets: Vec<WindowTarget> = source
        .available_targets()
        .unwrap_or_default()
        .into_iter()
        .filter(|target| target.session_name == session_name)
        .collect();
    targets.sort();
    if targets.is_empty() {
        return vec!["no windows in this session".into()];
    }

    let mut lines = Vec::new();
    for (idx, target) in targets.iter().enumerate() {
        if idx > 0 {
            lines.push(String::new());
        }
        let panes = source.list_panes(Some(target)).unwrap_or_default();
        push_window_tree(&mut lines, target, &panes);
    }
    lines
}

fn build_window_preview<P: PaneSource>(source: &P, target: &WindowTarget) -> Vec<String> {
    let panes = source.list_panes(Some(target)).unwrap_or_default();
    if panes.is_empty() {
        return vec!["no panes found in this window".into()];
    }
    let mut lines = Vec::new();
    push_window_tree(&mut lines, target, &panes);
    lines
}

fn push_window_tree(lines: &mut Vec<String>, target: &WindowTarget, panes: &[RawPaneSnapshot]) {
    let pane_count = panes.len();
    lines.push(format!(
        "window {} ({})",
        target.window_index,
        if pane_count == 1 {
            "1 pane".to_string()
        } else {
            format!("{pane_count} panes")
        }
    ));
    if panes.is_empty() {
        lines.push("└─ no panes found".into());
        return;
    }
    for (idx, pane) in panes.iter().enumerate() {
        let branch = if idx + 1 == panes.len() {
            "└─"
        } else {
            "├─"
        };
        lines.push(format!("{branch} {}", pane_preview_label(pane)));
    }
}

fn pane_preview_label(pane: &RawPaneSnapshot) -> String {
    let active = if pane.active { "*" } else { " " };
    let title = if pane.title.is_empty() {
        "untitled pane"
    } else {
        pane.title.as_str()
    };
    let mut label = format!("{active} {} · {}", pane.pane_id, title);
    if !pane.current_command.is_empty() && pane.current_command != pane.title {
        label.push_str(&format!(" :: {}", pane.current_command));
    }
    if pane.dead {
        label.push_str(" [dead]");
    }
    label
}

fn sync_target_choice_selection(
    state: &mut ListState,
    stage: TargetPickerStage,
    session_name: Option<&str>,
    choices: &[TargetChoice],
    selected: Option<&WindowTarget>,
) {
    if choices.is_empty() {
        state.select(None);
        return;
    }
    let selected_index = match stage {
        TargetPickerStage::Session => {
            let current_session = selected.map(|target| target.session_name.as_str());
            choices
                .iter()
                .position(|choice| match (&choice.value, current_session) {
                    (TargetChoiceValue::AllSessions, None) => true,
                    (TargetChoiceValue::Session(choice_session), Some(current)) => {
                        choice_session == current
                    }
                    _ => false,
                })
                .unwrap_or(0)
        }
        TargetPickerStage::Window => choices
            .iter()
            .position(|choice| match (&choice.value, selected, session_name) {
                (TargetChoiceValue::Window(choice_target), Some(current), Some(session)) => {
                    choice_target == current && current.session_name == session
                }
                _ => false,
            })
            .unwrap_or(0),
    };
    state.select(Some(selected_index));
}

fn build_session_choices(targets: &[WindowTarget]) -> Vec<TargetChoice> {
    let mut sessions: Vec<String> = targets
        .iter()
        .map(|target| target.session_name.clone())
        .collect();
    sessions.sort();
    sessions.dedup();
    let mut choices = vec![TargetChoice {
        label: "all sessions · all windows".into(),
        value: TargetChoiceValue::AllSessions,
    }];
    for session in sessions {
        let mut session_targets: Vec<WindowTarget> = targets
            .iter()
            .filter(|target| target.session_name == session)
            .cloned()
            .collect();
        session_targets.sort();
        choices.push(TargetChoice {
            label: session_choice_label(&session, &session_targets),
            value: TargetChoiceValue::Session(session),
        });
    }
    choices
}

fn build_window_choices(targets: &[WindowTarget], session_name: &str) -> Vec<TargetChoice> {
    let mut session_targets: Vec<WindowTarget> = targets
        .iter()
        .filter(|target| target.session_name == session_name)
        .cloned()
        .collect();
    session_targets.sort();
    session_targets
        .into_iter()
        .map(|target| TargetChoice {
            label: window_choice_label(&target),
            value: TargetChoiceValue::Window(target),
        })
        .collect()
}

fn session_choice_label(session_name: &str, targets: &[WindowTarget]) -> String {
    let mut lines = vec![format!(
        "{session_name} ({})",
        if targets.len() == 1 {
            "1 window".to_string()
        } else {
            format!("{} windows", targets.len())
        }
    )];
    for (idx, target) in targets.iter().enumerate() {
        let branch = if idx + 1 == targets.len() {
            "└─"
        } else {
            "├─"
        };
        lines.push(format!("{branch} window {}", target.window_index));
    }
    lines.join("\n")
}

fn window_choice_label(target: &WindowTarget) -> String {
    format!("{} · window {}", target.session_name, target.window_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::polling::{PaneSource, PollingError};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    struct PickerSource {
        panes: Vec<RawPaneSnapshot>,
        current: Option<WindowTarget>,
    }

    impl PickerSource {
        fn new() -> Self {
            Self {
                panes: vec![
                    pane("a", "0", "%1"),
                    pane("a", "1", "%2"),
                    pane("b", "0", "%3"),
                ],
                current: None,
            }
        }

        fn with_current(mut self, current: WindowTarget) -> Self {
            self.current = Some(current);
            self
        }
    }

    impl PaneSource for PickerSource {
        fn list_panes(
            &self,
            target: Option<&WindowTarget>,
        ) -> Result<Vec<RawPaneSnapshot>, PollingError> {
            Ok(self
                .panes
                .iter()
                .filter(|pane| {
                    target.is_none_or(|target| {
                        pane.session_name == target.session_name
                            && pane.window_index == target.window_index
                    })
                })
                .cloned()
                .collect())
        }

        fn current_target(&self) -> Result<Option<WindowTarget>, PollingError> {
            Ok(self.current.clone())
        }

        fn available_targets(&self) -> Result<Vec<WindowTarget>, PollingError> {
            let mut targets: Vec<WindowTarget> = self
                .panes
                .iter()
                .map(|pane| WindowTarget {
                    session_name: pane.session_name.clone(),
                    window_index: pane.window_index.clone(),
                })
                .collect();
            targets.sort();
            targets.dedup();
            Ok(targets)
        }

        fn capture_tail(&self, _pane_id: &str, _lines: usize) -> Result<String, PollingError> {
            Ok(String::new())
        }

        fn send_keys(&self, _pane_id: &str, _text: &str) -> Result<(), PollingError> {
            Ok(())
        }
    }

    struct PickerHarness {
        open: bool,
        stage: TargetPickerStage,
        session: Option<String>,
        state: ListState,
        choices: Vec<TargetChoice>,
        preview_title: String,
        preview_lines: Vec<String>,
        selected_target: Option<WindowTarget>,
    }

    impl Default for PickerHarness {
        fn default() -> Self {
            Self {
                open: false,
                stage: TargetPickerStage::Session,
                session: None,
                state: ListState::default(),
                choices: Vec::new(),
                preview_title: String::new(),
                preview_lines: Vec::new(),
                selected_target: None,
            }
        }
    }

    impl PickerHarness {
        fn controller(&mut self) -> TargetPickerController<'_> {
            TargetPickerController {
                open: &mut self.open,
                stage: &mut self.stage,
                session: &mut self.session,
                state: &mut self.state,
                choices: &mut self.choices,
                preview_title: &mut self.preview_title,
                preview_lines: &mut self.preview_lines,
                selected_target: &mut self.selected_target,
            }
        }
    }

    fn target(session_name: &str, window_index: &str) -> WindowTarget {
        WindowTarget {
            session_name: session_name.into(),
            window_index: window_index.into(),
        }
    }

    fn pane(session_name: &str, window_index: &str, pane_id: &str) -> RawPaneSnapshot {
        RawPaneSnapshot {
            session_name: session_name.into(),
            window_index: window_index.into(),
            pane_id: pane_id.into(),
            title: "claude".into(),
            current_command: "claude".into(),
            current_path: "/repo".into(),
            active: false,
            dead: false,
            tail: String::new(),
        }
    }

    #[test]
    fn initial_target_prefers_current_window_target() {
        let source = PickerSource::new().with_current(target("b", "0"));

        assert_eq!(initial_target(&source), Some(target("b", "0")));
    }

    #[test]
    fn initial_target_falls_back_to_first_available_target() {
        let source = PickerSource::new();

        assert_eq!(initial_target(&source), Some(target("a", "0")));
    }

    #[test]
    fn target_choice_index_accounts_for_multiline_tree_items() {
        let choices = vec![
            TargetChoice {
                label: "session-a\n├─ window 0\n└─ pane %1".into(),
                value: TargetChoiceValue::Session("session-a".into()),
            },
            TargetChoice {
                label: "session-b\n└─ window 1".into(),
                value: TargetChoiceValue::Session("session-b".into()),
            },
        ];
        let mut state = ListState::default();
        state.select(Some(0));

        assert_eq!(target_choice_index_at_row(&choices, &state, 0), Some(0));
        assert_eq!(target_choice_index_at_row(&choices, &state, 2), Some(0));
        assert_eq!(target_choice_index_at_row(&choices, &state, 3), Some(1));
    }

    #[test]
    fn open_target_picker_resets_to_session_choices() {
        let source = PickerSource::new();
        let mut harness = PickerHarness {
            stage: TargetPickerStage::Window,
            session: Some("a".into()),
            selected_target: Some(target("b", "0")),
            ..PickerHarness::default()
        };

        open_target_picker(&source, harness.controller());

        assert!(harness.open);
        assert_eq!(harness.stage, TargetPickerStage::Session);
        assert_eq!(harness.session, None);
        assert_eq!(harness.choices.len(), 3);
        assert_eq!(harness.state.selected(), Some(2));
        assert_eq!(harness.preview_title, "Session · b");
    }

    #[test]
    fn target_picker_enter_session_advances_to_window_stage() {
        let source = PickerSource::new();
        let mut harness = PickerHarness::default();
        open_target_picker(&source, harness.controller());
        harness.state.select(Some(1));

        let action = handle_target_picker_key(&source, harness.controller(), KeyCode::Enter);

        assert_eq!(action, TargetPickerAction::None);
        assert!(harness.open);
        assert_eq!(harness.stage, TargetPickerStage::Window);
        assert_eq!(harness.session.as_deref(), Some("a"));
        assert_eq!(harness.choices.len(), 2);
    }

    #[test]
    fn target_picker_enter_window_selects_target_and_closes() {
        let source = PickerSource::new();
        let mut harness = PickerHarness::default();
        open_target_picker(&source, harness.controller());
        harness.state.select(Some(1));
        assert_eq!(
            handle_target_picker_key(&source, harness.controller(), KeyCode::Enter),
            TargetPickerAction::None
        );
        harness.state.select(Some(1));

        let action = handle_target_picker_key(&source, harness.controller(), KeyCode::Enter);

        assert_eq!(action, TargetPickerAction::TargetSwitched("a:1".into()));
        assert!(!harness.open);
        assert_eq!(harness.selected_target, Some(target("a", "1")));
    }

    #[test]
    fn target_picker_mouse_close_button_closes() {
        let source = PickerSource::new();
        let mut harness = PickerHarness::default();
        open_target_picker(&source, harness.controller());
        let viewport = Rect::new(0, 0, 120, 40);
        let close = close_button_rect(target_picker_rects(viewport).list);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: close.x,
            row: close.y,
            modifiers: KeyModifiers::empty(),
        };

        let action = handle_target_picker_mouse(&source, harness.controller(), viewport, event);

        assert_eq!(action, TargetPickerAction::None);
        assert!(!harness.open);
    }

    #[test]
    fn session_choices_are_sorted_and_deduplicated() {
        let targets = vec![
            WindowTarget {
                session_name: "b".into(),
                window_index: "1".into(),
            },
            WindowTarget {
                session_name: "a".into(),
                window_index: "0".into(),
            },
            WindowTarget {
                session_name: "b".into(),
                window_index: "0".into(),
            },
        ];

        let choices = build_session_choices(&targets);
        let labels: Vec<&str> = choices.iter().map(|choice| choice.label.as_str()).collect();

        assert_eq!(labels[0], "all sessions · all windows");
        assert!(labels[1].starts_with("a (1 window)"));
        assert!(labels[2].starts_with("b (2 windows)"));
    }

    #[test]
    fn apply_window_choice_rejects_wrong_session() {
        let target = WindowTarget {
            session_name: "session-a".into(),
            window_index: "1".into(),
        };
        let choices = vec![TargetChoice {
            label: "session-a · window 1".into(),
            value: TargetChoiceValue::Window(target),
        }];
        let mut state = ListState::default();
        state.select(Some(0));
        let mut selected = None;

        assert_eq!(
            apply_target_choice(
                TargetPickerStage::Window,
                Some("session-b"),
                &choices,
                &state,
                &mut selected,
            ),
            None
        );
        assert_eq!(selected, None);
    }
}
