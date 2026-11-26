//! The text snapshot of a visible screen.
//!
//! ```text
//! |ab  日x              |    one line for each row, one character for each cell
//! |                    |
//! cursor 0,5 hidden bar blinking
//! modes +INSERT -AUTOWRAP
//! style 0,0-1 bold underline=curly fg=1 bg=#0a141e ul=200
//! combining 0,1 U+0301
//! ```
//!
//! - A row has no character for the second cell of a wide character, thus a
//!   row with a wide character is one character shorter.
//! - `cursor` has the row and the column from 0. The words after them are
//!   there only if the cursor is not a visible block that does not blink.
//! - `modes` has the modes that are different from the modes of a new
//!   terminal. The line is not there if no mode is different.
//! - `style` has the cells of one row, from the first column to the last
//!   column, that have the same style and not the default style. A color is
//!   an index of the palette or `#rrggbb`.
//! - `combining` has the combining characters of one cell.

use std::fmt::Write as _;

use nomux_term::{Cell, CellWidth, Color, Cursor, CursorShape, Position, Screen, Size, Style};
use nomux_term::{Terminal, Underline};

const ROWS: u16 = 5;
const COLUMNS: u16 = 20;

/// Compares the screen after `output` with `expected`. A snapshot in a test
/// starts with a line break, which is not a part of the snapshot.
pub(crate) fn assert_screen(output: &str, expected: &str) {
    let actual = screen_text(&terminal_after(output).screen());
    let expected = expected.strip_prefix('\n').unwrap_or(expected);
    assert!(
        actual == expected,
        "the screen is not the snapshot\n--- snapshot\n{expected}--- screen\n{actual}---"
    );
}

/// Compares the `modes` line after `output` with `expected`, without the
/// word `modes`.
pub(crate) fn assert_modes(output: &str, expected: &str) {
    assert_eq!(modes_text(&terminal_after(output).screen()), expected);
}

/// Compares the `cursor` line after `output` with `expected`, without the
/// word `cursor`.
pub(crate) fn assert_cursor(output: &str, expected: &str) {
    let cursor = terminal_after(output).screen().cursor();
    assert_eq!(cursor_text(cursor), expected);
}

fn new_terminal() -> Terminal {
    Terminal::new(Size::new(ROWS, COLUMNS).expect("the size is not zero"))
}

/// The snapshot of the screen and the answers of the terminal after the
/// parts of the output, one call of `Terminal::feed` for each part.
pub(crate) fn screen_and_replies_after<'a>(
    parts: impl IntoIterator<Item = &'a [u8]>,
) -> (String, Vec<u8>) {
    let mut terminal = new_terminal();
    let mut replies = Vec::new();
    for part in parts {
        terminal.feed(part, |_| {});
        replies.append(&mut terminal.take_replies());
    }
    (screen_text(&terminal.screen()), replies)
}

fn terminal_after(output: &str) -> Terminal {
    let mut terminal = new_terminal();
    terminal.feed(output.as_bytes(), |_| {});
    terminal
}

fn screen_text(screen: &Screen<'_>) -> String {
    let mut text = String::new();
    for row in 0..ROWS {
        let characters = cells(screen, row)
            .filter(|(_, cell)| cell.width != CellWidth::Continuation)
            .map(|(_, cell)| cell.character);
        writeln!(text, "|{}|", characters.collect::<String>()).expect("a string takes text");
    }
    writeln!(text, "cursor {}", cursor_text(screen.cursor())).expect("a string takes text");
    let modes = modes_text(screen);
    if !modes.is_empty() {
        writeln!(text, "modes {modes}").expect("a string takes text");
    }
    for row in 0..ROWS {
        write_styles(screen, row, &mut text);
    }
    for row in 0..ROWS {
        write_combining(screen, row, &mut text);
    }
    text
}

fn cells<'a>(screen: &Screen<'a>, row: u16) -> impl Iterator<Item = (u16, Cell<'a>)> {
    (0..COLUMNS).map(move |column| {
        let cell = screen.cell(Position { row, column });
        (column, cell.expect("the position is on the screen"))
    })
}

fn cursor_text(cursor: Cursor) -> String {
    let mut text = format!("{},{}", cursor.position.row, cursor.position.column);
    if !cursor.visible {
        text.push_str(" hidden");
    }
    text.push_str(match cursor.shape {
        CursorShape::Block => "",
        CursorShape::Underline => " underline",
        CursorShape::Bar => " bar",
    });
    if cursor.blinking {
        text.push_str(" blinking");
    }
    text
}

fn modes_text(screen: &Screen<'_>) -> String {
    let modes = screen.modes();
    let default_modes = new_terminal().screen().modes();
    let set = modes.difference(default_modes).iter_names();
    let reset = default_modes.difference(modes).iter_names();
    let words = set
        .map(|(name, _)| format!("+{name}"))
        .chain(reset.map(|(name, _)| format!("-{name}")));
    words.collect::<Vec<_>>().join(" ")
}

fn write_styles(screen: &Screen<'_>, row: u16, text: &mut String) {
    let styles: Vec<Style> = cells(screen, row).map(|(_, cell)| cell.style).collect();
    let mut first = 0;
    for cells in styles.chunk_by(|left, right| left == right) {
        let last = first + cells.len() - 1;
        if cells[0] != Style::default() {
            let columns = if first == last {
                first.to_string()
            } else {
                format!("{first}-{last}")
            };
            let style = style_text(cells[0]);
            writeln!(text, "style {row},{columns} {style}").expect("a string takes text");
        }
        first = last + 1;
    }
}

fn style_text(style: Style) -> String {
    let attributes = style.attributes.iter_names();
    let mut words: Vec<String> = attributes.map(|(name, _)| name.to_lowercase()).collect();
    if style.underline != Underline::None {
        let underline = format!("{:?}", style.underline).to_lowercase();
        words.push(format!("underline={underline}"));
    }
    let colors = [
        ("fg", style.foreground),
        ("bg", style.background),
        ("ul", style.underline_color),
    ];
    for (name, color) in colors {
        match color {
            Color::Default => {}
            Color::Indexed(index) => words.push(format!("{name}={index}")),
            Color::Rgb(red, green, blue) => {
                words.push(format!("{name}=#{red:02x}{green:02x}{blue:02x}"));
            }
        }
    }
    words.join(" ")
}

fn write_combining(screen: &Screen<'_>, row: u16, text: &mut String) {
    for (column, cell) in cells(screen, row) {
        if cell.combining.is_empty() {
            continue;
        }
        let code_points = cell.combining.iter();
        let code_points: Vec<String> = code_points
            .map(|character| format!("U+{:04X}", u32::from(*character)))
            .collect();
        let code_points = code_points.join(" ");
        writeln!(text, "combining {row},{column} {code_points}").expect("a string takes text");
    }
}
