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
use qmonster::domain::signal::{IdleCause, TaskType};

const CLAUDE_STATUS: &str = include_str!("fixtures/real/claude_status.txt");
const CODEX_WELCOME_V0_122: &str = include_str!("fixtures/real/codex_welcome_v0_122.txt");
const CODEX_BOTTOM_STATUS_V0_122: &str =
    include_str!("fixtures/real/codex_bottom_status_v0_122.txt");
const GEMINI_IDLE_V0_39: &str = include_str!("fixtures/real/gemini_idle_v0_39.txt");

#[test]
fn claude_status_view_does_not_false_fire_permission_or_waiting() {
    let s = parse_common_signals(CLAUDE_STATUS);
    assert!(
        s.idle_state != Some(IdleCause::PermissionWait),
        "Claude tail with `⏵⏵ bypass permissions on` keyboard-hint must \
         not raise PermissionWait — it is a passive UI hint, not an \
         interactive ask"
    );
    assert!(
        s.idle_state != Some(IdleCause::InputWait),
        "Claude active-conversation tail must not raise InputWait \
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
        s.idle_state != Some(IdleCause::PermissionWait),
        "Codex welcome box displays `│ permissions: YOLO mode` as config \
         — must not be confused with an interactive approval ask"
    );
}

#[test]
fn codex_bottom_status_v0_122_is_inert_for_alerts() {
    let s = parse_common_signals(CODEX_BOTTOM_STATUS_V0_122);
    assert!(
        s.idle_state != Some(IdleCause::PermissionWait),
        "Codex /status box has `Permissions: Full Access` as a config \
         row, not an interactive ask"
    );
    assert!(s.idle_state != Some(IdleCause::InputWait));
    assert!(!s.log_storm);
    assert!(!s.verbose_answer);
}

#[test]
fn gemini_idle_v0_39_status_line_does_not_false_fire_anything() {
    let s = parse_common_signals(GEMINI_IDLE_V0_39);
    assert!(
        s.idle_state != Some(IdleCause::PermissionWait),
        "Gemini status line shows sandbox/quota fields, no interactive ask"
    );
    assert!(s.idle_state != Some(IdleCause::InputWait));
    assert!(!s.log_storm);
    assert!(!s.verbose_answer);
}

// ───────────────────────────────────────────────────────────────────────
// v1.13.1 follow-up — close residual error_hint + context_pressure noise
// ───────────────────────────────────────────────────────────────────────

#[test]
fn claude_status_does_not_false_fire_error_hint() {
    let s = parse_common_signals(CLAUDE_STATUS);
    assert!(
        !s.error_hint,
        "Claude conversation tail contains `Stop hook error: Failed with...` \
         hook-output lines plus prose like `[\"traceback\", ..., \"failed\"]` — \
         these are not stack traces and must not raise error_hint. v1.13.1 \
         tightens ERROR_MARKERS from substring to line-start structural patterns."
    );
}

#[test]
fn codex_welcome_v0_122_does_not_false_fire_error_hint() {
    let s = parse_common_signals(CODEX_WELCOME_V0_122);
    assert!(
        !s.error_hint,
        "Codex welcome box has no errors — must not raise error_hint"
    );
}

#[test]
fn gemini_idle_v0_39_does_not_false_fire_error_hint() {
    let s = parse_common_signals(GEMINI_IDLE_V0_39);
    assert!(
        !s.error_hint,
        "Gemini idle status line has no errors — must not raise error_hint"
    );
}

#[test]
fn claude_status_does_not_false_fire_context_pressure() {
    let s = parse_common_signals(CLAUDE_STATUS);
    assert!(
        s.context_pressure.is_none(),
        "Claude conversation tail contains prose like `Context 100% left` (a \
         quoted Codex status line in the operator's analysis) — common.rs \
         substring matching of `context|window|usage|compact` + `%` is \
         fundamentally unreliable on prose. v1.13.1 drops the generic \
         parser; per-provider structured parsing belongs in S3-1/S3-3/S3-4."
    );
}

#[test]
fn codex_welcome_v0_122_does_not_false_fire_context_pressure_in_common() {
    // The Codex welcome box has no `Context X% used · ... · 0 in · 0 out`
    // status line, so Codex's structured parser correctly fails. Without
    // v1.13.1, common.rs's loose `parse_context_pressure` would substring-
    // match anything containing "context" + "%". Welcome box text doesn't
    // happen to trigger that, but this test pins the contract: common.rs
    // never populates context_pressure.
    let s = parse_common_signals(CODEX_WELCOME_V0_122);
    assert!(
        s.context_pressure.is_none(),
        "common.rs must not populate context_pressure; per-provider only"
    );
}

#[test]
fn codex_bottom_status_v0_122_does_not_false_fire_context_pressure_in_common() {
    let s = parse_common_signals(CODEX_BOTTOM_STATUS_V0_122);
    assert!(
        s.context_pressure.is_none(),
        "common.rs must not populate context_pressure even when tail \
         contains `Context X% used` — that is Codex adapter's responsibility"
    );
}

#[test]
fn gemini_idle_v0_39_does_not_false_fire_context_pressure_in_common() {
    let s = parse_common_signals(GEMINI_IDLE_V0_39);
    assert!(
        s.context_pressure.is_none(),
        "Gemini status line columns include a `context` header word and \
         `0% used` data; common.rs substring matching incorrectly bridged \
         them. GeminiAdapter owns the structured status-table parser; \
         common.rs must still leave context_pressure unset."
    );
}

// ───────────────────────────────────────────────────────────────────────
// v1.14.0 (Slice 4, Task 16) — neutralize detect_task_type substring
// matching on prose. v1.13.x measurement (2026-04-25) showed
// RecommendationEmitted/min stuck at 91.5% reduction (vs 95% target)
// because detect_task_type matched `review` / `search` / `summary` / etc
// against agent/operator prose. These tests pin the new contract: only
// CLI-command-anchored patterns → non-Unknown; everything else → Unknown.
// ───────────────────────────────────────────────────────────────────────

#[test]
fn task_type_does_not_match_review_word_in_prose() {
    let s = parse_common_signals("Let me review this implementation plan.");
    assert!(!matches!(s.task_type, TaskType::Review));
}

#[test]
fn task_type_does_not_match_search_in_prose() {
    let s = parse_common_signals("I'll search the codebase for similar patterns.");
    assert!(!matches!(s.task_type, TaskType::CodeExploration));
}

#[test]
fn task_type_does_not_match_summary_in_prose() {
    let s = parse_common_signals("Here's a summary of what changed.");
    assert!(!matches!(s.task_type, TaskType::Summary));
}

#[test]
fn task_type_real_claude_status_fixture_yields_unknown() {
    let s = parse_common_signals(CLAUDE_STATUS);
    assert!(
        matches!(s.task_type, TaskType::Unknown),
        "Real Claude conversation tail must not classify as Review/Summary/etc"
    );
}

#[test]
fn task_type_codex_exec_command_still_matches_automation() {
    let s = parse_common_signals("codex exec scripted-task.sh");
    assert!(matches!(s.task_type, TaskType::Automation));
}

#[test]
fn task_type_explicit_resume_command_matches_session_resume() {
    let s = parse_common_signals("$ codex resume 019db0ff-cf26-79d0-84ca-8be9b63f1c39");
    assert!(matches!(s.task_type, TaskType::SessionResume));
}
