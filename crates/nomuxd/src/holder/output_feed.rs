//! The read side of one holder for the clients that are attached to its
//! session: the ring buffer, the redraw sequence of the screen, and the
//! signal that the command wrote output.
//!
//! The feed has its own connection to the holder, without the lease. The
//! connection of the supervisor waits for the end of the command, and a
//! connection can have one `WaitOutput` request at a time. The feed exists
//! only while a client is attached, thus a session without a client costs
//! the daemon nothing when its command writes.

use std::collections::VecDeque;
use std::io;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::redraw;
use nomux_holder_protocol::ring::RingReader;
use nomux_holder_protocol::wire::{ErrorCode, Reply, Request};
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tracing::debug;

use super::connection::{HolderConnection, open_reader};

/// A few clients can ask for the screen at the same moment.
const REDRAW_QUEUE: usize = 8;

/// The redraw sequence of a screen and the position of the ring buffer that
/// the screen corresponds to: the sequence has the effect of each byte
/// before the position, and of no byte from the position on.
pub(crate) struct Redraw {
    pub(crate) position: u64,
    pub(crate) sequence: Vec<u8>,
}

type RedrawWaiter = oneshot::Sender<io::Result<Redraw>>;

/// The feed of one session. The attached clients of the session share it.
/// When the last client drops it, its connection to the holder closes.
pub(crate) struct OutputFeed {
    ring: OwnedFd,
    redraw_requests: mpsc::Sender<RedrawWaiter>,
    write_position: watch::Receiver<u64>,
}

impl OutputFeed {
    async fn open(socket: &Path, session_id: &SessionId) -> io::Result<Self> {
        let link = open_reader(socket, session_id).await?;
        if !link.hello.protocol.supports(&Request::Redraw) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the holder is too old to give its screen",
            ));
        }
        let (redraw_requests, waiters) = mpsc::channel(REDRAW_QUEUE);
        let (position, write_position) = watch::channel(0);
        let session_id = session_id.clone();
        tokio::spawn(async move {
            if let Err(error) = serve(link.connection, waiters, &position).await {
                debug!(session = %session_id, %error, "the output feed of the session ended");
            }
        });
        Ok(Self {
            ring: link.ring,
            redraw_requests,
            write_position,
        })
    }

    /// A new reader of the ring buffer. Each attached client has its own
    /// reader, thus a slow client changes nothing for the other clients.
    pub(crate) fn reader(&self) -> io::Result<RingReader> {
        RingReader::map(&self.ring)
    }

    /// Asks the holder for the current screen.
    pub(crate) async fn redraw(&self) -> io::Result<Redraw> {
        let (waiter, answer) = oneshot::channel();
        self.redraw_requests
            .send(waiter)
            .await
            .map_err(|_| feed_ended())?;
        answer.await.map_err(|_| feed_ended())?
    }

    /// The write position of the ring buffer that the holder reported last.
    /// The channel closes when the feed lost its connection to the holder.
    pub(crate) fn write_position(&self) -> watch::Receiver<u64> {
        self.write_position.clone()
    }
}

/// Owns the connection: keeps one `WaitOutput` request and one `Redraw`
/// request at most with the holder. Ends when no client uses the feed.
async fn serve(
    mut connection: HolderConnection,
    mut waiters: mpsc::Receiver<RedrawWaiter>,
    position: &watch::Sender<u64>,
) -> io::Result<()> {
    // The first waiter is the one whose request is with the holder.
    let mut redraws: VecDeque<RedrawWaiter> = VecDeque::new();
    let mut reported = 0;
    connection
        .send(Request::WaitOutput { after: reported })
        .await?;
    loop {
        tokio::select! {
            reply = connection.receive() => match reply?.1 {
                Reply::Output { write_position } => {
                    position.send_replace(write_position);
                    // An answer without new output: the command has ended,
                    // and a new request gets the same answer immediately.
                    if write_position > reported {
                        reported = write_position;
                        connection.send(Request::WaitOutput { after: reported }).await?;
                    }
                }
                Reply::Redraw { position, length } => {
                    let memory = connection.take_descriptor().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "the redraw reply has no descriptor")
                    })?;
                    let redraw = redraw::read(memory, length).map(|sequence| Redraw { position, sequence });
                    answer_first(&mut connection, &mut redraws, redraw).await?;
                }
                // `WaitOutput` does not fail on this connection, thus the
                // error is the answer to the `Redraw` request.
                Reply::Error(code) => {
                    answer_first(&mut connection, &mut redraws, Err(refused(code))).await?;
                }
                // The replies to requests that the feed does not send.
                _ => {}
            },
            waiter = waiters.recv() => {
                let Some(waiter) = waiter else {
                    return Ok(());
                };
                redraws.push_back(waiter);
                // The holder gets no second `Redraw` request before it
                // answered the first one: the descriptor of a reply has no
                // request identifier.
                if redraws.len() == 1 {
                    connection.send(Request::Redraw).await?;
                }
            }
        }
    }
}

/// Gives the answer of the holder to the client that asked first, and sends
/// the request of the next client.
async fn answer_first(
    connection: &mut HolderConnection,
    redraws: &mut VecDeque<RedrawWaiter>,
    answer: io::Result<Redraw>,
) -> io::Result<()> {
    if let Some(waiter) = redraws.pop_front() {
        // A client that is gone stopped waiting.
        let _ = waiter.send(answer);
    }
    if !redraws.is_empty() {
        connection.send(Request::Redraw).await?;
    }
    Ok(())
}

fn refused(code: ErrorCode) -> io::Error {
    io::Error::other(format!("the holder did not give its screen: {code:?}"))
}

fn feed_ended() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "the connection to the holder is lost",
    )
}

/// Gives each client that attaches to one session the same feed, and opens
/// the feed when the first client comes.
pub(crate) struct SharedOutputFeed {
    holder_socket: PathBuf,
    session_id: SessionId,
    /// The lock is held while the feed opens, thus two clients that attach
    /// at the same moment do not open two connections to the holder.
    current: Mutex<Weak<OutputFeed>>,
}

impl SharedOutputFeed {
    pub(crate) fn new(holder_socket: PathBuf, session_id: SessionId) -> Self {
        Self {
            holder_socket,
            session_id,
            current: Mutex::new(Weak::new()),
        }
    }

    /// The feed of the session. A feed that lost its connection is replaced.
    pub(crate) async fn get(&self) -> io::Result<Arc<OutputFeed>> {
        let mut current = self.current.lock().await;
        if let Some(feed) = current
            .upgrade()
            .filter(|feed| !feed.redraw_requests.is_closed())
        {
            return Ok(feed);
        }
        let feed = Arc::new(OutputFeed::open(&self.holder_socket, &self.session_id).await?);
        *current = Arc::downgrade(&feed);
        Ok(feed)
    }
}
