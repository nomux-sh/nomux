//! The contract of `Terminal::write_redraw`: the sequence makes the same
//! screen in a new terminal.

use nomux_term::{
    Attributes, CellWidth, Color, CursorShape, Modes, Position, Size, Style, Terminal, Underline,
};

const ROWS: u16 = 5;
const COLUMNS: u16 = 20;

fn new_terminal() -> Terminal {
    Terminal::new(Size::new(ROWS, COLUMNS).expect("the size is not zero"))
}

fn terminal_with(output: &str) -> Terminal {
    let mut terminal = new_terminal();
    terminal.feed(output.as_bytes(), |_| {});
    terminal
}

fn redraw_of(terminal: &mut Terminal) -> Vec<u8> {
    let mut redraw = Vec::new();
    terminal.write_redraw(&mut redraw);
    redraw
}

/// A new terminal that got the redraw sequence of `source`.
fn copy_of(source: &mut Terminal) -> Terminal {
    let mut copy = Terminal::new(source.screen().size());
    copy.feed(&redraw_of(source), |_| {});
    copy
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

fn character_at(terminal: &Terminal, row: u16, column: u16) -> char {
    let cell = terminal.screen().cell(Position { row, column });
    cell.expect("the position is on the screen").character
}

#[test]
fn plain_text_is_the_same_in_the_copy() {
    let mut source = terminal_with("first line\r\n\r\n  third line");
    let copy = copy_of(&mut source);

    assert_same_screen(&source, &copy);
    assert_eq!(character_at(&copy, 0, 0), 'f');
    assert_eq!(character_at(&copy, 2, 2), 't');
}

#[test]
fn colors_and_attributes_are_the_same_in_the_copy() {
    let mut source = terminal_with(concat!(
        "\x1b[1;3;31mA\x1b[0;2;7;38;5;200mB\x1b[0;8;9;38;2;1;2;3;48;2;4;5;6mC",
        "\x1b[0;4mD\x1b[0;4:2mE\x1b[0;4:3;58;2;7;8;9mF\x1b[0;4:4;58;5;10mG\x1b[0;4:5mH",
        // Blank cells with a background color at the end of the row.
        "\x1b[0;44m   \x1b[0m",
    ));
    let copy = copy_of(&mut source);

    assert_same_screen(&source, &copy);
    let style = |column| {
        let cell = copy.screen().cell(Position { row: 0, column }).unwrap();
        cell.style
    };
    assert_eq!(style(0).attributes, Attributes::BOLD | Attributes::ITALIC);
    assert_eq!(style(0).foreground, Color::Indexed(1));
    assert_eq!(style(1).attributes, Attributes::DIM | Attributes::INVERSE);
    assert_eq!(style(1).foreground, Color::Indexed(200));
    assert_eq!(
        style(2).attributes,
        Attributes::HIDDEN | Attributes::STRIKETHROUGH
    );
    assert_eq!(style(2).foreground, Color::Rgb(1, 2, 3));
    assert_eq!(style(2).background, Color::Rgb(4, 5, 6));
    assert_eq!(style(3).underline, Underline::Single);
    assert_eq!(style(4).underline, Underline::Double);
    assert_eq!(style(5).underline, Underline::Curly);
    assert_eq!(style(5).underline_color, Color::Rgb(7, 8, 9));
    assert_eq!(style(6).underline, Underline::Dotted);
    assert_eq!(style(6).underline_color, Color::Indexed(10));
    assert_eq!(style(7).underline, Underline::Dashed);
    assert_eq!(style(10).background, Color::Indexed(4));
}

#[test]
fn wide_and_combining_characters_are_the_same_in_the_copy() {
    let mut source = terminal_with("日本e\u{301}x");
    let copy = copy_of(&mut source);

    assert_same_screen(&source, &copy);
    assert_eq!(character_at(&copy, 0, 0), '日');
    let accented = copy.screen().cell(Position { row: 0, column: 4 }).unwrap();
    assert_eq!(accented.combining, ['\u{301}']);
}

#[test]
fn cursor_position_and_look_are_the_same_in_the_copy() {
    let mut source = terminal_with("text\x1b[3;7H\x1b[?25l\x1b[5 q");
    let copy = copy_of(&mut source);

    assert_same_screen(&source, &copy);
    let cursor = copy.screen().cursor();
    assert_eq!(cursor.position, Position { row: 2, column: 6 });
    assert!(!cursor.visible);
    assert_eq!((cursor.shape, cursor.blinking), (CursorShape::Bar, true));
}

#[test]
fn next_character_wraps_in_the_copy_after_a_full_row() {
    let full_row = "a".repeat(usize::from(COLUMNS));
    let mut source = terminal_with(&full_row);
    let mut copy = copy_of(&mut source);

    source.feed(b"b", |_| {});
    copy.feed(b"b", |_| {});

    assert_same_screen(&source, &copy);
    assert_eq!(character_at(&copy, 1, 0), 'b');
}

/// Backward tab moves the cursor away from the last column, and a wider
/// alternate screen moves the last column away from the cursor. The VT
/// library keeps the wrap for the next character in the two cases.
#[test]
fn next_character_does_not_wrap_after_the_cursor_left_the_last_column() {
    let full_row = "a".repeat(usize::from(COLUMNS));
    let mut after_backward_tab = terminal_with(&format!("{full_row}\x1b[Z"));
    let mut after_wider_screen = terminal_with(&format!("\x1b[?1049h{full_row}"));
    after_wider_screen.resize(Size::new(ROWS, COLUMNS + 10).unwrap());

    for (source, column) in [(&mut after_backward_tab, 16), (&mut after_wider_screen, 19)] {
        let mut copy = copy_of(source);
        assert_same_screen(source, &copy);

        source.feed(b"b", |_| {});
        copy.feed(b"b", |_| {});

        assert_same_screen(source, &copy);
        assert_eq!(character_at(&copy, 0, column), 'b');
    }
}

/// Delete character on one cell of a wide character leaves the other cell.
#[test]
fn one_cell_of_a_wide_character_is_an_empty_cell_with_its_style() {
    let red_background = Style {
        background: Color::Indexed(1),
        ..Style::default()
    };
    for delete_at in ["\x1b[1;1H", "\x1b[1;2H"] {
        let mut source = terminal_with(&format!("\x1b[41m日\x1b[0mx{delete_at}\x1b[P"));
        let copy = copy_of(&mut source);

        assert_same_screen(&source, &copy);
        let first = Position { row: 0, column: 0 };
        let cell = copy.screen().cell(first).unwrap();
        assert_eq!(
            (cell.character, cell.width, cell.style),
            (' ', CellWidth::Single, red_background)
        );
        assert_eq!(character_at(&copy, 0, 1), 'x');
    }
}

#[test]
fn next_character_has_the_current_style_in_the_copy() {
    let mut source = terminal_with("\x1b[1;32m");
    let mut copy = copy_of(&mut source);

    source.feed(b"x", |_| {});
    copy.feed(b"x", |_| {});

    assert_same_screen(&source, &copy);
    let cell = copy.screen().cell(Position { row: 0, column: 0 }).unwrap();
    assert_eq!(cell.style.attributes, Attributes::BOLD);
    assert_eq!(cell.style.foreground, Color::Indexed(2));
}

#[test]
fn modes_are_the_same_in_the_copy() {
    let mut source = terminal_with(
        "\x1b[?1h\x1b=\x1b[?2004h\x1b[?1004h\x1b[?1002h\x1b[?1006h\x1b[?7l\x1b[4h\x1b[>5u",
    );
    let copy = copy_of(&mut source);

    assert_same_screen(&source, &copy);
    assert_eq!(
        copy.screen().modes(),
        Modes::APPLICATION_CURSOR
            | Modes::APPLICATION_KEYPAD
            | Modes::BRACKETED_PASTE
            | Modes::FOCUS_REPORTING
            | Modes::MOUSE_DRAG
            | Modes::MOUSE_SGR
            | Modes::ALTERNATE_SCROLL
            | Modes::INSERT
            | Modes::KEYBOARD_DISAMBIGUATE
            | Modes::KEYBOARD_ALTERNATE_KEYS
    );
}

#[test]
fn alternate_screen_and_the_screen_behind_it_are_the_same_in_the_copy() {
    let mut source = terminal_with("main\x1b[?1049h\x1b[2;3H\x1b[35malternate");
    let mut copy = copy_of(&mut source);

    assert_same_screen(&source, &copy);
    assert!(copy.screen().modes().contains(Modes::ALTERNATE_SCREEN));
    assert_eq!(character_at(&copy, 1, 2), 'a');

    source.feed(b"\x1b[?1049l", |_| {});
    copy.feed(b"\x1b[?1049l", |_| {});

    assert_same_screen(&source, &copy);
    assert_eq!(character_at(&copy, 0, 0), 'm');
    assert_eq!(
        copy.screen().cursor().position,
        Position { row: 0, column: 4 }
    );
}

#[test]
fn redraw_does_not_change_the_terminal() {
    let mut source = terminal_with("main\x1b[?1049h\x1b[2;3Halternate\x1b[>1u");
    let first = redraw_of(&mut source);
    let second = redraw_of(&mut source);

    assert_eq!(first, second);
    assert_eq!(character_at(&source, 1, 2), 'a');
}

#[test]
fn copy_has_the_new_size_after_a_resize() {
    let mut source = terminal_with("text");
    let larger = Size::new(ROWS + 2, COLUMNS + 10).unwrap();
    source.resize(larger);
    source.feed(b"\x1b[7;28Hend", |_| {});
    let copy = copy_of(&mut source);

    assert_eq!(source.screen().size(), larger);
    assert_same_screen(&source, &copy);
    assert_eq!(character_at(&copy, 6, 29), 'd');
    let outside = Position {
        row: ROWS + 2,
        column: 0,
    };
    assert_eq!(copy.screen().cell(outside), None);
}

#[test]
fn copy_from_each_cut_of_the_output_continues_at_the_incomplete_tail() {
    // Text, colors, cursor addressing, wide and combining characters, OSC,
    // DCS and APC strings, and the alternate screen.
    let output = concat!(
        "first\x1b[1;38;2;10;20;30m bold\x1b[0m\r\n",
        "\x1b]0;title\x07\x1b]133;A\x1b\\$ caf\u{e9} \u{20ac} \u{4e2d}\u{6587} e\u{301}\r\n",
        "\x1b[2;4H\x1b[48;5;200mcell\x1b[m\x1bPq#0~~\x1b\\\x1b_payload\x1b\\",
        "\x1b[?1049h\x1b[3;2H\x1b[4:3;58;2;7;8;9mcurly\x1b[0m \u{1f600}\x1b[?2004h\x1b[5;20Hx",
    )
    .as_bytes();
    let mut complete = new_terminal();
    complete.feed(output, |_| {});

    for cut in 0..=output.len() {
        let mut source = new_terminal();
        source.feed(&output[..cut], |_| {});
        let mut copy = copy_of(&mut source);
        copy.feed(&output[cut - source.incomplete_tail()..], |_| {});

        assert_same_screen(&copy, &complete);
    }
}
