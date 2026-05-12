use std::io::{self, Write};

/// Terminal bell helper. Writes BEL (\x07) to stderr. Cheap enough that
/// no-op configuration is handled by callers, not here.
#[derive(Debug, Default, Clone, Copy)]
pub struct TerminalBell;

impl TerminalBell {
    pub fn ring(&self) {
        let _ = self.ring_to(&mut io::stderr());
    }

    /// Test-friendly variant: write the BEL byte into the supplied
    /// `Write` impl. Returns the underlying io result so tests can
    /// assert success on infallible buffers.
    pub fn ring_to<W: Write>(&self, sink: &mut W) -> io::Result<()> {
        sink.write_all(b"\x07")?;
        sink.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_to_writes_single_bel_byte() {
        let bell = TerminalBell;
        let mut buf: Vec<u8> = Vec::new();
        bell.ring_to(&mut buf).unwrap();
        assert_eq!(buf, b"\x07");
    }

    #[test]
    fn ring_to_appends_on_repeated_calls() {
        // Each ring writes exactly one BEL — no accidental
        // pre/post padding that would visually duplicate alerts.
        let bell = TerminalBell;
        let mut buf: Vec<u8> = Vec::new();
        bell.ring_to(&mut buf).unwrap();
        bell.ring_to(&mut buf).unwrap();
        bell.ring_to(&mut buf).unwrap();
        assert_eq!(buf, b"\x07\x07\x07");
    }
}
