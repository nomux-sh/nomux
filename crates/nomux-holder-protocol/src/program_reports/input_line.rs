//! The input line state: where the shell of the session is between a prompt
//! and a command, from its prompt marks (OSC 133).

use crate::wire::DecodeError;
use crate::wire::payload::PayloadReader;

const INPUT_AFTER_MARK: u8 = 1 << 0;
const EXIT_CODE_PRESENT: u8 = 1 << 1;

/// A prompt mark of a shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMark {
    /// OSC 133 A: the shell started to write a prompt.
    PromptStart,
    /// OSC 133 B: the prompt is complete, and the input line of the user
    /// started.
    InputStart,
    /// OSC 133 C: the shell took the input line, and a command runs.
    CommandStart,
    /// OSC 133 D: the command ended.
    CommandEnd,
}

impl PromptMark {
    fn encode(self) -> u8 {
        match self {
            Self::PromptStart => 1,
            Self::InputStart => 2,
            Self::CommandStart => 3,
            Self::CommandEnd => 4,
        }
    }

    fn decode(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::PromptStart),
            2 => Some(Self::InputStart),
            3 => Some(Self::CommandStart),
            4 => Some(Self::CommandEnd),
            _ => None,
        }
    }
}

/// The input line state of a session.
///
/// The input line is empty when `mark` is [`PromptMark::InputStart`] and
/// `input_after_mark` is `false`. A command runs when `mark` is
/// [`PromptMark::CommandStart`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputLine {
    /// The last prompt mark. `None` if the shell sent no mark: the state is
    /// then not known.
    pub mark: Option<PromptMark>,
    /// The exit code of the last [`PromptMark::CommandEnd`] mark. `None` if
    /// that mark had no exit code, or if there was no such mark.
    pub exit_code: Option<i32>,
    /// `true` if the holder wrote input to the terminal after the last mark.
    pub input_after_mark: bool,
    /// The revision of the program reports at the last change of this state.
    pub revision: u64,
}

impl InputLine {
    pub(super) fn encode(&self, output: &mut Vec<u8>) {
        let mut flags = 0;
        if self.input_after_mark {
            flags |= INPUT_AFTER_MARK;
        }
        if self.exit_code.is_some() {
            flags |= EXIT_CODE_PRESENT;
        }
        output.push(self.mark.map_or(0, PromptMark::encode));
        output.push(flags);
        output.extend_from_slice(&[0; 2]);
        output.extend_from_slice(&self.exit_code.unwrap_or(0).to_le_bytes());
        output.extend_from_slice(&self.revision.to_le_bytes());
    }

    pub(super) fn decode(header: &mut PayloadReader<'_>) -> Result<Self, DecodeError> {
        let mark = PromptMark::decode(header.u8()?);
        let flags = header.u8()?;
        header.bytes(2)?;
        let exit_code = header.i32()?;
        Ok(Self {
            mark,
            exit_code: (flags & EXIT_CODE_PRESENT != 0).then_some(exit_code),
            input_after_mark: flags & INPUT_AFTER_MARK != 0,
            revision: header.u64()?,
        })
    }
}
