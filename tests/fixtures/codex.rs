//! Shared Codex status-bar + /status-box fixtures for integration tests.
//!
//! Unit tests in `src/adapters/codex.rs::mod tests` keep their own inline
//! fixtures so `src/` stays self-contained; this module is for external
//! integration tests under `tests/`. When Codex CLI formats drift, both
//! copies must be updated together.

pub const CODEX_STATUS_FIXTURE_V0_122_0: &str = "Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · <redacted> · gp";

pub const CODEX_STATUS_BOX_FIXTURE: &str =
    "│  Model:                       gpt-5.4 (reasoning xhigh, summaries auto)                 │";
