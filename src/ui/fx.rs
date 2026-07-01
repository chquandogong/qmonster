//! v1.53.0: render the three decorative effects.
//!
//! Each `render_fx_*` function takes the corresponding state and a
//! ratatui `Frame`, draws to the full viewport, and assumes the caller
//! has already cleared / sequenced the overlay above other UI.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

use crate::app::fx_state::{
    BannerState, ConfettiState, FireworksState, FxScene, MatrixState, PlasmaState, SamplerState,
    SnowState, hue_to_rgb, plasma_glyph,
};

/// Top-level dispatcher: pick the renderer matching the active scene.
///
/// v1.54.0: the overlay is **non-destructive** — the underlying
/// dashboard / modals stay visible and the effect only paints its
/// own cells (block-letter glyphs / particles / stream characters).
/// Earlier behaviour cleared the entire viewport before drawing.
pub fn render_fx_overlay(frame: &mut Frame<'_>, scene: &FxScene, banner_text: &str) {
    let area = frame.area();
    render_fx_scene(frame, scene, banner_text);
    // Bottom-right dismiss hint — written cell-by-cell with non-space
    // characters only so it doesn't clobber the underlying dashboard
    // footer's bg / styling.
    paint_hint_inline(frame, area, " click / any key to dismiss · Q opens fx ");
}

/// Render the active scene without the dismiss hint. The Sampler
/// variant calls back into this with its inner scene so we don't paint
/// nested hints.
fn render_fx_scene(frame: &mut Frame<'_>, scene: &FxScene, banner_text: &str) {
    match scene {
        FxScene::Banner(b) => render_fx_banner(frame, b, banner_text),
        FxScene::Confetti(c) => render_fx_confetti(frame, c),
        FxScene::Matrix(m) => render_fx_matrix(frame, m),
        FxScene::Snow(s) => render_fx_snow(frame, s),
        FxScene::Fireworks(f) => render_fx_fireworks(frame, f),
        FxScene::Plasma(p) => render_fx_plasma(frame, p),
        FxScene::Sampler(s) => render_fx_sampler(frame, s, banner_text),
    }
}

fn paint_hint_inline(frame: &mut Frame<'_>, area: Rect, hint: &str) {
    let hint_w = hint.chars().count() as u16;
    if area.width < hint_w + 2 || area.height == 0 {
        return;
    }
    let hx = area.width - hint_w - 1;
    let hy = area.height.saturating_sub(1);
    let buf = frame.buffer_mut();
    let style = Style::default()
        .fg(Color::Rgb(180, 190, 210))
        .add_modifier(Modifier::DIM);
    for (i, ch) in hint.chars().enumerate() {
        let x = area.x + hx + i as u16;
        let y = area.y + hy;
        if x >= area.x + area.width || y >= area.y + area.height {
            break;
        }
        if ch == ' ' {
            // Skip spaces so the underlying footer cells stay intact.
            continue;
        }
        let cell = &mut buf[(x, y)];
        cell.set_char(ch);
        cell.set_style(style);
    }
}

// ---------------- Banner ----------------

