//! Reads the frames of the extension protocol from an asynchronous stream.
//!
//! `nomux_extension::frame` reads from a blocking stream. This reader keeps
//! the bytes of a frame that is not complete, thus a caller can stop a read
//! and start it again without the loss of a byte.

use std::io;

use nomux_extension::frame::{FrameError, MAX_ENVELOPE_LENGTH};
use nomux_extension::wire::v1::Envelope;
use prost::Message;
use tokio::io::{AsyncRead, AsyncReadExt};

/// The largest number of bytes of the length of a frame.
const MAX_LENGTH_BYTES: usize = 5;

/// The room that the reader asks for when it does not know the length of
/// the subsequent frame.
const READ_SIZE: usize = 8 * 1024;

pub(super) struct EnvelopeReader<Stream> {
    stream: Stream,
    /// The bytes that the reader has and that no envelope used yet.
    buffer: Vec<u8>,
}

impl<Stream: AsyncRead + Unpin> EnvelopeReader<Stream> {
    pub(super) fn new(stream: Stream) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
        }
    }

    /// Reads one envelope. The result is `None` when the stream ended
    /// between two frames.
    ///
    /// This function is cancel safe: the bytes of a frame that is not
    /// complete stay in the reader.
    pub(super) async fn next(&mut self) -> Result<Option<Envelope>, FrameError> {
        loop {
            let missing = match frame_position(&self.buffer)? {
                Some(frame) if frame.end <= self.buffer.len() => {
                    let envelope = Envelope::decode(&self.buffer[frame.start..frame.end])
                        .map_err(FrameError::Decode);
                    self.buffer.drain(..frame.end);
                    return envelope.map(Some);
                }
                Some(frame) => frame.end - self.buffer.len(),
                None => READ_SIZE,
            };
            // A read fills only the room that the buffer has, thus the
            // buffer does not grow past one frame.
            self.buffer.reserve(missing);
            if self.stream.read_buf(&mut self.buffer).await? == 0 {
                return self.end_of_stream();
            }
        }
    }

    fn end_of_stream(&self) -> Result<Option<Envelope>, FrameError> {
        if self.buffer.is_empty() {
            return Ok(None);
        }
        Err(FrameError::Io(io::ErrorKind::UnexpectedEof.into()))
    }
}

/// Where the body of the first frame of a buffer is.
struct FramePosition {
    start: usize,
    end: usize,
}

/// Reads the length of the first frame. The result is `None` if the buffer
/// does not have all the bytes of the length.
fn frame_position(buffer: &[u8]) -> Result<Option<FramePosition>, FrameError> {
    let mut length = 0;
    for (index, byte) in buffer.iter().take(MAX_LENGTH_BYTES).enumerate() {
        length |= u64::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 != 0 {
            continue;
        }
        let start = index + 1;
        return match usize::try_from(length) {
            Ok(length) if length <= MAX_ENVELOPE_LENGTH => Ok(Some(FramePosition {
                start,
                end: start + length,
            })),
            _ => Err(FrameError::TooLarge { length }),
        };
    }
    if buffer.len() >= MAX_LENGTH_BYTES {
        return Err(FrameError::InvalidLength);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use nomux_extension::frame;
    use nomux_extension::wire::v1::envelope::Kind;
    use nomux_extension::wire::v1::{Cancel, Event};
    use tokio::io::AsyncWriteExt;

    use super::*;

    fn event(payload_length: usize) -> Envelope {
        Envelope {
            kind: Some(Kind::Event(Event {
                name: "changed".to_owned(),
                payload: vec![7; payload_length],
            })),
        }
    }

    fn encode(envelope: &Envelope) -> Vec<u8> {
        let mut bytes = Vec::new();
        frame::write_envelope(&mut bytes, envelope).unwrap();
        bytes
    }

    async fn read_all(bytes: &[u8]) -> Result<Vec<Envelope>, FrameError> {
        let mut reader = EnvelopeReader::new(bytes);
        let mut envelopes = Vec::new();
        while let Some(envelope) = reader.next().await? {
            envelopes.push(envelope);
        }
        Ok(envelopes)
    }

    #[tokio::test]
    async fn frames_of_the_sdk_writer_are_read_in_sequence_to_the_end_of_the_stream() {
        let sent = [event(0), event(300), event(3 * READ_SIZE)];
        let bytes: Vec<u8> = sent.iter().flat_map(encode).collect();

        assert_eq!(read_all(&bytes).await.unwrap(), sent);
    }

    #[tokio::test]
    async fn a_read_that_is_cancelled_in_a_frame_loses_no_byte() {
        let sent = event(300);
        let bytes = encode(&sent);
        let (mut writer, stream) = tokio::io::duplex(1024);
        let mut reader = EnvelopeReader::new(stream);

        writer.write_all(&bytes[..100]).await.unwrap();
        let cancelled = tokio::time::timeout(std::time::Duration::from_millis(20), reader.next());
        assert!(cancelled.await.is_err());
        writer.write_all(&bytes[100..]).await.unwrap();

        assert_eq!(reader.next().await.unwrap(), Some(sent));
    }

    #[tokio::test]
    async fn the_end_of_the_stream_in_a_frame_is_an_error() {
        let bytes = encode(&event(300));

        for cut in [1, bytes.len() - 1] {
            let error = read_all(&bytes[..cut]).await.unwrap_err();
            assert!(
                matches!(&error, FrameError::Io(error) if error.kind() == io::ErrorKind::UnexpectedEof),
                "{error:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_length_over_the_limit_is_refused_before_the_body_arrives() {
        let mut length = Vec::new();
        prost::encoding::encode_varint(MAX_ENVELOPE_LENGTH as u64 + 1, &mut length);

        let error = read_all(&length).await.unwrap_err();

        assert!(
            matches!(error, FrameError::TooLarge { length } if length == MAX_ENVELOPE_LENGTH as u64 + 1),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_frame_of_the_largest_length_is_read() {
        let overhead = event(MAX_ENVELOPE_LENGTH).encoded_len() - MAX_ENVELOPE_LENGTH;
        let largest = event(MAX_ENVELOPE_LENGTH - overhead);
        assert_eq!(largest.encoded_len(), MAX_ENVELOPE_LENGTH);

        assert_eq!(read_all(&encode(&largest)).await.unwrap(), [largest]);
    }

    #[tokio::test]
    async fn a_length_of_more_than_five_bytes_and_a_body_that_is_not_an_envelope_are_refused() {
        let long_length = read_all(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00]).await;
        assert!(matches!(long_length, Err(FrameError::InvalidLength)));

        // A frame of one byte: a field tag without its value.
        let not_an_envelope = read_all(&[0x01, 0x0a]).await;
        assert!(matches!(not_an_envelope, Err(FrameError::Decode(_))));

        // The frame before a bad frame is not lost.
        let mut bytes = encode(&Envelope {
            kind: Some(Kind::Cancel(Cancel { id: 4 })),
        });
        bytes.extend([0x01, 0x0a]);
        let mut reader = EnvelopeReader::new(bytes.as_slice());
        assert!(reader.next().await.unwrap().is_some());
        assert!(matches!(reader.next().await, Err(FrameError::Decode(_))));
    }
}
