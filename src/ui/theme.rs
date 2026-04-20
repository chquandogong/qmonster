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

pub fn label_style() -> Style {
    Style::default().fg(TEXT_DIM).bg(BADGE_BG)
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
