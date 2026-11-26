//! The aggregation rules: which status of many is the most urgent.

use std::cmp::Reverse;

use crate::state::State;
use crate::status::Status;

/// How much a status needs the attention of the user. A later variant is
/// more urgent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    /// Nothing to look at: idle work, and a result that the user saw.
    Quiet,
    /// Work runs.
    Working,
    /// Work is complete, and the user did not see the result.
    DoneNotSeen,
    /// Work failed, and the user did not see the failure.
    ErrorNotSeen,
    /// Work waits for the user.
    NeedsYou,
}

impl Status {
    /// How much this status needs the attention of the user.
    #[must_use]
    pub fn urgency(&self) -> Urgency {
        match (self.state, self.seen) {
            (State::NeedsYou(_), _) => Urgency::NeedsYou,
            (State::Error, false) => Urgency::ErrorNotSeen,
            (State::Done, false) => Urgency::DoneNotSeen,
            (State::Working, _) => Urgency::Working,
            (State::Idle, _) | (State::Done | State::Error, true) => Urgency::Quiet,
        }
    }
}

/// The most urgent status of `statuses`, with the key of its item. With the
/// same urgency, the status with the oldest `since` wins: the one that
/// waits longest. With the same `since` too, the first item wins.
///
/// Returns `None` if there is no item.
pub fn most_urgent<'status, Key>(
    statuses: impl IntoIterator<Item = (Key, &'status Status)>,
) -> Option<(Key, &'status Status)> {
    let rank = |status: &Status| (status.urgency(), Reverse(status.since));
    statuses.into_iter().reduce(|winner, candidate| {
        if rank(candidate.1) > rank(winner.1) {
            candidate
        } else {
            winner
        }
    })
}
