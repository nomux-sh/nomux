//! `nomuxd`: the nomux daemon.
//!
//! # Responsibility
//!
//! One daemon runs for each user on each host. It finds the session holders,
//! connects to them, takes their single-writer lease, starts new holders and
//! serves the clients on a Unix socket. `docs/daemon.md` has the process
//! model and the lifecycle.
//!
//! The modules:
//!
//! - `daemon`: the start sequence and the stop sequence.
//! - `runtime_directory`, `instance_lock`: the private directory of the
//!   daemon, and the rule that one daemon uses it at a time.
//! - `listener`, `client_connection`, `handshake`, `handlers`: the client
//!   side. One task serves one client connection.
//! - `sessions`, `session_registry`, `session_id`: the operations on
//!   sessions and the list of the sessions that the daemon knows.
//! - `holder`: the holder side. One task owns the connection to one holder.
//! - `shutdown`, `in_flight_requests`: the stop of the daemon.
//! - `detach`, `logging`, `arguments`: the process setup.
//!
//! # Boundaries
//!
//! - The daemon does not own a PTY. A holder owns it, thus a daemon restart
//!   stops no session.
//! - The daemon talks to a holder only through `nomux-holder-protocol`.
//!
//! # Status
//!
//! The daemon creates, lists and ends sessions. It does not send terminal
//! data to a client yet, and it has no state database, no extension host and
//! no remote transport.

mod arguments;
mod client_connection;
mod daemon;
mod detach;
mod handlers;
mod handshake;
mod holder;
mod in_flight_requests;
mod instance_lock;
mod listener;
mod logging;
mod runtime_directory;
mod session_id;
mod session_registry;
mod sessions;
mod shutdown;

use std::env;
use std::io;
use std::process::ExitCode;

use rustix::io::Errno;

use crate::arguments::{Invocation, USAGE};

const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let result = match arguments::parse(env::args_os().skip(1)) {
        Ok(Invocation::Version) => {
            return nomux_core::version_only_main(nomux_core::version_line!());
        }
        Ok(Invocation::Detach) => detach::start_detached_daemon(),
        Ok(Invocation::Serve) => serve(),
        Err(message) => {
            eprint!("nomuxd: {message}\n\n{USAGE}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nomuxd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn serve() -> io::Result<()> {
    leave_session_of_starter()?;
    logging::start();
    // The daemon waits for I/O almost all the time. One thread is sufficient
    // and keeps the memory small.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(daemon::run())
}

/// Makes the daemon the leader of a new session. Thus the end of the
/// terminal or of the process group of the starter does not stop the daemon.
fn leave_session_of_starter() -> io::Result<()> {
    match rustix::process::setsid() {
        // `EPERM`: the daemon leads a process group already. This is the
        // case when a person starts it from an interactive shell.
        Ok(_) | Err(Errno::PERM) => Ok(()),
        Err(error) => Err(error.into()),
    }
}
