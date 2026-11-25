//! The framing of the control channel.

use std::io;

use nomux_proto::wire::v1::{Envelope, MAX_ENVELOPE_LENGTH};
use prost::Message;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::frame_stream::{FrameReader, FrameWriter};

/// A protobuf varint of 64 bits has 10 bytes at most.
const MAX_LENGTH_PREFIX: usize = 10;

/// One connection of the control channel: a byte stream that carries
/// envelopes. Each envelope has a varint length prefix.
#[derive(Debug)]
pub struct EnvelopeStream {
    stream: UnixStream,
    /// The bytes that came from the stream and are not an envelope yet.
    incoming: Vec<u8>,
}

impl EnvelopeStream {
    pub(crate) fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            incoming: Vec::new(),
        }
    }

    /// Sends one envelope.
    ///
    /// # Errors
    ///
    /// Returns an error if the envelope is longer than the maximum of the
    /// protocol, or if the write fails.
    pub async fn send(&mut self, envelope: &Envelope) -> io::Result<()> {
        if envelope.encoded_len() > MAX_ENVELOPE_LENGTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the envelope is longer than the protocol permits",
            ));
        }
        self.stream
            .write_all(&envelope.encode_length_delimited_to_vec())
            .await
    }

    /// Receives the next envelope. Returns `None` if the peer closed the
    /// connection after a complete envelope.
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails, if the peer closed the connection
    /// in the middle of an envelope, if a length is larger than the maximum
    /// of the protocol, or if the bytes are not an envelope. The stream
    /// cannot be read further then.
    pub async fn receive(&mut self) -> io::Result<Option<Envelope>> {
        loop {
            if let Some(envelope) = self.take_envelope()? {
                return Ok(Some(envelope));
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

    /// Changes the connection from the control channel to the terminal
    /// channels of an attach: the frames from the peer and the frames to the
    /// peer. The bytes that already came after the last envelope are the
    /// start of the first frame.
    #[must_use]
    pub fn into_frames(self) -> (FrameReader, FrameWriter) {
        let (read, write) = self.stream.into_split();
        (
            FrameReader::new(read, self.incoming),
            FrameWriter::new(write),
        )
    }

    /// Takes one envelope out of the received bytes, if they contain a
    /// complete one.
    fn take_envelope(&mut self) -> io::Result<Option<Envelope>> {
        let mut remaining = self.incoming.as_slice();
        let Ok(length) = prost::decode_length_delimiter(&mut remaining) else {
            let prefix_is_incomplete = self.incoming.len() < MAX_LENGTH_PREFIX;
            return if prefix_is_incomplete {
                Ok(None)
            } else {
                Err(invalid("the length prefix is not valid"))
            };
        };
        if length > MAX_ENVELOPE_LENGTH {
            return Err(invalid("the envelope is longer than the protocol permits"));
        }
        let Some(encoded) = remaining.get(..length) else {
            return Ok(None);
        };
        let envelope = Envelope::decode(encoded)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let consumed = self.incoming.len() - remaining.len() + length;
        self.incoming.drain(..consumed);
        Ok(Some(envelope))
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use nomux_proto::wire::v1::{Event, envelope};

    use super::*;

    fn event(seq: u64) -> Envelope {
        Envelope {
            kind: Some(envelope::Kind::Event(Event { seq })),
        }
    }

    fn pair() -> (EnvelopeStream, UnixStream) {
        let (near, far) = UnixStream::pair().unwrap();
        (EnvelopeStream::new(near), far)
    }

    #[tokio::test]
    async fn envelopes_that_arrive_in_pieces_and_together_are_separated() {
        let (mut stream, mut peer) = pair();
        let mut bytes = event(1).encode_length_delimited_to_vec();
        bytes.extend(event(2).encode_length_delimited_to_vec());
        let (first_piece, rest) = bytes.split_at(3);

        peer.write_all(first_piece).await.unwrap();
        let receive = tokio::spawn(async move {
            let first = stream.receive().await.unwrap();
            let second = stream.receive().await.unwrap();
            let end = stream.receive().await.unwrap();
            (first, second, end)
        });
        tokio::task::yield_now().await;
        peer.write_all(rest).await.unwrap();
        drop(peer);

        let (first, second, end) = receive.await.unwrap();
        assert_eq!(first, Some(event(1)));
        assert_eq!(second, Some(event(2)));
        assert_eq!(end, None);
    }

    #[tokio::test]
    async fn a_length_over_the_maximum_is_refused_before_the_bytes_arrive() {
        let (mut stream, mut peer) = pair();
        let mut prefix = Vec::new();
        prost::encode_length_delimiter(MAX_ENVELOPE_LENGTH + 1, &mut prefix).unwrap();
        peer.write_all(&prefix).await.unwrap();

        let error = stream.receive().await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn a_connection_that_closes_inside_an_envelope_is_an_error() {
        let (mut stream, mut peer) = pair();
        let bytes = event(300).encode_length_delimited_to_vec();
        peer.write_all(&bytes[..bytes.len() - 1]).await.unwrap();
        drop(peer);

        let error = stream.receive().await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
}
