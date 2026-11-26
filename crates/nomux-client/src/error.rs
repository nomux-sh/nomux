//! Why a connection or a request failed.

use std::error::Error;
use std::fmt;
use std::io;

use nomux_core::configuration::ConfigurationError;
use nomux_proto::wire::v1 as wire;
use nomux_transport::ssh::SshError;

use crate::Session;
use crate::extension::ExtensionCallError;

/// A connection to the daemon or a request did not succeed.
#[derive(Debug)]
pub enum ClientError {
    /// No daemon listens on the socket.
    DaemonNotRunning,
    /// The daemon cannot start.
    DaemonStart(io::Error),
    /// The connection failed, or the daemon sent something that is not a
    /// valid reply.
    Connection(io::Error),
    /// The client and the daemon have no common protocol version.
    VersionIncompatible(String),
    /// The user does not have permission.
    PermissionDenied(String),
    /// The item that the request names does not exist.
    NotFound(String),
    /// The operation did not complete in the permitted time.
    Timeout(String),
    /// The daemon did not do the request for a different reason.
    Failed(String),
    /// A method of an extension answered a call with an error.
    ExtensionAnswered(ExtensionCallError),
    /// The target names more than one session: the identifier or the name
    /// of each candidate starts with it.
    AmbiguousTarget {
        /// The text that the user gave.
        target: String,
        /// The sessions that the target can name.
        candidates: Vec<Session>,
    },
}

impl ClientError {
    /// The error for a reply that the protocol does not permit.
    pub(crate) fn invalid_reply(message: &'static str) -> Self {
        Self::Connection(io::Error::new(io::ErrorKind::InvalidData, message))
    }

    /// Returns `true` if the connection ended or cannot be made now. A new
    /// attempt can succeed then.
    pub(crate) fn is_connection_loss(&self) -> bool {
        matches!(self, Self::Connection(error) if error.kind() != io::ErrorKind::InvalidData)
    }
}

impl From<SshError> for ClientError {
    fn from(error: SshError) -> Self {
        match error {
            SshError::SocketUnavailable => Self::DaemonNotRunning,
            SshError::Authentication { .. } => Self::PermissionDenied(error.to_string()),
            error if error.is_connection_failure() => {
                Self::Connection(io::Error::new(io::ErrorKind::NotConnected, error))
            }
            error => Self::Failed(error.to_string()),
        }
    }
}

impl From<ConfigurationError> for ClientError {
    fn from(error: ConfigurationError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<wire::Error> for ClientError {
    fn from(error: wire::Error) -> Self {
        let message = error.message;
        match wire::ErrorCode::try_from(error.code) {
            Ok(wire::ErrorCode::NotFound) => Self::NotFound(message),
            Ok(wire::ErrorCode::Timeout) => Self::Timeout(message),
            Ok(wire::ErrorCode::PermissionDenied) => Self::PermissionDenied(message),
            Ok(wire::ErrorCode::VersionIncompatible) => Self::VersionIncompatible(message),
            // A code from a later protocol version is an error without a
            // more exact reason.
            Ok(wire::ErrorCode::Failed | wire::ErrorCode::Unspecified) | Err(_) => {
                Self::Failed(message)
            }
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DaemonNotRunning => formatter.write_str("the daemon does not run"),
            Self::DaemonStart(error) => write!(formatter, "the daemon cannot start: {error}"),
            Self::Connection(error) => {
                write!(formatter, "the connection to the daemon failed: {error}")
            }
            Self::VersionIncompatible(message)
            | Self::PermissionDenied(message)
            | Self::NotFound(message)
            | Self::Timeout(message)
            | Self::Failed(message) => formatter.write_str(message),
            Self::ExtensionAnswered(error) => error.fmt(formatter),
            Self::AmbiguousTarget { target, candidates } => {
                write!(
                    formatter,
                    "{target} can be more than one session. Give more of the identifier or of \
                     the name of one of them:"
                )?;
                let name_width = candidates
                    .iter()
                    .map(|session| session.name.as_deref().map_or(1, str::len))
                    .max()
                    .unwrap_or(0);
                for session in candidates {
                    let name = session.name.as_deref().unwrap_or("-");
                    let line = format!(
                        "  {}  {name:<name_width$}  {}",
                        session.id,
                        session.command.join(" ")
                    );
                    write!(formatter, "\n{}", line.trim_end())?;
                }
                Ok(())
            }
        }
    }
}

impl Error for ClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DaemonStart(error) | Self::Connection(error) => Some(error),
            _ => None,
        }
    }
}
