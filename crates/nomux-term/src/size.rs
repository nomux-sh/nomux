//! The size of a terminal.

use std::num::NonZeroU16;

/// The size of a terminal in cells: 1 row or more, and
/// [`Size::MIN_COLUMNS`] columns or more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    rows: NonZeroU16,
    columns: NonZeroU16,
}

impl Size {
    /// The smallest number of columns. A wide character uses 2 columns, and
    /// the VT library cannot keep one in a screen of 1 column.
    pub const MIN_COLUMNS: u16 = 2;

    /// Returns `None` if `rows` is zero or if `columns` is smaller than
    /// [`Size::MIN_COLUMNS`].
    #[must_use]
    pub fn new(rows: u16, columns: u16) -> Option<Self> {
        if columns < Self::MIN_COLUMNS {
            return None;
        }
        Some(Self {
            rows: NonZeroU16::new(rows)?,
            columns: NonZeroU16::new(columns)?,
        })
    }

    /// The number of rows.
    #[must_use]
    pub fn rows(self) -> u16 {
        self.rows.get()
    }

    /// The number of columns.
    #[must_use]
    pub fn columns(self) -> u16 {
        self.columns.get()
    }
}