pub fn render_fx_banner(frame: &mut Frame<'_>, state: &BannerState, text: &str) {
    let area = frame.area();
    let lines = render_block_text(text);
    let buf = frame.buffer_mut();
    let base_x = state.x.round() as i32;
    let base_y = state.y.round() as i32;
    for (row, line) in lines.iter().enumerate() {
        let y = base_y + row as i32;
        if y < area.y as i32 || y >= (area.y + area.height) as i32 {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        for (col, ch) in chars.iter().enumerate() {
            let x = base_x + col as i32;
            if x < area.x as i32 || x >= (area.x + area.width) as i32 {
                continue;
            }
            if *ch == ' ' {
                continue;
            }
            let hue = (state.hue_deg + (col as u16) * 6) % 360;
            let (r, g, b) = hue_to_rgb(hue);
            let cell = &mut buf[(x as u16, y as u16)];
            cell.set_char(*ch);
            cell.set_style(
                Style::default()
                    .fg(Color::Rgb(r, g, b))
                    .add_modifier(Modifier::BOLD),
            );
        }
    }
}

/// Hand-rolled 5-row block font. Each glyph is 5 lines wide-enough to
/// be readable; uppercased before lookup. Unsupported glyphs render as
/// a 4×5 solid box so the banner never blanks out.
fn render_block_text(text: &str) -> Vec<String> {
    let mut rows: [String; 5] = Default::default();
    let upper = text.to_uppercase();
    let mut chars = upper.chars().peekable();
    while let Some(ch) = chars.next() {
        let glyph = block_glyph(ch);
        for (i, line) in glyph.iter().enumerate() {
            rows[i].push_str(line);
        }
        if chars.peek().is_some() {
            for r in &mut rows {
                r.push(' ');
            }
        }
    }
    rows.into_iter().collect()
}

fn block_glyph(ch: char) -> [&'static str; 5] {
    match ch {
        'A' => ["▄▀▀▄", "█  █", "████", "█  █", "█  █"],
        'B' => ["▀▀▄ ", "█▄▄▀", "█  █", "█  █", "▀▀▀ "],
        'C' => [" ▄▄▄", "█   ", "█   ", "█   ", " ▀▀▀"],
        'D' => ["▀▀▄ ", "█  █", "█  █", "█  █", "▀▀▀ "],
        'E' => ["████", "█   ", "███ ", "█   ", "████"],
        'F' => ["████", "█   ", "███ ", "█   ", "█   "],
        'G' => [" ▄▄▄", "█   ", "█ ██", "█  █", " ▀▀▀"],
        'H' => ["█  █", "█  █", "████", "█  █", "█  █"],
        'I' => ["▀▀▀▀", " ██ ", " ██ ", " ██ ", "▀▀▀▀"],
        'J' => ["████", "  █ ", "  █ ", "█ █ ", "▀█▀ "],
        'K' => ["█  █", "█ █ ", "██  ", "█ █ ", "█  █"],
        'L' => ["█   ", "█   ", "█   ", "█   ", "████"],
        'M' => ["█▄ ▄█", "█ █ █", "█   █", "█   █", "█   █"],
        'N' => ["█▄  █", "█▀▄ █", "█ █ █", "█  ▀█", "█   █"],
        'O' => [" ██ ", "█  █", "█  █", "█  █", " ██ "],
        'P' => ["███ ", "█  █", "███ ", "█   ", "█   "],
        'Q' => [" ██ ", "█  █", "█  █", "█ ██", " ███"],
        'R' => ["███ ", "█  █", "███ ", "█ █ ", "█  █"],
        'S' => [" ▄▄▄", "█   ", " ▀▀▄", "   █", "▀▀▀ "],
        'T' => ["████", " ██ ", " ██ ", " ██ ", " ██ "],
        'U' => ["█  █", "█  █", "█  █", "█  █", " ██ "],
        'V' => ["█  █", "█  █", "█  █", " ██ ", "  ▀ "],
        'W' => ["█   █", "█   █", "█   █", "█ █ █", " █▀█ "],
        'X' => ["█  █", " ██ ", " ██ ", " ██ ", "█  █"],
        'Y' => ["█  █", "█  █", " ██ ", " ██ ", " ██ "],
        'Z' => ["████", "  █ ", " █  ", "█   ", "████"],
        '0' => [" ██ ", "█  █", "█  █", "█  █", " ██ "],
        '1' => [" ██ ", "███ ", " ██ ", " ██ ", "████"],
        '2' => [" ██ ", "█  █", "  █ ", " █  ", "████"],
        '3' => ["███ ", "   █", " ██ ", "   █", "███ "],
        '4' => ["█  █", "█  █", "████", "   █", "   █"],
        '5' => ["████", "█   ", "███ ", "   █", "███ "],
        '6' => [" ██ ", "█   ", "███ ", "█  █", " ██ "],
        '7' => ["████", "   █", "  █ ", " █  ", "█   "],
        '8' => [" ██ ", "█  █", " ██ ", "█  █", " ██ "],
        '9' => [" ██ ", "█  █", " ███", "   █", " ██ "],
        ' ' => ["    ", "    ", "    ", "    ", "    "],
        '-' => ["    ", "    ", "████", "    ", "    "],
        '!' => [" █ ", " █ ", " █ ", "   ", " █ "],
        '?' => ["███ ", "   █", "  █ ", "    ", "  █ "],
        '.' => ["    ", "    ", "    ", "    ", " █  "],
        '*' => [" █ ", "▀█▀", "███", "▀█▀", " █ "],
        // v1.55.0: tilde wave glyph for the new default text "~O~ Qmonster".
        '~' => ["    ", "    ", "▄▀▄ ", "  ▀▄", "    "],
        _ => ["████", "█  █", "████", "█  █", "████"],
    }
}

/// Compute the rendered banner dimensions for a given text.
pub fn block_text_dimensions(text: &str) -> (u16, u16) {
    let lines = render_block_text(text);
    let h = lines.len() as u16;
    let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
    (w, h)
}

// ---------------- Confetti ----------------

pub fn render_fx_confetti(frame: &mut Frame<'_>, state: &ConfettiState) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    for p in &state.particles {
        let x = p.x.round() as i32;
        let y = p.y.round() as i32;
        if x < area.x as i32 || x >= (area.x + area.width) as i32 {
            continue;
        }
        if y < area.y as i32 || y >= (area.y + area.height) as i32 {
            continue;
        }
        let (r, g, b) = hue_to_rgb(p.hue_deg);
        let fade = p.fade();
        let r = (r as f32 * (1.0 - fade) + 30.0 * fade) as u8;
        let g = (g as f32 * (1.0 - fade) + 30.0 * fade) as u8;
        let b = (b as f32 * (1.0 - fade) + 30.0 * fade) as u8;
        let cell = &mut buf[(x as u16, y as u16)];
        cell.set_char(p.glyph);
        cell.set_style(
            Style::default()
                .fg(Color::Rgb(r, g, b))
                .add_modifier(Modifier::BOLD),
        );
    }
}

