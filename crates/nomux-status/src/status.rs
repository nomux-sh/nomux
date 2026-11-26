//! The status of one Agent run or of one Child work.

use crate::source::{Source, Sourced};
use crate::state::State;

/// A time: milliseconds after the Unix epoch. This crate reads no clock,
/// thus its caller gives the time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub u64);

/// The status of one Agent run or of one Child work. Each part has its
/// source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// What the work does.
    pub state: State,
    /// The source of the state. `None` if no source reports a state: the
    /// state is then [`State::Idle`].
    pub source: Option<Source>,
    /// When the resolver saw the current state for the first time.
    pub since: Timestamp,
    /// `true` if the user saw the result. It has a meaning only for a state
    /// that [`State::is_result`].
    pub seen: bool,
    /// What the work does, waits for or completed.
    pub message: Option<Sourced<String>>,
    /// The label of the work.
    pub title: Option<Sourced<String>>,
    /// The percentage of the work that is done: 0 to 100.
    pub progress: Option<Sourced<u8>>,
    /// The name of the tool that runs.
    pub tool: Option<Sourced<String>>,
}

impl Status {
    /// The status of work that no source reports.
    #[must_use]
    pub fn unreported(now: Timestamp) -> Self {
        Self {
            state: State::Idle,
            source: None,
            since: now,
            seen: false,
            message: None,
            title: None,
            progress: None,
            tool: None,
        }
    }

    /// Records that the user saw the result. Returns `true` if the status
    /// changed.
    pub(crate) fn mark_seen(&mut self) -> bool {
        let changes = self.state.is_result() && !self.seen;
        self.seen |= changes;
        changes
    }
}
