//! Lets the daemon stop only between two requests.

use tokio::sync::{RwLock, RwLockReadGuard};

/// The requests that have no response yet.
///
/// A stop of the daemon closes each connection. Without this type, a client
/// can lose the response to a request that the daemon already did, for
/// example the response to the request that stops the daemon.
#[derive(Debug, Default)]
pub(crate) struct InFlightRequests {
    /// A request holds the lock for reading. The stop sequence takes it for
    /// writing, which is possible only when no request holds it.
    lock: RwLock<()>,
}

impl InFlightRequests {
    /// Starts one request. The request ends when the result is dropped.
    pub(crate) async fn begin(&self) -> RwLockReadGuard<'_, ()> {
        self.lock.read().await
    }

    /// Completes when each request that began has ended.
    pub(crate) async fn all_ended(&self) {
        drop(self.lock.write().await);
    }
}
