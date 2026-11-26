//! The states of the parser of the VT library (`vte` 0.15), and the byte
//! that moves the parser from one state to the next.
//!
//! Only the states that have different rules for the end of a sequence are
//! here. The four CSI states of the library are one state: they end at the
//! same bytes.

pub(crate) const BELL: u8 = 0x07;
pub(crate) const CANCEL: u8 = 0x18;
pub(crate) const SUBSTITUTE: u8 = 0x1a;
pub(crate) const ESCAPE: u8 = 0x1b;
/// The string terminator as one byte. The library knows it only in the data
/// of a DCS sequence.
const STRING_TERMINATOR: u8 = 0x9c;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// Outside of a sequence.
    Ground,
    /// After ESC.
    Escape,
    /// After ESC and one or more intermediate bytes.
    EscapeIntermediate,
    /// After `ESC [`.
    Csi,
    /// After `ESC P`.
    DcsEntry,
    /// In the parameters of a DCS sequence.
    DcsParam,
    /// In the intermediate bytes of a DCS sequence.
    DcsIntermediate,
    /// In a DCS sequence that is not valid. The library ignores its data.
    DcsIgnore,
    /// In the data of a DCS sequence.
    DcsPassthrough,
    /// After `ESC ]`.
    Osc,
    /// After `ESC X`, `ESC ^` or `ESC _`.
    SosPmApc,
}

impl State {
    /// The state of a parser that is in a sequence, after it takes `byte`.
    pub(crate) fn after(self, byte: u8) -> Self {
        match (self, byte) {
            (_, ESCAPE) => Self::Escape,

            (Self::Escape, 0x20..=0x2f) => Self::EscapeIntermediate,
            (Self::Escape, b'P') => Self::DcsEntry,
            (Self::Escape, b'X' | b'^' | b'_') => Self::SosPmApc,
            (Self::Escape, b'[') => Self::Csi,
            (Self::Escape, b']') => Self::Osc,

            (Self::DcsEntry | Self::DcsParam, 0x20..=0x2f) => Self::DcsIntermediate,
            (Self::DcsEntry, 0x30..=0x3f) => Self::DcsParam,
            (Self::DcsParam, 0x3c..=0x3f) | (Self::DcsIntermediate, 0x30..=0x3f) => Self::DcsIgnore,
            (Self::DcsEntry | Self::DcsParam | Self::DcsIntermediate, 0x40..=0x7e) => {
                Self::DcsPassthrough
            }

            // The end of a sequence.
            (_, CANCEL | SUBSTITUTE)
            | (Self::Escape | Self::EscapeIntermediate, 0x30..=0x7e)
            | (Self::Csi, 0x40..=0x7e)
            | (Self::DcsPassthrough, STRING_TERMINATOR)
            | (Self::Osc, BELL) => Self::Ground,

            (state, _) => state,
        }
    }
}
