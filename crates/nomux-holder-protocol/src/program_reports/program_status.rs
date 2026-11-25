//! One program status record: the last OSC 7501 report for one `id`.

use super::optional_text;
use crate::wire::DecodeError;
use crate::wire::payload::{PayloadReader, write_block, write_text};

const NO_KIND: u8 = 0;
const NO_PROGRESS: u8 = 255;

/// What a program does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramState {
    /// The program waits for the next instruction of the user.
    Idle,
    /// The program runs.
    Working,
    /// The program completed work that the user did not see yet.
    Done,
    /// The program cannot continue until the user does something.
    Blocked,
    /// The program failed and stopped.
    Error,
}

impl ProgramState {
    fn encode(self) -> u8 {
        match self {
            Self::Idle => 1,
            Self::Working => 2,
            Self::Done => 3,
            Self::Blocked => 4,
            Self::Error => 5,
        }
    }

    fn decode(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Idle),
            2 => Some(Self::Working),
            3 => Some(Self::Done),
            4 => Some(Self::Blocked),
            5 => Some(Self::Error),
            _ => None,
        }
    }
}

/// What a blocked program waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedKind {
    /// The user must approve an operation.
    Permission,
    /// The user must type an answer.
    Question,
    /// The user must give a login, a token or a credential.
    Auth,
}

impl BlockedKind {
    fn encode(self) -> u8 {
        match self {
            Self::Permission => 1,
            Self::Question => 2,
            Self::Auth => 3,
        }
    }

    fn decode(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Permission),
            2 => Some(Self::Question),
            3 => Some(Self::Auth),
            _ => None,
        }
    }
}

/// One program status record. It has exactly the keys of the last report
/// for its `id`: the holder adds nothing and reads no meaning into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStatus {
    /// The path of the record: names with `/` between them. Empty for the
    /// root record. `build/test` is below `build`, and `build` is below the
    /// root record.
    pub id: String,
    /// What the program does.
    pub state: ProgramState,
    /// What the program waits for. Only with [`ProgramState::Blocked`].
    pub kind: Option<BlockedKind>,
    /// The percentage of the work that is done, 0 to 100. Only with
    /// [`ProgramState::Working`] and [`ProgramState::Blocked`].
    pub progress: Option<u8>,
    /// The stable name of the program. A record that has none takes the
    /// name of the nearest record above it that has one.
    pub app: Option<String>,
    /// The label of the record: one line without control characters.
    pub title: Option<String>,
    /// What the record does, waits for or completed: one line without
    /// control characters.
    pub message: Option<String>,
    /// The revision of the program reports at the last report for this
    /// record.
    pub revision: u64,
}

impl ProgramStatus {
    pub(super) fn encode(&self, output: &mut Vec<u8>) {
        write_block(output, |block| {
            block.extend_from_slice(&self.revision.to_le_bytes());
            block.push(self.state.encode());
            block.push(self.kind.map_or(NO_KIND, BlockedKind::encode));
            block.push(self.progress.unwrap_or(NO_PROGRESS));
            block.push(0);
            let texts = [
                Some(&self.id),
                self.app.as_ref(),
                self.title.as_ref(),
                self.message.as_ref(),
            ];
            for text in texts {
                write_text(block, text.map_or("", String::as_str));
            }
        });
    }

    /// Returns `None` for a record with a state of a later version.
    pub(super) fn decode(block: &mut PayloadReader<'_>) -> Result<Option<Self>, DecodeError> {
        let revision = block.u64()?;
        let state = ProgramState::decode(block.u8()?);
        let kind = BlockedKind::decode(block.u8()?);
        let progress = Some(block.u8()?).filter(|progress| *progress != NO_PROGRESS);
        block.u8()?;
        let id = block.text()?;
        let app = optional_text(block.text()?);
        let title = optional_text(block.text()?);
        let message = optional_text(block.text()?);
        Ok(state.map(|state| Self {
            id,
            state,
            kind,
            progress,
            app,
            title,
            message,
            revision,
        }))
    }
}
