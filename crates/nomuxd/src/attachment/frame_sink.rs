//! Writes the frames of the terminal data channel of one attach.

use std::io;

use nomux_proto::frame::{EndReason, FrameKind, MAX_PAYLOAD_LEN};
use nomux_transport::FrameWriter;

/// The largest payload of one output frame. A small frame lets the task of
/// a client give the thread to the other clients often.
pub(super) const OUTPUT_FRAME_BYTES: usize = 64 * 1024;

pub(super) struct FrameSink {
    frames: FrameWriter,
    /// The session handle of the frames.
    handle: u32,
    /// The epoch of the session now.
    epoch: u32,
    /// The sequence number after the last output byte of the current epoch
    /// that the client got.
    sent: u64,
}

impl FrameSink {
    pub(super) fn new(frames: FrameWriter, handle: u32, epoch: u32) -> Self {
        Self {
            frames,
            handle,
            epoch,
            sent: 0,
        }
    }

    /// Sends output bytes of `epoch`. `seq` is the sequence number of the
    /// first byte.
    pub(super) async fn output(&mut self, epoch: u32, seq: u64, bytes: &[u8]) -> io::Result<()> {
        let mut seq = seq;
        for part in bytes.chunks(OUTPUT_FRAME_BYTES) {
            self.send(FrameKind::Output, epoch, seq, part).await?;
            seq += part.len() as u64;
            self.record_position(epoch, seq);
        }
        Ok(())
    }

    /// Sends one snapshot: the redraw sequence of the screen that
    /// corresponds to the position `seq` of `epoch`.
    pub(super) async fn snapshot(
        &mut self,
        epoch: u32,
        seq: u64,
        redraw_sequence: &[u8],
    ) -> io::Result<()> {
        // An empty sequence is one frame: the client must learn the
        // position of the screen.
        if redraw_sequence.is_empty() {
            self.send(FrameKind::ScreenSnapshot, epoch, seq, &[])
                .await?;
        }
        for part in redraw_sequence.chunks(MAX_PAYLOAD_LEN as usize) {
            self.send(FrameKind::ScreenSnapshot, epoch, seq, part)
                .await?;
        }
        self.record_position(epoch, seq);
        Ok(())
    }

    /// Tells the client that the output of `epoch` from `seq` to the next
    /// frame is gone.
    pub(super) async fn history_gap(&mut self, epoch: u32, seq: u64) -> io::Result<()> {
        self.send(FrameKind::HistoryGap, epoch, seq, &[]).await
    }

    /// Tells the client that the command of the session started again: the
    /// frames before this one are of an earlier epoch.
    pub(super) async fn restored(&mut self) -> io::Result<()> {
        self.send(FrameKind::Restored, self.epoch, 0, &[]).await
    }

    /// Sends the last frame of the attach, if the stream is at a frame
    /// boundary.
    pub(super) async fn end(&mut self, reason: EndReason) -> io::Result<()> {
        if !self.frames.can_send() {
            return Ok(());
        }
        self.send(FrameKind::End, self.epoch, self.sent, &reason.encode())
            .await
    }

    fn record_position(&mut self, epoch: u32, seq: u64) {
        if epoch == self.epoch {
            self.sent = seq;
        }
    }

    async fn send(
        &mut self,
        kind: FrameKind,
        epoch: u32,
        seq: u64,
        payload: &[u8],
    ) -> io::Result<()> {
        self.frames
            .send(kind, self.handle, epoch, seq, payload)
            .await
    }
}
