//! Accepts the connections of the clients.

use std::sync::Arc;
use std::time::Duration;

use nomux_transport::LocalListener;
use tokio::task::spawn_local;
use tokio::time::sleep;
use tracing::warn;

use crate::client_connection;
use crate::handlers::Context;

/// After a failed `accept`, the listener waits this time. An error such as
/// "too many open files" does not go away immediately.
const ACCEPT_ERROR_PAUSE: Duration = Duration::from_millis(100);

/// Accepts connections until a stop is requested. One task serves each
/// connection.
pub(crate) async fn serve(listener: LocalListener, context: Arc<Context>) {
    loop {
        tokio::select! {
            () = context.shutdown.requested() => return,
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    // A process of a sandboxed session uses the agent
                    // socket: on the main socket it could control each
                    // session.
                    if context.sessions.refuses_peer(peer) {
                        warn!(?peer, "a process of a sandboxed session was refused on the main socket");
                        spawn_local(client_connection::refuse_sandboxed(stream));
                        continue;
                    }
                    // An attach holds a reader of a ring buffer, which stays
                    // on the thread that mapped the memory.
                    spawn_local(client_connection::serve(stream, Arc::clone(&context)));
                }
                Err(error) => {
                    warn!(%error, "a connection was not accepted");
                    sleep(ACCEPT_ERROR_PAUSE).await;
                }
            },
        }
    }
}
