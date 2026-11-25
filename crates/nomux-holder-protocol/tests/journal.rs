//! The reader of the journal: ranges of output bytes, checkpoints, and
//! files that are cut, damaged or removed.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use nomux_holder_protocol::journal::{
    JournalReader, RecordEncoder, SegmentHeader, SegmentName, segment_files,
};
use nomux_holder_protocol::wire::WindowSize;
use tempfile::TempDir;

const EPOCH: u32 = 3;
const SIZE: WindowSize = WindowSize {
    rows: 24,
    columns: 80,
};
/// The output bytes of each segment that `write_journal` makes.
const SEGMENT_OUTPUT: usize = 40_000;

/// A small generator of numbers that gives the same numbers in each run.
struct Numbers(u64);

impl Numbers {
    fn below(&mut self, limit: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % limit as u64).expect("the number is below a usize")
    }
}

/// Output that an LZ4 block makes shorter, and that is different at each
/// position.
fn output(length: usize) -> Vec<u8> {
    let mut output = Vec::new();
    let mut line = 0;
    while output.len() < length {
        output
            .extend_from_slice(format!("\x1b[32m{line:06}\x1b[0m a line of output\r\n").as_bytes());
        line += 1;
    }
    output.truncate(length);
    output
}

/// The redraw sequence of the checkpoint at `position`: the test only
/// compares it.
fn redraw_sequence(position: usize) -> Vec<u8> {
    format!("\x1b[H\x1b[2Jscreen at {position}").into_bytes()
}

/// Where the records of a journal are in its files.
struct Written {
    directory: TempDir,
    output: Vec<u8>,
    /// For each record: the path of its segment, its offset in the file, and
    /// the position of the output after the records before it.
    records: Vec<(PathBuf, u64, usize)>,
    checkpoints: Vec<usize>,
}

/// Writes a journal of `segments` segments as a holder does: each segment
/// starts with a checkpoint, and its output is in records of different
/// lengths, some of them too short for an LZ4 block.
fn write_journal(segments: usize, numbers: &mut Numbers) -> Written {
    let directory = TempDir::new().expect("the test helper works");
    let output = output(segments * SEGMENT_OUTPUT);
    let mut encoder = RecordEncoder::new();
    let mut records = Vec::new();
    let mut checkpoints = Vec::new();
    let mut position = 0;
    for segment in 0..segments {
        let name = SegmentName {
            epoch: EPOCH,
            start: position as u64,
        };
        let path = directory.path().join(name.file_name());
        let mut file = File::create(&path).expect("the test helper works");
        let mut offset = SegmentHeader::LENGTH as u64;
        file.write_all(&SegmentHeader { name }.encode())
            .expect("the test helper works");
        let end = (segment + 1) * SEGMENT_OUTPUT;
        while position < end {
            if position % SEGMENT_OUTPUT == 0 || numbers.below(6) == 0 {
                let record = encoder.checkpoint(position as u64, SIZE, &redraw_sequence(position));
                records.push((path.clone(), offset, position));
                checkpoints.push(position);
                file.write_all(record).expect("the test helper works");
                offset += record.len() as u64;
            }
            let length = [20, 700, 5000][numbers.below(3)].min(end - position);
            let record = encoder.output(position as u64, &output[position..position + length]);
            records.push((path.clone(), offset, position));
            file.write_all(record).expect("the test helper works");
            offset += record.len() as u64;
            position += length;
        }
    }
    Written {
        directory,
        output,
        records,
        checkpoints,
    }
}

fn open(directory: &Path) -> JournalReader {
    JournalReader::open(directory).expect("the journal can be read")
}

/// Reads from `from` to the end of the journal. Returns each range with its
/// position.
fn read_ranges(reader: &JournalReader, from: u64, max_length: usize) -> Vec<(u64, Vec<u8>)> {
    let mut ranges = Vec::new();
    let mut next = from;
    while let Some(range) = reader.read(EPOCH, next, max_length).expect("no I/O error") {
        assert!(range.position >= next);
        assert!(!range.bytes.is_empty() && range.bytes.len() <= max_length);
        next = range.position + range.bytes.len() as u64;
        ranges.push((range.position, range.bytes));
    }
    ranges
}

