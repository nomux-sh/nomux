//! The terminal: the VT library behind the interface of this crate.

use std::fmt;
use std::time::Duration;

use alacritty_terminal::Term;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{Processor, Timeout};

use crate::osc::{Osc, OscScanner};
use crate::redraw;
use crate::screen::Screen;
use crate::size::Size;

/// The OSC codes that the VT library handles: titles, colors, hyperlinks,
/// the cursor shape and the clipboard.
const LIBRARY_OSC_CODES: [u32; 14] = [0, 2, 4, 8, 10, 11, 12, 22, 50, 52, 104, 110, 111, 112];

/// A terminal: it takes the output of a program and keeps the screen state.
pub struct Terminal {
    term: Term<VoidListener>,
    parser: Processor<ImmediateUpdate>,
    osc_scanner: OscScanner,
    size: Size,
}

impl Terminal {
    /// Makes a terminal with an empty screen. The terminal keeps no scrollback.
    #[must_use]
    pub fn new(size: Size) -> Self {
        let config = Config {
            scrolling_history: 0,
            kitty_keyboard: true,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size, VoidListener),
            parser: Processor::new(),
            osc_scanner: OscScanner::new(),
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
        self.osc_scanner.scan(bytes, |osc| {
            if !LIBRARY_OSC_CODES.contains(&osc.code) {
                unhandled_osc(osc);
            }
        });
        self.parser.advance(&mut self.term, bytes);
    }

    /// Changes the size of the screen.
    pub fn resize(&mut self, size: Size) {
        self.term.resize(size);
        self.size = size;
    }

    /// The visible screen.
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
    /// and the marks of wrapped lines.
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
