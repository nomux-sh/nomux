//! One cell of a screen.

use bitflags::bitflags;

/// A color of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    /// The default color of the client.
    #[default]
    Default,
    /// A color from the palette of 256 colors.
    Indexed(u8),
    /// A color with red, green and blue values.
    Rgb(u8, u8, u8),
}

bitflags! {
    /// The attributes of a cell that are on or off.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Attributes: u8 {
        /// Bold text.
        const BOLD = 1;
        /// Text with less intensity.
        const DIM = 1 << 1;
        /// Italic text.
        const ITALIC = 1 << 2;
        /// The foreground color and the background color change places.
        const INVERSE = 1 << 3;
        /// The text is not shown.
        const HIDDEN = 1 << 4;
        /// A line through the text.
        const STRIKETHROUGH = 1 << 5;
    }
}

/// The line below the text of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Underline {
    /// No line.
    #[default]
    None,
    /// One straight line.
    Single,
    /// Two straight lines.
    Double,
    /// A wave.
    Curly,
    /// A line of dots.
    Dotted,
    /// A line of dashes.
    Dashed,
}

/// The colors and the attributes of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    /// The color of the text.
    pub foreground: Color,
    /// The color behind the text.
    pub background: Color,
    /// The color of the underline. [`Color::Default`] is the color of the text.
    pub underline_color: Color,
    /// The line below the text.
    pub underline: Underline,
    /// The attributes that are on.
    pub attributes: Attributes,
}

/// The number of columns that the character of a cell uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellWidth {
    /// The character uses this cell only.
    #[default]
    Single,
    /// The character uses this cell and the cell at its right.
    Double,
    /// The cell at the right of a [`CellWidth::Double`] cell. It has no
    /// character of its own. A screen always has the two cells together.
    Continuation,
}

/// One cell of the visible screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell<'a> {
    /// The character. An empty cell has a space.
    pub character: char,
    /// The combining characters that follow [`Cell::character`].
    pub combining: &'a [char],
    /// The number of columns that the character uses.
    pub width: CellWidth,
    /// The colors and the attributes.
    pub style: Style,
}

impl Cell<'_> {
    /// An empty cell with the style of this cell.
    pub(crate) fn without_character(self) -> Self {
        Self {
            character: ' ',
            combining: &[],
            width: CellWidth::Single,
            style: self.style,
        }
    }

    /// `true` if a new, empty screen has the same cell.
    pub(crate) fn is_blank(&self) -> bool {
        self.character == ' '
            && self.combining.is_empty()
            && self.width == CellWidth::Single
            && self.style == Style::default()
    }
}
