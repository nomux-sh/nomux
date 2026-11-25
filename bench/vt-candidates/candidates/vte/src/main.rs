//! Benchmark probe for the `vte` parser.
//!
//! `vte` keeps no screen state. The probe shows the cost of the parser that
//! `vt100`, the psmux fork and `alacritty_terminal` are built on.

use std::hint::black_box;
use std::process::ExitCode;

use vt_probe::{Parser, Size};

struct Vte {
    parser: vte::Parser,
    printed: PrintedCharacters,
}

/// Counts the printed characters, thus the compiler cannot remove the parser.
struct PrintedCharacters(u64);

impl vte::Perform for PrintedCharacters {
    fn print(&mut self, character: char) {
        self.0 += u64::from(black_box(character) != '\0');
    }
}

impl Parser for Vte {
    const NAME: &'static str = "vte 0.15.0 (parser only)";

    fn new(_size: Size, _scrollback_lines: usize) -> Self {
        Self {
            parser: vte::Parser::new(),
            printed: PrintedCharacters(0),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.printed, bytes);
    }

    fn retained_scrollback_lines(&self) -> usize {
        0
    }
}

fn main() -> ExitCode {
    vt_probe::run_parser::<Vte>()
}
