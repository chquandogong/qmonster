use ratatui::style::{Color, Style};

use crate::domain::recommendation::Severity;

/// Low-saturation palette. Color is only used for severity transitions
/// and is always accompanied by the severity letter (`ui::alerts`
/// renders `[W]` / `[R]` / …).
pub const BORDER_IDLE: Color = Color::Rgb(90, 100, 120);
pub const BORDER_ACTIVE: Color = Color::Rgb(130, 160, 200);
pub const TEXT_PRIMARY: Color = Color::Rgb(210, 215, 225);
pub const TEXT_DIM: Color = Color::Rgb(120, 130, 145);
pub const BADGE_BG: Color = Color::Rgb(60, 70, 90);

pub fn severity_color(sev: Severity) -> Color {
    match sev {
        Severity::Safe => Color::Rgb(120, 135, 150),
        Severity::Good => Color::Rgb(110, 180, 150),
        Severity::Concern => Color::Rgb(200, 190, 130),
        Severity::Warning => Color::Rgb(220, 170, 100),
        Severity::Risk => Color::Rgb(220, 120, 120),
    }
}

pub fn severity_badge_style(sev: Severity) -> Style {
    Style::default()
        .fg(Color::Rgb(22, 24, 30))
        .bg(severity_color(sev))
}

pub fn label_style() -> Style {
    Style::default().fg(TEXT_DIM).bg(BADGE_BG)
}

/// Styled for a pane that has cleanly finished its work (dim gray — non-urgent).
pub fn idle_work_complete() -> Style {
    Style::default().fg(TEXT_DIM)
}

/// Styled for a pane that is stale / cause unknown (dimmer gray).
pub fn idle_stale() -> Style {
    Style::default().fg(Color::Rgb(90, 100, 120))
}

/// Styled for a pane awaiting user input (yellow — needs attention).
pub fn idle_input_wait() -> Style {
    Style::default().fg(Color::Yellow)
}

/// Styled for a pane awaiting operator permission (light yellow — elevated urgency).
pub fn idle_permission_wait() -> Style {
    Style::default().fg(Color::LightYellow)
}

/// Styled for a pane that has hit a resource limit (red — action required).
pub fn idle_limit_hit() -> Style {
    Style::default().fg(Color::Red)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_severity_maps_to_a_distinct_color() {
        let c = [
            severity_color(Severity::Safe),
            severity_color(Severity::Good),
            severity_color(Severity::Concern),
            severity_color(Severity::Warning),
            severity_color(Severity::Risk),
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j], "colors must be distinct");
            }
        }
    }
}