#[test]
fn a_range_has_the_bytes_of_its_positions_across_records_and_segments() {
    let mut numbers = Numbers(1);
    let written = write_journal(3, &mut numbers);
    let reader = open(written.directory.path());
    let end = written.output.len();

    for _ in 0..200 {
        let from = numbers.below(end);
        let max_length = numbers.below(9000) + 1;
        let range = reader
            .read(EPOCH, from as u64, max_length)
            .unwrap()
            .unwrap();
        assert_eq!(range.position, from as u64);
        let expected = &written.output[from..];
        assert_eq!(range.bytes, &expected[..range.bytes.len()]);
        // A range is shorter than the request only at the end of a segment.
        let segment_end = (from / SEGMENT_OUTPUT + 1) * SEGMENT_OUTPUT;
        assert_eq!(range.bytes.len(), max_length.min(segment_end - from));
    }
    let all: Vec<u8> = read_ranges(&reader, 0, 3000)
        .into_iter()
        .flat_map(|(_, bytes)| bytes)
        .collect();
    assert_eq!(all, written.output);
    assert_eq!(reader.read(EPOCH, end as u64, 100).unwrap(), None);
    assert_eq!(reader.read(EPOCH + 1, 0, 100).unwrap(), None);
    assert_eq!(reader.epochs(), [EPOCH]);
}

#[test]
fn the_checkpoint_before_a_position_is_the_last_one_that_is_not_after_it() {
    let mut numbers = Numbers(2);
    let written = write_journal(3, &mut numbers);
    let reader = open(written.directory.path());

    for _ in 0..200 {
        let position = numbers.below(written.output.len() + 1);
        let expected = written
            .checkpoints
            .iter()
            .rev()
            .find(|at| **at <= position)
            .unwrap();
        let checkpoint = reader
            .checkpoint_before(EPOCH, position as u64)
            .unwrap()
            .unwrap();
        assert_eq!(checkpoint.position, *expected as u64);
        assert_eq!(checkpoint.size, SIZE);
        assert_eq!(checkpoint.redraw_sequence, redraw_sequence(*expected));
    }
    assert_eq!(reader.checkpoint_before(EPOCH + 1, 100).unwrap(), None);
}

#[test]
fn a_reader_without_the_oldest_segment_starts_at_the_oldest_position_that_exists() {
    let mut numbers = Numbers(3);
    let written = write_journal(3, &mut numbers);
    let oldest = &segment_files(written.directory.path()).unwrap()[0];
    fs::remove_file(&oldest.path).unwrap();
    let reader = open(written.directory.path());

    let range = reader.read(EPOCH, 10, 100).unwrap().unwrap();
    assert_eq!(range.position, SEGMENT_OUTPUT as u64);
    assert_eq!(range.bytes, &written.output[SEGMENT_OUTPUT..][..100]);
    assert_eq!(
        reader
            .checkpoint_before(EPOCH, SEGMENT_OUTPUT as u64 - 1)
            .unwrap(),
        None
    );
    let first = reader
        .checkpoint_before(EPOCH, SEGMENT_OUTPUT as u64)
        .unwrap()
        .unwrap();
    assert_eq!(first.position, SEGMENT_OUTPUT as u64);
}

