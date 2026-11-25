//! The exit codes of `nomux`. Each command uses the same codes.

use nomux_client::ClientError;

pub(crate) const SUCCESS: u8 = 0;
pub(crate) const ERROR: u8 = 1;
pub(crate) const NOT_FOUND: u8 = 2;
pub(crate) const TIMEOUT: u8 = 3;
pub(crate) const NO_PERMISSION: u8 = 4;
pub(crate) const VERSION_MISMATCH: u8 = 5;

pub(crate) fn of_error(error: &ClientError) -> u8 {
    match error {
        ClientError::NotFound(_) => NOT_FOUND,
        ClientError::Timeout(_) => TIMEOUT,
        ClientError::PermissionDenied(_) => NO_PERMISSION,
        ClientError::VersionIncompatible(_) => VERSION_MISMATCH,
        ClientError::DaemonNotRunning
        | ClientError::DaemonStart(_)
        | ClientError::Connection(_)
        | ClientError::Failed(_) => ERROR,
    }
}

/// The name of an exit code in the JSON output.
pub(crate) fn name(code: u8) -> &'static str {
    match code {
        NOT_FOUND => "not_found",
        TIMEOUT => "timeout",
        NO_PERMISSION => "no_permission",
        VERSION_MISMATCH => "version_mismatch",
        _ => "error",
    }
}
