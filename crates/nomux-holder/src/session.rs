//! The session: the terminal, the child command, the ring buffer and the
//! screen state.

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::ring::RingWriter;
use nomux_holder_protocol::wire::{
    ChildSignal, ChildStatus, ErrorCode, Hello, ProtocolVersion, WindowSize,
};
use nomux_term::{Size, Terminal};
use rustix::event::PollFlags;
use rustix::io::Errno;

use crate::arguments::SessionArguments;
use crate::child::ChildProcess;
use crate::input_queue::InputQueue;
use crate::pty;

/// The largest number of bytes that one read moves from the terminal to the
/// ring buffer. The kernel gives 4096 bytes at most for one read of a
/// pseudoterminal, and a small chunk keeps more history readable.
const OUTPUT_CHUNK: usize = 4096;

/// After the child ended, the holder reads this number of chunks at most. A
/// process in the background can write to the terminal without an end.
const EXIT_DRAIN_CHUNKS: usize = 256;

pub(crate) struct Session {
    id: SessionId,
    /// The master side of the pseudoterminal. `None` after the child ended.
    terminal: Option<OwnedFd>,
    /// `false` after all processes closed the slave side.
    terminal_readable: bool,
    child: ChildProcess,
    ring: RingWriter,
    /// The state of the screen after each byte that the ring buffer got. It
    /// keeps no scrollback.
    screen: Terminal,
    input: InputQueue,
    size: WindowSize,
}

impl Session {
    /// Opens the pseudoterminal and starts the child command.
    pub(crate) fn start(arguments: &SessionArguments, ring: RingWriter) -> io::Result<Self> {
        let screen_size = screen_size(arguments.size).ok_or(io::ErrorKind::InvalidInput)?;
        let pty = pty::open(arguments.size)?;
        let child = ChildProcess::spawn(
            &arguments.program,
            &arguments.program_arguments,
            arguments.working_directory.as_deref(),
            pty.slave,
        )?;
        Ok(Self {
            id: arguments.session_id.clone(),
            terminal: Some(pty.master),
            terminal_readable: true,
            child,
            ring,
            screen: Terminal::new(screen_size),
            input: InputQueue::default(),
            size: arguments.size,
        })
    }

    pub(crate) fn hello(&self) -> Hello {
        Hello {
            protocol: ProtocolVersion::CURRENT,
            session_id: self.id.clone(),
            child_pid: self.child.pid(),
            size: self.size,
        }
    }

    /// The terminal and the events that the event loop must wait for.
    pub(crate) fn terminal_interest(&self) -> Option<(BorrowedFd<'_>, PollFlags)> {
        let terminal = self.terminal.as_ref()?;
        let mut interest = PollFlags::empty();
        if self.terminal_readable {
            interest |= PollFlags::IN;
        }
        if !self.input.is_empty() {
            interest |= PollFlags::OUT;
        }
        (!interest.is_empty()).then_some((terminal.as_fd(), interest))
    }

    /// Moves one chunk of output from the terminal into the ring buffer and
    /// gives the same bytes to the screen state. Returns `false` if the
    /// terminal had no bytes.
    pub(crate) fn read_output(&mut self) -> io::Result<bool> {
        let Some(terminal) = &self.terminal else {
            return Ok(false);
        };
        let screen = &mut self.screen;
        let read = self.ring.write_with(OUTPUT_CHUNK, |chunk| {
            let count = rustix::io::read(terminal, &mut *chunk)?;
            // The unhandled OSC sequences have no reader yet.
            screen.feed(&chunk[..count], |_| {});
            Ok::<_, Errno>(count)
        });
        match read {
            // No process has the slave side open: Linux reports `EIO`, macOS
            // reports the end of the file.
            Ok(0) | Err(Errno::IO) => {
                self.terminal_readable = false;
                Ok(false)
            }
            Ok(_) => Ok(true),
            Err(Errno::AGAIN | Errno::INTR) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn write_position(&self) -> u64 {
        self.ring.position()
    }

    /// The position of the ring buffer that the screen corresponds to: the
    /// write position, or the start of the escape sequence or the UTF-8
    /// character that the output ends in. The bytes of a sequence that is
    /// not complete did not change the screen.
    pub(crate) fn redraw_position(&self) -> u64 {
        let incomplete_tail = self.screen.incomplete_tail() as u64;
        self.ring.position().saturating_sub(incomplete_tail)
    }

    /// The byte sequence that makes the current screen in a new terminal.
    /// The screen corresponds to [`redraw_position`](Self::redraw_position).
    pub(crate) fn redraw_sequence(&mut self) -> Vec<u8> {
        let mut sequence = Vec::new();
        self.screen.write_redraw(&mut sequence);
        sequence
    }

    pub(crate) fn input_backlogged(&self) -> bool {
        self.input.is_backlogged()
    }

    pub(crate) fn write_input(&mut self, bytes: &[u8]) -> Result<(), ErrorCode> {
        if self.terminal.is_none() {
            return Err(ErrorCode::ChildExited);
        }
        self.input.push(bytes);
        self.flush_input();
        Ok(())
    }

    pub(crate) fn flush_input(&mut self) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        if self.input.flush(terminal).is_err() {
            // The terminal has no reader. The input has no destination.
            self.input.clear();
        }
    }

    pub(crate) fn resize(&mut self, size: WindowSize) -> Result<(), ErrorCode> {
        let terminal = self.terminal.as_ref().ok_or(ErrorCode::ChildExited)?;
        let screen_size = screen_size(size).ok_or(ErrorCode::InvalidPayload)?;
        pty::resize(terminal, size).map_err(|_| ErrorCode::Failed)?;
        self.screen.resize(screen_size);
        self.size = size;
        Ok(())
    }

    pub(crate) fn signal_child(&self, signal: ChildSignal) -> Result<(), ErrorCode> {
        if self.child.has_exited() {
            return Err(ErrorCode::ChildExited);
        }
        self.child.signal(signal).map_err(|_| ErrorCode::Failed)
    }

    pub(crate) fn status(&self) -> ChildStatus {
        self.child.status()
    }

    pub(crate) fn child_exited(&self) -> bool {
        self.child.has_exited()
    }

    /// Collects the exit status if the child ended. On the call that finds
    /// the end, reads the output that the child left in the terminal, closes
    /// the terminal and returns `true`.
    pub(crate) fn reap_child(&mut self) -> io::Result<bool> {
        if !self.child.reap()? {
            return Ok(false);
        }
        for _ in 0..EXIT_DRAIN_CHUNKS {
            if !self.read_output()? {
                break;
            }
        }
        self.terminal = None;
        self.input.clear();
        Ok(true)
    }
}

/// The size of the screen state for a window. `None` if the window has no
/// cell, or more cells than the holder keeps in memory.
fn screen_size(size: WindowSize) -> Option<Size> {
    if size.cells() > WindowSize::MAX_CELLS {
        return None;
    }
    Size::new(size.rows, size.columns)
}
