//! The size of a terminal window.

/// The size of a terminal window in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    /// The number of rows.
    pub rows: u16,
    /// The number of columns.
    pub columns: u16,
}
