//! Moves the screen and the output of one session to one attached client.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use nomux_holder_protocol::ring::RingReader;
use nomux_proto::frame::EndReason;
use tokio::sync::watch;
use tokio::task::yield_now;
use tokio::time::timeout;

use super::frame_sink::{FrameSink, OUTPUT_FRAME_BYTES};
use super::history_replay::{self, EarlierOutput};
use crate::holder::{OutputFeed, SessionEnd};
use crate::session_journal::SessionJournal;

/// After the feed lost the holder, the task of the session reports how the
/// session ended. If it does not do that in this time, the session
/// continues and only the feed failed.
const END_REPORT_TIMEOUT: Duration = Duration::from_secs(2);

/// Where the frames of an attach start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Start {
    /// The current screen.
    Screen,
    /// The output of the current epoch from this position: the client has
    /// the output before it.
    Resume(u64),
    /// Output of an earlier epoch, then the mark that the command started
    /// again, then the current screen.
    Earlier { epoch: u32, output: EarlierOutput },
}

/// Why the output of an attach stops.
pub(super) enum Stop {
    SessionEnded(SessionEnd),
    Failed(io::Error),
}

impl From<io::Error> for Stop {
    fn from(error: io::Error) -> Self {
        Self::Failed(error)
    }
}

/// The terminal data channel of one attach.
///
/// The pump reads the ring buffer of the holder only when the client can
/// take more bytes. The ring buffer is the only queue: if the holder wrote
/// over the bytes that the client needs next, the client gets the current
/// screen and the output from there. Only the start of an attach reads the
/// journal: for a client that asks for output that the ring buffer has no
/// more.
pub(super) struct OutputPump {
    sink: FrameSink,
    feed: Arc<OutputFeed>,
    journal: SessionJournal,
    reader: RingReader,
    /// `true` if the client reads the frames that say that output is gone.
    tell_gaps: bool,
    write_position: watch::Receiver<u64>,
    ended: watch::Receiver<Option<SessionEnd>>,
    /// The bytes that the pump read from the ring buffer and did not send:
    /// the only output that the daemon holds for a client.
    buffer: Box<[u8]>,
}

impl OutputPump {
    pub(super) fn new(
        sink: FrameSink,
        feed: Arc<OutputFeed>,
        journal: SessionJournal,
        reader: RingReader,
        tell_gaps: bool,
        ended: watch::Receiver<Option<SessionEnd>>,
    ) -> Self {
        Self {
            sink,
            write_position: feed.write_position(),
            feed,
            journal,
            reader,
            tell_gaps,
            ended,
            buffer: vec![0; OUTPUT_FRAME_BYTES].into_boxed_slice(),
        }
    }

    /// Sends the frames of `start`, and then sends the output until the
    /// session ends. After the last output, sends how the session ended.
    ///
    /// # Errors
    ///
    /// Returns an error if the client does not take a frame, or if the
    /// daemon lost the holder and the session did not end.
    pub(super) async fn run(&mut self, start: Start) -> io::Result<()> {
        match self.stream(start).await {
            Stop::SessionEnded(end) => {
                self.send_remaining_output().await?;
                self.end(end.into()).await
            }
            Stop::Failed(error) => Err(error),
        }
    }

    /// Sends the last frame of the attach.
    pub(super) async fn end(&mut self, reason: EndReason) -> io::Result<()> {
        self.sink.end(reason).await
    }

    async fn stream(&mut self, start: Start) -> Stop {
        let mut pending = match self.start(start).await {
            Ok(pending) => pending,
            Err(stop) => return stop,
        };
        loop {
            match self.step(pending).await {
                Ok(read) => pending = read,
                Err(stop) => return stop,
            }
        }
    }

    /// The first bytes of the ring buffer for the client are in the buffer
    /// after this: the result is their number.
    async fn start(&mut self, start: Start) -> Result<usize, Stop> {
        match start {
            Start::Screen => {}
            // A position that the holder did not write yet is not a
            // position of this epoch.
            Start::Resume(from) if from > self.reader.write_position() => {}
            Start::Resume(from) => {
                if let Some(count) = self.resume(from).await? {
                    return Ok(count);
                }
            }
            Start::Earlier { epoch, output } => {
                history_replay::earlier_epoch(&mut self.sink, &self.journal, epoch, output).await?;
            }
        }
        self.snapshot().await
    }

