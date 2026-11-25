//! The handshake: the answer of the daemon to the first request of a client.

use nomux_proto::compatibility::{VersionError, VersionRange, negotiate};
use nomux_proto::wire::v1::{Error, ErrorCode, Hello, Welcome};

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
            // No capability flag is active in this protocol version.
            capabilities: Vec::new(),
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
