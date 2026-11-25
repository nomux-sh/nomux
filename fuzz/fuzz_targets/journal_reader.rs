//! The journal reader on files that a power cut, a different process and a
//! hostile writer changed.
//!
//! The daemon and the holder read the journal of a session from the disk,
//! and they trust no byte of it. The reader must not panic and must not
//! allocate more than the limit. A range and a checkpoint that it returns
//! must be possible answers. The records of the writer before the first
//! changed byte of a segment must stay readable, and without a hostile
//! writer, each byte that the reader returns must be the output byte of its
//! position: the checksums find each changed byte.
//!
//! `src/journal_script.rs` has the format of the input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use nomux_fuzz::journal_script::{JournalScript, Step};
use nomux_fuzz::written_journal::{WrittenJournal, output_byte};
use nomux_holder_protocol::journal::{JournalReader, MAX_RANGE_LENGTH};
use nomux_holder_protocol::redraw;

fuzz_target!(|input: &[u8]| {
    let mut journal = WrittenJournal::new();
    for step in JournalScript::new(input) {
        match step {
            Step::Change(change) => journal.apply(&change),
            Step::Read {
                epoch,
                from,
                max_length,
            } => {
                let (epoch, from) = (journal.epoch_of(epoch), journal.position_of(from));
                assert_read_is_correct(&journal, epoch, from, max_length);
            }
            Step::CheckpointBefore { epoch, position } => {
                let epoch = journal.epoch_of(epoch);
                assert_checkpoint_is_correct(&journal, epoch, journal.position_of(position));
            }
        }
    }

    assert_eq!(reader(&journal).epochs(), journal.epochs());
    // Each segment from its start, also if the script has no query.
    for name in journal.names().collect::<Vec<_>>() {
        assert_read_is_correct(&journal, name.epoch, name.start, MAX_RANGE_LENGTH);
        assert_checkpoint_is_correct(&journal, name.epoch, u64::MAX);
    }
});

/// A reader of the files as they are now.
fn reader(journal: &WrittenJournal) -> JournalReader {
    JournalReader::open(journal.directory()).expect("the journal directory can be read")
}

fn assert_read_is_correct(journal: &WrittenJournal, epoch: u32, from: u64, max_length: usize) {
    let range = reader(journal)
        .read(epoch, from, max_length)
        .expect("the segment files can be read");
    let trusted_end = journal
        .trusted_output_end(epoch, from)
        .filter(|_| max_length != 0);
    let Some(range) = range else {
        assert_eq!(trusted_end, None, "no output at {from} of epoch {epoch}");
        return;
    };
    let length = range.bytes.len();
    assert!(range.position >= from, "{} < {from}", range.position);
    assert!((1..=max_length).contains(&length), "{length} bytes");
    let end = range
        .position
        .checked_add(length as u64)
        .expect("the last byte has a position");

    if let Some(trusted_end) = trusted_end {
        assert_eq!(range.position, from, "a gap before trusted output");
        // A range is shorter only at the end of a segment.
        let complete = length == max_length || end >= trusted_end;
        assert!(complete, "the range ends at {end}, before {trusted_end}");
    }
    // After the trusted output, a hostile writer selects the bytes.
    let output_end = if journal.is_forged(epoch) {
        trusted_end.map_or(range.position, |trusted_end| trusted_end.min(end))
    } else {
        end
    };
    for (position, byte) in (range.position..output_end).zip(&range.bytes) {
        assert_eq!(*byte, output_byte(epoch, position), "position {position}");
    }
}

fn assert_checkpoint_is_correct(journal: &WrittenJournal, epoch: u32, position: u64) {
    let checkpoint = reader(journal)
        .checkpoint_before(epoch, position)
        .expect("the segment files can be read");
    let trusted = journal.last_trusted_checkpoint(epoch, position);
    let Some(checkpoint) = checkpoint else {
        assert_eq!(trusted, None, "no checkpoint before {position}");
        return;
    };
    assert!(checkpoint.position <= position);
    assert_ne!(checkpoint.size.cells(), 0);
    let length = checkpoint.redraw_sequence.len();
    assert!((1..=redraw::MAX_LENGTH).contains(&length), "{length} bytes");
    if journal.is_forged(epoch) {
        return;
    }
    assert!(
        journal.wrote_checkpoint(epoch, &checkpoint),
        "a checkpoint at {} that the writer did not write",
        checkpoint.position
    );
    if let Some(trusted) = trusted {
        assert!(checkpoint.position >= trusted, "an older checkpoint");
    }
}
