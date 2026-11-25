//! The redraw sequence: the bytes that make a screen in a new terminal.
//!
//! This module reads the screen through [`Screen`] only. It does not use the
//! VT library.

use crate::cell::{Attributes, Cell, CellWidth, Color, Style, Underline};
use crate::cursor::{Cursor, CursorShape, Position};
use crate::modes::Modes;
use crate::screen::Screen;

const RESET_STYLE_AND_ERASE_SCREEN: &str = "\x1b[0m\x1b[H\x1b[2J";
const ENTER_ALTERNATE_SCREEN: &str = "\x1b[?1049h";

/// The DEC private modes, with the number of each mode.
const PRIVATE_MODES: [(Modes, u16); 11] = [
    (Modes::APPLICATION_CURSOR, 1),
    (Modes::ORIGIN, 6),
    (Modes::AUTOWRAP, 7),
    (Modes::MOUSE_CLICK, 1000),
    (Modes::MOUSE_DRAG, 1002),
    (Modes::MOUSE_MOTION, 1003),
    (Modes::FOCUS_REPORTING, 1004),
    (Modes::MOUSE_UTF8, 1005),
    (Modes::MOUSE_SGR, 1006),
    (Modes::ALTERNATE_SCROLL, 1007),
    (Modes::BRACKETED_PASTE, 2004),
];

/// The ANSI modes, with the number of each mode.
const ANSI_MODES: [(Modes, u16); 2] = [(Modes::INSERT, 4), (Modes::LINE_FEED_NEW_LINE, 20)];

/// The attributes, with the SGR parameter of each attribute.
const ATTRIBUTE_PARAMETERS: [(Attributes, u16); 6] = [
    (Attributes::BOLD, 1),
    (Attributes::DIM, 2),
    (Attributes::ITALIC, 3),
    (Attributes::INVERSE, 7),
    (Attributes::HIDDEN, 8),
    (Attributes::STRIKETHROUGH, 9),
];

const FOREGROUND: u16 = 38;
const BACKGROUND: u16 = 48;
const UNDERLINE_COLOR: u16 = 58;

/// Writes the screen that the client sees.
pub(crate) fn write_visible_screen(screen: &Screen<'_>, output: &mut Vec<u8>) {
    let mut writer = Writer { output };
    writer.cells(screen);
    // The modes come after the cells, because the insert mode and the origin
    // mode change how the cells are written. The cursor comes after the
    // modes, because the origin mode moves the cursor.
    writer.modes(screen.modes());
    writer.keyboard_flags(screen.modes());
    writer.cursor_position(screen);
    writer.style(screen.pen());
    writer.cursor_look(screen.cursor());
}

/// Writes the screen that the alternate screen hides. The terminal modes
/// are those of the visible screen, thus this function does not write them.
pub(crate) fn write_hidden_screen(screen: &Screen<'_>, output: &mut Vec<u8>) {
    let mut writer = Writer { output };
    writer.cells(screen);
    writer.keyboard_flags(screen.modes());
    writer.cursor_position(screen);
    writer.style(screen.pen());
}

pub(crate) fn write_enter_alternate_screen(output: &mut Vec<u8>) {
    output.extend_from_slice(ENTER_ALTERNATE_SCREEN.as_bytes());
}

struct Writer<'a> {
    output: &'a mut Vec<u8>,
}

