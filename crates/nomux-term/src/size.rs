//! The size of a terminal.

use std::num::NonZeroU16;

/// The size of a terminal in cells. The two numbers are not zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    rows: NonZeroU16,
    columns: NonZeroU16,
}

impl Size {
    /// Returns `None` if `rows` or `columns` is zero.
    #[must_use]
    pub fn new(rows: u16, columns: u16) -> Option<Self> {
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
