//! Why the host stopped the process of an extension.

use std::fmt;
use std::io;
use std::process::ExitStatus;

use nomux_extension::frame::{FrameError, MAX_ENVELOPE_LENGTH};

/// The reason why the process of an extension ended without a stop request
/// of the host, or why the host ended it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failure {
    /// The host cannot start the program of the daemon part.
    Spawn(String),
    /// The process ended.
    Exited(ExitStatus),
    /// The process closed its standard output and did not end.
    ClosedOutput,
    /// The extension did not complete the handshake and the initialization
    /// in the permitted time.
    StartTimeout,
    /// The extension did not answer a health request in the permitted time.
    HealthTimeout,
    /// The extension did not answer a call in the permitted time after the
    /// host cancelled the call.
    CancelTimeout,
    /// The extension did not take a frame from its standard input in the
    /// permitted time.
    InputBlocked,
    /// The process did not end in the permitted time after
    /// `ShutdownRequest`.
    ShutdownTimeout,
    /// The extension sent bytes that are not a frame.
    BadFrame(String),
    /// The extension sent a frame that is larger than the limit of the
    /// protocol.
    OversizedFrame {
        /// The length that the frame states.
        length: u64,
    },
    /// The extension sent a message that the protocol does not permit.
    Protocol(String),
    /// The extension wrote more bytes to its standard error than the host
    /// permits.
    StderrFlood,
    /// The extension cannot use the extension API version of the host, or
    /// its program was built for a version that its manifest does not name.
    VersionIncompatible(String),
    /// The program has a different identifier than the manifest.
    IdentityMismatch {
        /// The identifier in `Welcome`.
        claimed: String,
    },
    /// The extension answered `InitializeRequest` with an error.
    InitializeRefused(String),
    /// A stream of the process failed.
    Stream(String),
}

impl Failure {
    /// Returns `true` if a new start gives the same failure. The host
    /// disables the extension immediately.
    pub(crate) fn is_permanent(&self) -> bool {
        matches!(
            self,
            Self::VersionIncompatible(_) | Self::IdentityMismatch { .. }
        )
    }

    pub(crate) fn from_frame_error(error: FrameError) -> Self {
        match error {
            FrameError::TooLarge { length } => Self::OversizedFrame { length },
            FrameError::Io(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                Self::BadFrame("the standard output ended in a frame".to_owned())
            }
            FrameError::Io(error) => Self::Stream(error.to_string()),
            error @ (FrameError::InvalidLength | FrameError::Decode(_)) => {
                Self::BadFrame(error.to_string())
            }
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "the program did not start: {error}"),
            Self::Exited(status) => write!(formatter, "the process ended: {status}"),
            Self::ClosedOutput => formatter.write_str("the process closed its standard output"),
            Self::StartTimeout => formatter.write_str("the extension did not start in time"),
            Self::HealthTimeout => {
                formatter.write_str("the extension did not answer a health request in time")
            }
            Self::CancelTimeout => {
                formatter.write_str("the extension did not answer a cancelled call in time")
            }
            Self::InputBlocked => {
                formatter.write_str("the extension did not read its standard input in time")
            }
            Self::ShutdownTimeout => {
                formatter.write_str("the process did not end in time after the shutdown request")
            }
            Self::BadFrame(problem) => {
                write!(formatter, "the extension sent a bad frame: {problem}")
            }
            Self::OversizedFrame { length } => write!(
                formatter,
                "the extension sent a frame of {length} bytes, but the limit is {MAX_ENVELOPE_LENGTH} bytes"
            ),
            Self::Protocol(problem) => {
                write!(
                    formatter,
                    "the extension did not obey the protocol: {problem}"
                )
            }
            Self::StderrFlood => {
                formatter.write_str("the extension wrote too much to its standard error")
            }
            Self::VersionIncompatible(problem) => {
                write!(
                    formatter,
                    "the extension API version is not compatible: {problem}"
                )
            }
            Self::IdentityMismatch { claimed } => write!(
                formatter,
                "the program has the identifier `{claimed}`, which is not the identifier of the manifest"
            ),
            Self::InitializeRefused(message) => {
                write!(
                    formatter,
                    "the extension refused the initialization: {message}"
                )
            }
            Self::Stream(error) => write!(formatter, "a stream of the process failed: {error}"),
        }
    }
}

impl std::error::Error for Failure {}
