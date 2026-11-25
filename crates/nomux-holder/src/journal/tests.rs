use std::fs;
use std::time::Duration;

use nomux_holder_protocol::journal::{JournalReader, segment_files};
use nomux_holder_protocol::wire::WindowSize;
use nomux_term::{Size, Terminal};
use tempfile::TempDir;

use super::{Journal, JournalLimits};

const EPOCH: u32 = 7;

/// Limits that give many records, checkpoints and segments for a small
/// output.
const SMALL: JournalLimits = JournalLimits {
    block_length: 512,
    flush_delay: Duration::from_secs(1),
    checkpoint_interval: 2048,
    segment_length: 4096,
    total_length: 1 << 30,
};

const SIZES: [WindowSize; 3] = [
    WindowSize {
        rows: 24,
        columns: 80,
    },
    WindowSize {
        rows: 10,
        columns: 40,
    },
    WindowSize {
        rows: 30,
        columns: 132,
    },
];

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

/// Output as a program writes it: text, colours, cursor moves, erases and
/// characters of more than one byte.
///
/// It has no sequence for a state that a redraw sequence does not make: see
/// `Terminal::write_redraw`. For example, it does not leave the alternate
/// screen, because that uses the saved cursor.
fn program_output(numbers: &mut Numbers, length: usize) -> Vec<u8> {
    let mut output = Vec::new();
    while output.len() < length {
        let (row, column) = (numbers.below(30) + 1, numbers.below(100) + 1);
        let part = match numbers.below(8) {
            0 => format!("\x1b[{row};{column}H"),
            1 => format!("\x1b[3{};4{}m", numbers.below(8), numbers.below(8)),
            2 => format!("\x1b[38;2;{row};{column};7m\x1b[1m"),
            3 => "\r\n".to_owned(),
            4 => ["\x1b[2J", "\x1b[K", "\x1b[0m", "\x1b[?25l", "\x1b[?25h"][numbers.below(5)]
                .to_owned(),
            5 => "\u{e7}\u{20ac}\u{1f600}\u{4e16}".to_owned(),
            _ => format!("word{} ", numbers.below(1000)),
        };
        output.extend_from_slice(part.as_bytes());
    }
    output
}

fn terminal(size: WindowSize) -> Terminal {
    Terminal::new(screen_size(size))
}

fn screen_size(size: WindowSize) -> Size {
    Size::new(size.rows, size.columns).expect("the size is not zero")
}

fn redraw_sequence(terminal: &mut Terminal) -> Vec<u8> {
    let mut sequence = Vec::new();
    terminal.write_redraw(&mut sequence);
    sequence
}

/// A session without a terminal device: the journal and the screen state
/// get the same output, as in `Session::read_output`.
struct Recorded {
    directory: TempDir,
    journal: Journal,
    screen: Terminal,
    size: WindowSize,
    output: Vec<u8>,
    /// The position and the new size of each change of the window: the
    /// position is the one that the screen corresponded to.
    resizes: Vec<(usize, WindowSize)>,
}

impl Recorded {
    fn new(limits: JournalLimits) -> Self {
        let directory = TempDir::new().expect("the test helper works");
        let journal = Journal::create(directory.path().to_owned(), EPOCH, limits)
            .expect("the journal starts");
        Self {
            directory,
            journal,
            screen: terminal(SIZES[0]),
            size: SIZES[0],
            output: Vec::new(),
            resizes: Vec::new(),
        }
    }

    fn write(&mut self, chunk: &[u8]) {
        self.journal.before_output(&mut self.screen, self.size);
        self.screen.feed(chunk, |_| {});
        self.journal.append(chunk);
        self.output.extend_from_slice(chunk);
        self.journal.output_written(&mut self.screen, self.size);
    }

    fn resize(&mut self, size: WindowSize) {
        let position = self.output.len() - self.screen.incomplete_tail();
        self.screen.resize(screen_size(size));
        self.size = size;
        self.journal.window_resized();
        self.resizes.push((position, size));
    }

    fn flush(&mut self) {
        self.journal.flush(&mut self.screen, self.size);
    }

    fn reader(&self) -> JournalReader {
        JournalReader::open(self.directory.path()).expect("the journal can be read")
    }
}

/// A terminal that gets the output of a recorded session from its start,
/// with each change of the window at its position.
struct Complete<'recorded> {
    recorded: &'recorded Recorded,
    screen: Terminal,
    fed: usize,
    resizes_done: usize,
}

