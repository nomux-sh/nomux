//! The colors with which the terminal answers a color query of a program.

/// A color with 8 bits for each component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    /// The red component.
    pub red: u8,
    /// The green component.
    pub green: u8,
    /// The blue component.
    pub blue: u8,
}

impl Rgb {
    /// Makes a color from its three components.
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

/// The colors of the terminal that shows the session. The terminal answers
/// the color queries (OSC 4, 10, 11 and 12) with them, unless the program
/// set the color itself.
///
/// The default is a dark palette: the 16 colors of xterm, a light gray
/// foreground on a black background, and a cursor in the foreground color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The default color of the text (OSC 10).
    pub foreground: Rgb,
    /// The default color of the background (OSC 11).
    pub background: Rgb,
    /// The color of the cursor (OSC 12).
    pub cursor: Rgb,
    /// The 16 ANSI colors: palette indexes 0 to 15. Indexes 16 to 255 are
    /// the 6 × 6 × 6 color cube and the 24 grays of xterm.
    pub ansi: [Rgb; 16],
}

const XTERM_ANSI: [Rgb; 16] = [
    Rgb::new(0x00, 0x00, 0x00),
    Rgb::new(0xcd, 0x00, 0x00),
    Rgb::new(0x00, 0xcd, 0x00),
    Rgb::new(0xcd, 0xcd, 0x00),
    Rgb::new(0x00, 0x00, 0xee),
    Rgb::new(0xcd, 0x00, 0xcd),
    Rgb::new(0x00, 0xcd, 0xcd),
    Rgb::new(0xe5, 0xe5, 0xe5),
    Rgb::new(0x7f, 0x7f, 0x7f),
    Rgb::new(0xff, 0x00, 0x00),
    Rgb::new(0x00, 0xff, 0x00),
    Rgb::new(0xff, 0xff, 0x00),
    Rgb::new(0x5c, 0x5c, 0xff),
    Rgb::new(0xff, 0x00, 0xff),
    Rgb::new(0x00, 0xff, 0xff),
    Rgb::new(0xff, 0xff, 0xff),
];

impl Default for Palette {
    fn default() -> Self {
        let foreground = XTERM_ANSI[7];
        Self {
            foreground,
            background: XTERM_ANSI[0],
            cursor: foreground,
            ansi: XTERM_ANSI,
        }
    }
}

/// The indexes of the VT library for the named colors, after the 256
/// colors of the palette. The foreground is 256.
pub(crate) const BACKGROUND_INDEX: usize = 257;
pub(crate) const CURSOR_INDEX: usize = 258;

impl Palette {
    /// The color of an index of the VT library: 0 to 255 is the palette, then
    /// the foreground, the background and the cursor. The other named colors
    /// of the library (dim and bright foreground) are the foreground.
    pub(crate) fn color(&self, index: usize) -> Rgb {
        match index {
            0..=15 => self.ansi[index],
            16..=231 => cube_color(index - 16),
            232..=255 => {
                let level = gray_level(index - 232);
                Rgb::new(level, level, level)
            }
            BACKGROUND_INDEX => self.background,
            CURSOR_INDEX => self.cursor,
            _ => self.foreground,
        }
    }
}

fn cube_color(offset: usize) -> Rgb {
    Rgb::new(
        cube_level(offset / 36),
        cube_level(offset / 6 % 6),
        cube_level(offset % 6),
    )
}

fn cube_level(step: usize) -> u8 {
    const LEVELS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    LEVELS[step]
}

fn gray_level(step: usize) -> u8 {
    u8::try_from(8 + 10 * step).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::{BACKGROUND_INDEX, CURSOR_INDEX, Palette, Rgb};

    #[test]
    fn the_indexes_after_the_ansi_colors_are_the_cube_and_the_grays_of_xterm() {
        let palette = Palette::default();
        assert_eq!(palette.color(16), Rgb::new(0, 0, 0));
        assert_eq!(palette.color(196), Rgb::new(0xff, 0, 0));
        assert_eq!(palette.color(231), Rgb::new(0xff, 0xff, 0xff));
        assert_eq!(palette.color(232), Rgb::new(8, 8, 8));
        assert_eq!(palette.color(255), Rgb::new(0xee, 0xee, 0xee));
        assert_eq!(palette.color(256), palette.foreground);
        assert_eq!(palette.color(BACKGROUND_INDEX), palette.background);
        assert_eq!(palette.color(CURSOR_INDEX), palette.cursor);
    }
}
