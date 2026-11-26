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
//! - `sessions`, `session_registry`, `session_handle`, `session_id`,
//!   `session_error`: the operations on sessions, the list of the sessions
//!   that the daemon knows, and the ways into one session.
//! - `session_store`, `session_journal`, `stopped_sessions`: what a session
//!   keeps on disk, its output history, and the sessions without a holder:
//!   dormant, ended and damaged.
//! - `session_memory`: the memory of the processes of each session, measured
//!   when a client asks.
//! - `holder`: the holder side. One task owns the connection with the lease
//!   to one holder. A second connection reads the terminal while a client
//!   is attached.
//! - `attachment`: one attached client: the screen and the output go to the
//!   client, and the input comes from it.
//! - `status`, `events`, `clock`: the status of the Agent run of each
//!   session from its sources, the event log and the subscriptions of the
//!   clients. One status task for each session with a holder waits on the
//!   holder for a change of its program reports.
//! - `extensions`: the extension host of the daemon and the host services
//!   that an extension can call. `nomux-ext-host` runs the extensions.
//! - `shutdown`, `in_flight_requests`: the stop of the daemon.
//! - `detach`, `logging`, `arguments`, `settings`: the process setup.
//!
//! # Boundaries
//!
//! - The daemon does not own a PTY. A holder owns it, thus a daemon restart
//!   stops no session.
//! - The daemon talks to a holder only through `nomux-holder-protocol`.
//! - An extension is a separate process. Its failure stays in that process.
//!
//! # Status
//!
//! The daemon creates, lists and ends sessions, attaches clients to them,
//! reads their journal, and starts a dormant session again. A client on a
//! different host reaches the socket through SSH. It installs extensions
//! from a directory and calls their methods. The daemon has no state
//! database.

mod arguments;
mod attachment;
mod client_connection;
mod clock;
mod daemon;
mod detach;
mod events;
mod extensions;
mod handlers;
mod handshake;
mod holder;
mod hooks;
mod in_flight_requests;
mod instance_lock;
mod listener;
mod logging;
mod runtime_directory;
mod session_error;
mod session_handle;
mod session_id;
mod session_journal;
mod session_memory;
mod session_registry;
mod session_store;
mod sessions;
mod settings;
mod shutdown;
mod status;
mod stopped_sessions;

use std::env;
use std::io;
use std::process::ExitCode;

use rustix::io::Errno;

use crate::arguments::{Invocation, RunningDaemon, USAGE};

const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let result = match arguments::parse(env::args_os().skip(1)) {
        Ok(Invocation::Version) => {
            return nomux_core::version_only_main(nomux_core::version_line!());
        }
        Ok(Invocation::Detach(running_daemon)) => detach::start_detached_daemon(running_daemon),
        Ok(Invocation::Serve(running_daemon)) => serve(running_daemon),
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

fn serve(running_daemon: RunningDaemon) -> io::Result<()> {
    leave_session_of_starter()?;
    logging::start();
    // The daemon waits for I/O almost all the time. One thread is sufficient
    // and keeps the memory small.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    // The tasks of the client connections are local to this thread.
    runtime.block_on(tokio::task::LocalSet::new().run_until(daemon::run(running_daemon)))
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
