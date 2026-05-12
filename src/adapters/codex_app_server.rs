//! Phase F F-6 (v1.32.0): Codex `app-server` JSON-RPC client.
//!
//! Codex CLI 0.125.0 ships an `app-server` subcommand that speaks
//! line-delimited JSON-RPC 2.0 over stdin/stdout. This module wraps
//! a child `codex app-server` process (or any [`JsonRpcIo`] stub for
//! tests) and exposes the two calls Qmonster needs in v1.32.0:
//!
//! 1. `initialize` — handshake required before any other call. The
//!    response carries the server's `userAgent` string we keep for
//!    diagnostics.
//! 2. `account/rateLimits/read` — returns the *account-level* rate
//!    limit windows. Unlike the per-pane signals scraped from
//!    statuslines or sidefiles, these quotas apply to ALL Codex
//!    panes simultaneously (one upstream account = one bucket), so
//!    Task 2 will broadcast a single poll's `resets_at` +
//!    `usedPercent` into every Codex pane's signal set.
//!
//! ## Spawn rationale
//!
//! On Linux the dev host is missing bubblewrap, so `codex app-server`
//! refuses to start under the default `workspace-write` sandbox. The
//! operator has explicitly opted into running app-server via
//! `[provider_setup] codex_app_server = true` (G-2 config from
//! v1.30.0), so spawn passes the `sandbox_mode="danger-full-access"`
//! config override to bypass the sandbox.
//!
//! ## Protocol shape (verified live, Codex 0.125.0)
//!
//! Request line (NDJSON, exactly one JSON object per line):
//!
//! ```json
//! {"method":"initialize","id":1,"jsonrpc":"2.0",
//!  "params":{"clientInfo":{"name":"qmonster","version":"<v>"}}}
//! ```
//!
//! Initialize response:
//!
//! ```json
//! {"id":1,"result":{"userAgent":"...","codexHome":"...",
//!  "platformFamily":"unix","platformOs":"linux"}}
//! ```
//!
//! Rate-limits request:
//!
//! ```json
//! {"method":"account/rateLimits/read","id":2,"jsonrpc":"2.0"}
//! ```
//!
//! Rate-limits response (fields of interest only — extra ignored):
//!
//! ```json
//! {"id":1,"result":{"rateLimits":{
//!   "primary":{"usedPercent":2,"windowDurationMins":300,
//!              "resetsAt":1777448249},
//!   "secondary":{"usedPercent":1,"windowDurationMins":10080,
//!                "resetsAt":1777959698}}}}
//! ```
//!
//! `primary` is the rolling 5-hour window; `secondary` is the rolling
//! 7-day / weekly window.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

/// Account-level Codex rate-limit snapshot. Both windows are
/// optional because the server may omit one (e.g. brand-new account
/// with no usage on a window) and we want the parser to remain
/// best-effort for those shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRateLimits {
    /// 5-hour rolling window (`rateLimits.primary`).
    pub primary: Option<CodexRateWindow>,
    /// 7-day / weekly rolling window (`rateLimits.secondary`).
    pub secondary: Option<CodexRateWindow>,
}

/// A single Codex rate-limit window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexRateWindow {
    /// Integer 0..=100 share of the window already consumed.
    pub used_percent: u8,
    /// Length of the window in minutes (300 for 5h, 10080 for 7d).
    pub window_duration_mins: u32,
    /// Unix epoch seconds when the window next resets.
    pub resets_at_unix_seconds: u64,
}

/// Line-oriented JSON-RPC IO abstraction so tests can stub the
/// transport without spawning a real subprocess.
pub trait JsonRpcIo {
    /// Write one line. The implementation MUST append the trailing
    /// `\n` if it isn't present so the server sees NDJSON framing.
    fn write_line(&mut self, line: &str) -> std::io::Result<()>;

    /// Read one line. The returned string MUST NOT include the
    /// trailing `\n`.
    fn read_line(&mut self) -> std::io::Result<String>;
}

/// Subprocess-backed [`JsonRpcIo`] wrapping a `codex app-server`
/// child. Drops cleanly: closing stdin signals EOF, then we wait on
/// the child to reap the zombie.
pub struct SubprocessIo {
    /// Wrapped in `Option` so [`Drop`] can take ownership of the
    /// stdin handle to close it before waiting on the child.
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    child: Option<Child>,
}