// ---------------- Matrix ----------------

pub fn render_fx_matrix(frame: &mut Frame<'_>, state: &MatrixState) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    // v1.56.0: pre-dim the underlying viewport so matrix glyphs visibly
    // pop against the dashboard rather than blending into pane text.
    // Modifier::DIM uses the terminal's standard "dim" SGR attribute,
    // which most modern terminals render at ~50% brightness — bg/fg
    // semantic intact, just visually muted. Streams paint BOLD bright
    // green/white over this dim layer, restoring full contrast on
    // exactly the cells they touch.
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            let dimmed = cell.style().add_modifier(Modifier::DIM);
            cell.set_style(dimmed);
        }
    }
    for stream in &state.streams {
        if stream.column >= area.width {
            continue;
        }
        for (i, glyph) in stream.trail.iter().enumerate() {
            let row_y = stream.head_y - i as f32;
            if row_y < 0.0 || row_y >= area.height as f32 {
                continue;
            }
            let y = row_y.floor() as u16;
            let x = area.x + stream.column;
            if y >= area.y + area.height {
                continue;
            }
            let color = if i == 0 {
                Color::Rgb(220, 255, 220)
            } else {
                let f = 1.0 - (i as f32 / stream.length as f32);
                let intensity = (60.0 + 180.0 * f.max(0.0)) as u8;
                Color::Rgb(0, intensity, intensity / 3)
            };
            let cell = &mut buf[(x, area.y + y)];
            cell.set_char(*glyph);
            let mut style = Style::default().fg(color);
            if i == 0 {
                style = style.add_modifier(Modifier::BOLD);
            }
            cell.set_style(style);
        }
    }
}

// ---------------- Snow (v1.59.0) ----------------

pub fn render_fx_snow(frame: &mut Frame<'_>, state: &SnowState) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    for f in &state.flakes {
        let x = f.x.round() as i32;
        let y = f.y.round() as i32;
        if x < area.x as i32 || x >= (area.x + area.width) as i32 {
            continue;
        }
        if y < area.y as i32 || y >= (area.y + area.height) as i32 {
            continue;
        }
        // Larger flake glyphs render in white; small dots in pale blue
        // so the layer reads as snow against any underlying dashboard.
        let style = match f.glyph {
            '❄' | '❅' | '❆' => Style::default()
                .fg(Color::Rgb(245, 250, 255))
                .add_modifier(Modifier::BOLD),
            '*' => Style::default().fg(Color::Rgb(220, 230, 245)),
            _ => Style::default()
                .fg(Color::Rgb(180, 200, 230))
                .add_modifier(Modifier::DIM),
        };
        let cell = &mut buf[(x as u16, y as u16)];
        cell.set_char(f.glyph);
        cell.set_style(style);
    }
}

// ---------------- Fireworks (v1.59.0) ----------------

