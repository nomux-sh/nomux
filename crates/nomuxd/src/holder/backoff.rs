//! The pauses between the attempts to connect to a holder again.

use std::time::Duration;

const FIRST_DELAY: Duration = Duration::from_millis(100);
const MAX_DELAY: Duration = Duration::from_secs(5);

/// Gives a pause that becomes two times longer after each attempt, up to a
/// maximum. Thus a holder that does not answer costs almost no CPU time.
#[derive(Debug)]
pub(super) struct Backoff {
    next: Duration,
}

impl Backoff {
    pub(super) fn new() -> Self {
        Self { next: FIRST_DELAY }
    }

    /// The pause before the next attempt.
    pub(super) fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = (delay * 2).min(MAX_DELAY);
        delay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pause_grows_to_the_maximum_and_stays_there() {
        let mut backoff = Backoff::new();
        let delays: Vec<Duration> = (0..8).map(|_| backoff.next_delay()).collect();

        let milliseconds: Vec<u128> = delays.iter().map(Duration::as_millis).collect();
        assert_eq!(milliseconds, [100, 200, 400, 800, 1600, 3200, 5000, 5000]);
    }
}
