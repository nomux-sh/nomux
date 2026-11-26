//! The output history of one session: the bytes that its command wrote, in
//! each epoch that the journal has, and the checkpoints of its screen.
//!
//! The history has two sources. For the epoch of a holder that runs, the
//! holder answers: it has also the bytes that it did not write to a file
//! yet. For each other epoch, for a session without a holder, and when the
//! holder does not answer, the daemon reads the segment files itself. `docs/holder.md` has the format
//! and the rules of the journal.

mod output_before;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use nomux_holder_protocol::journal::{
    Checkpoint, JournalRange, JournalReader, MAX_RANGE_LENGTH, segment_files,
};
use tracing::debug;

use crate::holder::OutputFeed;

pub(crate) struct SessionJournal {
    directory: PathBuf,
    /// The read side of the holder that runs. Its epoch is the live epoch.
    live: Option<Arc<OutputFeed>>,
}

impl SessionJournal {
    /// The history of a session without a holder, from the segment files in
    /// `directory`.
    pub(crate) fn stored(directory: PathBuf) -> Self {
        Self {
            directory,
            live: None,
        }
    }

    /// The history of a session whose holder answers on `feed`.
    pub(crate) fn live(directory: PathBuf, feed: Arc<OutputFeed>) -> Self {
        Self {
            directory,
            live: Some(feed),
        }
    }

    /// The newest epoch of the session. `None` if the session has no
    /// holder and no segment file.
    pub(crate) fn newest_epoch(&self) -> io::Result<Option<u32>> {
        match &self.live {
            Some(feed) => Ok(Some(feed.epoch())),
            None => Ok(self.files()?.epochs().last().copied()),
        }
    }

    /// Reads output bytes of `epoch` from the position `from` on:
    /// `max_length` bytes at most, and fewer at the end of a segment.
    ///
    /// The range starts after `from` if the journal does not have the byte
    /// at `from`. Returns `None` if the journal has no byte at `from` or
    /// after it.
    pub(crate) async fn read(
        &self,
        epoch: u32,
        from: u64,
        max_length: usize,
    ) -> io::Result<Option<JournalRange>> {
        let max_length = max_length.clamp(1, MAX_RANGE_LENGTH);
        if let Some(feed) = self.live_feed(epoch) {
            let wanted = u32::try_from(max_length).unwrap_or(u32::MAX);
            match feed.journal_read(from, wanted).await {
                Ok(range) => return Ok(range),
                // The holder is gone: its command ended, or it was stopped.
                // The files have what it wrote.
                Err(error) => debug!(%error, "the holder does not give its journal"),
            }
        }
        self.files()?.read(epoch, from, max_length)
    }

    /// The position after the last output byte of `epoch` that the session
    /// has.
    pub(crate) fn end(&self, epoch: u32) -> io::Result<u64> {
        match self.live_feed(epoch) {
            Some(feed) => Ok(feed.reader()?.write_position()),
            None => self.end_of_stored_epoch(epoch),
        }
    }

    /// The last checkpoint of `epoch`: the newest screen that the files
    /// have.
    pub(crate) fn last_checkpoint(&self, epoch: u32) -> io::Result<Option<Checkpoint>> {
        self.files()?.checkpoint_before(epoch, u64::MAX)
    }

    /// The oldest checkpoint of `epoch` that is not before the position
    /// `from`, with the precision of one segment: the last checkpoint of
    /// the oldest segment that has one.
    pub(crate) fn oldest_checkpoint(
        &self,
        epoch: u32,
        from: u64,
    ) -> io::Result<Option<Checkpoint>> {
        let files = self.files()?;
        // The end of a segment is the start of the segment after it.
        let ends = segment_files(&self.directory)?
            .into_iter()
            .filter(|segment| segment.name.epoch == epoch)
            .map(|segment| segment.name.start)
            .skip(1)
            .chain([u64::MAX]);
        for end in ends {
            let checkpoint = files
                .checkpoint_before(epoch, end)?
                .filter(|checkpoint| checkpoint.position >= from);
            if checkpoint.is_some() {
                return Ok(checkpoint);
            }
        }
        Ok(None)
    }

    fn end_of_stored_epoch(&self, epoch: u32) -> io::Result<u64> {
        let files = self.files()?;
        let starts: Vec<u64> = segment_files(&self.directory)?
            .into_iter()
            .filter(|segment| segment.name.epoch == epoch)
            .map(|segment| segment.name.start)
            .collect();
        // The newest segment that has output ends the epoch. A segment
        // after it is empty or damaged.
        for start in starts.into_iter().rev() {
            let mut end = start;
            while let Some(range) = files.read(epoch, end, MAX_RANGE_LENGTH)? {
                end = range.position + range.bytes.len() as u64;
            }
            if end > start {
                return Ok(end);
            }
        }
        Ok(0)
    }

    fn live_feed(&self, epoch: u32) -> Option<&OutputFeed> {
        self.live.as_deref().filter(|feed| feed.epoch() == epoch)
    }

    /// The segment files at this moment. A holder that runs adds and
    /// removes segments, thus each operation lists them again.
    fn files(&self) -> io::Result<JournalReader> {
        JournalReader::open(&self.directory)
    }
}
