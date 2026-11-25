//! The OSC sequences in the output of a program.
//!
//! The VT library has no hook for the OSC codes that it does not know, thus
//! this module finds the OSC sequences itself. The scanner uses the rules of
//! the parser of the VT library for the start and the end of an OSC
//! sequence: the two always see the same sequences.

const BELL: u8 = 0x07;
const CANCEL: u8 = 0x18;
const SUBSTITUTE: u8 = 0x1a;
const ESCAPE: u8 = 0x1b;

/// The output of a program is not trusted. A sequence that is longer than
/// this limit is ignored, thus the scanner uses a fixed quantity of memory.
const MAX_SEQUENCE_BYTES: usize = 4096;

/// An operating system command (OSC) from the output of a program:
/// `ESC ] code ; payload`, with BEL or `ESC \` at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Osc<'a> {
    /// The number before the first `;`, for example 133 or 7501.
    pub code: u32,
    /// The bytes after the first `;`. Empty if the sequence has no `;`.
    pub payload: &'a [u8],
}

impl<'a> Osc<'a> {
    /// Returns `None` if the sequence does not start with a decimal number.
    fn parse(sequence: &'a [u8]) -> Option<Self> {
        let separator = sequence.iter().position(|byte| *byte == b';');
        let (digits, payload) = match separator {
            Some(index) => (&sequence[..index], &sequence[index + 1..]),
            None => (sequence, &[][..]),
        };
        let code = std::str::from_utf8(digits).ok()?.parse().ok()?;
        Some(Self { code, payload })
    }
}

#[derive(Debug, Default)]
enum State {
    /// Outside of an OSC sequence.
    #[default]
    Ground,
    /// After ESC.
    Escape,
    /// After `ESC ]`.
    Sequence,
}

/// Finds the OSC sequences in a byte stream. A sequence can start in one
/// call of [`OscScanner::scan`] and end in a later call.
#[derive(Debug)]
pub(crate) struct OscScanner {
    state: State,
    sequence: Vec<u8>,
    too_long: bool,
}

impl OscScanner {
    pub(crate) fn new() -> Self {
        Self {
            state: State::Ground,
            sequence: Vec::with_capacity(MAX_SEQUENCE_BYTES),
            too_long: false,
        }
    }

    /// Calls `found` for each OSC sequence that ends in `bytes`.
    pub(crate) fn scan(&mut self, bytes: &[u8], mut found: impl FnMut(Osc<'_>)) {
        let mut rest = bytes;
        while !rest.is_empty() {
            rest = match self.state {
                State::Ground => self.skip_to_escape(rest),
                State::Escape => self.after_escape(rest),
                State::Sequence => self.collect(rest, &mut found),
            };
        }
    }

    fn skip_to_escape<'b>(&mut self, bytes: &'b [u8]) -> &'b [u8] {
        let Some(index) = memchr::memchr(ESCAPE, bytes) else {
            return &[];
        };
        self.state = State::Escape;
        &bytes[index + 1..]
    }

    fn after_escape<'b>(&mut self, bytes: &'b [u8]) -> &'b [u8] {
        self.state = match bytes[0] {
            b']' => {
                self.sequence.clear();
                self.too_long = false;
                State::Sequence
            }
            // The parser stays after ESC for these bytes.
            0x00..=0x17 | 0x19 | 0x1b..=0x1f | 0x7f..=0xff => State::Escape,
            _ => State::Ground,
        };
        &bytes[1..]
    }

    fn collect<'b>(&mut self, bytes: &'b [u8], found: &mut impl FnMut(Osc<'_>)) -> &'b [u8] {
        for (index, byte) in bytes.iter().enumerate() {
            let next = match *byte {
                BELL | CANCEL | SUBSTITUTE => State::Ground,
                ESCAPE => State::Escape,
                // The parser ignores the other control characters.
                0x00..=0x1f => continue,
                byte => {
                    self.push(byte);
                    continue;
                }
            };
            self.finish(found);
            self.state = next;
            return &bytes[index + 1..];
        }
        &[]
    }

    fn push(&mut self, byte: u8) {
        if self.sequence.len() < MAX_SEQUENCE_BYTES {
            self.sequence.push(byte);
        } else {
            self.too_long = true;
        }
    }

    fn finish(&self, found: &mut impl FnMut(Osc<'_>)) {
        if self.too_long {
            return;
        }
        if let Some(osc) = Osc::parse(&self.sequence) {
            found(osc);
        }
    }
}
