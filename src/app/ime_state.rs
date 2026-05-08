//! v1.51.0: heuristic IME / non-English input indicator.
//!
//! Terminals do not expose IME state to TUI apps — once the OS-level
//! IME is engaged, the only signal we get is that incoming `Char`
//! events carry non-ASCII codepoints. This module turns that signal
//! into a small state machine the dashboard divider can render and
//! the loop can use to fire a single terminal bell on flip-on.
//!
//! Heuristic limits (accepted by the operator at design time):
//! - The first non-ASCII keystroke is what flips state on; we cannot
//!   detect "IME engaged but no key yet."
//! - We only inspect alphabetic characters: digits/punctuation/space
//!   are ambiguous (Korean IME composes through ASCII space) so we
//!   treat them as "no information" rather than evidence of either
//!   mode.

use std::time::{Duration, Instant};

/// How long after the last non-ASCII keystroke we keep the indicator
/// lit before the divider falls back to its normal rendering. Picked
/// to outlive a typical pause-between-words while still clearing if
/// the operator switches back to English without typing.
pub const IME_INDICATOR_TTL: Duration = Duration::from_secs(3);

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImeState {
    last_non_ascii_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeObservation {
    /// Char was not alphabetic — no information either way.
    Ignored,
    /// ASCII letter seen — IME state cleared. Caller should NOT bell.
    AsciiCleared,
    /// Non-ASCII letter seen — IME state set/refreshed. `transitioned_on`
    /// is true only on the inactive→active edge so the loop can bell once.
    NonAsciiSet { transitioned_on: bool },
}

impl ImeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a single `Char(c)` keystroke into the state machine.
    ///
    /// Non-alphabetic chars (digits, punctuation, whitespace, control)
    /// return `Ignored`. ASCII letters clear; non-ASCII letters set the
    /// active timestamp. The returned observation tells the caller
    /// whether to fire side-effects (bell / log).
    pub fn observe(&mut self, c: char, now: Instant) -> ImeObservation {
        if !c.is_alphabetic() {
            return ImeObservation::Ignored;
        }
        if c.is_ascii() {
            self.last_non_ascii_at = None;
            ImeObservation::AsciiCleared
        } else {
            let was_active = self.is_active(now);
            self.last_non_ascii_at = Some(now);
            ImeObservation::NonAsciiSet {
                transitioned_on: !was_active,
            }
        }
    }

    pub fn is_active(&self, now: Instant) -> bool {
        match self.last_non_ascii_at {
            Some(t) => now.duration_since(t) <= IME_INDICATOR_TTL,
            None => false,
        }
    }

    pub fn last_non_ascii_at(&self) -> Option<Instant> {
        self.last_non_ascii_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_is_not_active() {
        let state = ImeState::new();
        assert!(!state.is_active(Instant::now()));
    }

    #[test]
    fn non_ascii_letter_activates_and_signals_first_transition() {
        let mut state = ImeState::new();
        let now = Instant::now();
        let obs = state.observe('한', now);
        assert!(matches!(
            obs,
            ImeObservation::NonAsciiSet {
                transitioned_on: true
            }
        ));
        assert!(state.is_active(now));
    }

    #[test]
    fn second_non_ascii_within_ttl_does_not_re_signal_transition() {
        let mut state = ImeState::new();
        let t0 = Instant::now();
        state.observe('한', t0);
        let obs = state.observe('글', t0 + Duration::from_millis(500));
        assert!(matches!(
            obs,
            ImeObservation::NonAsciiSet {
                transitioned_on: false
            }
        ));
    }

    #[test]
    fn ascii_letter_clears_state_immediately() {
        let mut state = ImeState::new();
        let t0 = Instant::now();
        state.observe('한', t0);
        let obs = state.observe('a', t0 + Duration::from_millis(100));
        assert!(matches!(obs, ImeObservation::AsciiCleared));
        assert!(!state.is_active(t0 + Duration::from_millis(101)));
    }

    #[test]
    fn re_activation_after_clear_signals_transition_again() {
        let mut state = ImeState::new();
        let t0 = Instant::now();
        state.observe('한', t0);
        state.observe('a', t0 + Duration::from_millis(100));
        let obs = state.observe('가', t0 + Duration::from_millis(200));
        assert!(matches!(
            obs,
            ImeObservation::NonAsciiSet {
                transitioned_on: true
            }
        ));
    }

    #[test]
    fn state_decays_after_ttl_without_clearing_via_ascii() {
        let mut state = ImeState::new();
        let t0 = Instant::now();
        state.observe('한', t0);
        assert!(state.is_active(t0 + IME_INDICATOR_TTL));
        assert!(!state.is_active(t0 + IME_INDICATOR_TTL + Duration::from_millis(1)));
    }

    #[test]
    fn re_activating_after_ttl_decay_signals_transition() {
        let mut state = ImeState::new();
        let t0 = Instant::now();
        state.observe('한', t0);
        let later = t0 + IME_INDICATOR_TTL + Duration::from_secs(1);
        let obs = state.observe('한', later);
        assert!(matches!(
            obs,
            ImeObservation::NonAsciiSet {
                transitioned_on: true
            }
        ));
    }

    #[test]
    fn digits_punctuation_and_space_are_ignored() {
        let mut state = ImeState::new();
        let now = Instant::now();
        state.observe('한', now);
        // None of these should clear — they don't disambiguate mode.
        for c in ['1', '!', ' ', '.', '\t', '\n'] {
            let obs = state.observe(c, now + Duration::from_millis(50));
            assert_eq!(obs, ImeObservation::Ignored, "char {c:?} should be ignored");
        }
        assert!(state.is_active(now + Duration::from_millis(100)));
    }

    #[test]
    fn ascii_letter_when_already_inactive_returns_cleared_but_no_bell_caller_action() {
        // Caller-side contract: AsciiCleared never carries a transition flag
        // so the loop's "bell on edge" logic only fires for NonAsciiSet
        // {transitioned_on: true}. This test pins that contract.
        let mut state = ImeState::new();
        let obs = state.observe('a', Instant::now());
        assert_eq!(obs, ImeObservation::AsciiCleared);
    }

    #[test]
    fn cjk_characters_all_treated_as_non_ascii() {
        let now = Instant::now();
        for c in ['한', 'あ', '中', 'ñ', 'ü'] {
            let mut state = ImeState::new();
            let obs = state.observe(c, now);
            assert!(
                matches!(obs, ImeObservation::NonAsciiSet { .. }),
                "char {c:?} should activate IME state"
            );
        }
    }
}
