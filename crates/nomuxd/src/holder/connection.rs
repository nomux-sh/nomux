//! One connection of the daemon to the control socket of a holder.

use std::io;
use std::path::Path;
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::wire::{DecodeError, FrameBuffer, Hello, Reply, Request};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::timeout;

/// The longest time for the connect, the hello and the lease. A holder that
/// runs answers in less than a millisecond.
const OPEN_TIMEOUT: Duration = Duration::from_secs(1);

/// A connection that has the single-writer lease of its holder.
pub(crate) struct HolderLink {
    pub(crate) connection: HolderConnection,
    /// The process ID of the command of the session.
    pub(crate) command_pid: u32,
}

/// Connects to the holder of `session_id` and takes the single-writer lease.
pub(crate) async fn open(socket: &Path, session_id: &SessionId) -> io::Result<HolderLink> {
    let opening = async {
        let mut connection = HolderConnection::connect(socket).await?;
        let hello = connection.receive_hello().await?;
        if hello.session_id != *session_id {
            return Err(invalid(format!(
                "the holder owns session {}, not {session_id}",
                hello.session_id
            )));
        }
        connection.send(Request::AcquireLease).await?;
        match connection.receive().await? {
            Reply::Done => Ok(HolderLink {
                connection,
                command_pid: hello.child_pid,
            }),
            reply => Err(invalid(format!("the holder refused the lease: {reply:?}"))),
        }
    };
    timeout(OPEN_TIMEOUT, opening)
        .await
        .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into()))
}

/// The frames of one connection to a holder.
///
/// The daemon does not read terminal data yet. Thus it reads the hello as
/// plain bytes, and the operating system closes the descriptor of the ring
/// buffer that comes with the hello.
pub(crate) struct HolderConnection {
    stream: UnixStream,
    frames: FrameBuffer,
    outgoing: Vec<u8>,
    next_request_id: u32,
}

impl HolderConnection {
    async fn connect(socket: &Path) -> io::Result<Self> {
        Ok(Self {
            stream: UnixStream::connect(socket).await?,
            frames: FrameBuffer::new(),
            outgoing: Vec::new(),
            next_request_id: 1,
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

    /// Sends one request. The holder answers each request with one reply,
    /// in the sequence of the requests. Only the reply to `WaitOutput` can
    /// come later.
    pub(super) async fn send(&mut self, request: Request<'_>) -> io::Result<()> {
        self.outgoing.clear();
        request.encode(self.next_request_id, &mut self.outgoing);
        self.next_request_id = self.next_request_id.wrapping_add(1);
        self.stream.write_all(&self.outgoing).await
    }

    /// Reads the next reply. If the future is dropped before it completes,
    /// no byte is lost.
    pub(super) async fn receive(&mut self) -> io::Result<Reply> {
        loop {
            match self.frames.next_frame().map_err(invalid)? {
                Some(frame) => match Reply::decode(frame) {
                    Ok(reply) => return Ok(reply),
                    // A reply kind of a later minor version of the protocol.
                    Err(DecodeError::UnknownKind(_)) => {}
                    Err(error) => return Err(invalid(error)),
                },
                None => self.fill().await?,
            }
        }
    }

    async fn fill(&mut self) -> io::Result<()> {
        let read = self.stream.read(self.frames.unfilled()).await?;
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.frames.advance(read);
        Ok(())
    }
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
