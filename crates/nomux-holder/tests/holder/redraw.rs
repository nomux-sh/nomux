//! The redraw request: the screen state of the holder and the ring buffer
//! position that it corresponds to.

use nomux_holder_protocol::wire::{ErrorCode, Reply, Request, WindowSize};
use nomux_term::{Attributes, Color, Modes, Position, Size, Terminal};

use crate::fixture::{Client, Holder, runtime_directory};

const ROWS: u16 = 24;
const COLUMNS: u16 = 80;

/// Draws on the primary screen and on the alternate screen, puts the cursor
/// at row 3 and column 7, then waits.
const DRAW_KNOWN_SCREEN: &str = concat!(
    r"printf 'primary \033[1;31mred\033[0m\n';",
    r"printf '\033[?1049h\033[2;3H\033[38;2;10;20;30;48;5;200mcolour\033[0m';",
    r"printf '\033[20;1Hbottom row\033[3;7H';",
    "read line",
);
const END_OF_KNOWN_SCREEN: &[u8] = b"\x1b[3;7H";

/// Writes an escape sequence and a UTF-8 character in parts, and waits for
/// a line after each part. The terminal does not echo the lines.
const WRITE_IN_PARTS: &str = concat!(
    "stty -echo;",
    r"printf 'abc\033[3'; read line;",
    r"printf '1mred\342\202'; read line;",
    r"printf '\254'; read line",
);

/// Waits for a line, then writes 20000 lines in changing colours. The lines
/// go to the terminal in large blocks, thus the holder reads chunks that
/// end at each possible place of a line.
const WRITE_STREAM: &str = concat!(
    "read go;",
    r#"awk 'BEGIN { for (i = 0; i < 20000; i++) printf "\033[3%dm%d\033[0m line\n", i % 8, i }'"#,
    " | cat;",
    "printf end-of-stream; read line",
);
const END_OF_STREAM: &[u8] = b"end-of-stream";
/// Holds all the output of `WRITE_STREAM`.
const STREAM_RING_CAPACITY: &str = "1048576";

fn terminal(rows: u16, columns: u16) -> Terminal {
    Terminal::new(Size::new(rows, columns).expect("the size is not zero"))
}

/// A new terminal that got `output`.
fn terminal_with(output: &[u8]) -> Terminal {
    let mut terminal = terminal(ROWS, COLUMNS);
    terminal.feed(output, |_| {});
    terminal
}

fn assert_same_screen(left: &Terminal, right: &Terminal) {
    let (left, right) = (left.screen(), right.screen());
    assert_eq!(left.size(), right.size());
    for row in 0..left.size().rows() {
        for column in 0..left.size().columns() {
            let position = Position { row, column };
            assert_eq!(left.cell(position), right.cell(position), "{position:?}");
        }
    }
    assert_eq!(left.cursor(), right.cursor());
    assert_eq!(left.modes(), right.modes());
}

#[test]
fn the_redraw_makes_the_screen_that_the_program_drew() {
    let runtime = runtime_directory();
    let command = ["/bin/sh", "-c", DRAW_KNOWN_SCREEN];
    let _holder = Holder::start(runtime.path(), "drawn", &[], &command);
    let mut client = Client::connect(runtime.path());
    client.read_until_end(END_OF_KNOWN_SCREEN);

    let (position, sequence) = client.redraw();
    let mut copy = terminal_with(&sequence);

    assert_eq!(position, client.output.len() as u64);
    assert_same_screen(&copy, &terminal_with(&client.output));
    let screen = copy.screen();
    assert!(screen.modes().contains(Modes::ALTERNATE_SCREEN));
    assert_eq!(screen.cursor().position, Position { row: 2, column: 6 });
    let coloured = screen.cell(Position { row: 1, column: 2 }).unwrap();
    assert_eq!(coloured.character, 'c');
    assert_eq!(coloured.style.foreground, Color::Rgb(10, 20, 30));
    assert_eq!(coloured.style.background, Color::Indexed(200));

    // The sequence also has the screen behind the alternate screen.
    copy.feed(b"\x1b[?1049l", |_| {});
    let red = copy.screen().cell(Position { row: 0, column: 8 }).unwrap();
    assert_eq!(red.character, 'r');
    assert_eq!(red.style.foreground, Color::Indexed(1));
    assert_eq!(red.style.attributes, Attributes::BOLD);
}

#[test]
fn the_position_of_a_redraw_is_the_start_of_the_sequence_that_is_not_complete() {
    let runtime = runtime_directory();
    let command = ["/bin/sh", "-c", WRITE_IN_PARTS];
    let _holder = Holder::start(runtime.path(), "parts", &[], &command);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);

    client.read_until_end(b"abc\x1b[3");
    let (in_escape_sequence, first_sequence) = client.redraw();
    client.type_line("");
    client.read_until_end(b"red\xe2\x82");
    let (in_character, second_sequence) = client.redraw();
    client.type_line("");
    client.read_until_end(b"\xac");

    let output = client.output.clone();
    assert_eq!(output, "abc\x1b[31mred\u{20ac}".as_bytes());
    assert_eq!(in_escape_sequence, 3);
    assert_eq!(in_character, 11);
    // A client that attached at each redraw has the complete sequence and
    // the complete character.
    for (position, sequence) in [(3, first_sequence), (11, second_sequence)] {
        let mut attached = terminal_with(&sequence);
        attached.feed(&output[position..], |_| {});
        assert_same_screen(&attached, &terminal_with(&output));
        let red = attached
            .screen()
            .cell(Position { row: 0, column: 3 })
            .unwrap();
        assert_eq!(
            (red.character, red.style.foreground),
            ('r', Color::Indexed(1))
        );
        let euro = attached
            .screen()
            .cell(Position { row: 0, column: 6 })
            .unwrap();
        assert_eq!(euro.character, '\u{20ac}');
    }
}

