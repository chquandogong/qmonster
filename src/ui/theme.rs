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
///
/// v1.60.0: `Light` flips the foreground / background contrast
/// philosophy. Operators on bright-profile terminals (Light+, Solarized
/// Light, etc.) get a dark-text-on-light palette: foreground colors
/// shift toward black, background chips toward neutral grays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    HighContrast,
    Light,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::HighContrast => "high_contrast",
            Self::Light => "light",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Dark => Self::HighContrast,
            Self::HighContrast => Self::Light,
            Self::Light => Self::Dark,
        }
    }
}

static THEME_MODE_RAW: AtomicU8 = AtomicU8::new(0);

pub fn set_theme_mode(mode: ThemeMode) {
    let raw: u8 = match mode {
        ThemeMode::Dark => 0,
        ThemeMode::HighContrast => 1,
        ThemeMode::Light => 2,
    };
    THEME_MODE_RAW.store(raw, Ordering::Relaxed);
}

pub fn theme_mode() -> ThemeMode {
    match THEME_MODE_RAW.load(Ordering::Relaxed) {
        1 => ThemeMode::HighContrast,
        2 => ThemeMode::Light,
        _ => ThemeMode::Dark,
    }
}

/// Low-saturation palette. Color is only used for severity transitions
/// and is always accompanied by the severity letter (`ui::alerts`
/// renders `[W]` / `[R]` / …).
///
/// v1.60.0: these constants remain the Dark-mode values for backward
/// compat with test assertions; runtime call sites should prefer the
/// theme-aware accessor functions below (`text_dim()`, `text_primary()`,
/// `badge_bg()`, `border_idle()`, `border_active()`) which branch on
/// the active mode and return the right shade for Dark / HighContrast
/// / Light.
pub const BORDER_IDLE: Color = Color::Rgb(90, 100, 120);
pub const BORDER_ACTIVE: Color = Color::Rgb(130, 160, 200);
pub const TEXT_PRIMARY: Color = Color::Rgb(210, 215, 225);
pub const TEXT_DIM: Color = Color::Rgb(120, 130, 145);
pub const BADGE_BG: Color = Color::Rgb(60, 70, 90);

// v1.60.0 ----------- theme-aware accessors -----------

pub fn border_idle() -> Color {
    match theme_mode() {
        ThemeMode::Dark | ThemeMode::HighContrast => BORDER_IDLE,
        // Light: same hue family but darkened so it shows on a white
        // background instead of disappearing into it.
        ThemeMode::Light => Color::Rgb(150, 160, 175),
    }
}

pub fn border_active() -> Color {
    match theme_mode() {
        ThemeMode::Dark | ThemeMode::HighContrast => BORDER_ACTIVE,
        ThemeMode::Light => Color::Rgb(60, 95, 145),
    }
}

pub fn text_primary() -> Color {
    match theme_mode() {
        ThemeMode::Dark | ThemeMode::HighContrast => TEXT_PRIMARY,
        ThemeMode::Light => Color::Rgb(30, 35, 45),
    }
}

pub fn text_dim() -> Color {
    match theme_mode() {
        ThemeMode::Dark | ThemeMode::HighContrast => TEXT_DIM,
        ThemeMode::Light => Color::Rgb(100, 110, 125),
    }
}

pub fn badge_bg() -> Color {
    match theme_mode() {
        ThemeMode::Dark | ThemeMode::HighContrast => BADGE_BG,
        ThemeMode::Light => Color::Rgb(220, 226, 236),
    }
}

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
        // Light: darken severity hues so they remain distinguishable
        // against a white background. Distinctness invariant preserved.
        (ThemeMode::Light, Severity::Safe) => Color::Rgb(70, 90, 120),
        (ThemeMode::Light, Severity::Good) => Color::Rgb(40, 130, 90),
        (ThemeMode::Light, Severity::Concern) => Color::Rgb(170, 130, 30),
        (ThemeMode::Light, Severity::Warning) => Color::Rgb(195, 110, 30),
        (ThemeMode::Light, Severity::Risk) => Color::Rgb(190, 50, 50),
    }
}

pub fn severity_badge_style(sev: Severity) -> Style {
    let fg = match theme_mode() {
        ThemeMode::Dark => Color::Rgb(22, 24, 30),
        // HighContrast keeps a near-black fg but adds BOLD for legibility.
        ThemeMode::HighContrast => Color::Rgb(18, 18, 22),
        // Light: a near-white fg sits on the darkened severity bg with
        // BOLD for contrast.
        ThemeMode::Light => Color::Rgb(245, 248, 252),
    };
    let mut s = Style::default().fg(fg).bg(severity_color(sev));
    if matches!(theme_mode(), ThemeMode::HighContrast | ThemeMode::Light) {
        s = s.add_modifier(Modifier::BOLD);
    }
    s
}

pub fn label_style() -> Style {
    let mut s = Style::default().fg(text_dim()).bg(badge_bg());
    match theme_mode() {
        ThemeMode::HighContrast => {
            // Brighter fg for hint/footer text on the standard label
            // background so dim rows don't fade out.
            s = s.fg(Color::Rgb(180, 195, 220)).add_modifier(Modifier::BOLD);
        }
        ThemeMode::Light => {
            // Light bg already gives contrast; darken the fg so hints
            // remain readable instead of fading into the chip.
            s = s.fg(Color::Rgb(50, 60, 80));
        }
        ThemeMode::Dark => {}
    }
    s
}

pub fn modal_close_style() -> Style {
    Style::default()
        .fg(border_active())
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

    // v1.60.0 -----------------------------------------------------------

    #[test]
    fn theme_mode_cycle_covers_dark_high_contrast_light_in_order() {
        assert_eq!(ThemeMode::Dark.cycle(), ThemeMode::HighContrast);
        assert_eq!(ThemeMode::HighContrast.cycle(), ThemeMode::Light);
        assert_eq!(ThemeMode::Light.cycle(), ThemeMode::Dark);
    }

    #[test]
    fn theme_mode_round_trips_through_atomic_for_every_variant() {
        for mode in [ThemeMode::Dark, ThemeMode::HighContrast, ThemeMode::Light] {
            set_theme_mode(mode);
            assert_eq!(theme_mode(), mode);
        }
        // Reset to the test-suite default.
        set_theme_mode(ThemeMode::Dark);
    }

    #[test]
    fn light_theme_severity_colors_stay_distinct() {
        set_theme_mode(ThemeMode::Light);
        let c = [
            severity_color(Severity::Safe),
            severity_color(Severity::Good),
            severity_color(Severity::Concern),
            severity_color(Severity::Warning),
            severity_color(Severity::Risk),
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(
                    c[i], c[j],
                    "Light severity colors must remain pairwise distinct"
                );
            }
        }
        set_theme_mode(ThemeMode::Dark);
    }

    #[test]
    fn light_theme_text_dim_differs_from_dark() {
        set_theme_mode(ThemeMode::Dark);
        let dark = text_dim();
        set_theme_mode(ThemeMode::Light);
        let light = text_dim();
        assert_ne!(
            dark, light,
            "Light mode must shift text_dim away from the dark default"
        );
        set_theme_mode(ThemeMode::Dark);
    }

    #[test]
    fn dark_and_high_contrast_text_dim_match_legacy_constant() {
        set_theme_mode(ThemeMode::Dark);
        assert_eq!(text_dim(), TEXT_DIM);
        set_theme_mode(ThemeMode::HighContrast);
        assert_eq!(text_dim(), TEXT_DIM);
        set_theme_mode(ThemeMode::Dark);
    }
}
