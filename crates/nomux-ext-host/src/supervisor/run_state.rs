//! The state of the process of one extension, as the host reports it.

use std::time::Instant;

use crate::failure::Failure;

/// What the process of an extension does now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// No process runs. The subsequent call starts one.
    Stopped,
    /// The process started. The handshake or the initialization is not
    /// complete.
    Starting,
    /// The process runs and serves calls.
    Running,
    /// The process failed, and no process runs.
    Failed {
        /// Why the process failed.
        failure: Failure,
        /// From this time, a call starts the extension again. `None` if
        /// the host disabled the extension.
        retry_at: Option<Instant>,
    },
}