/// Cuts or changes the file of one record at an offset in that record.
fn damage(path: &Path, offset: u64, numbers: &mut Numbers) {
    assert!(offset < fs::metadata(path).expect("the test helper works").len());
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .expect("the test helper works");
    match numbers.below(3) {
        0 => file.set_len(offset).expect("the test helper works"),
        1 => {
            let mut bytes = fs::read(path).expect("the test helper works");
            bytes[usize::try_from(offset).expect("the test helper works")] ^= 1 << numbers.below(8);
            fs::write(path, bytes).expect("the test helper works");
        }
        _ => {
            // The rest of the file is bytes that no writer made.
            let mut bytes = fs::read(path).expect("the test helper works");
            for byte in &mut bytes[usize::try_from(offset).expect("the test helper works")..] {
                *byte = u8::try_from(numbers.below(256)).expect("the test helper works");
            }
            fs::write(path, bytes).expect("the test helper works");
        }
    }
}

#[test]
fn a_file_that_is_cut_or_damaged_at_a_random_offset_gives_its_valid_part() {
    for seed in 1..=60u64 {
        let mut numbers = Numbers(seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
        let written = write_journal(3, &mut numbers);
        let index = numbers.below(written.records.len());
        let (path, record_offset, valid_end) = &written.records[index];
        let record_end = written
            .records
            .get(index + 1)
            .filter(|next| next.0 == *path);
        let record_length = match record_end {
            Some(next) => next.1 - record_offset,
            None => fs::metadata(path).unwrap().len() - record_offset,
        };
        let offset = record_offset + numbers.below(usize::try_from(record_length).unwrap()) as u64;
        damage(path, offset, &mut numbers);

        let reader = open(written.directory.path());
        let ranges = read_ranges(&reader, 0, 4096);

        // Each byte that the reader gives is the byte of its position.
        let mut readable = vec![false; written.output.len()];
        for (position, bytes) in &ranges {
            let start = usize::try_from(*position).unwrap();
            assert_eq!(
                bytes,
                &written.output[start..start + bytes.len()],
                "seed {seed}"
            );
            readable[start..start + bytes.len()].fill(true);
        }
        // The records before the damage are readable, and the segments
        // after the damaged one are complete.
        let segment_start = valid_end / SEGMENT_OUTPUT * SEGMENT_OUTPUT;
        let next_segment = segment_start + SEGMENT_OUTPUT;
        assert!(
            readable[..*valid_end].iter().all(|byte| *byte),
            "seed {seed}"
        );
        assert!(
            readable[next_segment..].iter().all(|byte| *byte),
            "seed {seed}"
        );
        // A checkpoint that the reader gives is one that the writer made.
        for position in [
            *valid_end,
            next_segment.saturating_sub(1),
            written.output.len(),
        ] {
            if let Some(checkpoint) = reader.checkpoint_before(EPOCH, position as u64).unwrap() {
                let at = usize::try_from(checkpoint.position).unwrap();
                assert!(
                    at <= position && written.checkpoints.contains(&at),
                    "seed {seed}"
                );
                assert_eq!(
                    checkpoint.redraw_sequence,
                    redraw_sequence(at),
                    "seed {seed}"
                );
            }
        }
    }
}

#[test]
fn a_file_whose_header_is_not_the_header_of_its_name_is_no_segment() {
    let mut numbers = Numbers(4);
    let written = write_journal(2, &mut numbers);
    let files = segment_files(written.directory.path()).unwrap();
    let renamed = SegmentName {
        epoch: EPOCH,
        start: 5,
    };
    fs::rename(
        &files[0].path,
        written.directory.path().join(renamed.file_name()),
    )
    .unwrap();
    fs::write(written.directory.path().join("notes.txt"), "not a segment").unwrap();
    let reader = open(written.directory.path());

    let range = reader.read(EPOCH, 0, 100).unwrap().unwrap();
    assert_eq!(range.position, SEGMENT_OUTPUT as u64);
}

#[test]
fn a_directory_that_does_not_exist_is_an_empty_journal() {
    let directory = TempDir::new().unwrap();
    let reader = open(&directory.path().join("journal"));

    assert_eq!(reader.epochs(), [0u32; 0]);
    assert_eq!(reader.read(EPOCH, 0, 100).unwrap(), None);
    assert_eq!(reader.checkpoint_before(EPOCH, 0).unwrap(), None);
}
