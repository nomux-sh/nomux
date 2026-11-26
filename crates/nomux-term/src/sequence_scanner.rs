//! Follows the output of a program through the states of the parser of the
//! VT library.
//!
//! The VT library gives no access to the state of its parser, and it has no
//! hook for the OSC codes that it does not know. Thus this module uses the
//! rules of that parser for the start and the end of each sequence: the two
//! always see the same sequences. The scanner gives three results:
//!
//! - Each OSC sequence that ends.
//! - The end of each erase in display above the cursor (ED 1), which the
//!   terminal corrects after the library.
//! - The length of the incomplete tail: the bytes at the end of the output
//!   that are a sequence or a UTF-8 character that is not complete.

mod state;
#[cfg(test)]
mod tests;

use self::state::{BELL, CANCEL, ESCAPE, SUBSTITUTE, State};
use crate::osc::Osc;

/// The output of a program is not trusted. An OSC sequence that is longer
/// than this limit is ignored, thus the scanner uses a fixed quantity of
/// memory.
const MAX_OSC_BYTES: usize = 4096;

/// The largest number of bytes of one UTF-8 character.
const MAX_CHARACTER_BYTES: usize = 4;

/// The scanner keeps this number of parameter bytes of a CSI sequence that
/// the end of the output or a control character divides. Such an erase in
/// display with more parameter bytes is not reported.
const MAX_CSI_PARAMETER_BYTES: usize = 8;

/// A result of [`SequenceScanner::scan`].
#[derive(Debug)]
pub(crate) enum Found<'a> {
    /// An OSC sequence that ended.
    Osc(Osc<'a>),
    /// An erase in display above the cursor (`CSI 1 J`) ended at this
    /// offset of the bytes of the call.
    EraseAbove { end: usize },
}

/// Finds the sequences in a byte stream. A sequence can start in one call of
/// [`SequenceScanner::scan`] and end in a later call.
#[derive(Debug)]
pub(crate) struct SequenceScanner {
    state: State,
    /// In a sequence: the number of bytes from its escape character to the
    /// end of the output.
    sequence_length: usize,
    /// The start of a UTF-8 character that the end of the output cut.
    partial_character: [u8; MAX_CHARACTER_BYTES],
    partial_character_length: usize,
    osc: Vec<u8>,
    osc_too_long: bool,
    /// The first parameter bytes of the CSI sequence in progress, if the
    /// sequence was divided.
    csi_parameters: [u8; MAX_CSI_PARAMETER_BYTES],
    /// The number of parameter bytes of the CSI sequence in progress, also
    /// the bytes that `csi_parameters` does not keep.
    csi_parameters_length: usize,
    erase_above_ended: bool,
}

impl SequenceScanner {
    pub(crate) fn new() -> Self {
        Self {
            state: State::Ground,
            sequence_length: 0,
            partial_character: [0; MAX_CHARACTER_BYTES],
            partial_character_length: 0,
            osc: Vec::with_capacity(MAX_OSC_BYTES),
            osc_too_long: false,
            csi_parameters: [0; MAX_CSI_PARAMETER_BYTES],
            csi_parameters_length: 0,
            erase_above_ended: false,
        }
    }

    /// The number of bytes at the end of the output that are a sequence or a
    /// UTF-8 character that is not complete.
    pub(crate) fn incomplete_tail(&self) -> usize {
        match self.state {
            State::Ground => self.partial_character_length,
            _ => self.sequence_length,
        }
    }

