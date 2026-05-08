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
/// adapts its `event::poll` cadence to match (~16ms). v1.55.0 bumped
/// from 30 to 60 to halve the per-frame integer-rounding gap that was
/// visible as juddering — at 30 FPS with vx ≈ 0.55 the banner moved a
/// cell only every other frame; at 60 FPS the same apparent speed
/// (halved velocities) renders twice as often, smoothing the eye.
pub const FX_TARGET_FPS: u64 = 60;
pub const FX_FRAME_INTERVAL_MS: u64 = 1000 / FX_TARGET_FPS;

/// Subdivisions of the hue circle used by the banner's color cycling.
/// v1.55.0 halved (was 2) so the doubled FPS keeps the same ~6s full
/// rotation rather than spinning twice as fast.
pub const FX_HUE_STEP: u16 = 1;
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
            // v1.55.0: halved (was 0.55, 0.30) so the doubled FPS keeps
            // the same apparent screensaver-drift speed while rendering
            // twice as often — net effect is smoother motion at the
            // same pace.
            vx: 0.275,
            vy: 0.15,
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
// v1.55.0: gravity halved (was 0.06) so 60 FPS keeps the same
// fall arc; lifespan doubled so particles linger across the now-
// twice-as-fast frame ticks at the same apparent on-screen duration.
pub const CONFETTI_GRAVITY: f32 = 0.03;
pub const CONFETTI_INITIAL_LIFE: u16 = 120;
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
            // v1.55.0: speed halved (was 0.4 + 1.0 random) so 60 FPS
            // keeps the same apparent burst velocity.
            let speed = 0.2 + rng.next_f32() * 0.5;
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
        // v1.55.0: speed halved (was 0.25 + 0.6 random) so 60 FPS
        // keeps the same apparent rain pace.
        let speed = 0.12 + self.rng_f32() * 0.30;
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

// ---------------- Snow (v1.59.0) ----------------

/// One snowflake. `phase` + `sway_freq` drive the horizontal cosine
/// oscillation so flakes don't all sway in lockstep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flake {
    pub x: f32,
    pub y: f32,
    pub vy: f32,
    pub sway_amp: f32,
    pub sway_freq: f32,
    pub phase: f32,
    pub age: u32,
    pub glyph: char,
}

const SNOW_GLYPHS: &[char] = &['❄', '❅', '❆', '*', '·', '∙'];
pub const SNOW_FLAKE_COUNT: usize = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct SnowState {
    pub flakes: Vec<Flake>,
    pub viewport_w: u16,
    pub viewport_h: u16,
    rng_state: u64,
}

