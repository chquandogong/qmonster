use ratatui::widgets::ListState;

use crate::app::system_notice::SystemNotice;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::tmux::polling::PaneSource;
use crate::tmux::types::{RawPaneSnapshot, WindowTarget};

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
