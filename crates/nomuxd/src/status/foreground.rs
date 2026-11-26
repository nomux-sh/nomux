//! The foreground program of the terminal of a session as a status source.
//!
//! This is the weakest source: it says only that a program other than the
//! command of the session has the terminal. The operating system does not
//! tell the daemon when the foreground changes, thus the daemon reads it
//! when a client asks for the status and when a different source changes.

use nomux_holder_protocol::discovery::{program_name, terminal_foreground_group};
use nomux_status::{Observation, Source, SourceReport, State};

/// What the foreground of the terminal of the command `child_pid` says.
pub(crate) fn report(child_pid: u32) -> SourceReport {
    let silent = |note: &str| SourceReport::silent(Source::ForegroundProgram, note);
    match terminal_foreground_group(child_pid) {
        Err(_) | Ok(None) => silent("the terminal of the session has no foreground program"),
        // The command can be a shell that waits or a program that works:
        // its place in the foreground does not say which.
        Ok(Some(group)) if group == child_pid => {
            silent("the command of the session is in the foreground of its terminal")
        }
        Ok(Some(group)) => SourceReport {
            source: Source::ForegroundProgram,
            root: Some(Observation::of(State::Working)),
            children: Vec::new(),
            note: match program_name(group) {
                Ok(Some(name)) => {
                    format!("the program {name} is in the foreground of the terminal")
                }
                _ => "a program other than the command of the session is in the foreground of \
                      the terminal"
                    .to_owned(),
            },
        },
    }
}
