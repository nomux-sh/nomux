//! Why a call of a method of an extension did not give a result.

use std::fmt;
use std::time::Duration;

use nomux_extension::frame::MAX_ENVELOPE_LENGTH;
use nomux_extension::{ApiRequirement, ApiVersion};

use crate::failure::Failure;
use crate::host_services::ServiceError;

/// The reason why [`crate::ExtensionHost::call`] has no result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CallError {
    /// No extension with this identifier is installed.
    NotInstalled,
    /// The extension is disabled.
    Disabled {
        /// Why the extension is disabled.
        reason: String,
    },
    /// The package of the extension has no daemon part.
    NoDaemonPart,
    /// The manifest of the extension asks for an extension API version that
    /// the host does not supply. The host does not start the process.
    IncompatibleApi {
        /// The requirement in the manifest.
        required: ApiRequirement,
        /// The extension API version of the host.
        host: ApiVersion,
    },
    /// The extension failed a short time before. The host does not start it
    /// again before the pause ends.
    BackingOff {
        /// The last failure.
        failure: Failure,
        /// The time that is left of the pause.
        retry_in: Duration,
    },
    /// The process of the extension failed before it answered the call.
    Failed(Failure),
    /// The host stopped the extension before it answered the call.
    Stopped,
    /// The extension did not answer in the permitted time.
    Timeout,
    /// The call is larger than one frame of the protocol.
    TooLarge {
        /// The number of bytes of the encoded call.
        length: usize,
    },
    /// The extension answered the call with an error.
    Answered(ServiceError),
}

impl fmt::Display for CallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => formatter.write_str("the extension is not installed"),
            Self::Disabled { reason } => write!(formatter, "the extension is disabled: {reason}"),
            Self::NoDaemonPart => formatter.write_str("the extension has no daemon part"),
            Self::IncompatibleApi { required, host } => write!(
                formatter,
                "the extension needs the extension API version {required}, but the host supplies {host}"
            ),
            Self::BackingOff { failure, retry_in } => write!(
                formatter,
                "the extension failed ({failure}); the host starts it again in {retry_in:?}"
            ),
            Self::Failed(failure) => write!(formatter, "the extension failed: {failure}"),
            Self::Stopped => formatter.write_str("the host stopped the extension"),
            Self::Timeout => formatter.write_str("the extension did not answer in time"),
            Self::TooLarge { length } => write!(
                formatter,
                "the call has {length} bytes, but the limit is {MAX_ENVELOPE_LENGTH} bytes"
            ),
            Self::Answered(error) => write!(formatter, "the extension answered: {error}"),
        }
    }
}

impl std::error::Error for CallError {}
