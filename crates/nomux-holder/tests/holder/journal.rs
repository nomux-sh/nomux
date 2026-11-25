//! The journal: the output of a session on disk, the request that reads it
//! and the epoch of the session.

use std::fmt::Write;
use std::hash::{DefaultHasher, Hasher};
use std::path::{Path, PathBuf};

use nomux_holder_protocol::journal::{JournalReader, MAX_RANGE_LENGTH};
use nomux_holder_protocol::session_state::JOURNAL_DIRECTORY;
use nomux_term::{Size, Terminal};

use crate::fixture::{Client, Holder, runtime_directory, state_directory, wait_for};

/// The largest number of bytes that the journal keeps in memory: one block
/// and the bytes of one read.
const BLOCK_IN_MEMORY: u64 = 64 * 1024 + 4096;

/// The numbers from 1 to `last`, one in each line: the output of `seq`.
fn numbers(last: u32) -> Vec<u8> {
    let mut text = String::new();
    for number in 1..=last {
        writeln!(text, "{number}").expect("a write to a string works");
    }
    text.into_bytes()
}

/// Writes the numbers from 1 to `last` without a change by the terminal,
/// then waits.
fn write_numbers(last: u32) -> String {
    format!("stty raw -echo; seq 1 {last}; read line")
}

fn hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

fn journal_directory(runtime: &Path, session: &str) -> PathBuf {
    state_directory(runtime)
        .join("sessions")
        .join(session)
        .join(JOURNAL_DIRECTORY)
}

/// Reads the output of `epoch` from the files, from position 0 to the end.
/// Fails if the journal does not have each byte.
fn read_files(reader: &JournalReader, epoch: u32) -> Vec<u8> {
    let mut output = Vec::new();
    while let Some(range) = reader
        .read(epoch, output.len() as u64, MAX_RANGE_LENGTH)
        .expect("the journal can be read")
    {
        assert_eq!(range.position, output.len() as u64, "the journal has a gap");
        output.extend_from_slice(&range.bytes);
    }
    output
}

fn redraw_sequence(terminal: &mut Terminal) -> Vec<u8> {
    let mut sequence = Vec::new();
    terminal.write_redraw(&mut sequence);
    sequence
}

/// A small generator of numbers that gives the same numbers in each run.
struct Numbers(u64);

impl Numbers {
    fn below(&mut self, limit: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % limit
    }
}

#[test]
fn a_range_that_left_the_ring_buffer_is_read_from_the_journal() {
    let expected = numbers(400_000);
    let runtime = runtime_directory();
    let command = write_numbers(400_000);
    let _holder = Holder::start(runtime.path(), "long", &[], &["/bin/sh", "-c", &command]);
    let mut client = Client::connect(runtime.path());
    client.wait_for_position(expected.len() as u64);

    // The ring buffer has only the end of the output.
    let overrun = client
        .ring
        .read(&mut [0; 16])
        .expect_err("the start left the ring buffer");
    assert!(overrun.lost > 2_000_000, "{overrun:?}");
    let mut output = Vec::new();
    while output.len() < expected.len() {
        let (position, bytes) = client.journal(output.len() as u64, u32::MAX);
        assert_eq!(position, output.len() as u64);
        assert!(!bytes.is_empty() && bytes.len() <= MAX_RANGE_LENGTH);
        output.extend_from_slice(&bytes);
    }

    assert_eq!(output.len(), expected.len());
    assert_eq!(hash(&output), hash(&expected));
    // A range in the middle, and the end of the journal.
    let (position, bytes) = client.journal(1_234_567, 100);
    assert_eq!(
        (position, bytes.as_slice()),
        (1_234_567, &expected[1_234_567..][..100])
    );
    let end = expected.len() as u64;
    assert_eq!(client.journal(end, 100), (end, Vec::new()));
    assert_eq!(client.journal(end + 1000, 100), (end, Vec::new()));
}

