//! The size of the terminal window.

use super::payload::{DecodeError, PayloadReader};

/// The size of the terminal window in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    /// The number of rows.
    pub rows: u16,
    /// The number of columns.
    pub columns: u16,
}

impl WindowSize {
    /// The largest number of cells of a window. The holder keeps the screen
    /// state in memory, and refuses a window that has more cells.
    pub const MAX_CELLS: u32 = 1 << 20;

    pub(super) const ENCODED_LENGTH: usize = 4;

    /// The number of cells: rows times columns.
    #[must_use]
    pub fn cells(self) -> u32 {
        u32::from(self.rows) * u32::from(self.columns)
    }

    pub(super) fn encode(self) -> [u8; Self::ENCODED_LENGTH] {
        let [rows_low, rows_high] = self.rows.to_le_bytes();
        let [columns_low, columns_high] = self.columns.to_le_bytes();
        [rows_low, rows_high, columns_low, columns_high]
    }

    pub(super) fn decode(reader: &mut PayloadReader<'_>) -> Result<Self, DecodeError> {
        let rows = reader.u16()?;
        let columns = reader.u16()?;
        if rows == 0 || columns == 0 {
            return Err(DecodeError::InvalidPayload);
        }
        Ok(Self { rows, columns })
    }
}
