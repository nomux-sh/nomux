//! The terminal: the VT library behind the interface of this crate.

use std::fmt;
use std::time::Duration;

use alacritty_terminal::Term;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{Processor, Timeout};

use crate::osc::Osc;
use crate::redraw;
use crate::screen::Screen;
use crate::sequence_scanner::{Found, SequenceScanner};
use crate::size::Size;

/// The OSC codes that the VT library handles: titles, colors, hyperlinks,
/// the cursor shape and the clipboard.
const LIBRARY_OSC_CODES: [u32; 14] = [0, 2, 4, 8, 10, 11, 12, 22, 50, 52, 104, 110, 111, 112];

/// A terminal: it takes the output of a program and keeps the screen state.
pub struct Terminal {
    term: Term<VoidListener>,
    parser: Processor<ImmediateUpdate>,
    scanner: SequenceScanner,
    size: Size,
}

impl Terminal {
    /// Makes a terminal with an empty screen. The terminal keeps no history.
    #[must_use]
    pub fn new(size: Size) -> Self {
        Self::with_history(size, 0)
    }

    /// Makes a terminal with an empty screen that keeps up to
    /// `history_rows` rows that scroll out above the screen. When the
    /// history is full, a new history row removes the oldest row.
    ///
    /// [`Screen::history_cell`] reads the history. A row of the history
    /// uses memory only after it scrolls out of the screen.
    #[must_use]
    pub fn with_history(size: Size, history_rows: usize) -> Self {
        let config = Config {
            scrolling_history: history_rows,
            kitty_keyboard: true,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size, VoidListener),
            parser: Processor::new(),
            scanner: SequenceScanner::new(),
            size,
        }
    }

    /// Gives output of the program to the terminal. A sequence can start in
    /// one call and end in a later call.
    ///
    /// `unhandled_osc` gets each OSC sequence that ends in `bytes` and that
    /// the VT library does not handle, for example OSC 9, 133, 777 and 7501.
    /// A sequence without a decimal code or with more than 4096 bytes is
    /// ignored.
    pub fn feed(&mut self, bytes: &[u8], mut unhandled_osc: impl FnMut(Osc<'_>)) {
        let mut parsed = 0;
        self.scanner.scan(bytes, |found| match found {
            Found::Osc(osc) => {
                if !LIBRARY_OSC_CODES.contains(&osc.code) {
                    unhandled_osc(osc);
                }
            }
            Found::EraseAbove { end } => {
                self.parser.advance(&mut self.term, &bytes[parsed..end]);
                parsed = end;
                erase_the_first_row_from_the_second_row(&mut self.term);
            }
        });
        self.parser.advance(&mut self.term, &bytes[parsed..]);
        self.end_wrap_before_the_last_column();
    }

    /// The number of bytes at the end of the output that had no effect yet:
    /// an escape sequence or a UTF-8 character that is not complete.
    ///
    /// The redraw sequence does not make the state of the parser. Thus a
    /// client that gets the redraw sequence must then get the output from
    /// the start of the incomplete tail, not from the end of the output. The
    /// client then has the complete sequence, and the tail has no effect two
    /// times.
    ///
    /// One case is not exact: a control character in a CSI sequence or in an
    /// escape sequence has its effect immediately, and the client gets it a
    /// second time with the tail.
    #[must_use]
    pub fn incomplete_tail(&self) -> usize {
        self.scanner.incomplete_tail()
    }

    /// Changes the size of the screen.
    ///
    /// The primary screen and its history reflow when the number of columns
    /// changes: a row that wrapped joins the next row when the screen gets
    /// wider, and a long row wraps into more rows when the screen gets
    /// narrower. A row that ended with a line break does not reflow. The
    /// alternate screen does not reflow. When the screen gets fewer rows,
    /// the rows above the cursor go into the history; when it gets more
    /// rows, rows of the history come back onto the screen.
    pub fn resize(&mut self, size: Size) {
        self.term.resize(size);
        self.size = size;
        self.end_wrap_before_the_last_column();
    }

    /// The visible screen and the history.
    #[must_use]
    pub fn screen(&self) -> Screen<'_> {
        Screen::new(&self.term, self.size)
    }

    /// Appends to `output` the byte sequence that makes the current screen in
    /// a new terminal of the same size.
    ///
    /// The sequence makes the visible cells, the cursor, the modes and the
    /// style of the next character. If the alternate screen is visible, the
    /// sequence also makes the screen behind it, which comes back when the
    /// program leaves the alternate screen.
    ///
    /// The sequence does not make the scroll region, the tab stops, the saved
    /// cursor, the character sets, the hyperlinks, the changed palette colors
    /// and the marks of wrapped lines. It does not make the state of the
    /// parser: see [`incomplete_tail`](Self::incomplete_tail).
    ///
    /// The method needs `&mut self` because the VT library gives access to
    /// the hidden screen only through a swap. The terminal has the same state
    /// before and after the call.
    pub fn write_redraw(&mut self, output: &mut Vec<u8>) {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.write_primary_screen(output);
            redraw::write_enter_alternate_screen(output);
        }
        redraw::write_visible_screen(&self.screen(), output);
    }

    /// After a character in the last column, the next character goes to the
    /// next row. The VT library keeps this state when a backward tab or a
    /// wider alternate screen puts the cursor before the last column. A VT
    /// terminal ends the state there, and the redraw sequence can make the
    /// state only in the last column.
    fn end_wrap_before_the_last_column(&mut self) {
        let cursor = &self.term.grid().cursor;
        if cursor.input_needs_wrap && cursor.point.column < self.term.last_column() {
            self.term.grid_mut().cursor.input_needs_wrap = false;
        }
    }

    /// Writes the screen that the alternate screen hides.
    fn write_primary_screen(&mut self, output: &mut Vec<u8>) {
        // The swap back to the alternate screen erases the alternate screen.
        let alternate_screen = self.term.grid().clone();
        self.term.swap_alt();
        redraw::write_hidden_screen(&self.screen(), output);
        self.term.swap_alt();
        *self.term.grid_mut() = alternate_screen;
    }
}

/// Completes an erase in display above the cursor (ED 1) of the VT library.
/// The library erases the rows above the cursor only if the cursor is below
/// the second row: <https://github.com/alacritty/alacritty/issues/9096>.
fn erase_the_first_row_from_the_second_row(term: &mut Term<VoidListener>) {
    if term.grid().cursor.point.line == Line(1) {
        term.grid_mut().reset_region(..Line(1));
    }
}

impl fmt::Debug for Terminal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Terminal")
            .field("screen", &self.screen())
            .finish_non_exhaustive()
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows())
    }

    fn columns(&self) -> usize {
        usize::from(Size::columns(*self))
    }
}

/// Turns off the synchronized updates (DEC mode 2026) of the parser.
///
/// With a synchronized update, the parser keeps the output of the program in
/// a buffer and the screen does not change. A redraw sequence from that
/// moment does not have the bytes in the buffer, and the client never gets
/// them. Thus the terminal applies each byte immediately.
#[derive(Debug, Default)]
struct ImmediateUpdate;

impl Timeout for ImmediateUpdate {
    fn set_timeout(&mut self, _duration: Duration) {}

    fn clear_timeout(&mut self) {}

    fn pending_timeout(&self) -> bool {
        false
    }
}
