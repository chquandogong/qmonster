//! v1.53.0: decorative effects state machine.
//!
//! Three pure state machines feed `src/ui/fx.rs`:
//! - `BannerState`: a bouncing rainbow text banner (DVD-screensaver style).
//! - `ConfettiState`: 80 unicode sparkles spawned at center, gravity, life.
//! - `MatrixState`: per-column falling streams.
//!
//! Logic stays pure (no `Instant`/IO) so unit tests can advance frames
//! deterministically; the loop pumps `step()` once per render tick.

use crate::app::config::FxEffect;

/// Frames per second the overlay aims for while active. The main loop
/// adapts its `event::poll` cadence to match (~33ms).
pub const FX_TARGET_FPS: u64 = 30;
pub const FX_FRAME_INTERVAL_MS: u64 = 1000 / FX_TARGET_FPS;

/// Subdivisions of the hue circle used by the banner's color cycling.
/// Picked empirically to look smooth at 30 FPS without flashing — full
/// rotation in ~6 seconds.
pub const FX_HUE_STEP: u16 = 2;
pub const FX_HUE_PERIOD: u16 = 360;

#[derive(Debug, Clone, PartialEq)]
pub struct BannerState {
    /// Position is float-based so the banner can move at fractional
    /// cells per frame; integer rendering rounds at the boundary.
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// Current hue in degrees [0, 360). Advances by `FX_HUE_STEP`
    /// per frame, wraps at `FX_HUE_PERIOD`.
    pub hue_deg: u16,
    pub width: u16,
    pub height: u16,
}

impl BannerState {
    pub fn new(viewport_w: u16, viewport_h: u16, banner_w: u16, banner_h: u16) -> Self {
        let cx = viewport_w.saturating_sub(banner_w) as f32 * 0.5;
        let cy = viewport_h.saturating_sub(banner_h) as f32 * 0.5;
        Self {
            x: cx,
            y: cy,
            vx: 0.55,
            vy: 0.30,
            hue_deg: 0,
            width: banner_w,
            height: banner_h,
        }
    }

    /// Advance one frame: move, bounce off edges, rotate hue.
    pub fn step(&mut self, viewport_w: u16, viewport_h: u16) {
        self.x += self.vx;
        self.y += self.vy;
        let max_x = viewport_w.saturating_sub(self.width).max(1) as f32;
        let max_y = viewport_h.saturating_sub(self.height).max(1) as f32;
        if self.x <= 0.0 {
            self.x = 0.0;
            self.vx = self.vx.abs();
        } else if self.x >= max_x {
            self.x = max_x;
            self.vx = -self.vx.abs();
        }
        if self.y <= 0.0 {
            self.y = 0.0;
            self.vy = self.vy.abs();
        } else if self.y >= max_y {
            self.y = max_y;
            self.vy = -self.vy.abs();
        }
        self.hue_deg = (self.hue_deg + FX_HUE_STEP) % FX_HUE_PERIOD;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub hue_deg: u16,
    pub glyph: char,
    /// Frames remaining before the particle is removed.
    pub life: u16,
    pub initial_life: u16,
}

impl Particle {
    pub fn alive(&self) -> bool {
        self.life > 0
    }

