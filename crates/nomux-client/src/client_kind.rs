//! The kind of client that connects.

use nomux_proto::wire::v1 as wire;

/// The kind of client, which the daemon learns in the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    /// The `nomux` command-line client.
    Cli,
    /// The desktop client.
    Desktop,
    /// The mobile client.
    Mobile,
}

impl From<ClientKind> for wire::ClientKind {
    fn from(kind: ClientKind) -> Self {
        match kind {
            ClientKind::Cli => Self::Cli,
            ClientKind::Desktop => Self::Desktop,
            ClientKind::Mobile => Self::Mobile,
        }
    }
}