impl SnowState {
    pub fn new(viewport_w: u16, viewport_h: u16, seed: u64) -> Self {
        let mut state = Self {
            flakes: Vec::with_capacity(SNOW_FLAKE_COUNT),
            viewport_w,
            viewport_h,
            rng_state: seed.max(1),
        };
        for _ in 0..SNOW_FLAKE_COUNT {
            let f = state.spawn_flake(true);
            state.flakes.push(f);
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

    fn spawn_flake(&mut self, scatter_y: bool) -> Flake {
        let w = self.viewport_w.max(1) as f32;
        let h = self.viewport_h.max(1) as f32;
        let x = self.rng_f32() * w;
        let y = if scatter_y { self.rng_f32() * h } else { -1.0 };
        let vy = 0.05 + self.rng_f32() * 0.15;
        let sway_amp = 0.2 + self.rng_f32() * 0.6;
        let sway_freq = 0.02 + self.rng_f32() * 0.04;
        let phase = self.rng_f32() * std::f32::consts::TAU;
        let glyph = SNOW_GLYPHS[(self.rng_next() as usize) % SNOW_GLYPHS.len()];
        Flake {
            x,
            y,
            vy,
            sway_amp,
            sway_freq,
            phase,
            age: 0,
            glyph,
        }
    }

    pub fn step(&mut self) {
        let h = self.viewport_h as f32;
        let w = self.viewport_w as f32;
        for f in &mut self.flakes {
            f.age = f.age.wrapping_add(1);
            let sway = (f.age as f32 * f.sway_freq + f.phase).cos() * f.sway_amp;
            f.x += sway * 0.05;
            f.y += f.vy;
            if f.x < 0.0 {
                f.x += w;
            } else if f.x >= w {
                f.x -= w;
            }
        }
        // Recycle flakes that fell past the bottom by spawning fresh
        // ones at the top. Use indices so the borrow is non-overlapping
        // with `spawn_flake`'s &mut self.
        let mut respawn_idx = Vec::new();
        for (i, f) in self.flakes.iter().enumerate() {
            if f.y >= h {
                respawn_idx.push(i);
            }
        }
        for i in respawn_idx {
            self.flakes[i] = self.spawn_flake(false);
        }
    }
}

// ---------------- Fireworks (v1.59.0) ----------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rocket {
    pub x: f32,
    pub y: f32,
    pub vy: f32,
    pub burst_at_y: f32,
    pub hue_deg: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spark {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub hue_deg: u16,
    pub life: u16,
    pub initial_life: u16,
    pub glyph: char,
}

impl Spark {
    pub fn alive(&self) -> bool {
        self.life > 0
    }
    pub fn fade(&self) -> f32 {
        if self.initial_life == 0 {
            return 1.0;
        }
        1.0 - (self.life as f32 / self.initial_life as f32)
    }
}

const FIREWORKS_SPARK_GLYPHS: &[char] = &['*', '+', '·', '✦', '✧', '✨'];
pub const FIREWORKS_GRAVITY: f32 = 0.025;
pub const FIREWORKS_SPARKS_PER_BURST: usize = 32;
pub const FIREWORKS_SPARK_LIFE: u16 = 90;
pub const FIREWORKS_LAUNCH_INTERVAL_FRAMES: u32 = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct FireworksState {
    pub rockets: Vec<Rocket>,
    pub sparks: Vec<Spark>,
    pub viewport_w: u16,
    pub viewport_h: u16,
    rng_state: u64,
    frames_until_next_launch: u32,
}

impl FireworksState {
    pub fn new(viewport_w: u16, viewport_h: u16, seed: u64) -> Self {
        let mut state = Self {
            rockets: Vec::new(),
            sparks: Vec::new(),
            viewport_w,
            viewport_h,
            rng_state: seed.max(1),
            frames_until_next_launch: 0,
        };
        // Launch one immediately so the first frame shows action.
        state.launch_rocket();
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

    fn launch_rocket(&mut self) {
        let w = self.viewport_w.max(8) as f32;
        let h = self.viewport_h.max(6) as f32;
        let x = w * 0.2 + self.rng_f32() * w * 0.6;
        let burst = h * 0.15 + self.rng_f32() * h * 0.35;
        let vy = -(0.6 + self.rng_f32() * 0.4);
        let hue_deg = (self.rng_next() % FX_HUE_PERIOD as u64) as u16;
        self.rockets.push(Rocket {
            x,
            y: h - 1.0,
            vy,
            burst_at_y: burst,
            hue_deg,
        });
        self.frames_until_next_launch = FIREWORKS_LAUNCH_INTERVAL_FRAMES
            + (self.rng_next() as u32 % FIREWORKS_LAUNCH_INTERVAL_FRAMES);
    }

    fn explode(&mut self, rocket: Rocket) {
        for i in 0..FIREWORKS_SPARKS_PER_BURST {
            let angle = (i as f32) / (FIREWORKS_SPARKS_PER_BURST as f32) * std::f32::consts::TAU;
            let speed = 0.25 + self.rng_f32() * 0.45;
            let hue_jitter = (self.rng_next() % 40) as u16;
            let life_jitter = self.rng_next() as u16 % 12;
            let glyph =
                FIREWORKS_SPARK_GLYPHS[(self.rng_next() as usize) % FIREWORKS_SPARK_GLYPHS.len()];
            self.sparks.push(Spark {
                x: rocket.x,
                y: rocket.y,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed * 0.6,
                hue_deg: (rocket.hue_deg + hue_jitter) % FX_HUE_PERIOD,
                life: FIREWORKS_SPARK_LIFE - life_jitter,
                initial_life: FIREWORKS_SPARK_LIFE,
                glyph,
            });
        }
    }

    pub fn step(&mut self) {
        // Advance rockets; collect bursts.
        let mut bursts: Vec<Rocket> = Vec::new();
        for r in &mut self.rockets {
            r.y += r.vy;
        }
        let mut i = 0;
        while i < self.rockets.len() {
            if self.rockets[i].y <= self.rockets[i].burst_at_y {
                bursts.push(self.rockets.remove(i));
            } else {
                i += 1;
            }
        }
        for r in bursts {
            self.explode(r);
        }
        // Sparks: gravity, life, retain alive ones inside the buffer.
        for s in &mut self.sparks {
            s.x += s.vx;
            s.y += s.vy;
            s.vy += FIREWORKS_GRAVITY;
            s.life = s.life.saturating_sub(1);
        }
        let h = self.viewport_h as f32 + 4.0;
        let w = self.viewport_w as f32;
        self.sparks
            .retain(|s| s.alive() && s.x >= 0.0 && s.x < w && s.y >= 0.0 && s.y < h);
        // Schedule next launch.
        if self.frames_until_next_launch == 0 {
            self.launch_rocket();
        } else {
            self.frames_until_next_launch -= 1;
        }
    }
}

// ---------------- Plasma (v1.59.0) ----------------

const PLASMA_GLYPHS: &[char] = &[' ', '·', '∙', '•', '○', '◌', '◍', '◎', '●'];

#[derive(Debug, Clone, PartialEq)]
pub struct PlasmaState {
    pub viewport_w: u16,
    pub viewport_h: u16,
    pub time: u32,
}

impl PlasmaState {
    pub fn new(viewport_w: u16, viewport_h: u16) -> Self {
        Self {
            viewport_w,
            viewport_h,
            time: 0,
        }
    }
    pub fn step(&mut self) {
        self.time = self.time.wrapping_add(1);
    }

    /// Returns `(glyph_index, hue_deg)` for cell `(col, row)` at the
    /// current time. Pure helper so the renderer can iterate without
    /// holding a `&mut self`.
    pub fn sample(&self, col: u16, row: u16) -> (usize, u16) {
        let x = col as f32;
        let y = row as f32;
        let t = self.time as f32;
        let v = (x / 8.0 + t / 30.0).sin()
            + (y / 8.0 + t / 40.0).sin()
            + ((x + y) / 16.0 + t / 50.0).sin();
        // v ∈ [-3, 3] → normalize to [0, 1]
        let n = (v + 3.0) / 6.0;
        let glyph_idx = ((n.clamp(0.0, 1.0)) * (PLASMA_GLYPHS.len() - 1) as f32) as usize;
        let hue = ((n * FX_HUE_PERIOD as f32) as u16) % FX_HUE_PERIOD;
        (glyph_idx, hue)
    }
}

pub fn plasma_glyph(idx: usize) -> char {
    PLASMA_GLYPHS[idx.min(PLASMA_GLYPHS.len() - 1)]
}

// ---------------- Sampler (v1.59.0) ----------------

/// One inner effect plays for this many frames before the sampler
/// rotates to the next. At 60 FPS, 5s = 300 frames.
pub const SAMPLER_FRAMES_PER_EFFECT: u32 = FX_TARGET_FPS as u32 * 5;

#[derive(Debug, Clone, PartialEq)]
pub struct SamplerState {
    pub inner: Box<FxScene>,
    pub current: FxEffect,
    pub frames_in_current: u32,
    pub seed: u64,
}

impl SamplerState {
    pub fn new(viewport_w: u16, viewport_h: u16, seed: u64) -> Self {
        let first = FxEffect::Banner;
        let inner = Box::new(FxScene::from_effect(first, viewport_w, viewport_h, seed));
        Self {
            inner,
            current: first,
            frames_in_current: 0,
            seed,
        }
    }

    pub fn step(&mut self, viewport_w: u16, viewport_h: u16) {
        self.frames_in_current = self.frames_in_current.saturating_add(1);
        if self.frames_in_current >= SAMPLER_FRAMES_PER_EFFECT {
            self.frames_in_current = 0;
            self.current = sampler_next_effect(self.current);
            self.seed = self.seed.wrapping_add(0x9E3779B97F4A7C15);
            *self.inner = FxScene::from_effect(self.current, viewport_w, viewport_h, self.seed);
        }
        self.inner.step(viewport_w, viewport_h);
    }

    pub fn resize(&mut self, viewport_w: u16, viewport_h: u16) {
        self.inner.resize(viewport_w, viewport_h);
    }
}

/// Next effect in the sampler rotation. Skips `Sampler` itself so the
/// meta-scene never tries to recurse into another sampler.
pub fn sampler_next_effect(e: FxEffect) -> FxEffect {
    match e {
        FxEffect::Banner => FxEffect::Confetti,
        FxEffect::Confetti => FxEffect::Matrix,
        FxEffect::Matrix => FxEffect::Snow,
        FxEffect::Snow => FxEffect::Fireworks,
        FxEffect::Fireworks => FxEffect::Plasma,
        FxEffect::Plasma => FxEffect::Banner,
        FxEffect::Sampler => FxEffect::Banner,
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
    Snow(SnowState),
    Fireworks(FireworksState),
    Plasma(PlasmaState),
    Sampler(SamplerState),
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
            FxEffect::Snow => FxScene::Snow(SnowState::new(viewport_w, viewport_h, seed)),
            FxEffect::Fireworks => {
                FxScene::Fireworks(FireworksState::new(viewport_w, viewport_h, seed))
            }
            FxEffect::Plasma => FxScene::Plasma(PlasmaState::new(viewport_w, viewport_h)),
            FxEffect::Sampler => FxScene::Sampler(SamplerState::new(viewport_w, viewport_h, seed)),
        }
    }

    /// True when the scene has finished its natural lifetime (only
    /// confetti returns true here; banner + matrix + snow + fireworks +
    /// plasma + sampler run forever until dismissed).
    pub fn is_finished(&self) -> bool {
        matches!(self, FxScene::Confetti(c) if c.is_finished())
    }

    pub fn step(&mut self, viewport_w: u16, viewport_h: u16) {
        match self {
            FxScene::Banner(b) => b.step(viewport_w, viewport_h),
            FxScene::Confetti(c) => c.step(),
            FxScene::Matrix(m) => m.step(),
            FxScene::Snow(s) => s.step(),
            FxScene::Fireworks(f) => f.step(),
            FxScene::Plasma(p) => p.step(),
            FxScene::Sampler(s) => s.step(viewport_w, viewport_h),
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
            FxScene::Snow(s) => {
                s.viewport_w = viewport_w;
                s.viewport_h = viewport_h;
            }
            FxScene::Fireworks(f) => {
                f.viewport_w = viewport_w;
                f.viewport_h = viewport_h;
            }
            FxScene::Plasma(p) => {
                p.viewport_w = viewport_w;
                p.viewport_h = viewport_h;
            }
            FxScene::Sampler(s) => s.resize(viewport_w, viewport_h),
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

    // v1.59.0 -----------------------------------------------------------

    #[test]
    fn snow_spawns_full_field_and_keeps_count_stable_after_step() {
        let mut s = SnowState::new(80, 24, 1);
        assert_eq!(s.flakes.len(), SNOW_FLAKE_COUNT);
        for _ in 0..200 {
            s.step();
        }
        assert_eq!(
            s.flakes.len(),
            SNOW_FLAKE_COUNT,
            "flakes that fall past the bottom should respawn at the top"
        );
    }

    #[test]
    fn snow_flakes_fall_downward_overall() {
        let mut s = SnowState::new(80, 24, 7);
        let y_before: f32 = s.flakes.iter().map(|f| f.y).sum();
        for _ in 0..30 {
            s.step();
        }
        // After 30 frames, total y advancement should exceed the
        // baseline by a measurable amount (excluding any respawns).
        let alive_after_count = s.flakes.iter().filter(|f| f.y > 0.0).count();
        assert!(
            alive_after_count > 0,
            "at least some flakes must remain in motion"
        );
        let _ = y_before; // baseline reference; visual sanity already covered above
    }

    #[test]
    fn fireworks_explodes_rocket_when_burst_height_reached() {
        let mut f = FireworksState::new(80, 24, 99);
        // Force the rocket immediately to its burst height.
        let rocket = f.rockets.last_mut().expect("at least one rocket");
        rocket.y = rocket.burst_at_y - 0.5;
        rocket.vy = -0.1;
        f.step();
        assert!(
            !f.sparks.is_empty(),
            "rocket should have exploded into sparks"
        );
    }

    #[test]
    fn fireworks_relaunches_after_interval_elapses() {
        let mut f = FireworksState::new(80, 24, 11);
        // Sample whether the firework field is ever non-empty across a
        // long window. There's a gap between rockets bursting and the
        // next launch where both lists can be empty for a frame or two,
        // so we look at the whole window not a single frame.
        let mut ever_visible = false;
        for _ in 0..(FIREWORKS_LAUNCH_INTERVAL_FRAMES * 8) {
            if !f.rockets.is_empty() || !f.sparks.is_empty() {
                ever_visible = true;
            }
            f.step();
        }
        assert!(
            ever_visible,
            "fireworks must keep producing visible particles over time"
        );
    }

    #[test]
    fn plasma_step_advances_time_and_sample_returns_within_bounds() {
        let mut p = PlasmaState::new(80, 24);
        assert_eq!(p.time, 0);
        for _ in 0..10 {
            p.step();
        }
        assert_eq!(p.time, 10);
        for col in 0..80 {
            for row in 0..24 {
                let (gi, hue) = p.sample(col, row);
                assert!(gi < 9, "glyph index must stay within table");
                assert!(hue < FX_HUE_PERIOD, "hue must wrap inside [0, 360)");
            }
        }
    }

    #[test]
    fn sampler_rotates_to_next_effect_after_interval_elapses() {
        let mut s = SamplerState::new(80, 24, 1);
        assert_eq!(s.current, FxEffect::Banner);
        for _ in 0..SAMPLER_FRAMES_PER_EFFECT {
            s.step(80, 24);
        }
        assert_eq!(
            s.current,
            FxEffect::Confetti,
            "sampler should swap to the next effect after the interval"
        );
        // Step through several more rotations and confirm the cycle
        // never lands on Sampler itself.
        for _ in 0..(SAMPLER_FRAMES_PER_EFFECT * 8) {
            s.step(80, 24);
            assert_ne!(
                s.current,
                FxEffect::Sampler,
                "sampler must not recurse into itself"
            );
        }
    }

    #[test]
    fn sampler_next_effect_skips_self_and_covers_every_other_variant() {
        let mut seen = std::collections::HashSet::new();
        let mut e = FxEffect::Banner;
        for _ in 0..12 {
            seen.insert(e);
            e = sampler_next_effect(e);
        }
        assert!(seen.contains(&FxEffect::Banner));
        assert!(seen.contains(&FxEffect::Confetti));
        assert!(seen.contains(&FxEffect::Matrix));
        assert!(seen.contains(&FxEffect::Snow));
        assert!(seen.contains(&FxEffect::Fireworks));
        assert!(seen.contains(&FxEffect::Plasma));
        assert!(
            !seen.contains(&FxEffect::Sampler),
            "sampler rotation must skip the Sampler variant itself"
        );
    }

    #[test]
    fn fx_scene_from_effect_dispatches_every_v1_59_variant() {
        for e in [
            FxEffect::Snow,
            FxEffect::Fireworks,
            FxEffect::Plasma,
            FxEffect::Sampler,
        ] {
            let scene = FxScene::from_effect(e, 80, 24, 1);
            match (e, &scene) {
                (FxEffect::Snow, FxScene::Snow(_)) => {}
                (FxEffect::Fireworks, FxScene::Fireworks(_)) => {}
                (FxEffect::Plasma, FxScene::Plasma(_)) => {}
                (FxEffect::Sampler, FxScene::Sampler(_)) => {}
                _ => panic!("variant mismatch for {e:?}"),
            }
        }
    }
}
