//! The contract of attach: after arbitrary output and resizes, the redraw
//! sequence makes the same visible screen in a new terminal.
//!
//! The visible screen is what `Screen` gives: the cells, the cursor and the
//! modes. The redraw sequence must also be complete, because the daemon sends
//! the output of the program after it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use nomux_fuzz::terminal_script::{self, TerminalScript};
use nomux_term::{Position, Terminal};

fuzz_target!(|input: &[u8]| {
    let mut source = terminal_script::new_terminal();
    for step in TerminalScript::new(input) {
        step.apply(&mut source, |_| {});
    }

    let mut redraw = Vec::new();
    source.write_redraw(&mut redraw);
    let mut copy = Terminal::new(source.screen().size());
    copy.feed(&redraw, |_| {});

    assert_eq!(copy.incomplete_tail(), 0, "the redraw sequence is cut");
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
