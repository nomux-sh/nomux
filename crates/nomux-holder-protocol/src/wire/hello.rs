//! The hello: the first message on a connection, from the holder.
//!
//! ```text
//! offset  size  field
//!      0     2  protocol major version
//!      2     2  protocol minor version
//!      4     4  process ID of the child
//!      8     2  rows
//!     10     2  columns
//!     12     1  length of the session identifier
//!     13     n  session identifier (ASCII)
//! ```
//!
//! The holder sends the read-only descriptor of the ring buffer together
//! with the hello, as `SCM_RIGHTS` ancillary data.

use std::error::Error;
use std::fmt;

use super::frame::Frame;
use super::payload::{DecodeError, PayloadReader};
use super::request::Request;
use super::window_size::WindowSize;
use crate::SessionId;

const KIND: u16 = 0x0001;

/// The version of the holder protocol. It is independent of the product
/// version and of the control protocol between the clients and the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolVersion {
    /// A daemon and a holder with different major versions cannot talk.
    pub major: u16,
    /// Each addition to the protocol increases the minor version.
    pub minor: u16,
}

impl ProtocolVersion {
    /// The version that this crate implements.
    pub const CURRENT: Self = Self { major: 1, minor: 1 };

    /// `true` if a holder of this version knows `request`. The daemon asks
    /// this before it sends a request that a minor version added.
    #[must_use]
    pub fn supports(self, request: &Request<'_>) -> bool {
        self.major == Self::CURRENT.major && self.minor >= request.minor_version()
    }
}

/// What a holder says about itself when a connection opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    /// The protocol version of the holder.
    pub protocol: ProtocolVersion,
    /// The session that the holder owns.
    pub session_id: SessionId,
    /// The process ID of the child command.
    pub child_pid: u32,
    /// The current size of the terminal window.
    pub size: WindowSize,
}

impl Hello {
    /// Appends the encoded hello frame to `output`.
    pub fn encode(&self, output: &mut Vec<u8>) {
        let session_id = self.session_id.as_str().as_bytes();
        let mut payload = Vec::with_capacity(13 + session_id.len());
        payload.extend_from_slice(&self.protocol.major.to_le_bytes());
        payload.extend_from_slice(&self.protocol.minor.to_le_bytes());
        payload.extend_from_slice(&self.child_pid.to_le_bytes());
        payload.extend_from_slice(&self.size.encode());
        // A session identifier has 64 bytes at most.
        payload.push(session_id.len().to_le_bytes()[0]);
        payload.extend_from_slice(session_id);
        let frame = Frame {
            kind: KIND,
            request_id: 0,
            payload: &payload,
        };
        frame.encode(output);
    }

    /// Decodes the first frame of a connection.
    ///
    /// # Errors
    ///
    /// Returns [`HelloError::UnsupportedVersion`] if the holder has a
    /// different major version. The fields after the version are then not
    /// read. Returns [`HelloError::Invalid`] if the frame is not a hello.
    pub fn decode(frame: Frame<'_>) -> Result<Self, HelloError> {
        if frame.kind != KIND {
            return Err(HelloError::Invalid);
        }
        let mut reader = PayloadReader::new(frame.payload);
        let protocol = ProtocolVersion {
            major: reader.u16()?,
            minor: reader.u16()?,
        };
        if protocol.major != ProtocolVersion::CURRENT.major {
            return Err(HelloError::UnsupportedVersion(protocol));
        }
        let child_pid = reader.u32()?;
        let size = WindowSize::decode(&mut reader)?;
        let session_id_length = usize::from(reader.u8()?);
        let session_id = str::from_utf8(reader.bytes(session_id_length)?)
            .ok()
            .and_then(|text| text.parse().ok())
            .ok_or(HelloError::Invalid)?;
        Ok(Self {
            protocol,
            session_id,
            child_pid,
            size,
        })
    }
}

/// The first frame of a connection cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelloError {
    /// The holder speaks a major version that this crate does not implement.
    UnsupportedVersion(ProtocolVersion),
    /// The frame is not a hello, or its payload is not valid.
    Invalid,
}

impl From<DecodeError> for HelloError {
    fn from(_: DecodeError) -> Self {
        Self::Invalid
    }
}

impl fmt::Display for HelloError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => write!(
                formatter,
                "holder protocol version {}.{} is not supported",
                version.major, version.minor
            ),
            Self::Invalid => formatter.write_str("the first frame is not a valid hello"),
        }
    }
}

impl Error for HelloError {}
