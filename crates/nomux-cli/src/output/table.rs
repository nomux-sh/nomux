//! The tables of the text output. The columns are aligned by the width of
//! their text on a terminal, and a table fits the width of the terminal: a
//! column that can be shortened loses its end, and an ellipsis marks the
//! cut. If the shortened columns at the width of their headers are not
//! enough, the table leaves out columns that it can do without.

use std::io::{self, IsTerminal};

use rustix::termios::tcgetwinsize;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::human::RESET;

const SEPARATOR: &str = "  ";
/// The indentation of one level of a tree in the first column of a table.
pub(super) const INDENT: &str = "  ";
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
    /// The columns that the table leaves out when the shortened columns
    /// are not enough, in the sequence in which it leaves them out.
    omitted: Vec<usize>,
}

impl Table {
    pub(super) fn new(header: &[&str]) -> Self {
        Self {
            header: header.iter().map(|text| Cell::new(*text)).collect(),
            rows: Vec::new(),
            shortened: Vec::new(),
            omitted: Vec::new(),
        }
    }

    /// Shortens the columns `columns` in this sequence when the table is
    /// too wide. A column keeps the width of its header.
    pub(super) fn shorten(mut self, columns: &[usize]) -> Self {
        self.shortened = columns.to_vec();
        self
    }

    /// Leaves out the columns `columns` in this sequence when the table is
    /// too wide also with each shortened column at the width of its header.
    pub(super) fn omit(mut self, columns: &[usize]) -> Self {
        self.omitted = columns.to_vec();
        self
    }

    /// Adds a row with one cell for each column.
    pub(super) fn push(&mut self, row: Vec<Cell>) {
        self.rows.push(row);
    }

    /// The lines of the table. With `width`, a line is not wider than
    /// `width` columns if the shortened and the omitted columns permit it.
    pub(super) fn render(&self, width: Option<usize>) -> String {
        let columns = self.columns(width);
        let mut text = String::new();
        for row in std::iter::once(&self.header).chain(&self.rows) {
            text.push_str(line(row, &columns).trim_end());
            text.push('\n');
        }
        text
    }

    /// The columns that the table shows, with their widths. A column is
    /// left out only if the table is too wide with each shortened column at
    /// the width of its header. Then the shown columns are shortened.
    fn columns(&self, limit: Option<usize>) -> Vec<(usize, usize)> {
        let mut widths: Vec<usize> = (0..self.header.len())
            .map(|column| {
                std::iter::once(&self.header)
                    .chain(&self.rows)
                    .map(|row| row[column].text.width())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let mut shown: Vec<usize> = (0..widths.len()).collect();
        let Some(limit) = limit else {
            return shown.into_iter().zip(widths).collect();
        };
        let shortest = |column: usize, widths: &[usize]| {
            if self.shortened.contains(&column) {
                widths[column].min(self.header[column].text.width())
            } else {
                widths[column]
            }
        };
        for &column in &self.omitted {
            if line_width(&shown, |column| shortest(column, &widths)) <= limit {
                break;
            }
            shown.retain(|&shown_column| shown_column != column);
        }
        let mut excess = line_width(&shown, |column| widths[column]).saturating_sub(limit);
        for &column in self
            .shortened
            .iter()
            .filter(|column| shown.contains(column))
        {
            let cut = (widths[column] - shortest(column, &widths)).min(excess);
            widths[column] -= cut;
            excess -= cut;
        }
        shown
            .into_iter()
            .map(|column| (column, widths[column]))
            .collect()
    }
}

/// The width of a line with the columns `shown`.
fn line_width(shown: &[usize], width: impl Fn(usize) -> usize) -> usize {
    let separators = SEPARATOR.len() * shown.len().saturating_sub(1);
    shown.iter().map(|&column| width(column)).sum::<usize>() + separators
}

/// One line with the cells of `columns`: the shown columns and their widths.
fn line(row: &[Cell], columns: &[(usize, usize)]) -> String {
    let last = columns.len().saturating_sub(1);
    let cells: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(position, &(column, width))| {
            let cell = &row[column];
            let mut text = shortened(&cell.text, width);
            if position != last {
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

/// Fails if a line of `text` is wider than `width` columns.
#[cfg(test)]
pub(super) fn assert_fits(text: &str, width: usize) {
    for line in text.lines() {
        assert!(
            line.width() <= width,
            "{line:?} is wider than {width} columns:\n{text}"
        );
    }
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
    fn a_column_is_left_out_only_when_the_shortened_columns_are_not_enough() {
        let table = || {
            let mut table = Table::new(&["ID", "COMMAND", "STATE", "SIZE"])
                .shorten(&[1])
                .omit(&[3]);
            table.push(vec![
                Cell::new("s1"),
                Cell::new("a long command"),
                Cell::new("running"),
                Cell::new("12.0 MiB"),
            ]);
            table
        };

        assert_eq!(
            table().render(Some(36)),
            "ID  COMMAND        STATE    SIZE\n\
             s1  a long comma\u{2026}  running  12.0 MiB\n"
        );
        // The command at the width of its header is not enough: the size
        // goes, and the command gets the width back.
        assert_eq!(
            table().render(Some(27)),
            "ID  COMMAND         STATE\n\
             s1  a long command  running\n"
        );
    }

    #[test]
    fn without_a_terminal_nothing_is_shortened() {
        let text = table().render(None);

        assert!(text.contains("sh -c 'a long command line with many words'  working"));
        assert_eq!(text, table().render(Some(1000)));
    }
}
