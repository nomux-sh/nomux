//! The interface between the measurements and a candidate.

/// The size of a terminal in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// The number of rows.
    pub rows: u16,
    /// The number of columns.
    pub columns: u16,
}

/// A cell position. Row 0 is the top row and column 0 is the left column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// The row.
    pub row: u16,
    /// The column.
    pub column: u16,
}

/// A candidate that consumes terminal output.
pub trait Parser {
    /// The crate name and the version of the candidate.
    const NAME: &'static str;

    /// Makes a terminal that keeps at most `scrollback_lines` lines of scrollback.
    fn new(size: Size, scrollback_lines: usize) -> Self;

    /// Gives terminal output to the candidate.
    fn feed(&mut self, bytes: &[u8]);

    /// The number of scrollback lines that the candidate keeps now.
    fn retained_scrollback_lines(&self) -> usize;
}

/// A candidate that also keeps the screen state.
pub trait Screen: Parser {
    /// The text of one row of the visible screen.
    fn row_text(&self, row: u16) -> String;

    /// The cursor position.
    fn cursor(&self) -> Position;
}