    /// Continues the output at the position `from`: from the ring buffer,
    /// or from the journal until the ring buffer has the next byte. Returns
    /// `None` if the client must get the current screen.
    async fn resume(&mut self, from: u64) -> io::Result<Option<usize>> {
        self.reader.seek(from);
        if let Ok(count) = self.reader.read(&mut self.buffer) {
            return Ok(Some(count));
        }
        history_replay::catch_up(
            &mut self.sink,
            &self.journal,
            &mut self.reader,
            &mut self.buffer,
            self.feed.epoch(),
            from,
            self.tell_gaps,
        )
        .await
    }

    /// Sends the `pending` bytes of the buffer and reads the next bytes.
    async fn step(&mut self, pending: usize) -> Result<usize, Stop> {
        if pending > 0 {
            self.send_output(pending).await?;
            // One task serves all the clients. A client that takes a flood
            // of output must not keep the input of the other clients waiting.
            yield_now().await;
        }
        match self.reader.read(&mut self.buffer) {
            Ok(0) => {
                self.wait_for_output().await?;
                Ok(0)
            }
            Ok(count) => Ok(count),
            Err(_overrun) => self.snapshot().await,
        }
    }

    /// Sends the current screen. The output from the position of the screen
    /// is in the buffer after this: the result is its length.
    async fn snapshot(&mut self) -> Result<usize, Stop> {
        loop {
            let redraw = match self.feed.redraw().await {
                Ok(redraw) => redraw,
                Err(error) => return Err(self.feed_lost(error).await),
            };
            self.reader.seek(redraw.position);
            // The bytes after the screen are read before the screen is
            // sent: a client that takes the screen slowly loses nothing.
            if let Ok(count) = self.reader.read(&mut self.buffer) {
                self.sink
                    .snapshot(self.feed.epoch(), redraw.position, &redraw.sequence)
                    .await?;
                return Ok(count);
            }
            // The holder wrote over the position of this screen already.
            self.wait_for_output().await?;
        }
    }

    async fn wait_for_output(&mut self) -> Result<(), Stop> {
        let seen = self.reader.position();
        let Self {
            write_position,
            ended,
            ..
        } = self;
        let feed_runs = tokio::select! {
            biased;
            position = write_position.wait_for(|position| *position > seen) => position.is_ok(),
            end = session_end(ended) => return Err(Stop::SessionEnded(end)),
        };
        if feed_runs {
            Ok(())
        } else {
            Err(self.feed_lost(io::ErrorKind::BrokenPipe.into()).await)
        }
    }

    /// The feed ends when the holder ends. The task of the session learns
    /// the exit status at the same time, thus the pump waits for it.
    async fn feed_lost(&mut self, error: io::Error) -> Stop {
        match timeout(END_REPORT_TIMEOUT, session_end(&mut self.ended)).await {
            Ok(end) => Stop::SessionEnded(end),
            Err(_) => Stop::Failed(error),
        }
    }

    /// After the end of the command, the ring buffer has its last output.
    async fn send_remaining_output(&mut self) -> io::Result<()> {
        while let Ok(count @ 1..) = self.reader.read(&mut self.buffer) {
            self.send_output(count).await?;
        }
        Ok(())
    }

    /// Sends the first `count` bytes of the buffer, which the reader read
    /// last.
    async fn send_output(&mut self, count: usize) -> io::Result<()> {
        let seq = self.reader.position() - count as u64;
        self.sink
            .output(self.feed.epoch(), seq, &self.buffer[..count])
            .await
    }
}

/// Completes when the session ended. It does not complete if the task of
/// the session stopped without an end, which occurs when the daemon stops.
async fn session_end(ended: &mut watch::Receiver<Option<SessionEnd>>) -> SessionEnd {
    match ended.wait_for(Option::is_some).await.map(|end| *end) {
        Ok(Some(end)) => end,
        _ => std::future::pending().await,
    }
}

impl From<SessionEnd> for EndReason {
    fn from(end: SessionEnd) -> Self {
        match end {
            SessionEnd::Exited { code } => Self::Exited { code },
            SessionEnd::Signaled { signal } => Self::Signaled { signal },
            SessionEnd::Lost => Self::Lost,
        }
    }
}
