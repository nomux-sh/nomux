//! Benchmark probe for the `vt100` crate.

use std::num::NonZeroU16;
use std::process::ExitCode;

use vt_probe::{Parser, Position, Screen, Size};

struct Vt100(vt100::Parser);

impl Parser for Vt100 {
    const NAME: &'static str = "vt100 0.18.1";

    fn new(size: Size, scrollback_lines: usize) -> Self {
        let rows = NonZeroU16::new(size.rows).unwrap_or(NonZeroU16::MIN);
        let columns = NonZeroU16::new(size.columns).unwrap_or(NonZeroU16::MIN);
        Self(vt100::Parser::new(rows, columns, scrollback_lines))
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.0.process(bytes);
    }

    fn retained_scrollback_lines(&self) -> usize {
        self.0.screen().scrollback_len()
    }
}

impl Screen for Vt100 {
    fn row_text(&self, row: u16) -> String {
        let screen = self.0.screen();
        let (_, columns) = screen.size();
        screen
            .rows(0, columns.get())
            .nth(usize::from(row))
            .unwrap_or_default()
    }

    fn cursor(&self) -> Position {
        let (row, column) = self.0.screen().cursor_position();
        Position { row, column }
    }
}

fn main() -> ExitCode {
    vt_probe::run_screen::<Vt100>()
}