    /// Takes the next bytes of the output. Calls `found` for each OSC
    /// sequence and each erase in display above the cursor that ends in
    /// `bytes`.
    pub(crate) fn scan(&mut self, bytes: &[u8], mut found: impl FnMut(Found<'_>)) {
        let mut rest = bytes;
        if self.partial_character_length != 0 {
            rest = self.continue_partial_character(rest);
        }
        while !rest.is_empty() {
            rest = match self.state {
                State::Ground => self.text(rest),
                State::Osc => self.osc_data(rest, &mut found),
                State::Csi => self.csi(rest),
                _ => self.sequence(rest),
            };
            if self.erase_above_ended {
                self.erase_above_ended = false;
                found(Found::EraseAbove {
                    end: bytes.len() - rest.len(),
                });
            }
        }
    }

    /// Text goes to the next escape character. If there is none, the end of
    /// the output can cut the last character.
    fn text<'b>(&mut self, bytes: &'b [u8]) -> &'b [u8] {
        if let Some(index) = memchr::memchr(ESCAPE, bytes) {
            self.take(ESCAPE);
            return &bytes[index + 1..];
        }
        let partial = partial_character(bytes);
        self.partial_character[..partial.len()].copy_from_slice(partial);
        self.partial_character_length = partial.len();
        &[]
    }

    /// Gives the first bytes of `bytes` to the character that the previous
    /// call cut, with the steps of the library. The library puts 4 bytes
    /// together and continues after the bytes that are valid or not valid at
    /// the start: that can be more than the one character.
    fn continue_partial_character<'b>(&mut self, bytes: &'b [u8]) -> &'b [u8] {
        let kept = self.partial_character_length;
        let added = bytes.len().min(MAX_CHARACTER_BYTES - kept);
        self.partial_character[kept..kept + added].copy_from_slice(&bytes[..added]);
        let together = &self.partial_character[..kept + added];
        let taken = match std::str::from_utf8(together) {
            Ok(text) => text.chars().next().map_or(0, char::len_utf8),
            Err(error) if error.valid_up_to() > 0 => error.valid_up_to(),
            Err(error) => {
                let Some(invalid) = error.error_len() else {
                    self.partial_character_length = kept + added;
                    return &[];
                };
                invalid
            }
        };
        self.partial_character_length = 0;
        &bytes[taken.saturating_sub(kept)..]
    }

    /// The bytes of a CSI sequence. The parameter bytes and the intermediate
    /// bytes change no state, and they are most of the bytes of colored
    /// output: this function passes them without a look at the state.
    fn csi<'b>(&mut self, bytes: &'b [u8]) -> &'b [u8] {
        let is_parameter = |byte: &&u8| (0x20..=0x3f).contains(*byte);
        let parameters = bytes.iter().take_while(is_parameter).count();
        self.sequence_length += parameters;
        let Some(byte) = bytes.get(parameters) else {
            self.keep_csi_parameters(&bytes[..parameters]);
            return &[];
        };
        if !(0x40..=0x7e).contains(byte) {
            self.keep_csi_parameters(&bytes[..parameters]);
        } else if *byte == b'J' {
            self.erase_above_ended = self.ends_erase_above(&bytes[..parameters]);
        }
        self.take(*byte);
        &bytes[parameters + 1..]
    }

    /// Keeps the parameter bytes of a CSI sequence that continues after the
    /// end of the output or after a control character.
    fn keep_csi_parameters(&mut self, parameters: &[u8]) {
        let kept = self.csi_parameters_length.min(MAX_CSI_PARAMETER_BYTES);
        let added = parameters.len().min(MAX_CSI_PARAMETER_BYTES - kept);
        self.csi_parameters[kept..kept + added].copy_from_slice(&parameters[..added]);
        self.csi_parameters_length += parameters.len();
    }

    /// `true` if the `J` sequence with the `last` parameter bytes is an
    /// erase in display above the cursor.
    fn ends_erase_above(&mut self, last: &[u8]) -> bool {
        if self.csi_parameters_length == 0 {
            return is_erase_above(last);
        }
        self.keep_csi_parameters(last);
        self.csi_parameters_length <= MAX_CSI_PARAMETER_BYTES
            && is_erase_above(&self.csi_parameters[..self.csi_parameters_length])
    }

    /// The bytes of a sequence that is not a CSI sequence or an OSC
    /// sequence, up to the byte that changes the state.
    fn sequence<'b>(&mut self, bytes: &'b [u8]) -> &'b [u8] {
        for (index, byte) in bytes.iter().enumerate() {
            let before = self.state;
            self.take(*byte);
            if self.state != before {
                return &bytes[index + 1..];
            }
        }
        &[]
    }

    /// The data of an OSC sequence.
    fn osc_data<'b>(&mut self, bytes: &'b [u8], found: &mut impl FnMut(Found<'_>)) -> &'b [u8] {
        for (index, byte) in bytes.iter().enumerate() {
            match *byte {
                BELL | CANCEL | SUBSTITUTE | ESCAPE => {
                    self.finish_osc(found);
                    self.take(*byte);
                    return &bytes[index + 1..];
                }
                // The parser ignores the other control characters.
                0x00..=0x1f => {}
                byte => self.push_osc(byte),
            }
            self.sequence_length += 1;
        }
        &[]
    }

    /// Moves to the state after `byte`. Each escape character starts a
    /// sequence, also in a sequence.
    fn take(&mut self, byte: u8) {
        let before = self.state;
        self.state = if before == State::Ground {
            State::Escape
        } else {
            before.after(byte)
        };
        self.sequence_length = if byte == ESCAPE {
            1
        } else {
            self.sequence_length + 1
        };
        if self.state == State::Osc && before != State::Osc {
            self.osc.clear();
            self.osc_too_long = false;
        }
        if self.state == State::Csi && before != State::Csi {
            self.csi_parameters_length = 0;
        }
    }

    fn push_osc(&mut self, byte: u8) {
        if self.osc.len() < MAX_OSC_BYTES {
            self.osc.push(byte);
        } else {
            self.osc_too_long = true;
        }
    }

    fn finish_osc(&self, found: &mut impl FnMut(Found<'_>)) {
        if self.osc_too_long {
            return;
        }
        if let Some(osc) = Osc::parse(&self.osc) {
            found(Found::Osc(osc));
        }
    }
}

/// `true` if the library reads the parameters of a `J` sequence as "above
/// the cursor": the first parameter is 1, and the sequence has no private
/// marker and no intermediate byte.
fn is_erase_above(parameters: &[u8]) -> bool {
    if !parameters
        .iter()
        .all(|byte| matches!(byte, b'0'..=b'9' | b';' | b':'))
    {
        return false;
    }
    let mut groups = parameters.split(|byte| matches!(byte, b';' | b':'));
    let first = groups.next().unwrap_or_default();
    first.iter().skip_while(|digit| **digit == b'0').eq(b"1")
}

/// The bytes at the end of `text` that are the start of one UTF-8 character
/// and are valid up to the end. Empty if the last character is complete or
/// not valid.
fn partial_character(text: &[u8]) -> &[u8] {
    let longest = text.len().min(MAX_CHARACTER_BYTES - 1);
    let end = &text[text.len() - longest..];
    let Some(first_byte) = end.iter().rposition(|byte| !is_continuation(*byte)) else {
        return &[];
    };
    let character = &end[first_byte..];
    match std::str::from_utf8(character) {
        Err(error) if error.error_len().is_none() => character,
        _ => &[],
    }
}

fn is_continuation(byte: u8) -> bool {
    (0x80..=0xbf).contains(&byte)
}
