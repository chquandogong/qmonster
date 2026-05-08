use std::sync::atomic::{AtomicU8, Ordering};

use ratatui::style::{Color, Modifier, Style};

use crate::domain::recommendation::Severity;

/// v1.58.0: theme variant. `Dark` is the existing low-saturation
/// palette; `HighContrast` brightens severity colors and adds BOLD to
/// dim-text styles so the dashboard stays legible on bright terminal
/// profiles or for operators who need higher visual contrast. The
/// active mode lives in a process-wide `AtomicU8` so every call site
/// reads the same value without re-plumbing config through every
/// render call. Set by `tui_loop` at startup from `[ux] theme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    HighContrast,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::HighContrast => "high_contrast",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Dark => Self::HighContrast,
            Self::HighContrast => Self::Dark,
        }
    }
}

static THEME_MODE_RAW: AtomicU8 = AtomicU8::new(0);

pub fn set_theme_mode(mode: ThemeMode) {
    let raw: u8 = match mode {
        ThemeMode::Dark => 0,
        ThemeMode::HighContrast => 1,
    };
    THEME_MODE_RAW.store(raw, Ordering::Relaxed);
}

pub fn theme_mode() -> ThemeMode {
    match THEME_MODE_RAW.load(Ordering::Relaxed) {
        1 => ThemeMode::HighContrast,
        _ => ThemeMode::Dark,
    }
}

/// Low-saturation palette. Color is only used for severity transitions
/// and is always accompanied by the severity letter (`ui::alerts`
/// renders `[W]` / `[R]` / …).
pub const BORDER_IDLE: Color = Color::Rgb(90, 100, 120);
pub const BORDER_ACTIVE: Color = Color::Rgb(130, 160, 200);
pub const TEXT_PRIMARY: Color = Color::Rgb(210, 215, 225);
pub const TEXT_DIM: Color = Color::Rgb(120, 130, 145);
pub const BADGE_BG: Color = Color::Rgb(60, 70, 90);

pub fn severity_color(sev: Severity) -> Color {
    match (theme_mode(), sev) {
        // Dark (default): existing low-saturation palette.
        (ThemeMode::Dark, Severity::Safe) => Color::Rgb(120, 135, 150),
        (ThemeMode::Dark, Severity::Good) => Color::Rgb(110, 180, 150),
        (ThemeMode::Dark, Severity::Concern) => Color::Rgb(200, 190, 130),
        (ThemeMode::Dark, Severity::Warning) => Color::Rgb(220, 170, 100),
        (ThemeMode::Dark, Severity::Risk) => Color::Rgb(220, 120, 120),
        // HighContrast: pump saturation + lightness so chips remain
        // legible on bright-profile terminals (e.g. Light+ in iTerm).
        // Distinctness invariant preserved; values picked empirically
        // against ratatui's TestBackend rendering.
        (ThemeMode::HighContrast, Severity::Safe) => Color::Rgb(150, 175, 200),
        (ThemeMode::HighContrast, Severity::Good) => Color::Rgb(80, 220, 150),
        (ThemeMode::HighContrast, Severity::Concern) => Color::Rgb(255, 220, 90),
        (ThemeMode::HighContrast, Severity::Warning) => Color::Rgb(255, 165, 50),
        (ThemeMode::HighContrast, Severity::Risk) => Color::Rgb(255, 90, 90),
    }
}

pub fn severity_badge_style(sev: Severity) -> Style {
    let fg = match theme_mode() {
        ThemeMode::Dark => Color::Rgb(22, 24, 30),
        // HighContrast keeps a near-black fg but adds BOLD for legibility.
        ThemeMode::HighContrast => Color::Rgb(18, 18, 22),
    };
    let mut s = Style::default().fg(fg).bg(severity_color(sev));
    if matches!(theme_mode(), ThemeMode::HighContrast) {
        s = s.add_modifier(Modifier::BOLD);
    }
    s
}

pub fn label_style() -> Style {
    let mut s = Style::default().fg(TEXT_DIM).bg(BADGE_BG);
    if matches!(theme_mode(), ThemeMode::HighContrast) {
        // Brighter fg for hint/footer text on the standard label
        // background so dim rows don't fade out.
        s = s.fg(Color::Rgb(180, 195, 220)).add_modifier(Modifier::BOLD);
    }
    s
}

pub fn modal_close_style() -> Style {
    Style::default()
        .fg(BORDER_ACTIVE)
        .add_modifier(Modifier::BOLD)
}

/// Styled for a pane that has cleanly finished its work (dim gray — non-urgent).
pub fn idle_work_complete() -> Style {
    Style::default()
        .fg(Color::Rgb(236, 241, 247))
        .bg(Color::Rgb(74, 88, 108))
        .add_modifier(Modifier::BOLD)
}

/// Styled for a pane that is stale / cause unknown (dimmer gray).
pub fn idle_stale() -> Style {
    Style::default()
        .fg(Color::Rgb(228, 234, 242))
        .bg(Color::Rgb(58, 68, 84))
        .add_modifier(Modifier::BOLD)
}

/// Styled for a pane awaiting user input (yellow — needs attention).
pub fn idle_input_wait() -> Style {
    Style::default()
        .fg(Color::Rgb(28, 24, 16))
        .bg(Color::Rgb(236, 198, 98))
        .add_modifier(Modifier::BOLD)
}

/// Styled for a pane awaiting operator permission (light yellow — elevated urgency).
pub fn idle_permission_wait() -> Style {
    Style::default()
        .fg(Color::Rgb(28, 20, 12))
        .bg(Color::Rgb(245, 176, 82))
        .add_modifier(Modifier::BOLD)
}

/// Styled for a pane that has hit a resource limit (red — action required).
pub fn idle_limit_hit() -> Style {
    Style::default()
        .fg(Color::Rgb(248, 244, 244))
        .bg(Color::Rgb(188, 72, 72))
        .add_modifier(Modifier::BOLD)
}

/// High-contrast badge for the elapsed timer shown next to an idle state.
pub fn idle_elapsed_badge() -> Style {
    Style::default()
        .fg(Color::Rgb(232, 238, 248))
        .bg(Color::Rgb(54, 66, 86))
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_close_style_uses_active_border_not_severity() {
        let style = modal_close_style();
        assert_eq!(style.fg, Some(BORDER_ACTIVE));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

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
