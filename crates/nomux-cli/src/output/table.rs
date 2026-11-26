//! The tables of the text output. The columns are aligned by the width of
//! their text on a terminal, and a table fits the width of the terminal: a
//! column that can be shortened loses its end, and an ellipsis marks the
//! cut.

use std::io::{self, IsTerminal};

use rustix::termios::tcgetwinsize;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::human::RESET;

const SEPARATOR: &str = "  ";
const ELLIPSIS: char = '\u{2026}';

/// The width of the terminal of the standard output in columns. `None` if
/// the output is not a terminal: a program reads the full text then.
pub(super) fn terminal_width() -> Option<usize> {
    let output = io::stdout();
    if !output.is_terminal() {
        return None;
    }
    let size = tcgetwinsize(output).ok()?;
    (size.ws_col > 0).then_some(usize::from(size.ws_col))
}

/// One cell: its text and the colour of the text.
pub(super) struct Cell {
    text: String,
    colour: Option<&'static str>,
}

impl Cell {
    pub(super) fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            colour: None,
        }
    }

    /// A cell with `colour`. `None` is the colour of the terminal.
    pub(super) fn coloured(text: impl Into<String>, colour: Option<&'static str>) -> Self {
        Self {
            text: text.into(),
            colour,
        }
    }
}

/// A table with a header line. The columns have two spaces between them,
/// and the last column has no padding.
pub(super) struct Table {
    header: Vec<Cell>,
    rows: Vec<Vec<Cell>>,
    /// The columns that the table shortens when it is wider than its
    /// limit, in the sequence in which it shortens them.
    shortened: Vec<usize>,
}

impl Table {
    pub(super) fn new(header: &[&str]) -> Self {
        Self {
            header: header.iter().map(|text| Cell::new(*text)).collect(),
            rows: Vec::new(),
            shortened: Vec::new(),
        }
    }

    /// Shortens the columns `columns` in this sequence when the table is
    /// too wide. A column keeps the width of its header.
    pub(super) fn shorten(mut self, columns: &[usize]) -> Self {
        self.shortened = columns.to_vec();
        self
    }

    /// Adds a row with one cell for each column.
    pub(super) fn push(&mut self, row: Vec<Cell>) {
        self.rows.push(row);
    }

    /// The lines of the table. With `width`, a line is not wider than
    /// `width` columns if the shortened columns permit it.
    pub(super) fn render(&self, width: Option<usize>) -> String {
        let widths = self.column_widths(width);
        let mut text = String::new();
        for row in std::iter::once(&self.header).chain(&self.rows) {
            text.push_str(line(row, &widths).trim_end());
            text.push('\n');
        }
        text
    }

    fn column_widths(&self, limit: Option<usize>) -> Vec<usize> {
        let mut widths: Vec<usize> = (0..self.header.len())
            .map(|column| {
                std::iter::once(&self.header)
                    .chain(&self.rows)
                    .map(|row| row[column].text.width())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let Some(limit) = limit else {
            return widths;
        };
        let line_width = widths.iter().sum::<usize>() + SEPARATOR.len() * (widths.len() - 1);
        let mut excess = line_width.saturating_sub(limit);
        for &column in &self.shortened {
            let shortest = self.header[column].text.width();
            let cut = widths[column].saturating_sub(shortest).min(excess);
            widths[column] -= cut;
            excess -= cut;
        }
        widths
    }
}

fn line(row: &[Cell], widths: &[usize]) -> String {
    let last = row.len() - 1;
    let cells: Vec<String> = row
        .iter()
        .zip(widths)
        .enumerate()
        .map(|(column, (cell, &width))| {
            let mut text = shortened(&cell.text, width);
            if column != last {
                let padding = width.saturating_sub(text.width());
                text.extend(std::iter::repeat_n(' ', padding));
            }
            match cell.colour {
                Some(colour) => format!("{colour}{text}{RESET}"),
                None => text,
            }
        })
        .collect();
    cells.join(SEPARATOR)
}

/// `text` if it has `width` columns or fewer. If not, its start and an
/// ellipsis in `width` columns at most. A wide character is not cut: the
/// text can be narrower than `width` then.
fn shortened(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let room = width.saturating_sub(ELLIPSIS.width().unwrap_or(1));
    let mut used = 0;
    let mut kept: String = text
        .chars()
        .take_while(|character| {
            used += character.width().unwrap_or(0);
            used <= room
        })
        .collect();
    kept.push(ELLIPSIS);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        let mut table = Table::new(&["ID", "COMMAND", "STATE", "MESSAGE"]).shorten(&[1, 3]);
        table.push(vec![
            Cell::new("s1"),
            Cell::new("sh -c 'a long command line with many words'"),
            Cell::new("working"),
            Cell::new("tests run"),
        ]);
        table
    }

    #[test]
    fn a_narrow_terminal_shortens_the_command_and_keeps_the_state_and_the_message() {
        let text = table().render(Some(40));

        assert_eq!(
            text,
            "ID  COMMAND           STATE    MESSAGE\n\
             s1  sh -c 'a long c\u{2026}  working  tests run\n"
        );
        assert!(text.lines().all(|line| line.width() <= 40), "{text}");
    }

    #[test]
    fn a_very_narrow_terminal_shortens_the_next_column_after_the_command() {
        let text = table().render(Some(28));

        assert_eq!(
            text,
            "ID  COMMAND  STATE    MESSAGE\n\
             s1  sh -c \u{2026}  working  tests \u{2026}\n"
        );
    }

    #[test]
    fn a_wide_character_is_not_cut_and_the_columns_stay_aligned() {
        let mut table = Table::new(&["COMMAND", "STATE"]).shorten(&[0]);
        table.push(vec![Cell::new("echo 日本語のテキスト"), Cell::new("idle")]);
        table.push(vec![Cell::new("ls"), Cell::new("done")]);

        let text = table.render(Some(18));

        // `日` has 2 columns: after `echo 日本` and the ellipsis, one column
        // stays empty.
        assert_eq!(
            text,
            "COMMAND      STATE\n\
             echo 日本\u{2026}   idle\n\
             ls           done\n"
        );
        assert!(text.lines().all(|line| line.width() <= 18), "{text}");
    }

    #[test]
    fn without_a_terminal_nothing_is_shortened() {
        let text = table().render(None);

        assert!(text.contains("sh -c 'a long command line with many words'  working"));
        assert_eq!(text, table().render(Some(1000)));
    }
}