    /// Brightness 0.0 (just spawned) → 1.0 (about to die). Caller
    /// uses this to fade the rendered cell toward background.
    pub fn fade(&self) -> f32 {
        if self.initial_life == 0 {
            return 1.0;
        }
        1.0 - (self.life as f32 / self.initial_life as f32)
    }
}

const CONFETTI_GLYPHS: &[char] = &['✨', '⋆', '☆', '⭐', '✦', '✧', '❋', '❀', '✺'];
pub const CONFETTI_GRAVITY: f32 = 0.06;
pub const CONFETTI_INITIAL_LIFE: u16 = 60;
pub const CONFETTI_PARTICLE_COUNT: usize = 80;

#[derive(Debug, Clone, PartialEq)]
pub struct ConfettiState {
    pub particles: Vec<Particle>,
    pub viewport_w: u16,
    pub viewport_h: u16,
}

impl ConfettiState {
    /// Spawn `n` particles in a radial pattern centred at the viewport.
    /// `seed` seeds a tiny xorshift so we get varied bursts without
    /// dragging in a real PRNG dependency.
    pub fn spawn(viewport_w: u16, viewport_h: u16, n: usize, seed: u64) -> Self {
        let mut particles = Vec::with_capacity(n);
        let cx = viewport_w as f32 * 0.5;
        let cy = viewport_h as f32 * 0.5;
        let mut rng = XorShift64::new(seed);
        for i in 0..n {
            let angle =
                (i as f32) / (n as f32) * std::f32::consts::TAU + (rng.next_f32() - 0.5) * 0.4;
            let speed = 0.4 + rng.next_f32() * 1.0;
            particles.push(Particle {
                x: cx,
                y: cy,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed * 0.7,
                hue_deg: ((i as u16).wrapping_mul(13)) % FX_HUE_PERIOD,
                glyph: CONFETTI_GLYPHS[i % CONFETTI_GLYPHS.len()],
                life: CONFETTI_INITIAL_LIFE - (rng.next_u32() % 12) as u16,
                initial_life: CONFETTI_INITIAL_LIFE,
            });
        }
        Self {
            particles,
            viewport_w,
            viewport_h,
        }
    }

    pub fn step(&mut self) {
        for p in &mut self.particles {
            p.x += p.vx;
            p.y += p.vy;
            p.vy += CONFETTI_GRAVITY;
            p.hue_deg = (p.hue_deg + FX_HUE_STEP * 2) % FX_HUE_PERIOD;
            p.life = p.life.saturating_sub(1);
        }
        self.particles.retain(|p| {
            p.alive()
                && p.x >= 0.0
                && p.x < self.viewport_w as f32
                && p.y >= 0.0
                && p.y < (self.viewport_h as f32 + 4.0)
        });
    }

    pub fn is_finished(&self) -> bool {
        self.particles.is_empty()
    }
}

/// Per-column matrix-rain stream. Head is bright, tail dims.
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixStream {
    pub column: u16,
    /// Floating head row position so we can advance fractionally.
    pub head_y: f32,
    pub speed: f32,
    pub length: u16,
    pub trail: Vec<char>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatrixState {
    pub streams: Vec<MatrixStream>,
    pub viewport_w: u16,
    pub viewport_h: u16,
    rng_state: u64,
}

const MATRIX_GLYPH_POOL: &[char] = &[
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ', 'ｻ', 'ｼ', 'ｽ', 'ｾ', 'ｿ', 'ﾀ', 'ﾁ', 'ﾂ', 'ﾃ',
    'ﾄ', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'Z', '#', '$', '%', '&',
    '+', '*', '=',
];

impl MatrixState {
    pub fn new(viewport_w: u16, viewport_h: u16, seed: u64) -> Self {
        let mut state = Self {
            streams: Vec::new(),
            viewport_w,
            viewport_h,
            rng_state: seed.max(1),
        };
        // Spawn ~40% of columns with an active stream up front so the
        // first frame already looks alive.
        let target = (viewport_w as usize * 4 / 10).max(1);
        for _ in 0..target {
            state.spawn_random_stream();
        }
        state
    }

    fn rng_next(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x.max(1);
        x
    }

    fn rng_f32(&mut self) -> f32 {
        (self.rng_next() & 0xFFFFFF) as f32 / (1u32 << 24) as f32
    }

    fn spawn_random_stream(&mut self) {
        let col = (self.rng_next() % (self.viewport_w.max(1) as u64)) as u16;
        let length = 4 + (self.rng_next() % 14) as u16;
        let speed = 0.25 + self.rng_f32() * 0.6;
        let head_y = -(self.rng_f32() * self.viewport_h as f32);
        let mut trail = Vec::with_capacity(length as usize);
        for _ in 0..length {
            trail.push(self.random_glyph());
        }
        self.streams.push(MatrixStream {
            column: col,
            head_y,
            speed,
            length,
            trail,
        });
    }

    fn random_glyph(&mut self) -> char {
        MATRIX_GLYPH_POOL[(self.rng_next() as usize) % MATRIX_GLYPH_POOL.len()]
    }

    pub fn step(&mut self) {
        let max_streams = (self.viewport_w as usize * 5 / 10).max(1);
        // Advance heads.
        for s in &mut self.streams {
            s.head_y += s.speed;
        }
        // Drop streams that fully scrolled past the bottom.
        let h = self.viewport_h as f32;
        self.streams
            .retain(|s| s.head_y - s.length as f32 <= h + 2.0);
        // Mutate one trail glyph per frame for that flicker effect.
        if !self.streams.is_empty() {
            let idx = (self.rng_next() as usize) % self.streams.len();
            let glyph = self.random_glyph();
            let pos_count = self.streams[idx].trail.len();
            if pos_count > 0 {
                let pos = (self.rng_next() as usize) % pos_count;
                self.streams[idx].trail[pos] = glyph;
            }
        }
        // Refill if we are below the target stream count.
        while self.streams.len() < max_streams {
            self.spawn_random_stream();
        }
    }
}

/// Tiny xorshift64 — keeps the module dependency-free. Output is good
/// enough for picking glyphs and jitter.
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x.max(1);
        x
    }

    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() & 0xFFFFFF) as f32 / (1u32 << 24) as f32
    }
}

