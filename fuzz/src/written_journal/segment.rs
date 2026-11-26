//! One segment file of a written journal, and what a reader must find in
//! it.

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;

use nomux_holder_protocol::journal::{Checkpoint, SegmentHeader, SegmentName};

use crate::forged_record::Encoded;

/// What a record of the writer has.
#[derive(Debug)]
pub(super) enum Content {
    /// Output bytes up to this position.
    OutputTo(u64),
    Checkpoint(Checkpoint),
}

/// A record that the writer appended.
#[derive(Debug)]
struct WrittenRecord {
    /// The offset in the file after the record.
    end: u64,
    content: Content,
}

#[derive(Debug)]
pub(super) struct WrittenSegment {
    pub(super) name: SegmentName,
    file: File,
    length: u64,
    /// The file has the bytes of the writer from its start to this offset.
    /// The bytes after it are damaged or forged, or they follow such bytes.
    trusted_length: u64,
    /// `false` after the first change that the writer did not make. The
    /// trusted bytes then get no more record.
    intact: bool,
    /// `true` if a hostile writer made bytes of the file. Their checksums
    /// are correct, thus the reader can take them.
    pub(super) forged: bool,
    /// `true` if the file is a copy of a different segment.
    copy: bool,
    /// The position that a reader expects after the last record, if it took
    /// each record of the file.
    pub(super) next_position: u64,
    records: Vec<WrittenRecord>,
}

impl WrittenSegment {
    /// The segment as the holder starts it: a file with the header.
    pub(super) fn create(directory: &Path, name: SegmentName) -> Self {
        let header = SegmentHeader { name }.encode();
        let mut segment = Self::with_content(directory, name, &header);
        segment.trusted_length = segment.length;
        segment.intact = true;
        segment
    }

    /// A file of a hostile writer with `content`.
    pub(super) fn forged(directory: &Path, name: SegmentName, content: &[u8]) -> Self {
        let mut segment = Self::with_content(directory, name, content);
        segment.forged = true;
        segment
    }

    /// A copy of the file of `source` with the name of a different segment.
    /// A reader does not use it while its header says that it is `source`.
    /// A changed bit of the header can make the header say that it is this
    /// segment: the reader then takes the records of `source`, as from a
    /// hostile writer.
    pub(super) fn copy(directory: &Path, name: SegmentName, source: &Self) -> Self {
        let mut content = vec![0; usize::try_from(source.length).expect("a segment is small")];
        source
            .file
            .read_exact_at(&mut content, 0)
            .expect("the file has its length");
        let mut segment = Self::with_content(directory, name, &content);
        segment.forged = source.forged || !source.intact;
        segment.copy = true;
        segment
    }

    /// A file with `content` that has no byte that a reader must find.
    fn with_content(directory: &Path, name: SegmentName, content: &[u8]) -> Self {
        let file = File::create_new(directory.join(name.file_name()))
            .expect("the host has space for a segment");
        let mut segment = Self {
            name,
            file,
            length: 0,
            trusted_length: 0,
            intact: false,
            forged: false,
            copy: false,
            next_position: name.start,
            records: Vec::new(),
        };
        segment.write_at_end(content);
        segment
    }

    /// Appends a record of the writer.
    pub(super) fn append(&mut self, record: &[u8], content: Content) {
        if let Content::OutputTo(end) = content {
            self.next_position = end;
        }
        self.write_at_end(record);
        if self.intact {
            self.trusted_length = self.length;
        }
        self.records.push(WrittenRecord {
            end: self.length,
            content,
        });
    }

    /// Appends a record of a hostile writer.
    pub(super) fn append_forged(&mut self, record: &Encoded) {
        self.intact = false;
        self.forged = true;
        self.next_position = record.next_position;
        self.write_at_end(&record.bytes);
    }

    pub(super) fn flip_bits(&mut self, offset: u16, mask: u8) {
        if self.length == 0 {
            return;
        }
        let offset = u64::from(offset) % self.length;
        let mut byte = [0];
        self.file
            .read_exact_at(&mut byte, offset)
            .expect("the offset is in the file");
        byte[0] ^= mask;
        self.file
            .write_all_at(&byte, offset)
            .expect("the offset is in the file");
        self.intact = false;
        self.forged |= self.copy;
        self.trusted_length = self.trusted_length.min(offset);
    }

    pub(super) fn truncate(&mut self, length: u16) {
        self.set_length(u64::from(length) % (self.length + 1));
    }

    /// Cuts the file after one of the records of the writer.
    pub(super) fn cut_after_record(&mut self, record: u16) {
        let ends: Vec<u64> = self
            .records
            .iter()
            .map(|record| record.end)
            .filter(|end| *end <= self.length)
            .collect();
        if !ends.is_empty() {
            self.set_length(ends[usize::from(record) % ends.len()]);
        }
    }

    fn set_length(&mut self, length: u64) {
        self.file
            .set_len(length)
            .expect("a file can become shorter");
        self.length = length;
        self.intact = false;
        self.trusted_length = self.trusted_length.min(length);
    }

    /// The position after the output bytes that a reader must find: the
    /// output records of the writer before the first byte that is not of the
    /// writer. The start position if the segment has no such record.
    pub(super) fn trusted_output_end(&self) -> u64 {
        self.trusted_contents()
            .filter_map(|content| match content {
                Content::OutputTo(end) => Some(*end),
                Content::Checkpoint(_) => None,
            })
            .last()
            .unwrap_or(self.name.start)
    }

    /// The checkpoints that a reader must find.
    pub(super) fn trusted_checkpoints(&self) -> impl Iterator<Item = &Checkpoint> {
        self.trusted_contents().filter_map(checkpoint)
    }

    /// Each checkpoint that the writer appended, also after a damaged byte.
    pub(super) fn written_checkpoints(&self) -> impl Iterator<Item = &Checkpoint> {
        self.records
            .iter()
            .filter_map(|record| checkpoint(&record.content))
    }

    fn trusted_contents(&self) -> impl Iterator<Item = &Content> {
        let is_header_trusted = self.trusted_length >= SegmentHeader::LENGTH as u64;
        self.records
            .iter()
            .filter(move |record| is_header_trusted && record.end <= self.trusted_length)
            .map(|record| &record.content)
    }

    fn write_at_end(&mut self, bytes: &[u8]) {
        self.file
            .write_all_at(bytes, self.length)
            .expect("the host has space for a record");
        self.length += bytes.len() as u64;
    }
}

fn checkpoint(content: &Content) -> Option<&Checkpoint> {
    match content {
        Content::Checkpoint(checkpoint) => Some(checkpoint),
        Content::OutputTo(_) => None,
    }
}
