//! The sizes and the times of the journal writer.

use std::time::Duration;

/// When the journal writes, and how much it keeps. `docs/holder.md` has the
/// measurements behind the values of [`JournalLimits::DEFAULT`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct JournalLimits {
    /// The output bytes that the journal keeps in memory before it writes
    /// them as one record.
    pub(crate) block_length: usize,
    /// The time that an output byte waits in memory at most.
    pub(crate) flush_delay: Duration,
    /// The output bytes between two checkpoints.
    pub(crate) checkpoint_interval: u64,
    /// A segment file that has this number of bytes gets no more record.
    pub(crate) segment_length: u64,
    /// The bytes of all segment files of the session. The journal removes
    /// the oldest segments to stay below this number.
    pub(crate) total_length: u64,
}

impl JournalLimits {
    /// The limits of each session. A later version gets the total length
    /// from the configuration of the daemon, as an option of the holder.
    pub(crate) const DEFAULT: Self = Self {
        block_length: 64 * 1024,
        flush_delay: Duration::from_secs(1),
        checkpoint_interval: 1024 * 1024,
        segment_length: 2 * 1024 * 1024,
        total_length: 32 * 1024 * 1024,
    };
}