pub fn render_fx_fireworks(frame: &mut Frame<'_>, state: &FireworksState) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    // Rockets: a single bright trail glyph.
    for r in &state.rockets {
        let x = r.x.round() as i32;
        let y = r.y.round() as i32;
        if x < area.x as i32 || x >= (area.x + area.width) as i32 {
            continue;
        }
        if y < area.y as i32 || y >= (area.y + area.height) as i32 {
            continue;
        }
        let (rr, gg, bb) = hue_to_rgb(r.hue_deg);
        let cell = &mut buf[(x as u16, y as u16)];
        cell.set_char('|');
        cell.set_style(
            Style::default()
                .fg(Color::Rgb(rr, gg, bb))
                .add_modifier(Modifier::BOLD),
        );
    }
    // Sparks: fade their color toward black as life drains.
    for s in &state.sparks {
        let x = s.x.round() as i32;
        let y = s.y.round() as i32;
        if x < area.x as i32 || x >= (area.x + area.width) as i32 {
            continue;
        }
        if y < area.y as i32 || y >= (area.y + area.height) as i32 {
            continue;
        }
        let (rr, gg, bb) = hue_to_rgb(s.hue_deg);
        let fade = s.fade();
        let rr = (rr as f32 * (1.0 - fade) + 20.0 * fade) as u8;
        let gg = (gg as f32 * (1.0 - fade) + 20.0 * fade) as u8;
        let bb = (bb as f32 * (1.0 - fade) + 20.0 * fade) as u8;
        let cell = &mut buf[(x as u16, y as u16)];
        cell.set_char(s.glyph);
        cell.set_style(
            Style::default()
                .fg(Color::Rgb(rr, gg, bb))
                .add_modifier(Modifier::BOLD),
        );
    }
}

// ---------------- Plasma (v1.59.0) ----------------

pub fn render_fx_plasma(frame: &mut Frame<'_>, state: &PlasmaState) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    // Walk every cell the overlay covers. Plasma is the one effect that
    // intentionally overwrites every cell — a sin/cos field needs a
    // dense lattice to read as a continuous wash.
    for row in 0..area.height {
        for col in 0..area.width {
            let (gi, hue) = state.sample(col, row);
            let glyph = plasma_glyph(gi);
            // The lowest density glyph is a literal space — leave the
            // underlying cell untouched so the dashboard stays visible
            // through the gaps and the wash reads as ambient.
            if glyph == ' ' {
                continue;
            }
            let (r, g, b) = hue_to_rgb(hue);
            let cell = &mut buf[(area.x + col, area.y + row)];
            cell.set_char(glyph);
            cell.set_style(Style::default().fg(Color::Rgb(r, g, b)));
        }
    }
}

// ---------------- Sampler (v1.59.0) ----------------

pub fn render_fx_sampler(frame: &mut Frame<'_>, state: &SamplerState, banner_text: &str) {
    render_fx_scene(frame, &state.inner, banner_text);
    // Top-left chip showing which sub-effect is currently playing so
    // the operator knows what they're seeing without flipping settings.
    let chip = format!(" sampler · {} ", state.current.as_str());
    let area = frame.area();
    if area.width < (chip.chars().count() as u16 + 2) || area.height == 0 {
        return;
    }
    let buf = frame.buffer_mut();
    let style = Style::default()
        .fg(Color::Rgb(200, 210, 230))
        .add_modifier(Modifier::DIM);
    for (i, ch) in chip.chars().enumerate() {
        let x = area.x + 1 + i as u16;
        let y = area.y;
        if x >= area.x + area.width {
            break;
        }
        if ch == ' ' {
            continue;
        }
        let cell = &mut buf[(x, y)];
        cell.set_char(ch);
        cell.set_style(style);
    }
}

pub struct FxBackdrop;

impl Widget for FxBackdrop {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                let cell = &mut buf[(x, y)];
                cell.set_char(' ');
                cell.set_style(Style::default().bg(Color::Rgb(8, 10, 14)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::app::config::FxEffect;
    use crate::app::fx_state::FxScene;

    fn term(w: u16, h: u16) -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(w, h)).unwrap()
    }

    #[test]
    fn block_text_dimensions_match_rendered_lines() {
        let (w, h) = block_text_dimensions("AB");
        assert_eq!(h, 5);
        assert_eq!(w, 9, "'A' (4) + space (1) + 'B' (4) = 9");
    }

