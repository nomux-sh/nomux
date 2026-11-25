//! Moves the screen and the output of one session to one attached client.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use nomux_holder_protocol::ring::RingReader;
use nomux_proto::frame::{EndReason, FIRST_EPOCH, FrameKind, MAX_PAYLOAD_LEN};
use nomux_transport::FrameWriter;
use tokio::sync::watch;
use tokio::task::yield_now;
use tokio::time::timeout;

use crate::holder::{OutputFeed, Redraw, SessionEnd};

/// The largest payload of one output frame, and the only output that the
/// daemon holds for a client.
const OUTPUT_FRAME_BYTES: usize = 64 * 1024;

/// After the feed lost the holder, the task of the session reports how the
/// session ended. If it does not do that in this time, the session
/// continues and only the feed failed.
const END_REPORT_TIMEOUT: Duration = Duration::from_secs(2);

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
/// screen and the output from there.
pub(super) struct OutputPump {
    frames: FrameWriter,
    /// The session handle of the frames.
    handle: u32,
    feed: Arc<OutputFeed>,
    reader: RingReader,
    write_position: watch::Receiver<u64>,
    ended: watch::Receiver<Option<SessionEnd>>,
    /// The bytes that the pump read from the ring buffer and did not send.
    buffer: Box<[u8]>,
    /// The sequence number after the last output byte that the client got.
    sent: u64,
}

impl OutputPump {
    pub(super) fn new(
        frames: FrameWriter,
        handle: u32,
        feed: Arc<OutputFeed>,
        reader: RingReader,
        ended: watch::Receiver<Option<SessionEnd>>,
    ) -> Self {
        Self {
            frames,
            handle,
            write_position: feed.write_position(),
            feed,
            reader,
            ended,
            buffer: vec![0; OUTPUT_FRAME_BYTES].into_boxed_slice(),
            sent: 0,
        }
    }

    /// Sends the screen, or resumes at `resume`, and then sends the output
    /// until the session ends. After the last output, sends how the session
    /// ended.
    ///
    /// # Errors
    ///
    /// Returns an error if the client does not take a frame, or if the
    /// daemon lost the holder and the session did not end.
    pub(super) async fn run(&mut self, resume: Option<u64>) -> io::Result<()> {
        match self.stream(resume).await {
            Stop::SessionEnded(end) => {
                self.send_remaining_output().await?;
                self.end(end.into()).await
            }
            Stop::Failed(error) => Err(error),
        }
    }

    /// Sends the last frame of the attach, if the stream is at a frame
    /// boundary.
    pub(super) async fn end(&mut self, reason: EndReason) -> io::Result<()> {
        if !self.frames.can_send() {
            return Ok(());
        }
        self.send(FrameKind::End, self.sent, &reason.encode()).await
    }

    async fn stream(&mut self, resume: Option<u64>) -> Stop {
        let mut pending = match self.start(resume).await {
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

    /// The first bytes for the client are in the buffer after this: the
    /// result is their number.
    async fn start(&mut self, resume: Option<u64>) -> Result<usize, Stop> {
        if let Some(from) = resume.filter(|from| *from <= self.reader.write_position()) {
            self.reader.seek(from);
            if let Ok(count) = self.reader.read(&mut self.buffer) {
                return Ok(count);
            }
        }
        self.snapshot().await
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
                self.send_snapshot(&redraw).await?;
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
        let Self {
            frames,
            handle,
            buffer,
            ..
        } = self;
        frames
            .send(
                FrameKind::Output,
                *handle,
                FIRST_EPOCH,
                seq,
                &buffer[..count],
            )
            .await?;
        self.sent = self.reader.position();
        Ok(())
    }

    async fn send_snapshot(&mut self, redraw: &Redraw) -> io::Result<()> {
        for part in redraw.sequence.chunks(MAX_PAYLOAD_LEN as usize) {
            self.send(FrameKind::ScreenSnapshot, redraw.position, part)
                .await?;
        }
        self.sent = redraw.position;
        Ok(())
    }

    async fn send(&mut self, kind: FrameKind, seq: u64, payload: &[u8]) -> io::Result<()> {
        self.frames
            .send(kind, self.handle, FIRST_EPOCH, seq, payload)
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
