//! The framing of the terminal channels.
//!
//! A terminal frame is a fixed header and then a payload, as the `proto`
//! repository specifies. [`FrameReader`] and [`FrameWriter`] are the two
//! directions of one connection after it changed from the control channel to
//! the terminal channels of an attach.

use std::io;

use nomux_proto::frame::{FrameError, FrameHeader, FrameKind, HEADER_LEN, MAX_PAYLOAD_LEN};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::byte_stream::{ReadHalf, WriteHalf};

/// The receiving direction of a connection that carries terminal frames.
#[derive(Debug)]
pub struct FrameReader {
    stream: ReadHalf,
    /// The bytes that came from the stream. The frame that
    /// [`receive`](Self::receive) returned last is at the start.
    incoming: Vec<u8>,
    /// The length of the frame at the start of `incoming`: header and
    /// payload. 0 if there is none.
    current: usize,
}

impl FrameReader {
    pub(crate) fn new(stream: ReadHalf, incoming: Vec<u8>) -> Self {
        Self {
            stream,
            incoming,
            current: 0,
        }
    }

    /// Receives the next frame and returns its header.
    /// [`payload`](Self::payload) gives its payload until the next call.
    /// Returns `None` if the peer closed the connection after a complete
    /// frame. A frame of a kind that this version does not know is skipped.
    ///
    /// If the future is dropped before it completes, no byte is lost.
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails, if the peer closed the connection
    /// in the middle of a frame, or if a header is not valid. The stream
    /// cannot be read further then.
    pub async fn receive(&mut self) -> io::Result<Option<FrameHeader>> {
        self.incoming.drain(..self.current);
        self.current = 0;
        loop {
            if let Some(header) = self.take_header()? {
                return Ok(Some(header));
            }
            if self.stream.read_buf(&mut self.incoming).await? == 0 {
                return if self.incoming.is_empty() {
                    Ok(None)
                } else {
                    Err(io::ErrorKind::UnexpectedEof.into())
                };
            }
        }
    }

    /// The payload of the frame that [`receive`](Self::receive) returned
    /// last.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        self.incoming.get(HEADER_LEN..self.current).unwrap_or(&[])
    }

    /// Makes the first complete frame of the received bytes the current
    /// frame. Removes the frames of a kind that this version does not know.
    fn take_header(&mut self) -> io::Result<Option<FrameHeader>> {
        loop {
            let (header, payload_length) = match FrameHeader::decode_prefix(&self.incoming) {
                Ok((header, _)) => (Some(header), header.length),
                Err(FrameError::Truncated { .. }) => return Ok(None),
                Err(FrameError::UnknownKind { length, .. }) => (None, length),
                Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
            };
            let frame_length = HEADER_LEN + payload_length as usize;
            if self.incoming.len() < frame_length {
                return Ok(None);
            }
            if let Some(header) = header {
                self.current = frame_length;
                return Ok(Some(header));
            }
            self.incoming.drain(..frame_length);
        }
    }
}

/// The sending direction of a connection that carries terminal frames.
#[derive(Debug)]
pub struct FrameWriter {
    stream: WriteHalf,
    /// The header and the payload of one frame, thus one write sends them.
    outgoing: Vec<u8>,
    /// `true` while a part of a frame is on the stream and the other part
    /// is not.
    frame_is_incomplete: bool,
}

impl FrameWriter {
    pub(crate) fn new(stream: WriteHalf) -> Self {
        Self {
            stream,
            outgoing: Vec::new(),
            frame_is_incomplete: false,
        }
    }

    /// Sends one frame.
    ///
    /// If the future is dropped before it completes, the peer can have a
    /// part of the frame. [`can_send`](Self::can_send) is `false` then.
    ///
    /// # Errors
    ///
    /// Returns an error if the payload is longer than the maximum of the
    /// protocol, or if the write fails.
    pub async fn send(
        &mut self,
        kind: FrameKind,
        session: u32,
        epoch: u32,
        seq: u64,
        payload: &[u8],
    ) -> io::Result<()> {
        let too_long = || io::Error::new(io::ErrorKind::InvalidInput, "the payload is too long");
        let header = FrameHeader {
            kind,
            session,
            epoch,
            seq,
            length: u32::try_from(payload.len())
                .ok()
                .filter(|length| *length <= MAX_PAYLOAD_LEN)
                .ok_or_else(too_long)?,
        };
        let header = header.encode().map_err(|_| too_long())?;
        self.outgoing.clear();
        self.outgoing.extend_from_slice(&header);
        self.outgoing.extend_from_slice(payload);
        self.frame_is_incomplete = true;
        self.stream.write_all(&self.outgoing).await?;
        self.frame_is_incomplete = false;
        Ok(())
    }