impl<'recorded> Complete<'recorded> {
    fn new(recorded: &'recorded Recorded) -> Self {
        Self {
            recorded,
            screen: terminal(SIZES[0]),
            fed: 0,
            resizes_done: 0,
        }
    }

    /// The screen after the first `position` bytes of the output.
    /// `position` is not before the position of the call before it.
    fn advance_to(&mut self, position: usize) -> &mut Terminal {
        let output = &self.recorded.output;
        while let Some((resize_position, size)) = self.recorded.resizes.get(self.resizes_done)
            && *resize_position <= position
        {
            self.screen
                .feed(&output[self.fed..*resize_position], |_| {});
            self.screen.resize(screen_size(*size));
            self.fed = *resize_position;
            self.resizes_done += 1;
        }
        self.screen.feed(&output[self.fed..position], |_| {});
        self.fed = position;
        &mut self.screen
    }
}

/// Reads the bytes from `from` to `to` through the journal of the session:
/// from the files and from the block in memory.
fn read_range(journal: &Journal, from: usize, to: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    while from + bytes.len() < to {
        let next = from + bytes.len();
        let (position, part) = journal
            .read(next as u64, to - next)
            .expect("the journal can be read");
        assert_eq!(position, next as u64, "the journal has a gap");
        assert!(!part.is_empty(), "the journal ends at {next}, before {to}");
        bytes.extend_from_slice(&part);
    }
    bytes
}

