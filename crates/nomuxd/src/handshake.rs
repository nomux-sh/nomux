//! The handshake: the answer of the daemon to the first request of a client.

use nomux_proto::capabilities::{OUTPUT_HISTORY, SESSION_ACTIVITY, SESSION_NAMES, SESSION_SANDBOX};
use nomux_proto::compatibility::{VersionError, VersionRange, negotiate};
use nomux_proto::wire::v1::{Error, ErrorCode, Hello, Welcome};

/// The optional features of the protocol that are active on one
/// connection: the client and the daemon have their capability flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClientFeatures {
    /// The client reads output of an earlier epoch, and the frames that say
    /// that output is gone or that the command started again.
    pub(crate) output_history: bool,
}

impl ClientFeatures {
    pub(crate) fn of(hello: &Hello) -> Self {
        Self {
            output_history: hello.capabilities.iter().any(|flag| flag == OUTPUT_HISTORY),
        }
    }
}

/// Answers a `Hello`. The result is the `Welcome`, or the error for a client
/// that has no common protocol version with this daemon.
pub(crate) fn answer(hello: &Hello) -> Result<Welcome, Error> {
    let daemon = VersionRange::CURRENT;
    let client = VersionRange {
        version: hello.version,
        oldest: hello.oldest,
    };
    match negotiate(daemon, client) {
        Ok(_) => Ok(Welcome {
            version: daemon.version,
            oldest: daemon.oldest,
            capabilities: [
                OUTPUT_HISTORY,
                SESSION_NAMES,
                SESSION_ACTIVITY,
                SESSION_SANDBOX,
            ]
            .map(str::to_owned)
            .to_vec(),
            // The daemon has no host identity yet.
            host_id: String::new(),
        }),
        Err(error) => Err(Error {
            code: ErrorCode::VersionIncompatible.into(),
            message: incompatible_message(error, daemon),
        }),
    }
}

fn incompatible_message(error: VersionError, daemon: VersionRange) -> String {
    let product_version = env!("CARGO_PKG_VERSION");
    match error {
        VersionError::InvalidRange(range) => format!(
            "the client sent a protocol version range that is not valid: {} to {}",
            range.oldest, range.version
        ),
        VersionError::Incompatible { remote: client, .. } => format!(
            "the client and the daemon have no common protocol version: the client supports {} \
             to {}, the daemon (nomuxd {product_version}) supports {} to {}. Update the older \
             of the two.",
            client.oldest, client.version, daemon.oldest, daemon.version
        ),
    }
}
