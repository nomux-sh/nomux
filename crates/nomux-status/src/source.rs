//! Where a part of a status comes from.

/// A source of a status. The variants are in the sequence in which the
/// sources decide: a source decides before each source after it.
///
/// A later version adds sources. The resolver needs no change for a new
/// source: it uses only the sequence. A new variant goes to its place in
/// the sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// An adapter extension reported the status from the events of the
    /// agent tool, for example from its hooks.
    Adapter,
    /// A program reported the status in the terminal stream of the session:
    /// the Program Status Protocol (OSC 7501).
    ProgramStatus,
    /// A program in the session reported the status to the daemon.
    SelfReport,
    /// The session lost its process while work ran or waited for the user.
    SessionRestore,
    /// A guess from the prompt marks of the shell (OSC 133).
    PromptMarks,
    /// A guess from the foreground process group of the terminal.
    ForegroundProgram,
}

impl Source {
    /// `true` if the daemon infers the status and no program reported it.
    /// A client must not show a guess as a fact.
    #[must_use]
    pub fn is_guess(self) -> bool {
        matches!(self, Self::PromptMarks | Self::ForegroundProgram)
    }
}

/// A value and the source that gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sourced<T> {
    /// The value.
    pub value: T,
    /// The source of the value.
    pub source: Source,
}
