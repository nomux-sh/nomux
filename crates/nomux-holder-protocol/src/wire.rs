//! Control messages between a holder and the daemon.
//!
//! The messages go over the Unix socket of the holder. Each message is one
//! [`Frame`]. The holder sends a [`Hello`] when a connection opens. After
//! that, the daemon sends a [`Request`] and the holder answers each request
//! with one [`Reply`] that has the same request identifier.
//!
//! The encoding is written by hand and has fixed little-endian fields. The
//! rules that keep an old holder compatible with a new daemon are:
//!
//! - A receiver ignores the bytes of a payload that come after the fields it
//!   knows. A later version adds fields only at the end of a payload.
//! - A holder answers a request kind that it does not know with
//!   [`ErrorCode::UnknownKind`]. The connection stays open.
//! - The frame header and the first four bytes of the hello payload (the
//!   protocol version) never change.

mod descriptor;
mod frame;
mod hello;
mod payload;
mod reply;
mod request;
mod window_size;

pub use descriptor::{receive_with_descriptor, send_with_descriptor};
pub use frame::{Frame, FrameBuffer, FrameError, MAX_PAYLOAD_LENGTH};
pub use hello::{Hello, HelloError, ProtocolVersion};
pub use payload::DecodeError;
pub use reply::{ChildStatus, ErrorCode, Reply};
pub use request::{ChildSignal, Request};
pub use window_size::WindowSize;