impl Writer<'_> {
    fn cells(&mut self, screen: &Screen<'_>) {
        self.text(RESET_STYLE_AND_ERASE_SCREEN);
        let mut style = Style::default();
        for row in 0..screen.size().rows() {
            self.row(screen, row, &mut style);
        }
    }

    /// Writes one row up to its last cell that is not blank. `style` is the
    /// style that the terminal has now.
    fn row(&mut self, screen: &Screen<'_>, row: u16, style: &mut Style) {
        let cells =
            (0..screen.size().columns()).filter_map(|column| screen.cell(Position { row, column }));
        let not_blank = cells
            .clone()
            .enumerate()
            .filter(|(_, cell)| !cell.is_blank());
        let Some((last, _)) = not_blank.last() else {
            return;
        };
        self.move_cursor(Position { row, column: 0 });
        for cell in cells.take(last + 1) {
            if cell.width == CellWidth::Continuation {
                continue;
            }
            if cell.style != *style {
                self.style(cell.style);
                *style = cell.style;
            }
            self.cell_text(&cell);
        }
    }

    fn cell_text(&mut self, cell: &Cell<'_>) {
        self.character(cell.character);
        for character in cell.combining {
            self.character(*character);
        }
    }

    fn modes(&mut self, modes: Modes) {
        for (mode, number) in PRIVATE_MODES {
            self.text("\x1b[?");
            self.number(number);
            self.text(if modes.contains(mode) { "h" } else { "l" });
        }
        for (mode, number) in ANSI_MODES {
            self.text("\x1b[");
            self.number(number);
            self.text(if modes.contains(mode) { "h" } else { "l" });
        }
        self.text(if modes.contains(Modes::APPLICATION_KEYPAD) {
            "\x1b="
        } else {
            "\x1b>"
        });
    }

    /// Each screen has its own flags of the kitty keyboard protocol. The
    /// sequence puts the flags on the stack, thus the program can remove them.
    fn keyboard_flags(&mut self, modes: Modes) {
        let flags = modes.keyboard_flags();
        if flags == 0 {
            return;
        }
        self.text("\x1b[>");
        self.number(u16::from(flags));
        self.text("u");
    }

    fn cursor_position(&mut self, screen: &Screen<'_>) {
        let position = screen.cursor().position;
        if screen.wrap_pending() {
            self.character_before_wrap(screen, position);
        } else {
            self.move_cursor(position);
        }
    }

    /// The cursor is in the last column and the next character goes to the
    /// next row. Only a character that is written in the last column makes
    /// this state, thus this function writes that character again.
    fn character_before_wrap(&mut self, screen: &Screen<'_>, position: Position) {
        let mut start = position;
        if screen
            .cell(start)
            .is_some_and(|cell| cell.width == CellWidth::Continuation)
        {
            start.column = start.column.saturating_sub(1);
        }
        self.move_cursor(start);
        if let Some(cell) = screen.cell(start) {
            self.style(cell.style);
            self.cell_text(&cell);
        }
    }

    fn cursor_look(&mut self, cursor: Cursor) {
        self.text(if cursor.visible {
            "\x1b[?25h"
        } else {
            "\x1b[?25l"
        });
        // DECSCUSR. 0 is the default shape of the client.
        let shape = match (cursor.shape, cursor.blinking) {
            (CursorShape::Block, false) => 0,
            (CursorShape::Block, true) => 1,
            (CursorShape::Underline, true) => 3,
            (CursorShape::Underline, false) => 4,
            (CursorShape::Bar, true) => 5,
            (CursorShape::Bar, false) => 6,
        };
        self.text("\x1b[");
        self.number(shape);
        self.text(" q");
    }

    fn move_cursor(&mut self, position: Position) {
        self.text("\x1b[");
        self.number(position.row + 1);
        self.text(";");
        self.number(position.column + 1);
        self.text("H");
    }

    /// Writes one SGR sequence that resets the style and sets `style`.
    fn style(&mut self, style: Style) {
        self.text("\x1b[0");
        for (attribute, parameter) in ATTRIBUTE_PARAMETERS {
            if style.attributes.contains(attribute) {
                self.text(";");
                self.number(parameter);
            }
        }
        self.text(match style.underline {
            Underline::None => "",
            Underline::Single => ";4",
            Underline::Double => ";4:2",
            Underline::Curly => ";4:3",
            Underline::Dotted => ";4:4",
            Underline::Dashed => ";4:5",
        });
        self.color(FOREGROUND, style.foreground);
        self.color(BACKGROUND, style.background);
        self.color(UNDERLINE_COLOR, style.underline_color);
        self.text("m");
    }

    fn color(&mut self, selector: u16, color: Color) {
        let components: &[u8] = match color {
            Color::Default => return,
            Color::Indexed(index) => &[5, index],
            Color::Rgb(red, green, blue) => &[2, red, green, blue],
        };
        self.text(";");
        self.number(selector);
        for component in components {
            self.text(";");
            self.number(u16::from(*component));
        }
    }

    fn text(&mut self, text: &str) {
        self.output.extend_from_slice(text.as_bytes());
    }

    fn character(&mut self, character: char) {
        self.text(character.encode_utf8(&mut [0; 4]));
    }

    fn number(&mut self, number: u16) {
        let mut digits = [0; 5];
        let mut start = digits.len();
        let mut rest = number;
        loop {
            start -= 1;
            digits[start] = b'0' + u8::try_from(rest % 10).unwrap_or(0);
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        self.output.extend_from_slice(&digits[start..]);
    }
}
