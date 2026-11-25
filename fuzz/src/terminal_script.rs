//! The input of the terminal targets: the output of a program, with changes
//! of the terminal size between the parts of the output.
//!
//! The input is the output of the program as it is. The byte `0x00` and the
//! two bytes after it are a resize: the first byte selects the rows and the
//! second byte selects the columns. Thus a file with terminal output is a
//! valid input, and a mutation of one byte keeps the other steps.
//!
//! A resize can select 1 column, which `nomux-term` refuses. The terminal
//! then keeps its size, as the terminal of a holder that refuses a `Resize`.

use nomux_term::{Osc, Size, Terminal};

const RESIZE_MARK: u8 = 0x00;

const INITIAL_ROWS: u16 = 6;
const INITIAL_COLUMNS: u16 = 20;

/// The limits of a resize. A small screen keeps a run fast, and the edges of
/// the screen are where the defects are.
const MAX_ROWS: u8 = 32;
const MAX_COLUMNS: u8 = 128;

/// One step of a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step<'input> {
    /// The program writes these bytes.
    Feed(&'input [u8]),
    /// The window gets this size.
    Resize {
        /// The number of rows: 1 or more.
        rows: u16,
        /// The number of columns: 1 or more.
        columns: u16,
    },
}

impl Step<'_> {
    /// Gives the step to `terminal`. `unhandled_osc` is the argument of
    /// [`Terminal::feed`].
    pub fn apply(self, terminal: &mut Terminal, unhandled_osc: impl FnMut(Osc<'_>)) {
        match self {
            Self::Feed(output) => terminal.feed(output, unhandled_osc),
            Self::Resize { rows, columns } => {
                if let Some(size) = Size::new(rows, columns) {
                    terminal.resize(size);
                }
            }
        }
    }
}

/// The steps of one fuzz input.
#[derive(Debug, Clone)]
pub struct TerminalScript<'input> {
    rest: &'input [u8],
}

impl<'input> TerminalScript<'input> {
    /// Reads `input` as a script.
    #[must_use]
    pub fn new(input: &'input [u8]) -> Self {
        Self { rest: input }
    }
}

impl<'input> Iterator for TerminalScript<'input> {
    type Item = Step<'input>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.rest {
            [RESIZE_MARK, rows, columns, rest @ ..] => {
                self.rest = rest;
                Some(Step::Resize {
                    rows: 1 + u16::from(rows % MAX_ROWS),
                    columns: 1 + u16::from(columns % MAX_COLUMNS),
                })
            }
            // The end of the input, or a resize that the end of the input cut.
            [] | [RESIZE_MARK, ..] => None,
            output => {
                let end = output.iter().position(|byte| *byte == RESIZE_MARK);
                let (output, rest) = output.split_at(end.unwrap_or(output.len()));
                self.rest = rest;
                Some(Step::Feed(output))
            }
        }
    }
}

/// The terminal that each script starts with.
#[must_use]
pub fn new_terminal() -> Terminal {
    Terminal::new(initial_size())
}

fn initial_size() -> Size {
    Size::new(INITIAL_ROWS, INITIAL_COLUMNS).expect("a terminal can have the size")
}
