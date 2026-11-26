//! Appends records to the segment files of the journal, starts a new
//! segment when the open one is full, and applies the size limit.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::mem;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use nomux_holder_protocol::journal::{SegmentHeader, SegmentName, segment_files};

use super::limits::JournalLimits;
use super::retention::Retention;

const OWNER_READ_WRITE: u32 = 0o600;

/// The segment that gets the next record.
struct OpenSegment {
    file: File,
    path: PathBuf,
    name: SegmentName,
    length: u64,
}

impl OpenSegment {
    fn create(directory: &Path, name: SegmentName) -> io::Result<Self> {
        let path = directory.join(name.file_name());
        let mut file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(OWNER_READ_WRITE)
            .open(&path)?;
        let header = SegmentHeader { name }.encode();
        file.write_all(&header)?;
        Ok(Self {
            file,
            path,
            name,
            length: header.len() as u64,
        })
    }
}

pub(super) struct SegmentWriter {
    directory: PathBuf,
    open: OpenSegment,
    segment_length: u64,
    retention: Retention,
}

impl SegmentWriter {
    /// Starts the first segment of `epoch` in `directory` at the position
    /// `start`. The segments of the earlier epochs of the session count for
    /// the size limit: they are the oldest segments.
    pub(super) fn create(
        directory: PathBuf,
        epoch: u32,
        start: u64,
        limits: &JournalLimits,
    ) -> io::Result<Self> {
        let mut retention = Retention::new(limits.total_length);
        for file in segment_files(&directory)? {
            retention.add_closed(file.path, file.length);
        }
        let open = OpenSegment::create(&directory, SegmentName { epoch, start })?;
        Ok(Self {
            directory,
            open,
            segment_length: limits.segment_length,
            retention,
        })
    }

    /// Appends one record with one write. `position` is the position of the
    /// record: if the record starts a new segment, it is the start position
    /// of that segment.
    pub(super) fn append(&mut self, position: u64, record: &[u8]) -> io::Result<()> {
        // Two segments of one epoch cannot have the same start position.
        if self.open.length >= self.segment_length && position > self.open.name.start {
            self.start_segment(position)?;
        }
        self.open.file.write_all(record)?;
        self.open.length += record.len() as u64;
        self.retention.enforce(self.open.length)
    }

    /// Asks the system to write the open segment to the disk now.
    pub(super) fn sync(&self) -> io::Result<()> {
        self.open.file.sync_data()
    }

    fn start_segment(&mut self, start: u64) -> io::Result<()> {
        let name = SegmentName {
            epoch: self.open.name.epoch,
            start,
        };
        let next = OpenSegment::create(&self.directory, name)?;
        let closed = mem::replace(&mut self.open, next);
        self.retention.add_closed(closed.path, closed.length);
        Ok(())
    }
}