impl SubprocessIo {
    /// Spawn `codex app-server` with the sandbox override and piped
    /// stdio. `_version` is currently unused (the actual handshake
    /// happens in [`CodexAppServer::initialize`]); accepted for
    /// future-proofing if spawn ever needs the client version too.
    pub fn spawn(_version: &str) -> std::io::Result<Self> {
        let mut child = Command::new("codex")
            .arg("app-server")
            .arg("-c")
            .arg("sandbox_mode=\"danger-full-access\"")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("codex app-server child had no stdin pipe"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("codex app-server child had no stdout pipe"))?;
        Ok(Self {
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            child: Some(child),
        })
    }
}

impl JsonRpcIo for SubprocessIo {
    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("stdin already closed"))?;
        stdin.write_all(line.as_bytes())?;
        if !line.ends_with('\n') {
            stdin.write_all(b"\n")?;
        }
        stdin.flush()?;
        Ok(())
    }

    fn read_line(&mut self) -> std::io::Result<String> {
        let mut buf = String::new();
        let n = self.stdout.read_line(&mut buf)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "codex app-server closed stdout",
            ));
        }
        // Strip the trailing newline (and any \r) the server emits.
        while matches!(buf.chars().last(), Some('\n' | '\r')) {
            buf.pop();
        }
        Ok(buf)
    }
}

