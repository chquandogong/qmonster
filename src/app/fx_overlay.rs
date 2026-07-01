//! v1.53.0: lifecycle wrapper around `fx_state::FxScene`.
//!
//! `FxOverlay` answers "is the overlay open right now? what scene is
//! playing? should it auto-dismiss this tick?" — keeping that logic
//! out of `tui_loop` so the loop just calls `open()`, `step()`,
//! `dismiss()`.

use std::time::{Duration, Instant};

use crate::app::config::{FxConfig, FxEffect};
use crate::app::fx_state::FxScene;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxTrigger {
    /// Operator pressed `Q`.
    Hotkey,
    /// `p` accept fired a successful pending-action dispatch.
    Celebration,
    /// `screensaver_idle_secs` elapsed without input.
    Screensaver,
}

#[derive(Debug)]
pub struct FxOverlay {
    /// `Some` when the overlay is showing; `None` when idle.
    state: Option<FxOverlayState>,
}

#[derive(Debug)]
struct FxOverlayState {
    scene: FxScene,
    opened_at: Instant,
    duration: Option<Duration>,
    trigger: FxTrigger,
    viewport_w: u16,
    viewport_h: u16,
}

impl FxOverlay {
    pub fn new() -> Self {
        Self { state: None }
    }

    pub fn is_open(&self) -> bool {
        self.state.is_some()
    }

    pub fn trigger(&self) -> Option<FxTrigger> {
        self.state.as_ref().map(|s| s.trigger)
    }

    pub fn scene(&self) -> Option<&FxScene> {
        self.state.as_ref().map(|s| &s.scene)
    }

    pub fn open(
        &mut self,
        config: &FxConfig,
        trigger: FxTrigger,
        now: Instant,
        viewport_w: u16,
        viewport_h: u16,
    ) {
        if !config.enabled {
            return;
        }
        // Celebration always shoots confetti regardless of the
        // configured default — it's the natural confirmation glyph.
        let effect = match trigger {
            FxTrigger::Celebration => FxEffect::Confetti,
            _ => config.effect,
        };
        let seed = (now.elapsed().as_nanos() as u64)
            ^ (viewport_w as u64).wrapping_mul(31)
            ^ (viewport_h as u64);
        let scene = FxScene::from_effect(effect, viewport_w, viewport_h, seed);
        let duration = match config.duration_secs {
            0 => None,
            n => Some(Duration::from_secs(n as u64)),
        };
        self.state = Some(FxOverlayState {
            scene,
            opened_at: now,
            duration,
            trigger,
            viewport_w,
            viewport_h,
        });
    }

    pub fn dismiss(&mut self) {
        self.state = None;
    }

    /// Advance the active scene one frame and check for auto-dismiss.
    /// Returns `true` if the overlay just dismissed itself this tick.
    pub fn step(&mut self, now: Instant, viewport_w: u16, viewport_h: u16) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };
        if state.viewport_w != viewport_w || state.viewport_h != viewport_h {
            state.scene.resize(viewport_w, viewport_h);
            state.viewport_w = viewport_w;
            state.viewport_h = viewport_h;
        }
        state.scene.step(viewport_w, viewport_h);
        let lifetime_done = state
            .duration
            .map(|d| now.saturating_duration_since(state.opened_at) >= d)
            .unwrap_or(false);
        if lifetime_done || state.scene.is_finished() {
            self.state = None;
            return true;
        }
        false
    }

    pub fn viewport(&self) -> Option<(u16, u16)> {
        self.state.as_ref().map(|s| (s.viewport_w, s.viewport_h))
    }
}

impl Default for FxOverlay {
    fn default() -> Self {
        Self::new()
    }
}

