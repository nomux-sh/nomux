//! Keeps the window size of a `Resize` when a program writes an older size
//! back to the pseudoterminal.
//!
//! A shell with a line editor reads the window size and writes it back when
//! it starts a prompt. A `Resize` between the two calls is lost. The program
//! writes output after that, thus the holder compares the sizes when it
//! reads output: it needs no timer, and an idle holder does not wake.

use std::io;
use std::os::fd::BorrowedFd;

use nomux_holder_protocol::wire::WindowSize;

use crate::pty;

/// The number of reads of output after a `Resize` at which the holder
/// compares the sizes. The limit ends a contest with a program that sets its
/// own size after each change.
const VERIFIED_READS: u8 = 16;

#[derive(Default)]
pub(crate) struct ResizeVerification {
    remaining_reads: u8,
}

impl ResizeVerification {
    pub(crate) fn window_resized(&mut self) {
        self.remaining_reads = VERIFIED_READS;
    }

    /// Sets the window size of `terminal` to `size` again if a program
    /// changed it after the last `Resize`.
    pub(crate) fn output_read(&mut self, terminal: BorrowedFd<'_>, size: WindowSize) {
        if self.remaining_reads == 0 {
            return;
        }
        self.remaining_reads -= 1;
        // The session continues with the size that the program set.
        if let Err(error) = restore(terminal, size) {
            eprintln!("nomux-holder: cannot verify the window size: {error}");
        }
    }
}

fn restore(terminal: BorrowedFd<'_>, size: WindowSize) -> io::Result<()> {
    if pty::size(terminal)? != size {
        pty::resize(terminal, size)?;
    }
    Ok(())
}
