//! The terminal of a session for an attached client: the screen and the
//! output from the daemon, and the input to the daemon.
//!
//! This module moves bytes. It does not read a keyboard, does not write to
//! a terminal and does not know escape sequences: a CLI and a graphical
//! client use it in the same way.

use std::io;

use nomux_proto::frame::{EndReason, FrameHeader, FrameKind, MAX_PAYLOAD_LEN};
use nomux_transport::{FrameReader, FrameWriter};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::client::AttachedFrames;
use crate::reconnect::Backoff;
use crate::{ClientError, ClientKind, Host, WindowSize};

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
    /// The connection to the host ended, and the attach connects again.
    /// The client shows that the session does not answer now. Input is
    /// discarded until [`Reconnected`](Self::Reconnected).
    ConnectionLost,
    /// The attach has a connection again. The events continue at the
    /// output byte after the last one that the client got, or with a
    /// snapshot if the daemon no longer has that byte.
    Reconnected,
    /// The attach ended. No event follows.
    End(AttachEnd),
}

/// What an attach needs to connect again: the host, and the way to give
/// the input side its new channel.
#[derive(Debug)]
pub(crate) struct Reattach {
    pub(crate) host: Host,
    pub(crate) kind: ClientKind,
    pub(crate) session_id: String,
    pub(crate) new_input: mpsc::UnboundedSender<(FrameWriter, u32)>,
}

/// What [`TerminalOutput::receive`] got.
enum Received {
    Frame(FrameHeader),
    ConnectionLost,
    Reconnected,
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
    /// `None`: the attach ends with an error when its connection ends.
    reattach: Option<Reattach>,
    /// `true` after [`TerminalEvent::ConnectionLost`], until the attach has
    /// a connection again.
    connection_is_lost: bool,
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
            reattach: None,
            connection_is_lost: false,
        }
    }

    /// Makes the attach connect again when its connection ends.
    pub(crate) fn reattaching(mut self, reattach: Reattach) -> Self {
        self.reattach = Some(reattach);
        self
    }

    /// Waits for the next event.
    ///
    /// An attach of [`Host::attach`] connects again when its connection
    /// ends: the events are then [`TerminalEvent::ConnectionLost`] and,
    /// after a new connection, [`TerminalEvent::Reconnected`]. The attempts
    /// have a limit and a pause that grows.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Connection`] if the connection ends before the
    /// attach ends and the attach cannot connect again, and if the daemon
    /// sends output with a gap or a repeat. Returns
    /// [`ClientError::DaemonNotRunning`] if the daemon stopped. The session
    /// continues in these cases.
    pub async fn next(&mut self) -> Result<TerminalEvent<'_>, ClientError> {
        let header = match self.receive().await? {
            Received::Frame(header) => header,
            Received::ConnectionLost => return Ok(TerminalEvent::ConnectionLost),
            Received::Reconnected => return Ok(TerminalEvent::Reconnected),
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

    /// Receives the next frame of this attach. If the connection ended and
    /// the attach can connect again, reports that and connects on the next
    /// call.
    async fn receive(&mut self) -> Result<Received, ClientError> {
        if self.connection_is_lost {
            self.connect_again().await?;
            self.connection_is_lost = false;
            return Ok(Received::Reconnected);
        }
        let ended = loop {
            match self.frames.receive().await {
                Ok(Some(header)) if header.session == self.handle => {
                    return Ok(Received::Frame(header));
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    break io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the daemon closed the connection",
                    );
                }
                Err(error) => break error,
            }
        };
        let ended = ClientError::Connection(ended);
        if self.reattach.is_some() && ended.is_connection_loss() {
            self.connection_is_lost = true;
            Ok(Received::ConnectionLost)
        } else {
            Err(ended)
        }
    }

    /// Attaches again from the position of this attach, on a new connection
    /// to the daemon. Tries again, with a pause that grows, while the host
    /// cannot be reached. Starts no daemon: a daemon that stopped ends the
    /// attach.
    async fn connect_again(&mut self) -> Result<(), ClientError> {
        let Some(reattach) = &self.reattach else {
            return Err(ClientError::DaemonNotRunning);
        };
        let options = AttachOptions {
            size: None,
            resume: self.position(),
        };
        let mut backoff = Backoff::default();
        let attached = loop {
            let attempt = async {
                let client = reattach.host.running_client(reattach.kind).await?;
                let client = client.ok_or(ClientError::DaemonNotRunning)?;
                client.attach_frames(&reattach.session_id, options).await
            };
            match attempt.await {
                Ok(attached) => break attached,
                Err(error) if error.is_connection_loss() => match backoff.next_pause() {
                    Some(pause) => sleep(pause).await,
                    None => return Err(error),
                },
                Err(error) => return Err(error),
            }
        };
        let AttachedFrames {
            output,
            input,
            handle,
            epoch,
        } = attached;
        // The input side is gone if the client dropped it. The new
        // connection then closes with this function.
        let _ = reattach.new_input.send((input, handle));
        self.frames = output;
        self.handle = handle;
        if epoch != self.epoch {
            self.epoch = epoch;
            self.next_seq = None;
        }
        self.in_snapshot = false;
        Ok(())
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
    /// `None` while an attach that connects again has no connection.
    frames: Option<FrameWriter>,
    handle: u32,
    /// The channels of the new connections of an attach that connects
    /// again. `None`: the attach has one connection.
    new_frames: Option<mpsc::UnboundedReceiver<(FrameWriter, u32)>>,
}

impl TerminalInput {
    pub(crate) fn new(frames: FrameWriter, handle: u32) -> Self {
        Self {
            frames: Some(frames),
            handle,
            new_frames: None,
        }
    }

    /// Makes the input side of an attach that connects again. Returns also
    /// the sender that gives it the channel of each new connection.
    pub(crate) fn reattaching(
        frames: FrameWriter,
        handle: u32,
    ) -> (Self, mpsc::UnboundedSender<(FrameWriter, u32)>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let input = Self {
            frames: Some(frames),
            handle,
            new_frames: Some(receiver),
        };
        (input, sender)
    }

    /// Sends bytes to the terminal of the session, as the keyboard of the
    /// user makes them.
    ///
    /// An attach that connects again discards the bytes while it has no
    /// connection: nomux does not type into a session that the user cannot
    /// see.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Connection`] if the connection failed and the
    /// attach does not connect again.
    pub async fn send(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
        if let Some(new_frames) = &mut self.new_frames {
            while let Ok((frames, handle)) = new_frames.try_recv() {
                self.frames = Some(frames);
                self.handle = handle;
            }
        }
        let Some(frames) = &mut self.frames else {
            return Ok(());
        };
        match send_input(frames, self.handle, bytes).await {
            Ok(()) => Ok(()),
            Err(error) if self.new_frames.is_none() => Err(ClientError::Connection(error)),
            Err(_) => {
                self.frames = None;
                Ok(())
            }
        }
    }
}

async fn send_input(frames: &mut FrameWriter, handle: u32, bytes: &[u8]) -> io::Result<()> {
    for part in bytes.chunks(MAX_PAYLOAD_LEN as usize) {
        frames.send(FrameKind::Input, handle, 0, 0, part).await?;
    }
    Ok(())
}
