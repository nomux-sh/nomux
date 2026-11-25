//! Arbitrary bytes go into the decoders of the control socket of a holder.
//!
//! The daemon and the holder read these bytes from a process that they do not
//! trust. The frame buffer and each `decode` must not panic and must not
//! allocate more than a frame. A message that decodes must also give the same
//! message after `encode` and a second `decode`.
//!
//! The first byte of the input selects how many bytes the socket gives in one
//! read. The rest is the byte stream.

#![no_main]

use libfuzzer_sys::fuzz_target;
use nomux_holder_protocol::wire::{Frame, FrameBuffer, Hello, Reply, Request};

fuzz_target!(|input: &[u8]| {
    let Some((read_selector, stream)) = input.split_first() else {
        return;
    };
    let read_length = 1 + usize::from(*read_selector);

    let mut received = FrameBuffer::new();
    let mut encoded_again = FrameBuffer::new();
    let mut rest = stream;
    while !rest.is_empty() {
        let unfilled = received.unfilled();
        assert!(!unfilled.is_empty(), "no space for the rest of a frame");
        let count = unfilled.len().min(read_length).min(rest.len());
        unfilled[..count].copy_from_slice(&rest[..count]);
        received.advance(count);
        rest = &rest[count..];

        loop {
            match received.next_frame() {
                Ok(Some(frame)) => assert_each_decoder_is_stable(frame, &mut encoded_again),
                Ok(None) => break,
                // The receiver closes the connection.
                Err(_) => return,
            }
        }
    }
});

/// Gives `frame` to each decoder. `encoded_again` is an empty buffer for the
/// second decode.
fn assert_each_decoder_is_stable(frame: Frame<'_>, encoded_again: &mut FrameBuffer) {
    let mut bytes = Vec::new();

    frame.encode(&mut bytes);
    assert_eq!(only_frame(encoded_again, &bytes), frame);

    if let Ok(hello) = Hello::decode(frame) {
        bytes.clear();
        hello.encode(&mut bytes);
        assert_eq!(Hello::decode(only_frame(encoded_again, &bytes)), Ok(hello));
    }
    if let Ok(request) = Request::decode(frame) {
        bytes.clear();
        request.encode(frame.request_id, &mut bytes);
        let again = only_frame(encoded_again, &bytes);
        assert_eq!(again.request_id, frame.request_id);
        assert_eq!(Request::decode(again), Ok(request));
    }
    if let Ok(reply) = Reply::decode(frame) {
        bytes.clear();
        reply.encode(frame.request_id, &mut bytes);
        let again = only_frame(encoded_again, &bytes);
        assert_eq!(again.request_id, frame.request_id);
        assert_eq!(Reply::decode(again), Ok(reply));
    }
}

/// The frame that `encoded` is. `buffer` is empty before the call, and it is
/// empty again when the caller drops the frame.
fn only_frame<'buffer>(buffer: &'buffer mut FrameBuffer, encoded: &[u8]) -> Frame<'buffer> {
    buffer.unfilled()[..encoded.len()].copy_from_slice(encoded);
    buffer.advance(encoded.len());
    buffer
        .next_frame()
        .expect("an encoded frame has a valid length")
        .expect("an encoded frame is complete")
}
