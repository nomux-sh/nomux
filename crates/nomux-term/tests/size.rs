//! The sizes that a terminal can have.

use nomux_term::{CellWidth, Position, Size, Terminal};

const WIDE_CHARACTER: &str = "日";

/// The VT library cannot keep a wide character in 1 column: a resize to
/// that size does not return, and a wide character in that size panics.
#[test]
fn a_size_of_1_column_is_refused() {
    assert_eq!(Size::new(1, Size::MIN_COLUMNS - 1), None);
    assert_eq!(Size::new(0, Size::MIN_COLUMNS), None);
    assert!(Size::new(1, Size::MIN_COLUMNS).is_some());
}

#[test]
fn the_smallest_terminal_takes_a_wide_character() {
    let smallest = Size::new(1, Size::MIN_COLUMNS).unwrap();
    let first = Position { row: 0, column: 0 };

    let mut resized = Terminal::new(Size::new(6, 20).unwrap());
    resized.feed(WIDE_CHARACTER.as_bytes(), |_| {});
    resized.resize(smallest);
    let cell = resized.screen().cell(first).unwrap();
    assert_eq!(cell.width, CellWidth::Double);

    let mut written = Terminal::new(smallest);
    written.feed(format!("{WIDE_CHARACTER}x").as_bytes(), |_| {});
    let cell = written.screen().cell(first).unwrap();
    assert_eq!(cell.character, 'x');
}