    /// `false` if a send did not complete: the next frame would not start at
    /// a frame boundary of the stream.
    #[must_use]
    pub fn can_send(&self) -> bool {
        !self.frame_is_incomplete
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::UnixStream;

    use super::*;
    use crate::EnvelopeStream;

    const SESSION: u32 = 1;

    fn frame(kind: u8, seq: u64, payload: &[u8]) -> Vec<u8> {
        let header = FrameHeader {
            kind: FrameKind::Output,
            session: SESSION,
            epoch: 1,
            seq,
            length: u32::try_from(payload.len()).unwrap(),
        };
        let mut bytes = header.encode().unwrap().to_vec();
        bytes[1] = kind;
        bytes.extend_from_slice(payload);
        bytes
    }

    fn reader() -> (FrameReader, UnixStream) {
        let (near, far) = UnixStream::pair().unwrap();
        let (read, write) = near.into_split();
        let (reader, _writer) = EnvelopeStream::new(read, write).into_frames();
        (reader, far)
    }

    #[tokio::test]
    async fn frames_that_arrive_in_pieces_and_together_are_separated() {
        let (mut reader, mut peer) = reader();
        let mut bytes = frame(FrameKind::Output.as_u8(), 0, b"one");
        bytes.extend(frame(FrameKind::Output.as_u8(), 3, b""));
        bytes.extend(frame(FrameKind::ScreenSnapshot.as_u8(), 3, b"three"));
        let (first_piece, rest) = bytes.split_at(HEADER_LEN + 1);

        peer.write_all(first_piece).await.unwrap();
        let receive = tokio::spawn(async move {
            let mut frames = Vec::new();
            while let Some(header) = reader.receive().await.unwrap() {
                frames.push((header.kind, header.seq, reader.payload().to_vec()));
            }
            frames
        });
        tokio::task::yield_now().await;
        peer.write_all(rest).await.unwrap();
        drop(peer);

        assert_eq!(
            receive.await.unwrap(),
            [
                (FrameKind::Output, 0, b"one".to_vec()),
                (FrameKind::Output, 3, Vec::new()),
                (FrameKind::ScreenSnapshot, 3, b"three".to_vec()),
            ]
        );
    }

    #[tokio::test]
    async fn a_frame_of_an_unknown_kind_is_skipped_with_its_payload() {
        let (mut reader, mut peer) = reader();
        let mut bytes = frame(200, 0, b"from a later version");
        bytes.extend(frame(FrameKind::Output.as_u8(), 7, b"known"));
        peer.write_all(&bytes).await.unwrap();

        let header = reader.receive().await.unwrap().unwrap();
        assert_eq!((header.seq, reader.payload()), (7, b"known".as_slice()));
    }

    #[tokio::test]
    async fn a_connection_that_closes_inside_a_frame_is_an_error() {
        let (mut reader, mut peer) = reader();
        let bytes = frame(FrameKind::Output.as_u8(), 0, b"payload");
        peer.write_all(&bytes[..bytes.len() - 1]).await.unwrap();
        drop(peer);

        let error = reader.receive().await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn the_bytes_that_came_with_the_last_envelope_are_the_first_frame() {
        use nomux_proto::wire::v1::{Envelope, Event, envelope};
        use prost::Message;

        let (near, mut peer) = UnixStream::pair().unwrap();
        let (read, write) = near.into_split();
        let mut envelopes = EnvelopeStream::new(read, write);
        let event = Envelope {
            kind: Some(envelope::Kind::Event(Event {
                seq: 1,
                ..Event::default()
            })),
        };
        let mut bytes = event.encode_length_delimited_to_vec();
        bytes.extend(frame(FrameKind::Input.as_u8(), 0, b"key"));
        peer.write_all(&bytes).await.unwrap();

        assert_eq!(envelopes.receive().await.unwrap(), Some(event));
        let (mut reader, _writer) = envelopes.into_frames();
        let header = reader.receive().await.unwrap().unwrap();
        assert_eq!(
            (header.kind, reader.payload()),
            (FrameKind::Input, b"key".as_slice())
        );
    }
}
