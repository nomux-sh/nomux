//! The state of a daemon.

use nomux_proto::wire::v1 as wire;

/// What a daemon says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    /// The product version of the daemon.
    pub daemon_version: String,
    /// The process ID of the daemon on the host.
    pub daemon_pid: u32,
    /// The number of sessions that the daemon knows.
    pub session_count: u32,
}

impl From<wire::HostInfoResponse> for HostInfo {
    fn from(info: wire::HostInfoResponse) -> Self {
        Self {
            daemon_version: info.daemon_version,
            daemon_pid: info.daemon_pid,
            session_count: info.session_count,
        }
    }
}
