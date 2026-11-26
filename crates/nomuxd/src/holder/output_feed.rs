//! The read side of one holder: the ring buffer, the redraw sequence of the
//! screen, the output of the journal, and the signal that the command wrote
//! output.
//!
//! The feed has its own connection to the holder, without the lease. The
//! connection of the supervisor waits for the end of the command, and a
//! connection can have one `WaitOutput` request at a time. The feed exists
//! only while a client reads the session, thus a session without a client
//! costs the daemon nothing when its command writes.

use std::collections::VecDeque;
use std::io;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::journal::{self, JournalRange};
use nomux_holder_protocol::redraw;
use nomux_holder_protocol::ring::RingReader;
use nomux_holder_protocol::wire::{ErrorCode, Reply, Request};
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tracing::debug;

use super::connection::{HolderConnection, open_reader};

/// A few clients can ask the holder at the same moment.
const QUESTION_QUEUE: usize = 8;

/// The redraw sequence of a screen and the position of the ring buffer that
/// the screen corresponds to: the sequence has the effect of each byte
/// before the position, and of no byte from the position on.
pub(crate) struct Redraw {
    pub(crate) position: u64,
    pub(crate) sequence: Vec<u8>,
}

/// A request whose reply comes with shared memory. The holder gets one of
/// them at a time: the descriptor of a reply has no request identifier.
enum Question {
    Redraw(oneshot::Sender<io::Result<Redraw>>),
    Journal {
        from: u64,
        max_length: u32,
        answer: oneshot::Sender<io::Result<Option<JournalRange>>>,
    },
}

impl Question {
    fn request(&self) -> Request<'static> {
        match self {
            Self::Redraw(_) => Request::Redraw,
            Self::Journal {
                from, max_length, ..
            } => Request::JournalRead {
                from: *from,
                max_length: *max_length,
            },
        }
    }

    /// Gives the error to the client that asked. A client that is gone
    /// stopped waiting.
    fn fail(self, error: io::Error) {
        match self {
            Self::Redraw(answer) => drop(answer.send(Err(error))),
            Self::Journal { answer, .. } => drop(answer.send(Err(error))),
        }
    }
}

/// The feed of one session. The clients that read the session share it.
/// When the last client drops it, its connection to the holder closes.
pub(crate) struct OutputFeed {
    ring: OwnedFd,
    /// The epoch of the holder: one continuous numbering of its output.
    epoch: u32,
    /// `false` for a holder that is too old to have a journal.
    has_journal: bool,
    questions: mpsc::Sender<Question>,
    write_position: watch::Receiver<u64>,
}

impl OutputFeed {
    async fn open(socket: &Path, session_id: &SessionId) -> io::Result<Self> {
        let link = open_reader(socket, session_id).await?;
        let protocol = link.hello.protocol;
        if !protocol.supports(&Request::Redraw) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the holder is too old to give its screen",
            ));
        }
        let (questions, asked) = mpsc::channel(QUESTION_QUEUE);
        let (position, write_position) = watch::channel(0);
        let session_id = session_id.clone();
        tokio::spawn(async move {
            if let Err(error) = serve(link.connection, asked, &position).await {
                debug!(session = %session_id, %error, "the output feed of the session ended");
            }
        });
        Ok(Self {
            ring: link.ring,
            epoch: link.hello.epoch,
            has_journal: protocol.supports(&Request::JournalRead {
                from: 0,
                max_length: 1,
            }),
            questions,
            write_position,
        })
    }

    /// The epoch of the holder of the session.
    pub(crate) fn epoch(&self) -> u32 {
        self.epoch
    }

    /// A new reader of the ring buffer. Each attached client has its own
    /// reader, thus a slow client changes nothing for the other clients.
    pub(crate) fn reader(&self) -> io::Result<RingReader> {
        RingReader::map(&self.ring)
    }

    /// Asks the holder for the current screen.
    pub(crate) async fn redraw(&self) -> io::Result<Redraw> {
        let (answer, redraw) = oneshot::channel();
        self.ask(Question::Redraw(answer), redraw).await
    }

    /// Asks the holder for output bytes of its epoch from the position
    /// `from` on: `max_length` bytes at most. The holder gives the bytes of
    /// its journal, also the ones that it did not write to a file yet.
    ///
    /// The range starts after `from` if the journal does not have the byte
    /// at `from`. Returns `None` if the journal has no byte at `from` or
    /// after it, and for a holder that has no journal.
    pub(crate) async fn journal_read(
        &self,
        from: u64,
        max_length: u32,
    ) -> io::Result<Option<JournalRange>> {
        if !self.has_journal {
            return Ok(None);
        }
        let (answer, range) = oneshot::channel();
        let question = Question::Journal {
            from,
            max_length,
            answer,
        };
        self.ask(question, range).await
    }

    /// The write position of the ring buffer that the holder reported last.
    /// The channel closes when the feed lost its connection to the holder.
    pub(crate) fn write_position(&self) -> watch::Receiver<u64> {
        self.write_position.clone()
    }

    async fn ask<T>(
        &self,
        question: Question,
        answer: oneshot::Receiver<io::Result<T>>,
    ) -> io::Result<T> {
        self.questions
            .send(question)
            .await
            .map_err(|_| feed_ended())?;
        answer.await.map_err(|_| feed_ended())?
    }
}

