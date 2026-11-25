//! Benchmark probe for `libghostty-vt`, the Rust binding of the VT library of
//! the Ghostty project.

use std::process::ExitCode;

use libghostty_vt::screen::CellWide;
use libghostty_vt::terminal::{Options, Point, PointCoordinate, Terminal};
use vt_probe::{Parser, Position, Screen, Size};

/// More code points than one cell of the benchmark input has.
const MAX_CELL_CODE_POINTS: usize = 16;

struct LibghosttyVt(Terminal<'static, 'static>);

impl LibghosttyVt {
    fn cell_text(&self, row: u16, column: u16) -> Option<String> {
        let point = Point::Active(PointCoordinate {
            x: column,
            y: u32::from(row),
        });
        let cell = self.0.grid_ref(point).ok()?;
        if cell.cell().ok()?.wide().ok()? == CellWide::SpacerTail {
            return Some(String::new());
        }
        let mut code_points = ['\0'; MAX_CELL_CODE_POINTS];
        let count = cell.graphemes(&mut code_points).ok()?;
        Some(match count {
            0 => " ".to_owned(),
            _ => code_points[..count].iter().collect(),
        })
    }
}

impl Parser for LibghosttyVt {
    const NAME: &'static str = "libghostty-vt 0.2.2";

    fn new(size: Size, scrollback_lines: usize) -> Self {
        let options = Options {
            cols: size.columns,
            rows: size.rows,
            max_scrollback: scrollback_lines,
        };
        match Terminal::new(options) {
            Ok(terminal) => Self(terminal),
            Err(error) => panic!("libghostty-vt cannot make a terminal: {error}"),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.0.vt_write(bytes);
    }

    fn retained_scrollback_lines(&self) -> usize {
        self.0.scrollback_rows().unwrap_or(0)
    }
}

impl Screen for LibghosttyVt {
    fn row_text(&self, row: u16) -> String {
        let columns = self.0.cols().unwrap_or(0);
        (0..columns)
            .map(|column| self.cell_text(row, column).unwrap_or_default())
            .collect()
    }

    fn cursor(&self) -> Position {
        Position {
            row: self.0.cursor_y().unwrap_or(u16::MAX),
            column: self.0.cursor_x().unwrap_or(u16::MAX),
        }
    }
}

fn main() -> ExitCode {
    vt_probe::run_screen::<LibghosttyVt>()
}
