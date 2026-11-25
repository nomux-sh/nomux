//! Memory of one terminal, for a measurement from outside the process.

use std::io::{self, Read, Write};

use crate::candidate::{Parser, Size};

const SIZE: Size = Size {
    rows: 24,
    columns: 80,
};

/// Fills the screen and `scrollback_lines` lines of scrollback, prints
/// `ready` with the number of scrollback lines that the candidate keeps, and
/// keeps the terminal alive until the standard input closes.
pub(crate) fn hold<P: Parser>(scrollback_lines: usize) -> io::Result<()> {
    let mut parser = P::new(SIZE, scrollback_lines);
    for line in 0..scrollback_lines + usize::from(SIZE.rows) {
        parser.feed(colored_line(line).as_bytes());
    }
    println!("ready {}", parser.retained_scrollback_lines());
    io::stdout().flush()?;
    io::stdin().read_to_end(&mut Vec::new())?;
    drop(parser);
    Ok(())
}

/// One line of log output that fills all columns and has one color.
fn colored_line(line: usize) -> String {
    let text = format!("{line:08} the quick brown fox jumps over the lazy dog ");
    let columns = usize::from(SIZE.columns);
    let filled: String = text.chars().cycle().take(columns).collect();
    format!("\x1b[3{}m{filled}\x1b[0m\r\n", line % 8)
}
