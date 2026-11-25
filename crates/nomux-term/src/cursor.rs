//! The cursor of a screen.

/// A cell position. Row 0 is the top row and column 0 is the left column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// The row.
    pub row: u16,
    /// The column.
    pub column: u16,
}

/// The shape of the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// A block that covers the cell.
    #[default]
    Block,
    /// A line below the cell.
    Underline,
    /// A vertical line at the left of the cell.
    Bar,
}

/// The cursor of the visible screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// The cell of the cursor.
    pub position: Position,
    /// `false` if the program hides the cursor.
    pub visible: bool,
    /// The shape that the program selected.
    pub shape: CursorShape,
    /// `true` if the program selected a blinking cursor.
    pub blinking: bool,
}
