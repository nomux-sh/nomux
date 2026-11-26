//! Transport layer of nomux.
//!
//! # Responsibility
//!
//! This crate moves the envelopes of the control protocol and the terminal
//! frames between a client and the daemon. It contains the transport for the
//! local host, a Unix socket that only the user of the daemon can use, and
//! the transport for a remote host: SSH channels to that socket.
//!
//! - [`EnvelopeStream`]: the framing of the control channel. Each envelope
//!   has a varint length prefix, as the `proto` repository specifies.
//! - [`FrameReader`] and [`FrameWriter`]: the framing of the terminal
//!   channels of an attach. [`EnvelopeStream::into_frames`] makes them from
//!   the connection that sent the attach request.
//! - [`LocalListener`] and [`connect`]: the socket of the daemon on the
//!   local host.
//! - [`ssh`]: the SSH client. [`ssh::SshConnection::open_unix_socket`] gives
//!   an [`EnvelopeStream`] to the daemon of a remote host.
//!
//! # Boundaries
//!
//! - This crate does not define the protocol messages. The `proto` repository
//!   owns the schema.
//! - This crate does not read the content of the envelopes that it moves.
//! - The holder does not use this crate. The holder has no async runtime.
//!
//! # Status
//!
//! The crate has the Unix socket transport and the SSH transport for the
//! control channel and for the terminal channels. The flow control of an SSH
//! channel is the window of the SSH protocol. The QUIC streams are not
//! written yet.

mod byte_stream;
mod envelope_stream;
mod frame_stream;
mod local_socket;
pub mod ssh;

pub use envelope_stream::EnvelopeStream;
pub use frame_stream::{FrameReader, FrameWriter};
pub use local_socket::{LocalListener, SOCKET_FILE, connect};