    #[test]
    fn render_block_text_handles_unknown_chars_with_solid_block() {
        let lines = render_block_text("@");
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().all(|l| l.chars().count() == 4));
    }

    #[test]
    fn fx_banner_smoke_renders_without_panic() {
        let mut t = term(80, 24);
        let scene = FxScene::from_effect(FxEffect::Banner, 80, 24, 1);
        t.draw(|f| render_fx_overlay(f, &scene, "QMONSTER"))
            .unwrap();
        let buf = t.backend().buffer();
        let dump: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(
            dump.contains("dismiss"),
            "hint must render in the banner overlay"
        );
    }

    /// v1.54.0 regression guard: cells the fx overlay does NOT touch
    /// (the inter-particle / inter-glyph gaps and the row above the
    /// dismiss hint) must keep whatever the underlying TUI drew there.
    /// Mirrors `render_dashboard_frame`'s sequencing: paint a fake
    /// dashboard, then render the fx overlay LAST in the SAME draw
    /// call so the overlay sees a fully painted buffer underneath.
    #[test]
    fn fx_overlay_preserves_underlying_cells_outside_effect_glyphs() {
        let mut t = term(80, 24);
        let scene = FxScene::from_effect(FxEffect::Confetti, 80, 24, 99);
        t.draw(|f| {
            // 1) Paint sentinel cells across the buffer to simulate a
            //    fully drawn dashboard.
            {
                let buf = f.buffer_mut();
                for y in 0..24 {
                    for x in 0..80 {
                        buf[(x, y)].set_char('*');
                    }
                }
            }
            // 2) Render the overlay on top (post-dashboard slot).
            render_fx_overlay(f, &scene, "QMONSTER");
        })
        .unwrap();
        let buf = t.backend().buffer();
        let surviving = buf.content.iter().filter(|c| c.symbol() == "*").count();
        // 80×24 = 1920 cells. Confetti lands ~80 particles + a short
        // hint (~30 chars); the vast majority of sentinel cells must
        // survive. Earlier Clear-based render dropped this to ~0.
        assert!(
            surviving >= 1500,
            "non-destructive overlay must preserve underlying cells; surviving = {surviving}"
        );
    }

    #[test]
    fn fx_confetti_smoke_renders_without_panic() {
        let mut t = term(80, 24);
        let scene = FxScene::from_effect(FxEffect::Confetti, 80, 24, 7);
        t.draw(|f| render_fx_overlay(f, &scene, "X")).unwrap();
    }

    #[test]
    fn fx_matrix_smoke_renders_without_panic() {
        let mut t = term(80, 24);
        let scene = FxScene::from_effect(FxEffect::Matrix, 80, 24, 13);
        t.draw(|f| render_fx_overlay(f, &scene, "X")).unwrap();
    }

    // v1.59.0 -----------------------------------------------------------

    #[test]
    fn fx_snow_smoke_renders_without_panic() {
        let mut t = term(80, 24);
        let scene = FxScene::from_effect(FxEffect::Snow, 80, 24, 21);
        t.draw(|f| render_fx_overlay(f, &scene, "X")).unwrap();
    }

    #[test]
    fn fx_fireworks_smoke_renders_without_panic() {
        let mut t = term(80, 24);
        let scene = FxScene::from_effect(FxEffect::Fireworks, 80, 24, 31);
        t.draw(|f| render_fx_overlay(f, &scene, "X")).unwrap();
    }

    #[test]
    fn fx_plasma_smoke_renders_without_panic() {
        let mut t = term(80, 24);
        let scene = FxScene::from_effect(FxEffect::Plasma, 80, 24, 41);
        t.draw(|f| render_fx_overlay(f, &scene, "X")).unwrap();
    }

    #[test]
    fn fx_sampler_smoke_renders_without_panic_and_paints_sub_effect_chip() {
        let mut t = term(80, 24);
        let scene = FxScene::from_effect(FxEffect::Sampler, 80, 24, 51);
        t.draw(|f| render_fx_overlay(f, &scene, "QMONSTER"))
            .unwrap();
        let buf = t.backend().buffer();
        let dump: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(
            dump.contains("sampler"),
            "sampler must paint a chip identifying the active sub-effect"
        );
    }
}
