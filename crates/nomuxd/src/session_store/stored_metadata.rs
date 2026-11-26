//! Reads the session metadata that a holder wrote.

use std::path::Path;

use nomux_holder_protocol::session_state::SessionMetadata;

use super::small_file;

/// The command line of a process has 2 MiB at most on Linux, and each byte
/// of it can be 4 bytes in the file.
const MAX_LENGTH: u64 = 8 * 1024 * 1024;

/// Reads the metadata file `path`.
///
/// # Errors
///
/// Returns the reason, for a person, why the file cannot be used.
pub(super) fn read(path: &Path) -> Result<SessionMetadata, String> {
    let bytes = small_file::read(path, MAX_LENGTH)
        .map_err(|error| format!("the session metadata cannot be read: {error}"))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "the session metadata is not UTF-8 text".to_owned())?;
    SessionMetadata::parse(&text).map_err(|error| error.to_string())
}
