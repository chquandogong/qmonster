use std::io::{self, Write};

/// Terminal bell helper. Writes BEL (\x07) to stderr. Cheap enough that
/// no-op configuration is handled by callers, not here.
#[derive(Debug, Default, Clone, Copy)]
pub struct TerminalBell;

impl TerminalBell {
    pub fn ring(&self) {
        let _ = io::stderr().write_all(b"\x07");
        let _ = io::stderr().flush();
    }
}
