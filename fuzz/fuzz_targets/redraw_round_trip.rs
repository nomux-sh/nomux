//! The contract of attach: after arbitrary output and resizes, the redraw
//! sequence makes the same visible screen in a new terminal.
//!
//! The visible screen is what `Screen` gives: the cells, the cursor and the
//! modes. The redraw sequence must also be complete, because the daemon sends
//! the output of the program after it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use nomux_fuzz::terminal_script::{self, TerminalScript};
use nomux_term::{CellWidth, Position, Terminal};

fuzz_target!(|input: &[u8]| {
    let mut source = terminal_script::new_terminal();
    for step in TerminalScript::new(input) {
        step.apply(&mut source, |_| {});
    }
    if has_split_wide_character(&source) {
        return;
    }

    let mut redraw = Vec::new();
    source.write_redraw(&mut redraw);
    let mut copy = Terminal::new(source.screen().size());
    copy.feed(&redraw, |_| {});

    assert_eq!(copy.incomplete_tail(), 0, "the redraw sequence is cut");
    if cursor_moved_right(&source, &copy) {
        return;
    }
    assert_same_visible_screen(&source, &copy);
});

fn assert_same_visible_screen(source: &Terminal, copy: &Terminal) {
    let (source, copy) = (source.screen(), copy.screen());
    for row in 0..source.size().rows() {
        for column in 0..source.size().columns() {
            let position = Position { row, column };
            assert_eq!(copy.cell(position), source.cell(position), "{position:?}");
        }
    }
    assert_eq!(copy.cursor(), source.cursor());
    assert_eq!(copy.modes(), source.modes());
}

/// `true` if the cursor of the copy is at the right of the cursor of the
/// source in the same row. The redraw sequence writes the character at the
/// cursor a second time when the next character goes to the next row, and
/// that is correct only in the last column:
/// <https://github.com/nomux-sh/nomux/issues/27>. Until it is correct, the
/// target does not compare these screens.
fn cursor_moved_right(source: &Terminal, copy: &Terminal) -> bool {
    let source = source.screen().cursor().position;
    let copy = copy.screen().cursor().position;
    copy.row == source.row && copy.column > source.column
}

/// `true` if the screen has one cell of a wide character without the other
/// cell. The redraw sequence does not make such a row:
/// <https://github.com/nomux-sh/nomux/issues/25>. Until it does, the target
/// does not compare these screens.
fn has_split_wide_character(terminal: &Terminal) -> bool {
    let screen = terminal.screen();
    let width = |row, column| {
        let cell = screen.cell(Position { row, column });
        cell.map(|cell| cell.width)
    };
    (0..screen.size().rows()).any(|row| {
        (0..screen.size().columns()).any(|column| match width(row, column) {
            Some(CellWidth::Double) => width(row, column + 1) != Some(CellWidth::Continuation),
            Some(CellWidth::Continuation) => {
                column == 0 || width(row, column - 1) != Some(CellWidth::Double)
            }
            _ => false,
        })
    })
}
