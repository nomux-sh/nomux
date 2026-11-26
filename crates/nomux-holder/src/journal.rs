//! The journal of the session: the output on disk, with a checkpoint of the
//! screen at intervals. The format of the files is in
//! `nomux_holder_protocol::journal`. `docs/holder.md` has the rules of this
//! writer.
//!
//! The journal gets each byte that the ring buffer and the screen state
//! get, in the same step. It keeps the last bytes in memory and writes them
//! as one record when they are a full block, or when the flush delay
//! passed. It never calls `fsync` while the command runs.
//!
//! A record ends at the start of the escape sequence or the UTF-8 character
//! that the output ends in. Thus the position after a record is a position
//! that a checkpoint can have.

mod limits;
mod retention;
mod segment_writer;

use std::io;
use std::path::PathBuf;
use std::time::Instant;

use nomux_holder_protocol::journal::{JournalReader, RecordEncoder};
use nomux_holder_protocol::redraw;
use nomux_holder_protocol::wire::WindowSize;
use nomux_term::Terminal;

pub(crate) use self::limits::JournalLimits;
use self::segment_writer::SegmentWriter;

/// One read from the terminal: `session::OUTPUT_CHUNK`. The block in memory
/// can be longer than a full block by the bytes of the reads that an
/// incomplete sequence holds back.
const BLOCK_RESERVE: usize = 2 * 4096;

/// A checkpoint is not written more frequently than its size permits: the
/// output between two checkpoints is this number of times the stored size of
/// the first one, or more. Thus the checkpoints of a very large screen stay
/// a small part of the journal.
const OUTPUT_FOR_EACH_CHECKPOINT_BYTE: u64 = 16;

pub(crate) struct Journal {
    limits: JournalLimits,
    directory: PathBuf,
    epoch: u32,
    /// `None` after a write failed: the journal then gets no more byte.
    writer: Option<SegmentWriter>,
    encoder: RecordEncoder,
    /// The output bytes that are not in a file.
    block: Vec<u8>,
    /// The position of the first byte of `block`: the end of the output in
    /// the files.
    block_position: u64,
    /// When the bytes of `block` must go to a file.
    flush_deadline: Option<Instant>,
    checkpoint_due: bool,
    /// The output bytes in the files after the last checkpoint.
    output_after_checkpoint: u64,
    /// The output bytes that must follow the last checkpoint before the
    /// next one.
    checkpoint_interval: u64,
    /// The window has a size that no checkpoint has: at the start, and
    /// after a change of the size.
    window_changed: bool,
}

impl Journal {
    /// Starts the journal of `epoch` in `directory`, which must exist. The
    /// first byte of the journal is the output byte at the position `start`:
    /// 0 for a holder that started with its session, and the write position
    /// at the assignment for a pooled holder.
    pub(crate) fn create(
        directory: PathBuf,
        epoch: u32,
        start: u64,
        limits: JournalLimits,
    ) -> io::Result<Self> {
        let writer = SegmentWriter::create(directory.clone(), epoch, start, &limits)?;
        Ok(Self {
            limits,
            directory,
            epoch,
            writer: Some(writer),
            encoder: RecordEncoder::new(),
            block: Vec::with_capacity(limits.block_length + BLOCK_RESERVE),
            block_position: start,
            flush_deadline: None,
            checkpoint_due: false,
            output_after_checkpoint: 0,
            checkpoint_interval: limits.checkpoint_interval,
            // The first record of the journal is a checkpoint at `start`.
            window_changed: true,
        })
    }

    /// Records that the window changed its size. The checkpoint with the
    /// new size is written before the next output byte, thus many changes
    /// without output give one checkpoint.
    pub(crate) fn window_resized(&mut self) {
        self.window_changed = true;
    }

    /// Call this before the screen gets more output. `screen` and `size`
    /// are the screen state and the window of the session.
    pub(crate) fn before_output(&mut self, screen: &mut Terminal, size: WindowSize) {
        if self.window_changed {
            self.window_changed = false;
            self.checkpoint_due = true;
            self.flush(screen, size);
        }
    }

    /// Writes each byte that waits and then a checkpoint of `screen`, for
    /// the daemon before it ends the command.
    pub(crate) fn checkpoint_now(&mut self, screen: &mut Terminal, size: WindowSize) {
        self.checkpoint_due = true;
        self.flush(screen, size);
    }

    /// Takes output bytes that the screen also got.
    pub(crate) fn append(&mut self, output: &[u8]) {
        if self.writer.is_none() {
            return;
        }
        self.block.extend_from_slice(output);
        if self.flush_deadline.is_none() {
            self.flush_deadline = Some(Instant::now() + self.limits.flush_delay);
        }
    }

    /// Call this after the screen got the bytes of [`append`](Self::append).
    /// Writes the block if it is full.
    pub(crate) fn output_written(&mut self, screen: &mut Terminal, size: WindowSize) {
        if self.block.len() >= self.limits.block_length {
            self.flush(screen, size);
        }
    }

