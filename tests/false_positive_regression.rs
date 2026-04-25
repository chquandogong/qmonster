//! v1.13.0 emergency suppression regression suite.
//!
//! Loads real-world tails captured from running Claude Code v2.1.119 /
//! Codex v0.122 / Gemini v0.39 tmux panes on 2026-04-25 and asserts that
//! `parse_common_signals` does NOT raise false-positive permission /
//! waiting / log-storm / verbose flags on these idle/status views.
//!
//! These fixtures explain the >50K alerts/day audit volume that was
//! making the alert queue useless. Tightening must keep these green
//! while existing unit tests in `src/adapters/common.rs::tests` still
//! exercise the synthetic positive-case fixtures.

use qmonster::adapters::common::parse_common_signals;

const CLAUDE_STATUS: &str = include_str!("fixtures/real/claude_status.txt");
const CODEX_WELCOME_V0_122: &str = include_str!("fixtures/real/codex_welcome_v0_122.txt");
const CODEX_BOTTOM_STATUS_V0_122: &str =
    include_str!("fixtures/real/codex_bottom_status_v0_122.txt");
const GEMINI_IDLE_V0_39: &str = include_str!("fixtures/real/gemini_idle_v0_39.txt");

#[test]
fn claude_status_view_does_not_false_fire_permission_or_waiting() {
    let s = parse_common_signals(CLAUDE_STATUS);
    assert!(
        !s.permission_prompt,
        "Claude tail with `⏵⏵ bypass permissions on` keyboard-hint must \
         not raise permission_prompt — it is a passive UI hint, not an \
         interactive ask"
    );
    assert!(
        !s.waiting_for_input,
        "Claude active-conversation tail must not raise waiting_for_input \
         — bare 'press enter' / 'continue?' substrings appear in normal \
         prose, not as live prompts"
    );
}

#[test]
fn claude_status_view_does_not_false_fire_log_storm_or_verbose() {
    let s = parse_common_signals(CLAUDE_STATUS);
    assert!(
        !s.log_storm,
        "Claude conversation tail is prose, not log lines — log_storm is \
         a false positive when triggered by output length alone"
    );
    assert!(
        !s.verbose_answer,
        "Claude tail must require an explicit hedge marker for \
         verbose_answer; line-count fallback fires on every code display"
    );
}

#[test]
fn codex_welcome_v0_122_does_not_false_fire_permission() {
    let s = parse_common_signals(CODEX_WELCOME_V0_122);
    assert!(
        !s.permission_prompt,
        "Codex welcome box displays `│ permissions: YOLO mode` as config \
         — must not be confused with an interactive approval ask"
    );
}

#[test]
fn codex_bottom_status_v0_122_is_inert_for_alerts() {
    let s = parse_common_signals(CODEX_BOTTOM_STATUS_V0_122);
    assert!(
        !s.permission_prompt,
        "Codex /status box has `Permissions: Full Access` as a config \
         row, not an interactive ask"
    );
    assert!(!s.waiting_for_input);
    assert!(!s.log_storm);
    assert!(!s.verbose_answer);
}

#[test]
fn gemini_idle_v0_39_status_line_does_not_false_fire_anything() {
    let s = parse_common_signals(GEMINI_IDLE_V0_39);
    assert!(
        !s.permission_prompt,
        "Gemini status line shows sandbox/quota fields, no interactive ask"
    );
    assert!(!s.waiting_for_input);
    assert!(!s.log_storm);
    assert!(!s.verbose_answer);
}
