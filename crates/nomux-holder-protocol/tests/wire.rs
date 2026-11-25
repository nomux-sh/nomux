//! The encoding of the control messages and the compatibility rules.

use nomux_holder_protocol::wire::{
    ChildSignal, ChildStatus, DecodeError, ErrorCode, Frame, FrameBuffer, Hello, HelloError,
    ProtocolVersion, Reply, Request, WindowSize,
};

const SIZE: WindowSize = WindowSize {
    rows: 40,
    columns: 100,
};

fn feed(buffer: &mut FrameBuffer, bytes: &[u8]) {
    buffer.unfilled()[..bytes.len()].copy_from_slice(bytes);
    buffer.advance(bytes.len());
}

const VERSION_1_0: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

fn hello() -> Hello {
    Hello {
        protocol: VERSION_1_0,
        session_id: "s1".parse().expect("the test helper works"),
        child_pid: 0x0102_0304,
        size: SIZE,
    }
}

/// Encodes a frame of `kind` with the payload of `encoded` and more bytes
/// after it, as a later protocol version can do.
fn with_added_fields(encoded: &[u8]) -> Vec<u8> {
    let mut buffer = FrameBuffer::new();
    feed(&mut buffer, encoded);
    let frame = buffer
        .next_frame()
        .expect("the test helper works")
        .expect("the test helper works");
    let mut payload = frame.payload.to_vec();
    payload.extend_from_slice(&[0xAA; 6]);
    let mut output = Vec::new();
    Frame {
        payload: &payload,
        ..frame
    }
    .encode(&mut output);
    output
}

#[test]
fn the_bytes_of_a_request_frame_do_not_change() {
    let mut encoded = Vec::new();
    Request::Resize(SIZE).encode(7, &mut encoded);
    assert_eq!(
        encoded,
        [
            12, 0, 0, 0, // length after this field
            0x03, 0x01, // kind: Resize
            0, 0, // flags
            7, 0, 0, 0, // request identifier
            40, 0, 100, 0, // rows, columns
        ]
    );
}

#[test]
fn the_bytes_of_the_hello_frame_do_not_change() {
    let mut encoded = Vec::new();
    hello().encode(&mut encoded);
    assert_eq!(
        encoded,
        [
            23, 0, 0, 0, // length after this field
            0x01, 0x00, // kind: Hello
            0, 0, // flags
            0, 0, 0, 0, // request identifier
            1, 0, 0, 0, // protocol version 1.0
            4, 3, 2, 1, // process ID of the child
            40, 0, 100, 0, // rows, columns
            2, b's', b'1', // session identifier
        ]
    );
}

#[test]
fn the_bytes_of_the_redraw_frames_do_not_change() {
    let mut request = Vec::new();
    Request::Redraw.encode(7, &mut request);
    assert_eq!(
        request,
        [
            8, 0, 0, 0, // length after this field
            0x07, 0x01, // kind: Redraw request
            0, 0, // flags
            7, 0, 0, 0, // request identifier
        ]
    );

    let mut reply = Vec::new();
    Reply::Redraw {
        position: 0x0102_0304_0506_0708,
        length: 0x0A0B_0C0D,
    }
    .encode(7, &mut reply);
    assert_eq!(
        reply,
        [
            20, 0, 0, 0, // length after this field
            0x06, 0x00, // kind: Redraw reply
            0, 0, // flags
            7, 0, 0, 0, // request identifier
            8, 7, 6, 5, 4, 3, 2, 1, // position of the ring buffer
            0x0D, 0x0C, 0x0B, 0x0A, // length of the redraw sequence
        ]
    );
}

#[test]
fn a_holder_supports_only_the_requests_of_its_minor_version() {
    let version_1_1 = ProtocolVersion { major: 1, minor: 1 };
    let later_minor = ProtocolVersion { major: 1, minor: 9 };
    let later_major = ProtocolVersion { major: 2, minor: 1 };

    assert!(VERSION_1_0.supports(&Request::Status));
    assert!(!VERSION_1_0.supports(&Request::Redraw));
    assert!(version_1_1.supports(&Request::Redraw));
    assert!(later_minor.supports(&Request::Redraw));
    assert!(!later_major.supports(&Request::Status));
    assert!(ProtocolVersion::CURRENT.supports(&Request::Redraw));
}

