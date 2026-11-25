//! When the host starts an extension again after a failure, and when it
//! stops trying.

use std::time::{Duration, Instant};

use crate::config::Limits;

/// What the host does after a failure.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    /// The subsequent call can start the extension at this time.
    RetryAt(Instant),
    /// The extension failed too many times. The host disables it.
    GiveUp,
}

/// Counts the failures of one extension that came in sequence. The pause
/// before a new start becomes two times longer after each failure, thus an
/// extension that always fails costs almost nothing.
#[derive(Debug, Default)]
pub(super) struct RestartPolicy {
    failures: u32,
    retry_at: Option<Instant>,
}

impl RestartPolicy {
    pub(super) fn record_failure(&mut self, now: Instant, limits: &Limits) -> Verdict {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= limits.failures_before_disable {
            self.retry_at = None;
            return Verdict::GiveUp;
        }
        let doublings = self.failures - 1;
        let factor = 1_u32.checked_shl(doublings).unwrap_or(u32::MAX);
        let delay = limits
            .restart_delay_first
            .saturating_mul(factor)
            .min(limits.restart_delay_max);
        let retry_at = now + delay;
        self.retry_at = Some(retry_at);
        Verdict::RetryAt(retry_at)
    }

    /// The time that is left of the pause after the last failure.
    pub(super) fn pause_left(&self, now: Instant) -> Option<Duration> {
        let left = self.retry_at?.saturating_duration_since(now);
        (!left.is_zero()).then_some(left)
    }

    /// Forgets the failures: the extension stopped without a failure, or
    /// the user enabled it again.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Limits {
        Limits {
            restart_delay_first: Duration::from_secs(1),
            restart_delay_max: Duration::from_secs(5),
            failures_before_disable: 6,
            ..Limits::default()
        }
    }

    #[test]
    fn the_pause_doubles_to_the_maximum_and_then_the_host_gives_up() {
        let now = Instant::now();
        let mut policy = RestartPolicy::default();

        let verdicts: Vec<Verdict> = (0..6)
            .map(|_| policy.record_failure(now, &limits()))
            .collect();

        let pauses = [1, 2, 4, 5, 5].map(|seconds| now + Duration::from_secs(seconds));
        let mut expected: Vec<Verdict> = pauses.into_iter().map(Verdict::RetryAt).collect();
        expected.push(Verdict::GiveUp);
        assert_eq!(verdicts, expected);
    }

    #[test]
    fn a_reset_starts_the_sequence_again() {
        let now = Instant::now();
        let mut policy = RestartPolicy::default();
        policy.record_failure(now, &limits());
        policy.record_failure(now, &limits());
        assert_eq!(policy.pause_left(now), Some(Duration::from_secs(2)));
        assert_eq!(policy.pause_left(now + Duration::from_secs(2)), None);

        policy.reset();

        assert_eq!(policy.pause_left(now), None);
        let first_again = policy.record_failure(now, &limits());
        assert_eq!(first_again, Verdict::RetryAt(now + Duration::from_secs(1)));
    }
}
