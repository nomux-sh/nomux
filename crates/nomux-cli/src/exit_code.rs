//! The exit codes of `nomux`. Each command uses the same codes.

use nomux_client::{AttachEnd, ClientError};

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

/// The exit code of `attach`. If the session ended during the attach, the
/// code is the exit status of its command, as a shell gives it: the exit
/// code, or 128 plus the signal number.
pub(crate) fn of_attach_end(end: AttachEnd) -> u8 {
    const SIGNAL_BASE: i32 = 128;
    let status = match end {
        AttachEnd::Detached => return SUCCESS,
        AttachEnd::Lost | AttachEnd::Unknown => return ERROR,
        AttachEnd::Exited { code } => code,
        AttachEnd::Signaled { signal } => SIGNAL_BASE + signal,
    };
    // An exit status has 8 bits.
    status.to_le_bytes()[0]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exit_code_of_attach_is_the_exit_status_of_the_command() {
        assert_eq!(of_attach_end(AttachEnd::Detached), 0);
        assert_eq!(of_attach_end(AttachEnd::Exited { code: 0 }), 0);
        assert_eq!(of_attach_end(AttachEnd::Exited { code: 7 }), 7);
        // A shell gives 128 plus the signal number.
        assert_eq!(of_attach_end(AttachEnd::Signaled { signal: 15 }), 143);
        // An exit status has 8 bits.
        assert_eq!(of_attach_end(AttachEnd::Exited { code: 256 + 3 }), 3);
        assert_eq!(of_attach_end(AttachEnd::Lost), ERROR);
    }
}
