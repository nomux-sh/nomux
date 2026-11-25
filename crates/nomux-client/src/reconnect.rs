//! The connection policy: how often and how fast a client connects again.
//!
//! The number of attempts has a limit, and the pause between two attempts
//! becomes longer. Thus a host that is not reachable gets a small number of
//! connections, and a host that comes back gets one.

use std::time::Duration;

const FIRST_PAUSE: Duration = Duration::from_millis(250);
const LONGEST_PAUSE: Duration = Duration::from_secs(8);
/// With these values, a client tries for approximately 48 seconds.
const ATTEMPTS: u32 = 10;

/// The pauses of one sequence of attempts.
#[derive(Debug, Default)]
pub(crate) struct Backoff {
    failed_attempts: u32,
}

impl Backoff {
    /// The pause before the next attempt: 250 ms after the first failure,
    /// then two times the pause before, up to 8 seconds. Returns `None`
    /// when no attempt is left.
    pub(crate) fn next_pause(&mut self) -> Option<Duration> {
        if self.failed_attempts >= ATTEMPTS {
            return None;
        }
        let pause = FIRST_PAUSE
            .saturating_mul(1 << self.failed_attempts.min(16))
            .min(LONGEST_PAUSE);
        self.failed_attempts += 1;
        Some(pause)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pauses_grow_to_a_limit_and_the_attempts_end() {
        let mut backoff = Backoff::default();
        let pauses: Vec<u128> = std::iter::from_fn(|| backoff.next_pause())
            .map(|pause| pause.as_millis())
            .collect();

        assert_eq!(
            pauses,
            [250, 500, 1000, 2000, 4000, 8000, 8000, 8000, 8000, 8000]
        );
        assert_eq!(backoff.next_pause(), None);
    }
}