#[test]
fn the_nearest_checkpoint_and_the_bytes_after_it_make_the_screen_of_each_position() {
    for seed in 1..=6u64 {
        let mut numbers = Numbers(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut recorded = Recorded::new(SMALL);
        let output = program_output(&mut numbers, 60_000);
        let mut written = 0;
        while written < output.len() {
            // A chunk ends at each possible place of a sequence.
            let chunk = (numbers.below(700) + 1).min(output.len() - written);
            recorded.write(&output[written..written + chunk]);
            written += chunk;
            match numbers.below(40) {
                0 => recorded.resize(SIZES[numbers.below(SIZES.len())]),
                1 => recorded.flush(),
                _ => {}
            }
        }
        // The checkpoint of a change of the window comes with the next
        // output.
        recorded.write(b"\x1b[0mend");
        let output = recorded.output.clone();
        let reader = recorded.reader();
        assert!(segment_files(recorded.directory.path()).unwrap().len() > 3);

        let mut positions: Vec<usize> = (0..150).map(|_| numbers.below(output.len())).collect();
        positions.extend([0, output.len()]);
        positions.sort_unstable();
        let mut complete = Complete::new(&recorded);
        for position in positions {
            let checkpoint = reader
                .checkpoint_before(EPOCH, position as u64)
                .unwrap()
                .expect("each position has a checkpoint before it");
            let start = usize::try_from(checkpoint.position).unwrap();
            assert!(start <= position);

            let mut replayed = terminal(checkpoint.size);
            replayed.feed(&checkpoint.redraw_sequence, |_| {});
            let bytes = read_range(&recorded.journal, start, position);
            assert_eq!(bytes, &output[start..position]);
            replayed.feed(&bytes, |_| {});

            assert_eq!(
                redraw_sequence(&mut replayed),
                redraw_sequence(complete.advance_to(position)),
                "seed {seed}, position {position}, checkpoint at {start}"
            );
        }
    }
}

#[test]
fn the_size_limit_removes_the_oldest_segments_and_the_reader_gives_the_oldest_position() {
    let limits = JournalLimits {
        total_length: 20_000,
        ..SMALL
    };
    let mut recorded = Recorded::new(limits);
    let mut numbers = Numbers(11);
    let output = program_output(&mut numbers, 200_000);
    for chunk in output.chunks(300) {
        recorded.write(chunk);
        let files = segment_files(recorded.directory.path()).unwrap();
        let length: u64 = files.iter().map(|file| file.length).sum();
        assert!(length <= limits.total_length, "{length} bytes on disk");
    }
    recorded.flush();

    let files = segment_files(recorded.directory.path()).unwrap();
    let oldest = files[0].name.start;
    assert!(oldest > 100_000, "the oldest segment starts at {oldest}");
    let (position, bytes) = recorded.journal.read(0, 100).unwrap();
    assert_eq!(position, oldest);
    assert_eq!(bytes, &output[usize::try_from(oldest).unwrap()..][..100]);
    // The bytes that stay are complete, to the end of the output.
    let start = usize::try_from(oldest).unwrap();
    assert_eq!(
        read_range(&recorded.journal, start, output.len()),
        &output[start..]
    );
    assert_eq!(
        recorded
            .reader()
            .checkpoint_before(EPOCH, oldest - 1)
            .unwrap(),
        None
    );
}

#[test]
fn the_segments_of_an_earlier_epoch_are_the_first_that_the_size_limit_removes() {
    let limits = JournalLimits {
        total_length: 20_000,
        ..SMALL
    };
    let mut numbers = Numbers(5);
    let output = program_output(&mut numbers, 40_000);
    let mut earlier = Recorded::new(limits);
    for chunk in output[..15_000].chunks(300) {
        earlier.write(chunk);
    }
    earlier.flush();
    let earlier_files = segment_files(earlier.directory.path()).unwrap();
    assert!(earlier_files.len() > 1);

    let mut journal =
        Journal::create(earlier.directory.path().to_owned(), EPOCH + 1, limits).unwrap();
    let mut screen = terminal(SIZES[0]);
    for chunk in output.chunks(300) {
        screen.feed(chunk, |_| {});
        journal.append(chunk);
        journal.output_written(&mut screen, SIZES[0]);
    }

    let files = segment_files(earlier.directory.path()).unwrap();
    assert!(
        files.iter().all(|file| file.name.epoch == EPOCH + 1),
        "{files:?}"
    );
    assert!(files.len() > 1);
}

#[test]
fn a_sequence_that_is_not_complete_waits_in_memory_and_sets_no_deadline() {
    let mut recorded = Recorded::new(SMALL);
    assert_eq!(recorded.journal.flush_deadline(), None);

    recorded.write(b"text\x1b[3");
    assert!(recorded.journal.flush_deadline().is_some());
    recorded.flush();

    // An idle holder must not wake for the bytes that cannot go to a file.
    assert_eq!(recorded.journal.flush_deadline(), None);
    let in_files = recorded.reader().read(EPOCH, 0, 100).unwrap().unwrap();
    assert_eq!(
        (in_files.position, in_files.bytes.as_slice()),
        (0, &b"text"[..])
    );
    assert_eq!(read_range(&recorded.journal, 0, 7), b"text\x1b[3");

    recorded.journal.finish(&mut recorded.screen, recorded.size);
    let in_files = recorded.reader().read(EPOCH, 0, 100).unwrap().unwrap();
    assert_eq!(in_files.bytes, b"text\x1b[3");
}

#[test]
fn a_sequence_that_is_longer_than_a_block_does_not_stay_in_memory() {
    let mut recorded = Recorded::new(SMALL);
    let mut output = b"before\x1b]0;".to_vec();
    output.resize(5000, b't');
    for chunk in output.chunks(100) {
        recorded.write(chunk);
    }
    let in_memory = recorded.journal.block.len();
    assert!(
        in_memory < SMALL.block_length + 100,
        "{in_memory} bytes in memory"
    );

    recorded.write(b"\x07after");
    recorded.flush();
    assert_eq!(
        read_range(&recorded.journal, 0, output.len() + 6),
        [&output[..], b"\x07after"].concat()
    );
    // After the end of the sequence, a checkpoint is possible again.
    let checkpoint = recorded
        .reader()
        .checkpoint_before(EPOCH, u64::MAX)
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.position, output.len() as u64 + 6);
}

#[test]
fn a_journal_that_cannot_write_stops_and_the_session_continues() {
    let mut recorded = Recorded::new(SMALL);
    let mut numbers = Numbers(3);
    let output = program_output(&mut numbers, 30_000);
    recorded.write(&output[..1000]);
    recorded.flush();

    fs::remove_dir_all(recorded.directory.path()).unwrap();
    for chunk in output[1000..].chunks(300) {
        recorded.write(chunk);
    }
    recorded.flush();

    assert_eq!(recorded.journal.flush_deadline(), None);
    assert_eq!(recorded.journal.block.len(), 0);
    let (_, bytes) = recorded.journal.read(0, 100).unwrap();
    assert_eq!(bytes, b"");
}
