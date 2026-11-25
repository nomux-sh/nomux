//! When the host asks an extension for its health, and when the answer is
//! late.

use std::time::Duration;

use tokio::time::Instant;

/// What the host does when the deadline of the health check arrives.
pub(super) enum HealthStep {
    /// Send a health request.
    Ask,
    /// The extension did not answer the last health request in time.
    Overdue,
}

enum State {
    /// The subsequent health request goes out at this time.
    Resting { ask_at: Instant },
    /// A health request has no response yet.
    Asked { request_id: u64, answer_by: Instant },
}

/// The schedule of the health requests of one process. The extension
/// answers a health request also while a call runs, thus a late answer
/// means that the process does not respond.
pub(super) struct HealthCheck {
    interval: Duration,
    limit: Duration,
    state: State,
}

impl HealthCheck {
    pub(super) fn new(interval: Duration, limit: Duration) -> Self {
        Self {
            interval,
            limit,
            state: State::Resting {
                ask_at: Instant::now() + interval,
            },
        }
    }

    pub(super) fn deadline(&self) -> Instant {
        match self.state {
            State::Resting { ask_at } => ask_at,
            State::Asked { answer_by, .. } => answer_by,
        }
    }

    /// What the host does at the deadline.
    pub(super) fn step(&self) -> HealthStep {
        match self.state {
            State::Resting { .. } => HealthStep::Ask,
            State::Asked { .. } => HealthStep::Overdue,
        }
    }

    pub(super) fn asked(&mut self, request_id: u64) {
        self.state = State::Asked {
            request_id,
            answer_by: Instant::now() + self.limit,
        };
    }

    /// Records a response. Returns `true` if it answers the health request.
    pub(super) fn answered(&mut self, response_id: u64) -> bool {
        if !matches!(self.state, State::Asked { request_id, .. } if request_id == response_id) {
            return false;
        }
        self.state = State::Resting {
            ask_at: Instant::now() + self.interval,
        };
        true
    }
}
