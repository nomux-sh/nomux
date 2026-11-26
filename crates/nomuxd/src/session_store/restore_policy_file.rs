//! The restore policy of one session on disk: the file `restore-policy` in
//! its state directory. The file has one line: `auto`, `on-focus` or
//! `manual`.
//!
//! The daemon owns this file. The holder owns the `session` file and writes
//! it again from its memory, thus a value that the daemon puts there does
//! not stay.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use nomux_core::configuration::RestorePolicy;

use super::small_file;

const FILE: &str = "restore-policy";
/// The policy goes to this file first. A rename then replaces the policy
/// file in one step.
const NEW_FILE: &str = "restore-policy.new";
const OWNER_READ_WRITE: u32 = 0o600;
const MAX_LENGTH: u64 = 64;

/// The policy in `session_directory`. `None` if the session has no policy,
/// or if the file does not have the name of a policy: the session then has
/// the default policy.
pub(super) fn read(session_directory: &Path) -> Option<RestorePolicy> {
    let bytes = small_file::read(&session_directory.join(FILE), MAX_LENGTH).ok()?;
    std::str::from_utf8(&bytes).ok()?.trim().parse().ok()
}

pub(super) fn write(session_directory: &Path, policy: RestorePolicy) -> io::Result<()> {
    let new_file = session_directory.join(NEW_FILE);
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(OWNER_READ_WRITE)
        .open(&new_file)?
        .write_all(format!("{policy}\n").as_bytes())?;
    fs::rename(new_file, session_directory.join(FILE))
}