/// Owns the connection: keeps one `WaitOutput` request and one question at
/// most with the holder. Ends when no client uses the feed.
async fn serve(
    mut connection: HolderConnection,
    mut asked: mpsc::Receiver<Question>,
    position: &watch::Sender<u64>,
) -> io::Result<()> {
    // The first question is the one whose request is with the holder.
    let mut questions: VecDeque<Question> = VecDeque::new();
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
                // `WaitOutput` does not fail on this connection, thus each
                // other reply is the answer to the first question.
                reply => {
                    if let Some(question) = questions.pop_front() {
                        answer(question, reply, &mut connection)?;
                    }
                    if let Some(next) = questions.front() {
                        connection.send(next.request()).await?;
                    }
                }
            },
            question = asked.recv() => {
                let Some(question) = question else {
                    return Ok(());
                };
                if questions.is_empty() {
                    connection.send(question.request()).await?;
                }
                questions.push_back(question);
            }
        }
    }
}

/// Gives the reply of the holder to the client that asked.
///
/// # Errors
///
/// Returns an error if the reply does not have the descriptor of its
/// shared memory. The descriptors of the later replies cannot be trusted
/// then, thus the connection ends.
fn answer(question: Question, reply: Reply, connection: &mut HolderConnection) -> io::Result<()> {
    let mut memory = || {
        connection.take_descriptor().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the reply of the holder has no descriptor",
            )
        })
    };
    match (question, reply) {
        (Question::Redraw(answer), Reply::Redraw { position, length }) => {
            let redraw =
                redraw::read(memory()?, length).map(|sequence| Redraw { position, sequence });
            // A client that is gone stopped waiting.
            let _ = answer.send(redraw);
        }
        (Question::Journal { answer, .. }, Reply::Journal { length: 0, .. }) => {
            let _ = answer.send(Ok(None));
        }
        (Question::Journal { answer, .. }, Reply::Journal { position, length }) => {
            let range = journal::read_range(memory()?, length)
                .map(|bytes| Some(JournalRange { position, bytes }));
            let _ = answer.send(range);
        }
        (question, Reply::Error(code)) => question.fail(refused(code)),
        (question, _) => question.fail(io::Error::new(
            io::ErrorKind::InvalidData,
            "the holder sent a reply that does not answer its request",
        )),
    }
    Ok(())
}

fn refused(code: ErrorCode) -> io::Error {
    io::Error::other(format!("the holder refused the request: {code:?}"))
}

fn feed_ended() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "the connection to the holder is lost",
    )
}

/// Gives each client that reads one session the same feed, and opens the
/// feed when the first client comes.
pub(crate) struct SharedOutputFeed {
    holder_socket: PathBuf,
    session_id: SessionId,
    /// The lock is held while the feed opens, thus two clients that come at
    /// the same moment do not open two connections to the holder.
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
        if let Some(feed) = current.upgrade().filter(|feed| !feed.questions.is_closed()) {
            return Ok(feed);
        }
        let feed = Arc::new(OutputFeed::open(&self.holder_socket, &self.session_id).await?);
        *current = Arc::downgrade(&feed);
        Ok(feed)
    }
}
