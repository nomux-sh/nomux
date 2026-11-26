//! The history: the rows that scroll out above the screen.

use std::fmt::Write as _;

use nomux_term::{Position, Screen, Size, Terminal};

const ROWS: u16 = 5;
const COLUMNS: u16 = 20;

fn terminal_with_history(history_rows: usize) -> Terminal {
    let size = Size::new(ROWS, COLUMNS).expect("the size is not zero");
    Terminal::with_history(size, history_rows)
}

fn numbered_lines(count: usize) -> String {
    let mut lines = String::new();
    for number in 1..=count {
        let _ = write!(lines, "line {number}\r\n");
    }
    lines
}

fn history_text(screen: &Screen<'_>, rows_above: usize) -> String {
    let columns = screen.size().columns();
    let text: String = (0..columns)
        .filter_map(|column| screen.history_cell(rows_above, column))
        .map(|cell| cell.character)
        .collect();
    text.trim_end().to_owned()
}

fn screen_text(screen: &Screen<'_>, row: u16) -> String {
    let text: String = (0..screen.size().columns())
        .filter_map(|column| screen.cell(Position { row, column }))
        .map(|cell| cell.character)
        .collect();
    text.trim_end().to_owned()
}

#[test]
fn a_new_terminal_keeps_no_history() {
    let mut terminal = Terminal::new(Size::new(ROWS, COLUMNS).unwrap());
    terminal.feed(numbered_lines(500).as_bytes(), |_| {});
    let screen = terminal.screen();
    assert_eq!(screen.history_rows(), 0);
    assert!(screen.history_cell(1, 0).is_none());
}

#[test]
fn history_has_the_rows_above_the_screen_after_500_lines() {
    let mut terminal = terminal_with_history(1000);
    terminal.feed(numbered_lines(500).as_bytes(), |_| {});
    let screen = terminal.screen();

    // 500 lines and the empty row of the cursor: 501 rows, 5 of them on the screen.
    assert_eq!(screen.history_rows(), 496);
    assert_eq!(history_text(&screen, 1), "line 496");
    assert_eq!(history_text(&screen, 496), "line 1");
    assert_eq!(screen_text(&screen, 0), "line 497");
    assert!(screen.history_cell(0, 0).is_none());
    assert!(screen.history_cell(497, 0).is_none());
    assert!(screen.history_cell(1, COLUMNS).is_none());
}

#[test]
fn a_full_history_removes_the_oldest_row() {
    let mut terminal = terminal_with_history(100);
    terminal.feed(numbered_lines(500).as_bytes(), |_| {});
    let screen = terminal.screen();
    assert_eq!(screen.history_rows(), 100);
    assert_eq!(history_text(&screen, 100), "line 397");
    assert_eq!(history_text(&screen, 1), "line 496");
}

#[test]
fn history_cells_have_the_style_of_the_output() {
    let mut terminal = terminal_with_history(10);
    terminal.feed(b"\x1b[1mbold\x1b[0m\r\n\n\n\n\n", |_| {});
    let cell = terminal.screen().history_cell(1, 0).unwrap();
    assert_eq!(cell.character, 'b');
    assert!(cell.style.attributes.contains(nomux_term::Attributes::BOLD));
}

#[test]
fn a_narrower_screen_wraps_the_history_and_a_wider_screen_joins_it() {
    let mut terminal = terminal_with_history(1000);
    let long_line = "abcdefghijklmnopqrstuvwxyz0123";
    terminal.feed(format!("{long_line}\r\n").as_bytes(), |_| {});
    terminal.feed(numbered_lines(500).as_bytes(), |_| {});
    let before = terminal.screen().history_rows();
    assert_eq!(
        history_text(&terminal.screen(), before),
        "abcdefghijklmnopqrst"
    );

    terminal.resize(Size::new(ROWS, 10).unwrap());
    let screen = terminal.screen();
    let narrow = screen.history_rows();
    assert!(narrow > before, "the long row wraps into more rows");
    assert_eq!(history_text(&screen, narrow), "abcdefghij");
    assert_eq!(history_text(&screen, narrow - 1), "klmnopqrst");
    assert_eq!(history_text(&screen, narrow - 2), "uvwxyz0123");

    terminal.resize(Size::new(ROWS, 40).unwrap());
    let screen = terminal.screen();
    let wide = screen.history_rows();
    assert_eq!(history_text(&screen, wide), long_line);
    assert_eq!(history_text(&screen, wide - 1), "line 1");
}

#[test]
fn fewer_rows_move_rows_into_the_history_and_more_rows_bring_them_back() {
    let mut terminal = terminal_with_history(1000);
    terminal.feed(numbered_lines(500).as_bytes(), |_| {});

    terminal.resize(Size::new(2, COLUMNS).unwrap());
    let screen = terminal.screen();
    assert_eq!(screen.history_rows(), 499);
    assert_eq!(history_text(&screen, 1), "line 499");
    assert_eq!(screen_text(&screen, 0), "line 500");

    terminal.resize(Size::new(10, COLUMNS).unwrap());
    let screen = terminal.screen();
    assert_eq!(screen.history_rows(), 491);
    assert_eq!(screen_text(&screen, 0), "line 492");
    assert_eq!(screen.cursor().position.row, 9);
}

#[test]
fn the_alternate_screen_has_no_history_and_the_history_stays_behind_it() {
    let mut terminal = terminal_with_history(1000);
    terminal.feed(numbered_lines(500).as_bytes(), |_| {});

    terminal.feed(b"\x1b[?1049h", |_| {});
    terminal.feed(numbered_lines(50).as_bytes(), |_| {});
    assert_eq!(terminal.screen().history_rows(), 0);

    terminal.feed(b"\x1b[?1049l", |_| {});
    let screen = terminal.screen();
    assert_eq!(screen.history_rows(), 496);
    assert_eq!(history_text(&screen, 1), "line 496");
}

#[test]
fn erase_of_the_saved_lines_clears_the_history() {
    let mut terminal = terminal_with_history(1000);
    terminal.feed(numbered_lines(500).as_bytes(), |_| {});
    terminal.feed(b"\x1b[3J", |_| {});
    let screen = terminal.screen();
    assert_eq!(screen.history_rows(), 0);
    assert_eq!(screen_text(&screen, 0), "line 497");
}
