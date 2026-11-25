//! Conversion of the types of the VT library to the types of this crate.

use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell as LibraryCell, Flags};
use alacritty_terminal::vte::ansi::{
    Color as LibraryColor, CursorShape as LibraryCursorShape, NamedColor,
};

use crate::cell::{Attributes, Cell, CellWidth, Color, Style, Underline};
use crate::cursor::CursorShape;
use crate::modes::Modes;

/// The named colors of the library start with the 16 colors of the palette.
const NAMED_PALETTE_COLORS: u8 = 16;

const ATTRIBUTES: [(Flags, Attributes); 6] = [
    (Flags::BOLD, Attributes::BOLD),
    (Flags::DIM, Attributes::DIM),
    (Flags::ITALIC, Attributes::ITALIC),
    (Flags::INVERSE, Attributes::INVERSE),
    (Flags::HIDDEN, Attributes::HIDDEN),
    (Flags::STRIKEOUT, Attributes::STRIKETHROUGH),
];

const UNDERLINES: [(Flags, Underline); 5] = [
    (Flags::UNDERLINE, Underline::Single),
    (Flags::DOUBLE_UNDERLINE, Underline::Double),
    (Flags::UNDERCURL, Underline::Curly),
    (Flags::DOTTED_UNDERLINE, Underline::Dotted),
    (Flags::DASHED_UNDERLINE, Underline::Dashed),
];

const MODES: [(TermMode, Modes); 20] = [
    (TermMode::ALT_SCREEN, Modes::ALTERNATE_SCREEN),
    (TermMode::APP_CURSOR, Modes::APPLICATION_CURSOR),
    (TermMode::APP_KEYPAD, Modes::APPLICATION_KEYPAD),
    (TermMode::BRACKETED_PASTE, Modes::BRACKETED_PASTE),
    (TermMode::FOCUS_IN_OUT, Modes::FOCUS_REPORTING),
    (TermMode::MOUSE_REPORT_CLICK, Modes::MOUSE_CLICK),
    (TermMode::MOUSE_DRAG, Modes::MOUSE_DRAG),
    (TermMode::MOUSE_MOTION, Modes::MOUSE_MOTION),
    (TermMode::UTF8_MOUSE, Modes::MOUSE_UTF8),
    (TermMode::SGR_MOUSE, Modes::MOUSE_SGR),
    (TermMode::ALTERNATE_SCROLL, Modes::ALTERNATE_SCROLL),
    (TermMode::LINE_WRAP, Modes::AUTOWRAP),
    (TermMode::ORIGIN, Modes::ORIGIN),
    (TermMode::INSERT, Modes::INSERT),
    (TermMode::LINE_FEED_NEW_LINE, Modes::LINE_FEED_NEW_LINE),
    (
        TermMode::DISAMBIGUATE_ESC_CODES,
        Modes::KEYBOARD_DISAMBIGUATE,
    ),
    (TermMode::REPORT_EVENT_TYPES, Modes::KEYBOARD_EVENT_TYPES),
    (
        TermMode::REPORT_ALTERNATE_KEYS,
        Modes::KEYBOARD_ALTERNATE_KEYS,
    ),
    (
        TermMode::REPORT_ALL_KEYS_AS_ESC,
        Modes::KEYBOARD_ALL_AS_ESCAPE_CODES,
    ),
    (
        TermMode::REPORT_ASSOCIATED_TEXT,
        Modes::KEYBOARD_ASSOCIATED_TEXT,
    ),
];

pub(crate) fn cell(cell: &LibraryCell) -> Cell<'_> {
    Cell {
        // The library keeps a tab as a tab character in the first cell that it covers.
        character: if cell.c == '\t' { ' ' } else { cell.c },
        combining: cell.zerowidth().unwrap_or_default(),
        width: width(cell.flags),
        style: style(cell),
    }
}

pub(crate) fn style(cell: &LibraryCell) -> Style {
    Style {
        foreground: color(cell.fg),
        background: color(cell.bg),
        underline_color: cell.underline_color().map_or(Color::Default, color),
        underline: select(&UNDERLINES, cell.flags).next().unwrap_or_default(),
        attributes: select(&ATTRIBUTES, cell.flags).collect(),
    }
}

pub(crate) fn modes(mode: TermMode) -> Modes {
    MODES
        .iter()
        .filter(|(library_mode, _)| mode.contains(*library_mode))
        .map(|(_, mode)| *mode)
        .collect()
}

/// A program cannot select the other shapes of the library.
pub(crate) fn cursor_shape(shape: LibraryCursorShape) -> CursorShape {
    match shape {
        LibraryCursorShape::Underline => CursorShape::Underline,
        LibraryCursorShape::Beam => CursorShape::Bar,
        _ => CursorShape::Block,
    }
}

fn width(flags: Flags) -> CellWidth {
    if flags.contains(Flags::WIDE_CHAR) {
        CellWidth::Double
    } else if flags.contains(Flags::WIDE_CHAR_SPACER) {
        CellWidth::Continuation
    } else {
        CellWidth::Single
    }
}

fn color(color: LibraryColor) -> Color {
    match color {
        LibraryColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        LibraryColor::Indexed(index) => Color::Indexed(index),
        LibraryColor::Named(name) => named_color(name),
    }
}

/// The other named colors are the default foreground, the default
/// background and colors that only a renderer uses.
fn named_color(name: NamedColor) -> Color {
    u8::try_from(name as usize)
        .ok()
        .filter(|index| *index < NAMED_PALETTE_COLORS)
        .map_or(Color::Default, Color::Indexed)
}

/// The values of the table whose library flag is set.
fn select<T: Copy>(table: &[(Flags, T)], flags: Flags) -> impl Iterator<Item = T> {
    table
        .iter()
        .filter(move |(flag, _)| flags.contains(*flag))
        .map(|(_, value)| *value)
}
