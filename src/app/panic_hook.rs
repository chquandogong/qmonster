//! Process-wide panic hook.
//!
//! qmonster runs as a long-lived TUI inside a tmux pane. A bare panic
//! would exit the process — closing (or dropping to a bare shell in)
//! the pane — and, with no hook installed, leave **no trace of why**.
//! That is exactly the "the monitor died on its own" symptom operators
//! hit, with nothing to diagnose from afterwards.
//!
//! On any panic this hook:
//! 1. restores the terminal (leaves raw mode / alt-screen) so the panic
//!    text is readable and the pane is not wedged, then
//! 2. appends a timestamped post-mortem to `<root>/crash.log`, then
//! 3. chains to the previously-installed (default) hook so stderr and
//!    abort/backtrace behaviour are unchanged.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Install the process-wide panic hook. `crash_log_dir` is the qmonster
/// storage root; the post-mortem is written to `<crash_log_dir>/crash.log`.
pub fn install(crash_log_dir: PathBuf) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best-effort terminal restore + crash log, wrapped so a panic
        // INSIDE the hook (a poisoned lock, a write error path, a
        // formatting panic) cannot double-panic and abort the process
        // before the default hook runs (Codex P2).
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // 1. Restore the terminal first so the panic text is
            //    readable and the pane is not left stuck in raw /
            //    alternate-screen mode. Safe even if we never entered
            //    the session.
            crate::app::terminal_session::leave_terminal_session();

            // 2. Persist a post-mortem for later diagnosis.
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "unknown".to_string());
            let message = panic_message(info.payload());
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            let ts = chrono::Utc::now().to_rfc3339();
            let report = format_crash_report(&ts, &location, &message, &backtrace);
            let _ = append_crash_log(&crash_log_dir, &report);
        }));

        // 3. Always preserve prior behaviour (stderr / RUST_BACKTRACE /
        //    abort), even if the best-effort block above panicked.
        default_hook(info);
    }));
}

/// Extract a human-readable message from a panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Render a single crash-log entry. Pure (no I/O, no globals) so the
/// format is unit-testable without triggering a real panic.
fn format_crash_report(ts: &str, location: &str, message: &str, backtrace: &str) -> String {
    format!(
        "==== qmonster panic ====\n\
         when:      {ts}\n\
         location:  {location}\n\
         message:   {message}\n\
         backtrace:\n{backtrace}\n\n"
    )
}

/// Append `contents` to `<dir>/crash.log`, creating the directory and
/// file if missing. Append-mode so successive crashes accumulate.
fn append_crash_log(dir: &Path, contents: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("crash.log"))?;
    file.write_all(contents.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn format_crash_report_includes_time_location_message_and_backtrace() {
        let out = format_crash_report(
            "2026-07-01T00:00:00+00:00",
            "src/app/foo.rs:10:5",
            "boom happened",
            "0: frame_a\n1: frame_b",
        );
        assert!(
            out.contains("2026-07-01T00:00:00+00:00"),
            "missing timestamp"
        );
        assert!(out.contains("src/app/foo.rs:10:5"), "missing location");
        assert!(out.contains("boom happened"), "missing message");
        assert!(out.contains("frame_a"), "missing backtrace");
        assert!(out.contains("panic"), "missing panic marker");
    }

    #[test]
    fn append_crash_log_creates_file_and_appends_in_order() {
        let td = TempDir::new().unwrap();
        append_crash_log(td.path(), "first entry\n").unwrap();
        append_crash_log(td.path(), "second entry\n").unwrap();
        let body = std::fs::read_to_string(td.path().join("crash.log")).unwrap();
        assert!(body.contains("first entry"));
        assert!(body.contains("second entry"));
        assert!(
            body.find("first entry").unwrap() < body.find("second entry").unwrap(),
            "second crash must be appended after the first, not overwrite it"
        );
    }

    #[test]
    fn append_crash_log_creates_missing_parent_dirs() {
        let td = TempDir::new().unwrap();
        let nested = td.path().join("deep/storage/root");
        append_crash_log(&nested, "x\n").unwrap();
        assert!(nested.join("crash.log").exists());
    }
}
