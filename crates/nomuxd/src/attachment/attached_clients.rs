//! The clients that are attached to one session at this moment.

use tokio::sync::watch;

/// Counts the attached clients of one session and tells them to detach.
pub(crate) struct AttachedClients {
    /// The number of detach requests. Each attached client holds one
    /// receiver, thus the number of receivers is the number of clients.
    detach_requests: watch::Sender<u64>,
}

impl Default for AttachedClients {
    fn default() -> Self {
        Self {
            detach_requests: watch::Sender::new(0),
        }
    }
}

impl AttachedClients {
    /// Adds one client. The client is attached until it drops the result.
    pub(crate) fn join(&self) -> DetachRequest {
        DetachRequest(self.detach_requests.subscribe())
    }

    /// Tells each attached client to detach. Returns their number.
    pub(crate) fn detach_all(&self) -> usize {
        let clients = self.detach_requests.receiver_count();
        self.detach_requests.send_modify(|requests| *requests += 1);
        clients
    }
}

/// The membership of one attached client.
pub(crate) struct DetachRequest(watch::Receiver<u64>);

impl DetachRequest {
    /// Completes when a request detaches the clients of the session. It
    /// does not complete if the session leaves the registry first: the end
    /// of the session then ends the attach.
    pub(crate) async fn received(&mut self) {
        if self.0.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}