#[test]
fn the_screen_state_gets_the_size_of_a_resize() {
    let runtime = runtime_directory();
    let command = ["/bin/sh", "-c", DRAW_KNOWN_SCREEN];
    let _holder = Holder::start(runtime.path(), "resized", &[], &command);
    let mut client = Client::connect(runtime.path());
    client.read_until_end(END_OF_KNOWN_SCREEN);
    let (rows, columns) = (10, 40);
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);

    let resized = client.request(Request::Resize(WindowSize { rows, columns }));
    let (_, sequence) = client.redraw();

    assert_eq!(resized, Reply::Done);
    let mut expected = terminal_with(&client.output);
    expected.resize(Size::new(rows, columns).unwrap());
    let mut copy = terminal(rows, columns);
    copy.feed(&sequence, |_| {});
    assert_same_screen(&copy, &expected);
}

#[test]
fn a_window_with_more_cells_than_the_limit_is_refused() {
    let runtime = runtime_directory();
    let _holder = Holder::start(runtime.path(), "large", &[], &["/bin/sh"]);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    let too_large = WindowSize {
        rows: 2000,
        columns: 2000,
    };
    assert!(too_large.cells() > WindowSize::MAX_CELLS);

    assert_eq!(
        client.request(Request::Resize(too_large)),
        Reply::Error(ErrorCode::InvalidPayload)
    );

    client.type_line("stty size");
    client.read_until("24 80");
}

/// The screen state cannot keep a wide character in 1 column.
#[test]
fn a_window_of_1_column_is_refused() {
    let runtime = runtime_directory();
    let _holder = Holder::start(runtime.path(), "narrow", &[], &["/bin/sh"]);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    let too_narrow = WindowSize {
        rows: ROWS,
        columns: Size::MIN_COLUMNS - 1,
    };

    assert_eq!(
        client.request(Request::Resize(too_narrow)),
        Reply::Error(ErrorCode::InvalidPayload)
    );

    client.type_line("stty size");
    client.read_until("24 80");
}

/// One redraw reply.
struct Snapshot {
    position: usize,
    sequence: Vec<u8>,
}

#[test]
fn a_redraw_during_output_corresponds_exactly_to_its_position() {
    let runtime = runtime_directory();
    let options = ["--ring-capacity", STREAM_RING_CAPACITY];
    let command = ["/bin/sh", "-c", WRITE_STREAM];
    let _holder = Holder::start(runtime.path(), "stream", &options, &command);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.ring.position(), 0);
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);

    // The requests are served while the program writes.
    client.type_line("go");
    let mut snapshots = Vec::new();
    while !client.output.ends_with(END_OF_STREAM) {
        let (position, sequence) = client.redraw();
        snapshots.push(Snapshot {
            position: usize::try_from(position).unwrap(),
            sequence,
        });
        client.read_ring();
    }
    let output = client.output.clone();
    let in_a_line = |snapshot: &&Snapshot| {
        (1..output.len()).contains(&snapshot.position) && output[snapshot.position - 1] != b'\n'
    };
    assert!(snapshots.iter().any(|snapshot| in_a_line(&snapshot)));

    // Each sequence makes the screen of the output before its position, and
    // no sequence is cut at the position.
    let mut screen_at_position = terminal(ROWS, COLUMNS);
    let mut fed = 0;
    for snapshot in &snapshots {
        screen_at_position.feed(&output[fed..snapshot.position], |_| {});
        fed = snapshot.position;
        assert_same_screen(&terminal_with(&snapshot.sequence), &screen_at_position);
        assert_eq!(screen_at_position.incomplete_tail(), 0);
    }

    // Clients that attach during the output: the redraw, then the ring
    // buffer from the position, give the output with no gap and no repeat.
    let during_output = 1000..output.len() - 1000;
    let attached_during_output: Vec<&Snapshot> = snapshots
        .iter()
        .filter(|snapshot| during_output.contains(&snapshot.position))
        .collect();
    assert!(!attached_during_output.is_empty());
    let expected = terminal_with(&output);
    let step = attached_during_output.len().div_ceil(8);
    for snapshot in attached_during_output.into_iter().step_by(step) {
        client.ring.seek(snapshot.position as u64);
        client.output.clear();
        client.read_ring();
        assert_eq!(client.output, output[snapshot.position..]);
        let mut attached = terminal_with(&snapshot.sequence);
        attached.feed(&client.output, |_| {});
        assert_same_screen(&attached, &expected);
    }
}
