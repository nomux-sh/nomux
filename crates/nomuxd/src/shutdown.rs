//! The signal that tells the daemon to stop.

use std::io;

use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;

/// A handle to the stop signal of the daemon. Each copy can trigger the
/// stop and can wait for it.
#[derive(Debug, Clone)]
pub(crate) struct Shutdown {
    requested: watch::Sender<bool>,
}

impl Shutdown {
    pub(crate) fn new() -> Self {
        Self {
            requested: watch::Sender::new(false),
        }
    }

    /// Tells the daemon to stop.
    pub(crate) fn trigger(&self) {
        self.requested.send_replace(true);
    }

    /// Completes when a stop was requested.
    pub(crate) async fn requested(&self) {
        let mut requested = self.requested.subscribe();
        // The sender is in `self`, thus the channel cannot close here.
        let _ = requested.wait_for(|requested| *requested).await;
    }

    /// Triggers the stop when the process gets `SIGTERM` or `SIGINT`.
    pub(crate) fn trigger_on_signals(&self) -> io::Result<()> {
        let mut terminate = signal(SignalKind::terminate())?;
        let mut interrupt = signal(SignalKind::interrupt())?;
        let shutdown = self.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = terminate.recv() => {}
                _ = interrupt.recv() => {}
            }
            shutdown.trigger();
        });
        Ok(())
    }
}