#[test]
fn each_message_decodes_to_the_value_that_was_encoded() {
    let requests = [
        Request::AcquireLease,
        Request::Input(b"ls\n"),
        Request::Resize(SIZE),
        Request::Signal(ChildSignal::Kill),
        Request::Status,
        Request::WaitOutput {
            after: u64::MAX - 1,
        },
        Request::Redraw,
    ];
    let replies = [
        Reply::Done,
        Reply::Error(ErrorCode::LeaseRequired),
        Reply::Error(ErrorCode::Unrecognized(900)),
        Reply::Status(ChildStatus::Running),
        Reply::Status(ChildStatus::Exited { code: -3 }),
        Reply::Status(ChildStatus::Signaled { signal: 9 }),
        Reply::Output {
            write_position: 1 << 40,
        },
        Reply::Redraw {
            position: 1 << 40,
            length: 70_000,
        },
        Reply::Error(ErrorCode::RedrawTooLarge),
    ];
    let mut buffer = FrameBuffer::new();
    let mut encoded = Vec::new();

    for (request_id, request) in (1..).zip(requests) {
        encoded.clear();
        request.encode(request_id, &mut encoded);
        feed(&mut buffer, &encoded);
        let frame = buffer.next_frame().unwrap().unwrap();
        assert_eq!(frame.request_id, request_id);
        assert_eq!(Request::decode(frame), Ok(request));
    }
    for (request_id, reply) in (1..).zip(replies) {
        encoded.clear();
        reply.encode(request_id, &mut encoded);
        feed(&mut buffer, &encoded);
        let frame = buffer.next_frame().unwrap().unwrap();
        assert_eq!(frame.request_id, request_id);
        assert_eq!(Reply::decode(frame), Ok(reply));
    }
}

#[test]
fn frames_that_arrive_in_parts_are_put_together() {
    let mut encoded = Vec::new();
    Request::Input(b"first").encode(1, &mut encoded);
    Request::WaitOutput { after: 9 }.encode(2, &mut encoded);
    let mut buffer = FrameBuffer::new();
    let mut decoded = Vec::new();

    for byte in &encoded {
        feed(&mut buffer, &[*byte]);
        while let Some(frame) = buffer.next_frame().unwrap() {
            decoded.push(format!("{:?}", Request::decode(frame).unwrap()));
        }
    }

    assert_eq!(
        decoded,
        [
            format!("{:?}", Request::Input(b"first")),
            format!("{:?}", Request::WaitOutput { after: 9 }),
        ]
    );
}

#[test]
fn an_unknown_request_kind_is_an_error_and_the_next_frame_is_read() {
    let mut encoded = Vec::new();
    Frame {
        kind: 0x01F0,
        request_id: 1,
        payload: b"from a later version",
    }
    .encode(&mut encoded);
    Request::Status.encode(2, &mut encoded);
    let mut buffer = FrameBuffer::new();
    feed(&mut buffer, &encoded);

    let unknown = buffer.next_frame().unwrap().unwrap();
    assert_eq!(
        Request::decode(unknown),
        Err(DecodeError::UnknownKind(0x01F0))
    );
    let known = buffer.next_frame().unwrap().unwrap();
    assert_eq!(Request::decode(known), Ok(Request::Status));
}

#[test]
fn fields_that_a_later_version_adds_to_a_payload_are_ignored() {
    let mut buffer = FrameBuffer::new();

    let mut resize = Vec::new();
    Request::Resize(SIZE).encode(1, &mut resize);
    feed(&mut buffer, &with_added_fields(&resize));
    let frame = buffer.next_frame().unwrap().unwrap();
    assert_eq!(Request::decode(frame), Ok(Request::Resize(SIZE)));

    let mut status = Vec::new();
    Reply::Status(ChildStatus::Exited { code: 2 }).encode(1, &mut status);
    feed(&mut buffer, &with_added_fields(&status));
    let frame = buffer.next_frame().unwrap().unwrap();
    assert_eq!(
        Reply::decode(frame),
        Ok(Reply::Status(ChildStatus::Exited { code: 2 }))
    );

    let mut first_frame = Vec::new();
    Hello {
        protocol: ProtocolVersion { major: 1, minor: 9 },
        ..hello()
    }
    .encode(&mut first_frame);
    feed(&mut buffer, &with_added_fields(&first_frame));
    let frame = buffer.next_frame().unwrap().unwrap();
    assert_eq!(Hello::decode(frame).unwrap().session_id, hello().session_id);
}

#[test]
fn a_hello_of_a_different_major_version_gives_the_version_and_reads_no_other_field() {
    let frame = Frame {
        kind: 0x0001,
        request_id: 0,
        payload: &[2, 0, 5, 0],
    };
    assert_eq!(
        Hello::decode(frame),
        Err(HelloError::UnsupportedVersion(ProtocolVersion {
            major: 2,
            minor: 5
        }))
    );
}

#[test]
fn a_length_that_is_not_possible_is_an_error() {
    let mut too_short = FrameBuffer::new();
    feed(&mut too_short, &7u32.to_le_bytes());
    assert_eq!(too_short.next_frame().unwrap_err().length, 7);

    let mut too_long = FrameBuffer::new();
    feed(&mut too_long, &(8 + 65_537u32).to_le_bytes());
    assert!(too_long.next_frame().is_err());
}

#[test]
fn a_payload_with_a_value_that_is_not_permitted_is_invalid() {
    let zero_rows = Frame {
        kind: 0x0103,
        request_id: 1,
        payload: &[0, 0, 80, 0],
    };
    assert_eq!(Request::decode(zero_rows), Err(DecodeError::InvalidPayload));

    let unknown_signal = Frame {
        kind: 0x0104,
        request_id: 1,
        payload: &[99],
    };
    assert_eq!(
        Request::decode(unknown_signal),
        Err(DecodeError::InvalidPayload)
    );
}
