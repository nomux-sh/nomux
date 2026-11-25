//! One connection of the daemon to the control socket of a holder.

use std::collections::VecDeque;
use std::io;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::wire::{
    DecodeError, FrameBuffer, Hello, Reply, Request, receive_with_descriptor,
};
use tokio::io::{AsyncWriteExt, Interest};
use tokio::net::UnixStream;
use tokio::time::timeout;

/// The longest time for the connect, the hello and the lease. A holder that
/// runs answers in less than a millisecond.
const OPEN_TIMEOUT: Duration = Duration::from_secs(1);

/// A connection to a holder, and what the holder said in its hello.
pub(crate) struct HolderLink {
    pub(crate) connection: HolderConnection,
    pub(crate) hello: Hello,
    /// The read-only descriptor of the ring buffer.
    pub(crate) ring: OwnedFd,
}

/// Connects to the holder of `session_id` and takes the single-writer lease.
pub(crate) async fn open_with_lease(
    socket: &Path,
    session_id: &SessionId,
) -> io::Result<HolderLink> {
    within_open_timeout(async {
        let mut link = connect(socket, session_id).await?;
        link.connection.send(Request::AcquireLease).await?;
        match link.connection.receive().await? {
            (_, Reply::Done) => Ok(link),
            (_, reply) => Err(invalid(format!("the holder refused the lease: {reply:?}"))),
        }
    })
    .await
}

/// Connects to the holder of `session_id` without the lease. Such a
/// connection can read the terminal and cannot write to it.
pub(crate) async fn open_reader(socket: &Path, session_id: &SessionId) -> io::Result<HolderLink> {
    within_open_timeout(connect(socket, session_id)).await
}

async fn within_open_timeout(
    opening: impl Future<Output = io::Result<HolderLink>>,
) -> io::Result<HolderLink> {
    timeout(OPEN_TIMEOUT, opening)
        .await
        .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into()))
}

async fn connect(socket: &Path, session_id: &SessionId) -> io::Result<HolderLink> {
    let mut connection = HolderConnection::connect(socket).await?;
    let hello = connection.receive_hello().await?;
    if hello.session_id != *session_id {
        return Err(invalid(format!(
            "the holder owns session {}, not {session_id}",
            hello.session_id
        )));
    }
    let ring = connection
        .take_descriptor()
        .ok_or_else(|| invalid("the hello has no descriptor of the ring buffer"))?;
    Ok(HolderLink {
        connection,
        hello,
        ring,
    })
}

/// The frames of one connection to a holder.
pub(crate) struct HolderConnection {
    stream: UnixStream,
    frames: FrameBuffer,
    outgoing: Vec<u8>,
    next_request_id: u32,
    /// The descriptors that came with the frames, in their sequence: the
    /// ring buffer with the hello, and the memory of each redraw reply.
    descriptors: VecDeque<OwnedFd>,
}

impl HolderConnection {
    async fn connect(socket: &Path) -> io::Result<Self> {
        Ok(Self {
            stream: UnixStream::connect(socket).await?,
            frames: FrameBuffer::new(),
            outgoing: Vec::new(),
            next_request_id: 1,
            descriptors: VecDeque::new(),
        })
    }

    /// Reads the first frame of the connection. Fails if the holder has a
    /// different major protocol version.
    async fn receive_hello(&mut self) -> io::Result<Hello> {
        loop {
            if let Some(frame) = self.frames.next_frame().map_err(invalid)? {
                return Hello::decode(frame).map_err(invalid);
            }
            self.fill().await?;
        }
    }

    /// Sends one request and returns its identifier. The holder answers each
    /// request with one reply that has the same identifier, in the sequence
    /// of the requests. Only the reply to `WaitOutput` can come later.
    pub(super) async fn send(&mut self, request: Request<'_>) -> io::Result<u32> {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        self.outgoing.clear();
        request.encode(request_id, &mut self.outgoing);
        self.stream.write_all(&self.outgoing).await?;
        Ok(request_id)
    }

    /// Reads the next reply and the identifier of its request. If the future
    /// is dropped before it completes, no byte is lost.
    pub(super) async fn receive(&mut self) -> io::Result<(u32, Reply)> {
        loop {
            match self.frames.next_frame().map_err(invalid)? {
                Some(frame) => match Reply::decode(frame) {
                    Ok(reply) => return Ok((frame.request_id, reply)),
                    // A reply kind of a later minor version of the protocol.
                    Err(DecodeError::UnknownKind(_)) => {}
                    Err(error) => return Err(invalid(error)),
                },
                None => self.fill().await?,
            }
        }
    }

    /// The oldest descriptor that came with a frame and that no caller took.
    pub(super) fn take_descriptor(&mut self) -> Option<OwnedFd> {
        self.descriptors.pop_front()
    }

    /// A descriptor arrives with the first byte of its message, thus each
    /// read of the socket is a `recvmsg`.
    async fn fill(&mut self) -> io::Result<()> {
        let Self { stream, frames, .. } = self;
        let (read, descriptor) = stream
            .async_io(Interest::READABLE, || {
                receive_with_descriptor(&*stream, frames.unfilled())
            })
            .await?;
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.frames.advance(read);
        self.descriptors.extend(descriptor);
        Ok(())
    }
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
