//! Finds an extension that writes too much to its standard error.

use std::time::Duration;

use tokio::time::Instant;

/// Counts the bytes of one window of time. The meter has no timer: a new
/// window starts with the first bytes after the end of the window before.
pub(super) struct FloodMeter {
    limit: usize,
    window: Duration,
    window_start: Instant,
    bytes_in_window: usize,
}

impl FloodMeter {
    pub(super) fn new(limit: usize, window: Duration) -> Self {
        Self {
            limit,
            window,
            window_start: Instant::now(),
            bytes_in_window: 0,
        }
    }

    /// Records bytes that arrived at `now`. Returns `true` if the window
    /// has more bytes than the limit.
    pub(super) fn is_flood(&mut self, bytes: usize, now: Instant) -> bool {
        if now.duration_since(self.window_start) >= self.window {
            self.window_start = now;
            self.bytes_in_window = 0;
        }
        self.bytes_in_window = self.bytes_in_window.saturating_add(bytes);
        self.bytes_in_window > self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_bytes_of_one_window_count() {
        let window = Duration::from_secs(10);
        let start = Instant::now();
        let mut meter = FloodMeter::new(100, window);

        assert!(!meter.is_flood(100, start));
        assert!(meter.is_flood(1, start + window / 2));
        assert!(!meter.is_flood(100, start + window * 2));
    }
}
