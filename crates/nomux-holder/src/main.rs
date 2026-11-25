//! `nomux-holder`: the session holder.
//!
//! # Responsibility
//!
//! One holder owns the PTY of one session. It starts the command of the
//! session in a new PTY, always reads the PTY, and writes the output to a
//! ring buffer in shared memory that the daemon reads. The daemon controls
//! the holder through a Unix socket and finds it through its discovery
//! files. The holder stays alive when the daemon stops, crashes or is
//! updated, thus the session stays alive.
//!
//! `docs/holder.md` has the process model and the lifecycle. The contract
//! with the daemon is in the `nomux-holder-protocol` crate.
//!
//! # Boundaries
//!
//! - The holder has no async runtime and one thread. It must not depend on
//!   `tokio` or on a crate that starts a runtime.
//! - The holder does not talk to clients. Only the daemon connects to it.
//! - The holder stays small. The targets are in `bench/README.md`.
//!
//! # Status
//!
//! The holder does not have the VT state, the journal, the checkpoint and
//! the OSC parser yet.

mod arguments;
mod child;
mod child_exit_signal;
mod control;
mod discovery_directory;
mod event_loop;
mod input_queue;
mod pty;
mod ring_memory;
mod session;

use std::env;
use std::io;
use std::process::ExitCode;

use nomux_holder_protocol::ring::{RingWriter, region_length};
use rustix::io::Errno;

use crate::arguments::{Invocation, SessionArguments, USAGE};
use crate::child_exit_signal::ChildExitSignal;
use crate::control::ControlServer;
use crate::discovery_directory::DiscoveryDirectory;
use crate::session::Session;

const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    match arguments::parse(env::args_os().skip(1)) {
        Ok(Invocation::Version) => nomux_core::version_only_main(nomux_core::version_line!()),
        Ok(Invocation::Session(arguments)) => match run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("nomux-holder: {error}");
                ExitCode::FAILURE
            }
        },
        Err(message) => {
            eprint!("nomux-holder: {message}\n\n{USAGE}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn run(arguments: &SessionArguments) -> io::Result<()> {
    leave_session_of_starter()?;
    let child_exit = ChildExitSignal::install()?;

    let memory = ring_memory::create(region_length(arguments.ring_capacity))?;
    let ring = RingWriter::create(&memory.writable, arguments.ring_capacity)?;
    // The mapping keeps the memory. The holder needs the descriptor no more.
    drop(memory.writable);

    let directory = DiscoveryDirectory::create(&arguments.runtime_directory, &arguments.session_id)
        .map_err(|error| with_context("cannot make the discovery directory", &error))?;
    let server = ControlServer::bind(&directory.socket_path(), memory.read_only)
        .map_err(|error| with_context("cannot make the control socket", &error))?;
    let session = Session::start(arguments, ring)
        .map_err(|error| with_context("cannot start the command", &error))?;
    directory.mark_ready()?;

    event_loop::run(session, server, child_exit, arguments.exit_timeout)
}

/// Makes the holder the leader of a new session. Thus a signal for the
/// terminal or the process group of the starter, for example from Ctrl-C,
/// does not reach the holder.
fn leave_session_of_starter() -> io::Result<()> {
    match rustix::process::setsid() {
        // `EPERM`: the holder leads a process group already. This is the
        // case when a person starts it from an interactive shell.
        Ok(_) | Err(Errno::PERM) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn with_context(context: &str, error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{context}: {error}"))
}
