//! Terminal state for nomux.
//!
//! # Responsibility
//!
//! This crate is a thin abstraction over one VT library, `alacritty_terminal`.
//! It gives the holder and the clients one interface for the terminal state:
//!
//! - [`Terminal`] takes the output of a program and keeps the screen.
//! - [`Screen`] reads the visible cells, the cursor and the modes, and the
//!   history rows above the screen of a terminal that
//!   [`Terminal::with_history`] made.
//! - [`Terminal::write_redraw`] writes the byte sequence that makes the same
//!   screen in a new terminal. The daemon sends it to a client that attaches.
//! - [`Terminal::incomplete_tail`] is the number of bytes at the end of the
//!   output that are a sequence or a character that is not complete.
//! - [`Osc`] is an OSC sequence that the VT library does not handle.
//!   [`Osc::record`] parses the sequences with which a program or a shell
//!   reports its state: the program status (OSC 7501), the prompt marks
//!   (OSC 133) and the notifications (OSC 9, OSC 777). [`osc`] has the
//!   parsers.
//!
//! The types of the VT library are not in this interface, thus the project
//! can replace the library.
//!
//! # Redraw position
//!
//! The redraw sequence makes the screen, not the state of the parser. If the
//! output ends in a sequence that is not complete, a client that gets the
//! redraw sequence and then the next bytes of the output shows the end of
//! that sequence as text. Thus the rule is: after the redraw sequence, the
//! client gets the output from the start of the incomplete sequence. That
//! position is the end of the output minus [`Terminal::incomplete_tail`].
//!
//! # Boundaries
//!
//! - This crate does not open a PTY and does no I/O. It changes bytes into
//!   terminal state.
//! - This crate has no async runtime. The holder uses this crate.
//! - No other nomux crate uses the VT library directly.
//!
//! # Example
//!
//! ```
//! use nomux_term::{Position, Size, Terminal};
//!
//! let size = Size::new(24, 80).expect("the size is not zero");
//! let mut terminal = Terminal::new(size);
//! terminal.feed(b"\x1b[1mnomux\x1b[0m", |_osc| {});
//!
//! let cell = terminal.screen().cell(Position { row: 0, column: 0 });
//! assert_eq!(cell.map(|cell| cell.character), Some('n'));
//!
//! let mut redraw = Vec::new();
//! terminal.write_redraw(&mut redraw);
//! ```

mod cell;
mod conversion;
mod cursor;
mod modes;
pub mod osc;
mod redraw;
mod screen;
mod sequence_scanner;
mod size;
mod terminal;

pub use cell::{Attributes, Cell, CellWidth, Color, Style, Underline};
pub use cursor::{Cursor, CursorShape, Position};
pub use modes::Modes;
pub use osc::Osc;
pub use screen::Screen;
pub use size::Size;
pub use terminal::Terminal;