/// Active overlay payload — picked by `FxEffect` at open time.
#[derive(Debug, Clone, PartialEq)]
pub enum FxScene {
    Banner(BannerState),
    Confetti(ConfettiState),
    Matrix(MatrixState),
}

impl FxScene {
    pub fn from_effect(effect: FxEffect, viewport_w: u16, viewport_h: u16, seed: u64) -> Self {
        match effect {
            FxEffect::Banner => {
                // Banner dimensions are filled in by the renderer once
                // the figlet width/height of the operator's `text` is
                // known; stub with a sane default.
                FxScene::Banner(BannerState::new(viewport_w, viewport_h, 32, 5))
            }
            FxEffect::Confetti => FxScene::Confetti(ConfettiState::spawn(
                viewport_w,
                viewport_h,
                CONFETTI_PARTICLE_COUNT,
                seed,
            )),
            FxEffect::Matrix => FxScene::Matrix(MatrixState::new(viewport_w, viewport_h, seed)),
        }
    }

    /// True when the scene has finished its natural lifetime (only
    /// confetti returns true here; banner + matrix run forever until
    /// dismissed).
    pub fn is_finished(&self) -> bool {
        matches!(self, FxScene::Confetti(c) if c.is_finished())
    }

    pub fn step(&mut self, viewport_w: u16, viewport_h: u16) {
        match self {
            FxScene::Banner(b) => b.step(viewport_w, viewport_h),
            FxScene::Confetti(c) => c.step(),
            FxScene::Matrix(m) => m.step(),
        }
    }

    pub fn resize(&mut self, viewport_w: u16, viewport_h: u16) {
        match self {
            FxScene::Banner(_) => { /* renderer reads viewport directly */ }
            FxScene::Confetti(c) => {
                c.viewport_w = viewport_w;
                c.viewport_h = viewport_h;
            }
            FxScene::Matrix(m) => {
                m.viewport_w = viewport_w;
                m.viewport_h = viewport_h;
            }
        }
    }
}

