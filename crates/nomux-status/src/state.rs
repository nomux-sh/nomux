//! What an Agent run or a Child work does.

/// What the user must do for work that waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeedsYouKind {
    /// The user must approve an operation.
    Permission,
    /// The user must type an answer.
    Question,
    /// The user must give a login, a token or a credential.
    Auth,
}

/// The state of an Agent run or of a Child work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing runs, and nothing waits for the user.
    Idle,
    /// Work runs.
    Working,
    /// Work is complete.
    Done,
    /// The work cannot continue until the user does something. The kind is
    /// `None` if the source does not say what.
    NeedsYou(Option<NeedsYouKind>),
    /// The work failed and stopped.
    Error,
}

impl State {
    /// `true` if the two values are the same state. The kind of
    /// [`State::NeedsYou`] is not compared.
    #[must_use]
    pub fn is_same_state(self, other: Self) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }

    /// `true` if the state is a result that the user can see or not see:
    /// [`State::Done`] and [`State::Error`].
    #[must_use]
    pub fn is_result(self) -> bool {
        matches!(self, Self::Done | Self::Error)
    }

    /// `true` if the state needs a process: work that runs, or work that
    /// waits for the user.
    #[must_use]
    pub fn needs_process(self) -> bool {
        matches!(self, Self::Working | Self::NeedsYou(_))
    }
}
