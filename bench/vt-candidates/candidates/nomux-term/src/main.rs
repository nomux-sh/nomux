//! Benchmark probe for `nomux-term`: the selected VT library behind the
//! interface that the holder and the clients use.
//!
//! The difference between this probe and the probe of the VT library is the
//! cost of `nomux-term`. `nomux-term` keeps no scrollback.

use std::hint::black_box;
use std::process::ExitCode;

use nomux_term::{CellWidth, Terminal};
use vt_probe::{Parser, Position, Screen, Size};

struct NomuxTerm(Terminal);

impl Parser for NomuxTerm {
    const NAME: &'static str = "nomux-term (alacritty_terminal 0.26.0)";

    fn new(size: Size, _scrollback_lines: usize) -> Self {
        match nomux_term::Size::new(size.rows, size.columns) {
            Some(size) => Self(Terminal::new(size)),
            None => panic!("the benchmark uses a size that is zero"),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.0.feed(bytes, |osc| {
            black_box(osc);
        });
    }

    fn retained_scrollback_lines(&self) -> usize {
        0
    }
}

impl Screen for NomuxTerm {
    fn row_text(&self, row: u16) -> String {
        let screen = self.0.screen();
        let mut text = String::new();
        let cells = (0..screen.size().columns())
            .filter_map(|column| screen.cell(nomux_term::Position { row, column }));
        for cell in cells.filter(|cell| cell.width != CellWidth::Continuation) {
            text.push(cell.character);
            text.extend(cell.combining);
        }
        text
    }

    fn cursor(&self) -> Position {
        let position = self.0.screen().cursor().position;
        Position {
            row: position.row,
            column: position.column,
        }
    }
}

fn main() -> ExitCode {
    vt_probe::run_screen::<NomuxTerm>()
}
