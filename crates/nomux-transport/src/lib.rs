//! Transport layer of nomux.
//!
//! # Responsibility
//!
//! This crate moves the envelopes of the control protocol between a client
//! and the daemon. It contains the transport for the local host: a Unix
//! socket that only the user of the daemon can use.
//!
//! - [`EnvelopeStream`]: the framing of the control channel. Each envelope
//!   has a varint length prefix, as the `proto` repository specifies.
//! - [`LocalListener`], [`connect`] and [`default_socket_path`]: the socket
//!   of the daemon on the local host.
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
//! The crate has the Unix socket transport for the control channel. The SSH
//! channels, the QUIC streams, the terminal data channel and the credit
//! windows are not written yet.

mod envelope_stream;
mod local_socket;

pub use envelope_stream::EnvelopeStream;
pub use local_socket::{LocalListener, SOCKET_FILE, connect, default_socket_path};
