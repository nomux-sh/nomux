//! Benchmark probe for `alacritty_terminal`.

use std::process::ExitCode;

use alacritty_terminal::Term;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::Config;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::Processor;
use vt_probe::{Parser, Position, Screen, Size};

struct AlacrittyTerminal {
    term: Term<VoidListener>,
    processor: Processor,
}

impl Parser for AlacrittyTerminal {
    const NAME: &'static str = "alacritty_terminal 0.26.0";

    fn new(size: Size, scrollback_lines: usize) -> Self {
        let config = Config {
            scrolling_history: scrollback_lines,
            ..Config::default()
        };
        let size = TermSize::new(usize::from(size.columns), usize::from(size.rows));
        Self {
            term: Term::new(config, &size, VoidListener),
            processor: Processor::new(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    fn retained_scrollback_lines(&self) -> usize {
        self.term.grid().history_size()
    }
}

impl Screen for AlacrittyTerminal {
    fn row_text(&self, row: u16) -> String {
        let grid = self.term.grid();
        let cells = &grid[Line(i32::from(row))];
        let mut text = String::new();
        for cell in (0..grid.columns()).map(|column| &cells[Column(column)]) {
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            // The grid keeps a tab as a tab character in the first cell that it covers.
            text.push(if cell.c == '\t' { ' ' } else { cell.c });
            text.extend(cell.zerowidth().unwrap_or_default());
        }
        text
    }

    fn cursor(&self) -> Position {
        let point = self.term.grid().cursor.point;
        Position {
            row: u16::try_from(point.line.0).unwrap_or(u16::MAX),
            column: u16::try_from(point.column.0).unwrap_or(u16::MAX),
        }
    }
}

fn main() -> ExitCode {
    vt_probe::run_screen::<AlacrittyTerminal>()
}
