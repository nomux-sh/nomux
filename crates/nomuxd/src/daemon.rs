//! The start sequence and the stop sequence of the daemon.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use nomux_transport::{LocalListener, SOCKET_FILE};
use tracing::info;

use crate::arguments::RunningDaemon;
use crate::handlers::Context;
use crate::in_flight_requests::InFlightRequests;
use crate::instance_lock::InstanceLock;
use crate::listener;
use crate::runtime_directory;
use crate::sessions::Sessions;
use crate::shutdown::Shutdown;

/// Runs the daemon until a stop is requested.
pub(crate) async fn run(running_daemon: RunningDaemon) -> io::Result<()> {
    let runtime_directory = runtime_directory::locate()?;
    runtime_directory::prepare(&runtime_directory)?;
    // The lock comes first. Only its owner can remove the socket file and
    // take the leases of the holders.
    let lock = InstanceLock::acquire(&runtime_directory, running_daemon)?;
    let socket_path = runtime_directory.join(SOCKET_FILE);
    remove_socket_of_earlier_daemon(&socket_path)?;
    let listener = LocalListener::bind(&socket_path)?;

    let shutdown = Shutdown::new();
    shutdown.trigger_on_signals()?;
    let sessions = Sessions::new(runtime_directory, shutdown.clone());
    // A client that connects now waits in the queue of the socket. It gets
    // its answer after the daemon has the sessions of the earlier daemon.
    sessions.adopt_holders().await?;
    info!(socket = %socket_path.display(), "the daemon listens");

    let context = Arc::new(Context {
        sessions,
        shutdown,
        in_flight_requests: InFlightRequests::default(),
    });
    listener::serve(listener, Arc::clone(&context)).await;

    // The connections close when this function returns. Before that, each
    // request gets its response, and the socket and the lock go away. Thus
    // a client that sees the end of its connection can start a new daemon.
    context.in_flight_requests.all_ended().await;
    fs::remove_file(&socket_path)?;
    drop(lock);
    info!("the daemon stops; the holders continue");
    Ok(())
}

/// A daemon that was killed leaves its socket file. `bind` fails while the
/// file exists.
fn remove_socket_of_earlier_daemon(socket_path: &Path) -> io::Result<()> {
    match fs::remove_file(socket_path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
