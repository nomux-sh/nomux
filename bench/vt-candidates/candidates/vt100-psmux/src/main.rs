//! Benchmark probe for `vt100-psmux`, the fork of the `vt100` crate that the
//! psmux project keeps.

use std::process::ExitCode;

use vt_probe::{Parser, Position, Screen, Size};

struct Vt100Psmux(vt100_psmux::Parser);

impl Parser for Vt100Psmux {
    const NAME: &'static str = "vt100-psmux 0.16.13";

    fn new(size: Size, scrollback_lines: usize) -> Self {
        Self(vt100_psmux::Parser::new(
            size.rows,
            size.columns,
            scrollback_lines,
        ))
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.0.process(bytes);
    }

    fn retained_scrollback_lines(&self) -> usize {
        self.0.screen().scrollback_len()
    }
}

impl Screen for Vt100Psmux {
    fn row_text(&self, row: u16) -> String {
        let screen = self.0.screen();
        let (_, columns) = screen.size();
        screen
            .rows(0, columns)
            .nth(usize::from(row))
            .unwrap_or_default()
    }

    fn cursor(&self) -> Position {
        let (row, column) = self.0.screen().cursor_position();
        Position { row, column }
    }
}

fn main() -> ExitCode {
    vt_probe::run_screen::<Vt100Psmux>()
}