    /// The time at which the event loop must call [`flush`](Self::flush).
    /// `None` if the journal has no byte that waits.
    pub(crate) fn flush_deadline(&self) -> Option<Instant> {
        self.flush_deadline
    }

    /// Writes the bytes of the block to the open segment, without the
    /// sequence that is not complete at their end. Writes a checkpoint
    /// after them if one is due.
    pub(crate) fn flush(&mut self, screen: &mut Terminal, size: WindowSize) {
        self.flush_deadline = None;
        let result = self.write_block(screen, size);
        self.stop_after_error(result);
    }

    /// Call this after the last output byte of the session. Writes each
    /// byte that waits, and asks the system to write the open segment to
    /// the disk.
    pub(crate) fn finish(&mut self, screen: &mut Terminal, size: WindowSize) {
        self.flush(screen, size);
        // No byte follows, thus the incomplete sequence stays incomplete.
        let result = self
            .write_output(self.block.len())
            .and_then(|()| self.writer.as_ref().map_or(Ok(()), SegmentWriter::sync));
        self.stop_after_error(result);
    }

    /// Reads output bytes from the position `from` on: `max_length` bytes at
    /// most. Returns the position of the first byte and the bytes.
    ///
    /// The position is after `from` if the journal does not have the byte
    /// at `from`. The bytes are empty if the journal has no byte at `from`
    /// or after it: the position is then the end of the journal.
    pub(crate) fn read(&self, from: u64, max_length: usize) -> io::Result<(u64, Vec<u8>)> {
        if from < self.block_position {
            let reader = JournalReader::open(&self.directory)?;
            if let Some(range) = reader.read(self.epoch, from, max_length)? {
                return Ok((range.position, range.bytes));
            }
        }
        let skipped = usize::try_from(from.saturating_sub(self.block_position))
            .map_or(self.block.len(), |skipped| skipped.min(self.block.len()));
        let bytes = self.block[skipped..]
            .iter()
            .take(max_length)
            .copied()
            .collect();
        Ok((self.block_position + skipped as u64, bytes))
    }

    fn write_block(&mut self, screen: &mut Terminal, size: WindowSize) -> io::Result<()> {
        let incomplete_tail = screen.incomplete_tail();
        let complete = self.block.len().saturating_sub(incomplete_tail);
        // A sequence that is longer than a block cannot wait in memory.
        let is_one_long_sequence = complete == 0 && self.block.len() >= self.limits.block_length;
        self.write_output(if is_one_long_sequence {
            self.block.len()
        } else {
            complete
        })?;
        // The screen has the effect of each byte before the incomplete
        // sequence. If that sequence starts in a file, the end of the files
        // is not a position that the screen corresponds to.
        let screen_is_at_file_end = self.block.len() == incomplete_tail;
        let is_due =
            self.checkpoint_due || self.output_after_checkpoint >= self.checkpoint_interval;
        if screen_is_at_file_end && is_due {
            self.write_checkpoint(screen, size)?;
        }
        Ok(())
    }

    /// Writes the first `length` bytes of the block as one output record.
    fn write_output(&mut self, length: usize) -> io::Result<()> {
        let Some(writer) = &mut self.writer else {
            return Ok(());
        };
        if length == 0 {
            return Ok(());
        }
        let record = self
            .encoder
            .output(self.block_position, &self.block[..length]);
        writer.append(self.block_position, record)?;
        self.block.drain(..length);
        self.block_position += length as u64;
        self.output_after_checkpoint += length as u64;
        Ok(())
    }

    /// Writes a checkpoint of `screen` at the end of the output in the
    /// files.
    fn write_checkpoint(&mut self, screen: &mut Terminal, size: WindowSize) -> io::Result<()> {
        let Some(writer) = &mut self.writer else {
            return Ok(());
        };
        self.checkpoint_due = false;
        self.output_after_checkpoint = 0;
        let mut redraw_sequence = Vec::new();
        screen.write_redraw(&mut redraw_sequence);
        if redraw_sequence.len() > redraw::MAX_LENGTH {
            // The format has no record for this screen. The next interval
            // tries again.
            return Ok(());
        }
        let record = self
            .encoder
            .checkpoint(self.block_position, size, &redraw_sequence);
        self.checkpoint_interval = self
            .limits
            .checkpoint_interval
            .max(record.len() as u64 * OUTPUT_FOR_EACH_CHECKPOINT_BYTE);
        writer.append(self.block_position, record)
    }

    /// The session is more important than its journal: after a write
    /// failed, the holder continues without the journal.
    fn stop_after_error(&mut self, result: io::Result<()>) {
        if let Err(error) = result {
            eprintln!("nomux-holder: the journal stopped: {error}");
            self.writer = None;
            self.block = Vec::new();
            self.flush_deadline = None;
        }
    }
}

#[cfg(test)]
mod tests;
