//! One connection of the daemon.

use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use nomux_holder_protocol::wire::{FrameBuffer, Reply, send_with_descriptor};

/// A `WaitOutput` request that has no reply yet.
pub(super) struct PendingWait {
    pub(super) request_id: u32,
    pub(super) after: u64,
}

pub(super) struct Connection {
    pub(super) stream: UnixStream,
    pub(super) frames: FrameBuffer,
    pub(super) outgoing: Vec<u8>,
    pub(super) pending_wait: Option<PendingWait>,
}

impl Connection {
    /// Takes a new connection and sends the hello frame with the descriptor
    /// of the ring buffer.
    pub(super) fn open(
        stream: UnixStream,
        hello_frame: &[u8],
        ring: BorrowedFd<'_>,
    ) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        let sent = send_with_descriptor(&stream, hello_frame, ring)?;
        if sent != hello_frame.len() {
            return Err(io::ErrorKind::WriteZero.into());
        }
        Ok(Self {
            stream,
            frames: FrameBuffer::new(),
            outgoing: Vec::new(),
            pending_wait: None,
        })
    }

    /// Reads the bytes that the socket has now. Fails if the daemon closed
    /// the connection.
    pub(super) fn receive(&mut self) -> io::Result<()> {
        match self.stream.read(self.frames.unfilled()) {
            Ok(0) => Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => {
                self.frames.advance(count);
                Ok(())
            }
            Err(error) if is_transient(&error) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// A reply, and the descriptor that goes with it as ancillary data.
pub(super) struct Answer {
    pub(super) reply: Reply,
    pub(super) descriptor: Option<OwnedFd>,
}

impl From<Reply> for Answer {
    fn from(reply: Reply) -> Self {
        Self {
            reply,
            descriptor: None,
        }
    }
}

/// Writes one answer to the socket of a connection.
///
/// A reply is small. If the socket does not take it in one write, the daemon
/// does not read its replies, and the connection must close.
pub(super) fn send_answer(
    mut stream: &UnixStream,
    outgoing: &mut Vec<u8>,
    request_id: u32,
    answer: &Answer,
) -> io::Result<()> {
    outgoing.clear();
    answer.reply.encode(request_id, outgoing);
    let written = match &answer.descriptor {
        Some(descriptor) => send_with_descriptor(stream, outgoing, descriptor.as_fd())?,
        None => stream.write(outgoing)?,
    };
    if written != outgoing.len() {
        return Err(io::ErrorKind::WriteZero.into());
    }
    Ok(())
}

fn is_transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}
