//! The terminal of the user during an attach: raw mode, the window size,
//! and the state that the programs of the session give to the terminal.

use std::io::{self, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::panic;

use nomux_client::WindowSize;
use rustix::termios::{OptionalActions, Termios, isatty, tcgetattr, tcgetwinsize, tcsetattr};

/// Puts the terminal into the initial state that a snapshot of a screen
/// needs. The redraw sequence of a snapshot sets the modes and the cells,
/// and it does not set these parts: the alternate screen, the scroll
/// region, the saved cursor, the character sets and the keyboard flags.
///
/// In this sequence: leave the alternate screen, soft reset (DECSTR), full
/// scroll region, character set ASCII, and remove the flags of the kitty
/// keyboard protocol.
pub(super) const BEFORE_SNAPSHOT: &[u8] = b"\x1b[?1049l\x1b[!p\x1b[r\x1b(B\x0f\x1b[<99u";

/// Gives the terminal back to the shell of the user: the state of
/// [`BEFORE_SNAPSHOT`] with the cursor where it is, and then no mouse
/// reports, no focus reports, no bracketed paste, normal cursor keys and
/// keypad, automatic wrap, a visible cursor of the default shape, the
/// default style, and a new line.
const AFTER_ATTACH: &[u8] = b"\x1b[?1049l\x1b[!p\x1b7\x1b[r\x1b8\x1b(B\x0f\x1b[<99u\
    \x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1005l\x1b[?1006l\x1b[?2004l\
    \x1b[?1l\x1b>\x1b[?7h\x1b[?25h\x1b[0 q\x1b[0m\r\n";

/// Saves the title of the window (XTWINOPS 22) and sets a title that says
/// that the attach has no connection. The screen is not changed: the
/// output of the session continues on it after the connection is back.
pub(super) const SHOW_CONNECTION_LOST: &[u8] =
    b"\x1b[22;0t\x1b]0;nomux: connection lost, connecting again\x07";

/// Gives the window its saved title back (XTWINOPS 23).
pub(super) const HIDE_CONNECTION_LOST: &[u8] = b"\x1b[23;0t";

/// `true` if the standard input is a terminal.
pub(super) fn is_available() -> bool {
    isatty(io::stdin())
}

/// The size of the window of the terminal. `None` if the terminal does not
/// know its size.
pub(super) fn window_size() -> Option<WindowSize> {
    let size = tcgetwinsize(io::stdin()).ok()?;
    (size.ws_row > 0 && size.ws_col > 0).then_some(WindowSize {
        rows: size.ws_row,
        columns: size.ws_col,
    })
}

/// A terminal in raw mode: each key goes to the session without a change,
/// and the terminal shows only what the session writes.
///
/// The terminal gets its earlier mode back when this value is dropped, and
/// when the process panics.
pub(super) struct RawMode {
    terminal: OwnedFd,
    earlier: Termios,
}

impl RawMode {
    /// Puts the terminal of the standard input into raw mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the standard input is not a terminal.
    pub(super) fn enter() -> io::Result<Self> {
        Self::enter_on(io::stdin().as_fd())
    }

    fn enter_on(terminal: BorrowedFd<'_>) -> io::Result<Self> {
        let earlier = tcgetattr(terminal)?;
        let mut raw = earlier.clone();
        raw.make_raw();
        restore_on_panic(terminal.try_clone_to_owned()?, earlier.clone());
        tcsetattr(terminal, OptionalActions::Now, &raw)?;
        Ok(Self {
            terminal: terminal.try_clone_to_owned()?,
            earlier,
        })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        restore(&self.terminal, &self.earlier);
    }
}

/// The release profile stops the process at a panic and drops no value,
/// thus the panic hook restores the terminal. The message of the panic
/// comes after that, and the terminal shows it correctly.
fn restore_on_panic(terminal: OwnedFd, earlier: Termios) {
    let report_panic = panic::take_hook();
    panic::set_hook(Box::new(move |panic| {
        restore(&terminal, &earlier);
        report_panic(panic);
    }));
}

/// Runs at the end of the process. An error has no place to go: the
/// terminal is the output.
fn restore(terminal: &OwnedFd, earlier: &Termios) {
    let mut screen = io::stdout().lock();
    let _ = screen.write_all(AFTER_ATTACH);
    let _ = screen.flush();
    let _ = tcsetattr(terminal, OptionalActions::Now, earlier);
}

#[cfg(test)]
mod tests {
    use std::mem;

    use rustix::fs::{Mode, OFlags};
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    use rustix::termios::LocalModes;

    use super::*;

    fn pseudoterminal() -> (OwnedFd, OwnedFd) {
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let name = ptsname(&master, Vec::new()).unwrap();
        let terminal =
            rustix::fs::open(name, OFlags::RDWR | OFlags::NOCTTY, Mode::empty()).unwrap();
        (master, terminal)
    }

    fn is_raw(terminal: &OwnedFd) -> bool {
        let modes = tcgetattr(terminal).unwrap().local_modes;
        !modes.intersects(LocalModes::ICANON | LocalModes::ECHO)
    }

    #[test]
    fn a_panic_gives_the_terminal_its_earlier_mode_back() {
        let (_master, terminal) = pseudoterminal();
        let raw_mode = RawMode::enter_on(terminal.as_fd()).unwrap();
        assert!(is_raw(&terminal));

        // The release profile drops no value at a panic.
        mem::forget(raw_mode);
        let panicked = panic::catch_unwind(|| panic!("a panic during an attach"));

        assert!(panicked.is_err());
        assert!(!is_raw(&terminal));
    }

    #[test]
    fn the_end_of_the_attach_gives_the_terminal_its_earlier_mode_back() {
        // macOS sets `PENDIN` itself when a terminal goes back to line mode.
        let modes = |terminal: &OwnedFd| {
            let modes = tcgetattr(terminal).unwrap().local_modes;
            modes.difference(LocalModes::PENDIN)
        };
        let (_master, terminal) = pseudoterminal();
        let earlier = modes(&terminal);

        drop(RawMode::enter_on(terminal.as_fd()).unwrap());

        assert_eq!(modes(&terminal), earlier);
    }
}