#[test]
fn the_screen_at_each_position_of_a_flood_comes_from_a_checkpoint_and_the_bytes_after_it() {
    // Lines in changing colours, with a full-screen redraw at intervals.
    const FLOOD: &str = concat!(
        r#"awk 'BEGIN { for (i = 0; i < 160000; i++) {"#,
        r#" printf "\033[3%dm%d\033[0m line of the flood\n", i % 8, i;"#,
        r#" if (i % 5000 == 0) printf "\033[2J\033[%d;%dHmark %d", i % 20 + 1, i % 60 + 1, i } }'"#,
        "; printf end-of-flood; read line",
    );
    let runtime = runtime_directory();
    let _holder = Holder::start(runtime.path(), "flood", &[], &["/bin/sh", "-c", FLOOD]);
    let mut client = Client::connect(runtime.path());
    let journal_directory = journal_directory(runtime.path(), "flood");
    // The end of the flood goes to the files after the flush delay.
    let output = wait_for("the flood in the files", || {
        let reader = JournalReader::open(&journal_directory).expect("the journal can be read");
        Some(read_files(&reader, client.hello.epoch))
            .filter(|bytes| bytes.ends_with(b"end-of-flood"))
    });
    assert!(output.len() > 4_000_000, "{}", output.len());
    let end = client.wait_for_position(output.len() as u64);
    assert_eq!(end, output.len() as u64);

    let reader = JournalReader::open(&journal_directory).expect("the journal can be read");
    let mut numbers = Numbers(0x9E37_79B9_7F4A_7C15);
    let mut positions: Vec<u64> = (0..24).map(|_| numbers.below(end)).collect();
    positions.extend([0, 1, end - 1, end]);
    positions.sort_unstable();
    let size = Size::new(client.hello.size.rows, client.hello.size.columns).expect("not zero");
    let mut complete = Terminal::new(size);
    let mut fed = 0;
    let mut checkpoints = Vec::new();
    for position in positions {
        let index = usize::try_from(position).expect("the position is small");
        complete.feed(&output[fed..index], |_| {});
        fed = index;

        let checkpoint = reader
            .checkpoint_before(client.hello.epoch, position)
            .expect("the journal can be read")
            .expect("each position has a checkpoint before it");
        assert!(checkpoint.position <= position);
        assert_eq!(checkpoint.size, client.hello.size);
        let mut replayed = Terminal::new(size);
        replayed.feed(&checkpoint.redraw_sequence, |_| {});
        let start = usize::try_from(checkpoint.position).expect("the position is small");
        replayed.feed(&output[start..index], |_| {});

        assert_eq!(
            redraw_sequence(&mut replayed),
            redraw_sequence(&mut complete),
            "position {position}, checkpoint at {}",
            checkpoint.position
        );
        checkpoints.push(checkpoint.position);
    }
    checkpoints.dedup();
    // The positions used more than the checkpoint at the start.
    assert!(checkpoints.len() >= 4, "{checkpoints:?}");
}

#[test]
fn a_holder_that_is_killed_during_a_flood_leaves_a_prefix_of_the_output() {
    let runtime = runtime_directory();
    let command = write_numbers(100_000_000);
    let mut holder = Holder::start(runtime.path(), "killed", &[], &["/bin/sh", "-c", &command]);
    let mut client = Client::connect(runtime.path());
    let epoch = client.hello.epoch;
    let seen = client.wait_for_position(3_000_000);

    holder.kill();

    let reader = JournalReader::open(&journal_directory(runtime.path(), "killed"))
        .expect("the journal can be read");
    let output = read_files(&reader, epoch);
    let expected = numbers(2_000_000);
    assert!(output.len() < expected.len());
    assert_eq!(hash(&output), hash(&expected[..output.len()]));
    // The kill loses only the bytes that waited in memory.
    assert!(
        output.len() as u64 + BLOCK_IN_MEMORY >= seen,
        "{} of {seen}",
        output.len()
    );
    let checkpoint = reader
        .checkpoint_before(epoch, output.len() as u64)
        .expect("the journal can be read")
        .expect("the files have a checkpoint");
    assert!(checkpoint.position + 2_000_000 > output.len() as u64);
}

#[test]
fn a_new_holder_of_a_session_has_the_next_epoch_and_keeps_the_old_output() {
    let runtime = runtime_directory();
    let options = ["--exit-timeout", "0"];
    let first_command = ["/bin/sh", "-c", "stty raw -echo; printf first"];
    let mut first = Holder::start(runtime.path(), "again", &options, &first_command);
    assert!(first.wait_for_exit().success());

    let second_command = ["/bin/sh", "-c", "stty raw -echo; printf second; read line"];
    let _second = Holder::start(runtime.path(), "again", &[], &second_command);
    let mut client = Client::connect(runtime.path());
    client.read_until("second");

    assert_eq!(client.hello.epoch, 2);
    // The positions of the new holder start at 0.
    assert_eq!(client.journal(0, 100), (0, b"second".to_vec()));
    let reader = JournalReader::open(&journal_directory(runtime.path(), "again"))
        .expect("the journal can be read");
    assert_eq!(reader.epochs(), [1, 2]);
    assert_eq!(read_files(&reader, 1), b"first");
}
