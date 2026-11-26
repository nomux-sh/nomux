//! The record of a panic of the holder. The release build aborts at a
//! panic, and the daemon reads the standard error of a holder only while
//! the holder starts. Thus the panic hook also appends the message and its
//! location to the file `holder-panic` of the session state directory: the
//! evidence stays after the end of the holder.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use nomux_holder_protocol::session_state::PANIC_FILE;

const OWNER_READ_WRITE: u32 = 0o600;

/// Sets the panic hook: the default hook, then the record in
/// `session_directory` if the directory exists. A pooled holder has no
/// session state directory before its assignment.
pub(crate) fn install(session_directory: PathBuf) {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let _ = append(&session_directory, &line(info));
    }));
}

fn line(info: &PanicHookInfo<'_>) -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    let location = info
        .location()
        .map_or_else(|| "an unknown location".to_owned(), ToString::to_string);
    let message = info.payload_as_str().unwrap_or("a panic without a text");
    format!("{seconds} panicked at {location}: {message}\n")
}

fn append(session_directory: &Path, line: &str) -> io::Result<()> {
    if !session_directory.is_dir() {
        return Ok(());
    }
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(OWNER_READ_WRITE)
        .open(session_directory.join(PANIC_FILE))?
        .write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_panic_leaves_its_message_and_location_in_the_session_directory() {
        let directory = tempfile::tempdir().unwrap();
        install(directory.path().to_owned());

        let result = panic::catch_unwind(|| panic!("the test panic {}", 85));
        let _ = panic::take_hook();

        assert!(result.is_err());
        let record = fs::read_to_string(directory.path().join(PANIC_FILE)).unwrap();
        assert!(
            record.contains(" panicked at crates/nomux-holder/src/panic_record.rs:"),
            "{record}"
        );
        assert!(record.ends_with(": the test panic 85\n"), "{record}");
    }
}
