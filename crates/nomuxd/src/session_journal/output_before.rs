//! The output of a session before a position: one part of its scrollback.

use std::io;

use nomux_holder_protocol::journal::{FIRST_EPOCH, MAX_RANGE_LENGTH};

use super::SessionJournal;

/// Output bytes of one epoch of a session.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CapturedOutput {
    pub(crate) epoch: u32,
    /// The position of the first byte of `bytes`. With no bytes, the
    /// position before which the session has no output.
    pub(crate) position: u64,
    pub(crate) bytes: Vec<u8>,
}

impl SessionJournal {
    /// The last output bytes of `epoch` before the position `before` that
    /// the session has without a gap: `max_length` bytes at most. Without
    /// `epoch`, the newest epoch. Without `before`, the end of the output.
    ///
    /// The bytes end before `before` if the journal does not have the bytes
    /// up to that position. No bytes: the session has no output of the
    /// epoch before `before`.
    pub(crate) async fn output_before(
        &self,
        epoch: Option<u32>,
        before: Option<u64>,
        max_length: usize,
    ) -> io::Result<CapturedOutput> {
        let epoch = match epoch {
            Some(epoch) => epoch,
            None => self.newest_epoch()?.unwrap_or(FIRST_EPOCH),
        };
        let max_length = max_length.clamp(1, MAX_RANGE_LENGTH) as u64;
        let nothing = |position| CapturedOutput {
            epoch,
            position,
            bytes: Vec::new(),
        };
        let Some(oldest) = self.read(epoch, 0, 1).await?.map(|range| range.position) else {
            return Ok(nothing(0));
        };
        let end = self.end(epoch)?;
        let mut end = before.map_or(end, |before| before.min(end));
        // A window without output is a gap in the journal: the output
        // before the gap is in the windows before it.
        while end > oldest {
            let start = end.saturating_sub(max_length).max(oldest);
            let (position, bytes) = self.last_run(epoch, start, end).await?;
            if !bytes.is_empty() {
                return Ok(CapturedOutput {
                    epoch,
                    position,
                    bytes,
                });
            }
            end = start;
        }
        Ok(nothing(end))
    }

    /// The last bytes without a gap that the journal has from `start` to
    /// `end`, and the position of the first one.
    async fn last_run(&self, epoch: u32, start: u64, end: u64) -> io::Result<(u64, Vec<u8>)> {
        let mut position = start;
        let mut bytes: Vec<u8> = Vec::new();
        let mut next = start;
        while next < end {
            let wanted = usize::try_from(end - next).unwrap_or(usize::MAX);
            let Some(range) = self.read(epoch, next, wanted).await? else {
                break;
            };
            if range.position >= end {
                break;
            }
            if range.position > next {
                // The bytes after a gap are newer than the bytes before it.
                bytes.clear();
                position = range.position;
            }
            let wanted = usize::try_from(end - range.position).unwrap_or(usize::MAX);
            bytes.extend(range.bytes.iter().take(wanted));
            next = range.position + range.bytes.len() as u64;
        }
        Ok((position, bytes))
    }
}