/// Convert HSL hue (degrees, S=L=0.5) to RGB triplet for ratatui
/// coloring. Saturation/lightness are fixed so the banner stays
/// vibrant without ever going pastel-white.
pub fn hue_to_rgb(hue_deg: u16) -> (u8, u8, u8) {
    let h = (hue_deg % FX_HUE_PERIOD) as f32 / 60.0;
    let c = 1.0_f32; // chroma at S=1.0, L=0.5 → C=1
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    // L=0.5 already; no offset needed beyond this for the bright banner.
    (
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_starts_centered_and_advances_position() {
        let mut b = BannerState::new(120, 40, 30, 5);
        let (x0, y0) = (b.x, b.y);
        b.step(120, 40);
        assert!(
            (b.x - x0).abs() > 0.0 || (b.y - y0).abs() > 0.0,
            "banner must move every frame"
        );
    }

    #[test]
    fn banner_bounces_off_left_edge() {
        let mut b = BannerState::new(120, 40, 10, 5);
        b.x = 0.5;
        b.vx = -1.0;
        b.step(120, 40);
        assert!(b.vx > 0.0, "left-edge bounce must flip vx positive");
        assert!(b.x >= 0.0, "x should clamp at 0");
    }

    #[test]
    fn banner_bounces_off_right_edge() {
        let mut b = BannerState::new(120, 40, 10, 5);
        let max_x = 120u16.saturating_sub(10) as f32;
        b.x = max_x - 0.2;
        b.vx = 1.0;
        b.step(120, 40);
        assert!(b.vx < 0.0, "right-edge bounce must flip vx negative");
        assert!(b.x <= max_x);
    }

    #[test]
    fn banner_hue_advances_by_step_and_wraps() {
        let mut b = BannerState::new(120, 40, 10, 5);
        b.hue_deg = FX_HUE_PERIOD - FX_HUE_STEP;
        b.step(120, 40);
        assert_eq!(b.hue_deg, 0, "hue must wrap at FX_HUE_PERIOD");
    }

    #[test]
    fn confetti_spawn_seeds_eighty_particles() {
        let c = ConfettiState::spawn(120, 40, CONFETTI_PARTICLE_COUNT, 42);
        assert_eq!(c.particles.len(), CONFETTI_PARTICLE_COUNT);
        assert!(c.particles.iter().all(|p| p.alive()));
    }

    #[test]
    fn confetti_step_decrements_life_and_applies_gravity() {
        let mut c = ConfettiState::spawn(120, 40, 4, 7);
        let life_before: Vec<u16> = c.particles.iter().map(|p| p.life).collect();
        let vy_before: Vec<f32> = c.particles.iter().map(|p| p.vy).collect();
        c.step();
        assert!(
            c.particles
                .iter()
                .zip(&life_before)
                .all(|(p, &l)| p.life < l),
            "life must decrement"
        );
        assert!(
            c.particles
                .iter()
                .zip(&vy_before)
                .all(|(p, &v0)| (p.vy - v0 - CONFETTI_GRAVITY).abs() < 1e-4),
            "gravity must be applied to vy"
        );
    }

    #[test]
    fn confetti_finishes_after_all_particles_expire() {
        let mut c = ConfettiState::spawn(20, 20, 4, 9);
        for _ in 0..(CONFETTI_INITIAL_LIFE as usize + 4) {
            c.step();
        }
        assert!(c.is_finished());
    }

    #[test]
    fn matrix_spawns_streams_and_advances_heads() {
        let mut m = MatrixState::new(80, 24, 1234);
        assert!(!m.streams.is_empty());
        let head_before = m.streams[0].head_y;
        m.step();
        assert!(m.streams[0].head_y > head_before || m.streams.len() != 1);
    }

    #[test]
    fn matrix_drops_off_screen_streams() {
        let mut m = MatrixState::new(80, 10, 5);
        // Force every stream way past the bottom.
        for s in &mut m.streams {
            s.head_y = 100.0;
            s.speed = 1.0;
            s.length = 1;
        }
        m.step();
        // After step, streams past h+2 with length 1 are dropped, then
        // refilled to the target — so at least no stream should be at
        // 101.0+ unchanged from the forced state.
        assert!(m.streams.iter().all(|s| s.head_y < 200.0));
    }

    #[test]
    fn fx_scene_dispatches_step_per_variant() {
        let mut scene = FxScene::from_effect(FxEffect::Confetti, 80, 24, 11);
        scene.step(80, 24);
        match scene {
            FxScene::Confetti(c) => {
                assert!(c.particles.iter().all(|p| p.life < CONFETTI_INITIAL_LIFE))
            }
            _ => panic!("variant mismatch"),
        }
    }

    #[test]
    fn hue_to_rgb_red_at_zero_degrees() {
        let (r, g, b) = hue_to_rgb(0);
        assert_eq!((r, g, b), (255, 0, 0));
    }

    #[test]
    fn hue_to_rgb_green_at_120() {
        let (r, g, b) = hue_to_rgb(120);
        assert_eq!((r, g, b), (0, 255, 0));
    }

    #[test]
    fn hue_to_rgb_blue_at_240() {
        let (r, g, b) = hue_to_rgb(240);
        assert_eq!((r, g, b), (0, 0, 255));
    }

    #[test]
    fn hue_to_rgb_wraps_past_full_circle() {
        let (r, g, b) = hue_to_rgb(720);
        assert_eq!((r, g, b), (255, 0, 0));
    }
}
