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
    /// What the operating system of the host gives for a memory limit of a
    /// session. `None` from an older daemon.
    pub memory_limit: Option<MemoryLimitSupport>,
}

/// What the operating system of a host gives for a memory limit of a
/// session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryLimitSupport {
    /// `true` if the operating system ends the processes of a session at
    /// its limit. `false` if the limit is a threshold for hibernation only.
    pub applied: bool,
    /// How the limit is applied, or why the operating system applies none.
    pub detail: String,
}

impl From<wire::HostInfoResponse> for HostInfo {
    fn from(info: wire::HostInfoResponse) -> Self {
        Self {
            daemon_version: info.daemon_version,
            daemon_pid: info.daemon_pid,
            session_count: info.session_count,
            memory_limit: info.memory_limit.map(|support| MemoryLimitSupport {
                applied: support.applied,
                detail: support.detail,
            }),
        }
    }
}