impl Drop for SubprocessIo {
    fn drop(&mut self) {
        // Closing stdin first lets the server shut down gracefully on
        // EOF; without this the child can sit waiting on the next
        // request and the subsequent `wait` would block forever.
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            // Best-effort reap. If wait fails for any reason we kill
            // and try once more so we don't leak zombies. Errors past
            // that point are intentionally swallowed — Drop must not
            // panic.
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// High-level JSON-RPC client. Holds a [`JsonRpcIo`] transport, the
/// next request id to issue, and a flag tracking whether the
/// `initialize` handshake has completed.
pub struct CodexAppServer<IO: JsonRpcIo> {
    io: IO,
    next_id: u64,
    initialized: bool,
}

impl<IO: JsonRpcIo> CodexAppServer<IO> {
    /// Wrap an existing transport. The returned client is *not*
    /// initialized — call [`Self::initialize`] before any other
    /// method.
    pub fn new(io: IO) -> Self {
        // next_id starts at 1: id=0 is sometimes ambiguous with
        // "no id" in JSON-RPC server implementations, and starting at
        // 1 keeps client-issued ids strictly positive integers.
        Self {
            io,
            next_id: 1,
            initialized: false,
        }
    }

    /// Send `initialize` and consume the matching response. Returns
    /// the server's `userAgent` string (or an empty string if the
    /// server omits the field) for diagnostics.
    ///
    /// Errors when the transport fails, the response cannot be
    /// parsed as JSON, or the response carries an `error` object
    /// instead of a `result`.
    pub fn initialize(
        &mut self,
        client_name: &str,
        client_version: &str,
    ) -> Result<String, String> {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({
            "method": "initialize",
            "id": id,
            "jsonrpc": "2.0",
            "params": {
                "clientInfo": {
                    "name": client_name,
                    "version": client_version,
                }
            }
        });
        self.send_request(&req)?;
        let resp = self.read_response_for(id)?;
        if let Some(err) = resp.get("error") {
            return Err(format!("initialize returned error: {err}"));
        }
        let result = resp
            .get("result")
            .ok_or_else(|| "initialize response missing `result`".to_string())?;
        let user_agent = result
            .get("userAgent")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        self.initialized = true;
        Ok(user_agent)
    }

    /// Send `account/rateLimits/read` and parse the response into a
    /// [`CodexRateLimits`] snapshot.
    ///
    /// Errors when the transport fails, the response cannot be
    /// parsed as JSON, the response carries an `error` object, or
    /// the `result.rateLimits` object is missing.
    pub fn read_rate_limits(&mut self) -> Result<CodexRateLimits, String> {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({
            "method": "account/rateLimits/read",
            "id": id,
            "jsonrpc": "2.0",
        });
        self.send_request(&req)?;
        let resp = self.read_response_for(id)?;
        if let Some(err) = resp.get("error") {
            return Err(format!("rateLimits/read returned error: {err}"));
        }
        let rate_limits = resp
            .get("result")
            .and_then(|r| r.get("rateLimits"))
            .ok_or_else(|| "rateLimits/read response missing `result.rateLimits`".to_string())?;
        Ok(CodexRateLimits {
            primary: parse_window(rate_limits.get("primary")),
            secondary: parse_window(rate_limits.get("secondary")),
        })
    }

    fn send_request(&mut self, req: &Value) -> Result<(), String> {
        let line =
            serde_json::to_string(req).map_err(|e| format!("failed to serialize request: {e}"))?;
        self.io
            .write_line(&line)
            .map_err(|e| format!("failed to write request: {e}"))
    }

    fn read_response_for(&mut self, expected_id: u64) -> Result<Value, String> {
        loop {
            let line = self
                .io
                .read_line()
                .map_err(|e| format!("failed to read response: {e}"))?;
            let value: Value = serde_json::from_str(&line)
                .map_err(|e| format!("failed to parse response JSON: {e}"))?;
            // JSON-RPC 2.0 notification: has `method`, no `id`. Skip — the
            // codex CLI v0.128.0 sends `remoteControl/status/changed`
            // immediately after `initialize` and may push others between
            // request/response pairs. Notifications are not addressed to
            // any of our outstanding requests.
            if value.get("id").is_none() && value.get("method").is_some() {
                continue;
            }
            // Strict id matching: a response addressed to a different id is
            // a protocol violation under our single-inflight assumption.
            // Surface it instead of silently consuming, so a future
            // multi-request rework hits an honest error rather than a
            // misrouted response.
            match value.get("id").and_then(Value::as_u64) {
                Some(id) if id == expected_id => return Ok(value),
                Some(id) => {
                    return Err(format!(
                        "response id mismatch: got {id}, expected {expected_id}"
                    ));
                }
                None => {
                    return Err(
                        "response missing `id` field (and is not a notification)".to_string()
                    );
                }
            }
        }
    }
}

impl CodexAppServer<SubprocessIo> {
    /// Convenience constructor: spawn the `codex app-server` child,
    /// run `initialize`, and return the ready-to-poll client. The
    /// returned `userAgent` is dropped — wrap with [`Self::new`] +
    /// [`Self::initialize`] manually if you need it.
    pub fn spawn(client_name: &str, client_version: &str) -> Result<Self, String> {
        let io = SubprocessIo::spawn(client_version)
            .map_err(|e| format!("failed to spawn codex app-server: {e}"))?;
        let mut client = Self::new(io);
        client.initialize(client_name, client_version)?;
        Ok(client)
    }
}

/// Parse a single window object (`primary` / `secondary`) into a
/// [`CodexRateWindow`]. Returns `None` if the field is absent or any
/// of the three required sub-fields fails to parse — we'd rather
/// publish "no data" than partial garbage to the UI layer.
fn parse_window(v: Option<&Value>) -> Option<CodexRateWindow> {
    let v = v?;
    let used_percent = v.get("usedPercent").and_then(Value::as_u64)?;
    let window_duration_mins = v.get("windowDurationMins").and_then(Value::as_u64)?;
    let resets_at = v.get("resetsAt").and_then(Value::as_u64)?;
    Some(CodexRateWindow {
        used_percent: used_percent.min(100) as u8,
        window_duration_mins: window_duration_mins.min(u32::MAX as u64) as u32,
        resets_at_unix_seconds: resets_at,
    })
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
/// Test-only stub IO: queues up canned response lines and captures
/// the request lines the client writes. Public to the test module
/// only — production code never sees it.
pub(crate) struct VecDequeIo {
    pub write_buffer: Vec<String>,
    pub read_buffer: std::collections::VecDeque<String>,
}

#[cfg(test)]
impl VecDequeIo {
    pub fn new(reads: Vec<&str>) -> Self {
        Self {
            write_buffer: Vec::new(),
            read_buffer: reads.into_iter().map(String::from).collect(),
        }
    }
}

#[cfg(test)]
impl JsonRpcIo for VecDequeIo {
    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        self.write_buffer.push(line.to_string());
        Ok(())
    }

    fn read_line(&mut self) -> std::io::Result<String> {
        self.read_buffer.pop_front().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "no canned response left")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_writes_initialize_then_returns_user_agent() {
        let io = VecDequeIo::new(vec![
            r#"{"id":1,"jsonrpc":"2.0","result":{"userAgent":"foo/1.0","codexHome":"/tmp/cx"}}"#,
        ]);
        let mut client = CodexAppServer::new(io);
        let ua = client
            .initialize("qmonster", "1.32.0")
            .expect("initialize must succeed on canned ok response");
        assert_eq!(ua, "foo/1.0");
        // Exactly one line written, containing both `method` and the
        // clientInfo fields we promise the server.
        assert_eq!(client.io.write_buffer.len(), 1);
        let written = &client.io.write_buffer[0];
        assert!(
            written.contains("\"method\":\"initialize\""),
            "request must carry method=initialize, got: {written}"
        );
        assert!(
            written.contains("\"clientInfo\""),
            "request must carry clientInfo, got: {written}"
        );
        assert!(
            written.contains("\"name\":\"qmonster\""),
            "request must carry the supplied client name, got: {written}"
        );
    }

    #[test]
    fn read_rate_limits_parses_primary_and_secondary() {
        // Verbatim shape captured from a live `codex app-server`
        // probe (Codex 0.125.0) — extra fields like `credits`,
        // `planType`, `rateLimitsByLimitId` must be ignored.
        let body = r#"{"id":1,"jsonrpc":"2.0","result":{"rateLimits":{
            "limitId":"codex",
            "primary":{"usedPercent":2,"windowDurationMins":300,"resetsAt":1777448249},
            "secondary":{"usedPercent":1,"windowDurationMins":10080,"resetsAt":1777959698},
            "credits":{"used":0},
            "planType":"pro",
            "rateLimitReachedType":null
        },"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":2}}}}}"#;
        let io = VecDequeIo::new(vec![body]);
        let mut client = CodexAppServer::new(io);
        // Skip initialize for this unit — we exercise read_rate_limits
        // directly so the test isn't coupled to handshake plumbing.
        client.initialized = true;
        let rl = client
            .read_rate_limits()
            .expect("full canned response must parse");
        let primary = rl.primary.expect("primary window must be present");
        assert_eq!(primary.used_percent, 2);
        assert_eq!(primary.window_duration_mins, 300);
        assert_eq!(primary.resets_at_unix_seconds, 1777448249);
        let secondary = rl.secondary.expect("secondary window must be present");
        assert_eq!(secondary.used_percent, 1);
        assert_eq!(secondary.window_duration_mins, 10080);
        assert_eq!(secondary.resets_at_unix_seconds, 1777959698);
    }

    #[test]
    fn read_rate_limits_returns_err_when_response_has_error() {
        let io = VecDequeIo::new(vec![
            r#"{"id":1,"jsonrpc":"2.0","error":{"code":-32601,"message":"method not found"}}"#,
        ]);
        let mut client = CodexAppServer::new(io);
        client.initialized = true;
        let err = client
            .read_rate_limits()
            .expect_err("error-shaped response must surface as Err");
        assert!(
            err.contains("method not found"),
            "Err string must include the upstream message, got: {err}"
        );
    }

    #[test]
    fn read_rate_limits_handles_missing_secondary_field() {
        let io = VecDequeIo::new(vec![
            r#"{"id":1,"result":{"rateLimits":{"primary":{"usedPercent":5,"windowDurationMins":300,"resetsAt":100}}}}"#,
        ]);
        let mut client = CodexAppServer::new(io);
        client.initialized = true;
        let rl = client
            .read_rate_limits()
            .expect("response without secondary must still parse");
        let primary = rl.primary.expect("primary must parse");
        assert_eq!(primary.used_percent, 5);
        assert_eq!(primary.window_duration_mins, 300);
        assert_eq!(primary.resets_at_unix_seconds, 100);
        assert!(
            rl.secondary.is_none(),
            "missing secondary must surface as None, not a default-valued window"
        );
    }

    #[test]
    fn read_rate_limits_returns_err_on_malformed_json() {
        let io = VecDequeIo::new(vec!["not json {"]);
        let mut client = CodexAppServer::new(io);
        client.initialized = true;
        let err = client
            .read_rate_limits()
            .expect_err("malformed response line must surface as Err");
        assert!(
            err.to_lowercase().contains("parse"),
            "Err string should mention the parse failure, got: {err}"
        );
    }

    #[test]
    fn initialize_returns_err_when_response_carries_error_field() {
        // id matches; the failure path is the JSON-RPC `error` object.
        // (Strict id-mismatch failure is covered separately by
        // `read_response_errors_on_id_mismatch`.)
        let io = VecDequeIo::new(vec![
            r#"{"id":1,"jsonrpc":"2.0","error":{"code":-32000,"message":"boom"}}"#,
        ]);
        let mut client = CodexAppServer::new(io);
        let err = client
            .initialize("qmonster", "1.32.0")
            .expect_err("initialize with error response must fail");
        assert!(
            err.contains("boom"),
            "Err string must include the upstream error message, got: {err}"
        );
        assert!(
            !client.initialized,
            "client must remain uninitialized after a failed handshake"
        );
    }

    #[test]
    fn parse_window_returns_none_when_required_field_missing() {
        // Sanity check on the helper: drop `resetsAt` and we must
        // refuse the whole window rather than synthesize a 0.
        let v = json!({"usedPercent": 5, "windowDurationMins": 300});
        assert!(parse_window(Some(&v)).is_none());
    }

    #[test]
    fn parse_window_clamps_used_percent_above_100() {
        // The server sometimes returns 102 / 105 just past a window
        // rollover; clamp to 100 so downstream `u8`-typed UI fields
        // never overflow.
        let v = json!({
            "usedPercent": 105,
            "windowDurationMins": 300,
            "resetsAt": 12345,
        });
        let w = parse_window(Some(&v)).expect("valid window must parse");
        assert_eq!(w.used_percent, 100);
        assert_eq!(w.resets_at_unix_seconds, 12345);
    }

    #[test]
    fn read_response_skips_jsonrpc_notification_between_request_and_response() {
        let io = VecDequeIo::new(vec![
            // init response
            r#"{"id":1,"jsonrpc":"2.0","result":{"userAgent":"qm/0.128"}}"#,
            // unsolicited notification (codex 0.128 emits this after init)
            r#"{"method":"remoteControl/status/changed","jsonrpc":"2.0","params":{"status":"disabled","environmentId":null}}"#,
            // rateLimits response
            r#"{"id":2,"jsonrpc":"2.0","result":{"rateLimits":{"primary":{"usedPercent":1,"windowDurationMins":300,"resetsAt":1777896971},"secondary":{"usedPercent":9,"windowDurationMins":10080,"resetsAt":1777959698}}}}"#,
        ]);
        let mut client = CodexAppServer::new(io);
        let user_agent = client
            .initialize("qmonster", "1.x")
            .expect("initialize should succeed");
        assert!(user_agent.contains("qm/0.128"));
        let rl = client
            .read_rate_limits()
            .expect("rateLimits/read should skip notification and parse the next line");
        assert_eq!(rl.primary.unwrap().used_percent, 1);
        assert_eq!(rl.secondary.unwrap().used_percent, 9);
        assert_eq!(rl.primary.unwrap().resets_at_unix_seconds, 1777896971);
        assert_eq!(rl.secondary.unwrap().resets_at_unix_seconds, 1777959698);
    }

    #[test]
    fn read_response_errors_on_id_mismatch() {
        // Protocol violation: server returns id=999 for our id=1
        // initialize. We must surface this rather than silently consume
        // the response.
        let io = VecDequeIo::new(vec![
            r#"{"id":999,"jsonrpc":"2.0","result":{"userAgent":""}}"#,
        ]);
        let mut client = CodexAppServer::new(io);
        let err = client
            .initialize("qm", "1.x")
            .expect_err("id mismatch must error");
        assert!(
            err.contains("id mismatch") && err.contains("999") && err.contains("expected 1"),
            "error must name both ids; got: {err}"
        );
    }

    #[test]
    fn read_response_errors_on_response_missing_id() {
        // Malformed response with neither id nor method should error rather
        // than be treated as a valid result.
        let io = VecDequeIo::new(vec![r#"{"jsonrpc":"2.0","result":{}}"#]);
        let mut client = CodexAppServer::new(io);
        let err = client
            .initialize("qm", "1.x")
            .expect_err("missing-id response must error");
        assert!(
            err.contains("missing `id`"),
            "error must mention missing id; got: {err}"
        );
    }

    #[test]
    fn read_response_skips_multiple_consecutive_notifications() {
        // Defensive: codex may push more than one notification between
        // request-response pairs. Skip them all.
        let io = VecDequeIo::new(vec![
            r#"{"id":1,"jsonrpc":"2.0","result":{"userAgent":""}}"#,
            r#"{"method":"a","jsonrpc":"2.0","params":{}}"#,
            r#"{"method":"b","jsonrpc":"2.0","params":{}}"#,
            r#"{"method":"c","jsonrpc":"2.0","params":{}}"#,
            r#"{"id":2,"jsonrpc":"2.0","result":{"rateLimits":{"primary":{"usedPercent":3,"windowDurationMins":300,"resetsAt":100}}}}"#,
        ]);
        let mut client = CodexAppServer::new(io);
        client.initialize("qm", "1.x").unwrap();
        let rl = client.read_rate_limits().unwrap();
        assert_eq!(rl.primary.unwrap().used_percent, 3);
    }
}