/// Decide whether the screensaver should auto-open right now.
///
/// Pure helper so the loop can call it once per render with the
/// current `last_user_activity` and `now`. Returns `true` when:
/// - `[fx]` is enabled,
/// - `[fx] screensaver_enabled` is true,
/// - the overlay is currently closed,
/// - and `now - last_user_activity` exceeds `screensaver_idle_secs`.
pub fn should_auto_open_screensaver(
    config: &FxConfig,
    overlay: &FxOverlay,
    last_user_activity: Instant,
    now: Instant,
) -> bool {
    if !config.enabled || !config.screensaver_enabled {
        return false;
    }
    if overlay.is_open() {
        return false;
    }
    let idle = now.saturating_duration_since(last_user_activity);
    idle >= Duration::from_secs(config.screensaver_idle_secs as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(effect: FxEffect, duration: u32) -> FxConfig {
        FxConfig {
            enabled: true,
            text: "TEST".into(),
            effect,
            duration_secs: duration,
            hotkey_enabled: true,
            celebration_enabled: true,
            screensaver_enabled: true,
            screensaver_idle_secs: 600,
        }
    }

    #[test]
    fn open_initializes_scene_matching_effect() {
        let mut o = FxOverlay::new();
        let now = Instant::now();
        o.open(&cfg(FxEffect::Banner, 5), FxTrigger::Hotkey, now, 80, 24);
        assert!(o.is_open());
        assert_eq!(o.trigger(), Some(FxTrigger::Hotkey));
        assert!(matches!(o.scene(), Some(FxScene::Banner(_))));
    }

    #[test]
    fn open_disabled_config_is_a_no_op() {
        let mut o = FxOverlay::new();
        let mut c = cfg(FxEffect::Banner, 5);
        c.enabled = false;
        o.open(&c, FxTrigger::Hotkey, Instant::now(), 80, 24);
        assert!(!o.is_open());
    }

    #[test]
    fn celebration_trigger_forces_confetti_regardless_of_configured_effect() {
        let mut o = FxOverlay::new();
        o.open(
            &cfg(FxEffect::Banner, 5),
            FxTrigger::Celebration,
            Instant::now(),
            80,
            24,
        );
        assert!(matches!(o.scene(), Some(FxScene::Confetti(_))));
    }

    #[test]
    fn dismiss_closes_the_overlay() {
        let mut o = FxOverlay::new();
        o.open(
            &cfg(FxEffect::Banner, 5),
            FxTrigger::Hotkey,
            Instant::now(),
            80,
            24,
        );
        o.dismiss();
        assert!(!o.is_open());
    }

    #[test]
    fn step_returns_true_when_duration_elapses() {
        let mut o = FxOverlay::new();
        let t0 = Instant::now();
        o.open(&cfg(FxEffect::Banner, 1), FxTrigger::Hotkey, t0, 80, 24);
        let elapsed = t0 + Duration::from_millis(1500);
        let dismissed = o.step(elapsed, 80, 24);
        assert!(dismissed);
        assert!(!o.is_open());
    }

    #[test]
    fn step_keeps_running_when_duration_is_zero() {
        let mut o = FxOverlay::new();
        let t0 = Instant::now();
        o.open(&cfg(FxEffect::Matrix, 0), FxTrigger::Hotkey, t0, 80, 24);
        for i in 1..50 {
            let dismissed = o.step(t0 + Duration::from_secs(i), 80, 24);
            assert!(!dismissed, "duration=0 should never auto-dismiss");
        }
        assert!(o.is_open());
    }

    #[test]
    fn step_self_dismisses_when_confetti_scene_finishes() {
        let mut o = FxOverlay::new();
        let t0 = Instant::now();
        o.open(
            &cfg(FxEffect::Confetti, 0),
            FxTrigger::Celebration,
            t0,
            20,
            20,
        );
        // Step past every particle's life.
        let mut dismissed = false;
        for i in 1..200 {
            if o.step(t0 + Duration::from_millis(i * 33), 20, 20) {
                dismissed = true;
                break;
            }
        }
        assert!(dismissed, "confetti scene should self-dismiss when empty");
    }

    #[test]
    fn screensaver_auto_open_fires_after_idle_threshold() {
        let mut o = FxOverlay::new();
        let mut c = cfg(FxEffect::Banner, 0);
        c.screensaver_enabled = true;
        c.screensaver_idle_secs = 5;
        let last = Instant::now();
        let now = last + Duration::from_secs(6);
        assert!(should_auto_open_screensaver(&c, &o, last, now));

        o.open(&c, FxTrigger::Screensaver, now, 80, 24);
        assert!(!should_auto_open_screensaver(&c, &o, last, now));
    }

    #[test]
    fn screensaver_auto_open_skipped_when_disabled() {
        let o = FxOverlay::new();
        let mut c = cfg(FxEffect::Banner, 0);
        c.screensaver_enabled = false;
        let last = Instant::now();
        let now = last + Duration::from_secs(6000);
        assert!(!should_auto_open_screensaver(&c, &o, last, now));
    }

    #[test]
    fn screensaver_auto_open_skipped_when_fx_globally_disabled() {
        let o = FxOverlay::new();
        let mut c = cfg(FxEffect::Banner, 0);
        c.enabled = false;
        c.screensaver_enabled = true;
        let last = Instant::now();
        let now = last + Duration::from_secs(6000);
        assert!(!should_auto_open_screensaver(&c, &o, last, now));
    }

    #[test]
    fn overlay_open_and_step_survives_degenerate_viewports() {
        // Restored v3.1.4 hardening guard: opening + stepping every
        // effect at a 0×0 / 1×1 viewport (e.g. a pane mid-resize) must
        // never panic. The panic hook would restore the terminal, but a
        // crash still kills the monitor — so keep the scenes panic-free.
        let effects = [
            FxEffect::Banner,
            FxEffect::Confetti,
            FxEffect::Matrix,
            FxEffect::Snow,
            FxEffect::Fireworks,
            FxEffect::Plasma,
            FxEffect::Sampler,
        ];
        let t0 = Instant::now();
        for effect in effects {
            for (w, h) in [(0u16, 0u16), (1, 1), (2, 1), (1, 3)] {
                let mut o = FxOverlay::new();
                o.open(&cfg(effect, 0), FxTrigger::Hotkey, t0, w, h);
                for i in 0..8 {
                    o.step(t0 + Duration::from_millis(i * 33), w, h);
                }
            }
        }
    }
}
