//! Arbitrary output and resizes go into a terminal.
//!
//! The terminal must not panic, and after each step its interface must give
//! a state that is possible: the cursor is on the screen, each cell of the
//! screen can be read, and the incomplete tail is not longer than the output.
//! The target also gives each OSC sequence that the terminal reports to the
//! parsers of `nomux_term::osc`.
//! libFuzzer stops the run if the process uses more memory than its limit.

#![no_main]

use libfuzzer_sys::fuzz_target;
use nomux_fuzz::terminal_script::{self, Step, TerminalScript};
use nomux_term::{Position, Terminal};

/// `Terminal::feed` ignores an OSC sequence that has more bytes.
const MAX_OSC_BYTES: usize = 4096;

fuzz_target!(|input: &[u8]| {
    let mut terminal = terminal_script::new_terminal();
    let mut output_length = 0;
    for step in TerminalScript::new(input) {
        step.apply(&mut terminal, |osc| {
            assert!(osc.payload.len() <= MAX_OSC_BYTES);
            // The parsers of the program status, the prompt marks and the
            // notifications must not panic.
            let _record = osc.record();
        });
        if let Step::Feed(output) = step {
            output_length += output.len();
        }
        assert!(terminal.incomplete_tail() <= output_length);
        assert_screen_is_readable(&terminal);
    }
});

fn assert_screen_is_readable(terminal: &Terminal) {
    let screen = terminal.screen();
    let size = screen.size();
    let cursor = screen.cursor().position;
    assert!(
        cursor.row < size.rows() && cursor.column < size.columns(),
        "the cursor {cursor:?} is outside {size:?}"
    );
    for row in 0..size.rows() {
        for column in 0..size.columns() {
            let position = Position { row, column };
            assert!(screen.cell(position).is_some(), "no cell at {position:?}");
        }
    }
    let below = Position {
        row: size.rows(),
        column: 0,
    };
    assert_eq!(screen.cell(below), None);
}
