//! The terminal of a session for an attached client: the screen and the
//! output from the daemon, and the input to the daemon.
//!
//! This module moves bytes. It does not read a keyboard, does not write to
//! a terminal and does not know escape sequences: a CLI and a graphical
//! client use it in the same way.

use std::io;

use nomux_proto::frame::{EndReason, FrameKind, MAX_PAYLOAD_LEN};
use nomux_transport::{FrameReader, FrameWriter};

use crate::{ClientError, WindowSize};

/// A position in the output of a session: the epoch, and the sequence number
/// of the next output byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputPosition {
    /// The epoch of the session.
    pub epoch: u32,
    /// The number of output bytes of the epoch before this position.
    pub seq: u64,
}

/// How a client attaches to a session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachOptions {
    /// The window size of the client. The session gets this size. With
    /// `None`, the session keeps its size.
    pub size: Option<WindowSize>,
    /// The position after the last output that the client applied in an
    /// earlier attach to the same session. If the daemon still has the
    /// output from this position, the client gets only that output and no
    /// snapshot. A client that starts with a new terminal gives `None`.
    pub resume: Option<OutputPosition>,
}

/// How an attach ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachEnd {
    /// The command of the session ended with an exit code.
    Exited {
        /// The exit code.
        code: i32,
    },
    /// A signal ended the command of the session.
    Signaled {
        /// The signal number of the operating system of the host.
        signal: i32,
    },
    /// A request detached the clients of the session. The session continues.
    Detached,
    /// The session ended, and the daemon does not know how the command
    /// ended.
    Lost,
    /// A reason from a later protocol version.
    Unknown,
}

/// One step of the terminal of an attached client.
#[derive(Debug, PartialEq, Eq)]
pub enum TerminalEvent<'a> {
    /// A part of a snapshot of the screen: bytes of a redraw sequence, which
    /// makes the current screen in a terminal that is in its initial state.
    Snapshot {
        /// The next bytes of the redraw sequence.
        bytes: &'a [u8],
        /// `true` for the first part of a snapshot. The client puts its
        /// terminal into the initial state before it applies these bytes.
        starts: bool,
    },
    /// Bytes that the command of the session wrote to its terminal.
    Output(&'a [u8]),
    /// The attach ended. No event follows.
    End(AttachEnd),
}

/// The screen and the output of a session, from the daemon.
#[derive(Debug)]
pub struct TerminalOutput {
    frames: FrameReader,
    /// The session handle of the frames of this attach.
    handle: u32,
    epoch: u32,
    /// The sequence number of the next output byte. `None` before the first
    /// frame of a client that does not resume.
    next_seq: Option<u64>,
    /// `true` if the last frame was a part of a snapshot.
    in_snapshot: bool,
}

impl TerminalOutput {
    pub(crate) fn new(
        frames: FrameReader,
        handle: u32,
        epoch: u32,
        resume: Option<OutputPosition>,
    ) -> Self {
        Self {
            frames,
            handle,
            epoch,
            next_seq: resume
                .filter(|position| position.epoch == epoch)
                .map(|position| position.seq),
            in_snapshot: false,
        }
    }

    /// Waits for the next event.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Connection`] if the connection ends before the
    /// attach ends, which occurs when the daemon stops, and if the daemon
    /// sends output with a gap or a repeat. The session continues in the
    /// two cases.
    pub async fn next(&mut self) -> Result<TerminalEvent<'_>, ClientError> {
        let header = loop {
            let received = self.frames.receive().await;
            match received.map_err(ClientError::Connection)? {
                Some(header) if header.session == self.handle => break header,
                Some(_) => {}
                None => {
                    return Err(ClientError::Connection(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the daemon closed the connection",
                    )));
                }
            }
        };
        match header.kind {
            FrameKind::ScreenSnapshot => {
                let starts = !self.in_snapshot || self.next_seq != Some(header.seq);
                self.in_snapshot = true;
                self.next_seq = Some(header.seq);
                Ok(TerminalEvent::Snapshot {
                    bytes: self.frames.payload(),
                    starts,
                })
            }
            FrameKind::Output => {
                if self.next_seq != Some(header.seq) {
                    return Err(ClientError::invalid_reply(
                        "the output from the daemon has a gap or a repeat",
                    ));
                }
                self.in_snapshot = false;
                self.next_seq = Some(header.seq + u64::from(header.length));
                Ok(TerminalEvent::Output(self.frames.payload()))
            }
            FrameKind::End => EndReason::decode(self.frames.payload())
                .map(|reason| TerminalEvent::End(reason.into()))
                .ok_or_else(|| ClientError::invalid_reply("the end of the attach has no reason")),
            _ => Err(ClientError::invalid_reply(
                "the daemon sent a frame that an attach does not have",
            )),
        }
    }

    /// The position after the last output that [`next`](Self::next) gave.
    /// A later attach can resume there. `None` before the first event.
    #[must_use]
    pub fn position(&self) -> Option<OutputPosition> {
        self.next_seq.map(|seq| OutputPosition {
            epoch: self.epoch,
            seq,
        })
    }
}

impl From<EndReason> for AttachEnd {
    fn from(reason: EndReason) -> Self {
        match reason {
            EndReason::Exited { code } => Self::Exited { code },
            EndReason::Signaled { signal } => Self::Signaled { signal },
            EndReason::Detached => Self::Detached,
            EndReason::Lost => Self::Lost,
            EndReason::Unrecognized(_) => Self::Unknown,
        }
    }
}

/// The input of an attached client, to the daemon. The input has its own
/// direction of the connection: it does not wait for the output.
///
/// The attach ends when this value is dropped: that is how a client
/// detaches. A client that only shows the session keeps the value.
#[derive(Debug)]
pub struct TerminalInput {
    frames: FrameWriter,
    handle: u32,
}

impl TerminalInput {
    pub(crate) fn new(frames: FrameWriter, handle: u32) -> Self {
        Self { frames, handle }
    }

    /// Sends bytes to the terminal of the session, as the keyboard of the
    /// user makes them.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Connection`] if the connection failed.
    pub async fn send(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
        for part in bytes.chunks(MAX_PAYLOAD_LEN as usize) {
            self.frames
                .send(FrameKind::Input, self.handle, 0, 0, part)
                .await
                .map_err(ClientError::Connection)?;
        }
        Ok(())
    }
}
