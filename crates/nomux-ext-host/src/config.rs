//! The configuration of an extension host.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

/// Where the host keeps the extensions and what it gives to their processes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HostConfig {
    /// The directory of the host. It has the packages, the state files and
    /// the working directories of the installed extensions.
    pub directory: PathBuf,
    /// The names of the environment variables of the daemon that the process
    /// of an extension gets. The process gets no other variable.
    pub inherited_variables: Vec<OsString>,
    /// The limits that the host applies to each extension.
    pub limits: Limits,
}

impl HostConfig {
    /// A configuration with the default limits. The process of an extension
    /// gets `PATH` and `HOME`.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            inherited_variables: vec!["PATH".into(), "HOME".into()],
            limits: Limits::default(),
        }
    }
}

/// The time limits and the size limits of the host.
///
/// The limit of one frame is not here: the extension protocol sets it
/// (`nomux_extension::frame::MAX_ENVELOPE_LENGTH`).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Limits {
    /// The time from the start of the process to the response to
    /// `InitializeRequest`.
    pub start: Duration,
    /// The time that one call of a method of an extension can take.
    pub call: Duration,
    /// The time in which the extension answers a call after the host
    /// cancelled it.
    pub cancel: Duration,
    /// The pause between two health requests to a process that runs.
    pub health_interval: Duration,
    /// The time in which the extension answers a health request, and the
    /// time in which it takes one frame from its standard input.
    pub health: Duration,
    /// The time without a call after which the host stops the process.
    pub idle: Duration,
    /// The time from `ShutdownRequest` to the end of the process. After it,
    /// the host ends the process.
    pub shutdown: Duration,
    /// The pause before the first start after a failure. Each subsequent
    /// failure makes the pause two times longer.
    pub restart_delay_first: Duration,
    /// The longest pause before a start after a failure.
    pub restart_delay_max: Duration,
    /// The number of failures in sequence after which the host disables the
    /// extension.
    pub failures_before_disable: u32,
    /// The number of bytes of the standard error of one extension that the
    /// host keeps.
    pub stderr_log_bytes: usize,
    /// The number of bytes that an extension can write to its standard error
    /// in one `stderr_flood_window`.
    pub stderr_flood_bytes: usize,
    /// The length of the window of `stderr_flood_bytes`.
    pub stderr_flood_window: Duration,
    /// The number of events that the host keeps for a subscriber that is
    /// slow. A slower subscriber loses the oldest events.
    pub event_buffer: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            start: Duration::from_secs(10),
            call: Duration::from_secs(30),
            cancel: Duration::from_secs(5),
            health_interval: Duration::from_secs(10),
            health: Duration::from_secs(5),
            idle: Duration::from_secs(60),
            shutdown: Duration::from_secs(3),
            restart_delay_first: Duration::from_secs(1),
            restart_delay_max: Duration::from_secs(60),
            failures_before_disable: 5,
            stderr_log_bytes: 64 * 1024,
            stderr_flood_bytes: 1024 * 1024,
            stderr_flood_window: Duration::from_secs(10),
            event_buffer: 64,
        }
    }
}
