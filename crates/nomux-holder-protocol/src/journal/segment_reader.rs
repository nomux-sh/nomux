//! Reads the records of one segment file in their sequence, and stops at
//! the first one that is not valid.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use super::checksum::crc32;
use super::compression::decompress;
use super::record::{Codec, RecordHeader, RecordKind};
use super::segment_files::SegmentFile;
use super::segment_header::SegmentHeader;

pub(super) struct SegmentReader {
    file: File,
    /// The position that the next record must have.
    next_position: u64,
}

impl SegmentReader {
    /// Opens a segment. Returns `None` if the file is gone, or if its header
    /// is not the header of the segment that its name says.
    pub(super) fn open(segment: &SegmentFile) -> io::Result<Option<Self>> {
        let mut file = match File::open(&segment.path) {
            Ok(file) => file,
            // The holder removes the oldest segments while a reader reads.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = [0; SegmentHeader::LENGTH];
        if !read_fully(&mut file, &mut bytes)? {
            return Ok(None);
        }
        let Some((header, first_record)) = SegmentHeader::decode(&bytes) else {
            return Ok(None);
        };
        if header.name != segment.name {
            return Ok(None);
        }
        file.seek(SeekFrom::Start(first_record))?;
        Ok(Some(Self {
            file,
            next_position: header.name.start,
        }))
    }

    /// The header of the next record. Returns `None` at the end of the
    /// valid records. After a header, the caller must call
    /// [`payload`](Self::payload) or [`skip_payload`](Self::skip_payload).
    pub(super) fn next_header(&mut self) -> io::Result<Option<RecordHeader>> {
        let mut bytes = [0; RecordHeader::LENGTH];
        if !read_fully(&mut self.file, &mut bytes)? {
            return Ok(None);
        }
        // An output record continues the output, and a checkpoint is at the
        // position of the next output byte.
        let Some(header) =
            RecordHeader::decode(&bytes).filter(|header| header.position == self.next_position)
        else {
            return Ok(None);
        };
        if header.kind == RecordKind::Output {
            let Some(end) = header.position.checked_add(u64::from(header.raw_length)) else {
                return Ok(None);
            };
            self.next_position = end;
        }
        Ok(Some(header))
    }

    /// The payload of the record of `header`, after decompression. Returns
    /// `None` if the payload is not complete or not valid.
    pub(super) fn payload(&mut self, header: &RecordHeader) -> io::Result<Option<Vec<u8>>> {
        // `RecordHeader::decode` limits the two lengths.
        let mut stored = vec![0; header.stored_length as usize];
        if !read_fully(&mut self.file, &mut stored)? || crc32(&stored) != header.payload_checksum {
            return Ok(None);
        }
        match header.codec {
            Codec::Stored => Ok(Some(stored)),
            Codec::Lz4 => Ok(decompress(&stored, header.raw_length as usize).ok()),
        }
    }

    pub(super) fn skip_payload(&mut self, header: &RecordHeader) -> io::Result<()> {
        self.file
            .seek(SeekFrom::Current(i64::from(header.stored_length)))?;
        Ok(())
    }
}

/// Fills `buffer` from `file`. Returns `false` if the file ends first.
fn read_fully(file: &mut File, buffer: &mut [u8]) -> io::Result<bool> {
    match file.read_exact(buffer) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error),
    }
}
